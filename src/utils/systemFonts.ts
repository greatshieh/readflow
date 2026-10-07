/**
 * 系统字体列表加载工具：为「界面字体 / 内容字体」下拉提供本机字体选项
 *
 * # 职责
 * 调用后端 `system_fonts_list` 命令枚举本机已安装字体，并转换成下拉需要的
 * `{ value, label }` 选项数组。
 *
 * # 设计意图
 * - **懒加载 + 模块级缓存**：字体扫描需要遍历磁盘上数百个字体文件，属于典型的
 *   "只该做一次"的工作。这里把 Promise 缓存在模块作用域，界面字体与内容字体两个
 *   下拉共享同一次请求；重复打开设置页也不会重新扫描。
 * - **失败不抛错**：枚举失败（或本机无字体目录）时返回空数组，下拉退化为只显示
 *   内置预设 —— 拿不到系统字体不该让整个设置页报错。
 */

/** 下拉选项结构（与 AppSelect 的 SelectOption 同形，此处独立声明以保持本文件不依赖组件类型） */
export interface FontOption {
  /** 选项值：CSS `font-family` 用的字体族名 */
  value: string
  /** 选项展示名：中文名与英文名不同时形如「微软雅黑（Microsoft YaHei）」 */
  label: string
}

/** 后端返回的字体族结构（与 Rust 端 `fonts::FontFamily` 对应） */
interface SystemFont {
  /** CSS `font-family` 用名（优先英文名） */
  name: string
  /** 展示名 */
  label: string
}

/** 已缓存的加载 Promise（null 表示尚未发起过请求） */
let cached: Promise<FontOption[]> | null = null

/**
 * 加载系统字体选项列表
 *
 * @returns 字体族选项数组；后端调用失败时返回空数组（不会 reject）
 */
export function loadSystemFonts(): Promise<FontOption[]> {
  if (!cached) {
    cached = (async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const fonts = await invoke<SystemFont[]>('system_fonts_list')
        return fonts.map((font) => ({ value: font.name, label: font.label }))
      } catch (e) {
        console.error('加载系统字体列表失败:', e)
        return []
      }
    })()
  }
  return cached
}
