/**
 * 文章列表的分组规则（纯函数工具）
 *
 * # 职责
 * 把文章数组切成「带标题的组」，供列表栏按组渲染。两种形态：
 * - `groupByDate`：Folo 式时间线分组（今天 / 昨天 / 本周 / 本月 / 更早）；
 * - `groupByTag`：把标签当收藏夹分组（书签视图的默认形态），组顺序依标签名称序。
 *
 * # 为什么抽出来
 * 这两段规则承载了较多口径约定（多标签文章只归「最靠前」的一组、目录里没有的标签排最后、
 * 「未分类」恒置底、非时间序不分组），它们与组件渲染无关。抽成纯函数后
 * `ArticleColumn.vue` 只需回答「什么时候用哪种分组」，规则本身可独立演进与复用。
 */

import type { Article } from '@/types'
import { tagColorVar } from '@/stores/tags'

/**
 * 列表分组
 *
 * `label` 为空串表示该组不渲染分组头（非时间序排序时的单组形态）；
 * `tone` 有值时在分组头前画一个该色圆点（仅标签分组有颜色可言）。
 */
export interface ArticleGroup {
  label: string
  items: Article[]
  tone?: string
}

/**
 * 按发布时间把文章切成时间线分组
 *
 * 依赖后端按 `published_at` 倒序返回：一旦分组标签交错出现（今天 / 更早 / 今天），
 * 分组头就不再表达「时间远近」，所以调用方必须只在时间序下使用本函数。
 * 时间戳无法解析的文章归入「更早」——宁可归到最旧一档，也不要凭空造出一个无意义的组。
 *
 * @param list - 已按发布时间倒序排好的文章
 * @returns 分组数组，顺序即 今天 → 昨天 → 本周 → 本月 → 更早（只含非空组）
 */
export function groupByDate(list: Article[]): ArticleGroup[] {
  const now = new Date()
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const DAY = 24 * 60 * 60 * 1000
  const groups: ArticleGroup[] = []
  const indexByLabel = new Map<string, number>()
  for (const a of list) {
    const t = new Date(a.published_at).getTime()
    let label = '更早'
    if (!isNaN(t)) {
      if (t >= startOfToday) label = '今天'
      else if (t >= startOfToday - DAY) label = '昨天'
      else if (t >= startOfToday - 6 * DAY) label = '本周'
      else if (t >= startOfToday - 29 * DAY) label = '本月'
    }
    let idx = indexByLabel.get(label)
    if (idx === undefined) {
      idx = groups.push({ label, items: [] }) - 1
      indexByLabel.set(label, idx)
    }
    groups[idx].items.push(a)
  }
  return groups
}

/**
 * 把文章按标签分组（书签视图的「收藏夹」）
 *
 * 归组规则（与用户确认过的口径一致）：
 * - 多标签文章只出现在**最靠前**的那个标签组，不在多个组里重复出现；
 *   "最靠前"用的是与组顺序同一套排序，避免出现"文章落在它所属却排得更后面的一组"。
 * - 没有标签的文章归入末尾的「未分类」组，该组恒置底——它是兜底桶而非真正的收藏夹。
 * - 目录里没有的标签（目录未加载 / 标签刚在别处被删）排到最后，名称与颜色取自文章自身的标签对象。
 * - 只输出非空组，因此不会出现点开就空的收藏夹。
 *
 * @param list - 当前视图下的文章（书签视图下已由后端按 `is_bookmarked` 过滤）
 * @param rank - 标签 ID → 名称序位置（由标签目录构造，见 `ArticleColumn` 的 `tagRank`）
 * @returns 分组数组，按标签名称序排列，「未分类」在末尾
 */
export function groupByTag(list: Article[], rank: Map<number, number>): ArticleGroup[] {
  const order = (id: number) => rank.get(id) ?? Number.MAX_SAFE_INTEGER
  const buckets = new Map<number, ArticleGroup>()
  const untagged: Article[] = []

  for (const a of list) {
    const tags = a.tags ?? []
    if (!tags.length) {
      untagged.push(a)
      continue
    }
    let pick = tags[0]
    for (const t of tags) {
      if (order(t.id) < order(pick.id)) pick = t
    }
    let g = buckets.get(pick.id)
    if (!g) {
      g = { label: pick.name, items: [], tone: tagColorVar(pick.color) }
      buckets.set(pick.id, g)
    }
    g.items.push(a)
  }

  const sorted = [...buckets.entries()]
    .sort((x, y) => order(x[0]) - order(y[0]))
    .map(([, g]) => g)
  if (untagged.length) sorted.push({ label: '未分类', items: untagged })
  return sorted
}
