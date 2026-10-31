/**
 * 报告正文里的引用编号 `[n]` → 可点按钮
 *
 * # 为什么不直接让渲染器输出按钮
 * 正文来自 `utils/markdown.ts`，而同一份 Markdown 还要被导出成 md / html。
 * 若在渲染阶段就把 `[n]` 变成 `<button>`，导出的 HTML 里会留下一堆点不动的按钮，
 * 导出的 md 里则完全没有编号可读。所以：
 * - Markdown 里保持纯文本 `[n]`（导出后就是可读编号，Obsidian 里也不会出现死链）；
 * - **只有应用内阅读区**在渲染完成后，对文本节点做一次原地替换。
 *
 * # 为什么遍历文本节点而不是正则替换 innerHTML
 * `innerHTML` 正则替换会误伤标签属性（例如 `title="[1]"`、`href` 里的方括号），
 * 也会把已有 `<a>` 链接的文字改掉。文本节点遍历天然避开这些位置。
 */

/** 一条被切分出来的片段 */
interface RefSegment {
  /** `text` = 普通文本；`ref` = 引用编号 */
  type: 'text' | 'ref'
  /** 文本内容，或引用编号的字符串形式 */
  value: string
}

/** 引用编号：方括号内 1–3 位数字（`[12]` 合法，`[abc]` 不匹配） */
const REF_PATTERN = /\[(\d{1,3})\]/g

/**
 * 把一段纯文本按 `[n]` 切成片段
 *
 * 与 DOM 无关的纯函数，便于单测覆盖边界（连续编号、行首编号、无编号）。
 *
 * @param text - 原始文本
 * @returns 片段数组；没有任何编号时返回单个 `text` 片段
 */
function splitRefText(text: string): RefSegment[] {
  const out: RefSegment[] = []
  let last = 0
  for (const m of text.matchAll(REF_PATTERN)) {
    const idx = m.index ?? 0
    if (idx > last) out.push({ type: 'text', value: text.slice(last, idx) })
    out.push({ type: 'ref', value: m[1] })
    last = idx + m[0].length
  }
  if (last < text.length) out.push({ type: 'text', value: text.slice(last) })
  return out
}

/** 不处理的宿主元素：链接内、代码内、已有按钮内的方括号不是引用编号 */
const SKIP_SELECTOR = 'a, code, pre, button'

/**
 * 把容器内所有 `[n]` 文本替换成可点按钮
 *
 * 幂等：第二次调用不会再匹配到任何 `[n]`（已被换成按钮），因此重复渲染安全。
 *
 * @param root - 渲染后的正文容器
 * @returns 替换出的按钮数量
 */
export function markRefs(root: HTMLElement): number {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const targets: Text[] = []
  let node = walker.nextNode()
  while (node) {
    const text = node as Text
    if (text.data.includes('[') && !text.parentElement?.closest(SKIP_SELECTOR)) {
      targets.push(text)
    }
    node = walker.nextNode()
  }

  let count = 0
  for (const text of targets) {
    const segments = splitRefText(text.data)
    if (!segments.some(s => s.type === 'ref')) continue
    const frag = document.createDocumentFragment()
    for (const seg of segments) {
      if (seg.type === 'text') {
        frag.appendChild(document.createTextNode(seg.value))
      } else {
        const btn = document.createElement('button')
        btn.type = 'button'
        btn.className = 'rc-ref'
        btn.dataset.ref = seg.value
        btn.textContent = seg.value
        btn.setAttribute('aria-label', `引用源 ${seg.value}`)
        frag.appendChild(btn)
        count += 1
      }
    }
    text.replaceWith(frag)
  }
  return count
}
