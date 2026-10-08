<template>
  <!-- 正文阅读栏：三栏布局的"右列"，自身是一个独立 flex 列。
       结构：顶栏工具条 → 标题/meta → 摘要卡 + 正文（+ 内嵌网页视图），
       另有右下角两个 FAB（研究事件 / 文章追问）与图片灯箱；
       未选中文章时显示空状态引导页。
       四个子组件各管一块：ContentToolbar（顶栏）、ArticleSummaryCard（摘要卡）、
       ResearchFab（研究事件 FAB + 面板）、HighlightPalette（高亮调色板）。 -->
  <div class="content-column" ref="paneRef">
    <!-- 跟随边框：正文区保留这一层，但**不加光斑**——
         正文是实底（--read-bg，见 .content-column），透不出光，光斑只会糊成脏斑。 -->
    <div class="pane-frame" ref="frameRef" aria-hidden="true"></div>
    <!-- 正文内容：仅在存在选中文章时渲染 -->
    <div class="content" v-if="selectedArticle" @click="showArticleList = true">
      <!-- 顶部固定工具条：对齐 Folo 的 Entry ActionBar（标记已读/未读 · 收藏 · 标签 ·
           目录 · 译文 · 专注 · 导出 · 更多）。
           浮层开合、外点关闭与 Esc 由工具条自持；只有"切译文"与"打开原文"两个会改变
           本栏渲染结果的动作回抛给这里（showTranslation / webViewUrl 都是本栏状态）。 -->
      <ContentToolbar
        v-show="!webViewUrl"
        :article="selectedArticle"
        :toc-items="tocItems"
        :scroll-el="readingScrollRef"
        :show-translation="showTranslation"
        :translation-generating="translationGenerating"
        :ai-translation-enabled="aiTranslationEnabled"
        @toggle-translation="toggleTranslation"
        @open-url="openUrl"
      />

      <!-- 标题区：标题 + 元信息（工具条下方、固定不滚动） -->
      <div class="content-head">
        <h1 class="content-title">{{ selectedArticle.title }}</h1>
        <div class="content-meta">
          <span class="content-source">{{ getFeedName(selectedArticle.feed_id) }}</span>
          <span class="content-time">{{ formatFullDate(selectedArticle.published_at) }}</span>
          <span class="content-author" v-if="selectedArticle.author">· {{ selectedArticle.author }}</span>
        </div>
        <!-- 生成状态行：只承载"翻译中"与错误提示。
             摘要的生成进度已移入摘要卡片内部（此处不再重复显示，避免两处同时转圈） -->
        <div class="ai-status" :class="{ error: !!articlesStore.aiError, 'is-generating': articlesStore.generating }" v-if="showToolbarAiStatus">
          <template v-if="articlesStore.generating">
            <span class="ai-spinner"></span>
            <span class="ai-status-text">{{ aiStatusText }}</span>
          </template>
          <template v-else-if="articlesStore.aiError">
            <span class="ai-err-ico">⚠</span>{{ articlesStore.aiError }}
          </template>
        </div>
      </div>

      <!-- 阅读内容区：文章视图（.reading-scroll）与网页视图（.webview-wrap）承载 -->
      <div class="content-body-area">
        <div
          class="reading-scroll"
          ref="readingScrollRef"
          v-show="!webViewUrl"
          @scroll.passive="onReadingScroll"
        >
          <!-- 摘要卡片：生成中即提前出现（卡片内自带转圈 + 骨架屏），
               生成完成后骨架替换为摘要正文，避免进度提示游离在卡片之外 -->
          <!-- 摘要卡片：有摘要或正在生成时出现，卡片内部自带转圈 + 骨架屏 -->
          <ArticleSummaryCard
            v-if="selectedArticle?.ai_summary || summaryGenerating"
            :summary="selectedArticle?.ai_summary ?? null"
            :streaming-text="streamingSummary"
            :generating="summaryGenerating"
          />

          <div class="content-body" ref="contentBodyRef" v-html="displayContent" @click="onContentClick" @mouseup="onContentMouseUp"></div>
        </div>
        <div class="webview-wrap" v-if="webViewUrl">
          <iframe class="webview-iframe" :src="webViewUrl" referrerpolicy="no-referrer"></iframe>
          <button class="webview-back" @click="closeWebView" title="返回正文">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="19" y1="12" x2="5" y2="12"></line>
              <polyline points="12 19 5 12 12 5"></polyline>
            </svg>
            <span>正文</span>
          </button>
        </div>
      </div>
    </div>

    <!-- 高亮调色板：选中正文文字后浮出（外观与"保住选区"的交互见 HighlightPalette.vue） -->
    <HighlightPalette
      v-if="selectedArticle && showHighlightPalette"
      :colors="HIGHLIGHT_COLORS"
      @pick="createHighlight"
      @cancel="cancelHighlight"
    />

    <!-- 研究事件悬浮入口：模块与自持状态（面板开合/外点关闭/Esc）见 ResearchFab.vue。
         与「文章追问」FAB 共用同一个可见条件（见 showReadingFabs）——两者同处右下角
         上下叠放，各自判断一旦出现分歧，就会留下一个悬空的按钮。
         `&& selectedArticle` 对运行时是冗余的（showReadingFabs 已含此项），作用是让
         模板里的 article 收窄为非空，省掉一处非空断言。 -->
    <ResearchFab v-if="showReadingFabs && selectedArticle" :article="selectedArticle" />

    <!-- 文章追问入口：与「研究事件」FAB 同处右下角、叠放其上方。
         会话状态、流式消费、外点关闭全部封装在组件内，此处只负责挂载与传入当前文章。 -->
    <ArticleChat v-if="showReadingFabs" :article="selectedArticle" />

    <!-- 图片灯箱 -->
    <div class="lightbox" v-if="lightboxOpen" @click="lightboxOpen = false">
      <img :src="lightboxSrc" alt="图片预览" @click.stop />
    </div>

    <!-- 空状态：未选中文章时的引导页。骨架屏 / 空态 / 错误态的视觉语言
         来自全局 .state-block + .state-logo，与订阅栏、文章列表共用一套。 -->
    <div class="empty-state" v-if="!selectedArticle">
      <!-- 标志复用 AppLogo，与标题栏同源；此处放大到 80px 作为主空状态的视觉锚点 -->
      <span class="state-logo"><AppLogo :size="80" /></span>
      <p class="empty-title">接下来读点什么？</p>
      <p class="empty-hint">选择一篇未读文章，或从订阅源中探索</p>
      <div class="empty-actions">
        <button class="empty-action" @click="readRandom" v-if="articlesStore.displayArticles.length > 0">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon>
          </svg>
          随机读一篇
        </button>
        <button class="empty-action" @click="refreshAll">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="23 4 23 10 17 10"></polyline>
            <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"></path>
          </svg>
          刷新订阅源
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 正文阅读栏组件（三栏布局的右列）
 *
 * # 职责
 * 本栏的**编排与阅读状态**：标题/meta、正文渲染（含高亮标注）、网页视图（iframe 内嵌）、
 * 图片灯箱、空状态引导，以及阅读进度的上报与恢复。逐块渲染已拆给四个子组件：
 * - `ContentToolbar.vue`：顶栏工具条（已读/收藏/标签/目录/译文/专注/导出/更多）；
 * - `ArticleSummaryCard.vue`：AI 摘要卡（含流式与骨架屏）；
 * - `ResearchFab.vue`：研究事件 FAB + 面板（自持开合与外点关闭）；
 * - `HighlightPalette.vue`：高亮调色板。
 * 本组件仍持有"决定别处渲染什么"的状态：`showTranslation`（原文/译文）与
 * `webViewUrl`（文章视图/网页视图），故工具条的这两个动作以事件回抛到这里。
 *
 * # 正文高亮
 * 逻辑整体收在 `composables/useHighlights.ts`（调色板状态、选区快照、标记渲染、
 * 增删确认），本组件只解构使用。两块与 DOM 事件强耦合的代码留在宿主：
 * `onContentClick` 里高亮块的删除分发（与链接 / 灯箱共用事件委托），以及切换
 * 文章 watcher 里的调色板复位与选区快照丢弃。
 *
 * # 与 Pinia store 的关系
 * - `useArticlesStore()`：selectedArticle、displayArticles、highlights、生成/收藏/已读/导出等行为；
 * - `useFeedsStore()`：getFeedName 映射、refreshFeeds（空状态"刷新"）；
 * - `useUiStore()`：focusMode（专注阅读模式开关）；
 * - `useSettingsStore()`：读 AI 摘要 / 翻译开关，决定是否自动发起请求；
 * - composable useHighlights()：正文高亮（选区捕获 / 标记渲染 / 增删确认）。
 */

import { ref, shallowRef, watch, computed, nextTick, onUnmounted } from 'vue'
import AppLogo from '@/components/AppLogo.vue'
import ArticleChat from '@/components/ArticleChat.vue'
import ArticleSummaryCard from '@/components/ArticleSummaryCard.vue'
import ContentToolbar from '@/components/ContentToolbar.vue'
import HighlightPalette from '@/components/HighlightPalette.vue'
import ResearchFab from '@/components/ResearchFab.vue'
import { useHighlights, HIGHLIGHT_COLORS } from '@/composables/useHighlights'
import { useCursorGlow } from '@/composables/useCursorGlow'
import { useArticlesStore } from '@/stores/articles'
import { useFeedsStore } from '@/stores/feeds'
import { useUiStore } from '@/stores/ui'
import { useSettingsStore } from '@/stores/settings'
import { formatFullDate } from '@/utils/format'
import { collectTocItems, type TocItem } from '@/utils/articleToc'
import type { Article } from '@/types'

/** 正文栏根元素（跟随边框的宿主；光斑层未挂 .pane-glow-host，故不生效） */
const paneRef = ref<HTMLElement | null>(null)
/** 跟随边框层：靠近指针的那一侧亮起一段 */
const frameRef = ref<HTMLElement | null>(null)
/** 只启用跟随边框。正文是实底，加光斑会糊成脏斑，故不传 .pane-glow-host 的宿主用法 */
useCursorGlow(paneRef, frameRef)

const articlesStore = useArticlesStore()
const feedsStore = useFeedsStore()
const uiStore = useUiStore()
const settingsStore = useSettingsStore()
/** 正文容器引用：高亮标记要直接写进这段 v-html 渲染出的 DOM 里 */
const contentBodyRef = ref<HTMLElement | null>(null)

// ─── 阅读进度：滚动位置的上报与恢复 ────────────────────────────────────────────
//
// 滚动容器是 `.reading-scroll`（`.content-body` 只是被它滚动的内层 v-html 内容），
// 因此所有读写都落在这个元素上。三条规则值得说明：
// 1. **分母用「可滚距离」**（`scrollHeight - clientHeight`）而不是总高度——用总高度
//    会让长文永远滚不到 1.0，恢复时也会永远偏上；
// 2. **切换文章必须先存后取**：监听 `selectedArticle` 的 watcher 是 pre-flush 的，
//    回调触发时 DOM 还是旧正文，所以先 `flushProgress()`，再等 `nextTick` 之后的新
//    正文 `restoreProgress()`；
// 3. **切换窗口期屏蔽 scroll**：DOM 尚未换掉的那一小段时间里若触发滚动，会把旧文章
//    的位置误记到新文章上（靠 `progressSuppressed` 挡住）。

/** 进度上报的防抖间隔（毫秒）：scroll 是高频事件，写库必须降频 */
const PROGRESS_THROTTLE_MS = 800
/** 低于此比例视为"停在顶部"：既省掉 0 附近抖动产生的无意义写入，也让回到顶部不留残值 */
const PROGRESS_MIN_RATIO = 0.02

/** 正文滚动容器（`.reading-scroll`） */
const readingScrollRef = ref<HTMLElement | null>(null)

/** 待写库的进度，节流期间只保留最后一次 */
let pendingProgress: { id: number; ratio: number } | null = null
/** 防抖计时器句柄 */
let progressTimer: ReturnType<typeof setTimeout> | undefined
/** 切换文章期间为 true，屏蔽滚动上报 */
let progressSuppressed = false

/**
 * 计算当前滚动比例
 *
 * @returns 0.0–1.0 的比例；内容不足一屏（无可滚距离）时返回 0
 */
function currentProgress(): number {
  const el = readingScrollRef.value
  if (!el) return 0
  const scrollable = el.scrollHeight - el.clientHeight
  if (scrollable <= 0) return 0
  return Math.min(1, Math.max(0, el.scrollTop / scrollable))
}

/**
 * 立即把待写进度落库并取消防抖；无待写内容时是空操作
 *
 * 不 `await`：进度写库不该阻塞切文或卸载流程，store 内部已做静默失败。
 */
function flushProgress(): void {
  if (progressTimer !== undefined) {
    clearTimeout(progressTimer)
    progressTimer = undefined
  }
  const pending = pendingProgress
  pendingProgress = null
  if (!pending) return
  void articlesStore.setProgress(pending.id, pending.ratio)
}

/**
 * 滚动时记录比例并延迟上报
 *
 * 属于 `@scroll.passive` 处理器：**不得调用 `preventDefault`**，也不做重活，
 * 只算一次比例后交给防抖定时器。
 */
function onReadingScroll(): void {
  if (progressSuppressed) return
  const id = selectedArticle.value?.id
  if (id === undefined) return

  const ratio = currentProgress()
  pendingProgress = { id, ratio: ratio < PROGRESS_MIN_RATIO ? 0 : ratio }
  if (progressTimer !== undefined) return
  progressTimer = setTimeout(() => {
    progressTimer = undefined
    flushProgress()
  }, PROGRESS_THROTTLE_MS)
}

/**
 * 按当前文章已存的进度恢复滚动位置，完成后解除上报屏蔽
 *
 * 用双 `requestAnimationFrame`：`v-html` 换成新正文后图片等元素才完成布局，
 * 只等 `nextTick` 时 `scrollHeight` 偏小，会把恢复位置算得偏上。
 */
async function restoreProgress(): Promise<void> {
  await nextTick()
  const apply = (): void => {
    const el = readingScrollRef.value
    const article = selectedArticle.value
    if (el && article) {
      const ratio = article.read_progress ?? 0
      const scrollable = el.scrollHeight - el.clientHeight
      // scrollable <= 0 表示正文不足一屏，本来就没有可恢复的位置
      el.scrollTop = ratio < PROGRESS_MIN_RATIO || scrollable <= 0 ? 0 : ratio * scrollable
    }
    progressSuppressed = false
  }
  requestAnimationFrame(() => requestAnimationFrame(apply))
}

/** 当前选中的文章 */
const selectedArticle = computed<Article | null>(() => articlesStore.selectedArticle)

/** 窄屏文章列表抽屉态（点击正文区视为想看列表） */
const showArticleList = ref(false)
/** 图片灯箱 */
const lightboxOpen = ref(false)
const lightboxSrc = ref('')
/** 网页视图内嵌 URL（空=文章视图） */
const webViewUrl = ref('')
/** 翻译目标语言（固定中文） */
const TARGET_LANG = '中文'
/** 正文区是否显示译文 */
const showTranslation = ref(false)
/**
 * 阅读区右下角悬浮入口是否显示（研究事件 FAB + 文章追问 FAB）
 *
 * **两个 FAB 必须由同一个条件驱动**：它们同处右下角、上下叠放，若各自判断，
 * 一旦条件出现分歧就会留下一个悬空的按钮。这也是它在宿主而非各 FAB 内部的原因
 * ——ResearchFab 自持面板与事件状态，但"显不显示"由这里统一裁决。
 * 三个隐藏条件各有原因：网页视图下按钮会压住 iframe 内容；专注模式追求零干扰；
 * 未选中文章时两者都没有作用对象。除此之外一律常驻——即便尚未提取事件也要露出入口，
 * 否则用户根本不会知道有研究事件、以及可以追问这回事。
 */
const showReadingFabs = computed(
  () => !!selectedArticle.value && !webViewUrl.value && !uiStore.focusMode
)

/**
 * 是否处于"摘要生成中"
 *
 * 摘要进度已移入摘要卡片内部展示，因此这里需要单独判断模式：
 * generating 为真且模式不是 translate 时即视为摘要生成中。
 *
 * @returns 正在生成摘要时返回 true
 */
const summaryGenerating = computed(
  () => articlesStore.generating && articlesStore.generatingMode !== 'translate'
)

/**
 * 是否处于"翻译生成中"
 *
 * 与 `summaryGenerating` 对称：同一个 `generating` 标志按 `generatingMode` 分流，
 * 二者互补（`generating && !summaryGenerating` 恒等于本式）。工具条的译文按钮据此
 * 置灰——此前该绑定误写成了裸标识符 `generating`，而模板作用域里并无此变量，
 * 于是"生成中禁用"从未生效，直到接入 `.vue` 类型检查才暴露。
 *
 * @returns 正在翻译时返回 true
 */
const translationGenerating = computed(
  () => articlesStore.generating && articlesStore.generatingMode === 'translate'
)

/**
 * 设置里的「AI 摘要」开关是否开启
 *
 * 缺省视为开启（与 seed 默认值 '1' 一致）。只约束**自动**生成：关掉之后
 * 打开文章不再自动发请求，用户仍可从「更多 → 重新生成摘要」手动触发。
 *
 * @returns 开启时返回 true
 */
const aiSummaryEnabled = computed(() => settingsStore.getSetting('ai_summary_enabled') !== '0')

/**
 * 设置里的「AI 翻译」开关是否开启
 *
 * 关掉之后工具条上的译文按钮置灰不可点，避免用户以为功能坏了。
 *
 * @returns 开启时返回 true
 */
const aiTranslationEnabled = computed(
  () => settingsStore.getSetting('ai_translation_enabled') !== '0'
)

/**
 * 标题区状态行是否需要显示
 *
 * 摘要进度已由卡片承载，故此处仅在"翻译中"或"出错"时显示，避免两处重复提示。
 *
 * @returns 需要显示状态行时返回 true
 */
const showToolbarAiStatus = computed(
  () => translationGenerating.value || !!articlesStore.aiError
)

/** 生成状态文字：根据当前生成模式显示不同提示 */
const aiStatusText = computed(() => {
  if (!articlesStore.generating) return ''
  const mode = articlesStore.generatingMode
  if (mode === 'translate') return '正在翻译中...'
  return '正在生成摘要...'
})

/**
 * 摘要的流式累积文本
 *
 * 三个条件缺一不可：正在生成摘要、增量已到达、增量属于当前这篇。
 * 最后一条用于"生成途中切换文章"——增量是全局累积的，
 * 不加文章归属判断就会把上一篇的摘要画到这一篇上。
 *
 * @returns 可供摘要卡片直接渲染的文本；条件不满足时为空串（走骨架屏分支）
 */
const streamingSummary = computed(() => {
  const a = selectedArticle.value
  if (!a) return ''
  if (articlesStore.streamingKind !== 'summary') return ''
  if (articlesStore.streamingArticleId !== a.id) return ''
  return articlesStore.streamingText
})

/**
 * 去掉 HTML 标签，只留可见文本（流式译文预览用）
 *
 * 流式过程中拿到的 HTML 必然是"半截的"——`<p`、`</di` 这类片段都可能出现，
 * 直接 v-html 会把这些碎片当文本渲染出来。预览阶段只求"能读、在动"，
 * 完成后整体换成后端消毒过的 HTML。
 *
 * @param html - 可能尚未闭合的 HTML 片段
 * @returns 去掉标签后的文本
 */
function stripTags(html: string): string {
  // 正则末尾的 `>?` 用于吃掉行尾那个还没闭合的 `<xxx`
  return html.replace(/<[^>]*>?/g, '')
}

/** 正在流式生成的译文增量（属于当前文章时才非空） */
const streamingTranslation = computed(() => {
  const a = selectedArticle.value
  if (!a) return ''
  if (articlesStore.streamingKind !== 'translate') return ''
  if (articlesStore.streamingArticleId !== a.id) return ''
  return articlesStore.streamingText
})

/** 正文实际渲染的 HTML：流式译文预览 / 原文 / 译文切换 */
const displayContent = computed(() => {
  const a = selectedArticle.value
  if (!a) return ''
  // 翻译进行中优先显示增量预览：否则用户点了「译文」却只能盯着原文，
  // 而译文其实正在一批批到达
  if (streamingTranslation.value) return stripTags(streamingTranslation.value)
  if (showTranslation.value && a.ai_translation) return a.ai_translation
  return a.content || a.summary
})

// ─── 正文目录（TOC）────────────────────────────────────────────────────────────
//
// 条目从**已渲染的正文 DOM** 里取（见 `utils/articleToc.ts` 文件头说明），
// 因此必须在 `nextTick` 之后再取：`v-html` 换成新正文之前，容器里还是上一篇的标题。

/**
 * 目录条目
 *
 * 用 `shallowRef` 而非 `ref`：条目里存的是 DOM 元素，交给 Vue 深度代理既没有
 * 必要，也可能让元素身份比较（`contains` / 观察器目标匹配）出现意外。
 */
const tocItems = shallowRef<TocItem[]>([])

/**
 * 从当前正文 DOM 重建目录条目
 *
 * @returns 无返回值；结果写入 `tocItems`
 */
function rebuildToc(): void {
  tocItems.value = collectTocItems(contentBodyRef.value, selectedArticle.value?.title ?? '')
}

// 正文变化 = 换了文章或切了原文/译文，两种情况都要按新正文重取目录
watch(
  displayContent,
  () => {
    nextTick(rebuildToc)
  },
  { immediate: true }
)

// 注：原先这里还有一条"切文章收起目录浮层"的 watcher——目录浮层的开合态已随
// ContentToolbar 走，收起由该组件自己的切文章 watcher 负责，宿主不再穿透。

/** 由 feed_id 反查订阅源名称 */
function getFeedName(feedId: number): string {
  const feed = feedsStore.feeds.find(f => f.id === feedId)
  return feed ? feed.name : `Feed ${feedId}`
}

/**
 * 切换译文
 *
 * 首次点击时先请求翻译再切到译文视图；已有译文则直接在原文 / 译文之间来回切。
 * 设置里关掉「AI 翻译」时按钮已禁用，这里只做一道兜底判断。
 *
 * @returns 无返回值；副作用为更新 `showTranslation` 或触发一次翻译
 */
async function toggleTranslation() {
  if (!selectedArticle.value) return
  if (showTranslation.value) { showTranslation.value = false; return }
  if (!aiTranslationEnabled.value) return
  if (!selectedArticle.value.ai_translation) {
    try {
      await articlesStore.generateTranslation(selectedArticle.value.id, 'agnes', TARGET_LANG)
    } catch (e) {
      // 具体原因由 store 写入 aiError，标题区会展示出来
      console.error('[ContentColumn] 翻译失败:', e)
    }
  }
  if (selectedArticle.value.ai_translation) showTranslation.value = true
}

/**
 * 在正文区域内嵌打开原文（iframe 网页视图）
 *
 * 由工具条的「更多 → 打开原文」触发；"更多"菜单自身的收起是工具条的内部状态，
 * 由工具条负责，这里只管切换本栏的网页视图。
 */
function openUrl() {
  const url = selectedArticle.value?.link
  if (!url || !/^https?:\/\//i.test(url)) return
  webViewUrl.value = url
}
/** 关闭网页视图 */
function closeWebView() { webViewUrl.value = '' }

/** 正文点击委托：内嵌打开链接 / 图片灯箱 */
function onContentClick(e: MouseEvent) {
  const target = e.target as HTMLElement

  // 高亮块：点击即询问是否删除。判断放在最前面——高亮块可能落在链接或图片上，
  // 若先处理链接就会把"删高亮"变成"打开原文/放大图"。
  const mark = target.closest('mark[data-highlight-id]') as HTMLElement | null
  if (mark) {
    e.preventDefault()
    e.stopPropagation()
    void removeHighlight(Number(mark.dataset.highlightId))
    return
  }

  const anchor = target.closest('a')
  if (anchor) {
    e.preventDefault(); e.stopPropagation()
    const href = anchor.getAttribute('href') || ''
    if (/^https?:\/\//i.test(href)) webViewUrl.value = href
    return
  }
  if (target.tagName === 'IMG') {
    e.stopPropagation()
    const img = target as HTMLImageElement
    lightboxSrc.value = img.currentSrc || img.src
    lightboxOpen.value = true
  }
}

/** 空状态：随机读一篇 */
function readRandom() {
  const list = articlesStore.displayArticles
  if (list.length === 0) return
  const pick = list[Math.floor(Math.random() * list.length)]
  articlesStore.selectArticle(pick.id)
  showArticleList.value = false
}
/** 空状态：刷新全部 */
async function refreshAll() {
  try {
    await feedsStore.refreshFeeds()
    await articlesStore.loadArticles(feedsStore.selectedFeed?.id ?? null)
  } catch (e) {
    console.error('刷新失败:', e)
  }
}

/** 切换文章时重置随文章清除的视图态，并自动触发摘要生成 */
watch(
  () => selectedArticle.value?.id,
  async () => {
    // 先落盘上一篇的进度、再屏蔽上报：本 watcher 是 pre-flush 触发的，
    // 此刻 DOM 仍是旧正文，scrollTop 才代表上一篇真实的阅读位置。
    // 顺序不能颠倒——先屏蔽会把上一篇的进度一起挡掉。
    flushProgress()
    progressSuppressed = true

    // 先重置状态。工具条（更多菜单 / 标签浮层 / 目录浮层）与调色板各自的
    // 内部态由它们自己随文章切换重置——这里的"随文章变化"归各自所有者维护，
    // 宿主不再穿透去改别人的状态。
    articlesStore.aiError = null
    showTranslation.value = false
    webViewUrl.value = ''
    showHighlightPalette.value = false
    // 丢弃上一篇文章的选区快照：留着会把高亮建到新文章上
    pendingSelection.value = null
    // 清空上一篇文章的高亮：留着会让新文章的相同文字短暂地被标黄
    articlesStore.highlights = []

    if (!selectedArticle.value) {
      // 回到空态：没有正文也没有可恢复的位置，直接解除上报屏蔽，
      // 否则 progressSuppressed 会一直为 true，之后所有滚动都不再记录
      progressSuppressed = false
      return
    }

    // 恢复上一篇存的滚动位置。不 await：恢复要等两帧 rAF，而下面的高亮加载与
    // 摘要生成不必等它；恢复完成后由 restoreProgress 自己解除上报屏蔽。
    void restoreProgress()

    // 异步加载高亮（加载完由下面的 watch 负责把它们画进正文）
    await articlesStore.loadHighlights(selectedArticle.value.id)

    // 重新检查状态（防止在 loadHighlights 期间状态变化）
    const currentArticle = selectedArticle.value
    if (currentArticle.has_ai_summary || articlesStore.generating) return
    // 设置里关掉了「AI 摘要」就不再自动发请求——这是用户明确表达的"不要替我花额度"，
    // 手动入口（更多菜单 → 重新生成摘要）不受此限制
    if (!aiSummaryEnabled.value) return

    try {
      await articlesStore.generateSummary(currentArticle.id)
    } catch (e) {
      // 失败原因由 store 写入 aiError，标题区会显示出来，这里只需留痕
      console.error('[ContentColumn] 摘要生成失败:', e)
    }
  },
  { immediate: true } // 立即执行一次，确保初始状态也能触发
)

// 卸载时把最后一次进度落盘：否则丢掉的正是"最后读到的位置"，
// 而它恰恰是最有价值的那一次记录。
// （组件内的临时提示定时器已随各自的功能搬去 ContentToolbar.vue，本组件不再持有 setTimeout 句柄。）
onUnmounted(() => {
  flushProgress()
})

// ─── 正文高亮 ────────────────────────────────────────────────────────────────
//
// 选区捕获、标记渲染、增删确认等逻辑整体收在 composables/useHighlights.ts；
// 这里只解构出模板与点击委托用到的状态和处理器。两块与 DOM 事件强耦合的代码
// 留在宿主：onContentClick 里的删除分发（与链接 / 灯箱共用事件委托），以及
// 上方切换文章 watcher 里的调色板复位与选区快照丢弃。
const {
  showHighlightPalette,
  pendingSelection,
  onContentMouseUp,
  createHighlight,
  cancelHighlight,
  removeHighlight
} = useHighlights({ contentBodyRef, displayContent })

</script>

<style scoped>
.content-column {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
  /* 正文区用 --read-bg 而非 --surface：深色下必须比侧栏更暗一档，
     让阅读区"沉"到界面之下。正文面积是侧栏三倍，与侧栏同色会显得整块贴脸。
     这里也刻意**不做玻璃化**——玻璃的背景光斑干扰会被正文面积放大三倍，
     长文阅读时文字底下有明暗在动。 */
  background: var(--read-bg);
  color: var(--read-fg);
  height: 100%;
  /* 定位上下文：研究事件悬浮按钮（.research-fab-wrap）以此为基准固定在右下角 */
  position: relative;
  border-radius: var(--r-panel);
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    0 2px 6px rgba(31, 45, 70, 0.04),
    0 10px 30px rgba(31, 45, 70, 0.06);
  border: 1px solid var(--glass-hair);
}

.content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: transparent;
}

.content-head {
  padding: var(--sp-4) 0;
  flex-shrink: 0;
  border-bottom: 1px solid var(--border);
  /* 与 .content-column 同一底色：正文区不再是玻璃，这层若留 --surface 会在
     深色下与容器不同色，凭空多出一条分界 */
  background: var(--read-bg);
  max-width: var(--read-max);
  margin: 0 auto;
  width: 100%;
}
.content-title {
  font-size: calc(24px * var(--reading-scale, 1));
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: var(--sp-15);
  line-height: 1.4;
  /* 负字距 + 收紧的字距让标题真正成为视觉中心（原先 600 字重偏"列表项"气质） */
  letter-spacing: -0.02em;
  font-family: var(--reading-font, inherit);
}
.content-meta {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  /* 只保留下方留白，不再自带分隔线：
     外层 .content-head 底部已有一条 border-bottom，两条线相距十几像素会显得重复割裂。
     14px 的作用是让元信息文字与那条外层分隔线之间留出呼吸间距。 */
  padding-bottom: var(--sp-35);
}
.content-source { color: var(--primary); }
.content-time, .content-author { color: var(--text-tertiary); }

.ai-status {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  font-style: italic;
  margin-top: var(--sp-2);
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.ai-status.is-generating {
  color: var(--primary);
}

.ai-status-text {
  white-space: nowrap;
}
.ai-status.error {
  color: var(--primary);
  background: var(--primary-fg);
  border: 1px solid var(--primary);
  border-radius: var(--r-sm);
  padding: var(--sp-05) var(--sp-15);
  font-style: normal;
}
.ai-err-ico { margin-right: var(--sp-1); font-style: normal; }

.content-body-area {
  flex: 1;
  min-height: 0;
  position: relative;
  display: flex;
}
.reading-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  /* 与两个列表同理：补一个滚动条等宽的左侧内边距，
     否则居中阅读列（--read-max）在扣除滚动条后会整体左偏半个滚动条宽度 */
  padding-left: var(--sb-w, var(--sp-1));
}

.content-body {
  overflow-x: auto;
  padding: var(--sp-5) 0;
  max-width: var(--read-max);
  margin: 0 auto;
  font-size: calc(15px * var(--reading-scale, 1));
  line-height: 1.8;
  /* 正文文字色走 --read-fg：它在深色下比 --text-primary 略暗一档，
     大段正文用主色会偏亮偏累。标题等强调层级仍用 --text-primary。 */
  color: var(--read-fg);
  word-wrap: break-word;
  overflow-wrap: break-word;
  font-family: var(--reading-font, inherit);
}
.content-body :deep(p) { margin: 0 0 var(--sp-4); line-height: 1.85; }
/* 标题刻意用 --text-primary 而非 --read-fg：正文已降到略暗的一档，
   标题再与之同色会失去层级。h5/h6 用 secondary，是正文与标题之间的过渡层。 */
.content-body :deep(h1) { font-size: var(--fs-2xl); font-weight: 600; line-height: 1.3; margin: var(--sp-6) 0 var(--sp-3); color: var(--text-primary); }
.content-body :deep(h2) { font-size: var(--fs-lg); font-weight: 500; margin: var(--sp-5) 0 var(--sp-15); color: var(--text-primary); }
.content-body :deep(h3) { font-size: var(--fs-base); font-weight: 500; margin: var(--sp-4) 0 var(--sp-2); color: var(--text-primary); }
.content-body :deep(h4) { font-size: var(--fs-base); font-weight: 500; margin: var(--sp-35) 0 var(--sp-05); color: var(--text-primary); }
.content-body :deep(h5), .content-body :deep(h6) { font-size: var(--fs-sm); font-weight: 500; margin: var(--sp-3) 0 var(--sp-05); color: var(--text-secondary); }
.content-body :deep(strong), .content-body :deep(b) { font-weight: 600; color: var(--text-primary); }
.content-body :deep(em), .content-body :deep(i) { font-style: italic; }
.content-body :deep(a) { color: var(--primary); text-decoration: none; border-bottom: 1px solid var(--primary-fg); }
.content-body :deep(a:hover) { border-bottom-color: var(--primary); }
.content-body :deep(ul), .content-body :deep(ol) { margin: 0 0 var(--sp-35); padding-left: 22px; }
.content-body :deep(li) { margin-bottom: var(--sp-05); }
.content-body :deep(blockquote) {
  border-left: 3px solid var(--primary);
  margin: 18px 0;
  color: var(--text-secondary);
  background: var(--fill);
  border-radius: 0 var(--r-lg) var(--r-lg) 0;
  padding: var(--sp-3) 18px;
  font-size: 0.95em;
  line-height: 1.7;
}
.content-body :deep(img) { max-width: 100%; width: auto; height: auto; display: block; border-radius: var(--r-sm); margin: var(--sp-2) 0; }
.content-body :deep(hr) { border: none; border-top: 1px solid var(--border); margin: var(--sp-5) 0; }
.content-body :deep(code) { font-family: 'Menlo', 'Consolas', monospace; font-size: 0.9em; background: var(--fill); padding: var(--sp-025) 5px; border-radius: var(--r-sm); color: var(--text-primary); }
.content-body :deep(pre) { background: var(--fill); padding: var(--sp-35) var(--sp-4); border-radius: var(--r-md); overflow-x: auto; margin: 0 0 var(--sp-35); font-size: 0.9em; line-height: 1.5; }
.content-body :deep(pre) :deep(code) { background: none; padding: 0; }
.content-body :deep(table) { width: 100%; border-collapse: collapse; margin: 0 0 var(--sp-35); font-size: 0.95em; }
.content-body :deep(th), .content-body :deep(td) { border: 1px solid var(--border); padding: var(--sp-2) var(--sp-15); text-align: left; }
.content-body :deep(th) { background: var(--fill); font-weight: 500; }

.webview-wrap {
  position: absolute;
  inset: 0;
  z-index: 10;
  /* 与文章视图同一底色：iframe 本身已用 --page-canvas（恒白，网页自身假定白底），
     这层只是加载时的底衬，不该在深色下与文章视图不同色。 */
  background: var(--read-bg);
  display: flex;
  flex-direction: column;
}
.webview-iframe { flex: 1; width: 100%; height: 100%; border: 0; background: var(--page-canvas); }
.webview-back {
  position: absolute;
  left: 12px;
  top: 12px;
  z-index: 20;
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-05) var(--sp-15);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  color: var(--text-primary);
  font-size: var(--fs-sm);
  font-family: inherit;
  cursor: pointer;
  box-shadow: var(--shadow-sm);
}
.webview-back:hover { border-color: var(--primary); color: var(--primary); }
.webview-back svg { width: 15px; height: 15px; flex-shrink: 0; }

.lightbox {
  position: fixed;
  inset: 0;
  z-index: 2000;
  background: var(--scrim-heavy);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--sp-10);
  cursor: zoom-out;
  animation: lightboxIn 0.15s ease;
}
@keyframes lightboxIn { from { opacity: 0; } to { opacity: 1; } }
.lightbox img { max-width: 100%; max-height: 100%; object-fit: contain; border-radius: var(--r-md); box-shadow: var(--shadow-pop); cursor: default; }

/* 主空状态：撑满剩余高度并居中，垂直间距比列表型空态略大——
   正文栏是应用的视觉主角，标志与双按钮需要更从容的呼吸感。 */
.empty-state {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  padding: var(--sp-10) var(--sp-5);
  text-align: center;
}
.empty-state .state-logo {
  margin-bottom: 18px;
  opacity: 0.9;
}
.empty-title {
  font-size: var(--fs-xl);
  font-weight: 500;
  color: var(--text-primary);
  margin-bottom: var(--sp-2);
}
.empty-hint {
  font-size: var(--fs-md);
  color: var(--text-tertiary);
  margin-bottom: var(--sp-6);
}
.empty-actions { display: flex; flex-direction: column; align-items: flex-start; gap: var(--sp-15); margin-top: var(--sp-5); }
.empty-action {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  padding: var(--sp-05) var(--sp-3);
  border: none;
  background: none;
  border-radius: var(--r-md);
  font-size: var(--fs-md);
  color: var(--primary);
  cursor: pointer;
  font-family: inherit;
  transition: background 0.15s;
}
.empty-action:hover { background: var(--primary-fg); }
.empty-action svg { width: 14px; height: 14px; flex-shrink: 0; }

/* 移动端：正文占满 */
@media (max-width: 768px) {
  .content-column { width: 100%; min-width: 0; }
}

/* ─── 高亮条目的样式（标记由脚本注入到 v-html 的正文内） ─── */
.content-body :deep(mark[data-highlight-id]) {
  cursor: pointer;
  border-radius: 2px;
  padding: 0 var(--sp-025);
  /* 高亮块内文字用固定的深色：浅底上若沿用暗色主题的浅色文字会看不清 */
  color: var(--hl-fg);
  outline: 1px dashed transparent;
  transition: outline 0.15s;
}
.content-body :deep(mark[data-highlight-id]:hover) {
  outline: 1px dashed currentColor;
}
.content-body :deep(mark[data-hl-color="yellow"]) { background: var(--hl-yellow); }
.content-body :deep(mark[data-hl-color="green"]) { background: var(--hl-green); }
.content-body :deep(mark[data-hl-color="blue"]) { background: var(--hl-blue); }
.content-body :deep(mark[data-hl-color="pink"]) { background: var(--hl-pink); }
</style>
