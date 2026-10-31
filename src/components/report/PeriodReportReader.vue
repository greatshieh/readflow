<template>
  <div class="reader">
    <!-- 固定头：返回 + 标题 + 元信息 + KPI 带。不参与正文滚动，
         这样长报告滚到一半也能一眼看到"这是哪一期、底料有多厚"。 -->
    <header class="rd-head">
      <div class="rd-crumb">
        <button class="link" type="button" @click="emit('back')">← 报告列表</button>
        <span class="spacer"></span>
        <button
          v-if="!failed"
          class="btn-ghost"
          type="button"
          @click="openExport"
          aria-haspopup="menu"
          :aria-expanded="exportOpen"
        >
          导出
        </button>
        <!-- 删除是"清掉这条记录"的唯一出口：某期文章后来被清理后 coverage 归零，
             生成按钮会被禁用，那条失败记录否则永远留在列表里 -->
        <button class="btn-ghost" type="button" @click="emit('delete')">删除</button>
      </div>
      <h1 ref="titleRef" class="rd-title" tabindex="-1">{{ detail.summary.title }}</h1>
      <div class="rd-meta">
        <span>{{ kindText }}</span>
        <span class="dot">·</span>
        <span>{{ periodText }}</span>
        <span class="dot">·</span>
        <span>{{ detail.summary.createdAt }} 生成</span>
        <template v-if="detail.summary.model">
          <span class="dot">·</span>
          <span>{{ detail.summary.model }}</span>
        </template>
      </div>
      <!-- KPI 带：失败时全是 0，展示出来只会误导，直接不渲染 -->
      <div v-if="!failed" class="kpis">
        <div class="kpi">
          <b>{{ detail.summary.articleCount }}</b><span>纳入文章</span>
        </div>
        <div class="kpi">
          <b>{{ detail.summary.topicCount }}</b><span>归并主题</span>
        </div>
        <div class="kpi">
          <b>{{ detail.summary.feedCount }}</b><span>覆盖订阅源</span>
        </div>
        <div class="kpi">
          <b>{{ detail.sources.length }}</b><span>引用源</span>
        </div>
      </div>
    </header>

    <!-- 失败态：正文是空的，硬渲染只会得到一片白。
         直接把原因和下一步写清楚，用户不必猜"是不是还在生成"。 -->
    <div v-if="failed" class="rd-failed">
      <div class="rd-failed-title">这份{{ kindText }}没有生成成功</div>
      <p class="rd-failed-msg">{{ detail.summary.error || '模型没有返回可用的内容。' }}</p>
      <p class="rd-failed-hint">回到列表点「重新生成」再试一次；同一期会被覆盖，不会多出一条。</p>
    </div>

    <!-- 阅读区：正文 + 引用源右栏，两列各自独立滚动 -->
    <div v-else class="rd-body">
      <div class="rd-doc-wrap">
        <article ref="docRef" class="rc rd-doc" v-html="html" @click="onDocClick"></article>

        <!-- 本期值得跟进：正文 md 里也有一节同名段落（导出到 Obsidian 后仍可读），
             这里额外渲染一遍是因为**md 里不能放按钮**。
             两份同源（都来自 report_followups），所以不会出现内容打架；
             差别只在能力：md 是给人读的，这里是给人点的。 -->
        <section v-if="detail.followups.length > 0" class="rf" aria-label="本期值得跟进">
          <div class="rf-head">
            <h2>接下来盯什么</h2>
            <span class="rf-n">{{ detail.followups.length }}</span>
          </div>
          <p class="rf-tip">
            这些不是因为「出现了新闻」，而是因为出现了变化。点「跟踪」才会加进你的实体库——
            不加就不占位。
          </p>
          <ul class="rf-list">
            <li v-for="f in detail.followups" :key="f.id" class="rf-item">
              <div class="rf-main">
                <span class="rf-name">{{ f.entityName }}</span>
                <span class="rf-reason">{{ f.reason }}</span>
              </div>
              <button
                v-if="f.tracked"
                class="rf-btn done"
                type="button"
                disabled
                :title="`已在实体库中（ID ${f.entityId}）`"
              >
                已跟踪
              </button>
              <button
                v-else
                class="rf-btn"
                type="button"
                :disabled="trackingId === f.id"
                @click="onTrack(f.id)"
              >
                {{ trackingId === f.id ? '添加中…' : '跟踪' }}
              </button>
            </li>
          </ul>
        </section>
      </div>

      <aside class="rd-src" aria-label="引用源">
        <div class="rd-src-head">
          <h2>引用源</h2>
          <span>{{ detail.sources.length }}</span>
        </div>
        <div v-if="detail.sources.length === 0" class="rd-src-empty">这份报告没有引用任何文章</div>
        <div v-else class="rd-src-list" ref="srcListRef">
          <div
            v-for="s in detail.sources"
            :key="s.refNo"
            class="ref-item"
            :class="{ flash: flashSource === s.refNo }"
            :data-ref-item="s.refNo"
            tabindex="0"
            role="button"
            :aria-label="`定位到正文中的第 ${s.refNo} 条引用`"
            @click="jumpToRef(s.refNo)"
            @keydown="onRefKey($event, s.refNo)"
          >
            <span class="ref-no">{{ s.refNo }}</span>
            <div class="ref-body">
              <div class="ref-title">{{ s.title }}</div>
              <div class="ref-src">{{ srcMeta(s) }}</div>
            </div>
            <button
              class="ref-open"
              type="button"
              aria-label="预览这篇原文"
              :title="s.articleId === null ? '原文已不在库中' : '预览'"
              @click.stop="openIsland(s, $event)"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M7 17 17 7" />
                <path d="M9 7h8v8" />
              </svg>
            </button>
          </div>
        </div>
      </aside>
    </div>

    <!-- 预览浮岛 -->
    <SourcePreviewIsland
      v-if="islandSource && islandAnchor"
      :source="islandSource"
      :anchor="islandAnchor"
      @close="closeIsland"
      @open="onOpenArticle(islandSource)"
    />

    <!-- 导出菜单（与列表卡片共用同一组件） -->
    <ReportExportMenu
      v-if="exportOpen && exportAnchor"
      :anchor="exportAnchor"
      :target="{ kind: 'period', id: detail.summary.id, label: detail.summary.title }"
      @close="exportOpen = false"
    />
  </div>
</template>

<script setup lang="ts">
/**
 * 周期报告阅读页
 *
 * # 职责
 * 渲染一份周期报告：固定头（标题/元信息/KPI）+ 正文（Markdown 渲染）+ 引用源右栏，
 * 并实现三处联动：
 * 1. 点正文里的 `[n]` → 高亮右栏对应条目并滚到它；
 * 2. 点右栏条目 → 高亮正文里的 `[n]` 并滚到它；
 * 3. 点右栏 `↗` → 预览浮岛 → 「打开全文」交给上层（写来源上下文 + 跳文章 + 关弹窗）。
 *
 * # 正文渲染
 * 复用 `utils/markdown.ts`：与研究报告**同一个渲染器**，导出也用它，因此三处版式必然一致。
 * 渲染后由 `utils/refMarker.ts` 把纯文本 `[n]` 换成可点按钮——Markdown 源里保持纯文本，
 * 导出的 md/html 才不会留下一堆点不动的按钮。
 *
 * # 焦点
 * 打开报告时把焦点移到标题（`tabindex="-1"`）：报告卡片此时已被卸载，
 * 不收拢焦点会让键盘用户的 Tab 从页面开头重来。
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { renderMarkdown } from '@/utils/markdown'
import { markRefs } from '@/utils/refMarker'
import { formatLocalStamp, periodKindLabel, type PeriodKind } from '@/utils/periodWindow'
import { useReportPeriodStore } from '@/stores/reportPeriod'
import { useToastStore } from '@/stores/toast'
import type { PeriodDetail, PeriodSource } from '@/stores/reportPeriod'
import SourcePreviewIsland from './SourcePreviewIsland.vue'
import ReportExportMenu from './ReportExportMenu.vue'
import '@/styles/report-content.css'

const props = defineProps<{
  /** 报告完整内容 */
  detail: PeriodDetail
}>()

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'open-article', source: PeriodSource): void
  /** 请求删除这份报告（确认框由上层弹，保证与"覆盖"同一种确认口吻） */
  (e: 'delete'): void
}>()

const store = useReportPeriodStore()
const toast = useToastStore()

/** 正文容器（引用联动的作用域） */
const docRef = ref<HTMLElement | null>(null)
/** 标题（打开后接管焦点） */
const titleRef = ref<HTMLElement | null>(null)
/** 引用源列表容器（页内定位的作用域） */
const srcListRef = ref<HTMLElement | null>(null)

/** 正在闪的引用编号（正文与右栏同步；1.4s 后清空） */
const flashSource = ref<number | null>(null)
let flashTimer: ReturnType<typeof setTimeout> | null = null

/** 预览浮岛：当前文章与锚点 */
const islandSource = ref<PeriodSource | null>(null)
const islandAnchor = ref<{ x: number; y: number } | null>(null)

/** 导出菜单 */
const exportOpen = ref(false)
const exportAnchor = ref<{ x: number; y: number } | null>(null)

/** 正文 HTML（Markdown → 安全 HTML） */
const html = computed(() => renderMarkdown(props.detail.markdown))

/** 正在「跟踪」的跟进行 ID（0 = 空闲），用于按钮禁用与文案 */
const trackingId = ref(0)

/**
 * 把一条「值得跟进」加进实体库
 *
 * 写入由 store 完成（就地更新 detail.followups，不整份重拉）：
 * 重拉会让阅读区滚动位置跳动，而用户此刻正停在跟进段落上。
 *
 * @param id - `report_followups` 行 ID
 */
async function onTrack(id: number) {
  if (trackingId.value !== 0) return
  trackingId.value = id
  try {
    await store.trackFollowup(id)
    toast.show('已加入实体库，可在「实体管理」里调整类型与告警规则')
  } catch (e) {
    toast.show(e instanceof Error ? e.message : String(e))
  } finally {
    trackingId.value = 0
  }
}


/**
 * 是否为生成失败的残留
 *
 * 失败行也会被列表点开（用户要能看到"为什么没成"），此时正文是空的：
 * 不特判就会渲染出一片白，看起来像卡住了。
 */
const failed = computed(() => props.detail.summary.status === 'failed')

/** 粒度中文名 */
const kindText = computed(() => periodKindLabel(props.detail.summary.kind as PeriodKind))

/** 周期展示（用报告自己的窗口起止，不用"当前浏览期"——翻页后两者会不同） */
const periodText = computed(() => {
  const from = new Date(props.detail.summary.periodStart)
  const to = new Date(props.detail.summary.periodEnd)
  to.setDate(to.getDate() - 1)
  const p = (n: number) => String(n).padStart(2, '0')
  if (props.detail.summary.kind === 'monthly') {
    return `${from.getFullYear()}年${from.getMonth() + 1}月`
  }
  if (props.detail.summary.kind === 'weekly') {
    return `${p(from.getMonth() + 1)}月${p(from.getDate())}日 – ${p(to.getMonth() + 1)}月${p(to.getDate())}日`
  }
  return `${from.getFullYear()}年${from.getMonth() + 1}月${from.getDate()}日`
})

/**
 * 引用源副标题：来源 · 时间
 *
 * @param s - 引用源
 * @returns 展示串
 */
function srcMeta(s: PeriodSource): string {
  const parts = [s.feedName, s.published ? formatLocalStamp(s.published) : ''].filter(Boolean)
  return parts.length ? parts.join(' · ') : '来源未知'
}

/**
 * 高亮一次引用（正文按钮与右栏条目同步闪）
 *
 * 两侧都要显式加/撤类：右栏是 Vue 渲染的（靠 `flashSource` 驱动），
 * 而正文里的按钮是 `v-html` + DOM 替换产物，Vue 管不到它，只能手动 toggle。
 *
 * @param n - 引用编号
 */
function flash(n: number) {
  // 编号必须是有限数字。后端 `PeriodSource` 若漏了 camelCase 重命名，`refNo` 会是
  // `undefined`，而 `flashSource === s.refNo` 在两侧同为 undefined 时对**每一个**条目
  // 都成立 —— 表现为"点一条却全部高亮，且怎么点都选不中"。这里早退，让契约破裂时
  // 只表现为"没反应"，不会误导成选择状态。
  if (!Number.isFinite(n)) return
  docRef.value?.querySelector(`[data-ref="${n}"]`)?.classList.add('is-flash')
  flashSource.value = n
  if (flashTimer) clearTimeout(flashTimer)
  flashTimer = setTimeout(() => {
    docRef.value?.querySelector(`[data-ref="${n}"]`)?.classList.remove('is-flash')
    flashSource.value = null
    flashTimer = null
  }, 1400)
}

/**
 * 右栏条目的键盘激活
 *
 * `e.target !== e.currentTarget` 早退：条目里嵌着「预览」按钮，
 * 在按钮上按 Enter 会冒泡到这里，不拦就变成"预览的同时还跳正文"。
 *
 * @param e - 键盘事件
 * @param n - 引用编号
 */
function onRefKey(e: KeyboardEvent, n: number) {
  if (e.target !== e.currentTarget) return
  if (e.key !== 'Enter' && e.key !== ' ') return
  e.preventDefault()
  jumpToRef(n)
}

/**
 * 右栏条目 → 正文对应的 `[n]`
 *
 * @param n - 引用编号
 */
function jumpToRef(n: number) {
  flash(n)
  const target = docRef.value?.querySelector<HTMLElement>(`[data-ref="${n}"]`)
  target?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

/**
 * 正文 `[n]` → 右栏对应条目
 *
 * 用事件委托而不是给每个按钮绑监听：`v-html` 生成的节点在每次重渲染时都会重建，
 * 逐个绑定会在重渲染后全部失效。
 *
 * @param e - 点击事件
 */
function onDocClick(e: MouseEvent) {
  const btn = (e.target as HTMLElement).closest<HTMLElement>('.rc-ref')
  const n = Number(btn?.dataset.ref ?? '')
  if (!btn || !Number.isFinite(n)) return
  flash(n)
  const item = srcListRef.value?.querySelector<HTMLElement>(`[data-ref-item="${n}"]`)
  item?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
}

/**
 * 打开预览浮岛
 *
 * @param s - 引用源
 * @param e - 触发按钮事件（取其视口坐标做锚点）
 */
function openIsland(s: PeriodSource, e: MouseEvent) {
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  islandAnchor.value = { x: rect.left, y: rect.top }
  islandSource.value = s
}

/** 关闭浮岛 */
function closeIsland() {
  islandSource.value = null
  islandAnchor.value = null
}

/**
 * 浮岛「打开全文」
 *
 * @param s - 引用源
 */
function onOpenArticle(s: PeriodSource) {
  closeIsland()
  emit('open-article', s)
}

/**
 * 打开导出菜单
 *
 * @param e - 触发按钮事件（菜单贴它下方）
 */
function openExport(e: MouseEvent) {
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  exportAnchor.value = { x: rect.right - 224, y: rect.bottom }
  exportOpen.value = true
}

/**
 * 正文渲染完成后把 `[n]` 标记成按钮，并接管焦点
 *
 * 必须在 DOM 更新后做（`nextTick`），否则 `v-html` 还没写入，
 * `markRefs` 遍历的是一个空容器。
 *
 * 点击监听不在这里绑：绑在 `<article @click>` 上（Vue 委托），
 * 否则每换一份报告就多叠一个 `addEventListener`，而节点始终是同一个。
 */
async function decorate() {
  await nextTick()
  const root = docRef.value
  if (root) markRefs(root)
  titleRef.value?.focus()
}

// 进入 / 换报告时重新装饰（detail 变化说明换了另一份报告）
watch(
  () => props.detail.summary.id,
  () => {
    void decorate()
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  if (flashTimer) clearTimeout(flashTimer)
})
</script>

<style scoped>
/* ── 整体：头固定 + 双列阅读区 ───────────────────────────────────────── */
.reader {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
}
.rd-head {
  flex-shrink: 0;
  padding-bottom: var(--sp-3);
}
.rd-crumb {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-15);
}
.spacer {
  flex: 1 1 auto;
  min-width: 0;
}
.link {
  border: none;
  background: none;
  padding: 0;
  color: var(--primary);
  font-family: inherit;
  font-size: var(--fs-modal-label);
  cursor: pointer;
}
.link:hover {
  text-decoration: underline;
}
.link:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  border-radius: 3px;
}
.btn-ghost {
  height: 28px;
  padding: 0 12px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--fs-modal-label);
  font-weight: 500;
  cursor: pointer;
  transition: background var(--m-tap, 90ms) ease-out;
}
.btn-ghost:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.btn-ghost:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.rd-title {
  margin: 0 0 6px;
  font-size: var(--fs-modal-title);
  font-weight: 600;
  line-height: var(--lh-modal-title);
  color: var(--text-primary);
  outline: none;
}
.rd-meta {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  align-items: center;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  margin-bottom: var(--sp-3);
}
.rd-meta .dot {
  opacity: 0.6;
}
.kpis {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sp-2);
}
.kpi {
  background: var(--fill);
  border-radius: var(--r-md);
  padding: 8px 11px;
}
.kpi b {
  display: block;
  font-size: var(--fs-modal-num);
  font-weight: 700;
  line-height: var(--lh-modal-num);
  font-variant-numeric: tabular-nums;
  color: var(--text-primary);
}
.kpi span {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}

/* ── 双列：正文与引用源各自滚动 ─────────────────────────────────────── */
.rd-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 246px;
  gap: var(--sp-4);
  min-height: 0;
  flex: 1;
}

/* ── 失败态 ─────────────────────────────────────────────────────────── */
.rd-failed {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  align-items: flex-start;
  padding: var(--sp-4);
  border: 1px dashed color-mix(in srgb, var(--danger) 45%, var(--border));
  border-radius: var(--r-lg);
  background: color-mix(in srgb, var(--danger) 4%, transparent);
}
.rd-failed-title {
  font-size: var(--fs-modal-section);
  font-weight: 600;
  color: var(--danger);
}
.rd-failed-msg {
  margin: 0;
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--text-primary);
  word-break: break-word;
}
.rd-failed-hint {
  margin: 0;
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
}
.rd-doc-wrap {
  min-width: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding-right: var(--sp-2);
  padding-bottom: var(--sp-6);
}
.rd-doc {
  /* 正文行宽上限：中文舒适行宽 30–45 字；弹窗加宽后不给上限会一路拉到 600+ px，
     每行 40 多个字反而更难读 */
  max-width: 34em;
}

/* ═══ 接下来盯什么（正文之后的行动区）═══
   宽度跟正文一致（34em）：它读起来是正文的最后一段，不是右侧的侧栏。 */
.rf {
  max-width: 34em;
  margin-top: var(--sp-5);
  padding: var(--sp-4);
  border-radius: var(--r-lg);
  background: var(--fill);
  border: 1px solid var(--border);
}
.rf-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-15);
}
.rf-head h2 {
  margin: 0;
  font-size: var(--fs-lg);
  font-weight: 600;
  color: var(--text-primary);
}
.rf-n {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  background: var(--surface);
  border-radius: 999px;
  padding: 1px 8px;
}
.rf-tip {
  margin: 0 0 var(--sp-3);
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-secondary);
}
.rf-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.rf-item {
  display: flex;
  align-items: flex-start;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-15);
  background: var(--surface);
  border-radius: var(--r-md);
}
.rf-main {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.rf-name {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
}
.rf-reason {
  font-size: var(--fs-sm);
  line-height: 1.55;
  color: var(--text-secondary);
}
.rf-btn {
  flex-shrink: 0;
  align-self: center;
  border: 1px solid var(--primary);
  background: transparent;
  color: var(--primary);
  cursor: pointer;
  font-family: inherit;
  font-size: var(--fs-sm);
  padding: 3px var(--sp-15);
  border-radius: var(--r-sm);
  white-space: nowrap;
}
.rf-btn:hover:not(:disabled) {
  background: var(--primary-fg);
}
.rf-btn:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.rf-btn:disabled {
  cursor: default;
  opacity: 0.55;
}
/* 已跟踪：去掉边框与主题色，让它明确"不用再点了" */
.rf-btn.done {
  border-color: var(--border);
  color: var(--text-tertiary);
  opacity: 1;
}

/* 引用源右栏 */
.rd-src {
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-left: 1px solid var(--border);
  padding-left: var(--sp-3);
}
.rd-src-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 2px 0 var(--sp-2);
  flex-shrink: 0;
}
.rd-src-head h2 {
  margin: 0;
  font-size: var(--fs-modal-label);
  font-weight: 600;
  color: var(--text-primary);
}
.rd-src-head span {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
.rd-src-empty {
  font-size: var(--fs-modal-label);
  color: var(--text-tertiary);
  padding: var(--sp-2) 0;
}
.rd-src-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-bottom: var(--sp-4);
}
.ref-item {
  display: flex;
  gap: var(--sp-2);
  align-items: flex-start;
  padding: 7px 6px;
  border: 1px solid transparent;
  border-radius: var(--r-sm);
  cursor: pointer;
  transition:
    background var(--m-tap, 90ms) ease-out,
    border-color var(--m-tap, 90ms) ease-out;
}
.ref-item:hover {
  background: var(--fill);
  border-color: var(--border);
}
.ref-item:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}
.ref-item.flash {
  background: var(--primary-fg);
  border-color: var(--primary);
}
.ref-no {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  border-radius: 5px;
  border: 1px solid var(--border);
  background: var(--surface);
  display: grid;
  place-items: center;
  font-size: 10px;
  font-weight: 600;
  color: var(--primary);
  font-variant-numeric: tabular-nums;
}
.ref-body {
  min-width: 0;
  flex: 1;
}
.ref-title {
  font-size: var(--fs-modal-label);
  line-height: 1.45;
  color: var(--text-primary);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.ref-src {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
/* `↗` 只有一个图标，命中区补到 24×24（WCAG 2.5.8 下限的舒适区） */
.ref-open {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: none;
  background: none;
  border-radius: var(--r-sm);
  color: var(--text-tertiary);
  cursor: pointer;
}
.ref-open svg {
  width: 12px;
  height: 12px;
}
.ref-open:hover {
  background: var(--primary-fg);
  color: var(--primary);
}
.ref-open:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
</style>
