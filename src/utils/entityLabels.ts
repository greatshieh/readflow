/**
 * 实体类型标签与配色类名映射（唯一事实来源）
 *
 * # 职责
 * 把后端 `entity_type`（自由字符串）映射成中文标签与 CSS 类后缀。
 * 此前同一份映射在 EntityGraph（图节点标签 / 图例）与 ResearchEntitiesTab
 * （实体管理列表 / 新建实体下拉）各写一份，新增类型时极易漏改一处，故收敛到 utils。
 *
 * # 边界
 * ResearchTimelineTab 的 `TYPE_LABELS` 是「事件类型」（product_launch /
 * executive_change / …），与这里的「实体类型」是两个不同概念，刻意不合并。
 */

/** 实体类型 → 中文标签 */
export const ENTITY_TYPE_LABELS: Record<string, string> = {
  company: '公司',
  person: '人物',
  product: '产品',
  industry: '行业',
  other: '其他'
}

/** 关系图图例：颜色 → 实体类型（与 ENTITY_TYPE_LABELS 同源，组件里不再抄一份） */
export const ENTITY_LEGEND = [
  { key: 'company', label: '公司' },
  { key: 'person', label: '人物' },
  { key: 'product', label: '产品' },
  { key: 'industry', label: '行业' },
  { key: 'other', label: '其他' }
]

/**
 * 实体类型 → 已归一化的 CSS 类后缀
 *
 * 未知类型回退 `other`：后端 `entity_type` 是自由字符串，若直接拼进类名
 * 会出现无样式的透明节点（比"颜色不对"更难排查）。
 *
 * @param type - 后端返回的实体类型
 * @returns 类后缀
 */
export function entityTypeClass(type: string): string {
  return Object.prototype.hasOwnProperty.call(ENTITY_TYPE_LABELS, type) ? type : 'other'
}

/**
 * 实体类型 → 中文标签
 *
 * @param type - 后端返回的实体类型
 * @returns 中文标签（未知类型原样返回）
 */
export function entityTypeLabel(type: string): string {
  return ENTITY_TYPE_LABELS[type] ?? type
}
