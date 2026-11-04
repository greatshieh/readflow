/**
 * 来源快照的文案组装（纯函数）
 *
 * # 职责
 * 把一条「从哪来」记录翻译成上下文栏要显示的三段文字（锚点 / 类别词 / 可隐藏描述）、
 * 读屏整句，以及失效态那一句话。
 *
 * # 为什么单独抽成纯函数
 * 这段文案是整个往返流程里**最容易被后来人"顺手优化"坏**的部分：
 * 「把类别词拼进锚点更好看」「描述为空时留个占位符」看上去都无害，但前者会让
 * 窄栏里的收缩顺序失效（见下），后者会在栏上留下一个悬空的分隔符。
 * 规则写进函数体与注释里，比写在设计文档里活得久。
 *
 * # 三段的分工（决定窄栏时谁先让位）
 * - `kind`：类别词（`日报` / `事件` / `论点`）。**系统枚举，长度有界**，样式上永不收缩、
 *   永不截断——它是"回到哪一类东西"的最后一道信息，窄到只剩两三个字时留下的就是它；
 * - `anchor`：坐标（期号跨度 `10.26 – 11.01` / 日期 `2026-10-14` / 实体名）。可收缩、
 *   可 ellipsis：被压缩时**看得见省略号**，用户知道"这里还有字"；
 * - `desc`：用户写的东西（AI 标题 / 论点陈述 / 事件事实），长度无界，窄栏时整段隐藏
 *   （`display: none`，不是 ellipsis 到 0——后者会留一段悬空的 gap）。
 *
 * ⚠ 类别词**不许**折回 `anchor`。折回去的后果不是"多几个字"，而是窄栏把
 * 「10月30日 日报」压成「10月30日 日」——而 `.cb-text` 只有 `overflow: hidden`、
 * 没有省略号，用户看不到自己被裁过。`reportLabel` 的自测里有一条正则断言守这件事。
 */
import { compactPeriodSpan } from '@/stores/reportPeriod'
import { periodKindLabel, type PeriodKind } from '@/utils/periodWindow'
import type { OriginKind, OriginLabel, OriginRecord, OriginStale } from '@/stores/originTrail'

/** 组装论点类文案所需的最小字段集（`ThesisDetail` 直接传自己那一条论点即可） */
export interface ThesisLabelInput {
  id: number
  entityName: string
  statement: string
}

/** 组装报告类文案所需的最小字段集（`PeriodSummary` 的前三个字段即可） */
export interface ReportLabelInput {
  kind: string
  periodStart: string
  title: string
}

/** 目的地名词：读屏里"回到什么"，与类别词一起构成完整坐标 */
const DEST_NAME: Record<OriginKind, string> = {
  report: '报告',
  timeline: '事件时间线',
  thesis: '论点',
  coverage: '覆盖',
  entity: '实体',
}

/** 失效原因 → 栏上那一句话（主语一律是目的地，句式一律「已不存在」） */
const STALE_TEXT: Record<OriginStale, string> = {
  'report-missing': '这份报告已不存在',
  'thesis-missing': '这条论点已不存在',
  'article-missing': '这篇原文已被清理',
  'event-missing': '这条事件已不存在',
}

/**
 * 时间线事件的锚点与描述
 *
 * 锚点取 `event_date` 的日期部分：后端存的是 ISO 串，可能带时间与时区，
 * 一律截到 10 字符（`YYYY-MM-DD`）才是定宽。日期缺失时锚点留空串，
 * 由模板的 `v-if` 把整个 `.cb-anchor` 去掉——**不要**塞「日期未知」这类占位：
 * 那会让「事件」旁边永远跟着一段假坐标，而它本来就没有坐标。
 *
 * @param ev - 带上下文的事件行（`entity_name` / `fact` 由后端 JOIN 好）
 * @returns 三段文案（`kind` 恒为「事件」）
 */
export function timelineLabel(ev: {
  event_date: string
  entity_name: string
  fact: string
}): OriginLabel {
  const entity = (ev.entity_name || '').trim()
  const fact = (ev.fact || '').trim()
  return {
    anchor: (ev.event_date || '').slice(0, 10),
    kind: '事件',
    // 实体名是分组的锚（时间线按实体分组），放在事实前面读起来才是"谁的什么事"；
    // 两者任一为空时只留另一个，**不留分隔符**——`·` 的存在必须由两段都非空兜底
    desc: entity && fact ? `${entity} · ${fact}` : fact || entity,
  }
}

/**
 * 论点的锚点与描述
 *
 * 锚点用实体名而不是「10-14」这类日期：论点是被反复回看的判断，
 * 用户记住的是"关于哪家公司的那条"，不是"哪一天写的"。
 *
 * @param thesis - 论点（只需实体名与陈述，详情页直接传 `detail.thesis`）
 * @returns 三段文案（`kind` 恒为「论点」）；实体名为空时锚点为空串，
 *          不退化填「论点」——那会和 `kind` 重复成「论点 论点」
 */
export function thesisLabel(thesis: Pick<ThesisLabelInput, 'entityName' | 'statement'>): OriginLabel {
  return {
    anchor: (thesis.entityName || '').trim(),
    kind: '论点',
    desc: (thesis.statement || '').trim(),
  }
}

/**
 * 报告的锚点与描述
 *
 * 锚点是**只有跨度**的期号（`10.26 – 11.01`），类别词单独走 `kind`——
 * 理由见文件头：窄栏必须能先弃跨度、保住「周报」这两个字。
 * 跨度由 `compactPeriodSpan` 给出（不在本文件对整段期号做字符串切割：
 * 切片对格式变化太脆，改一次分隔符就静默失效且没有编译错误兜底）。
 *
 * @param summary - 报告的粒度 / 起点 / AI 标题
 * @returns 三段文案；`desc` 是标题全文（截断 / 隐藏交给 CSS，`title` 属性里要能看全）
 */
export function reportLabel(summary: ReportLabelInput): OriginLabel {
  // 后端 `kind` 是字符串：先收口一次，保证锚点的日期写法与类别词说的是同一粒度
  const grain: PeriodKind =
    summary.kind === 'weekly' ? 'weekly' : summary.kind === 'monthly' ? 'monthly' : 'daily'
  return {
    anchor: compactPeriodSpan(grain, summary.periodStart),
    kind: periodKindLabel(grain),
    desc: summary.title || '',
  }
}

/**
 * 主按钮的读屏文案
 *
 * 视觉上只有 `←` + 锚点 + 类别词，读屏必须补出"回到什么"与完整描述——
 * 描述在界面上可能已被隐藏或截断，这里给的是全文。
 *
 * @param record - 来源记录（只用 `kind` 与 `label`）
 * @returns 例如「回到 报告：10月30日 日报，宁德时代三季度毛利率回升」；
 *          锚点为空时不留多余空格
 */
export function originAriaLabel(record: Pick<OriginRecord, 'kind' | 'label'>): string {
  const { anchor, kind: word, desc } = record.label
  const head = anchor ? `${anchor} ${word}` : word
  return desc
    ? `回到 ${DEST_NAME[record.kind]}：${head}，${desc}`
    : `回到 ${DEST_NAME[record.kind]}：${head}`
}

/**
 * 失效原因 → 栏上那句话
 *
 * 栏组件与 toast 共用同一份文案：报告路径失败时两处会同时出现（栏转失效态 + toast 说明），
 * 各写一份就会出现"栏说已不存在、toast 说已被删除"这种自相矛盾的提示。
 *
 * @param reason - 失效原因
 * @returns 面向用户的一句话
 */
export function originStaleText(reason: OriginStale): string {
  return STALE_TEXT[reason]
}
