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
      <div class="rd-doc-wrap" ref="wrapRef">
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
  /**
   * 请求跳到某条引用的原文
   *
   * 第二个参数是"跳转现场"：滚动量只有本组件知道（滚动容器 `.rd-doc-wrap` 在这里），
   * 上层要把它存进来源上下文，回来时才恢复得回去。
   */
  (e: 'open-article', source: PeriodSource, meta: { scrollTop: number }): void
  /** 请求删除这份报告（确认框由上层弹，保证与"覆盖"同一种确认口吻） */
  (e: 'delete'): void
}>()

const store = useReportPeriodStore()
const toast = useToastStore()

/** 正文滚动容器（引用联动的作用域；回退时按它恢复滚动量） */
const wrapRef = ref<HTMLElement | null>(null)
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
 * 顺带把**当前滚动量**带上去：跳走之后本组件可能被卸载/重渲染，而"我刚才滚到哪"
 * 只有这里知道。上层把它存进来源上下文，回退时再喂回来。
 *
 * @param s - 引用源
 */
function onOpenArticle(s: PeriodSource) {
  closeIsland()
  emit('open-article', s, { scrollTop: wrapRef.value?.scrollTop ?? 0 })
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

/** 有界重试的 rAF 句柄（等容器真正有布局盒） */
let restoreRaf: number | null = null
/** 恢复后接管焦点用的 rAF 句柄（卸载时取消，避免回调里访问已卸载的 DOM） */
let restoreFocusRaf: number | null = null

/**
 * 恢复最多尝试几次（同步 1 次 + 两帧重试）
 *
 * # 为什么 3 次就够——阈值取小是有机制的，不是"拍脑袋觉得够快"
 * `getClientRects()` 会**强制同步布局**：只要祖先不是 `display: none`，调用当场就有盒。
 * 于是"大报告渲染卡顿 50ms"表现为**这一帧算得久**，而不会表现为"连续多帧都拿不到布局盒"。
 * 真正能返回空列表的只有两类情形：
 * 1. 元素在 `display: none` 的子树里（上游 `ResearchModal` 已把页签切到 `reports` 提到
 *    所有 await 之前，正常路径第一次同步尝试就过）；
 * 2. 元素还没进文档（`wrapRef === null`，例如阅读页刚挂载）。
 * 两类都会在 2 帧内必然分出结果（要么可见、要么这份报告根本不会渲染——失败态没有
 * `.rd-doc-wrap`、或组件已被卸载）。所以 3 次足够。
 *
 * ⚠ **不要因为"看起来阈值太小"把它调大**：调大只会延长快照滞留窗口，并不会让恢复更可靠。
 */
const RESTORE_MAX_FRAMES = 3

/** 一次位置恢复的快照（与 `store.pendingRestore` 同构） */
type RestoreSnapshot = { refNo: number | null; scrollTop: number }

/**
 * 判断元素是否完整落在某个滚动容器的可视区内
 *
 * @param el - 待判断的目标元素
 * @param container - 滚动容器
 * @returns 完整可见时 true（部分露出也算 false）
 */
function isFullyVisible(el: HTMLElement, container: HTMLElement): boolean {
  const r = el.getBoundingClientRect()
  const c = container.getBoundingClientRect()
  return r.top >= c.top && r.bottom <= c.bottom
}

/**
 * 把快照应用到当前 DOM：滚动量 → 引用高亮 →（必要时）居中 → 下一帧接管焦点
 *
 * 调用前提：`wrap` 已有布局盒（`display: none` 时 `scrollTop` 是空操作、
 * `getBoundingClientRect` 全是 0，本函数会静默做错事，所以由调用方把关）。
 *
 * # 为什么"居中"是有条件的
 * 只有那条引用**不在视野内**时才居中。内容没变过时它本来就在视野里，此时强行居中会把
 * 刚恢复的滚动量改掉——而"清 `ctx` 之后位置仍然对得上"正是这次改动的回归判据
 * （DESIGN §5.5 第 8 条）。
 *
 * # 焦点为什么再晚一帧
 * `BaseModal` 在 `props.open` 变真后的 `nextTick` 里执行 `moveFocusIn`，其微任务排在
 * 恢复之后，会把焦点抢走；晚一帧（rAF，在所有微任务之后）才落得下。
 *
 * @param snapshot - 待恢复的快照
 * @param wrap - 正文滚动容器（`.rd-doc-wrap`）
 */
function applyRestore(snapshot: RestoreSnapshot, wrap: HTMLElement): void {
  wrap.scrollTop = snapshot.scrollTop
  const n = snapshot.refNo
  if (n === null) return
  // `[data-ref]` 由 `utils/refMarker` 生成，是可聚焦的 `<button>`，
  // 天然能 `.focus()`，不需要补 `tabindex`
  const target = docRef.value?.querySelector<HTMLElement>(`[data-ref="${n}"]`) ?? null
  if (!target) return
  flash(n)
  if (!isFullyVisible(target, wrap)) target.scrollIntoView({ block: 'center' })
  if (restoreFocusRaf !== null) cancelAnimationFrame(restoreFocusRaf)
  restoreFocusRaf = requestAnimationFrame(() => {
    restoreFocusRaf = null
    target.focus()
  })
}

/**
 * 回退时恢复"离开现场"：滚动位置 → 原引用高亮 → 焦点落到那个 `[n]`
 *
 * # 为什么只读 `pendingRestore`、绝不读 `sourceContext`
 * 清除 `ctx` 发生在"送达之后"，位置恢复也在"送达之后"，两者天然有先后竞争。
 * 若这里从 `ctx` 取滚动量，`ctx` 先被清就拿到 null，表现为**"回来了但位置丢了"**——
 * 比原问题（回不去）更难查。`ctx` 只管栏的显隐与文案，位置只认这份快照。
 *
 * # 为什么先等"容器有布局盒"，而不是直接写 `scrollTop`
 * 页签是 `v-show` 常驻的：只要 `tab` 还是 `timeline`，整棵子树就是 `display: none`。
 * 在 `display: none` 里写 `scrollTop` 会被钳成 0（空操作），而
 * `getBoundingClientRect()` 两边都是 0 会让 `isFullyVisible` 恒判"可见"从而不补滚——
 * 于是"恢复"看起来执行了，实际什么都没做，随后快照还被消费掉：**永久静默失效**。
 * 所以用 `getClientRects().length > 0` 作为"真的有布局盒"的判据
 * （不用 `offsetParent`：它对 `position: fixed` 元素会是 null，判据不干净）。
 *
 * 上游 `ResearchModal` 已经把 `tab = 'reports'` 提到所有 `await` 之前，正常路径在
 * 第一次尝试时页签就可见；这里的重试是**双保险**，兜住任何"以 display:none 挂载"的场景。
 *
 * # 命中不到 `[data-ref]` 时
 * 静默只恢复滚动，不 flash、不报错：报告可能已被覆盖重生成，编号对不上了，
 * 这不是错误，只是"那条引用没了"。
 *
 * # 为什么带 `immediate`
 * 本组件可能整个被卸载过（`detail` 被清空、或将来弹窗改成 `v-if`），重新挂载时
 * 要能把还挂着的快照捡起来——否则"降级重放 + 位置恢复"那条路会静默失效。
 */
watch(
  () => store.pendingRestore,
  snapshot => {
    if (!snapshot) return
    // 上一轮重试若还没跑完，先取消：否则它会在下一帧把**旧快照**应用上去
    if (restoreRaf !== null) cancelAnimationFrame(restoreRaf)
    let frames = 0
    /**
     * 试一次；没有布局盒就排到下一帧再试（有界）
     *
     * @returns 无返回值；成功或放弃时都会 `consumeRestore()`
     */
    const attempt = (): void => {
      restoreRaf = null
      const wrap = wrapRef.value
      if (wrap && wrap.getClientRects().length > 0) {
        applyRestore(snapshot, wrap)
        store.consumeRestore()
        return
      }
      frames += 1
      if (frames >= RESTORE_MAX_FRAMES) {
        // 放弃前留痕：用户的阅读位置在这里被吞掉，线上出问题时总得有线索可查
        console.warn('[report] 位置恢复放弃：容器始终无布局盒', { refNo: snapshot.refNo })
        // 放弃，但**必须消费**：留着会在下次打开任意报告时被误用
        // （报告被滚到别人的位置、还闪了一个不属于它的编号）
        store.consumeRestore()
        return
      }
      restoreRaf = requestAnimationFrame(attempt)
    }
    // 先同步试一次：容器若已就绪，不必白等一帧（否则用户能看到"先停在顶部再跳下去"）
    attempt()
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  if (flashTimer) clearTimeout(flashTimer)
  if (restoreRaf !== null) cancelAnimationFrame(restoreRaf)
  if (restoreFocusRaf !== null) cancelAnimationFrame(restoreFocusRaf)
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
  background: var(--primary-tint);
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
  background: var(--primary-tint);
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
  background: var(--primary-tint);
  color: var(--primary);
}
.ref-open:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
</style>
