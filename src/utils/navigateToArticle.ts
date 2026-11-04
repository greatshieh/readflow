/**
 * 「打开某篇文章」的完整编排：把左两列一起带到这篇文章所在的位置
 *
 * # 职责
 * 从**任意入口**（研究工作台的报告引用 / 时间线来源 / 论点证据、实体仪表盘、
 * 命令面板）打开一篇文章时，只写 `selectedArticle` 是不够的：中栏列表还停在
 * 原来的范围里，目标条目根本不在其中，"高亮到第几条"无从谈起；左栏也不会
 * 高亮它所属的订阅源。本模块把"切范围 → 打开 → 等条目 → 定位"收成一条路径，
 * 各入口只传一个文章 id。
 *
 * # 步骤顺序是有约束的
 * 1. **先切范围**：标签视图优先级高于订阅源（见 `ArticleColumn` 的 `scopeKey`），
 *    不清标签则 `selectFeed` 不生效、列表不重载。
 * 2. **筛选切回全部**：`selectArticle` 内部会立即标已读；若当前是「仅未读」视图，
 *    这篇文章一打开就从列表里消失——滚到了也留不住。书签 / 喜欢 / 跳过同理。
 * 3. **先出正文，再做定位**：`selectArticle` 早于等条目，正文立即可读；
 *    定位是锦上添花，不该让用户盯着空正文等 1 秒。
 * 4. **最后派发 `reveal-in-list`**：两列各自把条目滚进视野并播一次定位脉冲。
 *    事件必须最后发——早于选中态更新的话，宿主量到的还是旧位置。
 *
 * # 为什么不在这里直接操作 DOM
 * 列表与订阅栏的滚动容器、折叠分组、胶囊定位都是各组件私有的实现细节。
 * 本模块只表达"要定位到这条"，怎么滚由各自决定。
 */
import { nextTick } from 'vue'
import { useArticlesStore } from '@/stores/articles'
import { useFeedsStore } from '@/stores/feeds'

/** 等某个条目出现在 DOM 里的上限（毫秒） */
const ITEM_WAIT_MS = 1200

/** 目标不在首页时，最多再向后翻几页（每页 50 条） */
const MAX_EXTRA_PAGES = 3

/**
 * 轮询等待某个条目真正出现在 DOM 里
 *
 * 切范围会触发列表异步重载（走 IPC），`nextTick` 早于数据返回，此刻量位置
 * 只会拿到空列表。等它真正出现再定位，胶囊才不会滑到顶部。
 *
 * @param selector - 形如 `.article-item[data-article-id="12"]`
 * @param timeoutMs - 最长等待；超时返回 false，调用方自行降级
 * @returns 在超时前出现即 true
 */
export async function waitForItem(selector: string, timeoutMs = ITEM_WAIT_MS): Promise<boolean> {
  const deadline = performance.now() + timeoutMs
  while (performance.now() < deadline) {
    if (document.querySelector(selector)) return true
    await new Promise<void>((r) => requestAnimationFrame(() => r()))
  }
  return false
}

/**
 * 确认目标文章已在中栏列表里，必要时向后翻页
 *
 * 报告引用的常常是**历史文章**，它可能散落在几千条之后。只等首页会出现
 * "文章在，但列表里搜不到它"——用户点完仍看不到定位。这里按页补齐，
 * 命中即停，最多 {@link MAX_EXTRA_PAGES} 页。
 *
 * @param articleId - 目标文章 id
 * @param feedId - 当前范围对应的订阅源 id（`null` 表示全部）
 * @returns 最终是否在列表中找到该条目
 */
async function ensureListed(articleId: number, feedId: number | null): Promise<boolean> {
  const articlesStore = useArticlesStore()
  const selector = `.article-item[data-article-id="${articleId}"]`
  // 首页先给足时间：切源后的重载是异步的，此时翻页会与首次加载抢 `loading`
  if (await waitForItem(selector, ITEM_WAIT_MS)) return true
  for (let page = 0; page < MAX_EXTRA_PAGES; page++) {
    if (!articlesStore.hasMore) return false
    await articlesStore.loadMoreArticles(feedId)
    if (await waitForItem(selector, 300)) return true
  }
  return false
}

/**
 * 打开一篇文章，并把左两列定位到它
 *
 * @param articleId - 目标文章 id；非正数直接返回（历史上 0 表示"无"）
 * @returns 无返回值；副作用为切范围、写选中态、必要时翻页，并派发 `reveal-in-list`
 */
export async function navigateToArticle(articleId: number): Promise<void> {
  if (articleId <= 0) return
  const articlesStore = useArticlesStore()
  const feedsStore = useFeedsStore()

  // 文章可能不在当前列表 / 分页内（研究台引用的多是历史文章），故不能只查 displayArticles
  const article =
    articlesStore.articles.find((a) => a.id === articleId) ??
    (await articlesStore.loadArticle(articleId))
  if (!article) {
    console.error('定位文章失败：文章不存在或已被清理, id =', articleId)
    return
  }

  // ① 标签视图优先级高于源：不清标签，切源不会让列表重载
  const hadTag = articlesStore.tagFilterId !== null
  if (hadTag) articlesStore.setTagFilter(null)

  // ② 筛选若会把这篇排除，就切回全部（未读 / 书签 / 喜欢 / 跳过）
  const hadFilter = articlesStore.viewFilter !== 'all'
  if (hadFilter) articlesStore.setViewFilter('all')

  // ③ 切源。同源且筛选也没变时不重载，列表本来就是对的
  const target = feedsStore.feeds.find((f) => f.id === article.feed_id) ?? null
  const switchedFeed = target !== null && feedsStore.selectedFeed?.id !== article.feed_id
  if (switchedFeed && target) {
    feedsStore.selectFeed(target)
  } else if (!hadTag && hadFilter) {
    // 范围键（标签 + 源）没变、只有筛选变了：组件里的 scopeKey watch 不会触发，
    // 得自己补一次重载，否则列表仍是旧筛选下的那一页，目标条目可能不在其中。
    // 范围键变过时不要补——那两条 watch 已经会重载，补了就是白白多打一次后端。
    await articlesStore.loadArticles(feedsStore.selectedFeed?.id ?? null)
  }

  // ④ 先出正文：定位失败不该让用户对着空正文等待
  await articlesStore.selectArticle(article.id)

  // ⑤ 再定位：等条目出现（必要时翻页），然后让两列自己滚
  await ensureListed(article.id, feedsStore.selectedFeed?.id ?? null)

  // ⑥ 等一次 DOM 刷新：中栏可能在别的视图下刚被挂载（graph / recommend 时它是卸载的），
  //    否则本事件会派发给不存在的监听者
  await nextTick()
  window.dispatchEvent(
    new CustomEvent('reveal-in-list', { detail: { feedId: article.feed_id, articleId: article.id } }),
  )
}
