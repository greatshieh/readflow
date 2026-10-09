<template>
  <!-- 根容器：专注模式下追加 focus-mode 类，隐藏订阅栏与文章列表栏（见 styles.css） -->
  <div class="app" :class="{ 'focus-mode': uiStore.focusMode }">
    <!-- 顶部窗口栏：fixed 定位横跨全宽（含最小化/最大化/关闭、主题切换、设置入口） -->
    <TitleBar />
    <!-- 三栏主体：订阅栏 | 文章列表栏 | 正文栏，真正并列，各自顶部有独立操作条（对齐 Folo） -->
    <div class="app-body" :style="{ '--feed-w': uiStore.feedColWidth + 'px', '--article-w': uiStore.articleColWidth + 'px' }">
      <!-- 左：订阅栏（FeedPanel，含"全部文章"入口与添加/刷新） -->
      <FeedPanel />
      <!-- 订阅栏 ↔ 右侧内容区 之间的可拖拽分隔条（对齐 Folo 的 PanelSplitter）；
           关系图视图下订阅栏右侧不再分列，分隔条随之隐藏 -->
      <div v-if="uiStore.mainView === 'articles'" class="col-resizer" :class="{ active: resizing === 'feed' }" @mousedown="startResize('feed', $event)" title="拖拽调整订阅栏宽度"></div>
      <!-- 主屏内容二选一：文章阅读（中栏列表 + 右栏正文）或实体关系图（全宽）。
           互斥由 uiStore.mainView 驱动，切换入口在研究工作台图页签与侧栏「更多」菜单 -->
      <GraphView v-if="uiStore.mainView === 'graph'" />
      <template v-else>
        <!-- 中：文章列表栏（ArticleColumn，顶部固定操作条：当前源 / 搜索 / 仅未读 / 书签 / 刷新当前源 / 全部已读） -->
        <ArticleColumn />
        <!-- 文章列表栏 ↔ 正文栏 之间的可拖拽分隔条 -->
        <div class="col-resizer" :class="{ active: resizing === 'article' }" @mousedown="startResize('article', $event)" title="拖拽调整文章列表栏宽度"></div>
        <!-- 右：正文栏（ContentColumn，顶部工具条：标记已读未读 / 收藏 / 译文 / 专注 / 导出 / 更多菜单） -->
        <ContentColumn />
      </template>
    </div>
    <!-- 设置模态：由 window 事件 `open-settings` 控制显隐 -->
    <SettingsModal :open="showSettings" @close="showSettings = false" />
    <!-- 快捷键一览：由 `?` 键或命令面板底栏入口唤起，内容来自 utils/shortcuts.ts -->
    <ShortcutsModal :open="showShortcuts" @close="showShortcuts = false" />
    <!-- 命令面板：Ctrl/⌘K 唤起，挂在最外层以覆盖全部界面（含主屏关系图视图） -->
    <CommandPalette />
    <!-- 全局弹窗：confirm / prompt / alert 统一由 GlobalModal 渲染 -->
    <GlobalModal />
  </div>
</template>

<script setup lang="ts">
/**
 * 应用根组件
 *
 * # 职责
 * 负责整体"三栏并列"骨架布局，并承担应用级的一次性初始化：
 * - 顶部：TitleBar（自绘窗口栏）；
 * - 主体 `.app-body`：FeedPanel（订阅栏）| ArticleColumn（文章列表栏）| ContentColumn（正文栏），
 *   三列真正并列，各自顶部有独立操作条（对齐 Folo 的 SubscriptionColumn / Timeline / EntryContent）。
 * - 全局：SettingsModal（设置入口，由 TitleBar 齿轮按钮经 `open-settings` 事件唤起）。
 *
 * # 与 Pinia store 的关系
 * - 持有 `useFeedsStore()`，在 mounted 时调用一次 loadFeeds() 完成订阅源首次拉取；
 * - 持有 `useSettingsStore()`，在 mounted 时调用 loadSettings() 把 settings 表全量缓存到内存；
 * - 文章列表与正文数据分别由 ArticleColumn / ContentColumn 各自按需加载，根组件不再预取文章。
 */

import { ref, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useFeedsStore } from '@/stores/feeds'
import { useArticlesStore } from '@/stores/articles'
import { useSettingsStore } from '@/stores/settings'
import { useUiStore } from '@/stores/ui'
import { useModalStore } from '@/stores/modal'
import { applyAppearance } from '@/utils/appearance'
import { anyDialogOpen } from '@/utils/dialogStack'
import { SHORTCUT_KEYS } from '@/utils/shortcuts'
import FeedPanel from './components/FeedPanel.vue'
import TitleBar from './components/TitleBar.vue'
import ArticleColumn from './components/ArticleColumn.vue'
import ContentColumn from './components/ContentColumn.vue'
import GraphView from './components/GraphView.vue'
import SettingsModal from './components/SettingsModal.vue'
import ShortcutsModal from './components/ShortcutsModal.vue'
import GlobalModal from './components/GlobalModal.vue'
import CommandPalette from './components/CommandPalette.vue'

/** 订阅源 store 实例：供 mounted 触发首次加载 */
const feedsStore = useFeedsStore()
/** 设置 store 实例：供 mounted 预加载，供设置模态与 AI 面板读取 */
const settingsStore = useSettingsStore()
/** UI 状态 store 实例：专注阅读模式开关，用于联动隐藏订阅栏与文章列表栏 */
const uiStore = useUiStore()
/**
 * 文章 store 实例：全局快捷键要操作文章列表与选中文章
 *
 * 此前该文件直接引用了未声明的 `articlesStore`（既没 import 也没实例化），
 * 按任一快捷键都会抛 ReferenceError，快捷键实际处于完全不可用状态。
 */
const articlesStore = useArticlesStore()
/** 全局弹窗 store 实例：快捷键需要判断当前是否有弹窗抢占交互 */
const modalStore = useModalStore()

/** 设置模态显隐状态，由 `open-settings` 窗口事件置为 true */
const showSettings = ref(false)
/** 快捷键一览模态显隐状态，由 `?` 键或命令面板底栏入口置为 true */
const showShortcuts = ref(false)

/**
 * 打开设置模态的回调（绑定到 window 的 `open-settings` 事件，由 TitleBar 齿轮按钮派发）
 * @returns 无返回值；副作用为把 showSettings 置为 true
 */
function handleOpenSettings() {
  showSettings.value = true
}

/**
 * 打开快捷键一览模态的回调（由命令面板底栏入口经 `open-shortcuts` 事件派发）
 *
 * 与 `?` 键是同一件事的两个入口：键盘入口快但难发现，命令面板入口可被点到。
 *
 * @returns 无返回值；副作用为把 showShortcuts 置为 true
 */
function handleOpenShortcuts() {
  showShortcuts.value = true
}

/**
 * 派发「打开添加订阅弹窗」意图
 *
 * 弹窗本体挂在 `FeedPanel` 里、由它的局部状态 `showAdd` 控制，本组件够不着。
 * 走 window 事件而非把开关提升到 store：与既有的 `open-settings`（TitleBar 派发、
 * 本组件接收）完全同构，只是这里方向反过来。一个瞬时开关进全局 store 语义偏重。
 *
 * @returns 无返回值；副作用为派发 `open-add-feed` 事件
 */
function openAddFeed() {
  window.dispatchEvent(new CustomEvent('open-add-feed'))
}

/**
 * 加载更多文章（按当前列表长度继续向后取）
 *
 * @returns 无返回值
 */
function loadMoreArticles() {
  void articlesStore.loadArticles(
    feedsStore.selectedFeed?.id ?? null,
    20,
    articlesStore.articles.length
  )
}

/**
 * 回到列表顶部并重新加载第一页
 *
 * @returns 无返回值
 */
function reloadArticlesFromTop() {
  void articlesStore.loadArticles(feedsStore.selectedFeed?.id ?? null, 20, 0)
}

/**
 * 在系统浏览器中打开当前文章的原文
 *
 * @returns 无返回值；未选中文章时静默
 */
function openInBrowser() {
  const a = articlesStore.selectedArticle
  if (a) void invoke('open_external', { url: a.link })
}

/**
 * 把当前文章导出到 Obsidian
 *
 * @returns 无返回值；未选中文章时静默
 */
function exportToObsidian() {
  const a = articlesStore.selectedArticle
  if (a) void invoke('article_export_obsidian', { articleId: a.id })
}

/**
 * 切换当前文章的收藏状态
 *
 * @returns 无返回值；未选中文章时静默
 */
function toggleBookmarkCurrent() {
  const a = articlesStore.selectedArticle
  if (a) void articlesStore.toggleBookmark(a.id)
}

/**
 * 标记当前范围全部已读
 *
 * @returns 无返回值
 */
function markAllReadCurrent() {
  void articlesStore.markAllRead(feedsStore.selectedFeed?.id ?? null)
}

/**
 * 聚焦文章列表顶部的搜索框
 *
 * 选择器 `.tb-search input` 由 ArticleListToolbar 提供并被注释锁定，不可改名。
 *
 * @returns 无返回值
 */
function focusSearchBox() {
  document.querySelector<HTMLInputElement>('.tb-search input')?.focus()
}

/** 切换书签视图（`b` 的对称动作见 toggleUnreadView） */
function toggleBookmarkedView() {
  articlesStore.setViewFilter(
    articlesStore.viewFilter === 'bookmarked' ? 'all' : 'bookmarked'
  )
}

/** 切换未读视图（ViewFilter 现含 'unread'，与书签视图同为正反开关） */
function toggleUnreadView() {
  articlesStore.setViewFilter(articlesStore.viewFilter === 'unread' ? 'all' : 'unread')
}

/**
 * 回到全部文章视图
 *
 * 直接置空选中态即可：store 内部对 `selectedFeed` 的读写本就是 ref 语义。
 *
 * @returns 无返回值
 */
function gotoAllArticles() {
  feedsStore.selectedFeed = null
  void articlesStore.loadArticles(null, 20, 0)
}

/**
 * 跳到第 N 个订阅源（数字键 1–9）
 *
 * 序号按 `feedsStore.feeds` 的原始顺序，不做本地重排——它也是 store 的权威顺序。
 *
 * @param n - 1 起的序号
 * @returns 无返回值；越界时静默
 */
function selectFeedByIndex(n: number) {
  const target = feedsStore.feeds[n - 1]
  if (!target) return
  feedsStore.selectFeed(target)
}

/**
 * 把选中文章在列表里移动若干格（n / p 键）
 *
 * # 为什么走 displayArticles
 * 视图可能处在未读 / 书签 / 搜索过滤下，`articles` 是未经筛选的原始列表，
 * 在其中前后移动会让跳出来的文章与用户看到的列表不一致。
 *
 * # 边界策略
 * - 向上越界、向下且确无更多时**静默**，不循环回另一端——循环会导致连续按 n
 *   绕回第一篇重复标记，属于误读；
 * - 向下走到当前列表末尾但 `hasMore` 为真时先加载更多，加载成功再前进一格。
 *
 * # 已读标记无需本函数关心
 * `selectArticle` 内部已组合了 markAsRead，移动即自动落已读。
 *
 * @param delta - 位移：1 向下，-1 向上
 * @returns 无返回值
 */
async function moveSelection(delta: number) {
  const before = articlesStore.displayArticles
  if (before.length === 0) return

  const id = articlesStore.selectedArticle?.id
  const i = id === undefined ? -1 : before.findIndex((a) => a.id === id)

  if (delta < 0) {
    // 未选中任何文章时按 p 无处可去；已在首篇同样静默
    if (i <= 0) return
    await articlesStore.selectArticle(before[i - 1].id)
    return
  }

  if (i < 0) {
    // 还没选中：跳到当前列表的第一篇
    await articlesStore.selectArticle(before[0].id)
    return
  }
  if (i + 1 < before.length) {
    await articlesStore.selectArticle(before[i + 1].id)
    return
  }

  // 已在末尾：还有更多就先加载。加载后新列表的前半段仍是原来那些，
  // 故直接取新列表的第 before.length 项即原来末位的下一篇。
  if (!articlesStore.hasMore) return
  await articlesStore.loadMoreArticles(feedsStore.selectedFeed?.id ?? null)
  const grown = articlesStore.displayArticles
  if (grown.length > before.length) {
    await articlesStore.selectArticle(grown[before.length].id)
  }
}

/**
 * 快捷键动作注册表
 *
 * 键为单个字符，与 `KeyboardEvent.key.toLowerCase()` 对齐；值为对应动作。
 *
 * # 数字键为何在下方循环注入
 * 1–9 是同一个动作的九个变体，逐个手写既冗余也容易漏掉某个数字。
 *
 * # 与 utils/shortcuts.ts 的分工
 * 拦截集合取自那里的元数据而非本表的键：元数据是用户可见的"说明"，
 * 漏登元数据只会得到"按了没反应"这种**可见**的失败；反过来若以本表为准，
 * 漏登就会变成"能触发却不见说明"的隐形漂移。
 *
 * @see {@link file://./utils/shortcuts.ts} 展示用的键位说明
 */
const ACTIONS: Record<string, () => void> = {
  // ---- 订阅 ----
  o: openAddFeed,
  r: () => void feedsStore.refreshFeeds(),
  // ---- 文章 ----
  n: () => void moveSelection(1),
  p: () => void moveSelection(-1),
  j: loadMoreArticles,
  k: reloadArticlesFromTop,
  v: openInBrowser,
  e: exportToObsidian,
  s: toggleBookmarkCurrent,
  m: markAllReadCurrent,
  // ---- 视图 ----
  '/': focusSearchBox,
  b: toggleBookmarkedView,
  u: toggleUnreadView,
  g: gotoAllArticles,
  f: () => uiStore.toggleFocusMode(),
  // ---- 应用 ----
  ',': () => {
    showSettings.value = true
  },
  '?': () => {
    showShortcuts.value = true
  },
}
// 数字键 1–9：跳到第 N 个订阅源
for (let n = 1; n <= 9; n += 1) {
  ACTIONS[String(n)] = () => selectFeedByIndex(n)
}

/**
 * 全局键盘快捷键处理
 *
 * # 响应条件（须全部满足）
 * 1. 未按下 Ctrl / Cmd / Alt —— 组合键一律留给系统与浏览器；
 * 2. 未按下 Shift（除 `?` 外）—— 否则 Shift+N 会被整体 `toLowerCase` 折成 `n` 而误触发；
 * 3. 事件目标不是输入类元素（input / textarea / contenteditable）—— 避免打字被当成命令；
 * 4. 没有弹窗打开 —— 否则弹窗里按 `m` 会在用户看不见列表的情况下触发"全部已读"。
 *
 * Esc 退出专注模式的判断放在最前，因为它不涉及修饰键冲突，且应当在任何情况下可用。
 *
 * # 弹窗判定为何读全局计数
 * `anyDialogOpen`（`utils/dialogStack`）由所有 `BaseModal` 弹窗在开合时登记，计数器
 * 覆盖了 RulesModal / DigestModal / ResearchModal / SettingsModal / FeedManageModal
 * 以及 `GlobalModal`（confirm / prompt / alert，BaseModal 的 `global` 变体）等全部模态弹窗；
 * `modalStore.activeModal` 与计数条件并列判断，作为语义上更直接的"全局确认框开着"表达。
 *
 * # 为何不再用 switch
 * 键位增至 20 个后 switch 会膨胀到 60 行以上。改为「守卫 + 查表」后本函数只负责
 * "该不该响应"，具体做什么由 `ACTIONS` 决定，新增键位是加一行而非加一个分支。
 *
 * @param e - 键盘事件
 * @returns 无返回值；命中接管范围内的按键时阻止默认行为并执行对应动作
 */
function handleKeydown(e: KeyboardEvent) {
  // Esc 退出专注模式
  if (e.key === 'Escape' && uiStore.focusMode) {
    uiStore.setFocusMode(false)
    return
  }
  // 组合键直接放行（不拦截、也不阻止默认行为）
  if (e.metaKey || e.ctrlKey || e.altKey) return

  const key = e.key.toLowerCase()

  // Shift + 字母 / 数字同样放行：不带这条，Shift+N 会被上面的 toLowerCase 折成 'n'
  // 从而穿透守卫。'?' 是唯一例外——绝大多数键盘布局只能靠 Shift 打出它。
  if (e.shiftKey && key !== '?') return

  // 输入框 / 文本框内不响应快捷键，避免冲突
  const target = e.target as HTMLElement | null
  const tag = target?.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || target?.isContentEditable) return
  // 弹窗打开时不响应：避免误触"全部已读"这类不可逆操作
  if (anyDialogOpen.value || modalStore.activeModal) return

  if (!SHORTCUT_KEYS.has(key)) return
  const action = ACTIONS[key]
  if (!action) return
  // 仅对真正接管的按键阻止默认行为（如 Firefox 下 "/" 是快速查找）
  e.preventDefault()
  action()
}

/**
 * 应用挂载后加载订阅源与设置
 * @returns 无返回值（Promise 在两路加载完成后 resolve）
 * @副作用 触发 feedsStore.loadFeeds() 与 settingsStore.loadSettings()、回填图标、应用外观
 */
/**
 * 当前正在拖拽的列分隔条标识，用于高亮激活态（'feed' | 'article' | null）
 */
const resizing = ref<null | 'feed' | 'article'>(null)
/**
 * 拖拽起始信息
 *
 * `zoom` 是拖拽开始瞬间 `.app` 的 CSS zoom 系数（「界面字体大小」设置驱动）：
 * clientX 增量按视觉像素计，而列宽是未缩放的布局像素，二者相差 zoom 倍，
 * 必须在拖拽开始时快照一次（拖拽中途缩放不会变，逐帧 getComputedStyle 纯浪费）。
 */
let resizeStart: {
  key: 'feed' | 'article'
  startX: number
  startW: number
  zoom: number
} | null = null

/**
 * 按下分隔条开始拖拽：记录起始位置并挂载全局鼠标监听
 * @param key - 被调整宽度的栏：'feed' = 订阅栏，'article' = 文章列表栏
 * @param e - 鼠标按下事件
 */
function startResize(key: 'feed' | 'article', e: MouseEvent) {
  // preventDefault：阻断 WebView 在拖拽起点触发的原生文本选择，避免光标闪变成 I-beam
  e.preventDefault()
  const appEl = document.querySelector('.app')
  resizeStart = {
    key,
    startX: e.clientX,
    startW: key === 'feed' ? uiStore.feedColWidth : uiStore.articleColWidth,
    zoom: Number(appEl ? getComputedStyle(appEl).zoom : 1) || 1,
  }
  resizing.value = key
  document.body.style.cursor = 'col-resize'
  document.body.style.userSelect = 'none'
  window.addEventListener('mousemove', onResizeMove)
  window.addEventListener('mouseup', onResizeEnd)
}

/**
 * 拖拽过程中实时计算新列宽并写入 store（带最小/最大约束，避免栏过窄或过宽）
 * @param e - 鼠标移动事件
 */
function onResizeMove(e: MouseEvent) {
  if (!resizeStart) return
  // clientX 是视觉像素，除以 zoom 换算回布局像素（zoom=1 时为恒等变换）
  const delta = (e.clientX - resizeStart.startX) / resizeStart.zoom
  const min = resizeStart.key === 'feed' ? 200 : 280
  const max = resizeStart.key === 'feed' ? 460 : 620
  const w = Math.min(max, Math.max(min, resizeStart.startW + delta))
  if (resizeStart.key === 'feed') uiStore.setFeedColWidth(w)
  else uiStore.setArticleColWidth(w)
}

/** 拖拽结束：卸载全局监听并恢复光标与文本选择，落定最终列宽 */
function onResizeEnd() {
  resizeStart = null
  resizing.value = null
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
  window.removeEventListener('mousemove', onResizeMove)
  window.removeEventListener('mouseup', onResizeEnd)
}

onMounted(async () => {
  window.addEventListener('open-settings', handleOpenSettings)
  window.addEventListener('open-shortcuts', handleOpenShortcuts)
  window.addEventListener('keydown', handleKeydown)
  // 回填存量订阅源的图标（站点 favicon 兜底，仅补空值）：必须在 loadFeeds 之前执行
  await invoke('feeds_backfill_icons').catch((e) => console.error('回填订阅源图标失败:', e))
  await feedsStore.loadFeeds()
  await settingsStore.loadSettings().catch((e) => console.error('加载设置失败:', e))
  // 统一应用外观设置（主题 / 界面字体 / 内容字体 / 阅读字号）
  applyAppearance(settingsStore)
})

/** 组件卸载时移除全局事件监听，避免泄漏 */
onUnmounted(() => {
  window.removeEventListener('open-settings', handleOpenSettings)
  window.removeEventListener('open-shortcuts', handleOpenShortcuts)
  window.removeEventListener('keydown', handleKeydown)
})
</script>
