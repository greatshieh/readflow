<template>
  <!-- ═══ Tab 1：事件时间线 ═══ -->
  <div class="m-body">
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
          <div class="event-row">
            <span class="event-type" :class="`type-${ev.event_type}`">
              {{ typeLabel(ev.event_type) }}
            </span>
            <span class="event-sentiment" :class="getSentimentClass(ev.sentiment)">
              {{ sentimentLabel(ev.sentiment) }}
            </span>
            <span class="event-date">{{ ev.event_date || '日期未知' }}</span>
            <span class="event-fact">{{ ev.fact }}</span>
            <span class="expand-arrow">{{ expandedEventId === ev.id ? '▾' : '▸' }}</span>
            <button
              class="event-del"
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
              <a class="source-link" @click.stop="openSourceArticle(ev.article_id)">
                {{ ev.article_title }}
              </a>
            </div>
            <div class="event-meta">提取模型：{{ ev.source_model || '未知' }}</div>
          </div>
        </div>
      </div>
    </div>

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
import { ref, computed, watch } from 'vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import { useResearchStore } from '@/stores/research'
import { useEntitiesStore } from '@/stores/entities'
import { useArticlesStore } from '@/stores/articles'
import { useEventTypesStore } from '@/stores/eventTypes'
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
const articlesStore = useArticlesStore()
const eventTypesStore = useEventTypesStore()

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
 * 跳转到来源文章的阅读视图（先请外壳关弹窗，再选中文章）
 *
 * @param articleId - 文章 ID
 */
async function openSourceArticle(articleId: number) {
  emit('close')
  await articlesStore.selectArticle(articleId)
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
 */
async function reload(resetExpanded = true) {
  if (resetExpanded) expandedEventId.value = -1
  await researchStore.loadEvents(filterDays.value, filterEntityId.value || undefined, {
    eventType: filterEventType.value,
    sentiment: filterSentiment.value,
    keyword: keyword.value,
  })
}

/** 监听打开状态：打开即重置临时态（拆分前由外壳 close() 手工清零）并加载时间线 */
watch(
  () => props.open,
  async val => {
    if (!val) return
    expandedEventId.value = -1
    extractMsg.value = ''
    extractError.value = false
    // 同步加载生效事件类型枚举（启用预设领域 + 自定义类型），供类型筛选与标签渲染
    await eventTypesStore.loadAll()
    await reload(false)
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
  font-size: var(--fs-md);
  transition: border-color var(--dur) var(--ease), background-color var(--dur) var(--ease);
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
.event-count {
  margin-left: auto;
  flex-shrink: 0;
  white-space: nowrap;
  font-size: var(--fs-sm);
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
  font-size: var(--fs-base);
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
.entity-tag {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--primary);
  background: var(--primary-fg);
  padding: var(--sp-1) var(--sp-15);
  border-radius: var(--r-sm);
}
.entity-count {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}

/* 情感徽标：与 .event-type 同几何，但语义色用 positive/negative/neutral */
.event-sentiment {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  font-weight: 600;
  padding: var(--sp-025) var(--sp-2);
  border-radius: var(--r-sm);
  line-height: 1.4;
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
  transition: background 0.15s;
}
.event-item:hover {
  background: var(--hover-bg);
}
.event-row {
  display: flex;
  align-items: baseline;
  gap: var(--sp-2);
}
.event-type {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  font-weight: 600;
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
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
.event-fact {
  flex: 1;
  font-size: var(--fs-md);
  color: var(--text-primary);
  line-height: 1.5;
}
.expand-arrow {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

/* 删除按钮：常态隐藏、hover 该条事件时淡入。
   时间线动辄几十条，若每行常驻一个 × 会让列表显得嘈杂且容易误点。 */
.event-del {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: none;
  border-radius: var(--r-sm);
  color: var(--text-tertiary);
  font-size: 15px;
  line-height: 1;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s, color 0.15s, background 0.15s;
}
.event-item:hover .event-del {
  opacity: 1;
}
.event-del:hover {
  color: var(--danger);
  background: var(--fill-secondary);
}
.event-del:disabled {
  opacity: 1;
  cursor: default;
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
.evidence {
  margin: 0;
  padding: var(--sp-05) var(--sp-15);
  border-left: 3px solid var(--primary);
  background: var(--hover-bg);
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  line-height: 1.6;
}
.event-source {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}
.source-link {
  color: var(--primary);
  cursor: pointer;
}
.source-link:hover { text-decoration: underline; }
.event-meta {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
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
