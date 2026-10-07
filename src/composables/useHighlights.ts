/**
 * 正文高亮 composable：选区捕获、标记渲染与高亮增删
 *
 * 从 ContentColumn 抽出的高亮逻辑整体。正文是 v-html 注入的 HTML，高亮片段本身
 * 只存「文本 + 颜色」（见 types.Highlight），因此渲染方式是：把正文的全部文本节点
 * 拼成一段纯文本，找到片段位置后按节点切分并包成 <mark data-highlight-id>。这样
 * 不依赖 HTML 结构，也能覆盖「选中跨越 <strong> / <a> 等内联标签」的情况
 * （拼接后的文本里依然连续）。
 *
 * 与宿主的分工：两块与 DOM 事件强耦合的代码留在 ContentColumn——
 * - 正文点击委托里的删除分发（高亮块 → removeHighlight，与链接 / 灯箱共用事件委托）；
 * - 切换文章 watcher 里的调色板复位与选区快照丢弃。
 */
import { nextTick, ref, watch, type Ref } from 'vue'
import { useArticlesStore } from '@/stores/articles'
import { useModalStore } from '@/stores/modal'
import type { Highlight } from '@/types'

/**
 * 高亮可选颜色
 *
 * `key` 会写进 `highlights.color` 并回传给后端保存，`label` 只用于悬浮提示；
 * 实际底色由 `styles.css` 的 `--hl-*` 变量按主题给出，这里不写颜色值。
 */
export const HIGHLIGHT_COLORS: { key: string; label: string }[] = [
  { key: 'yellow', label: '黄色高亮' },
  { key: 'green', label: '绿色高亮' },
  { key: 'blue', label: '蓝色高亮' },
  { key: 'pink', label: '粉色高亮' }
]

/** 单条高亮最多保存的字符数（防止把整篇文章存成一条高亮） */
const MAX_HIGHLIGHT_CHARS = 2000

/** 待高亮的选区快照：纯文本 + 在正文纯文本坐标系里的起止偏移 */
interface PendingSelection {
  text: string
  start: number
  end: number
}

/**
 * 取元素内的纯文本（按文档顺序拼接全部文本节点）
 *
 * 与 `innerText` 的区别：不受 CSS 显示 / 空白折叠影响，切分文本节点时能与
 * 渲染阶段用同一套坐标系，避免「算出的偏移和实际位置对不上」。
 *
 * @param root - 容器元素
 * @returns 拼接后的纯文本
 */
function plainTextOf(root: HTMLElement): string {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  let out = ''
  let node: Node | null
  while ((node = walker.nextNode())) {
    out += (node as Text).data
  }
  return out
}

/**
 * 把一个高亮片段包成 <mark>
 *
 * @param root - 正文容器
 * @param highlight - 待渲染的高亮片段
 * @returns 成功标记了至少一个文本节点时返回 true
 */
function wrapHighlight(root: HTMLElement, highlight: Highlight): boolean {
  const needle = highlight.text.trim()
  if (!needle) return false

  // 先收集全部文本节点及其在纯文本中的区间，再一次性定位
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
  const segments: { node: Text; start: number }[] = []
  let cursor = 0
  let full = ''
  let node: Node | null
  while ((node = walker.nextNode())) {
    const text = node as Text
    segments.push({ node: text, start: cursor })
    full += text.data
    cursor += text.data.length
  }

  const at = full.indexOf(needle)
  if (at < 0) return false
  const from = at
  const to = at + needle.length

  let marked = false
  for (const segment of segments) {
    const segStart = segment.start
    const segEnd = segment.start + segment.node.data.length
    const localStart = Math.max(segStart, from) - segStart
    const localEnd = Math.min(segEnd, to) - segStart
    if (localStart >= localEnd) continue

    const range = document.createRange()
    range.setStart(segment.node, localStart)
    range.setEnd(segment.node, localEnd)
    const mark = document.createElement('mark')
    mark.dataset.highlightId = String(highlight.id)
    mark.dataset.hlColor = highlight.color
    mark.title = '点击删除该高亮'
    try {
      range.surroundContents(mark)
      marked = true
    } catch {
      // 极端结构（如片段边界正好落在标签中间）下 surroundContents 会抛错，
      // 跳过这一段即可：宁可少标一段，也不该让整篇正文渲染失败
    }
  }
  return marked
}

/**
 * 创建正文高亮 composable
 *
 * @param options.contentBodyRef - 正文容器（v-html 渲染出的 DOM），标记直接写进去
 * @param options.displayContent - 当前显示的正文 HTML（原文或译文），变化后重画标记
 * @returns 调色板状态、选区快照与增删处理器（详见各成员注释）
 */
export function useHighlights(options: {
  contentBodyRef: Ref<HTMLElement | null>
  displayContent: Ref<string>
}) {
  const { contentBodyRef, displayContent } = options
  const articlesStore = useArticlesStore()
  const modalStore = useModalStore()

  /** 高亮调色板显示态（正文里选中文字后浮出） */
  const showHighlightPalette = ref(false)

  /**
   * 待高亮的选区快照
   *
   * 必须在 `mouseup` 时就记下来：点调色板按钮会先触发一次 mousedown，
   * 浏览器默认会在那时折叠文档选区，等到 click 再读 `window.getSelection()`
   * 往往已经空了。快照同时存下纯文本偏移量，与渲染阶段用同一套坐标系。
   */
  const pendingSelection = ref<PendingSelection | null>(null)

  /**
   * 把当前文章的全部高亮画进正文
   *
   * 每次重画前先拆掉上一轮的 mark（高亮被删除、切换原文/译文后正文都会重建），
   * 否则会残留指向已删除记录的黄色块，点它还会去删一条不存在的记录。
   *
   * @returns 无返回值；直接操作正文容器的 DOM
   */
  function renderHighlightMarks() {
    const root = contentBodyRef.value
    if (!root) return

    root.querySelectorAll('mark[data-highlight-id]').forEach((el) => {
      const parent = el.parentNode
      if (!parent) return
      while (el.firstChild) parent.insertBefore(el.firstChild, el)
      parent.removeChild(el)
      // 合并被切碎的文本节点，否则反复重画会让节点数越积越多
      parent.normalize()
    })

    for (const highlight of articlesStore.highlights) {
      wrapHighlight(root, highlight)
    }
  }

  /**
   * 正文内松开鼠标：若形成了有效选区则浮出调色板
   *
   * 只接受完全落在正文容器内的选区——在标题、工具条或摘要卡上划选也会触发
   * `mouseup`，不加这层判断会让调色板在最奇怪的地方冒出来。
   *
   * @returns 无返回值；副作用为切换 `showHighlightPalette`
   */
  function onContentMouseUp() {
    const root = contentBodyRef.value
    const selection = window.getSelection()
    if (!root || !selection || selection.rangeCount === 0) return

    // 点一下（没拖选）也会触发 mouseup：此时收起调色板，避免它挂着一份已经失效的选区
    const raw = selection.toString().trim()
    if (!raw) {
      showHighlightPalette.value = false
      pendingSelection.value = null
      return
    }
    const range = selection.getRangeAt(0)
    if (!root.contains(range.commonAncestorContainer)) {
      showHighlightPalette.value = false
      pendingSelection.value = null
      return
    }

    // 截断超长片段（避免把整页塞进一条高亮）；偏移量取「片段在正文纯文本中的位置」，
    // 文本因 HTML 结构被拆分时可能定位不到，此时退化为 0（仅影响排序，不影响渲染）
    const text = raw.length > MAX_HIGHLIGHT_CHARS ? raw.slice(0, MAX_HIGHLIGHT_CHARS) : raw
    const at = plainTextOf(root).indexOf(text)
    pendingSelection.value = {
      text,
      start: at >= 0 ? at : 0,
      end: at >= 0 ? at + text.length : 0
    }
    showHighlightPalette.value = true
  }

  /**
   * 创建高亮：写入快照里的选中片段
   *
   * @param color - 颜色（从调色板传入，取值见 {@link HIGHLIGHT_COLORS}）
   * @returns 无返回值；成功后高亮立即出现在正文上（渲染由 watch 触发）
   */
  async function createHighlight(color: string) {
    const article = articlesStore.selectedArticle
    const pending = pendingSelection.value
    showHighlightPalette.value = false
    pendingSelection.value = null
    if (!article || !pending) return

    await articlesStore.createHighlight(
      article.id,
      pending.text,
      '',
      color,
      pending.start,
      pending.end
    )

    // 收起选区：留着高亮色块与蓝色选区叠在一起会看不清效果
    window.getSelection()?.removeAllRanges()
  }

  /**
   * 取消高亮：收起调色板并丢弃选区快照
   *
   * @returns 无返回值；副作用为清掉 `pendingSelection`
   */
  function cancelHighlight() {
    showHighlightPalette.value = false
    pendingSelection.value = null
  }

  /**
   * 删除一条高亮（点击正文里的高亮块触发）
   *
   * @param id - 高亮 ID
   * @returns 无返回值；用户取消时不做任何改动
   */
  async function removeHighlight(id: number) {
    const ok = await modalStore.showConfirm({
      title: '删除高亮',
      message: '确定删除这条高亮吗？此操作不可撤销。'
    })
    if (!ok) return
    await articlesStore.deleteHighlight(id)
    // 立即重画：不等 watch 的下一个 tick，避免被删的色块还留在屏幕上
    nextTick(renderHighlightMarks)
  }

  /** 正文变化（换文章、切原文/译文）或高亮列表变化后重画标记 */
  watch(
    [displayContent, () => articlesStore.highlights],
    () => {
      nextTick(renderHighlightMarks)
    }
  )

  return {
    showHighlightPalette,
    pendingSelection,
    onContentMouseUp,
    createHighlight,
    cancelHighlight,
    removeHighlight
  }
}
