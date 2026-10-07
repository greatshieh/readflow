<template>
  <!-- ═══ Tab 1：事件时间线 ═══ -->
  <div class="m-body">
    <!-- 筛选条：时间窗口 + 实体 -->
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
        <div class="group-header">
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

/** 事件类型枚举 → 中文标签（与后端 EVENT_TYPES 对齐） */
const TYPE_LABELS: Record<string, string> = {
  product: '产品',
  executive: '高管',
  ma: '并购',
  regulatory: '监管',
  strategy: '战略',
  competition: '竞争',
  industry_signal: '行业信号'
}

/**
 * 事件类型的中文标签
 *
 * @param type - 事件类型枚举值
 * @returns 中文标签；未知类型原样返回（后端扩展枚举时前端不崩）
 */
function typeLabel(type: string): string {
  return TYPE_LABELS[type] ?? type
}

/**
 * 按实体分组的事件（后端已按实体名排序，相邻聚合即可保序）
 *
 * @returns 分组映射（实体名 → 事件列表）
 */
const groupedEvents = computed<Record<string, ResearchEventWithContext[]>>(() => {
  const groups: Record<string, ResearchEventWithContext[]> = {}
  for (const ev of researchStore.events) {
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
 * 手动批量提取（补跑入口）
 */
async function handleExtractBatch() {
  extracting.value = true
  extractMsg.value = ''
  extractError.value = false
  try {
    extractMsg.value = await researchStore.extractBatch()
    // 提取产生新事件后刷新当前时间线
    await researchStore.loadEvents(filterDays.value, filterEntityId.value || undefined)
  } catch (e) {
    extractError.value = true
    extractMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    extracting.value = false
  }
}

/** 监听打开状态：打开即重置临时态（拆分前由外壳 close() 手工清零）并加载时间线 */
watch(
  () => props.open,
  async val => {
    if (!val) return
    expandedEventId.value = -1
    extractMsg.value = ''
    extractError.value = false
    await researchStore.loadEvents(filterDays.value, filterEntityId.value || undefined)
  }
)

/** 监听筛选变化：重新查询时间线（守卫 open：弹窗关着时筛选不会变，仅防御） */
watch([filterDays, filterEntityId], async () => {
  if (props.open) {
    expandedEventId.value = -1
    await researchStore.loadEvents(filterDays.value, filterEntityId.value || undefined)
  }
})
</script>

<style scoped>
/* 筛选条里的下拉由 AppSelect 渲染（自绘浮层），宽度由组件 width prop 控制；
   .filter-bar 本体是被多页签共用的全局类（styles.css） */
.event-count {
  margin-left: auto;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
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
