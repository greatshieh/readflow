<template>
  <!-- 顶部固定工具条：对齐 Folo 的 Entry ActionBar（主栏按钮 + 更多菜单）。
       标签浮层与目录浮层各自"按钮 + 面板"整体算一个容器，点击其外部收起。 -->
  <div class="content-toolbar">
    <button
      class="tb-btn"
      :class="{ on: article.is_read }"
      :title="article.is_read ? '标记为未读' : '标记为已读'"
      @click.stop="toggleRead"
    >
      <!-- 未读态：用"信封"图标语义地表示"尚未打开"；已读态：用对勾表示已完成 -->
      <svg v-if="!article.is_read" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"></path>
        <polyline points="22,6 12,13 2,6"></polyline>
      </svg>
      <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"></path>
        <polyline points="22,6 12,13 2,6"></polyline>
        <polyline points="15,12 17,14 21,10"></polyline>
      </svg>
      <span>{{ article.is_read ? '已读' : '未读' }}</span>
    </button>
    <button class="tb-btn" :class="{ on: article.is_bookmarked }" @click.stop="handleToggleBookmark" title="收藏">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"></path>
      </svg>
    </button>
    <!-- 标签：展开 TagPicker 浮层（选择 / 管理两个页签），已打标时按钮点亮并显示数量 -->
    <div class="tags-wrap" ref="tagsWrapRef">
      <button
        class="tb-btn"
        :class="{ on: tagCount > 0 }"
        @click.stop="toggleTags"
        :title="tagCount > 0 ? `标签（${tagCount}）` : '标签'"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round">
          <path d="M20.6 13.4 12 22l-9-9V4h9l8.6 8.6a1.4 1.4 0 0 1 0 2z"></path>
          <circle cx="7.5" cy="7.5" r="1.2" fill="currentColor" stroke="none"></circle>
        </svg>
        <span v-if="tagCount > 0">{{ tagCount }}</span>
      </button>
      <div class="tags-pop" v-if="showTags">
        <TagPicker @close="showTags = false" />
      </div>
    </div>
    <!-- 目录：只在正文含 ≥2 个可用标题时出现。正文只来自 RSS 自带的
         content/summary（本项目不做全文抓取），实测仅约 6% 的文章有章节结构，
         因此没有标题时整个入口不渲染，而不是点开一个空目录 -->
    <div class="toc-wrap" ref="tocWrapRef" v-if="tocItems.length >= MIN_TOC_ITEMS">
      <button
        class="tb-btn"
        :class="{ on: showToc }"
        :title="`目录（${tocItems.length}）`"
        @click.stop="showToc = !showToc"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <line x1="8" y1="6" x2="21" y2="6"></line>
          <line x1="8" y1="12" x2="21" y2="12"></line>
          <line x1="8" y1="18" x2="21" y2="18"></line>
          <circle cx="4" cy="6" r="1" fill="currentColor" stroke="none"></circle>
          <circle cx="4" cy="12" r="1" fill="currentColor" stroke="none"></circle>
          <circle cx="4" cy="18" r="1" fill="currentColor" stroke="none"></circle>
        </svg>
      </button>
      <div class="toc-pop" v-if="showToc">
        <ArticleToc :items="tocItems" :scroll-el="scrollEl" />
      </div>
    </div>
    <button
      class="tb-btn"
      :class="{ on: showTranslation }"
      :title="
        !aiTranslationEnabled
          ? 'AI 翻译已在设置中关闭'
          : showTranslation
            ? '显示原文'
            : '译文'
      "
      :disabled="translationGenerating || !aiTranslationEnabled"
      @click.stop="emit('toggle-translation')"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M5 8l6 6"></path>
        <path d="M4 14l6-6 2-3"></path>
        <path d="M2 5h12"></path>
        <path d="M7 2h1"></path>
        <path d="M22 22l-5-10-5 10"></path>
        <path d="M14 18h6"></path>
      </svg>
    </button>
    <button
      class="tb-btn"
      :class="{ on: uiStore.focusMode }"
      :title="uiStore.focusMode ? '退出专注模式 (Esc)' : '专注阅读模式'"
      @click.stop="uiStore.toggleFocusMode()"
    >
      <svg v-if="!uiStore.focusMode" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"></path>
      </svg>
      <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M8 3v3a2 2 0 0 1-2 2H3m18 0h-3a2 2 0 0 1-2-2V3m0 18v-3a2 2 0 0 1 2-2h3M3 16h3a2 2 0 0 1 2 2v3"></path>
      </svg>
    </button>
    <button class="tb-btn" @click.stop="handleExportObsidian" title="导出到 Obsidian">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
        <polyline points="7 10 12 15 17 10"></polyline>
        <line x1="12" y1="15" x2="12" y2="3"></line>
      </svg>
    </button>
    <!-- 更多菜单：下拉含 复制链接 / 复制标题 / 打开原文 -->
    <div class="more-wrap">
      <button class="tb-btn" @click.stop="showMore = !showMore" title="更多">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="5" r="1"></circle>
          <circle cx="12" cy="12" r="1"></circle>
          <circle cx="12" cy="19" r="1"></circle>
        </svg>
      </button>
      <div class="more-menu" v-if="showMore" @click.stop>
        <button class="more-item" @click="copyLink">复制链接</button>
        <button class="more-item" @click="copyTitle">复制标题</button>
        <button class="more-item" @click="openOriginal">打开原文</button>
        <!-- 手动摘要入口：设置里关掉「AI 摘要」后自动生成不再发生，这里保留一次手动机会 -->
        <button class="more-item" @click="regenerateSummary">
          {{ article.ai_summary ? '重新生成摘要' : '生成摘要' }}
        </button>
      </div>
    </div>
    <!-- 导出反馈 -->
    <div class="export-status" :class="{ error: exportMsgError }" v-if="exportMsg">{{ exportMsg }}</div>
    <div class="copy-status" v-if="copyMsg">{{ copyMsg }}</div>
  </div>
</template>

<script setup lang="ts">
/**
 * 正文栏顶部工具条（Entry ActionBar）
 *
 * # 职责
 * 标记已读/未读、收藏、标签浮层、目录浮层、译文、专注、导出、更多菜单，
 * 以及导出/复制的短暂反馈。所有"看一眼文章就能自己做完"的动作都在组件内部完成，
 * 只有会改动**宿主渲染结果**的两个动作向外抛：
 * - `toggle-translation`：`showTranslation` 决定正文渲染的是原文还是译文；
 * - `open-url`：会在宿主里打开内嵌网页视图（`webViewUrl`）。
 *
 * # 边界（与 ArticleListToolbar 同一套取舍）
 * 展示态与"自足动作"直接读 store（`uiStore.focusMode`、`articlesStore.setRead` /
 * `toggleBookmark` / `generateSummary`、`tagsStore.loadTags`），不再逐层透传；
 * 只有宿主状态才走 props / emits。工具条自己的浮层开合态、外点关闭与 Esc
 * 也全在本组件内——宿主不需要知道 `tagsWrapRef` / `tocWrapRef` 的存在。
 *
 * # 自带定时器
 * 导出与复制反馈靠 `setTimeout` 到期复位，句柄统一登记并在卸载时清空，
 * 否则组件销毁后定时器仍会向已销毁的 ref 写入。
 */

import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import TagPicker from '@/components/TagPicker.vue'
import ArticleToc from '@/components/ArticleToc.vue'
import { useArticlesStore } from '@/stores/articles'
import { useTagsStore } from '@/stores/tags'
import { useUiStore } from '@/stores/ui'
import { MIN_TOC_ITEMS, type TocItem } from '@/utils/articleToc'
import type { Article } from '@/types'

const props = defineProps<{
  /** 当前正在阅读的文章 */
  article: Article
  /** 正文目录条目（由宿主从已渲染的正文 DOM 里取） */
  tocItems: TocItem[]
  /** 正文滚动容器（目录点击跳转需要） */
  scrollEl: HTMLElement | null
  /** 当前是否显示译文（决定译文按钮的点亮态） */
  showTranslation: boolean
  /** 是否正在翻译（决定译文按钮是否禁用） */
  translationGenerating: boolean
  /** 设置里的「AI 翻译」开关（关闭时按钮禁用并给出原因） */
  aiTranslationEnabled: boolean
}>()

const emit = defineEmits<{
  /** 请求切换原文 / 译文（译文状态由宿主持有，因为它决定正文渲染什么） */
  (e: 'toggle-translation'): void
  /** 请求在正文区内嵌打开原文（网页视图由宿主持有） */
  (e: 'open-url'): void
}>()

const articlesStore = useArticlesStore()
const tagsStore = useTagsStore()
const uiStore = useUiStore()

/** 更多菜单展开态 */
const showMore = ref(false)
/** 标签浮层展开态 */
const showTags = ref(false)
/** 标签按钮所在容器（点击外部关闭浮层时用于判断命中范围） */
const tagsWrapRef = ref<HTMLElement | null>(null)
/** 目录浮层容器（同样加进外点关闭的判断范围） */
const tocWrapRef = ref<HTMLElement | null>(null)
/** 目录浮层是否展开 */
const showToc = ref(false)
/** 当前文章的标签数（驱动按钮点亮与角标） */
const tagCount = computed<number>(() => props.article.tags?.length ?? 0)

/**
 * 切换文章时收起全部浮层并清空短暂提示
 *
 * 宿主已不再穿透重置工具条的内部状态（见 ContentColumn 切文章 watcher 的注释），
 * 所以"换文章要收起菜单"必须由状态的所有者自己保证：更多菜单与标签浮层的内容
 * 都只对当前文章成立，留着会让用户以为新文章也是那个状态。
 */
watch(
  () => props.article.id,
  () => {
    showMore.value = false
    showTags.value = false
    showToc.value = false
    exportMsg.value = ''
    exportMsgError.value = false
    copyMsg.value = ''
  }
)

/** 导出到 Obsidian 的反馈 */
const exportMsg = ref('')
const exportMsgError = ref(false)
/** 复制反馈 */
const copyMsg = ref('')

/** 组件内在途定时器句柄（卸载时统一清空） */
const timers: ReturnType<typeof setTimeout>[] = []

/**
 * 登记一个延时回调并返回句柄
 *
 * @param fn - 到期执行的回调
 * @param delay - 延迟毫秒数
 * @returns 定时器句柄
 */
function schedule(fn: () => void, delay: number) {
  const id = setTimeout(fn, delay)
  timers.push(id)
  return id
}

/** 切换当前文章已读/未读 */
async function toggleRead() {
  await articlesStore.setRead(props.article.id, !props.article.is_read)
}

/** 切换收藏 */
async function handleToggleBookmark() {
  await articlesStore.toggleBookmark(props.article.id)
}

/**
 * 开关标签浮层
 *
 * 展开时顺带重载一次标签目录：文章数（chip 右侧计数）会随打标/删标变化，
 * 用挂载时的旧快照会让用户看到过时数字。目录只有几十条，重载成本可忽略。
 *
 * @returns 无返回值；翻转 showTags
 */
function toggleTags() {
  showTags.value = !showTags.value
  if (showTags.value) tagsStore.loadTags()
}

/** 导出到 Obsidian */
async function handleExportObsidian() {
  exportMsg.value = ''
  exportMsgError.value = false
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const msg = await invoke<string>('article_export_obsidian', { articleId: props.article.id })
    exportMsg.value = msg
    exportMsgError.value = false
  } catch (e) {
    exportMsg.value = typeof e === 'string' ? e : (e as Error)?.message || '导出失败'
    exportMsgError.value = true
  } finally {
    // 导出结果提示 3 秒后自动消失（定时器在卸载时统一清理）
    schedule(() => {
      exportMsg.value = ''
      exportMsgError.value = false
    }, 3000)
  }
}

/** 复制反馈（2 秒后自动消失） */
let copyTimer: ReturnType<typeof setTimeout> | null = null
function flashCopy(msg: string) {
  copyMsg.value = msg
  if (copyTimer) clearTimeout(copyTimer)
  copyTimer = schedule(() => { copyMsg.value = '' }, 2000)
}

/**
 * 更多菜单 → 打开原文
 *
 * 先收起菜单（它是本组件的内部状态），再请宿主切换到网页视图；
 * 宿主那边只管 webViewUrl，不再回头重置本组件的菜单态。
 *
 * @returns 无返回值
 */
function openOriginal() {
  showMore.value = false
  emit('open-url')
}

/** 复制链接到剪贴板 */
async function copyLink() {
  const url = props.article.link
  showMore.value = false
  if (!url) return
  try {
    await navigator.clipboard.writeText(url)
    flashCopy('已复制链接')
  } catch {
    flashCopy('复制失败')
  }
}

/** 复制标题到剪贴板 */
async function copyTitle() {
  const title = props.article.title
  showMore.value = false
  if (!title) return
  try {
    await navigator.clipboard.writeText(title)
    flashCopy('已复制标题')
  } catch {
    flashCopy('复制失败')
  }
}

/**
 * 手动重新生成 AI 摘要
 *
 * 与自动摘要共用 store 的调用；作为「AI 摘要」开关关掉后的手动出口存在。
 *
 * @returns 无返回值；结果由 store 写入 ai_summary / aiError
 */
async function regenerateSummary() {
  showMore.value = false
  if (articlesStore.generating) return
  try {
    await articlesStore.generateSummary(props.article.id)
  } catch (e) {
    console.error('[ContentToolbar] 重新生成摘要失败:', e)
  }
}

/**
 * 点击浮层之外的地方时收起浮层
 *
 * 用 document 级监听而非全屏透明遮罩：遮罩会吞掉滚轮事件导致正文无法滚动，
 * 而监听只判断点击目标，不影响任何滚动或文本选择行为。
 *
 * @param e - 鼠标事件
 */
function onDocumentClick(e: MouseEvent) {
  const target = e.target as Node | null
  if (!target) return
  // 标签浮层：点击其容器（按钮 + 面板）之外即关闭
  if (showTags.value && !tagsWrapRef.value?.contains(target)) {
    showTags.value = false
  }
  // 目录浮层：同样是「按钮 + 面板」整体算容器
  if (showToc.value && !tocWrapRef.value?.contains(target)) {
    showToc.value = false
  }
}

/**
 * Esc 收起浮层
 *
 * @param e - 键盘事件
 */
function onDocumentKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  if (showTags.value) showTags.value = false
  if (showToc.value) showToc.value = false
}

// 挂在 document 的冒泡阶段：浮层内部的点击由各自的 handler 处理，
// 冒泡上来时已被上面的 contains 判断放行，不会误关。
onMounted(() => {
  document.addEventListener('click', onDocumentClick)
  document.addEventListener('keydown', onDocumentKeydown)
})

// 与上面的注册成对解绑；顺带清空在途定时器，避免它们向已销毁的组件写状态
onUnmounted(() => {
  document.removeEventListener('click', onDocumentClick)
  document.removeEventListener('keydown', onDocumentKeydown)
  timers.forEach(clearTimeout)
  timers.length = 0
})
</script>

<style scoped>
/* 顶部工具条：固定不滚动，含主栏按钮 + 更多菜单 */
.content-toolbar {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-2) var(--sp-4);
  flex-shrink: 0;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  position: relative;
}
.tb-btn {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: 5px var(--sp-15);
  border: 1px solid transparent;
  border-radius: var(--r-md);
  background: var(--surface);
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
  font-family: inherit;
}
.tb-btn:hover { border-color: var(--border); color: var(--text-primary); background: var(--fill); }
.tb-btn.on { background: var(--primary-fg); border-color: var(--primary); color: var(--primary); }
.tb-btn:disabled { opacity: 0.5; cursor: default; }
.tb-btn svg { width: 15px; height: 15px; flex-shrink: 0; }

/* 标签浮层：与"更多"菜单同一套锚定方式（相对按钮定位、浮在工具条下方）。
   宽度交给 TagPicker 自身（260px），这里只管定位与层级。 */
.tags-wrap { position: relative; }
.tags-pop {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 30;
}

/* 目录浮层：与标签浮层同款定位，但自带面板外观（内层 ArticleToc 只管内容） */
.toc-wrap { position: relative; }
.toc-pop {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 30;
  padding: var(--sp-1);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  box-shadow: var(--shadow-hover);
}

/* 更多菜单 */
.more-wrap { position: relative; }.more-menu {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  z-index: 30;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  box-shadow: var(--shadow-hover);
  padding: var(--sp-1);
  min-width: 120px;
}
.more-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: var(--sp-2) var(--sp-15);
  border: none;
  background: none;
  font-size: var(--fs-sm);
  color: var(--text-primary);
  border-radius: var(--r-sm);
  cursor: pointer;
  font-family: inherit;
}
.more-item:hover { background: var(--fill); color: var(--primary); }

.export-status {
  font-size: var(--fs-sm);
  color: var(--primary);
  margin-left: var(--sp-2);
}
.export-status.error { color: var(--primary); }
.copy-status {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  margin-left: var(--sp-2);
}
</style>
