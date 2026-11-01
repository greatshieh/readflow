/**
 * 「打开某条判断」的待决意图通道
 *
 * # 为什么需要它
 * 命令面板检索到判断或证据时，点进去要落到「研究工作台 → 论点 → 那一条」。
 * 但面板与工作台是两个互不相识的组件：面板只认得 `open-research` 这个窗口事件，
 * 而工作台打开时**固定落在报告页签**（`ResearchModal` 的 open watch 里写死）。
 *
 * 直接在事件之后补一次"切页签 + 打开论点"是不行的：事件是同步派发，
 * 工作台的 `open` prop 要等下一次渲染才变真，那一次补完立刻被 open watch
 * 里的 `tab = 'reports'` 覆盖掉——表现是"点了论点，弹出来的是报告"。
 *
 * 所以走「先记账、再由 open watch 消费」：面板只写下 ID，工作台在自己
 * **确定要打开**的那一刻来取。这与 `reportPeriod.consumePendingOpen()`
 * 是同一个模式（同一条教训：意图必须由接收方在确定能消费的时刻取走）。
 *
 * # 为什么是模块级变量而不是 Pinia
 * 它只表达"这一次打开的落点"，没有跨组件观察者、不驱动任何渲染。
 * 放进 store 会让人以为可以 `watch` 它，而它本来就是一次性的。
 *
 * # `thesisId` 为 null 的含义
 * "只切到论点页签，不打开具体某条"。命令面板里一条还没挂到任何论点的高亮
 * 就是这种情况——把它拦下来不给点，比"点了落到论点列表"更糟：
 * 用户要找的就是那段原文，至少让他看到自己在找什么。
 *
 * # 滞留风险
 * 只在 `ResearchModal` 的 open watch 里消费，且消费即清空。
 * 若工作台最终没被打开（例如外层拦住了），这个值会留到下一次打开——
 * 那一次的落点就会变成上次点的那条判断。这是刻意的取舍：
 * 反过来的代价（提前清空）是"点了没反应"，更难解释。
 */

/** 待决意图：`thesisId` 为 null 表示只切页签 */
export interface PendingThesisOpen {
  thesisId: number | null
}

/** 当前待决意图（null = 没有） */
let pending: PendingThesisOpen | null = null

/**
 * 记下"接下来打开工作台时要落到这条判断"
 *
 * @param thesisId - 论点 ID；传 null 表示只切到论点页签
 */
export function requestThesisOpen(thesisId: number | null) {
  pending = { thesisId }
}

/**
 * 取走待决意图（取走即清空）
 *
 * @returns 待决意图；没有时返回 null
 */
export function consumeThesisOpen(): PendingThesisOpen | null {
  const current = pending
  pending = null
  return current
}
