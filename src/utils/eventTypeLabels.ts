/**
 * 事件类型标签与预设领域（唯一事实来源）
 *
 * # 职责
 * 把后端 `research::extract::PRESET_DOMAINS`（Rust 侧）的预设领域与事件类型枚举，
 * 在前端以「中文标签」形式镜像一份，供时间线筛选、告警规则、仪表盘等消费。
 *
 * 此前「事件类型 → 中文标签」的映射在 ResearchTimelineTab / ResearchFab /
 * EntityDashboardModal / EntityAlertRuleModal 四处各写一份，新增类型极易漏改；
 * 收敛到本文件后，配合 `useEventTypesStore` 的「生效事件类型枚举」即可数据化渲染。
 *
 * # 与后端的边界
 * - `PRESET_EVENT_TYPES` 的 key 必须与 Rust 侧 `PRESET_DOMAINS` 完全一致
 *   （模型输出的 event_type 就是这些 key，落库到 research_events.event_type）。
 * - 标签（中文）仅前端展示用；自定义类型（custom_event_types 表）由用户在设置页录入，
 *   其 label 即用户写的值（为空则回退为 value 原样展示）。
 */

/** 单个事件类型项 */
export interface EventTypeItem {
  /** 枚举值（与后端 PRESET_DOMAINS / custom_event_types.value 对齐） */
  value: string
  /** 中文标签 */
  label: string
}

/** 自定义事件类型项（来自 custom_event_types 表） */
export interface CustomEventTypeItem {
  id: number
  value: string
  label: string
}

/**
 * 预设领域 → 事件类型清单（key 与 Rust 侧 PRESET_DOMAINS 同源）
 *
 * 首版预置：商业（business）/ 科研学术（academic）/ 政策监管（policy）/ 金融财经（finance）。
 * 后续新增领域只需在此与 Rust 侧 PRESET_DOMAINS 同步扩展。
 */
export const PRESET_EVENT_TYPES: Record<string, EventTypeItem[]> = {
  business: [
    { value: 'product', label: '产品' },
    { value: 'executive', label: '高管' },
    { value: 'ma', label: '并购' },
    { value: 'regulatory', label: '监管' },
    { value: 'strategy', label: '战略' },
    { value: 'competition', label: '竞争' },
    { value: 'industry_signal', label: '行业信号' },
  ],
  academic: [
    { value: 'paper', label: '论文' },
    { value: 'grant', label: '经费' },
    { value: 'clinical', label: '临床' },
    { value: 'conference', label: '会议' },
    { value: 'hire', label: '人才' },
    { value: 'collaboration', label: '合作' },
    { value: 'patent', label: '专利' },
    { value: 'dataset', label: '数据集' },
    { value: 'funding', label: '融资' },
  ],
  policy: [
    { value: 'legislation', label: '立法' },
    { value: 'enforcement', label: '执法' },
    { value: 'consultation', label: '征求意见' },
    { value: 'guideline', label: '指南' },
  ],
  finance: [
    { value: 'earnings', label: '财报' },
    { value: 'profit_warning', label: '业绩预告' },
    { value: 'rating', label: '评级' },
    { value: 'ipo', label: '上市' },
    { value: 'dividend', label: '分红' },
    { value: 'buyback', label: '回购增持' },
    { value: 'reduce', label: '减持' },
    { value: 'restructuring', label: '重组' },
    { value: 'financing', label: '再融资' },
    { value: 'unlock', label: '解禁' },
    { value: 'violation', label: '违规退市' },
    { value: 'margin', label: '两融质押' },
    { value: 'northbound', label: '北向' },
    { value: 'dragon_tiger', label: '龙虎榜' },
  ],
}

/** 预设领域 key → 中文名（设置页领域多选展示用） */
export const PRESET_DOMAIN_LABELS: Record<string, string> = {
  business: '商业',
  academic: '科研学术',
  policy: '政策监管',
  finance: '金融财经',
}

/**
 * 合并「生效事件类型枚举」：启用预设领域的类型并集 + 自定义类型
 *
 * @param enabledDomains - 设置页启用的预设领域 key 列表（可为空）
 * @param customTypes - 自定义事件类型列表（来自 custom_event_types 表）
 * @returns 去重后的事件类型项（用于筛选下拉 / 标签渲染）
 */
export function mergeEffectiveEventTypes(
  enabledDomains: string[],
  customTypes: CustomEventTypeItem[],
): EventTypeItem[] {
  const seen = new Set<string>()
  const out: EventTypeItem[] = []
  for (const d of enabledDomains) {
    for (const it of PRESET_EVENT_TYPES[d] ?? []) {
      if (!seen.has(it.value)) {
        seen.add(it.value)
        out.push(it)
      }
    }
  }
  for (const c of customTypes) {
    if (!seen.has(c.value)) {
      seen.add(c.value)
      out.push({ value: c.value, label: c.label || c.value })
    }
  }
  return out
}

/**
 * 从生效事件类型列表中取某值的显示标签；未知类型原样返回（兼容 legacy 数据）
 *
 * @param list - 生效事件类型列表
 * @param type - 事件类型枚举值
 * @returns 中文标签或原值
 */
export function eventTypeLabelFrom(list: EventTypeItem[], type: string): string {
  return list.find((i) => i.value === type)?.label ?? type
}
