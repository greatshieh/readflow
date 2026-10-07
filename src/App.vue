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
import FeedPanel from './components/FeedPanel.vue'
import TitleBar from './components/TitleBar.vue'
import ArticleColumn from './components/ArticleColumn.vue'
import ContentColumn from './components/ContentColumn.vue'
import GraphView from './components/GraphView.vue'
import SettingsModal from './components/SettingsModal.vue'
import GlobalModal from './components/GlobalModal.vue'

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

/**
 * 打开设置模态的回调（绑定到 window 的 `open-settings` 事件，由 TitleBar 齿轮按钮派发）
 * @returns 无返回值；副作用为把 showSettings 置为 true
 */
function handleOpenSettings() {
  showSettings.value = true
}

/**
 * 被本组件接管的快捷键集合
 *
 * 只有落在这个集合里的按键才会 `preventDefault()`。此前是无条件拦截所有按键，
 * 结果把 Ctrl+C / Ctrl+R / Ctrl+F 之类的系统与浏览器快捷键一并吞掉。
 */
const SHORTCUT_KEYS = new Set(['j', 'k', 'v', 'e', 'g', '/', 'b', 'm'])

/**
 * 全局键盘快捷键处理
 *
 * # 响应条件（须全部满足）
 * 1. 未按下 Ctrl / Cmd / Alt —— 组合键一律留给系统与浏览器；
 * 2. 事件目标不是输入类元素（input / textarea / contenteditable）—— 避免打字被当成命令；
 * 3. 没有弹窗打开 —— 否则弹窗里按 `m` 会在用户看不见列表的情况下触发"全部已读"。
 *
 * Esc 退出专注模式的判断放在最前，因为它不涉及修饰键冲突，且应当在任何情况下可用。
 *
 * # 弹窗判定为何读全局计数
 * `anyDialogOpen`（`utils/dialogStack`）由所有 `BaseModal` 弹窗在开合时登记，计数器
 * 覆盖了 RulesModal / DigestModal / ResearchModal / SettingsModal / FeedManageModal
 * 以及 `GlobalModal`（confirm / prompt / alert，BaseModal 的 `global` 变体）等全部模态弹窗；
 * `modalStore.activeModal` 与计数条件并列判断，作为语义上更直接的"全局确认框开着"表达。
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
  // 输入框 / 文本框内不响应快捷键，避免冲突
  const target = e.target as HTMLElement | null
  const tag = target?.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || target?.isContentEditable) return
  // 弹窗打开时不响应：避免误触"全部已读"这类不可逆操作
  if (anyDialogOpen.value || modalStore.activeModal) return

  const key = e.key.toLowerCase()
  if (!SHORTCUT_KEYS.has(key)) return
  // 仅对真正接管的按键阻止默认行为（如 Firefox 下 "/" 是快速查找）
  e.preventDefault()

  switch (key) {
    case 'j': // 下一篇文章
      articlesStore.loadArticles(
        feedsStore.selectedFeed?.id ?? null,
        20,
        articlesStore.articles.length
      )
      break
    case 'k': // 上一篇文章（跳回顶部、重新加载）
      articlesStore.loadArticles(feedsStore.selectedFeed?.id ?? null, 20, 0)
      break
    case 'v': // 在浏览器中打开原文
      if (articlesStore.selectedArticle) {
        invoke('open_external', { url: articlesStore.selectedArticle.link })
      }
      break
    case 'e': // 导出到 Obsidian
      if (articlesStore.selectedArticle) {
        invoke('article_export_obsidian', { articleId: articlesStore.selectedArticle.id })
      }
      break
    case 'g': // 回到全部文章视图
      // 直接置空选中态即可：store 内部对 selectedFeed 的读写本就是 ref 语义
      feedsStore.selectedFeed = null
      articlesStore.loadArticles(null, 20, 0)
      break
    case '/': // 聚焦搜索框
      document.querySelector<HTMLInputElement>('.tb-search input')?.focus()
      break
    case 'b': // 切换书签视图
      articlesStore.setViewFilter(
        articlesStore.viewFilter === 'bookmarked' ? 'all' : 'bookmarked'
      )
      break
    case 'm': // 全部已读
      articlesStore.markAllRead(feedsStore.selectedFeed?.id ?? null)
      break
    default:
      break
  }
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
  window.removeEventListener('keydown', handleKeydown)
})
</script>
