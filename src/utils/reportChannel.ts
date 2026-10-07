/**
 * 研究报告的输出通道偏好
 *
 * # 职责
 * 统一报告通道的取值域、持久化键，以及"该用哪个通道"的默认值推导。
 * 手动生成工具条与「设置 → 自动化」的报告任务表单都从这里取默认值，
 * 避免两处各写一份判断而漂移。
 *
 * # 为什么要持久化
 * 通道原先只活在组件内存里，每次打开弹窗都回到 `app`——用户明明想双写，
 * 忘了切下拉就只会落到应用数据目录。现在把选择写进 settings 表
 * （`report_channel`），下次打开弹窗、重启应用后都保持。
 *
 * # 默认值规则
 * 没有存储值（首次使用，或值是历史遗留的非法字符串）时：**已配置 Vault 路径
 * 就默认 `obsidian`**，否则退回 `app`。理由：报告归档进 Obsidian 是主线用法，
 * 应用内目录那份只是查看器的缓存；但 `obsidian` 在 Vault 未配置时会整个失败
 * （一份都不写，见后端 `generate_report`），所以没配 Vault 的用户必须退回
 * `app`，否则一按"生成报告"就报错——等于把开箱即用的功能变成不可用。
 */

/** 报告的输出通道 */
export type ReportChannel = 'app' | 'obsidian' | 'both'

/** 通道偏好的设置键（与 Rust 侧 settings 表的 key 一致） */
export const REPORT_CHANNEL_KEY = 'report_channel'

/** Obsidian Vault 路径的设置键（能否双写取决于它有没有配） */
export const VAULT_PATH_KEY = 'obsidian_vault_path'

/**
 * 判断取值是否为合法的通道枚举
 *
 * @param value - 待判断的值（可能来自未设置或历史遗留的设置项）
 * @returns 是合法通道时返回 true，并收窄类型
 */
export function isReportChannel(value: string | undefined): value is ReportChannel {
  return value === 'app' || value === 'obsidian' || value === 'both'
}

/**
 * 推导应当使用的报告通道
 *
 * @param stored - 持久化的通道偏好（可能未设置或为非法值）
 * @param vaultPath - 「设置 → Obsidian」中的 Vault 路径
 * @returns 合法偏好优先；否则按 Vault 是否配置决定是 Obsidian 还是应用内
 */
export function resolveReportChannel(
  stored: string | undefined,
  vaultPath: string | undefined
): ReportChannel {
  if (isReportChannel(stored)) return stored
  return vaultPath && vaultPath.trim() ? 'obsidian' : 'app'
}
