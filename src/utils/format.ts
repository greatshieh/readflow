/**
 * 展示层格式化工具
 *
 * # 职责
 * 提供纯函数式的展示格式化能力：相对时间、完整日期时间、文本截断。
 * 对外导出 `formatDistance` / `formatFullDate` / `truncate`，
 * 主要被 `components/ArticleColumn.vue`（文章列表）与 `components/ContentColumn.vue`（正文）等界面消费。
 *
 * # 设计意图
 * - **纯函数、无副作用**：不依赖 Pinia、不触后端、不读全局状态，
 *   因此可以安全地在模板、computed 中被高频调用。
 * - **本地化写死为 zh-CN**：ReadFlow 面向中文阅读场景，文案与日期格式统一使用
 *   中文习惯（"3 分钟前"、"2024/01/15 14:30"），不随浏览器语言漂移。
 * - **只做"必要的数据处理"**：这些转换属于展示层必需，
 *   符合"前端只做展示 + 必要数据处理"的架构规则，不涉及任何业务数据获取。
 *
 * # 注意事项
 * - 后端时间统一以 UTC（ISO 8601 字符串）下发，`new Date(dateStr)` 会按运行机器的
 *   本地时区解析，因此展示结果为本地时间，这是预期行为。
 * - `formatDistance` 以 `now - date` 计算差值：若传入未来时间会得到负数，
 *   从而落到"刚刚"分支，属于既有实现的边界行为。
 */

/**
 * 将时间格式化为相对时间描述（如"3 分钟前"）
 *
 * 采用"相对时间 + 超过一周退化为绝对日期"的分级策略：阅读器场景中用户最关心
 * "最近有没有更新"，相对时间比完整日期更易扫读；但超过一周后精确到天反而冗余，
 * 故直接退回本地化的短日期。
 *
 * @param dateStr - ISO 8601 时间字符串（来自后端，UTC）
 * @returns 相对时间文案；超过 7 天返回 zh-CN 的本地日期字符串。
 *          传入非法时间时 `new Date()` 得到 Invalid Date，比较结果恒为 false，
 *          最终落到 `toLocaleDateString` 分支返回 "Invalid Date"
 */
export function formatDistance(dateStr: string): string {
  const date = new Date(dateStr)
  const now = new Date()
  const diff = now.getTime() - date.getTime()
  const minutes = Math.floor(diff / (1000 * 60))
  const hours = Math.floor(diff / (1000 * 60 * 60))
  const days = Math.floor(diff / (1000 * 60 * 60 * 24))

  if (minutes < 1) return '刚刚'
  if (minutes < 60) return `${minutes} 分钟前`
  if (hours < 24) return `${hours} 小时前`
  if (days < 7) return `${days} 天前`
  return date.toLocaleDateString('zh-CN')
}

/**
 * 将时间格式化为完整的本地日期时间（如"2024/01/15 14:30"）
 *
 * 用于文章详情页等需要精确时间的场景；与 `formatDistance` 互补：
 * 列表用相对时间扫读，详情用完整时间溯源。
 *
 * @param dateStr - ISO 8601 时间字符串（来自后端，UTC）
 * @returns zh-CN 本地化、月/日/时/分补零到两位的日期时间字符串
 */
export function formatFullDate(dateStr: string): string {
  return new Date(dateStr).toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  })
}

/**
 * 按字符长度截断文本并追加省略号
 *
 * 按 `String.length`（UTF-16 码元）而非"视觉宽度"或"词边界"截断：
 * 列表摘要只需防止溢出，精确的分词处理会带来不必要的复杂度；
 * 中英文混排时中文每字 1 个码元，实际宽度差异可接受。
 *
 * @param text - 待截断的原始文本
 * @param length - 允许保留的最大字符数（不含省略号）
 * @returns 未超长时原样返回；超长时返回前 `length` 个字符 + '...'，
 *          因此结果长度可能为 `length + 3`
 */
export function truncate(text: string, length: number): string {
  if (text.length <= length) return text
  return text.slice(0, length) + '...'
}
