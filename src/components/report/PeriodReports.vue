<template>
  <div class="rp-pane">
    <!-- ═══ 阅读态：整块换成阅读页（工具栏收起来，避免"在读某期时还能改浏览期"的错位） ═══ -->
    <PeriodReportReader
      v-if="store.detail"
      :detail="store.detail"
      @back="store.closeReport()"
      @open-article="onOpenArticle"
      @delete="onDelete"
    />

    <!-- ═══ 生成态：进度卡（阻塞：关闭被拦时由外壳给一次 toast） ═══ -->
    <div v-else-if="store.generating" class="rp-gen">
      <div class="gen-card">
        <div class="gen-head">
          <span class="spin" aria-hidden="true"></span>
          <div>
            <div class="gen-title">正在生成 {{ store.windowLabel }} {{ kindLabel }}</div>
            <div class="gen-sub">
              {{ genSub }}
            </div>
          </div>
        </div>
        <div class="bar" role="progressbar" :aria-valuenow="store.progressPercent" aria-valuemin="0" aria-valuemax="100">
          <i :style="{ width: `${store.progressPercent}%` }"></i>
        </div>
        <div class="steps">
          <div v-for="s in STEPS" :key="s.key" class="step" :class="stepState(s.key)">
            <span class="tick">{{ stepState(s.key) === 'done' ? '✓' : '' }}</span>
            <span>{{ s.label }}</span>
            <span v-if="s.key === 'collect' && store.stage === 'collect' && store.stageTotal > 0" class="step-n">
              {{ store.stageDone }} 篇
            </span>
            <span v-else-if="s.key === 'sources' && store.stage === 'sources' && store.stageTotal > 0" class="step-n">
              {{ store.stageTotal }} 条
            </span>
          </div>
        </div>
        <div class="gen-foot">
          <span class="gen-hint">生成期间暂不能关闭窗口（Esc 会提示）</span>
          <button class="btn btn-ghost" type="button" @click="store.cancelGenerate()">取消生成</button>
        </div>
      </div>
    </div>

    <!-- ═══ 列表态 ═══ -->
    <template v-else>
      <!-- 工具条：粒度 / 周期翻页 / 生成 -->
      <div class="rp-toolbar">
        <div class="grain" role="group" aria-label="报告粒度">
          <button
            v-for="g in GRAINS"
            :key="g.value"
            type="button"
            :aria-pressed="store.kind === g.value"
            @click="store.setKind(g.value)"
          >
            {{ g.label }}
          </button>
        </div>

        <div v-if="store.windowMode === 'natural'" class="period">
          <button class="nav" type="button" aria-label="上一期" @click="store.shift(-1)">‹</button>
          <button class="cur" type="button" :title="`切换到自定义区间`" @click="toggleCustom">
            {{ store.windowLabel }}
            <span class="sub">自定义</span>
          </button>
          <button class="nav" type="button" aria-label="下一期" :disabled="!store.canGoNext" @click="store.shift(1)">
            ›
          </button>
        </div>
        <button v-else class="period cur solo" type="button" @click="toggleCustom" title="回到自然周期">
          {{ store.windowLabel }} <span class="sub">自然周期</span>
        </button>

        <span class="spacer"></span>
        <button
          class="btn btn-primary"
          type="button"
          :disabled="!canGenerate"
          :title="generateHint"
          @click="onGenerate"
        >
          {{ store.generateLabel }}
        </button>
      </div>

      <!-- 自定义区间：只在切到自定义口径时展开，不常驻占位 -->
      <div v-if="store.windowMode === 'custom'" class="range-row">
        <label>
          起
          <input v-model="store.customStart" type="date" @change="store.loadCoverage()" />
        </label>
        <label>
          止
          <input v-model="store.customEnd" type="date" @change="store.loadCoverage()" />
        </label>
        <span class="range-note">两端都含；按本地时区切日</span>
      </div>

      <!-- 空态：当前浏览期还没有报告。先把"有没有素材"告诉用户，
           避免点了生成才发现 0 篇（此时按钮已禁用并说明原因）。 -->
      <div v-if="!store.currentReport" class="rp-empty">
        <div class="empty-title">{{ emptyTitle }}</div>
        <p class="empty-fact">{{ emptyFact }}</p>
        <button class="btn btn-primary" type="button" :disabled="!canGenerate" @click="onGenerate">
          {{ store.generateLabel }}
        </button>
      </div>

      <div v-if="store.listError" class="rp-error">{{ store.listError }}</div>

      <div v-if="store.list.length > 0" class="rp-list">
        <div class="rp-list-head">
          <span>最近 {{ store.list.length }} 份{{ kindLabel }}</span>
          <span class="hint">按周期倒序</span>
        </div>
        <PeriodReportCard
          v-for="r in store.list"
          :key="r.id"
          :report="r"
          :active="store.currentReport?.id === r.id"
          @open="store.openReport(r.id)"
          @export="anchor => openExport(r, anchor)"
          @regenerate="onRegenerate(r)"
        />
      </div>
      <div v-else-if="!store.listLoading && !store.currentReport" class="rp-first">
        还没有任何{{ kindLabel }}。上面点「{{ store.generateLabel }}」即刻生成第一份。
      </div>

      <!-- 导出菜单（与阅读页共用同一组件，首行写明作用对象） -->
      <ReportExportMenu
        v-if="exportTarget && exportAnchor"
        :anchor="exportAnchor"
        :target="exportTarget"
        @close="closeExport"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
/**
 * 周期报告面板（列表 / 空态 / 生成中 / 阅读）
 *
 * # 职责
 * 「报告」页签里 `周期报告` 那一段的全部界面与编排：
 * - 选粒度（日/周/月）与浏览期（自然周期翻页 / 自定义区间）；
 * - 列表、空态、生成进度、阅读四态切换；
 * - 把"打开全文"翻译成一次跨组件的跳转（写来源上下文 + 选文章 + 关弹窗）。
 *
 * # 四态为什么是互斥的（不是叠加）
 * 阅读时要隐藏工具栏：一边读 10-30 的日报、一边箭头还停在 10-28 上，
 * 会让人以为正在读的是 10-28。生成时同理，进度卡独占整块区域，
 * 也顺带解释了"为什么这时点不动别的"。
 *
 * # 生成是阻塞的
 * P0–P2 不做后台任务框架（决策⑬）：生成期间弹窗不可关，Esc/✕ 由外壳给一次
 * toast 反馈。取消走 AI 请求取消键，不留半份报告。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { showConfirm } from '@/stores/modal'
import { useReportPeriodStore, type PeriodSummary, type PeriodSource } from '@/stores/reportPeriod'
import { useArticlesStore } from '@/stores/articles'
import { useUiStore } from '@/stores/ui'
import { useToastStore } from '@/stores/toast'
import { periodKindLabel, type PeriodKind } from '@/utils/periodWindow'
import type { ExportTarget } from '@/utils/reportExport'
import PeriodReportCard from './PeriodReportCard.vue'
import PeriodReportReader from './PeriodReportReader.vue'
import ReportExportMenu from './ReportExportMenu.vue'

const props = defineProps<{
  /** 弹窗是否打开（打开时刷新列表与素材量） */
  open: boolean
}>()

const emit = defineEmits<{
  /** 请求关闭研究工作台弹窗（跳文章时用） */
  (e: 'close'): void
}>()

const store = useReportPeriodStore()
const articlesStore = useArticlesStore()
const uiStore = useUiStore()
const toast = useToastStore()

/** 三个粒度 */
const GRAINS: { value: PeriodKind; label: string }[] = [
  { value: 'daily', label: '日报' },
  { value: 'weekly', label: '周报' },
  { value: 'monthly', label: '月报' },
]

/** 生成阶段（与后端 stage 一一对应，顺序即推进顺序） */
const STEPS = [
  { key: 'collect', label: '收集窗口内文章' },
  { key: 'cluster', label: '归并主题' },
  { key: 'compose', label: '撰写要点与主题' },
  { key: 'sources', label: '生成引用源清单' },
] as const

/** 当前粒度中文名 */
const kindLabel = computed(() => periodKindLabel(store.kind))

/** 生成副标题：跟随阶段文案 */
const genSub = computed(() => {
  const parts = [store.stageLabel]
  if (store.stageTotal > 0) parts.push(`${store.stageDone} / ${store.stageTotal}`)
  parts.push('约 30–60 秒')
  return parts.join(' · ')
})

/**
 * 计算某阶段的状态：done / active / todo
 *
 * @param key - 阶段标识
 * @returns 状态串
 */
function stepState(key: string): 'done' | 'active' | 'todo' {
  const order = STEPS.map(s => s.key) as readonly string[]
  const i = order.indexOf(key)
  const cur = order.indexOf(store.stage)
  if (i < cur) return 'done'
  if (i === cur) return 'active'
  return 'todo'
}

/** 当前期有没有文章（决定生成按钮可用性） */
const articleCount = computed(() => store.coverage?.articles ?? 0)

/** 能否生成 */
const canGenerate = computed(
  () => articleCount.value > 0 && !store.generating && store.currentRange() !== null,
)

/** 按钮禁用原因（写在 title 里，别让用户猜） */
const generateHint = computed(() => {
  if (store.currentRange() === null) return '请先选择完整的自定义区间'
  if (articleCount.value === 0) return '该周期没有新文章'
  return ''
})

/** 空态标题 */
const emptyTitle = computed(() => {
  if (store.currentRange() === null) return '请选择要生成的区间'
  return store.kind === 'daily'
    ? '这一天还没有报告'
    : store.kind === 'weekly'
      ? '这一周还没有报告'
      : '这一个月还没有报告'
})

/** 空态事实句：把"有没有素材"直接说出来 */
const emptyFact = computed(() => {
  if (store.currentRange() === null) return '选好起止日期后即可生成。'
  const n = articleCount.value
  if (n === 0) return `${store.windowLabel} 没有收到任何文章，换个周期试试。`
  return `${store.windowLabel} 共收到 ${n} 篇文章，覆盖 ${store.coverage?.feeds ?? 0} 个订阅源，够生成一份${kindLabel.value}。`
})

/** 导出：目标与锚点 */
const exportTarget = ref<ExportTarget | null>(null)
const exportAnchor = ref<{ x: number; y: number } | null>(null)

/**
 * 打开导出菜单
 *
 * @param report - 目标报告
 * @param anchor - 触发按钮坐标
 */
function openExport(report: PeriodSummary, anchor: { x: number; y: number }) {
  exportTarget.value = {
    kind: 'period',
    id: report.id,
    label: report.title || `${kindLabel.value} ${report.createdAt}`,
  }
  exportAnchor.value = anchor
}

/** 关闭导出菜单 */
function closeExport() {
  exportTarget.value = null
  exportAnchor.value = null
}

/** 在自然周期与自定义区间之间切换 */
function toggleCustom() {
  store.setWindowMode(store.windowMode === 'natural' ? 'custom' : 'natural')
}

/**
 * 生成当前窗口的报告
 *
 * 失败信息写进 toast（不占界面）：进度卡已经收起，留一个红色区块会长期占位。
 */
async function onGenerate() {
  try {
    await store.generate()
  } catch (e) {
    toast.show(e instanceof Error ? e.message : String(e), { duration: 6000 })
  }
}

/**
 * 重新生成某一期（覆盖）
 *
 * 必须明说会覆盖：同一周期在 `UNIQUE` 约束下只能有一份，**没有历史版本**
 * （`CONFLICTS.md` §2.2）。无声覆盖是信任级缺陷。
 *
 * @param report - 目标报告
 */
async function onRegenerate(report: PeriodSummary) {
  const ok = await showConfirm({
    title: '覆盖并重新生成？',
    message: `将覆盖「${report.title}」这一期已有的内容，原内容不再保留。`,
  })
  if (!ok) return
  // 把浏览期对齐到这份报告的窗口，generate 才会覆盖同一行而不是新建一期
  store.kind = (report.kind as PeriodKind) ?? store.kind
  store.windowMode = report.windowMode === 'custom' ? 'custom' : 'natural'
  store.anchor = new Date(report.periodStart)
  if (store.windowMode === 'custom') {
    const p = (n: number) => String(n).padStart(2, '0')
    const from = new Date(report.periodStart)
    const to = new Date(new Date(report.periodEnd).getTime() - 86400000)
    store.customStart = `${from.getFullYear()}-${p(from.getMonth() + 1)}-${p(from.getDate())}`
    store.customEnd = `${to.getFullYear()}-${p(to.getMonth() + 1)}-${p(to.getDate())}`
  }
  await onGenerate()
}

/**
 * 删除正在读的这份报告
 *
 * 删除是"清掉这条记录"，不是"取消生成"，所以要走确认框：
 * 同一期没有历史版本，删了只能重新调模型再生成一遍。
 * 删除后回到列表（没有别的东西可显示），列表随之刷新。
 */
async function onDelete() {
  const target = store.detail?.summary
  if (!target) return
  const ok = await showConfirm({
    title: '删除这份报告？',
    message: `「${target.title}」将被彻底删除，无法恢复。同一期可以重新生成，但内容不会与原来一致。`,
  })
  if (!ok) return
  try {
    await store.removeReport(target.id)
    toast.show('已删除')
  } catch (e) {
    toast.show(e instanceof Error ? e.message : String(e), { duration: 6000 })
  }
}

/**
 * 跳转顺序不能乱（见 README §4.6.4）
 *
 * 1. 先写来源上下文——后面任何一步失败，至少还回得去；
 * 2. 切主屏到 articles 并选中文章（让 DOM 先就位）；
 * 3. 关弹窗；
 * 4. 焦点由正文栏接管（`ContentColumn` 的标题）。
 *
 * 反序会触发 `BaseModal` 的焦点归还 → 焦点落到订阅栏按钮上 → 再选中文章，
 * 键盘用户会看到焦点闪两下。
 */
async function onOpenArticle(source: PeriodSource) {
  if (source.articleId === null) {
    toast.show('这篇原文已被清理，只剩报告里的快照')
    return
  }
  store.setSourceContext({
    reportId: store.detail?.summary.id ?? 0,
    reportTitle: store.detail?.summary.title ?? '',
    refNo: source.refNo,
  })
  uiStore.setMainView('articles')
  await articlesStore.selectArticle(source.articleId)
  emit('close')
}

// 弹窗每次打开：刷新列表与当前期素材量（报告可能已被定时任务生成过）
watch(
  () => props.open,
  val => {
    if (val) void store.refresh()
  },
)

// 挂载时只在弹窗已开的情况下拉一次：
// 本组件随页签 `v-show` 常驻，应用启动就会挂载，无条件拉列表等于每次启动多跑两条查询
onMounted(() => {
  if (props.open) void store.refresh()
})
</script>

<style scoped>
.rp-pane {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
  gap: var(--sp-3);
}

/* ── 工具条 ─────────────────────────────────────────────────────────── */
.rp-toolbar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.grain {
  display: flex;
  gap: 2px;
  padding: 3px;
  background: var(--fill);
  border-radius: var(--r-md);
}
.grain button {
  border: none;
  background: none;
  padding: 5px 11px;
  border-radius: 6px;
  cursor: pointer;
  font-family: inherit;
  font-size: var(--fs-modal-label);
  font-weight: 500;
  color: var(--text-secondary);
  transition:
    background var(--m-state, 160ms) ease-out,
    color var(--m-state, 160ms) ease-out;
}
.grain button[aria-pressed='true'] {
  background: var(--primary);
  /* 主色底上的文字用 --tone-fg（亮/暗两套主题都保证对比） */
  color: var(--tone-fg);
}
.grain button:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.period {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
}
.period.solo {
  padding: 4px 10px;
  font-size: var(--fs-modal-label);
  color: var(--text-primary);
  cursor: pointer;
}
.period .nav {
  width: 26px;
  height: 26px;
  border: none;
  background: none;
  border-radius: var(--r-sm);
  color: var(--text-secondary);
  cursor: pointer;
  display: grid;
  place-items: center;
  font-size: 14px;
}
.period .nav:hover:not(:disabled) {
  background: var(--fill);
  color: var(--text-primary);
}
.period .nav:disabled {
  color: var(--text-tertiary);
  opacity: 0.45;
  cursor: default;
}
.period .cur {
  border: none;
  background: none;
  padding: 4px 8px;
  border-radius: var(--r-sm);
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 6px;
  white-space: nowrap;
  font-family: inherit;
  font-size: var(--fs-modal-label);
  font-weight: 500;
  color: var(--text-primary);
}
.period .cur:hover {
  background: var(--fill);
}
.period .cur .sub {
  font-size: var(--fs-modal-meta);
  font-weight: 400;
  color: var(--text-tertiary);
}
.period :focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.spacer {
  flex: 1 1 auto;
  min-width: 0;
}
.btn {
  height: 30px;
  padding: 0 13px;
  border: 1px solid transparent;
  border-radius: var(--r-md);
  cursor: pointer;
  white-space: nowrap;
  font-family: inherit;
  font-size: var(--fs-modal-label);
  font-weight: 500;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.btn-primary {
  background: var(--primary);
  color: var(--tone-fg);
}
.btn-primary:hover:not(:disabled) {
  background: var(--primary-hover);
}
.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn-ghost {
  background: var(--surface);
  border-color: var(--border);
  color: var(--text-secondary);
}
.btn-ghost:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.btn:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}

/* ── 自定义区间 ─────────────────────────────────────────────────────── */
.range-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
}
.range-row label {
  display: flex;
  align-items: center;
  gap: 6px;
}
.range-row input[type='date'] {
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--fs-modal-label);
}
.range-note {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}

/* ── 空态 ───────────────────────────────────────────────────────────── */
.rp-empty {
  border: 1px dashed var(--border);
  border-radius: var(--r-lg);
  padding: var(--sp-4);
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--sp-2);
}
.empty-title {
  font-size: var(--fs-modal-section);
  font-weight: 600;
  color: var(--text-primary);
}
.empty-fact {
  margin: 0;
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--text-secondary);
}

/* ── 列表 ───────────────────────────────────────────────────────────── */
.rp-list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  padding-bottom: var(--sp-2);
}
.rp-list-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  padding: 2px 2px 0;
}
.rp-list-head .hint {
  opacity: 0.8;
}
.rp-error {
  font-size: var(--fs-modal-label);
  color: var(--danger);
}
.rp-first {
  font-size: var(--fs-modal-label);
  color: var(--text-tertiary);
  padding: var(--sp-2) 2px;
}

/* ── 生成进度 ───────────────────────────────────────────────────────── */
.rp-gen {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.gen-card {
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  background: var(--surface);
  padding: var(--sp-4);
}
.gen-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-3);
}
.spin {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  flex-shrink: 0;
  border: 2px solid var(--primary-fg);
  border-top-color: var(--primary);
  animation: rp-spin 900ms linear infinite;
}
@keyframes rp-spin {
  to {
    transform: rotate(360deg);
  }
}
.gen-title {
  font-size: var(--fs-modal-section);
  font-weight: 600;
  color: var(--text-primary);
}
.gen-sub {
  margin-top: 2px;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
.bar {
  height: 4px;
  border-radius: 999px;
  background: var(--fill);
  overflow: hidden;
}
.bar i {
  display: block;
  height: 100%;
  background: var(--primary);
  border-radius: inherit;
  transition: width var(--m-state, 160ms) ease-out;
}
.steps {
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
  margin-top: var(--sp-3);
}
.step {
  display: flex;
  align-items: center;
  gap: var(--sp-15);
  font-size: var(--fs-modal-label);
  color: var(--text-tertiary);
}
.step.done {
  color: var(--text-secondary);
}
.step.active {
  color: var(--text-primary);
  font-weight: 500;
}
.tick {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1px solid var(--border);
  display: grid;
  place-items: center;
  font-size: 9px;
  flex-shrink: 0;
}
.step.done .tick {
  border-color: var(--primary);
  background: var(--primary);
  color: var(--tone-fg);
}
.step.active .tick {
  border-color: var(--primary);
  border-top-color: transparent;
  animation: rp-spin 900ms linear infinite;
}
.step-n {
  margin-left: auto;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
.gen-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  margin-top: var(--sp-4);
}
.gen-hint {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
</style>
