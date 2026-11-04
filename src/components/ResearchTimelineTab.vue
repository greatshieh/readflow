<template>
  <!-- ═══ Tab 1：事件时间线 ═══ -->
  <div class="m-body" ref="bodyRef">
    <!-- 筛选条：时间窗口 + 实体 + 类型 + 情感 + 关键词 + 排序 -->
    <div class="filter-bar">
      <!-- 筛选条两个下拉改用自绘 AppSelect（原生 option 弹层无法跟随应用风格）。
           选项以数组形式传入：时间窗是固定枚举，实体来自 store 动态拼接。 -->
      <AppSelect v-model="filterDays" :options="dayOptions" width="auto" title="时间窗口" />
      <AppSelect
        v-model="filterEntityId"
        :options="entityOptions"
        width="auto"
        title="实体筛选"
      />
      <AppSelect
        v-model="filterEventType"
        :options="typeOptions"
        width="auto"
        title="事件类型"
      />
      <AppSelect
        v-model="filterSentiment"
        :options="sentimentOptions"
        width="auto"
        title="情感倾向"
      />
      <input
        v-model="keyword"
        type="text"
        class="kw-input"
        placeholder="搜索事实/证据…"
        title="按关键词筛选事件（事实与证据全文匹配）"
      />
      <AppSelect
        v-model="sortBy"
        :options="sortOptions"
        width="auto"
        title="排序"
      />
      <span class="event-count">{{ researchStore.events.length }} 条事件</span>
    </div>

    <!-- 加载中 -->
    <div v-if="researchStore.eventsLoading" class="loading-state">
      <p>正在加载事件...</p>
    </div>

    <!-- 空状态：给出下一步引导 -->
    <div v-else-if="researchStore.events.length === 0" class="empty-state">
      <p>暂无研究事件</p>
      <p class="hint">
        事件由"智能提取"任务从高分文章中自动抽取；也可在阅读文章时于正文区手动提取
      </p>
    </div>

    <!-- 事件列表（按实体分组，组内时间倒序） -->
    <div v-else class="timeline">
      <div
        v-for="(group, entityName) in groupedEvents"
        :key="entityName"
        class="entity-group"
      >
        <div class="group-header clickable" role="button" :title="`查看「${entityName}」的实体仪表盘`" @click="openEntityDashboard(entityName)">
          <span class="entity-tag">{{ entityName }}</span>
          <span class="entity-count">{{ group.length }} 条</span>
        </div>
        <!-- 每条事件：类型徽标 + 日期 + 事实；点击展开证据与来源 -->
        <div
          v-for="ev in group"
          :key="ev.id"
          class="event-item"
          :class="{ expanded: expandedEventId === ev.id }"
          @click="toggleExpand(ev.id)"
        >
          <!-- 行的左边缘只放「宽度恒定」的两样东西：情感徽标（永远两字）与日期
               （YYYY-MM-DD，tabular-nums）。这样每一行的正文都起于同一条竖线，
               扫读时视线不用来回找左边缘。 -->
          <div class="event-row">
            <span class="event-sentiment" :class="getSentimentClass(ev.sentiment)">
              {{ sentimentLabel(ev.sentiment) }}
            </span>
            <span class="event-date">{{ ev.event_date || '日期未知' }}</span>
            <!-- 价格反应：宽度恒定的窄列（+99.9% 与 — 同宽），紧跟日期 -->
            <span
              class="event-market"
              :class="marketClass(ev.id)"
              :title="marketTitle(ev.id)"
            >
              {{ marketText(ev.id) }}
            </span>
            <span class="event-fact">{{ ev.fact }}</span>
            <!-- 类型标签放在行尾，而不是行首：它的字数随类型变（财报 2 / 业绩预告 4 /
                 自定义类型可能更长），排在正文前面会把情感、日期、正文的左边缘一起推歪。
                 移到行尾后，宽度差异只影响它自己，右端对齐成一条整齐的标签列。 -->
            <span
              class="event-type"
              :class="`type-${ev.event_type}`"
              :title="typeLabel(ev.event_type)"
            >
              {{ typeLabel(ev.event_type) }}
            </span>
            <span class="expand-arrow">{{ expandedEventId === ev.id ? '▾' : '▸' }}</span>
            <button
              class="btn btn-icon btn-mini btn-quiet btn-quiet-danger event-del"
              :disabled="deletingId === ev.id"
              title="删除该条事件"
              @click.stop="handleDeleteEvent(ev)"
            >
              ×
            </button>
          </div>
          <!-- 展开区：证据引用块 + 来源文章（点击跳转阅读） -->
          <div v-if="expandedEventId === ev.id" class="event-detail">
            <blockquote v-if="ev.evidence" class="evidence">"{{ ev.evidence }}"</blockquote>
            <div v-if="ev.article_title" class="event-source">
              来源：
              <a class="source-link" @click.stop="openSourceArticle(ev)">
                {{ ev.article_title }}
              </a>
            </div>
            <!-- 市场反应：只有拿到数据才渲染整块（没有就什么都不显示，不留空壳） -->
            <div v-if="hasMarket(ev.id)" class="event-market-detail">
              <span class="emd-h">市场反应</span>
              <span class="emd-item">
                当日
                <b :class="marketClass(ev.id)">{{ marketText(ev.id) }}</b>
                <span v-if="reactionOf(ev.id)?.baseClose"
                      class="emd-base">（基准收盘 {{ reactionOf(ev.id)!.baseClose!.toFixed(2) }}）</span>
              </span>
              <span class="emd-item">
                5 日累计
                <b :class="winClass(ev.id)">{{ winText(ev.id) }}</b>
              </span>
              <span class="emd-note">数据来自已同步的日线，非实时</span>
            </div>
            <div class="event-meta">提取模型：{{ ev.source_model || '未知' }}</div>

            <!-- 人的反馈落点：机器抽出的事件要能被"认可"或"否证"，
                 只让用户看而不给归档的动作，时间线永远只是噪音。
                 放在展开区而非行内：行内已经挤了类型/情感/日期/删除四件事。 -->
            <div class="event-acts">
              <span class="acts-hint">挂到论点</span>
              <button
                v-for="s in STANCE_ACTS"
                :key="s.value"
                class="btn btn-mini btn-round act"
                :class="s.tone"
                :disabled="evActing === ev.id"
                :title="`把这条事件作为「${s.label}」证据挂到某个论点`"
                @click.stop="openAttach(ev, s.value)"
              >
                {{ s.label }}
              </button>
              <span v-if="attachedHint[ev.id]" class="acts-done">{{ attachedHint[ev.id] }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 选论点弹层：时间线里挂证据时只需选"挂哪条、什么立场"，
         事件本身（fact/evidence/来源）已由后端取快照，前端不重复传文本。 -->
    <BaseModal :open="attachTarget !== null" title="挂到论点" size="sm" @close="closeAttach">
      <div class="at">
        <p class="at-fact">{{ attachTarget?.fact }}</p>
        <label class="at-field">
          <span class="at-label">论点</span>
          <AppSelect
            :model-value="attachThesisId"
            :options="attachOptions"
            placeholder="选一个论点"
            @update:model-value="attachThesisId = $event"
          />
        </label>
        <p v-if="attachOptions.length === 0" class="at-empty">
          还没有可用于挂载的论点。先在「论点」页签里立一条判断。
        </p>
        <p v-if="attachError" class="at-error">{{ attachError }}</p>
        <div class="at-foot">
          <button class="btn btn-secondary" type="button" @click="closeAttach">取消</button>
          <button
            class="btn btn-primary"
            type="button"
            :disabled="attachThesisId === '' || attachSaving"
            @click="submitAttach"
          >
            {{ attachSaving ? '挂着…' : '挂上' }}
          </button>
        </div>
      </div>
    </BaseModal>

    <!-- 操作栏：手动批量提取（补跑入口） -->
    <div class="actions">
      <button class="btn btn-secondary" :disabled="extracting" @click="handleExtractBatch">
        {{ extracting ? '提取中...' : '手动批量提取' }}
      </button>
      <span v-if="extractMsg" class="extract-msg" :class="{ error: extractError }">
        {{ extractMsg }}
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 研究工作台 · 事件时间线页签
 *
 * 按实体分组展示结构化事件，支持时间窗与实体筛选；每条事件可展开查看
 * 原文证据与来源文章（点击跳转阅读并关闭弹窗），也可删除抽错/过期的
 * 单条事件（二次确认后物理删除），并提供手动批量提取的补跑入口。
 *
 * # 挂载语义（与外壳的契约）
 * 本页签经 `v-show` 常驻挂载（外壳四页签共用此模式，为的是筛选参数跨页签
 * 与跨开关持久），因此**不能用 `onMounted` 当"进入页签"信号**：
 * - 弹窗打开（`open` 翻真）→ 重置临时态并拉取时间线——对应拆分前外壳
 *   `watch(props.open)` 里的 `loadEvents` 分支；
 * - 筛选变化 → 重新查询（守卫 `open`，与拆分前一致）。
 * 事件数据来自 research store（后端 JOIN 好实体名与文章标题，前端零拼装）。
 */
import { ref, computed, watch, nextTick } from 'vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import BaseModal from './BaseModal.vue'
import { useResearchStore } from '@/stores/research'
import { useEntitiesStore } from '@/stores/entities'
import { useEventTypesStore } from '@/stores/eventTypes'
import { useOriginTrailStore, type OriginRestore, type OriginRestoreResult } from '@/stores/originTrail'
import { useThesesStore, type EvidenceStance } from '@/stores/theses'
import { useToastStore } from '@/stores/toast'
import { timelineLabel } from '@/utils/originLabel'
import { navigateToArticle } from '@/utils/navigateToArticle'
import { restoreScrollTop } from '@/utils/scrollRestore'
import { showConfirm } from '@/stores/modal'
import type { ResearchEventWithContext } from '@/types'

const props = defineProps<{
  /** 弹窗是否打开：打开即重置临时态并（重新）加载时间线 */
  open: boolean
}>()

const emit = defineEmits<{
  /** 跳转来源文章前先请外壳关闭弹窗（阅读视图在弹窗底下） */
  (e: 'close'): void
}>()

const researchStore = useResearchStore()
const entitiesStore = useEntitiesStore()
const eventTypesStore = useEventTypesStore()
const originTrail = useOriginTrailStore()
const thesesStore = useThesesStore()
const toast = useToastStore()

/**
 * 页签根元素（`.m-body`）：本页签的**滚动容器**
 *
 * 不是 window——弹窗固定高度、右列自己滚。跳原文前读它的 `scrollTop`，
 * 返回时再写回去；写成 window 会让"回到原处"永远落在顶部。
 */
const bodyRef = ref<HTMLElement | null>(null)

/** 时间窗口（天数，0 = 不限） */
const filterDays = ref<number>(30)

/** 时间窗口选项：0 表示不限制时间 */
const dayOptions: SelectOption[] = [
  { value: 7, label: '近 7 天' },
  { value: 30, label: '近 30 天' },
  { value: 0, label: '全部' },
]

/** 实体筛选（0 = 全部实体） */
const filterEntityId = ref<number>(0)

/** 实体下拉选项：首项是"全部实体"，其余来自实体表（动态，故用 computed）。
    实体列表的加载由外壳在弹窗打开时统一保证（时间线筛选与实体页签共用）。 */
const entityOptions = computed<SelectOption[]>(() => [
  { value: 0, label: '全部实体' },
  ...entitiesStore.entities.map((e) => ({ value: e.id, label: e.name })),
])

/** 事件类型筛选（'' = 全部类型） */
const filterEventType = ref<string>('')

/** 情感筛选（'' = 全部情感） */
const filterSentiment = ref<string>('')

/** 关键词搜索（作用于事实与证据全文，空串 = 不过滤） */
const keyword = ref('')

/** 排序方式：entity = 按实体分组；date = 按时间倒序（忽略实体边界） */
const sortBy = ref<'entity' | 'date'>('entity')

/** 事件类型下拉选项：首项"全部类型"，其余来自生效枚举 */
const typeOptions: SelectOption[] = [
  { value: '', label: '全部类型' },
  ...eventTypesStore.effectiveTypes.map((t) => ({ value: t.value, label: t.label })),
]

/** 情感下拉选项 */
const sentimentOptions: SelectOption[] = [
  { value: '', label: '全部情感' },
  { value: 'positive', label: '利好' },
  { value: 'negative', label: '利空' },
  { value: 'neutral', label: '中性' },
]

/** 排序下拉选项 */
const sortOptions: SelectOption[] = [
  { value: 'entity', label: '按实体' },
  { value: 'date', label: '按时间' },
]

/** 当前展开的事件 ID（-1 = 全部收起） */
const expandedEventId = ref<number>(-1)

/** 正在删除的事件 ID（-1 = 无删除进行中），用于按钮禁用与串行保护 */
const deletingId = ref<number>(-1)

/** 批量提取进行中 */
const extracting = ref(false)

// ─── 挂到论点（P3）────────────────────────────────────────────────────────────
/** 立场档位：顺序与论点详情页一致（反驳 → 存疑 → 支持） */
const STANCE_ACTS: Array<{ value: EvidenceStance; label: string; tone: string }> = [
  { value: 'oppose', label: '反驳', tone: 'bad' },
  { value: 'mixed', label: '存疑', tone: 'mixed' },
  { value: 'support', label: '支持', tone: 'ok' },
]

/** 弹层正在挂载的事件（null = 关闭） */
const attachTarget = ref<ResearchEventWithContext | null>(null)
/** 本次要用的立场（由点哪个按钮决定，弹层里不再重复问） */
const attachStance = ref<EvidenceStance>('mixed')
/** 选中的论点 ID（空串 = 未选） */
const attachThesisId = ref<string | number>('')
const attachSaving = ref(false)
const attachError = ref('')
/** 正在挂载的事件 ID（-1 = 空闲），用于按钮禁用 */
const evActing = ref<number>(-1)
/** 已挂成功的事件 → 提示文案（按事件 ID 索引，只在本会话有效） */
const attachedHint = ref<Record<number, string>>({})

/**
 * 可选论点
 *
 * 只列未归档的；且**优先把该事件所属实体的论点排在最前**——
 * 时间线按实体分组，用户点某条事件时最可能挂的是这个实体自己的判断。
 */
const attachOptions = computed<SelectOption[]>(() => {
  const eid = attachTarget.value?.entity_id ?? 0
  const rows = thesesStore.list.filter((t) => t.status !== 'archived')
  const own = rows.filter((t) => t.entityId === eid)
  const other = rows.filter((t) => t.entityId !== eid)
  return [...own, ...other].map((t) => ({
    value: t.id,
    label: `${t.entityName} · ${t.statement.length > 22 ? `${t.statement.slice(0, 22)}…` : t.statement}`,
  }))
})

/**
 * 打开挂载弹层
 *
 * @param ev - 目标事件
 * @param stance - 由行内按钮决定的立场
 */
function openAttach(ev: ResearchEventWithContext, stance: EvidenceStance) {
  attachTarget.value = ev
  attachStance.value = stance
  attachThesisId.value = ''
  attachError.value = ''
  if (thesesStore.list.length === 0) void thesesStore.loadList()
}

/** 关闭挂载弹层并清空临时态 */
function closeAttach() {
  attachTarget.value = null
  attachThesisId.value = ''
  attachError.value = ''
}

/**
 * 提交挂载
 *
 * 事件的内容（fact / evidence / 来源文章）由后端按 `kind='event'` 取快照，
 * 前端只给 ID 与立场——避免前端再拼一遍文本，也避免快照与事件原文不一致。
 */
async function submitAttach() {
  const ev = attachTarget.value
  if (!ev || attachThesisId.value === '') return
  attachSaving.value = true
  evActing.value = ev.id
  attachError.value = ''
  try {
    await thesesStore.addEvidence({
      thesisId: Number(attachThesisId.value),
      kind: 'event',
      refId: ev.id,
      stance: attachStance.value,
    })
    const label = attachStance.value === 'oppose' ? '反驳' : attachStance.value === 'mixed' ? '存疑' : '支持'
    attachedHint.value[ev.id] = `已挂为${label}证据`
    toast.show(`已挂为${label}证据`)
    closeAttach()
  } catch (e) {
    attachError.value = e instanceof Error ? e.message : String(e)
  } finally {
    attachSaving.value = false
    evActing.value = -1
  }
}

/** 批量提取结果消息 */
const extractMsg = ref('')

/** 批量提取是否出错 */
const extractError = ref(false)

/** 情感枚举 → 中文标签 */
const SENTIMENT_LABELS: Record<string, string> = {
  positive: '利好',
  negative: '利空',
  neutral: '中性'
}

/** 情感枚举 → CSS class */
const SENTIMENT_CLASSES: Record<string, string> = {
  positive: 'positive',
  negative: 'negative',
  neutral: 'neutral'
}

/**
 * 获取事件类型的中文标签
 */
function typeLabel(type: string): string {
  return eventTypesStore.effectiveTypes.find((t) => t.value === type)?.label ?? type
}

/**
 * 获取情感的中文标签
 */
function sentimentLabel(sentiment?: string): string {
  if (!sentiment) return ''
  return SENTIMENT_LABELS[sentiment] ?? sentiment
}

/**
 * 获取情感的 CSS class
 */
function getSentimentClass(sentiment?: string): string {
  if (!sentiment) return ''
  return `sentiment-${SENTIMENT_CLASSES[sentiment] ?? 'neutral'}`
}

// ─── 价格反应（附加层，数据缺失时一律显示 —）────────────────────────────────

/** 取某事件的反应记录（无则 undefined） */
function reactionOf(id: number) {
  return researchStore.priceReactions.get(id)
}

/** 是否拿到了可展示的反应（当日或 5 日任一有值即算） */
function hasMarket(id: number): boolean {
  const r = reactionOf(id)
  return !!r && (r.dayChange !== null || r.window5d !== null)
}

/**
 * 行内价格列文案
 *
 * 无数据显示 `—` 而不是 `0%`：**0% 与"没有数据"是两件事**，
 * 把后者显示成前者会让用户以为"这条新闻市场毫无反应"。
 */
function marketText(id: number): string {
  const v = reactionOf(id)?.dayChange
  if (v === null || v === undefined) return '—'
  return `${v > 0 ? '+' : ''}${v.toFixed(2)}%`
}

/** 5 日累计文案 */
function winText(id: number): string {
  const v = reactionOf(id)?.window5d
  if (v === null || v === undefined) return '数据不足 5 天'
  return `${v > 0 ? '+' : ''}${v.toFixed(2)}%`
}

/**
 * 涨跌配色：**国内惯例，涨红跌绿**
 *
 * 与欧美相反，这也是本项目时间线圆点已在用的口径（红=利多/绿=利空）。
 * 用 `--tone-red` / `--tone-green` 语义色而非新增色板。
 */
function trendClass(v: number | null | undefined): string {
  if (v === null || v === undefined) return 'mk-flat'
  if (v > 0) return 'mk-up'
  if (v < 0) return 'mk-down'
  return 'mk-flat'
}

function marketClass(id: number): string {
  return trendClass(reactionOf(id)?.dayChange)
}

function winClass(id: number): string {
  return trendClass(reactionOf(id)?.window5d)
}

/**
 * 行内价格列的悬停说明
 *
 * 把"为什么是 —"讲清楚：未关联代码 / 未同步行情 / 事件日期未知，
 * 三种原因的用户动作完全不同（去关联 / 去同步 / 无解）。
 */
function marketTitle(id: number): string {
  const r = reactionOf(id)
  if (!r) return '该实体尚未关联证券代码，无法计算价格反应'
  if (!r.symbol) return '该实体尚未关联证券代码，无法计算价格反应'
  if (r.dayChange === null) return `已关联 ${r.symbol}，但没有事件日附近的行情数据（请先在行情面板同步）`
  return `事件日涨跌（基准收盘 ${r.baseClose?.toFixed(2) ?? '—'}）`
}

/**
 * 分组前按排序方式排序的副本
 *
 * `entity` 模式沿用后端「按实体名 + 时间倒序」的固有顺序（已在 SQL ORDER BY 中保证）；
 * `date` 模式忽略实体边界，按事件日期倒序，便于跨实体看"最近发生的事"。
 *
 * @returns 排序后的事件数组（不改动 store 中的原始顺序）
 */
const sortedEvents = computed<ResearchEventWithContext[]>(() => {
  if (sortBy.value === 'date') {
    return [...researchStore.events].sort((a, b) =>
      (b.event_date || '').localeCompare(a.event_date || '')
    )
  }
  return researchStore.events
})

/**
 * 按实体分组的事件
 *
 * 在 `sortedEvents` 之上聚合（排序模式决定组内/组间顺序），相邻同实体即归为一组。
 *
 * @returns 分组映射（实体名 → 事件列表）
 */
const groupedEvents = computed<Record<string, ResearchEventWithContext[]>>(() => {
  const groups: Record<string, ResearchEventWithContext[]> = {}
  for (const ev of sortedEvents.value) {
    const key = ev.entity_name || '未知实体'
    if (!groups[key]) groups[key] = []
    groups[key].push(ev)
  }
  return groups
})

/**
 * 展开 / 收起一条事件
 *
 * @param id - 事件 ID
 */
function toggleExpand(id: number) {
  expandedEventId.value = expandedEventId.value === id ? -1 : id
}

/**
 * 删除单条事件（带二次确认）
 *
 * 删除不可恢复，且是物理删除——若日后对同一篇文章再次运行提取，
 * 模型仍可能抽出相同事实重新入库，因此确认文案里把这点说清楚，
 * 避免用户误以为"删了就永久没了"。
 *
 * 失败信息复用操作栏的 extractMsg 通道展示，不额外引入一套提示 UI。
 *
 * @param ev - 待删除的事件
 */
async function handleDeleteEvent(ev: ResearchEventWithContext) {
  // 串行保护：删除进行中忽略其它点击，避免并发 invoke 与重复确认弹窗
  if (deletingId.value !== -1) return
  const brief = ev.fact.length > 40 ? `${ev.fact.slice(0, 40)}…` : ev.fact
  const ok = await showConfirm({
    title: '删除研究事件',
    message: `确定删除「${brief}」？删除后不可恢复，再次提取该文章时可能重新出现。`
  })
  if (!ok) return

  deletingId.value = ev.id
  try {
    await researchStore.deleteEvent(ev.id)
    // 被删的若正处于展开态，收起以免残留一个空的展开区
    if (expandedEventId.value === ev.id) expandedEventId.value = -1
  } catch (e) {
    extractError.value = true
    extractMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    deletingId.value = -1
  }
}

/**
 * 跳转到来源文章的阅读视图
 *
 * 跳之前先把"从哪来、当时停在哪"记下来（`originTrail`）：正文栏据此出一条上下文栏，
 * 返回时按快照把展开项与滚动位置都还原。**先记再跳**——顺序反过来，一旦
 * `selectArticle` 抛错，用户就没有任何回到原处的手段了。
 *
 * 滚动量读的是本页签根元素（`.m-body`），不是 window。
 *
 * @param ev - 被点开的来源事件（整条传进来：文案要用到日期、实体名与事实）
 */
async function openSourceArticle(ev: ResearchEventWithContext) {
  originTrail.push({
    kind: 'timeline',
    // 事件 ID 天然唯一；同一事件重复点「来源」只替换快照，不堆叠
    key: `timeline:${ev.id}`,
    label: timelineLabel(ev),
    articleId: ev.article_id,
    restore: {
      tab: 'timeline',
      scrollTop: bodyRef.value?.scrollTop ?? 0,
      // -1 表示当时没有展开项：这种情况下返回后也全收起，不必再猜"该展开哪一条"
      expandedEventId: expandedEventId.value,
    },
    stale: null,
  })
  emit('close')
  // 切源 + 打开 + 定位：与报告引用、论点证据、仪表盘、命令面板同一条编排
  await navigateToArticle(ev.article_id)
}

/**
 * 打开时间线分组头对应实体的「实体仪表盘」
 *
 * 分组按实体名聚合，需先反查实体 ID（实体名唯一，未命中则静默忽略）。
 * 走 window 事件 `open-entity-dashboard`，与实体列表入口同构（App.vue 收到后切换主屏到实体仪表盘全屏视图）。
 *
 * @param entityName - 分组头显示的实体名
 */
function openEntityDashboard(entityName: string) {
  const ent = entitiesStore.entities.find((e) => e.name === entityName)
  if (!ent) return
  window.dispatchEvent(
    new CustomEvent('open-entity-dashboard', { detail: { entityId: ent.id } })
  )
}

/**
 * 手动批量提取（补跑入口）
 */
async function handleExtractBatch() {
  // 防重入：按钮已 loading 时直接返回，避免重复触发批量任务
  if (extracting.value) return
  extracting.value = true
  extractMsg.value = ''
  extractError.value = false
  // 看门狗：命令异常长时间不返回时兜底解除 loading，避免按钮永久卡死
  const watchdog = window.setTimeout(() => {
    extracting.value = false
  }, 120000)
  try {
    extractMsg.value = await researchStore.extractBatch()
    // 命令已返回：立即解除 loading，不让后续刷新阻塞按钮
    extracting.value = false
    clearTimeout(watchdog)
    // 刷新时间线（不强等，失败不影响提取结果展示）
    reload(false).catch(() => {})
  } catch (e) {
    extractError.value = true
    extractMsg.value = e instanceof Error ? e.message : String(e)
    extracting.value = false
    clearTimeout(watchdog)
  }
}

/**
 * 重新拉取时间线：把当前所有筛选条件（时间窗 / 实体 / 类型 / 情感 / 关键词）
 * 透传给 research store 的 `loadEvents`。所有 watch 与手动触发都走这里，
 * 避免筛选参数散落多处造成同步漂移（单一真源）。
 *
 * @param resetExpanded - 是否收起已展开项（筛选变化 / 重新打开时都应收起，避免错位）
 * @returns 本次拉取完成时 resolve；失败不抛（错误留在 store 里）
 */
async function reload(resetExpanded = true): Promise<void> {
  const task = runReload(resetExpanded)
  reloading = task
  await task
}

/**
 * `reload` 的本体
 *
 * 单独一层是为了让 `restoreFrom` 能拿到"这一轮拉取的 Promise"：返回时的恢复要等
 * **数据到位**才能判断"那条事件还在不在"，直接在恢复里再拉一次会白跑一趟后端。
 *
 * @param resetExpanded - 是否收起已展开项
 * @returns 拉取完成时 resolve
 */
async function runReload(resetExpanded: boolean): Promise<void> {
  if (resetExpanded) expandedEventId.value = -1
  // 先清空旧的价格反应，避免上一次筛选的价格短暂显示在新列表上（错配比没有更糟）
  researchStore.priceReactions = new Map()
  await researchStore.loadEvents(filterDays.value, filterEntityId.value || undefined, {
    eventType: filterEventType.value,
    sentiment: filterSentiment.value,
    keyword: keyword.value,
  })
  // 价格是附加层，**不 await 阻塞列表渲染**：先让事件出现，价格算完再补上。
  // loadPriceReactions 内部吞错，失败只会留在 console。
  void researchStore.loadPriceReactions()
}

/**
 * 最近一次 `reload` 的 Promise（`restoreFrom` 等它落地）
 *
 * 初值是已 resolve 的 Promise：没有在飞的拉取时，等待是零成本的。
 */
let reloading: Promise<void> = Promise.resolve()

/**
 * 从快照恢复页签（外壳在弹窗打开、数据就绪后调用）
 *
 * # 为什么要先等 `reloading`
 * 弹窗打开时会先 `reload` 一轮。若不等它，这里看到的是**上一轮**的事件列表——
 * 返回前刚被删掉的事件还在、刚新增的不在，于是"展开项"会被判成"不存在"，
 * 用户拿到一条失效提示，而他其实什么都没做错。
 * ⚠ 读 `reloading` 之前必须先进 `nextTick`：外壳的 watch 排在子组件之前，
 * 本函数可能比"打开 watch 里那次 reload"更早继续执行，直接读会拿到上一轮
 * 已 resolve 的旧 Promise（等待等于没等）。
 *
 * # 为什么不在这里再拉一次
 * 打开 watch 已经拉过一轮；再拉一次会让同一次打开产生两次后端查询，
 * 并且两次结果到达的先后无法保证（后到的覆盖先到的）。
 *
 * # 恢复顺序
 * 先设展开项（DOM 还得再渲染一次，展开会把下面的内容顶下去），
 * 再 `nextTick` 等渲染、然后才写滚动量；滚动量写早了会被展开造成的布局变化作废。
 *
 * @param restore - 来源快照里的恢复指令
 * @returns `ok` 表示已恢复；`missing` 表示那条事件已经不在了（调用方转失效态）
 */
async function restoreFrom(restore: OriginRestore): Promise<OriginRestoreResult> {
  // 先等一帧再读 `reloading`：本函数可能比"打开 watch 里那次 reload"更早继续执行
  // （外壳的 watch 先于子组件 watch 跑），此时直接读会拿到**上一轮**已 resolve 的
  // 旧 Promise。`nextTick` 之后那次 reload 已经把新 Promise 写进去了
  await nextTick()
  await reloading
  const target = restore.expandedEventId
  if (target !== undefined && target >= 0) {
    if (!researchStore.events.some((ev) => ev.id === target)) {
      // 展开项没了：全收起，交给调用方决定是"转失效态"还是"照常打开"
      expandedEventId.value = -1
      return { ok: false, kind: 'missing' }
    }
    expandedEventId.value = target
  }
  await nextTick()
  await restoreScrollTop(() => bodyRef.value, restore.scrollTop)
  return { ok: true }
}

defineExpose({ restoreFrom })

/**
 * 监听打开状态：打开即重置临时态（拆分前由外壳 close() 手工清零）并加载时间线
 *
 * # 为什么"收起展开项"要看有没有待返回意图
 * 手动关掉弹窗再打开，期望是干净的（全收起、全重拉）；而"从原文返回"期望回到
 * 刚才的位置，包括那条展开的事件。两者在同一个 watch 里，靠
 * `originTrail.returnsTo('timeline')` 区分——它同时回答"这次打开是一次返回"与
 * "这次返回落在我这里"（返回报告时本页签也会收到 open，但那不是冲它来的）。
 * 写成"总是收起"会让返回后的 `restoreFrom` 与这里的重置抢同一个变量（谁后跑谁赢）。
 */
watch(
  () => props.open,
  async val => {
    if (!val) return
    const returning = originTrail.returnsTo('timeline')
    if (!returning) expandedEventId.value = -1
    extractMsg.value = ''
    extractError.value = false
    // 同步加载生效事件类型枚举（启用预设领域 + 自定义类型），供类型筛选与标签渲染
    await eventTypesStore.loadAll()
    await reload(!returning)
  }
)

/** 关键词输入防抖：停止输入 350ms 后才重新查询，避免逐字符触发后端 */
let kwTimer: ReturnType<typeof setTimeout> | undefined
watch(keyword, () => {
  if (!props.open) return
  if (kwTimer) clearTimeout(kwTimer)
  kwTimer = setTimeout(() => reload(), 350)
})

/** 其余筛选（时间窗 / 实体 / 类型 / 情感）立即生效；排序是前端计算，不触发后端 */
watch([filterDays, filterEntityId, filterEventType, filterSentiment], async () => {
  if (props.open) await reload()
})
</script>

<style scoped>
/* 筛选条里的下拉由 AppSelect 渲染（自绘浮层），宽度由组件 width prop 控制；
   .filter-bar 本体是被多页签共用的全局类（styles.css）。
   本页签元素最多（4 下拉 + 关键词 + 排序 + 计数共 7 项），窄窗时允许换行兜底：
   不加这条时整行装不下，全部项都会被压缩，末位计数被压成竖排细条并被弹窗右缘裁切。
   scoped 规则特异性高于全局 .filter-bar，故不会波及关系图那条同名的参数条。 */
.filter-bar {
  flex-wrap: wrap;
  row-gap: var(--sp-2);
}

/* 关键词输入框：此前类名 .kw-input 全项目无定义（唯一出现处就是本文件的模板），
   输入框实际用浏览器原生外观——固有宽度不可控、2px 系统边框与 AppSelect 触发器
   风格不一致，还挤占了筛选条的宽度预算。这里对齐 AppSelect 触发器的几何与令牌；
   聚焦环沿用全局 input:focus-visible（styles.css）。
   flex: 1 1 140px 让它成为行内唯一可伸缩项：有余量时长满、不足时先让位。 */
.kw-input {
  flex: 1 1 140px;
  min-width: 120px;
  height: 34px;
  padding: 0 var(--sp-2);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--fill);
  color: var(--text-primary);
  font-size: var(--fs-modal-body);
  transition: border-color var(--m-state) var(--m-ease-out),
    background-color var(--m-state) var(--m-ease-out);
}
.kw-input::placeholder {
  color: var(--text-tertiary);
}
.kw-input:hover {
  background: var(--fill-secondary);
}

/* 事件计数：margin-left:auto 把它推到筛选条最右端。
   flex-shrink:0 + nowrap 让它永不参与收缩（此前缺这两条，行超宽时被压到最小内容宽度，
   文字折成「7/条/事件」三竖行并被弹窗右缘裁掉，即用户看到的"看不清"）；
   颜色由三级文字提升到二级，白底对比度约 3:1 → 7:1，小字也清晰可读。 */
/* 计数是元信息档（11），不需要与被计数内容争层级 */
.event-count {
  margin-left: auto;
  flex-shrink: 0;
  white-space: nowrap;
  font-size: var(--fs-modal-meta);
  color: var(--text-secondary);
}

/* 加载态：.empty-state 是全局类，本页签专属的加载占位留这里 */
.loading-state {
  text-align: center;
  padding: var(--sp-10) 0;
  color: var(--text-tertiary);
}
.loading-state p {
  margin: 0 0 var(--sp-2);
  font-size: var(--fs-modal-body);
}

/* 时间线分组 */
.timeline {
  display: flex;
  flex-direction: column;
  gap: var(--sp-5);
}
.entity-group {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.group-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-bottom: var(--sp-2);
  border-bottom: 1px solid var(--border);
}
.group-header.clickable {
  cursor: pointer;
  border-radius: var(--r-sm);
}
.group-header.clickable:hover .entity-tag {
  filter: brightness(0.95);
  text-decoration: underline;
}
/* 分组头 = 弹窗内的区块标题（16/600）。原先 14/600，与下面 14 的事件正文同字号，
   只靠字重分辨（中文只有 400/700 两档，字重形不成层级）→ 提到 16。 */
.entity-tag {
  font-size: var(--fs-modal-section);
  font-weight: 600;
  line-height: var(--lh-modal-section);
  color: var(--primary-text);
  background: var(--primary-tint);
  padding: var(--sp-1) var(--sp-15);
  border-radius: var(--r-sm);
}
.entity-count {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}

/* 情感徽标：与 .event-type 同几何，但语义色用 positive/negative/neutral。
   徽标属元信息档 11；角色默认 400，色块反白小字保留 500 补强；行高取标签档 1.4 压住色块高度。

   它是整行的第一列，兼任"对齐锚"：min-width 固定成「两字 + 两侧内边距」，
   因为情感标签恒为两字（利好/利空/中性）。没有这一条，未标注情感的事件
   会渲染成只有内边距的空壳，日期随之左移，一整列就对不齐了。 */
.event-sentiment {
  flex-shrink: 0;
  min-width: calc(2em + 2 * var(--sp-2));
  text-align: center;
  white-space: nowrap;
  font-size: var(--fs-modal-meta);
  font-weight: 500;
  padding: var(--sp-025) var(--sp-2);
  border-radius: var(--r-sm);
  line-height: var(--lh-modal-label);
}
.sentiment-positive { color: var(--tone-fg); background: var(--entity-product); }
.sentiment-negative { color: var(--tone-fg); background: var(--entity-regulatory); }
.sentiment-neutral   { color: var(--tone-fg); background: var(--entity-other); }

/* 事件条目 */
.event-item {
  padding: var(--sp-2) var(--sp-15);
  background: var(--fill);
  border-radius: var(--r-md);
  cursor: pointer;
  transition: background var(--m-tap) var(--m-ease-out);
}
.event-item:hover {
  background: var(--hover-bg);
}
.event-row {
  display: flex;
  align-items: baseline;
  gap: var(--sp-2);
}
/* 类型徽标：与 .event-sentiment 同档（11/500/1.4），但**不做定宽**——
   它的字数是 2～4（财报/业绩预告）且自定义类型可能更长，钉死宽度要么被截断，
   要么把短标签撑成一圈空荡的色块。所以它不参与左侧对齐，
   而是靠"排在正文之后 + flex 自动推到右端"形成右对齐的标签列。
   max-width 只兜住极端长的自定义类型，避免它把正文挤成窄条。 */
.event-type {
  flex-shrink: 0;
  max-width: 9em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: var(--fs-modal-meta);
  font-weight: 500;
  line-height: var(--lh-modal-label);
  padding: var(--sp-025) var(--sp-2);
  border-radius: var(--r-sm);
  color: var(--text-secondary);
  background: var(--border);
}
/* 类型配色：借鉴评分徽标的语义色（产品绿/并购橙/监管红等），暗色由变量自动适配 */
.type-product { color: var(--tone-fg); background: var(--entity-product); }
.type-executive { color: var(--tone-fg); background: var(--entity-executive); }
.type-ma { color: var(--tone-fg); background: var(--entity-ma); }
.type-regulatory { color: var(--tone-fg); background: var(--entity-regulatory); }
.type-strategy { color: var(--tone-fg); background: var(--entity-strategy); }
.type-competition { color: var(--tone-fg); background: var(--entity-competition); }
.type-industry_signal { color: var(--tone-fg); background: var(--entity-industry_signal); }

.event-date {
  flex-shrink: 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
/* 行内价格列：固定宽度是硬要求。
   左边缘只放宽度恒定的元素，这一列若随内容变宽（+9.9% 与 +99.9% 差 7px），
   整列正文的起始竖线就会变成锯齿。56px 能容下 "+99.99%" 与 "—"。 */
.event-market {
  flex-shrink: 0;
  width: 56px;
  text-align: right;
  font-size: var(--fs-modal-meta);
  font-variant-numeric: tabular-nums;
  color: var(--text-tertiary);
}
/* 涨红跌绿：国内惯例，与时间线圆点同一口径（红=利多/绿=利空） */
.event-market.mk-up,
.emd-item b.mk-up {
  color: var(--tone-red);
}
.event-market.mk-down,
.emd-item b.mk-down {
  color: var(--tone-green);
}
/* 市场反应详情卡：展开区内的补充信息，弱化处理避免抢证据引用的视觉重心 */
.event-market-detail {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: var(--sp-1) var(--sp-3);
  margin-top: var(--sp-1);
  padding: var(--sp-1) var(--sp-2);
  border-radius: var(--r-sm);
  background: var(--surface-muted);
  font-size: var(--fs-modal-meta);
  color: var(--text-secondary);
}
.emd-h {
  font-weight: 500;
  color: var(--text-secondary);
}
.emd-item {
  font-variant-numeric: tabular-nums;
}
.emd-item b {
  font-weight: 500;
}
.emd-base,
.emd-note {
  color: var(--text-tertiary);
}
/* 窄屏隐藏价格列：手机上横向空间优先给事实正文（价格在展开区仍可看到） */
@media (max-width: 768px) {
  .event-market {
    display: none;
  }
}
/* 事件正文：一次阅读的主体句，用正文档 14 / 1.65（原先 14 + 裸写 1.5，行距偏挤）。
   min-width: 0 是 flex 行里长文本的必需项：默认 min-width:auto 会让它撑到
   max-content，把行尾的类型徽标和箭头顶出行外。 */
.event-fact {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-modal-body);
  color: var(--text-primary);
  line-height: var(--lh-modal-body);
}
/* 展开箭头是图形不是文字，不占角色位；取标签档 13 保证在 11 的日期旁仍看得见 */
.expand-arrow {
  flex-shrink: 0;
  font-size: var(--fs-modal-label);
  color: var(--text-tertiary);
}

/* 删除按钮：常态隐藏、hover 该条事件时淡入。
   时间线动辄几十条，若每行常驻一个 × 会让列表显得嘈杂且容易误点。
   字形 18（原裸写 15px，与各弹窗关闭钮对齐）；命中区取 24×24 而非 32：
   本行行盒由 14/1.65 ≈ 23px 决定，撑到 32 会把每条事件拉高近 10px。
   :focus-visible 必须强制显形 —— 否则键盘 Tab 到这里时按钮仍是 opacity:0，
   焦点落在看不见的元素上。 */
/* 事件删除钮：外观来自 `.btn .btn-icon .btn-mini .btn-quiet .btn-quiet-danger`
   （24×24 幽灵图标钮，悬停透危险色）。这里只留"悬停事件行才显形"这个交互，
   以及 ✕ 字形的字号（它是文字而不是 svg）。 */
.event-del {
  font-size: var(--fs-lg);
  line-height: 1;
  opacity: 0;
  transition: opacity var(--m-tap) var(--m-ease-out);
}
.event-item:hover .event-del,
.event-del:focus-visible {
  opacity: 1;
}
/* 禁用时保持可见（"正在删除中"要看得见），这条状态覆盖 `button:disabled` 的 .5 */
.event-del:disabled {
  opacity: 1;
  color: var(--text-tertiary);
}

/* 展开区：证据 + 来源 */
.event-detail {
  margin-top: var(--sp-2);
  padding-top: var(--sp-2);
  border-top: 1px dashed var(--border);
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}
/* 证据引文：整句，走正文档 14 / 1.65（原先 13 + 裸写 1.6） */
.evidence {
  margin: 0;
  padding: var(--sp-05) var(--sp-15);
  border-left: 3px solid var(--primary);
  background: var(--hover-bg);
  font-size: var(--fs-modal-body);
  color: var(--text-secondary);
  line-height: var(--lh-modal-body);
}
/* 来源行内含可点链接（.source-link），不降到 meta 11 —— 11px 的可点目标太小，
   取标签档 13；纯元信息（.event-meta）仍留在 11。 */
.event-source {
  font-size: var(--fs-modal-label);
  color: var(--text-tertiary);
}
.source-link {
  color: var(--primary);
  cursor: pointer;
}
.source-link:hover { text-decoration: underline; }
.event-meta {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}

/* ═══ 挂到论点（展开区底部的一行动作）═══ */
.event-acts {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  margin-top: var(--sp-15);
  padding-top: var(--sp-15);
  border-top: 1px dashed var(--border);
  flex-wrap: wrap;
}
.acts-hint {
  flex-shrink: 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  margin-right: var(--sp-05);
}
/* 「支持 / 反驳 / 存疑」：外观来自 `.btn .btn-mini .btn-round`（24px 药丸）。
   此前是 22px 自建药丸，比所有行内小钮都矮 2px。三类立场只在**悬停**时透出颜色
   且只变描边——这一行是次要动作，不该抢过事件正文。 */
.act.bad:hover:not(:disabled) { border-color: var(--danger); color: var(--danger-text); }
.act.mixed:hover:not(:disabled) { border-color: var(--warning); color: var(--warning-text); }
.act.ok:hover:not(:disabled) { border-color: var(--success); color: var(--success-text); }
.acts-done {
  font-size: var(--fs-modal-meta);
  color: var(--success-text);
}

/* ═══ 选论点弹层内容 ═══ */
.at {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.at-fact {
  margin: 0;
  padding: var(--sp-2) var(--sp-3);
  background: var(--fill);
  border-radius: var(--r-md);
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--text-primary);
}
.at-field {
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}
.at-label {
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
}
.at-empty,
.at-error {
  margin: 0;
  font-size: var(--fs-modal-label);
  line-height: var(--lh-modal-label);
}
.at-empty { color: var(--text-tertiary); }
.at-error { color: var(--danger-text); }
.at-foot {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-2);
  margin-top: var(--sp-05);
}

/* 操作栏 */
.actions {
  display: flex;
  align-items: center;
  gap: var(--sp-15);
  padding-top: var(--sp-3);
  border-top: 1px solid var(--border);
}
</style>
