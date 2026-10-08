<template>
  <!-- 文章列表栏：三栏布局的"中间列"，自身是一个独立的 flex 列。
       顶部是操作条组件与排序栏，下方是分组后的文章列表。
       列表行渲染见 ArticleListItem.vue，头部控件见 ArticleListToolbar.vue。
       窄屏（<=768px）下整列变为抽屉，由 showArticleList 控制滑出。 -->
  <div class="article-column pane-glow-host" :class="{ open: showArticleList }" ref="paneRef">
    <!-- 跟随边框：靠近指针的那一侧亮起一段 -->
    <div class="pane-frame" ref="frameRef" aria-hidden="true"></div>
    <!-- 顶部固定操作条：搜索 / 标签范围 / 仅未读 / 仅书签 / 刷新 / 全部已读 -->
    <ArticleListToolbar
      :active-tag="activeTag"
      :unread-in-scope="unreadInScope"
      @clear-search="clearSearch"
      @clear-tag-scope="clearTagScope"
      @refresh="refreshCurrent"
      @mark-all="markAll"
    />

    <!-- 操作反馈浮层：标记已读等批量动作完成后短暂显示，2.5s 后自动消失。
         留在本组件而不是操作条内部：它绝对定位在整列（.article-column）下方居中，
         需要以列容器为定位基准。 -->
    <transition name="toast-fade">
      <div v-if="toast" class="tb-toast">{{ toast }}</div>
    </transition>

    <!-- 排序栏：切换后按当前范围重新从第 0 页加载（排序由后端在全库范围内完成）。
         书签视图额外露出「分组」维度——默认按标签（收藏夹），可切回时间。
         外观与交互见 ArticleSortBar.vue；排序动作（清缩略图缓存 + 重载）留在本组件。 -->
    <ArticleSortBar
      :group-mode="bookmarkGroupMode"
      @update:group-mode="bookmarkGroupMode = $event"
      @change-sort="changeSort"
    />

    <!-- 文章条目列表：按当前排序方式渲染（时间序按日期分组，其它排序平铺）。
         SelectCapsule 是选中态的唯一视觉载体（琥珀描边变体），必须与条目同处
         .article-body 这个定位基准内，且渲染在条目之前。 -->
    <div class="article-body" v-if="articles.length > 0" ref="articleBodyRef"
         @mouseover="onListMouseOver"
         @mouseout="onListMouseOut"
    >
      <SelectCapsule ref="articleCapsule" variant="accent" :container="() => articleBodyRef" />
      <template v-for="group in groupedArticles" :key="group.label">
        <div class="date-group-header" v-if="group.label">
          <!-- 色点只在标签分组（收藏夹）下出现；「未分类」是兜底桶，没有颜色可言 -->
          <span
            class="group-dot"
            v-if="group.tone"
            :style="{ '--tag-c': group.tone }"
            aria-hidden="true"
          ></span>
          <span class="group-name">{{ group.label }}</span>
          <!-- 条目数只在标签分组下有意义（日期分组的条数对阅读没有帮助） -->
          <span class="group-count" v-if="isTagGrouping">{{ group.items.length }}</span>
        </div>
        <ArticleListItem
          v-for="article in group.items"
          :key="article.id"
          :article="article"
          :active="selectedArticle?.id === article.id"
          :data-article-id="article.id"
          @select="handleSelectArticle"
          @preference="handlePreference"
        />
      </template>

      <!-- 加载更多：单源与全部视图都适用；搜索结果由后端一次性返回，不分页 -->
      <button
        class="load-more"
        v-if="!searchActive && articlesStore.hasMore"
        :disabled="articlesStore.loading"
        @click="loadMore"
      >
        {{ articlesStore.loading ? '加载中…' : '加载更多' }}
      </button>
      <p class="list-end" v-else-if="!searchActive">已经到底了</p>
    </div>

    <!-- 加载态：首屏 / 切换订阅源时 articles 为空但仍在拉取。
         必须先于空状态判断——否则加载期间会闪出「暂无文章」，
         让用户以为真的没内容（这是本分支此前存在的实际缺陷）。 -->
    <ArticleListSkeleton v-else-if="articlesStore.loading" />

    <!-- 列表空状态：仅在「加载完成 且 确实没有文章」时出现。
         搜索无结果时补一个放大镜图标，与正文的引导文案区分开。 -->
    <div v-else class="empty-list state-block">
      <svg v-if="searchActive" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
        <circle cx="11" cy="11" r="7"></circle>
        <line x1="20" y1="20" x2="16.2" y2="16.2"></line>
      </svg>
      <p v-if="searchActive">未找到与「{{ articlesStore.search }}」相关的文章</p>
      <p v-else>暂无文章，点击「刷新」或稍后再试</p>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 文章列表栏组件（三栏布局的中间列）
 *
 * # 职责
 * 只负责本列的**编排**：操作条与排序栏的摆放、按「当前范围 + 视图筛选」加载文章、
 * 分组渲染、加载更多与空态。渲染细节已拆给两个子组件：
 * - `ArticleListToolbar.vue`：搜索框 / 标签范围指示 / 仅未读 / 仅书签 / 刷新 / 全部已读；
 * - `ArticleListItem.vue`：单篇文章那一行（圆点 / 标题 / 来源 / 时间 / 标签 chip / 缩略图）。
 * 分组规则（时间线 / 收藏夹）是纯函数，住在 `utils/articleGroups.ts`。
 *
 * # 与 Pinia store 的关系
 * - `useFeedsStore()`：读 selectedFeed（当前范围）与 feeds（未读汇总），刷新走 refreshOneFeed / refreshFeeds；
 * - `useArticlesStore()`：读 displayArticles / viewFilter / search / tagFilterId / sortMode，
 *   并通过 loadArticles / loadMoreArticles / selectArticle / setViewFilter / markAllRead 触发行为；
 * - `useTagsStore()`：读 tags 目录，把标签范围解析成名称与颜色，其名称序也用于收藏夹分组排序。
 *
 * # 唯一的加载入口
 * 「什么时候该重新拉列表」只由 `scopeKey` 这一个 watcher 回答：把范围（标签 > 订阅源 > 全部）
 * 与视图筛选（书签 / 未读）合成一个可比较的字符串，避免一次切换触发两轮加载。
 */

import { ref, watch, computed, onMounted, onUnmounted, nextTick } from 'vue'
import { useArticlesStore, type ArticleSortKey } from '@/stores/articles'
import { useFeedsStore } from '@/stores/feeds'
import { useTagsStore } from '@/stores/tags'
import { groupByDate, groupByTag, type ArticleGroup } from '@/utils/articleGroups'
import { clearThumbnailCache } from '@/utils/articleThumb'
import ArticleListItem from './ArticleListItem.vue'
import ArticleListSkeleton from './ArticleListSkeleton.vue'
import ArticleListToolbar from './ArticleListToolbar.vue'
import ArticleSortBar from './ArticleSortBar.vue'
import SelectCapsule from './SelectCapsule.vue'
import { useCursorGlow } from '@/composables/useCursorGlow'
import type { Feed, Article, Tag } from '@/types'


const articlesStore = useArticlesStore()
const feedsStore = useFeedsStore()
const tagsStore = useTagsStore()

/** 移动端文章列表抽屉是否展开（窄屏语义） */
const showArticleList = ref(false)

/** 顶部操作条的一次性提示文案（如"已全部标记为已读"），2.5s 后自动清空 */
const toast = ref('')
let toastTimer: ReturnType<typeof setTimeout> | undefined

/**
 * 组件内在途定时器的句柄
 *
 * 提示与搜索防抖都靠 `setTimeout` 到期复位 / 触发，统一登记以便卸载时一次性清空
 * ——尤其是搜索防抖：组件销毁后仍可能补发一次后端检索请求。
 */
const timers: ReturnType<typeof setTimeout>[] = []

/**
 * 登记一个延时回调并返回句柄
 *
 * @param fn - 到期执行的回调
 * @param delay - 延迟毫秒数
 * @returns 定时器句柄（需要提前取消时交给 `clearTimeout`）
 */
function schedule(fn: () => void, delay: number) {
  const id = setTimeout(fn, delay)
  timers.push(id)
  return id
}

// 卸载时清空全部在途定时器：既防状态写入已销毁组件，也避免防抖补发无意义的后端请求
onUnmounted(() => {
  timers.forEach(clearTimeout)
  timers.length = 0
})

/**
 * 显示一条短暂的操作反馈
 *
 * @param text - 提示文案
 * @returns 无
 */
function showToast(text: string) {
  toast.value = text
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = schedule(() => { toast.value = '' }, 2500)
}

/** 当前选中的订阅源（决定列表范围；null 表示"全部文章"统一时间线） */
const selectedFeed = computed<Feed | null>(() => feedsStore.selectedFeed)

/**
 * 当前生效的标签（标签视图的范围标识；null 表示不按标签筛选）
 *
 * 从 tags 目录里反查而非直接存 Tag 对象：目录重载（改名/改色）后指示条会自动跟着更新，
 * 不会停留在旧名字上。
 */
const activeTag = computed<Tag | null>(() => {
  const id = articlesStore.tagFilterId
  if (id === null) return null
  return tagsStore.tags.find((t) => t.id === id) ?? null
})

/**
 * 范围键
 *
 * 把「当前范围」压成一个可与 watch 比较的字符串：`all` / `feed:{id}` / `tag:{id}`，
 * 视图筛选再追加 `|bm`（书签）或 `|un`（仅未读），如 `feed:3|bm`。
 * 用一个键而不是分别 watch selectedFeed 与 tagFilterId，是为了避免切换范围时
 * 两者先后变化触发**两次**加载（先按旧范围拉一页、再按新范围拉一页，中间白白闪一次）。
 *
 * 为什么书签与未读都要进范围键：两者自走后端参数（`bookmarked` / `unread`）后，
 * 进出对应视图**必须重新拉取**，否则后端过滤了、前端还拿着未过滤的旧列表。
 * 「全部」视图不追加后缀——它是无范围的基准形态。
 *
 * @returns 范围标识字符串
 */
const scopeKey = computed<string>(() => {
  let key = 'all'
  if (articlesStore.tagFilterId !== null) key = `tag:${articlesStore.tagFilterId}`
  else if (feedsStore.selectedFeed) key = `feed:${feedsStore.selectedFeed.id}`
  if (articlesStore.viewFilter === 'bookmarked') return `${key}|bm`
  if (articlesStore.viewFilter === 'unread') return `${key}|un`
  return key
})

/**
 * 退出标签视图
 *
 * 只清标签范围、不选任何源，回到"全部文章"。列表重载由上面的 scopeKey watcher 负责。
 */
function clearTagScope() {
  articlesStore.setTagFilter(null)
}
/** 是否处于后端全文搜索模式（搜索框非空） */
const searchActive = computed<boolean>(() => articlesStore.search.trim().length > 0)
/**
 * 当前渲染的文章列表（已按视图 tab + 搜索过滤）
 *
 * 搜索模式（search 非空）下取后端 `searchResults`（FTS5 全文检索，覆盖标题/正文/摘要），
 * 否则取本地 `displayArticles`（仅按视图 tab 过滤，不做标题子串过滤以避免与全文检索重复）。
 */
const articles = computed<Article[]>(() =>
  searchActive.value ? articlesStore.searchResults : articlesStore.displayArticles
)
/** 当前选中的文章（用于列表项 active 高亮） */
const selectedArticle = computed<Article | null>(() => articlesStore.selectedArticle)

/**
 * 标签范围内的未读数（由后端 `articles_unread_count` 返回）
 *
 * 标签横跨所有源，未读数不落在任何一张表上，只能回后端数一次——否则「全部已读」
 * 按钮的计数会退化成"已加载那几页里的未读"，与实际会被影响的文章数不符。
 */
const tagUnreadCount = ref(0)

/**
 * 重新统计标签范围内的未读数
 *
 * 抽成函数而非只写在 watch 里：markAll 把当前范围全部置已读后，未读数必须立刻归零，
 * 否则按钮会停留在"还有未读"的旧状态上。
 *
 * @returns 无返回值；结果写入 `tagUnreadCount`
 */
async function loadTagUnread() {
  const id = articlesStore.tagFilterId
  if (id === null) {
    tagUnreadCount.value = 0
    return
  }
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    tagUnreadCount.value = await invoke<number>('articles_unread_count', { tagId: id })
  } catch (e) {
    // 计数失败不阻断阅读：退化为按已加载页统计，宁可数字偏小也不弹错误
    console.error('统计标签未读数失败:', e)
    tagUnreadCount.value = articles.value.filter((a) => !a.is_read).length
  }
}

// 切换标签即重新统计；immediate 让直接落在标签视图时也能拿到数字
watch(() => articlesStore.tagFilterId, loadTagUnread, { immediate: true })

/**
 * 当前范围内的未读总数
 *
 * 单源视图取该源 `unread_count`、全部视图对所有源求和（两者都来自 `feeds` 表，
 * 即时且零请求）；标签视图没有预存数字，回后端数一次。
 * 标签视图下的**未读视图**是例外：此时列表本身就是"后端返回的全部未读"，
 * 本地统计既准确又实时（读完一篇会立刻被兜底过滤移出列表，计数随之下降），
 * 不必等后端往返。
 * 仅用于顶部操作条的按钮可用性，不参与筛选逻辑。
 *
 * @returns 未读文章数量
 */
const unreadInScope = computed<number>(() => {
  if (articlesStore.tagFilterId !== null) {
    if (articlesStore.viewFilter === 'unread') {
      return articles.value.filter((a) => !a.is_read).length
    }
    return tagUnreadCount.value
  }
  if (selectedFeed.value) return selectedFeed.value.unread_count
  return feedsStore.feeds.reduce((sum, f) => sum + f.unread_count, 0)
})

/**
 * 书签视图的分组维度
 *
 * `tag` = 把标签当作「收藏夹」（默认）；`time` = 保留原来的日期分组。
 * 只影响书签视图，其它视图恒走日期分组。
 */
const bookmarkGroupMode = ref<'tag' | 'time'>('tag')

/**
 * 当前是否按标签（收藏夹）分组
 *
 * 模板据此决定分组头要不要显示条目数——`groupedArticles` 是否走标签分支
 * 与这里同源，两处共用同一个判断，避免"分组方式与头部样式不一致"。
 */
const isTagGrouping = computed<boolean>(
  () => articlesStore.viewFilter === 'bookmarked' && bookmarkGroupMode.value === 'tag'
)

/**
 * 标签的排序位置
 *
 * 目录由 `tags_list` 按名称返回，而每篇文章的 `tags` 由后端按同样的名称序挂载，
 * 因此这里的位置既可给分组排序，也可判定文章"最靠前的标签"。收藏夹分组的分组顺序
 * 与"文章归哪一组"都读它，传进 `utils/articleGroups.ts` 的 `groupByTag`。
 */
const tagRank = computed<Map<number, number>>(() => {
  const m = new Map<number, number>()
  tagsStore.tags.forEach((t, i) => m.set(t.id, i))
  return m
})

/**
 * 渲染用的分组列表
 *
 * 两种分组形态（规则见 `utils/articleGroups.ts`）：
 * 1. **书签视图（默认按标签）**——把标签当收藏夹，组顺序按标签名称序，「未分类」置底。
 *    这个形态在任意排序下都成立：组标签不会被"交错出现"，所以不像日期分组那样
 *    需要限定在时间序下（见下）。
 * 2. **日期分组**——与 Folo 时间线一致，按 今天 / 昨天 / 本周 / 本月 / 更早，依赖后端
 *    `published_at` 倒序返回。切到「重要」或「名称」后不再分组：分组标签表达的是
 *    "时间远近"，按评分排序时会出现"今天 / 更早 / 今天"交错，反而看不清顺序；
 *    此时返回一个无名分组，模板据此不渲染分组头。
 *
 * @returns 分组数组；各组 label 唯一、items 非空
 */
const groupedArticles = computed<ArticleGroup[]>(() => {
  if (isTagGrouping.value) return groupByTag(articles.value, tagRank.value)
  if (articlesStore.sortMode !== 'latest') return [{ label: '', items: articles.value }]
  return groupByDate(articles.value)
})

/**
 * 选中文章：委托 store 标记已读并写入 selectedArticle；窄屏下收起抽屉
 *
 * **不**在这里同步胶囊：`articlesStore.selectArticle` 是async（内部 await
 * `loadArticle` 走一次 IPC），它的`selectedArticle` 赋值发生在若干微任务之后。
 * 若在此处 `nextTick(syncCapsule)`，读到的 `.article-item.active` 还是**上一次**
 * 选中的那项——高亮于是滞后一次点击。
 *
 * 胶囊同步统一交给下面的 watch（选中态的**唯一**监听点），这里只登记"本次变化
 * 来自用户点击"，让那条路径决定用滑动而非瞬时落位。
 *
 * @param article - 被点击的文章
 * @returns 无返回值
 */
function handleSelectArticle(article: Article) {
  // 先置意图再调用：selectArticle 可能在 await 之后的同一轮里就改变选中态
  clickedRecently = true
  void articlesStore.selectArticle(article.id)
  showArticleList.value = false
}

/**
 * 选中态变化后的胶囊同步入口
 *
 * # 为什么 watch 是唯一路径
 * 曾用「点击回调里同步 + watch 里跳过」的组合，结果**高亮滞后一次点击**：
 * 点击回调的 nextTick 早于 selectArticle 的 await，读到的是旧选中项；
 * 而真正拿到新选中项的 watch 恰好被标记跳过。两者正好互换。
 *
 * 改为只保留 watch 一条路径后，选中态与胶囊永远同源，不可能错位。
 *
 * # clickedRecently 的作用
 * 仅用于决定**动效**而非**同步与否**：用户点击要"滑过去"，
 * 命令面板跳转 / 外部赋值只需瞬时落位。消费后立即复位。
 */
let clickedRecently = false
watch(
  () => selectedArticle.value?.id,
  () => {
    const sliding = clickedRecently
    clickedRecently = false
    // 等active 类落位再测量：此时 selectedArticle 已是新值，
    // querySelector('.article-item.active') 才能取到本次点击的那一项。
    void nextTick(() => syncCapsule(sliding))
  },
)

/** 文章列表滚动区（滑动胶囊的定位基准） */
const articleBodyRef = ref<HTMLElement | null>(null)
/** 面板根元素（光斑与跟随边框的宿主） */
const paneRef = ref<HTMLElement | null>(null)
/** 跟随边框层 */
const frameRef = ref<HTMLElement | null>(null)
/** 启用面板光斑跟随 + 靠近侧边框亮起 */
useCursorGlow(paneRef, frameRef)
/** 选中胶囊组件实例：把琥珀胶囊滑到当前正在读的那一篇 */
const articleCapsule = ref<InstanceType<typeof SelectCapsule> | null>(null)

/**
 * 把选中胶囊同步到当前选中文章的位置
 *
 * 搜索 / 换排序 / 换范围都会让条目重排，这些是"布局已变、位置直接跳过去"的场景，
 * 必须跳过过渡（否则用户会看到胶囊从旧位置慢慢飞过来，很像卡顿）；
 * 只有点击选中才用带过渡的滑动。
 *
 * @param noTransition - 是否跳过过渡直接落位
 * @returns 无返回值
 */
/**
 * 把选中胶囊同步到当前选中文章的位置
 *
 * @param sliding - true 表示用户主动点击，胶囊滑动过去；
 *   false 表示布局已变（换范围 / 换排序 / 搜索），瞬时落位。
 *   默认 false：布局变化远比点击频繁，那些场景下带过渡会被看成卡顿。
 * @returns 无返回值
 */
function syncCapsule(sliding = false) {
  const body = articleBodyRef.value
  const capsule = articleCapsule.value
  if (!body || !capsule) return
  capsule.sync(body.querySelector<HTMLElement>('.article-item.active'), sliding)
}

/**
 * 指针移动时把 hover 胶囊跟到所在条目上
 *
 * 用捕获型事件委托而非逐条绑定：条目列表会随加载更多 / 换排序重排，
 * 逐条绑定需要反复解绑重绑。`mouseover` 会冒泡故可委托，`mouseenter` 不行。
 *
 * @param e - 鼠标事件
 * @returns 无返回值
 */
function onListMouseOver(e: MouseEvent) {
  const body = articleBodyRef.value
  const capsule = articleCapsule.value
  if (!body || !capsule) return
  capsule.hover((e.target as HTMLElement | null)?.closest<HTMLElement>('.article-item') ?? null)
}

/**
 * 指针离开列表时隐藏 hover 胶囊
 *
 * 若只在 mouseover 里维护，指针移出列表后胶囊会停在最后一项上不消失。
 *
 * @param e - 鼠标事件；用 relatedTarget 区分"真离开"与"在列表内部移动"
 * @returns 无返回值
 */
function onListMouseOut(e: MouseEvent) {
  const body = articleBodyRef.value
  const capsule = articleCapsule.value
  if (!body || !capsule) return
  const to = e.relatedTarget as Node | null
  if (to && body.contains(to)) return
  capsule.hover(null)
}

/**
 * 用户点击偏好按钮：调用 store 更新偏好，刷新列表以应用过滤
 *
 * @param articleId - 文章 ID
 * @param preference - 'like' | 'skip'
 */
async function handlePreference(articleId: number, preference: 'like' | 'skip') {
  await articlesStore.setPreference(articleId, preference)
  // 重新加载列表以应用新的过滤条件
  await articlesStore.loadArticles(feedsStore.selectedFeed?.id ?? null)
}

/** 刷新当前范围：有选中源时只刷该源，否则刷新全部；完成后重载当前列表取回新文章 */
async function refreshCurrent() {
  try {
    if (selectedFeed.value) {
      await feedsStore.refreshOneFeed(selectedFeed.value.id)
    } else {
      await feedsStore.refreshFeeds()
      // 全量刷新可能被后端拒掉（与定时刷新重叠），把原因显示出来而不是静默无反应
      if (feedsStore.refreshError) showToast(feedsStore.refreshError)
    }
    clearThumbnailCache()
    await articlesStore.loadArticles(selectedFeed.value?.id ?? null)
  } catch (e) {
    console.error('刷新失败:', e)
    showToast('刷新失败：' + String(e))
  }
}

/**
 * 切换排序方式
 *
 * 排序在后端完成，因此必须重新从第 0 页加载：只在已加载的几页里重排会得到
 * "看着像排好了、其实只排了一半"的假象（比如第 51 条才是最高分的文章）。
 * 用同一个 key 重复点击时直接返回，避免无谓的重新加载。
 *
 * @param key - 目标排序方式
 * @returns 无返回值；副作用为更新 store 的 sortMode 并重载列表
 */
async function changeSort(key: ArticleSortKey) {
  if (articlesStore.sortMode === key) return
  articlesStore.setSortMode(key)
  clearThumbnailCache()
  await articlesStore.loadArticles(selectedFeed.value?.id ?? null, undefined, 0)
}

/**
 * 加载下一页
 *
 * 以 store 已持有的列表长度为偏移量，由 store 决定"是否还有更多 / 是否正在加载"，
 * 组件只负责把当前范围传下去。
 *
 * @returns 无返回值；新一页追加到列表末尾
 */
async function loadMore() {
  await articlesStore.loadMoreArticles(selectedFeed.value?.id ?? null)
}

/**
 * 全部已读：标记当前范围未读为已读，并切回"全部"视图
 *
 * 切回"全部"视图是为了避免"仅未读"视图下列表瞬间清空，
 * 让用户以为自己误删了内容。失败时给出明确提示而非静默失败。
 *
 * 标签视图下范围由 store 的 `tagFilterId` 一并下推（见 `markAllRead`），
 * 因此这里只负责传订阅源部分。
 */
async function markAll() {
  try {
    const count = unreadInScope.value
    await articlesStore.markAllRead(selectedFeed.value?.id ?? null)
    // 范围内已全部置已读，标签未读计数必须跟着归零，否则按钮会停在旧状态
    await loadTagUnread()
    articlesStore.setViewFilter('all')
    showToast(count > 0 ? `已将 ${count} 篇标记为已读` : '当前没有未读文章')
  } catch (e) {
    showToast('标记已读失败：' + String(e))
  }
}

/**
 * 监听「范围键」变化，重新加载对应范围的文章
 *
 * 范围三选一：标签 > 订阅源 > 全部（标签优先级最高，因为选中标签时会先清空源选择）。
 * 用一个 scopeKey 而非分别 watch `selectedFeed` / `tagFilterId`，是为了让
 * "切到标签视图"这种同时改动两个状态的操只触发一次加载。
 */
watch(scopeKey, async () => {
  clearThumbnailCache()
  // 标签视图下不再限定源（标签本身即是范围）；loadArticles 会自动带上 tagFilterId
  const feedId = articlesStore.tagFilterId !== null ? null : (feedsStore.selectedFeed?.id ?? null)
  await articlesStore.loadArticles(feedId)
  showArticleList.value = true
  // 列表整体换掉了，胶囊要跟着落到新列表里当前选中的那一篇
  void nextTick(() => syncCapsule())
})

/**
 * 响应命令面板的跳转：把琥珀胶囊滑到目标文章并播一次定位脉冲
 *
 * 目标文章可能不在**当前已加载的分页**里（命令面板搜的是全库）。此时条目不存在，
 * 只能把胶囊隐藏——正文区照样会渲染该文章，定位反馈由订阅栏那一侧承担。
 *
 * @param e - `command-palette-jump` 事件，detail 含 articleId
 * @returns 无返回值
 */
function onPaletteJump(e: Event) {
  const articleId = (e as CustomEvent<{ articleId: number }>).detail?.articleId
  if (typeof articleId !== 'number') return
  const body = articleBodyRef.value
  const capsule = articleCapsule.value
  if (!body || !capsule) return
  const el = body.querySelector<HTMLElement>(`.article-item[data-article-id="${articleId}"]`)
  if (!el) {
    capsule.sync(null)
    return
  }
  // 先滚进视野，否则用户看不到胶囊落在哪
  el.scrollIntoView({ block: 'nearest' })
  capsule.sync(el)
  capsule.pulse()
}

/** 监听搜索框：输入后防抖 250ms 触发后端全文检索；清空时回落本地列表 */
let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(
  () => articlesStore.search,
  (q) => {
    if (searchTimer) clearTimeout(searchTimer)
    const query = q.trim()
    if (!query) {
      // 清空搜索：复位后端结果，列表自动回落到本地 displayArticles
      articlesStore.searchResults = []
      return
    }
    // 防抖：避免每次击键都打后端；FTS5 trigram 子串匹配在本地库上足够快
    searchTimer = schedule(() => { articlesStore.searchArticles(query) }, 250)
  }
)

/** 清空搜索：复位搜索框与后端结果，回落到本地列表 */
function clearSearch() {
  articlesStore.search = ''
  articlesStore.searchResults = []
}

/** 挂载时首次取数：未选中任何源时加载全部文章统一时间线（启动默认视图） */
onMounted(async () => {
  window.addEventListener('command-palette-jump', onPaletteJump)
  if (!feedsStore.selectedFeed) {
    await articlesStore.loadArticles(null)
  }
})

onUnmounted(() => {
  window.removeEventListener('command-palette-jump', onPaletteJump)
})
</script>

<style scoped>
.article-column {
  /* 列宽可由拖拽分隔条调整（见 App.vue 的 .col-resizer），默认 360px，对齐 Folo 的 entryColWidth */
  width: var(--article-w, 360px);
  /* 作为内部绝对定位元素（操作反馈浮层 .tb-toast、滑动胶囊）的定位基准 */
  position: relative;
  /* 玻璃化：与订阅栏同一套玻璃令牌（--glass / --glass-hi / --glass-hair），
     保证两块面板材质一致、看起来像同一片玻璃被裁成两块。 */
  background: var(--glass);
  backdrop-filter: blur(var(--blur)) saturate(1.5);
  -webkit-backdrop-filter: blur(var(--blur)) saturate(1.5);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  border-radius: var(--r-panel);
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    inset 0 -1px 0 var(--glass-lo),
    0 2px 6px rgba(31, 45, 70, 0.05),
    0 10px 30px rgba(31, 45, 70, 0.07);
  border: 1px solid var(--glass-hair);
  height: 100%;
  overflow: hidden;
}

/* 操作反馈浮层：绝对定位在操作条下方居中，不占布局空间、不遮挡列表点击 */
.tb-toast {
  position: absolute;
  top: 48px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 50;
  padding: var(--sp-05) var(--sp-35);
  border-radius: var(--r-md);
  background: var(--text-primary);
  color: var(--surface);
  font-size: var(--fs-sm);
  white-space: nowrap;
  box-shadow: var(--shadow-hover);
  pointer-events: none;
}
.toast-fade-enter-active,
.toast-fade-leave-active { transition: opacity 0.2s, transform 0.2s; }
.toast-fade-enter-from,
.toast-fade-leave-to { opacity: 0; transform: translate(-50%, -6px); }

/* 「加载更多」与列表终点提示：两者共用同一条底部分隔线的位置，
   一个负责"还能继续翻"，一个负责明确告诉用户"没有更多了" */
.load-more {
  display: block;
  width: calc(100% - 24px);
  margin: var(--sp-15) var(--sp-3) var(--sp-35);
  padding: var(--sp-2) 0;
  border-radius: var(--r-md);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  font-family: inherit;
  cursor: pointer;
  transition: border-color var(--dur) var(--ease), color var(--dur) var(--ease),
    background var(--dur) var(--ease);
}
.load-more:hover:not(:disabled) {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--primary-fg);
}
.load-more:disabled { cursor: default; opacity: 0.6; }
.list-end {
  margin: var(--sp-15) 0 18px;
  text-align: center;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

/* 列表滚动区：左侧补一个与滚动条等宽的内边距做"视觉配重"——
   滚动条只出现在右侧并占据 --sb-w 的宽度，若不补偿，卡片左边贴面板边、
   右边却空出 13px，hover / active 的高亮块看起来就是偏左的。

   position: relative 是滑动胶囊的定位基准；--cap-* 决定胶囊的左右留白，
   需与下方 .article-item 的外边距对齐。 */
.article-body {
  position: relative;
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  padding-left: var(--sb-w, 13px);
  --cap-left: var(--sp-05);
  --cap-right: var(--sp-05);
}
/* 滚动条风格统一收敛到 styles.css 全局规则，此处不再单独重写 */

/* 加载态骨架屏：按真实 .article-item 几何铺排——左侧「标题行 +  meta 行」，
   右侧 52px 方形缩略图位。padding / gap / 缩略图圆角与真实条目一致，
   保证加载完成切换时列表不跳版（几何数值改动需与 ArticleListItem.vue 同改）。 */
.skeleton-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-left: var(--sb-w, 13px);
}

/* 日期分组头：小号灰字 + 浅底，扫读时快速定位时间段，吸顶在列表滚动区。
   面板已是半透明玻璃，故底色必须用**不透明**色：半透明色会让下方条目从分组头里透出来，
   滚动时形成一片糊影。 */
.date-group-header {
  /* 左内边距 26px = 卡片外边距 6 + 卡片左内边距 12 + 圆点悬挂缩进 14，
     让分组头的文字与下方卡片标题严格左对齐（圆点占位后标题整体右移了） */
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-05) var(--sp-4) var(--sp-05) 26px;
  font-size: var(--fs-xs);
  font-weight: 500;
  color: var(--text-tertiary);
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 0;
  /* z-index 高于胶囊（0）与条目（1）：分组头吸顶时必须压住滚过去的条目，
     否则条目会从分组头下方"穿"过来 */
  z-index: 2;
}
/* 注：原先这里有一条 `.date-group-header + .article-item { border-top: none }`，
   条目卡片化（圆角卡片、无边框）后它已无任何可覆盖的声明，属遗留空规则；
   且它需要跨到子组件根元素（ArticleListItem）上才生效，语义脆弱，故随拆分一并删除。 */

/* 标签分组的色点：绝对定位在左侧内边距里，不参与文字排版——
   若让它占据流内空间，分组头文字会被推右，与下方卡片标题失去对齐 */
.group-dot {
  position: absolute;
  left: var(--sp-2);
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--tag-c, var(--text-tertiary));
}
/* 长标签名截断，避免把右侧的条目数挤出行外 */
.group-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 条目数：等宽数字让多组之间纵向对齐，弱化一档避免与组名抢注意力 */
.group-count {
  flex-shrink: 0;
  font-weight: 400;
  font-variant-numeric: tabular-nums;
  opacity: 0.75;
}

/* 空状态：只保留「撑满剩余高度」这一条布局职责，
   居中 / 配色 / 间距 / 错误态全部交给全局 .state-block，
   与订阅栏、正文栏空态共用同一套状态表达。 */
.empty-list {
  flex: 1;
  min-height: 0;
}

/* 移动端响应式：文章列表变抽屉 */
@media (max-width: 768px) {
  .article-column {
    position: fixed;
    inset: var(--tb-h) 0 auto 0;
    width: 85%;
    max-width: 360px;
    z-index: 300;
    transform: translateX(-100%);
    transition: transform 0.25s ease;
    box-shadow: var(--shadow-panel);
  }
  .article-column.open { transform: translateX(0); }
}
</style>
