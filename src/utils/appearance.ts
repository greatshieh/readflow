/**
 * 外观应用工具：把设置表中的外观偏好落到 DOM
 *
 * # 职责
 * 集中维护五件"设置值 → 视觉效果"的映射，避免 App.vue（启动时）与
 * SettingsModal.vue（保存后）各自实现一份导致不一致：
 * 1. 主题（theme）：light / dark / system → html 的 data-theme 属性；
 * 2. 主题色（accent）：强调色预设 → html 的 data-accent 属性；
 * 3. 界面字体大小（ui_font_size）：档位 → 根节点 --ui-zoom 缩放变量；
 * 4. 界面字体族（ui_font）：字体族 → 根节点 --ui-font 变量（全局 UI）；
 * 5. 内容字体（content_font）：字体族 → 根节点 --reading-font 变量（仅阅读区）
 *   （阅读字号另有 font_size → --reading-scale，映射保持在此处一并处理，
 *   使"应用外观"成为一次调用生效全部的单一入口）。
 *
 * # 设计意图
 * - **单一入口**：任何修改外观设置的代码只需调用 [`applyAppearance`]，
 *   无需关心变量名与映射关系；未来新增外观维度也只改本文件。
 * - **跟随系统**：theme 为 system 时监听系统配色变化并实时切换；
 *   监听器只在主题确为 system 时挂载，切换到 light/dark 时自动移除。
 */

/** 从 settings store 读取外观设置所需的最小接口（避免整包依赖 store 类型） */
export interface AppearanceSettings {
  /** 按键读取设置值；缺失返回 undefined */
  getSetting(key: string): string | undefined
}

/**
 * 主题色（强调色）预设清单
 *
 * 取自主流产品的蓝灰系 + 暖色系色值，key 必须与 styles.css 中 `[data-accent="..."]` 的取值一致，
 * color 仅用于设置界面的色块预览（展示的是浅色主题下的主色）。
 * 新增一套只需：① 在此追加一项；② 在 styles.css 补对应的浅色 / 暗色变量组。
 */
export const ACCENT_PRESETS: ReadonlyArray<{ key: string; label: string; color: string }> = [
  { key: 'graphite', label: '石墨蓝灰', color: '#5a6b82' },
  { key: 'indigo', label: '靛青', color: '#5e6ad2' },
  { key: 'azure', label: '海蓝', color: '#1f6feb' },
  { key: 'sky', label: '天青', color: '#1d9bf0' },
  { key: 'teal', label: '青碧', color: '#128a86' },
  { key: 'amber', label: '琥珀', color: '#d97706' },
  { key: 'violet', label: '紫罗兰', color: '#7c3aed' },
  { key: 'rose', label: '玫红', color: '#db2777' },
  { key: 'emerald', label: '翠绿', color: '#059669' },
]

/** 默认强调色（设置缺失或取值非法时回落到此预设） */
const DEFAULT_ACCENT = 'graphite'

/** 界面字体档位 → 应用级 zoom 系数（作用于 .app 根容器的 CSS zoom） */
const UI_ZOOM: Record<string, string> = {
  small: '0.9',
  medium: '1',
  large: '1.1',
  xlarge: '1.2',
}

/** 阅读字号档位 → 正文/标题/摘要卡片的字号缩放系数 */
const READING_SCALE: Record<string, string> = {
  small: '0.9',
  medium: '1',
  large: '1.15',
  xlarge: '1.3',
}

/** 内容字体族预设档位 → CSS font-family 值（仿主流阅读器的衬线/无衬线选择） */
const CONTENT_FONT: Record<string, string> = {
  // 默认跟随界面字体：inherit 让阅读区继承 body 的 --ui-font，
  // 用户换界面字体时正文一并跟随，无需再单独设置
  system: 'inherit',
  // 无衬线：与 UI 一致的现代观感
  sans: "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif",
  // 衬线：长文阅读的传统书卷感（中文回退到宋体系）
  serif: "Georgia, 'Times New Roman', 'Songti SC', 'Noto Serif CJK SC', 'SimSun', serif",
}

/** 界面字体族的默认档位值（表示"用样式表内置的默认字体栈"） */
const UI_FONT_DEFAULT = 'system'

/** 界面字体缺失时的回退栈（与 styles.css 的 --font 保持一致） */
const UI_FONT_FALLBACK = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif"

/** 内容字体缺失时的回退栈（与 CONTENT_FONT.sans 保持一致） */
const CONTENT_FONT_FALLBACK =
  "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif"

/**
 * 把字体族名转成可安全拼进 font-family 的片段
 *
 * 字体名常含空格（如 `Noto Sans CJK SC`），不加引号会被 CSS 当成多个族名逐个查找；
 * 名字里的反斜杠与双引号需按 CSS 字符串转义，否则会构造出非法声明 ——
 * 而非法声明会让整个 font-family 失效、文字掉回浏览器默认字体。
 * 先转义反斜杠再转义引号，避免后者引入的 `\` 被重复转义。
 *
 * @param name - 字体族名（来自系统字体扫描结果或历史设置值）
 * @returns 带引号的族名片段
 */
function quoteFontFamily(name: string): string {
  return `"${name.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
}

/**
 * 解析字体设置值为 CSS font-family
 *
 * 设置值有两种形态：
 * - **预设 key**（如 `serif`）：查表得到固定字体栈；
 * - **系统字体族名**（如 `Noto Sans CJK SC`）：不在表中，视为具体字体名并追加回退栈，
 *   这样字体日后被卸载时不会让文字掉回浏览器默认字体。
 *
 * @param value - 设置值
 * @param presets - 预设映射表
 * @param fallback - 自定义字体名之后追加的回退栈
 * @returns 可直接写入 CSS 变量的 font-family 值；值为空时返回回退栈
 */
function resolveFontStack(
  value: string,
  presets: Record<string, string>,
  fallback: string,
): string {
  if (!value.trim()) return fallback
  const preset = presets[value]
  if (preset !== undefined) return preset
  return `${quoteFontFamily(value)}, ${fallback}`
}

/** 系统配色变化监听器（仅 theme=system 时挂载；用于卸载旧监听） */
let systemThemeQuery: MediaQueryList | null = null

/**
 * 按设置解析当前应使用的主题
 *
 * @param theme - 设置值：light / dark / system（其他值按 light 处理）
 * @returns 'light' 或 'dark'；system 时取系统当前配色
 */
function resolveTheme(theme: string): 'light' | 'dark' {
  if (theme === 'dark') return 'dark'
  if (theme === 'system') {
    // matchMedia 在非浏览器环境不可用的可能性极低（Tauri WebView 支持），兜底浅色
    return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  }
  return 'light'
}

/**
 * 把当前外观设置应用到 DOM
 *
 * 一次调用同时处理：data-theme、data-accent、--ui-zoom、--ui-font、
 * --reading-scale、--reading-font。
 * 启动时（App.vue onMounted）与设置保存后（SettingsModal）都调用它，
 * 保证两处行为完全一致。
 *
 * @param settings - 已加载的设置 store（至少支持 getSetting）
 * @returns 无返回值；副作用为写 html 的 data-theme 与 CSS 变量、维护系统配色监听
 */
export function applyAppearance(settings: AppearanceSettings): void {
  // ── 主题 ──
  const theme = settings.getSetting('theme') || 'light'
  document.documentElement.dataset.theme = resolveTheme(theme)
  // 记录"设置意图"（light/dark/system）：系统配色回调据此判断是否需要跟随变化
  document.documentElement.dataset.themeIntent = theme
  // ── 主题色（强调色预设）──
  // 只写一个 data-accent 属性，具体色值由 styles.css 中的 [data-accent] 变量组提供；
  // 非法取值回落到默认预设，避免脏数据导致整站主色失效
  const accent = settings.getSetting('accent') || DEFAULT_ACCENT
  document.documentElement.dataset.accent = ACCENT_PRESETS.some((p) => p.key === accent)
    ? accent
    : DEFAULT_ACCENT

  // theme=system 时跟随系统配色实时切换；其他主题则移除旧监听
  if (theme === 'system') {
    // 避免重复挂载：先摘掉旧监听再挂新的
    systemThemeQuery?.removeEventListener('change', onSystemThemeChange)
    systemThemeQuery = window.matchMedia('(prefers-color-scheme: dark)')
    systemThemeQuery.addEventListener('change', onSystemThemeChange)
  } else if (systemThemeQuery) {
    systemThemeQuery.removeEventListener('change', onSystemThemeChange)
    systemThemeQuery = null
  }

  // ── 界面字体大小（应用级 zoom）──
  const uiFontSize = settings.getSetting('ui_font_size') || 'medium'
  document.documentElement.style.setProperty('--ui-zoom', UI_ZOOM[uiFontSize] || '1')

  // ── 界面字体族（全局 UI）──
  // 'system' 表示"用应用内置的默认栈"：直接移除内联变量，让样式表里的
  // `--ui-font: var(--font)` 生效 —— 避免在 JS 与 CSS 两处各维护一份字体栈。
  const uiFontFamily = settings.getSetting('ui_font') || UI_FONT_DEFAULT
  if (uiFontFamily === UI_FONT_DEFAULT) {
    document.documentElement.style.removeProperty('--ui-font')
  } else {
    document.documentElement.style.setProperty(
      '--ui-font',
      resolveFontStack(uiFontFamily, {}, UI_FONT_FALLBACK),
    )
  }

  // ── 阅读字号（正文/标题/摘要卡片）──
  const fontSize = settings.getSetting('font_size') || 'medium'
  document.documentElement.style.setProperty('--reading-scale', READING_SCALE[fontSize] || '1')

  // ── 内容字体族（仅阅读区）──
  const contentFont = settings.getSetting('content_font') || 'system'
  document.documentElement.style.setProperty(
    '--reading-font',
    resolveFontStack(contentFont, CONTENT_FONT, CONTENT_FONT_FALLBACK),
  )
}

/**
 * 系统配色变化的回调（theme=system 时挂载）
 *
 * 直接读 html 当前的 data-theme 语义不够——需要按设置值重新解析，
 * 但设置值只能由 store 提供；此处从 html 上取回的 data-theme 是解析后的结果，
 * 因此改为读取本次会话缓存的设置值（挂在 dataset 上）再判断是否为 system。
 *
 * @returns 无返回值；副作用为 system 配色变化时切换 data-theme
 */
function onSystemThemeChange(): void {
  // 仅当"设置意图"是 system 时才跟随变化；意图值由 applyAppearance 写入 dataset
  if (document.documentElement.dataset.themeIntent === 'system') {
    const dark = window.matchMedia('(prefers-color-scheme: dark)').matches
    document.documentElement.dataset.theme = dark ? 'dark' : 'light'
  }
}
