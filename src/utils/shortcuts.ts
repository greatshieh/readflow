/**
 * 全局快捷键定义（单一真源）
 *
 * # 为什么要有这张表
 * 键位一旦从 8 个涨到 20 个，就会同时被两个地方消费：
 * 1. `App.vue` 的按键派发（哪些键要 `preventDefault` 并执行什么动作）；
 * 2. `ShortcutsModal` 的帮助面板（给用户看的一览表）。
 *
 * 若两边各抄一份，改一个键位只改一处是必然发生的漂移。故把「键位 → 说明 → 分组」
 * 的元数据收在这里，两处消费同一份数据。
 *
 * # 边界
 * 本表**只描述键位本身，不含动作实现**——动作要访问 store 实例，留在 `App.vue`。
 * 因此一张表的双向都不是强约束：新增元数据而没写动作只是"按了没反应"（可见），
 * 反之在 `App.vue` 里加了动作却漏登记则永不可触发。改动时两侧一起改即可。
 *
 * # Esc 为什么不在表内
 * Esc 退出专注模式必须在「弹窗已打开」「焦点在输入框」这些守卫**之前**判定，
 * 走不了统一查表的路径，故单独处理，只在帮助面板的底注里写明。
 */

/** 快捷键分组：帮助面板按此分区展示 */
export type ShortcutGroup = '订阅' | '文章' | '视图' | '应用'

/** 单条快捷键定义 */
export interface ShortcutDef {
  /**
   * 触发键列表：每项与 `KeyboardEvent.key.toLowerCase()` 逐字比较。
   *
   * 用数组是因为存在"多个键同一动作"的情形（数字键 1–9 都是「跳到第 N 个订阅源」），
   * 它们共享一行说明，不必在帮助面板里重复九遍。
   */
  keys: string[]
  /** 帮助面板展示的键位文本（如 `1 – 9`），与 `keys` 一一对应但更可读 */
  display: string
  /** 动作说明（用户视角的短句，不用技术名词） */
  label: string
  /** 所属分组 */
  group: ShortcutGroup
}

/**
 * 快捷键总表
 *
 * 排列顺序即帮助面板的展示顺序：先订阅（来源）、再文章（内容操作）、
 * 再视图（切换）、最后应用（应用级），符合"从大到小"的信息层级。
 */
export const SHORTCUTS: ShortcutDef[] = [
  // ---- 订阅 ----
  { keys: ['1', '2', '3', '4', '5', '6', '7', '8', '9'], display: '1 – 9', label: '跳到第 N 个订阅源', group: '订阅' },
  { keys: ['o'], display: 'O', label: '新增订阅', group: '订阅' },
  { keys: ['r'], display: 'R', label: '刷新全部订阅源', group: '订阅' },

  // ---- 文章 ----
  { keys: ['n'], display: 'N', label: '下一篇', group: '文章' },
  { keys: ['p'], display: 'P', label: '上一篇', group: '文章' },
  { keys: ['j'], display: 'J', label: '加载更多文章', group: '文章' },
  { keys: ['k'], display: 'K', label: '回到列表顶部', group: '文章' },
  { keys: ['v'], display: 'V', label: '在浏览器中打开原文', group: '文章' },
  { keys: ['e'], display: 'E', label: '导出到 Obsidian', group: '文章' },
  { keys: ['s'], display: 'S', label: '收藏 / 取消收藏', group: '文章' },
  { keys: ['m'], display: 'M', label: '全部标记为已读', group: '文章' },

  // ---- 视图 ----
  { keys: ['/'], display: '/', label: '聚焦搜索框', group: '视图' },
  { keys: ['b'], display: 'B', label: '书签视图开关', group: '视图' },
  { keys: ['u'], display: 'U', label: '未读视图开关', group: '视图' },
  { keys: ['g'], display: 'G', label: '回到全部文章', group: '视图' },
  { keys: ['f'], display: 'F', label: '专注阅读模式开关', group: '视图' },

  // ---- 应用 ----
  { keys: [','], display: ',', label: '打开设置', group: '应用' },
  { keys: ['?'], display: '?', label: '显示快捷键一览', group: '应用' },
]

/**
 * 本应用接管的按键集合
 *
 * 只有落在这个集合里的按键才会 `preventDefault()`。此前是无条件拦截所有按键，
 * 结果把 Ctrl+C / Ctrl+R / Ctrl+F 之类的系统与浏览器快捷键一并吞掉。
 */
export const SHORTCUT_KEYS: ReadonlySet<string> = new Set(
  SHORTCUTS.flatMap((s) => s.keys),
)

/** 帮助面板的分组标题顺序 */
export const SHORTCUT_GROUPS: ShortcutGroup[] = ['订阅', '文章', '视图', '应用']

/**
 * 按分组取快捷键列表
 *
 * @param group - 目标分组
 * @returns 该分组下的定义数组（保持 SHORTCUTS 内的相对顺序）
 */
export function shortcutsOfGroup(group: ShortcutGroup): ShortcutDef[] {
  return SHORTCUTS.filter((s) => s.group === group)
}
