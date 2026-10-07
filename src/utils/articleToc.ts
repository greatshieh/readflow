/**
 * 正文目录（TOC）条目的提取
 *
 * 职责：把**已经渲染进 DOM** 的正文，转成一份「可跳转的目录条目」数组。
 *
 * 为什么从 DOM 取，而不是解析 `displayContent` 字符串：
 * 1. **自动跟随「原文 / 译文」切换**——显示的是哪一份正文，就取哪一份的标题；
 * 2. 拿到的是**真正要滚动的那个元素**，不必生成 `id` 再 `querySelector`，
 *    也就不会与正文里已有的 `id` 撞车；
 * 3. 标题文本已被 `v-html` 解析过，`textContent` 天然不含标签。
 *
 * 只收录 `h1`–`h3`：对真实订阅库抽样（1189 篇非空正文）后确认，`h4` 绝大多数
 * 是代码块说明（`Code block 1: …`）、`h6` 是空标签，收进来只会把目录刷屏；
 * 而 `h1`–`h3` 才是真章节，且常带分层编号（`Method 2` → `1.2 Loading the model`）。
 *
 * 覆盖面提醒：本项目**不做网页全文抓取**，正文只来自 RSS 自带的
 * `content` / `summary`。实测仅约 6% 的文章具备 ≥2 个可用标题，因此调用方必须
 * 按「条目不足 `MIN_TOC_ITEMS` 条就不显示目录入口」处理，否则绝大多数文章
 * 会点开一个空目录。
 */

/**
 * 构成一个「目录」所需的最少条目数
 *
 * 只有一条标题时，目录与正文顶部的标题是重复信息，不值得占用一个工具条位置。
 */
export const MIN_TOC_ITEMS = 2

/** 一个目录条目 */
export interface TocItem {
  /** 原始标题层级：1 / 2 / 3，用于决定缩进 */
  level: number
  /** 归并空白后的标题文本 */
  text: string
  /** 正文中对应的标题元素，点击跳转直接滚到它 */
  el: HTMLElement
}

/**
 * 归并空白
 *
 * RSS 正文里的标题常带换行与多余空格（尤其缩进过的 HTML），
 * 不归并会让目录里出现断成两行的条目。
 *
 * @param s - 原始字符串
 * @returns 空白折叠为单空格并去首尾的结果
 */
function normalizeText(s: string): string {
  return s.replace(/\s+/g, ' ').trim()
}

/**
 * 判断某个标题是否只是文章标题的复述
 *
 * RSS 正文常把文章标题原样再放一个 `h1`，收进目录就是第一条与顶上标题重复。
 * 只在**文本足够长**时做包含判断：短标题（如「结论」）被文章标题包含属于正常，
 * 误删会丢掉真实章节。
 *
 * @param text - 已归并空白、转小写的标题文本
 * @param title - 已归并空白、转小写的文章标题
 * @returns true 表示该标题应被跳过
 */
function isTitleEcho(text: string, title: string): boolean {
  if (!title) return false
  if (text === title) return true
  if (text.length < 8) return false
  return title.includes(text) || text.includes(title)
}

/**
 * 从正文容器中提取目录条目
 *
 * @param root - 正文容器（`v-html` 渲染出的那个元素）；为空时返回空数组
 * @param articleTitle - 当前文章标题，用于剔除正文里复述标题的首个标题
 * @returns 按文档顺序排列的目录条目；无可用标题时为空数组
 */
export function collectTocItems(root: HTMLElement | null, articleTitle: string): TocItem[] {
  if (!root) return []
  const title = normalizeText(articleTitle).toLowerCase()
  const items: TocItem[] = []

  for (const el of Array.from(root.querySelectorAll<HTMLElement>('h1, h2, h3'))) {
    const text = normalizeText(el.textContent ?? '')
    // 空标题（实测有源会输出空 h6 / 只放图片的 h3）不该出现在目录里
    if (!text) continue
    if (items.length === 0 && isTitleEcho(text.toLowerCase(), title)) continue
    items.push({ level: Number(el.tagName.charAt(1)), text, el })
  }

  return items
}
