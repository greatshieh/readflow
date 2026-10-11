/**
 * 轻量 Markdown 渲染器（零依赖）
 *
 * # 职责
 * 把研究报告 / AI 摘要等自生成 Markdown 渲染为安全 HTML 字符串，供模板 `v-html` 消费。
 *
 * # 安全模型
 * - 所有文档文本先经 `escapeHtml` 转义再拼接，绝不保留原始 HTML 标签；
 * - 仅输出本函数自己生成的标签（h1–h3 / p / ul / li / blockquote / a / code / strong / em / del / 自定义 callout 容器）；
 * - 链接 URL 经 `sanitizeUrl` 协议白名单校验（仅 http/https/mailto），非法 URL 退化为纯文本。
 * 因此 `v-html` 无需 DOMPurify 即可安全使用。
 *
 * # 支持的语法子集（对齐 ReadFlow 报告实际产出）
 * - 标题：`#` / `##` / `###`
 * - 行内：`**加粗**` / `*斜体*` / `_斜体_` / `` `代码` `` / `~~删除线~~`
 * - 链接：`[文字](url)`
 * - 双链：`[[实体]]` 降级为纯文本（应用内无双链目标）
 * - 无序列表：`-` / `*`（支持缩进嵌套）
 * - 引用块：`>`；首行 `> [!type]` 识别为 Obsidian callout，渲染成带类型徽标的卡片
 * - 段落：连续非空行，空行分隔
 */

import { stripFrontmatter } from './reportMarkdown'

/** Obsidian callout 类型 → 中文徽标文案 */
const CALLOUT_LABELS: Record<string, string> = {
  summary: '摘要',
  info: '提示',
  note: '笔记',
  tip: '建议',
  quote: '引用',
  warning: '警告',
  danger: '注意',
  success: '完成',
  question: '疑问',
  bug: '缺陷',
  example: '示例',
  abstract: '概览',
  todo: '待办',
}

/**
 * HTML 转义
 *
 * @param s - 原始文本
 * @returns 转义后的文本（& < > " ' 均被替换）
 */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/**
 * 链接协议白名单校验
 *
 * @param url - 原始 URL 字符串
 * @returns 安全则返回原串，否则返回 null（调用方降级为纯文本）
 */
function sanitizeUrl(url: string): string | null {
  const u = url.trim()
  return /^(https?:|mailto:)/i.test(u) ? u : null
}

/**
 * 行内渲染：加粗 / 斜体 / 代码 / 删除线 / 链接 / 双链
 *
 * 顺序经过权衡：先处理双链与代码（内部可能含 `*`/`~` 符号），再处理链接，
 * 最后做强调，避免强调正则误伤已生成的标签内容。
 *
 * @param text - 一段纯文本（尚未转义）
 * @returns 转义并嵌入安全标签后的 HTML 片段
 */
function renderInline(text: string): string {
  let s = escapeHtml(text)
  // 双链 [[实体]] → 纯文本（去掉外层括号）
  s = s.replace(/\[\[([^\]]+)\]\]/g, '$1')
  // 行内代码（先于链接 / 强调，避免内部符号被误处理）
  s = s.replace(/`([^`]+)`/g, (_m, code: string) => `<code>${code}</code>`)
  // 链接 [文字](url)
  s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (m, label: string, url: string) => {
    const safe = sanitizeUrl(url)
    return safe ? `<a href="${safe}" target="_blank" rel="noopener noreferrer">${label}</a>` : m
  })
  // 加粗 **x**
  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  // 删除线 ~~x~~
  s = s.replace(/~~([^~]+)~~/g, '<del>$1</del>')
  // 斜体 *x*（前后不是 *，避免与加粗冲突）
  s = s.replace(/(^|[^*])\*([^*\n]+)\*(?!\*)/g, '$1<em>$2</em>')
  // 斜体 _x_（前后不是单词字符，避免误伤 snake_case）
  s = s.replace(/(^|[^\w])_([^_\n]+)_(?!\w)/g, '$1<em>$2</em>')
  return s
}

/**
 * 取一行的前导空白宽度（制表符按 2 计，整体以空格数近似）
 *
 * @param line - 单行文本
 * @returns 缩进空格数
 */
function indentOf(line: string): number {
  const lead = line.length - line.trimStart().length
  const tabs = (line.slice(0, lead).match(/\t/g) || []).length
  return lead + tabs
}

/**
 * 解析引用块 / callout
 *
 * 从 `start` 开始收集所有连续以 `>` 开头的行；若首行匹配 `[!type]` 则按 callout
 * 渲染（类型徽标 + 标题 + 后续正文），否则按普通引用块渲染。
 *
 * @param lines - 全文按行切分
 * @param start - 引用块起始行索引
 * @returns [HTML 字符串, 下一处处理的行索引]
 */
function parseQuote(lines: string[], start: number): [string, number] {
  const buf: string[] = []
  let i = start
  while (i < lines.length && /^\s*>/.test(lines[i])) {
    buf.push(lines[i].replace(/^\s*>\s?/, ''))
    i++
  }
  const first = buf[0] ?? ''
  const cm = /^\[!(\w+)\]\s*(.*)$/.exec(first)
  if (cm) {
    const type = cm[1].toLowerCase()
    const title = cm[2].trim()
    const body = buf
      .slice(1)
      .map((l) => `<p class="md-callout-p">${renderInline(l)}</p>`)
      .join('')
    const label = CALLOUT_LABELS[type] ?? type
    const head = `<div class="md-callout-head"><span class="md-callout-badge md-callout-badge-${type}">${label}</span></div>`
    const titleHtml = title ? `<p class="md-callout-title">${renderInline(title)}</p>` : ''
    return [`<div class="md-callout md-callout-${type}">${head}${titleHtml}${body}</div>`, i]
  }
  const inner = buf.map((l) => renderInline(l)).join('<br>')
  return [`<blockquote class="md-quote">${inner}</blockquote>`, i]
}

/**
 * 解析缩进段落（列表项内、比基准缩进更深的普通文本）
 *
 * @param lines - 全文按行切分
 * @param start - 段落起始行索引
 * @param baseIndent - 基准缩进（更深才属于本段落）
 * @returns [HTML 字符串, 下一处处理的行索引]
 */
function parseIndentedParagraph(
  lines: string[],
  start: number,
  baseIndent: number,
): [string, number] {
  const buf: string[] = []
  let i = start
  while (i < lines.length) {
    const cl = lines[i]
    if (cl.trim() === '') break
    if (indentOf(cl) <= baseIndent) break
    if (/^\s*[-*]\s+/.test(cl) || /^\s*>/.test(cl)) break
    buf.push(cl.trim())
    i++
  }
  return [`<p class="md-p md-indented">${renderInline(buf.join(' '))}</p>`, i]
}

/**
 * 解析无序列表（含缩进嵌套）
 *
 * 收集与 `baseIndent` 同缩进的若干 `-`/`*` 条目；条目下方更深缩进的内容
 * （嵌套 callout / 子列表 / 缩进段落）作为该条目的子节点。
 *
 * @param lines - 全文按行切分
 * @param start - 列表起始行索引
 * @param baseIndent - 当前层级的缩进宽度
 * @returns [HTML 字符串, 下一处处理的行索引]
 */
function parseList(lines: string[], start: number, baseIndent: number): [string, number] {
  const items: string[] = []
  let i = start
  while (i < lines.length) {
    const line = lines[i]
    const m = /^(\s*)[-*]\s+(.*)$/.exec(line)
    if (!m) break
    const indent = m[1].length
    if (indent < baseIndent) break
    if (indent > baseIndent) break // 更深层由上一层作为子内容处理

    let item = `<li>${renderInline(m[2].trim())}`
    i++

    const children: string[] = []
    while (i < lines.length) {
      const cl = lines[i]
      if (cl.trim() === '') {
        // 空行：若下一行是更深子内容则跳过，否则结束本条目
        const nxt = lines[i + 1] ?? ''
        if (/^\s{2,}\S/.test(nxt)) {
          i++
          continue
        }
        break
      }
      if (indentOf(cl) > baseIndent) {
        if (/^\s*>/.test(cl)) {
          const [q, ni] = parseQuote(lines, i)
          children.push(q)
          i = ni
        } else if (/^\s*[-*]\s+/.test(cl)) {
          const [sub, ni] = parseList(lines, i, indentOf(cl))
          children.push(sub)
          i = ni
        } else {
          const [p, ni] = parseIndentedParagraph(lines, i, baseIndent)
          children.push(p)
          i = ni
        }
      } else {
        break
      }
    }
    if (children.length) item += `<div class="md-li-children">${children.join('')}</div>`
    item += '</li>'
    items.push(item)
  }
  return [`<ul class="md-ul">${items.join('')}</ul>`, i]
}

/**
 * 把 Markdown 文本渲染为安全 HTML
 *
 * 先剥离 YAML frontmatter（展示层不需要），再按行做块级解析，
 * 行内片段经 `renderInline` 处理。
 *
 * @param src - 完整 Markdown 文本
 * @returns 可直接 `v-html` 的 HTML 字符串
 */
export function renderMarkdown(src: string): string {
  const body = stripFrontmatter(src).trim()
  const lines = body.split('\n')
  const out: string[] = []
  let i = 0
  while (i < lines.length) {
    const line = lines[i]
    if (line.trim() === '') {
      i++
      continue
    }
    // 标题
    const h = /^(#{1,3})\s+(.*)$/.exec(line)
    if (h) {
      const level = h[1].length
      out.push(`<h${level} class="md-h md-h${level}">${renderInline(h[2].trim())}</h${level}>`)
      i++
      continue
    }
    // 引用块 / callout
    if (/^\s*>/.test(line)) {
      const [html, next] = parseQuote(lines, i)
      out.push(html)
      i = next
      continue
    }
    // 列表
    if (/^\s*[-*]\s+/.test(line)) {
      const [html, next] = parseList(lines, i, indentOf(line))
      out.push(html)
      i = next
      continue
    }
    // 段落：连续非空、非块起始的行
    const para: string[] = []
    while (
      i < lines.length &&
      lines[i].trim() !== '' &&
      !/^(#{1,3})\s+/.test(lines[i]) &&
      !/^\s*>/.test(lines[i]) &&
      !/^\s*[-*]\s+/.test(lines[i])
    ) {
      para.push(lines[i].trim())
      i++
    }
    if (para.length) out.push(`<p class="md-p">${renderInline(para.join(' '))}</p>`)
  }
  return out.join('\n')
}
