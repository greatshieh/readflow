/**
 * AI 流式请求标识
 *
 * # 职责
 * 为每一次会产出 `ai-stream` 增量的调用生成一个进程内唯一标识。后端把该标识原样
 * 回传在事件载荷的 `requestId` 字段里，前端据此**丢弃上一轮请求迟到的事件**——
 * 连点两次「重新生成」时，若不比对标识，新旧文本会在界面上交替闪烁。
 *
 * # 设计意图
 * - **独立成模块**：摘要 / 翻译（`stores/articles.ts`）与文章追问
 *   （`stores/articleChat.ts`）两条链路都需要它。此前只有前者持有实现，
 *   后者若各写一份，日后修 bug 必然只修一处、另一处悄悄漂移。
 * - **不需要密码学强度**：它只用于进程内区分并发请求，不上线、不落库、不参与鉴权，
 *   因此优先用 `crypto.randomUUID`；老内核缺该 API 时退化为时间戳 + 随机后缀，
 *   不为这一点小事引入依赖。
 */

/**
 * 生成一个进程内唯一的请求标识
 *
 * @returns 新的请求标识；环境支持时为 UUID，否则为「时间戳-随机串」
 */
export function newRequestId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`
}
