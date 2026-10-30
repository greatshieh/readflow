/**
 * 单文件 HTML 导出
 *
 * # 职责
 * 把"Markdown 渲染出的正文 HTML" + "报告正文 CSS" 拼成一个可以脱离应用直接打开的
 * 独立 HTML 文件，并把 CSS 里的 `var(--x)` 全部换成**导出瞬间的实际取值**。
 *
 * # 为什么必须展开变量
 * 导出的文件不在应用里跑，`:root` 上没有任何自定义属性，`color: var(--text-primary)`
 * 会整条声明作废 —— 白底黑字变成浏览器默认的"无样式页面"。
 *
 * # 为什么是"补 :root 定义"而不是"正则替换 var()"
 * 两者都能让颜色出来，但替换会破坏带兜底的写法：`var(--warn, #b45309)` 里的
 * 逗号与 fallback 不是变量名的一部分，正则一换就把整条声明写坏。
 * 补 `:root` 定义则是零侵入：CSS 原文一个字不改，只是让它能解析出值。
 *
 * # 主题一致性
 * 取值来自 `getComputedStyle(document.documentElement)`，即**导出那一刻界面的真实配色**
 * （明暗 + 语义色），所以导出的文件与用户看到的报告同色。这比 `prefers-color-scheme`
 * 猜系统主题准确 —— "应用里选了亮色、系统是暗色"是最常见的错配。
 */

/** 变量引用：只匹配变量名，不碰 fallback 部分 */
const VAR_REF = /var\(\s*(--[A-Za-z0-9_-]+)/g

/**
 * 从 CSS 原文里提取全部被引用的变量名（去重）
 *
 * @param css - CSS 原文
 * @returns 变量名数组（含前导 `--`）
 */
export function collectVarNames(css: string): string[] {
  const out = new Set<string>()
  for (const m of css.matchAll(VAR_REF)) out.add(m[1])
  return [...out]
}

/**
 * 把一条变量值里的嵌套 `var()` 就地展开
 *
 * 例如 `--hover-bg: var(--fill)` —— 直接写进 `:root` 也能被浏览器解析，
 * 但一旦导出文件被粘贴到别处（只带走部分规则）就会断链。
 * 这里最多展开 10 层，防御环形引用。
 *
 * @param name - 变量名
 * @param table - 变量名 → 原始值（来自 getComputedStyle）
 * @returns 展开后的值；取不到时返回空串
 */
function resolveValue(name: string, table: Map<string, string>): string {
  let value = table.get(name) ?? ''
  for (let i = 0; i < 10 && value.includes('var('); i += 1) {
    value = value.replace(/var\(\s*(--[A-Za-z0-9_-]+)\s*(?:,\s*([^)]*))?\)/g, (_m, ref: string, fb?: string) => {
      const next = table.get(ref)
      if (next && next.trim() !== '') return next
      return fb ?? ''
    })
  }
  return value.trim()
}

/**
 * 生成写进导出文件的 `:root { ... }` 声明块
 *
 * @param names - 需要定义的变量名
 * @returns CSS 声明块；一个变量都取不到时返回空串
 */
export function buildRootVars(names: string[]): string {
  const style = getComputedStyle(document.documentElement)
  const table = new Map<string, string>()
  for (const name of names) table.set(name, style.getPropertyValue(name))

  const decls: string[] = []
  for (const name of names) {
    const value = resolveValue(name, table)
    // 空值不写：写 `--x: ;` 会让依赖它的声明在导出文件里变成非法值
    if (value) decls.push(`${name}: ${value};`)
  }
  return decls.length ? `:root {\n  ${decls.join('\n  ')}\n}` : ''
}

/**
 * 导出页的外壳样式
 *
 * 独立于报告正文：正文那份 `.rc` 样式由调用方传入（与应用内同一份），
 * 外壳只负责页面复位与阅读栏宽。**它也会参与变量收集**——
 * `--bg` 只出现在这里，漏收集会让暗色主题导出成"浅底 + 亮字"，整篇读不了。
 */
const SHELL_CSS = `* { box-sizing: border-box; }
body {
  margin: 0;
  padding: 32px 20px 64px;
  background: var(--bg, #f5f9ff);
  color: var(--text-primary, #191c22);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC",
    "Microsoft YaHei", "Hiragino Sans GB", sans-serif;
}
.wrap { max-width: 820px; margin: 0 auto; }
@media print {
  body { padding: 0; background: #fff; }
  .wrap { max-width: none; }
}`

/**
 * 构建导出用的独立 HTML
 *
 * @param title - 文档标题（浏览器标签页与打印页眉）
 * @param bodyHtml - 正文 HTML（来自 `renderMarkdown`，已转义、仅含安全标签）
 * @param reportCss - 报告正文 CSS 原文（通常是 `@/styles/report-content.css?raw`）
 * @returns 完整的 HTML 文档字符串
 */
export function buildExportHtml(title: string, bodyHtml: string, reportCss: string): string {
  // 两份 CSS 一起收集：只收正文那份会漏掉外壳用到的 `--bg`
  const rootVars = buildRootVars(collectVarNames(reportCss + SHELL_CSS))
  const safeTitle = title.replace(/[<>&]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;' })[c] ?? c)
  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${safeTitle}</title>
<style>
${rootVars}
/* ── 页面外壳：导出文件独立于应用，这里补最小复位与阅读栏宽 ── */
${SHELL_CSS}
/* 报告正文版式：与应用内阅读区**同一份 CSS**，只换了容器名 */
${reportCss}
</style>
</head>
<body>
<div class="wrap"><article class="rc">${bodyHtml}</article></div>
</body>
</html>`
}
