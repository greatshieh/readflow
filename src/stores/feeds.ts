/**
 * 订阅源状态管理
 *
 * # 职责
 * 管理订阅源（Feed）列表与"当前选中订阅源"：加载列表、手动刷新、新增、更新（重命名/归类）、删除、选中，
 * 以及 OPML 的导入 / 导出。对外暴露 `useFeedsStore()`，主要被 `App.vue`、
 * `components/FeedPanel.vue`、`components/ArticleColumn.vue`、`components/ContentColumn.vue` 消费。
 *
 * # 设计意图（核心架构规则：前端只做展示 + 必要数据处理）
 * - **RSS 抓取在 Rust 后端**：刷新订阅源时前端只发一个 `feeds_refresh` 命令，
 *   真正的 HTTP 抓取、解析、入库都在后端完成，前端不接触网络与数据库。
 * - **写操作后整体重载**：新增 / 删除 / 刷新成功后统一调用 `loadFeeds()` 重新拉取，
 *   而不是在本地数组里手工增删。订阅源数量很小，全量重载的代价远低于
 *   维护本地增量同步逻辑，且能与后端的 `unread_count` 重算结果保持一致。
 *   （例外：刷新过程中后端会逐源广播 `refresh-progress` 事件，未读数与进度
 *   由事件监听器就地增量更新，让用户不必等整轮刷新结束才看到变化。）
 * - **失败可感知**：写操作（add / delete）重新抛出异常交由界面提示，
 *   避免用户误以为操作成功；读操作（loadFeeds）则收敛为 `error` 状态。
 *
 * # 运行环境
 * 本应用为 Tauri 桌面应用，所有数据读写都经由 `invoke()` 交给 Rust 后端，
 * 前端不持有业务数据副本（无浏览器预览态的 mock 回退）。
 *
 * # Tauri invoke 参数约定（改动前必读）
 * 前端传参一律使用 **camelCase**，Rust 端以 snake_case 接收，Tauri 自动转换。
 * 本文件中 `name` / `url` / `id` 均为单词名无需转换；
 * 若后续新增多词参数（如 `feedId`），切勿写成 snake_case。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { Feed, Folder, FeedAiConfig } from '@/types'

/** 刷新进度事件载荷（与 Rust 端 `RefreshProgress` 对应，Tauri 事件键为 camelCase） */
interface RefreshProgressPayload {
  /** 刚完成刷新的订阅源 ID */
  feedId: number
  /** 订阅源名称 */
  feedName: string
  /** 本次刷新该源新增的文章数 */
  newCount: number
  /** 重算后该源的未读文章数 */
  unreadCount: number
  /** 已完成的源序号（从 1 起） */
  done: number
  /** 本轮待刷新的源总数 */
  total: number
}

/**
 * 订阅源后台补全完成事件载荷（与 Rust 端 `FeedUpdated` 对应）
 *
 * 服务两个场景：新增订阅的首次抓取、修改链接后的重抓。两者都先落库再异步回填。
 */
interface FeedUpdatedPayload {
  /** 订阅源 ID */
  feedId: number
  /** 回填后的最终名称（抓不到标题时为域名兜底名） */
  name: string
  /** 回填后的图标地址（可能为 null） */
  icon: string | null
  /** 首屏文章入库后的未读数 */
  unreadCount: number
  /** 首屏入库的新增文章数 */
  newCount: number
  /**
   * 后台抓取失败原因（成功时为 null）
   *
   * 非空时 `name` / `icon` / `unreadCount` 均为占位值，不得采信——
   * 它是"链接可能填错了"这一场景下用户唯一可见的反馈来源。
   */
  error: string | null
}

/** 按来源自动分组的整理结果（与 Rust 端 `grouping::GroupResult` 对应） */
export interface AutoGroupResult {
  /** 本次新建的分组数量（复用已有分组不计入） */
  created_folders: number
  /** 本次被归入分组的订阅源数量 */
  moved_feeds: number
  /** 涉及的分组名（新建 + 复用），已排序去重 */
  group_names: string[]
}

/**
 * 防止重复注册事件监听的模块级标记
 *
 * Pinia store 的 setup 函数正常只执行一次，但 HMR 热更新会重建 store；
 * 用模块级变量（而非 store 内部状态）保证监听器在整个应用生命周期内只挂一个。
 * 该标记守护本 store 的全部后端事件监听（刷新进度 + 订阅源回填）。
 */
let feedEventListenersRegistered = false

export const useFeedsStore = defineStore('feeds', () => {
  /** 订阅源列表（侧边栏渲染源，含后端重算的 unread_count） */
  const feeds = ref<Feed[]>([])
  /** 当前选中的订阅源（决定文章列表按哪个源筛选；未选中时为 null） */
  const selectedFeed = ref<Feed | null>(null)
  /** 加载状态（列表请求进行中） */
  const loading = ref(false)
  /** 刷新状态（后端正在抓取全部订阅源，耗时较长，需单独指示） */
  const refreshing = ref(false)
  /** 错误信息（最近一次操作的失败原因，成功时置为 null） */
  const error = ref<string | null>(null)
  /** 文件夹列表（侧边栏分组渲染源） */
  const folders = ref<Folder[]>([])
  /**
   * 全量刷新进度（n/m；由后端逐源广播的 `refresh-progress` 事件驱动）
   *
   * null 表示当前没有可展示的进度（未在刷新、或事件尚未到达）；
   * 刷新全部完成后由 `refreshFeeds` 的 finally 复位为 null。
   */
  const refreshProgress = ref<{ done: number; total: number } | null>(null)
  /**
   * 待提示的订阅源抓取错误（null 表示无）
   *
   * 修改链接后后台重抓失败时由 `feed-updated` 事件写入，界面层 watch 到后
   * 弹一次提示并调用 `clearFeedError` 复位。放在 store 而非直接弹窗，
   * 是为了让 store 只承载状态、UI 提示仍由组件决定。
   */
  const feedError = ref<string | null>(null)
  /**
   * 手动全量刷新的失败原因（null 表示上次刷新没出错）
   *
   * 与 `feedError` 分开是为了让提示文案准确：那个通道专指"改链接后重抓失败"，
   * 这里是点击刷新按钮时后端给出的拒绝原因（例如"已有刷新任务在进行中"——
   * 定时任务与手动刷新共用一段逻辑，两者重叠时后端会拒掉后到的那个）。
   * 调用方在 `refreshFeeds()` 返回后读它，读出即用，无需复位。
   */
  const refreshError = ref<string | null>(null)

  // 订阅后端逐源刷新进度事件：每完成一个源就原地更新该源在列表中的未读数，
  // 不必等整轮刷新结束才看到数字变化。监听失败（非 Tauri 环境等）只记日志，
  // 不影响 store 的其余功能——刷新结束后仍会整表重载兜底。
  if (!feedEventListenersRegistered) {
    feedEventListenersRegistered = true
    import('@tauri-apps/api/event')
      .then(({ listen }) => {
        // ① 逐源刷新进度：刷新过程中实时更新未读数与 n/m
        listen<RefreshProgressPayload>('refresh-progress', (event) => {
          const p = event.payload
          // 原地更新对应源的未读数（该源可能刚被删除而不在列表中，此时忽略即可）
          const feed = feeds.value.find((f) => f.id === p.feedId)
          if (feed) feed.unread_count = p.unreadCount
          // 推进 n/m 进度；done === total 时保留最后值，由 refreshFeeds 的 finally 统一复位
          refreshProgress.value = { done: p.done, total: p.total }
        })

        // ② 订阅源后台补全结果：标题 / 图标 / 首屏未读数回填，失败时携带错误。
        //    新增订阅与修改链接都先落库（新增用域名兜底名）立即返回，网络结果
        //    稍后到达，这里就地改写条目，用户无需手动刷新就能看到真实信息。
        listen<FeedUpdatedPayload>('feed-updated', (event) => {
          const p = event.payload
          // 抓取失败：载荷里除 name 外都是占位值，一律不采信，只把错误交给界面提示。
          // 典型场景是"修改订阅链接时地址填错"——没有这条提示用户完全无从察觉。
          if (p.error) {
            feedError.value = p.error
            return
          }
          const feed = feeds.value.find((f) => f.id === p.feedId)
          // 源可能已被删除（或列表尚未重载），此时忽略即可
          if (!feed) return
          feed.name = p.name
          if (p.icon) feed.icon = p.icon
          feed.unread_count = p.unreadCount
        })
      })
      .catch((e) => console.error('注册订阅源事件监听失败:', e))
  }

  /**
   * 加载所有订阅源
   *
   * 整体覆盖而非增量合并：订阅源数量少，且后端会在刷新流程中重算
   * `unread_count`，全量拉取才能拿到最新的未读数。
   *
   * @returns 无返回值；结果写入 `feeds`
   * @throws 不向外抛出：异常被捕获后写入 `error` 并打印日志，`loading` 在 finally 中复位
   */
  async function loadFeeds() {
    loading.value = true
    error.value = null
    try {
      // 动态 import：Tauri 运行时模块按需加载，避免阻塞首屏
      const { invoke } = await import('@tauri-apps/api/core')
      feeds.value = await invoke<Array<Feed>>('feeds_list')
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      console.error('加载订阅源失败:', e)
    } finally {
      loading.value = false
    }
  }

  /**
   * 手动触发所有订阅源刷新
   *
   * 抓取动作（HTTP 请求 + 解析 + 入库）完全由 Rust 后端执行，前端只取回新增条数；
   * 刷新过程中后端会逐源广播 `refresh-progress` 事件（未读数已在上面的监听器里
   * 原地更新），结束后仍整表 `loadFeeds()` 兜底，保证与后端最终一致；
   * finally 复位 `refreshProgress`，避免进度文案残留在界面上。
   *
   * @returns 本次新增的文章数量
   * @throws 不向外抛出：异常被捕获后打印日志并返回 0，`refreshing` 在 finally 中复位
   */
  async function refreshFeeds(): Promise<number> {
    refreshing.value = true
    refreshError.value = null
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const count = await invoke<number>('feeds_refresh')
      // 刷新会改变未读数，必须整表重载以与后端保持一致
      await loadFeeds()
      return count
    } catch (e) {
      // 失败原因写进 refreshError 供调用方提示：只 console 的话，
      // "已有刷新任务在进行中"这种可解释的拒绝会变成用户眼里的"点了没反应"
      refreshError.value = e instanceof Error ? e.message : String(e)
      console.error('刷新订阅源失败:', e)
      return 0
    } finally {
      refreshing.value = false
      // 复位进度指示（监听器只在刷新过程中推进它）
      refreshProgress.value = null
    }
  }

  /**
   * 只刷新单个订阅源（文章列表顶部"刷新当前源"按钮）
   *
   * 与 `refreshFeeds`（全量刷新）不同，只抓取传入的这一个源，适合局部重试而不必等待全部源。
   * 同样在完成后 `loadFeeds()` 重载未读数 / 图标，与后端保持一致。
   *
   * @param feedId - 订阅源 ID（对应 Rust 端 `feed_id`）
   * @returns 本次新增的文章数量；失败时返回 0
   * @throws 不向外抛出：异常被捕获后打印日志并返回 0
   */
  async function refreshOneFeed(feedId: number): Promise<number> {
    refreshing.value = true
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const count = await invoke<number>('feeds_refresh_one', { feedId })
      // 刷新会改变未读数 / 图标，必须整表重载以与后端保持一致
      await loadFeeds()
      return count
    } catch (e) {
      console.error('刷新订阅源失败:', e)
      return 0
    } finally {
      refreshing.value = false
      // 单源刷新也会收到一次进度事件（done=1/total=1），同样复位避免残留
      refreshProgress.value = null
    }
  }

  /**
   * 添加新的订阅源
   *
   * 名称缺省时传空串：后端会先用"域名兜底名"立即落库并返回 ID，真实标题、
   * 图标与首屏文章由后台任务抓取后经 `feed-updated` 事件回填——
   * 因此新源**立即出现在侧边栏**，名称随后自动变为真实标题，无需手动刷新。
   * （旧实现会让后端同步抓取标题后再插入，最坏阻塞 30 秒，期间列表毫无变化。）
   *
   * @param url - RSS/Atom URL（后端据此抓取，表级 UNIQUE，重复订阅会被拒绝）
   * @param name - 订阅源名称（可选，默认使用 URL）
   * @param folderId - 归属文件夹 ID，默认 0（未分类；后端会尝试按来源自动归组）
   * @returns 新订阅源的 ID（自增主键）
   * @throws 后端失败时原样抛出，由界面提示用户（失败时不触发重载）
   */
  async function addFeed(url: string, name?: string, folderId: number = 0) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      // 名称留空时不传 URL 占位——后端会在名称为空/等于 URL 时抓取源并取真实标题，
      // 避免"手动新增的订阅显示为链接而非名称"
      const id = await invoke<number>('feeds_add', {
        name: name || '',
        url,
        folderId
      })
      // 同时重载文件夹列表：后端在新增订阅时会尝试"按来源自动分组"，
      // 可能顺带新建了分组头，只重载 feeds 会让新分组在侧边栏缺失。
      await Promise.all([loadFeeds(), loadFolders()])
      return id
    } catch (e) {
      console.error('添加订阅源失败:', e)
      throw e
    }
  }

  /**
   * 删除订阅源
   *
   * 删除后若该源正处于选中态，需清空 `selectedFeed`，否则界面会继续筛选一个
   * 已不存在的源，展示出空白列表。
   *
   * @param id - 订阅源 ID（后端级联删除其下文章）
   * @returns 无返回值；副作用为后端删除 + 重载列表 + 必要时清空选中态
   * @throws 后端失败时原样抛出
   */
  async function deleteFeed(id: number) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('feeds_delete', { id })
      // 避免选中已删除的源导致界面筛选不到数据
      if (selectedFeed.value?.id === id) {
        selectedFeed.value = null
      }
      // 节流表里的时间戳一并清掉：源删除后 ID 不会复用，但留着属于泄漏
      lastAutoRefreshAt.delete(id)
      await loadFeeds()
    } catch (e) {
      console.error('删除订阅源失败:', e)
      throw e
    }
  }

  /**
   * 更新订阅源（重命名 / 移动文件夹 / 修改链接）
   *
   * 链接是否触发重抓由后端比对后决定：变更才发网络请求，未变则纯落库。
   * 已抓取的历史文章保留在原源下，书签与已读状态不受影响。
   *
   * @param id - 订阅源 ID
   * @param url - 订阅源链接（支持裸 RSSHub 路由，由后端补实例前缀）
   * @param name - 新的订阅源名称；留空表示保持原名
   * @param folderId - 归属文件夹 ID，默认 0（未分类）
   * @returns 无返回值；副作用为后端更新 + 重载列表（名称/分组即时反映到侧边栏与选中态）
   * @throws 后端失败时原样抛出（如「该链接已被其它订阅源占用」），由界面内联提示
   */
  async function updateFeed(
    id: number,
    url: string,
    name: string,
    folderId: number = 0
  ) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      // 参数名与 Rust 端 snake_case 形参同名（url/name/folderId），id 为单字无需转换
      await invoke<void>('feeds_update', { id, url, name, folderId })
      await loadFeeds()
    } catch (e) {
      console.error('更新订阅源失败:', e)
      throw e
    }
  }

  /**
   * 清空待提示的订阅源抓取错误
   *
   * 由界面层在弹出提示后调用，避免同一条错误在下次事件到达前被反复提示。
   *
   * @returns 无返回值；副作用为把 `feedError` 复位为 null
   */
  function clearFeedError() {
    feedError.value = null
  }

  /**
   * 从 OPML 文件批量导入订阅源
   *
   * 文件的选择与读取都在后端：Rust 弹系统「打开文件」对话框并直接解析入库，
   * 前端不参与任何文件操作（与 `exportOpml` 对称）。
   *
   * @returns 新导入订阅源的 ID 列表；用户在对话框中取消时为 `null`（不是错误）
   * @throws 读取文件、解析或写入失败时原样抛出
   */
  async function importOpml(): Promise<number[] | null> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const ids = await invoke<number[] | null>('feeds_import_opml')
      // 取消时不重载列表：什么都没变，避免无谓的往返
      if (ids === null) return null
      await loadFeeds()
      return ids
    } catch (e) {
      console.error('导入 OPML 失败:', e)
      throw e
    }
  }

  /**
   * 导出全部订阅源为 OPML 文件
   *
   * 保存路径由后端弹出系统「另存为」对话框让用户选择，选好后由 Rust 直接写盘；
   * 前端不做任何文件操作，只负责展示结果。
   *
   * @returns 已写入文件的绝对路径；用户在对话框中取消时为 `null`（不是错误）
   * @throws 后端读库或写盘失败时原样抛出
   */
  async function exportOpml(): Promise<string | null> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      return await invoke<string | null>('feeds_export_opml')
    } catch (e) {
      console.error('导出 OPML 失败:', e)
      throw e
    }
  }

  /**
   * 选中订阅源（纯本地状态切换，不触后端）
   *
   * 设计为同步函数，因为它只改变视图筛选条件，没有任何持久化语义；
   * 文章列表的联动加载由调用方（如 ArticleColumn.vue）监听选中态后再触发。
   *
   * @param feed - 订阅源对象（直接持有引用，故界面可即时读取其 name 等字段）
   * @returns 无返回值；副作用为更新 `selectedFeed`
   */
  function selectFeed(feed: Feed) {
    selectedFeed.value = feed
  }

  /* ─── 选中即刷新（带节流）────────────────────────────────────────────────
     点击订阅源时后台静默刷一次该源，让"我刚点开就有新文章"成为常态；
     但刷新是网络动作，不节流的话快速切源会形成刷新风暴，故同一源
     5 分钟内只刷一次。全量刷新进行中（refreshing 已被 refreshFeeds 持有）
     时跳过——避免两路刷新同时写库、进度文案互相覆盖。 */

  /** 自动刷新的节流窗口（毫秒）：同一源在此间隔内的重复选中不再触发刷新 */
  const AUTO_REFRESH_INTERVAL_MS = 5 * 60 * 1000

  /** 各订阅源上次自动刷新的时间戳（key = feedId） */
  const lastAutoRefreshAt = new Map<number, number>()

  /**
   * 选中订阅源后的静默自动刷新（fire-and-forget）
   *
   * 由 FeedPanel 的选中处理器调用，**不 await**——刷新结果经 loadFeeds 重载
   * 未读数、经 ArticleColumn 的既有 watch 反映到列表，调用方不该被网络阻塞。
   *
   * 节流规则：同一源 5 分钟内只刷一次（无论结果成败，时间戳都推进——
   * 失败重试应走列表顶部的「刷新当前源」按钮，而不是每次点击都重试）。
   *
   * @param feedId - 被选中的订阅源 ID
   * @returns 无返回值；失败已由 refreshOneFeed 内部吞掉（返回 0）
   */
  async function autoRefreshOnSelect(feedId: number): Promise<void> {
    if (refreshing.value) return
    const now = Date.now()
    const last = lastAutoRefreshAt.get(feedId) ?? 0
    if (now - last < AUTO_REFRESH_INTERVAL_MS) return
    lastAutoRefreshAt.set(feedId, now)
    await refreshOneFeed(feedId)
  }

  /**
   * 取消选中订阅源（回到"全部文章"范围）
   *
   * 供标签视图切换时使用：标签与订阅源是**互斥的范围**，选中标签必须先退出源选择，
   * 否则两个范围同时生效，列表会陷入"看着是标签、实际被源过滤掉一半"的混乱。
   *
   * @returns 无返回值；副作用为把 `selectedFeed` 置空
   */
  function clearSelectedFeed() {
    selectedFeed.value = null
  }

  /**
   * 加载文件夹列表（侧边栏分组渲染源）
   *
   * 失败仅记录日志：文件夹是分组辅助信息，加载失败不应阻塞订阅源列表展示。
   */
  async function loadFolders() {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      folders.value = await invoke<Array<Folder>>('folders_list')
    } catch (e) {
      console.error('加载文件夹失败:', e)
    }
  }

  /**
   * 新建文件夹
   *
   * @param name - 文件夹名称
   * @returns 无返回值；副作用为后端创建 + 重载列表
   * @throws 后端失败时原样抛出
   */
  async function createFolder(name: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<number>('folders_create', { name, position: folders.value.length })
      await loadFolders()
    } catch (e) {
      console.error('新建文件夹失败:', e)
      throw e
    }
  }

  /**
   * 重命名 / 调整文件夹顺序
   *
   * @param id - 文件夹 ID
   * @param name - 新名称
   * @param position - 新顺序
   * @throws 后端失败时原样抛出
   */
  async function updateFolder(id: number, name: string, position: number) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('folders_update', { id, name, position })
      await loadFolders()
    } catch (e) {
      console.error('更新文件夹失败:', e)
      throw e
    }
  }

  /**
   * 删除文件夹（其下订阅源自动移回"未分类"）
   *
   * @param id - 文件夹 ID
   * @throws 后端失败时原样抛出
   */
  async function deleteFolder(id: number) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('folders_delete', { id })
      await loadFolders()
    } catch (e) {
      console.error('删除文件夹失败:', e)
      throw e
    }
  }

  /**
   * 把订阅源移动到指定文件夹（folderId=0 表示移出、归入"未分类"）
   *
   * @param feedId - 订阅源 ID
   * @param folderId - 目标文件夹 ID
   * @throws 后端失败时原样抛出
   */
  async function moveFeedToFolder(feedId: number, folderId: number) {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke<void>('feeds_set_folder', { feedId, folderId })
      await loadFeeds()
    } catch (e) {
      console.error('移动订阅源失败:', e)
      throw e
    }
  }

  /**
   * 按来源自动分组（一键整理存量订阅源）
   *
   * 由后端扫描全部订阅源，把 URL 指向同一来源（普通站点取域名、RSSHub 源取路由
   * 第一段平台名）且数量达到 2 个的**未分类**源归入同名分组。
   * 已被手工归入文件夹的源不受影响，因此可以放心重复点击。
   *
   * 副作用不仅限于 feeds：后端可能新建文件夹，所以两者都要重载，
   * 否则侧边栏会出现"订阅源已归组但分组头不存在"的错乱。
   *
   * @returns 后端返回的整理结果（新建分组数 / 归入源数 / 分组名列表）
   * @throws 后端失败时原样抛出，由界面提示
   */
  async function autoGroupBySource(): Promise<AutoGroupResult> {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const result = await invoke<AutoGroupResult>('feeds_auto_group')
      await Promise.all([loadFolders(), loadFeeds()])
      return result
    } catch (e) {
      console.error('按来源自动分组失败:', e)
      throw e
    }
  }

  /**
   * 获取指定订阅源的 AI 配置
   */
  async function getFeedAiConfig(feedId: number): Promise<FeedAiConfig> {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<FeedAiConfig>('feed_ai_config_get', { feedId })
  }

  /**
   * 保存订阅源的 AI 配置
   */
  async function saveFeedAiConfig(config: FeedAiConfig): Promise<number> {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<number>('feed_ai_config_save', { config })
  }

  /**
   * 删除订阅源的 AI 配置
   */
  async function deleteFeedAiConfig(feedId: number): Promise<void> {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<void>('feed_ai_config_delete', { feedId })
  }

  /**
   * 获取全部订阅源的 AI 配置列表
   */
  async function listFeedAiConfigs(): Promise<FeedAiConfig[]> {
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<FeedAiConfig[]>('feed_ai_configs_list')
  }

  return {
    feeds,
    selectedFeed,
    loading,
    refreshing,
    refreshProgress,
    refreshError,
    feedError,
    clearFeedError,
    folders,
    loadFeeds,
    refreshFeeds,
    refreshOneFeed,
    addFeed,
    deleteFeed,
    updateFeed,
    importOpml,
    exportOpml,
    selectFeed,
    autoRefreshOnSelect,
    clearSelectedFeed,
    loadFolders,
    createFolder,
    updateFolder,
    deleteFolder,
    moveFeedToFolder,
    autoGroupBySource,
    getFeedAiConfig,
    saveFeedAiConfig,
    deleteFeedAiConfig,
    listFeedAiConfigs
  }
})
