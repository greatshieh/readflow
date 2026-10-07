/**
 * 文章状态管理
 *
 * # 职责
 * 管理文章列表与"当前阅读文章"的全部前端状态：列表分页加载、详情读取、
 * 已读 / 收藏标记、视图筛选（全部 / 未读 / 书签）、标签范围筛选、关键字搜索，以及
 * AI 摘要与 AI 翻译的调用封装。对外暴露 `useArticlesStore()`，
 * 主要被 `components/ArticleColumn.vue`（文章列表）与 `components/ContentColumn.vue`（正文）消费。
 *
 * # 设计意图（核心架构规则：前端只做展示 + 必要数据处理）
 * - **所有数据读写都在 Rust 后端**：本 store 不做任何 RSS 抓取、AI HTTP 调用或
 *   数据库操作，只通过 `invoke('command_name', { 参数 })` 请求后端；
 *   AI 请求由 Rust 侧发起（见 `ai.rs`），前端只负责传 `providerId` 等入参并渲染结果。
 * - **乐观的本地状态同步**：后端返回后立刻把结果写回 `articles` / `selectedArticle`，
 *   避免整列表重新拉取，保证交互即时反馈。
 * - **前端仅做已加载列表的筛选 / 搜索**：`viewFilter` 与 `search` 作用在
 *   已经从后端取回的 `articles` 上（按已读状态、收藏状态、标题关键字过滤），
 *   不发起新的网络请求——这是规则允许的"必要数据处理"。
 *   例外是**标签范围**（`tagFilterId`）：标签下的文章横跨所有订阅源，前端过滤必漏，
 *   故它作为范围参数下推给后端，与 `selectedFeed` 互斥。
 * - **错误不冒泡到 UI 崩溃**：加载类操作捕获异常并写入 `error`，AI 类操作
 *   （摘要 / 翻译）则重新抛出，由界面决定是否弹出提示——因为 AI 结果需要显式反馈。
 *
 * # 运行环境
 * 本应用为 Tauri 桌面应用，前端不持有业务数据副本（无浏览器预览态的 mock 回退）。
 *
 * # Tauri invoke 参数约定（改动前必读）
 * 前端传参一律使用 **camelCase**（`feedId`、`articleId`、`providerId`、`targetLanguage`），
 * Rust 端以 snake_case 形参接收，Tauri 自动完成两者转换。切勿改成 snake_case，否则后端收不到值。
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { Article, AiStreamPayload, Highlight } from '@/types'
// 批量标记已读后需要同步侧边栏未读角标，因此依赖 feeds store 的 loadFeeds；
// Pinia 的 store 之间可以互相 useStore()，在函数体内调用可避免模块初始化阶段的循环依赖。
import { useFeedsStore } from './feeds'
// 请求标识的实现与文章追问共用（见 utils/requestId.ts），避免两条 AI 链路各写一份
import { newRequestId } from '@/utils/requestId'

/** 视图筛选类型：全部 / 未读 / 书签 */
export type ViewFilter = 'all' | 'unread' | 'bookmarked' | 'liked' | 'skipped'

/**
 * 列表排序方式
 *
 * 取值与后端 `db::ArticleSort` 的解析规则一一对应（`latest` / `score` / `title`）；
 * 排序在后端完成——只有后端能在**全库**范围内排序，前端只能在已加载的那几页里排，
 * 会出现"第 51 条重要文章永远排在最后"的错觉。
 */
export type ArticleSortKey = 'latest' | 'score' | 'title'

/** 列表每页条数（首屏、加载更多、翻页都用同一个档位，保证 hasMore 判定一致） */
export const ARTICLE_PAGE_SIZE = 50

/**
 * 流式增量累积文本
 *
 * 刻意放在模块作用域而非 store 的 setup 内：`ai-stream` 监听器只注册一次，
 * 若闭包捕获的是某个 store 实例的 ref，store 被重建（HMR、多实例）之后
 * 增量就会写进已经没人渲染的旧 ref，界面再也不动。
 */
const streamingText = ref('')
/** 当前流式产物的类型（`null` 表示没有流在进行） */
const streamingKind = ref<'summary' | 'translate' | null>(null)
/** 正在生成的文章 ID：用户切换文章后不该把增量画到另一篇上 */
const streamingArticleId = ref<number | null>(null)
/** 当前在途请求的标识；非响应式，只用于过滤迟到事件 */
let currentRequestId: string | null = null
/** 监听是否已注册（模块级，避免 store 重建时重复订阅） */
let aiStreamListenerRegistered = false

/** 结束一次流式渲染：清空累积并断开事件归属，此后迟到事件一律被丢弃 */
function clearStream() {
  streamingText.value = ''
  streamingKind.value = null
  streamingArticleId.value = null
  currentRequestId = null
}

// 订阅后端逐批推出的 AI 增量。监听失败（非 Tauri 环境等）只记日志：
// 拿不到增量最多是少了逐字效果，`invoke` 的返回值里仍有完整文本。
if (!aiStreamListenerRegistered) {
  aiStreamListenerRegistered = true
  import('@tauri-apps/api/event')
    .then(({ listen }) => {
      listen<AiStreamPayload>('ai-stream', (event) => {
        const p = event.payload
        // 只收当前这次请求的增量：连点两次「重新生成」时，上一轮的迟到事件
        // 必须丢弃，否则新旧文本会交替闪烁
        if (p.requestId !== currentRequestId) return
        streamingText.value += p.delta
      })
    })
    .catch((e) => console.warn('订阅 AI 流式事件失败:', e))
}

export const useArticlesStore = defineStore('articles', () => {
  /** 当前显示的文章列表（按加载顺序累加，分页时向后追加） */
  const articles = ref<Article[]>([])
  /** 当前选中的文章（详情面板渲染源，未选中时为 null） */
  const selectedArticle = ref<Article | null>(null)
  /** 加载状态（列表请求进行中，用于界面展示骨架屏/loading） */
  const loading = ref(false)
  /** 是否还有更多文章（分页"加载更多"按钮的依据） */
  const hasMore = ref(true)
  /** AI 生成状态（摘要/翻译进行中，用于禁用按钮与显示进度） */
  const generating = ref(false)
  /** 当前生成模式：'summary' 或 'translate'，用于 UI 显示对应提示 */
  const generatingMode = ref<'summary' | 'translate' | null>(null)
  /** AI 调用错误（最近一次摘要/翻译失败的提示文案，成功时置为 null） */
  const aiError = ref<string | null>(null)
  /** 错误信息（最近一次操作的失败原因，成功时置为 null） */
  const error = ref<string | null>(null)

  /**
   * 视图筛选条件（全部 / 未读 / 书签）
   *
   * 由文章列表栏（ArticleColumn）的筛选按钮写入，作用于已加载的 `articles` 列表，不触发新请求。
   */
  const viewFilter = ref<ViewFilter>('all')
  /**
   * 文章关键字搜索（标题模糊匹配）
   *
   * 同样由文章列表栏（ArticleColumn）的搜索框写入，在已加载列表上做客户端过滤，避免每次击键都请求后端。
   */
  const search = ref('')
  /**
   * 列表排序方式（最新 / 重要 / 名称）
   *
   * 由文章列表栏的排序按钮写入；改动后必须重新从第 0 页加载（排序是后端行为，
   * 只重排已加载的几页会得到"看着像排了、其实只排了一半"的假象）。
   */
  const sortMode = ref<ArticleSortKey>('latest')

  /**
   * 当前生效的标签范围（null 表示不按标签筛选）
   *
   * 标签是**范围**而非叠加筛选：它由侧边栏的标签分区写入，与 `feeds.selectedFeed`
   * 互斥（选中标签即取消选源，反之亦然），由 `ArticleColumn` 的「范围键」统一驱动重载。
   * 之所以必须在后端过滤，是因为标签下的文章可能横跨所有订阅源，
   * 前端只能在已加载的那几页里过滤，会漏掉范围之外的命中项。
   */
  const tagFilterId = ref<number | null>(null)

  /**
   * 设置标签范围
   *
   * @param id - 标签 ID；null 表示退出标签视图
   * @returns 无返回值；副作用为更新 `tagFilterId`，由列表栏监听后重新加载
   */
  function setTagFilter(id: number | null) {
    tagFilterId.value = id
  }

  /**
   * 经视图筛选后的展示列表
   *
   * 在 `articles` 之上做一层前端过滤，按 `viewFilter` 过滤已读 / 收藏状态。
   *
   * 注意**书签与未读两个视图后端都已过滤过一遍**（`loadArticles` 分别传
   * `bookmarked: true` / `unread: true`），这里的两层过滤于是成了等价的兜底：
   * 它让切换视图的瞬间就能按旧列表给出即时反馈，不必等重新拉取返回；
   * 更关键的是**读完一篇 / 取消收藏后能立刻把它移出列表**——单篇状态变化并不
   * 触发列表重新拉取，没有这层兜底它就会滞留在列表里。保留它不会造成双重过滤错误。
   *
   * 标题 / 正文 / 摘要的**全文搜索**走后端 `articles_search`（见 [`searchArticles`]），
   * 结果存于 `searchResults`，不再在客户端做标题子串过滤，
   * 否则会与后端的全文检索重复过滤、且搜不到正文。
   *
   * @returns 最终用于渲染的文章数组（响应式，随 `articles` / `viewFilter` 变化）
   */
  const displayArticles = computed<Article[]>(() => {
    let list = articles.value

    if (viewFilter.value === 'unread') {
      list = list.filter((a) => !a.is_read)
    } else if (viewFilter.value === 'bookmarked') {
      list = list.filter((a) => a.is_bookmarked)
    } else if (viewFilter.value === 'liked') {
      // 偏好过滤同书签/未读：后端已过滤，这里是切视图即时反馈与
      // 单篇标记变化的即时移出兜底（见上方注释）
      list = list.filter((a) => a.user_preference === 'like')
    } else if (viewFilter.value === 'skipped') {
      list = list.filter((a) => a.user_preference === 'skip')
    }

    return list
  })

  /** 全文搜索结果（后端 `articles_search` 返回，覆盖标题 / 正文 / 摘要） */
  const searchResults = ref<Article[]>([])
  /** 全文搜索进行中标志 */
  const searching = ref(false)

  /**
   * 设置视图筛选条件
   *
   * @param v - 'all' | 'unread' | 'bookmarked' | 'liked' | 'skipped'
   * @returns 无返回值；副作用为更新 `viewFilter` 并联动 `displayArticles`
   */
  function setViewFilter(v: ViewFilter) {
    viewFilter.value = v
  }

  /**
   * 设置文章搜索关键字
   *
   * @param q - 搜索串
   * @returns 无返回值；副作用为更新 `search` 并联动 `displayArticles`
   */
  function setSearch(q: string) {
    search.value = q
  }

  /**
   * 加载文章列表（支持分页追加；feedId 为 null 时加载全部订阅源）
   *
   * 以 `offset === 0` 区分"首次加载 / 刷新"与"加载更多"：首页直接替换列表，
   * 后续页追加到末尾，这样滚动加载不会打断用户当前位置。
   * 参数 `feedId` 用 camelCase 传给后端（Rust 侧为 `feed_id`）；
   * 传 null 时后端省略 WHERE 子句，返回全库文章的统一时间线（仿 Folo 的 Articles 视图）。
   * 排序方式取当前 `sortMode`，由后端在**全库范围**内排序。
   *
   * @param feedId - 订阅源 ID；null 表示全部订阅源（对应 Rust 端 `Option<i64>`）
   * @param limit - 每页数量，默认 {@link ARTICLE_PAGE_SIZE}
   * @param offset - 偏移量，默认 0；传 0 表示重置列表
   * @returns 无返回值；结果写入 `articles`，并更新 `hasMore`
   * @throws 不向外抛出：异常被捕获后写入 `error` 并打印日志，`loading` 在 finally 中复位
   */
  async function loadArticles(
    feedId: number | null,
    limit: number = ARTICLE_PAGE_SIZE,
    offset: number = 0
  ) {
    loading.value = true
    error.value = null
    try {
      // 动态 import：Tauri 运行时模块按需加载
      const { invoke } = await import('@tauri-apps/api/core')
      const data = await invoke<Array<Article>>('articles_list', {
        feedId,
        // 标签范围由后端过滤（标签下文章可横跨所有源，前端过滤必漏）
        tagId: tagFilterId.value,
        // 书签范围同样必须由后端过滤：前端每页只有 50 条，收藏若散落在上千条历史里，
        // 靠"加载更多"逐步露出既慢、又会让 hasMore 失真。
        // 注意这里刻意传 false 而非 undefined——后端把 null/false 一视同仁为"不限"，
        // 显式布尔值能让 Tauri 的参数绑定稳定落在 Option<bool> 上
        bookmarked: viewFilter.value === 'bookmarked',
        // 未读范围同理必须走后端：未读文章同样会散落在上千条历史里，
        // 只在前端筛已加载页会让「仅未读」列表残缺、hasMore 失真
        unread: viewFilter.value === 'unread',
        sort: sortMode.value,
        limit,
        offset,
        // 用户偏好过滤（后端对称语义）：'like' = 只显示喜欢的，'skip' = 只显示跳过的
        // （便于检查与纠正错标文章；与书签/未读同理必须后端过滤，散落历史时前端必漏）
        userPreference: viewFilter.value === 'liked' ? 'like' : viewFilter.value === 'skipped' ? 'skip' : null
      })
      if (offset === 0) {
        articles.value = data
      } else {
        articles.value.push(...data)
      }
      // 用"本页是否取满"判断是否还有下一页，避免额外发一次 count 请求
      hasMore.value = data.length === limit
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      console.error('加载文章失败:', e)
    } finally {
      loading.value = false
    }
  }

  /**
   * 加载下一页（列表底部"加载更多"按钮）
   *
   * 以"当前已加载条数"作为 offset，与 `loadArticles` 的追加语义配合即可翻页；
   * 正在加载或已无更多时直接返回，防止用户连点造成重复请求。
   *
   * @param feedId - 当前选中的订阅源 ID；null 表示全部订阅源
   * @returns 无返回值；新一页追加到 `articles` 末尾
   */
  async function loadMoreArticles(feedId: number | null) {
    if (loading.value || !hasMore.value) return
    await loadArticles(feedId, ARTICLE_PAGE_SIZE, articles.value.length)
  }

  /**
   * 切换列表排序方式
   *
   * 只写状态不发起请求：调用方（列表栏）需要在切排序时按当前订阅源重新加载，
   * 只有它知道"当前选中的是哪个源 / 是否处于搜索态"。
   *
   * @param mode - 目标排序方式
   * @returns 无返回值；副作用为更新 `sortMode`
   */
  function setSortMode(mode: ArticleSortKey) {
    sortMode.value = mode
  }

  /**
   * 加载单篇文章详情
   *
   * @param id - 文章 ID（对应 Rust 端同名 `id`，单名词无需 camelCase 转换）
   * @returns 文章对象；找不到或调用失败时返回 null
   * @throws 不向外抛出：异常被捕获后打印日志并返回 null
   */
  async function loadArticle(id: number): Promise<Article | null> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      return await invoke<Article | null>('article_get', { id })
    } catch (e) {
      console.error('加载文章失败:', e)
      return null
    }
  }

  /**
   * 选中文章并顺带标记为已读（阅读动作的组合入口）
   *
   * 把"选中"和"已读"合并成一个动作，是因为业务上打开文章即视为已读，
   * 分开调用容易漏掉已读标记，导致未读数与后端不一致。
   *
   * @param id - 文章 ID
   * @returns 无返回值；成功时写入 `selectedArticle`，同时落库已读状态
   * @throws 不向外抛出：`markAsRead` 内部已吞掉异常，只记录日志
   */
  async function selectArticle(id: number) {
    const article = await loadArticle(id)
    if (article) {
      selectedArticle.value = article
      // 已读状态必须持久化到后端，否则刷新后未读数会回退
      await markAsRead(id)
    }
  }

  /**
   * 标记文章为已读（持久化到后端）
   *
   * @param id - 文章 ID
   * @returns 无返回值；副作用为后端落库，并同步 `selectedArticle.is_read`
   * @throws 不向外抛出：失败仅打印日志（已读失败不应阻塞阅读）
   */
  async function markAsRead(id: number) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('article_read', { id })
      // 后端返回后同步内存，避免界面还要再发一次详情请求
      if (selectedArticle.value?.id === id) {
        selectedArticle.value.is_read = true
      }
    } catch (e) {
      console.error('标记已读失败:', e)
    }
  }

  /**
   * 切换文章收藏状态
   *
   * 由后端返回最终的布尔值（而非前端自行取反），是为了以数据库结果为准，
   * 避免并发操作下前端状态与持久层不一致。
   *
   * @param id - 文章 ID
   * @returns 收藏后的状态：true 表示已收藏；失败时返回 false
   * @throws 不向外抛出：异常被捕获后打印日志并返回 false
   */
  async function toggleBookmark(id: number): Promise<boolean> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const isBookmarked = await invoke<boolean>('article_bookmark', { id })
      if (selectedArticle.value?.id === id) {
        selectedArticle.value.is_bookmarked = isBookmarked
      }
      return isBookmarked
    } catch (e) {
      console.error('切换收藏状态失败:', e)
      return false
    }
  }

  /**
   * 设置文章的用户偏好标记（喜欢/跳过/取消）
   *
   * 用户点击文章列表中的 👍/👎 按钮时触发。
   * 乐观更新本地状态，避免闪烁；失败时静默忽略（不影响列表展示）。
   *
   * @param id - 文章 ID
   * @param preference - 'like' | 'skip' | null（null 表示取消标记）
   */
  async function setPreference(id: number, preference: 'like' | 'skip' | null): Promise<void> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const prefValue = preference ?? ''
      await invoke<void>('article_preference_set', { id, preference: prefValue })
      // 乐观更新选中的文章
      if (selectedArticle.value?.id === id) {
        selectedArticle.value.user_preference = preference
      }
      // 同步更新列表中对应文章（若已加载）
      const list = articles.value
      const idx = list.findIndex(a => a.id === id)
      if (idx !== -1) list[idx].user_preference = preference
    } catch (e) {
      console.error('设置偏好失败:', e)
    }
  }

  /**
   * 精确设置文章的已读 / 未读状态
   *
   * 由后端 `article_set_read` 持久化，并同步所属源的未读数（见 Rust 端 `Article::set_read`）。
   * 用于正文工具条的"标记为已读/未读"双向切换；乐观地把结果写回内存，界面无需重载即可反映。
   *
   * @param id - 文章 ID
   * @param isRead - true=已读，false=未读
   * @returns 无返回值；失败仅打印日志（状态切换不应阻塞阅读）
   * @throws 不向外抛出：异常被捕获后打印日志
   */
  async function setRead(id: number, isRead: boolean): Promise<void> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('article_set_read', { id, isRead })
      // 后端已落库并同步未读数，这里同步内存让侧边栏与正文状态即时更新
      if (selectedArticle.value?.id === id) {
        selectedArticle.value.is_read = isRead
      }
      const idx = articles.value.findIndex(a => a.id === id)
      if (idx !== -1) {
        articles.value[idx].is_read = isRead
      }
    } catch (e) {
      console.error('设置已读状态失败:', e)
    }
  }

  /**
   * 记录文章的阅读进度（正文滚动比例）
   *
   * 由正文区滚动时节流调用，**频率约为每秒一次**。因此这里刻意不打印日志：
   * 用 `console.error` 会把控制台淹没，而进度既不影响阅读也不影响数据正确性，
   * 失败静默丢弃即可（与 `setRead` 故意不同——那个是用户显式操作，必须可诊断）。
   *
   * 会同步写回内存中的 `read_progress`：否则本次会话内"滚到 60% → 切走 → 切回"
   * 会恢复到切走前的旧位置（内存值仍是加载时的快照），与写库结果不一致。
   *
   * @param id - 文章 ID
   * @param progress - 滚动比例，0.0 = 顶部，1.0 = 底部；越界与非有限值由后端 clamp
   * @returns 无返回值；失败静默（不抛出、不打印）
   */
  async function setProgress(id: number, progress: number): Promise<void> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('article_set_progress', { id, progress })
      // 写回内存：正文区不展示该字段，赋值不会触发列表重渲染
      if (selectedArticle.value?.id === id) {
        selectedArticle.value.read_progress = progress
      }
      const idx = articles.value.findIndex(a => a.id === id)
      if (idx !== -1) {
        articles.value[idx].read_progress = progress
      }
    } catch {
      // 静默：进度写库失败不影响阅读，且调用点在高频 scroll 上
    }
  }

  /**
   * 批量标记当前范围为已读（文章列表顶部"全部已读"按钮）
   *
   * 委托后端 `articles_mark_all_read`（按 feedId / 标签范围批量置已读并统一重算未读数）。
   * 前端做乐观更新：把当前列表里属于该范围的未读文章本地置为已读，避免整列表重载打断阅读位置。
   *
   * @param feedId - 订阅源 ID；null 表示不按源限定
   * @returns 无返回值
   * @throws 后端调用失败时向上抛出，由调用方决定如何提示用户（不在此静默吞掉，
   *         否则按钮点击后既无反馈也无数据变化，用户会误判为"功能没实现"）
   */
  async function markAllRead(feedId: number | null): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core')
    // 标签范围必须一并传下去：标签视图下若只传 feedId=null，会越界清空全库未读
    const tagId = tagFilterId.value
    // 后端批量置已读并统一重算未读数；失败时异常向上抛出，交由 UI 层提示
    await invoke<number>('articles_mark_all_read', { feedId, tagId })

    // 乐观更新：只清当前范围内（源 + 标签）的未读标记
    for (const a of articles.value) {
      const inFeed = feedId === null || a.feed_id === feedId
      const inTag = tagId === null || a.tags?.some((t) => t.id === tagId)
      if (inFeed && inTag) {
        a.is_read = true
      }
    }
    const sel = selectedArticle.value
    if (sel) {
      const inFeed = feedId === null || sel.feed_id === feedId
      const inTag = tagId === null || sel.tags?.some((t) => t.id === tagId)
      if (inFeed && inTag) {
        sel.is_read = true
      }
    }
    // 未读数已变化，同步重载订阅源列表，保证侧边栏角标与后端一致
    await useFeedsStore().loadFeeds()
  }

  /**
   * 生成 AI 摘要
   *
   * AI 的实际 HTTP 调用发生在 Rust 后端（前端不持有 API Key、不发起外网请求），
   * 这里只传 `articleId` 与 `providerId`，拿到结果后回填到选中项与列表项中，
   * 让"已生成"状态无需重新拉取列表即可呈现。
   *
   * @param articleId - 文章 ID（对应 Rust 端 `article_id`）
   * @param providerId - AI 提供商 ID（对应 Rust 端 `provider_id`），默认 'agnes'
   * @returns AI 生成的摘要文本
   * @throws 后端失败（网络 / 额度 / 配置错误）时原样抛出，由界面提示用户
   */
  async function generateSummary(articleId: number, providerId: string = 'agnes'): Promise<string> {
    generating.value = true
    generatingMode.value = 'summary'
    aiError.value = null
    // 先换标识再清旧文本：顺序颠倒的话，上一轮的迟到事件会落进刚清空的累积里，
    // 界面会闪出半截旧摘要。此后只有带新标识的事件才被接受。
    currentRequestId = newRequestId()
    streamingArticleId.value = articleId
    streamingKind.value = 'summary'
    streamingText.value = ''
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const summary = await invoke<string>('ai_generate_summary', {
        articleId,
        providerId,
        requestId: currentRequestId
      })
      // 更新选中文章
      if (selectedArticle.value?.id === articleId) {
        selectedArticle.value.ai_summary = summary
        selectedArticle.value.has_ai_summary = true
      }
      // 更新列表中的文章
      const idx = articles.value.findIndex(a => a.id === articleId)
      if (idx !== -1) {
        articles.value[idx].ai_summary = summary
        articles.value[idx].has_ai_summary = true
      }
      return summary
    } catch (e) {
      // 把错误写入 aiError 供界面内联展示（而非弹窗），同时保留抛出供调用方可选处理
      const msg = e instanceof Error ? e.message : String(e)
      aiError.value = msg
      console.error('生成摘要失败:', e)
      throw e
    } finally {
      generating.value = false
      // 统一在收尾丢弃流式文本：成功时界面已改用 `ai_summary` 渲染（同一份内容），
      // 失败时后端不写库，界面也不该留着一截"看着像生成了"的文本
      clearStream()
    }
  }

  /**
   * 生成 AI 翻译
   *
   * 与 `generateSummary` 同构：由 Rust 后端完成翻译请求，前端仅传参与回填结果。
   *
   * @param articleId - 文章 ID（对应 Rust 端 `article_id`）
   * @param providerId - AI 提供商 ID（对应 Rust 端 `provider_id`），默认 'agnes'
   * @param targetLanguage - 目标语言（对应 Rust 端 `target_language`），默认 '中文'
   * @returns 翻译后的文本
   * @throws 后端失败时原样抛出
   */
  async function generateTranslation(
    articleId: number,
    providerId: string = 'agnes',
    targetLanguage: string = '中文'
  ): Promise<string> {
    generating.value = true
    generatingMode.value = 'translate'
    aiError.value = null
    // 同 generateSummary：先换标识再清空，避免上一轮的迟到增量混进来
    currentRequestId = newRequestId()
    streamingArticleId.value = articleId
    streamingKind.value = 'translate'
    streamingText.value = ''
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const translation = await invoke<string>('ai_translate_article', {
        articleId,
        providerId,
        targetLanguage,
        requestId: currentRequestId
      })
      // 更新选中文章
      if (selectedArticle.value?.id === articleId) {
        selectedArticle.value.ai_translation = translation
        selectedArticle.value.has_ai_translation = true
      }
      // 更新列表中的文章
      const idx = articles.value.findIndex(a => a.id === articleId)
      if (idx !== -1) {
        articles.value[idx].ai_translation = translation
        articles.value[idx].has_ai_translation = true
      }
      return translation
    } catch (e) {
      // 同 generateSummary：错误写入 aiError 供界面内联展示，并保留抛出
      const msg = e instanceof Error ? e.message : String(e)
      aiError.value = msg
      console.error('生成翻译失败:', e)
      throw e
    } finally {
      generating.value = false
      clearStream()
    }
  }

  /**
   * 全文搜索文章（后端 FTS5，覆盖标题 / 正文 / 摘要，含中文）
   *
   * 与旧版"仅客户端标题过滤"不同，这里把查询交给后端 `articles_search`，
   * 可对正文做子串匹配；结果写入 `searchResults` 供文章列表栏渲染。
   *
   * @param query - 搜索词（空串时清空结果）
   * @returns 无返回值；结果写入 `searchResults`，`searching` 指示状态
   */
  async function searchArticles(query: string): Promise<void> {
    searching.value = true
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      // limit 用 camelCase 传参，Rust 端为 limit；后端用 FTS5 trigram 做子串匹配
      searchResults.value = await invoke<Array<Article>>('articles_search', {
        query,
        limit: 200
      })
    } catch (e) {
      console.error('全文搜索失败:', e)
      searchResults.value = []
    } finally {
      searching.value = false
    }
  }

  /**
   * 当前文章的高亮片段列表（响应式，随 `loadHighlights` / 增删操作变化）
   */
  const highlights = ref<Highlight[]>([])

  /**
   * 加载某文章的全部高亮
   *
   * @param articleId - 文章 ID
   * @returns 无返回值；结果写入 `highlights`
   */
  async function loadHighlights(articleId: number): Promise<void> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      highlights.value = await invoke<Array<Highlight>>('highlights_list', { articleId })
    } catch (e) {
      console.error('加载高亮失败:', e)
      highlights.value = []
    }
  }

  /**
   * 新建高亮片段（乐观写入内存）
   *
   * @param articleId - 文章 ID
   * @param text - 高亮原文
   * @param note - 批注（可选）
   * @param color - 高亮颜色（默认 yellow）
   * @param startOffset - 文本起点偏移
   * @param endOffset - 文本终点偏移
   * @returns 无返回值；成功时把高亮追加进 `highlights`
   */
  async function createHighlight(
    articleId: number,
    text: string,
    note: string = '',
    color: string = 'yellow',
    startOffset: number = 0,
    endOffset: number = 0
  ): Promise<void> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const id = await invoke<number>('highlights_create', {
        articleId,
        text,
        note,
        color,
        startOffset,
        endOffset
      })
      // 乐观追加，避免重新拉取；created_at 用本地时间近似（仅展示用）
      highlights.value.push({
        id,
        article_id: articleId,
        text,
        note,
        color,
        start_offset: startOffset,
        end_offset: endOffset,
        created_at: new Date().toISOString()
      })
    } catch (e) {
      console.error('创建高亮失败:', e)
    }
  }

  /**
   * 删除高亮片段
   *
   * @param id - 高亮 ID
   * @returns 无返回值；成功时从 `highlights` 中移除
   */
  async function deleteHighlight(id: number): Promise<void> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('highlights_delete', { id })
      highlights.value = highlights.value.filter((h) => h.id !== id)
    } catch (e) {
      console.error('删除高亮失败:', e)
    }
  }

  /**
   * 生成 AI 每日简报
   * @param days - 回溯天数（默认 1）
   * @returns 生成的简报文本
   */
  async function aiDailyBrief(days: number = 1): Promise<string> {
    const { invoke } = await import('@tauri-apps/api/core')
    return await invoke<string>('ai_daily_brief', { days })
  }

  /**
   * 获取每日阅读统计
   * @param days - 天数（默认 7）
   * @returns 每日已读数数组，每项为 [日期字符串, 数量]
   */
  async function statsDailyRead(days: number = 7): Promise<Array<[string, number]>> {
    const { invoke } = await import('@tauri-apps/api/core')
    return await invoke<Array<[string, number]>>('stats_daily_read', { days })
  }

  return {
    articles,
    selectedArticle,
    loading,
    hasMore,
    generating,
    generatingMode,
    aiError,
    streamingText,
    streamingKind,
    streamingArticleId,
    error,
    viewFilter,
    search,
    sortMode,
    tagFilterId,
    displayArticles,
    setViewFilter,
    setSearch,
    setSortMode,
    setTagFilter,
    loadArticles,
    loadMoreArticles,
    selectArticle,
    setRead,
    setProgress,
    markAllRead,
    toggleBookmark,
    setPreference,
    generateSummary,
    generateTranslation,
    searchResults,
    searching,
    searchArticles,
    highlights,
    loadHighlights,
    createHighlight,
    deleteHighlight,
    aiDailyBrief,
    statsDailyRead
  }
})
