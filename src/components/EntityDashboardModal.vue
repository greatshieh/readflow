<template>
  <!--
    实体仪表盘（弹窗）：点实体后弹出的完整仪表盘。
    以实体为中心一屏聚合：综合舆情 + 财务快照 + 近期事件 + 多源交叉验证 + 关联文章。
    尺寸 2xl（820px，沿用此前「仪表盘偏小、适当放大」的定档），内容区内部滚动。
    数据来自既有后端命令（entity_sentiment / financial_data_list_by_entity /
    research_cross_reference / entity_articles / research_events_timeline），
    本组件只做并行聚合与结构化渲染，不引入任何新查询。
    入口：实体列表项 / 时间线分组头 / 关系图节点 经 `open-entity-dashboard` 事件，
    由 App.vue 调 uiStore.openEntityDashboard 打开本弹窗。
  -->
  <BaseModal :open="open" size="2xl" title="实体仪表盘" @close="onClose">
    <div class="dash-modal">
      <!-- 头部：标题 + 实体名/类型 + 关闭钮（关闭钮复用全局 .btn-icon） -->
      <header class="dash-modal-head">
        <div class="dash-head-titles">
          <h3 class="dash-head-title">实体仪表盘</h3>
          <span class="dash-head-name">{{ entityName }}</span>
          <span v-if="entityTypeLabel" class="dash-head-type">{{ entityTypeLabel }}</span>
        </div>
        <button class="btn-icon" @click="onClose" aria-label="关闭" title="关闭">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
          </svg>
        </button>
      </header>

      <!-- 主体：加载态/错误态/五区内容，内部纵向滚动 -->
      <div class="dash-modal-body">
        <div v-if="loading" class="dash-state">加载中…</div>
        <div v-else-if="error" class="dash-state dash-error">{{ error }}</div>

        <div v-else class="dash">
          <!-- 综合舆情 -->
          <section class="dash-section">
            <h4 class="dash-h">综合舆情</h4>
            <div class="dash-sentiment">
              <span class="badge" :class="sentimentClass">{{ sentimentLabel }}</span>
              <span class="dash-stat">
                利好 {{ counts.positive }} · 利空 {{ counts.negative }} · 中性 {{ counts.neutral }}
              </span>
            </div>
          </section>

          <!-- 财务快照 -->
          <section class="dash-section">
            <h4 class="dash-h">财务快照</h4>
            <ul v-if="financials.length" class="dash-fin">
              <li v-for="f in financials" :key="f.id" class="fin-item">
                <span class="fin-k">{{ f.metric_key }}</span>
                <span class="fin-v">{{ f.metric_value }}<em v-if="f.currency"> {{ f.currency }}</em></span>
                <span v-if="f.period" class="fin-p">{{ f.period }}</span>
              </li>
            </ul>
            <p v-else class="dash-empty">暂无财务数据</p>
          </section>

          <!-- 近期事件 -->
          <section class="dash-section">
            <h4 class="dash-h">近期事件</h4>
            <ul v-if="events.length" class="dash-events">
              <li v-for="ev in events" :key="ev.id" class="dash-event">
                <span class="ev-type" :class="`type-${ev.event_type}`">{{ typeLabel(ev.event_type) }}</span>
                <span class="ev-date">{{ ev.event_date || '日期未知' }}</span>
                <span class="ev-fact">{{ ev.fact }}</span>
              </li>
            </ul>
            <p v-else class="dash-empty">暂无相关事件</p>
          </section>

          <!-- 多源交叉验证 -->
          <section class="dash-section">
            <h4 class="dash-h">来源验证（{{ crossRefs.length }}）</h4>
            <ul v-if="crossRefs.length" class="dash-cross">
              <li v-for="c in crossRefs" :key="c.event_id" class="cross-item">
                <div class="cross-head">
                  <span class="cross-src">{{ c.source_name ?? '未知来源' }}</span>
                  <span class="cross-sent" :class="sentClass(c.sentiment)">{{ sentLabel(c.sentiment) }}</span>
                </div>
                <p class="cross-fact">{{ c.fact }}</p>
              </li>
            </ul>
            <p v-else class="dash-empty">该实体暂无多源报道</p>
          </section>

          <!-- 关联文章 -->
          <section class="dash-section">
            <h4 class="dash-h">关联文章</h4>
            <ul v-if="articles.length" class="dash-articles">
              <li v-for="a in articles" :key="a.id" class="art-item">
                <a :href="a.link" target="_blank" rel="noopener" class="art-link">{{ a.title }}</a>
              </li>
            </ul>
            <p v-else class="dash-empty">暂无关联文章</p>
          </section>
        </div>
      </div>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 实体仪表盘（弹窗）
 *
 * # 职责
 * 以实体为中心聚合研究流水线的五类产物：情感倾向（综合舆情）、财务指标、
 * 近期事件、多源交叉验证、关联文章。数据全部来自既有后端命令，
 * 经 research store 的封装方法并行取回（这些封装此前在 store 里定义却未被任何
 * 组件调用，本组件是它们的首个正式消费者）。
 *
 * # 形态
 * 以弹窗（BaseModal，size=2xl 820px）呈现：点实体后弹出，展示完整仪表盘，
 * 不占用主屏、不隐藏订阅栏。区别于关系图的全屏视图（uiStore.mainView==='graph'）。
 *
 * # 打开方式
 * 由 `open-entity-dashboard` 事件经 App.vue 调 `uiStore.openEntityDashboard(id)`
 * 打开；本组件常驻挂载（BaseModal 受 `open` 控制显隐），故数据加载由
 * `watch(open ? entityId : null)` 驱动——打开或切换实体时重新聚合。
 *
 * # 数据聚合方式
 * 按当前 entityId 并行 Promise.allSettled 拉取五类数据；实体名/类型优先从
 * entitiesStore.entities 解析，为空时补拉一次。实体级情感取 entity_sentiment，
 * 事件级情感分布（利好/利空/中性计数）由近期事件列表本地统计，避免再发聚合查询。
 */
import { ref, watch, computed } from 'vue'
import BaseModal from './BaseModal.vue'
import { useResearchStore } from '@/stores/research'
import { useEntitiesStore } from '@/stores/entities'
import { useEventTypesStore } from '@/stores/eventTypes'
import { entityTypeLabel as labelOf } from '@/utils/entityLabels'
import type { EventSentiment, FinancialData, CrossReferenceEntry, ArticlePreview, ResearchEventWithContext } from '@/types'

const props = defineProps<{
  /** 弹窗是否打开 */
  open: boolean
  /** 目标实体 ID（null = 未指定，打开即忽略） */
  entityId: number | null
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

/** 通知父组件关闭弹窗（Esc / 点遮罩 / 点关闭钮都走 BaseModal 的 @close） */
function onClose() {
  emit('close')
}

const researchStore = useResearchStore()
const entitiesStore = useEntitiesStore()
const eventTypesStore = useEventTypesStore()
// 仪表盘打开时加载一次生效事件类型枚举，供事件区中文标签
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void eventTypesStore.loadAll()
  },
  { immediate: true },
)

/** 实体名称（首屏兜底显示「实体 #id」） */
const entityName = ref('')
/** 实体类型中文标签 */
const entityTypeLabel = ref('')

/** 加载中 */
const loading = ref(false)
/** 加载失败信息（空串 = 成功） */
const error = ref('')

/** 实体级情感（综合舆情区展示） */
const sentiment = ref<EventSentiment | null>(null)
/** 财务指标列表 */
const financials = ref<FinancialData[]>([])
/** 近期事件列表（用于迷你时间线 + 情感分布统计） */
const events = ref<ResearchEventWithContext[]>([])
/** 多源交叉验证条目 */
const crossRefs = ref<CrossReferenceEntry[]>([])
/** 关联文章列表 */
const articles = ref<ArticlePreview[]>([])

/** 事件级情感分布计数（利好/利空/中性） */
const counts = computed(() => {
  const c = { positive: 0, negative: 0, neutral: 0 }
  for (const ev of events.value) {
    const s = (ev.sentiment ?? 'neutral') as keyof typeof c
    if (s in c) c[s] += 1
  }
  return c
})

/** 综合舆情徽标文案 */
const sentimentLabel = computed(() => {
  const s = sentiment.value?.sentiment
  if (s === 'positive') return '利好'
  if (s === 'negative') return '利空'
  if (s === 'neutral') return '中性'
  return '暂无舆情'
})

/** 综合舆情徽标样式类 */
const sentimentClass = computed(() => {
  const s = sentiment.value?.sentiment
  if (s === 'positive') return 'positive'
  if (s === 'negative') return 'negative'
  if (s === 'neutral') return 'neutral'
  return 'neutral'
})

/** 事件类型 → 中文标签（来自生效枚举：预设领域 ∪ 自定义类型） */
function typeLabel(t: string): string {
  return eventTypesStore.effectiveTypes.find((i) => i.value === t)?.label ?? t
}

/** 情感 → 中文标签 */
const SENTIMENT_LABELS: Record<string, string> = {
  positive: '利好',
  negative: '利空',
  neutral: '中性'
}
function sentLabel(s: string): string {
  return SENTIMENT_LABELS[s] ?? s
}
function sentClass(s: string): string {
  return `sentiment-${s === 'positive' || s === 'negative' || s === 'neutral' ? s : 'neutral'}`
}

/**
 * 按当前 entityId 并行聚合五类数据
 *
 * 实体名/类型优先从 `entitiesStore.entities` 解析（研究工作台打开时已由外壳加载）；
 * 若为空（如从主屏关系图入口打开）则先补拉一次实体列表再解析，保证头部信息不缺失。
 */
async function load() {
  if (props.entityId == null) return
  const id = props.entityId
  loading.value = true
  error.value = ''
  // 重置上一次的数据，避免新实体看到旧实体的残留
  sentiment.value = null
  financials.value = []
  events.value = []
  crossRefs.value = []
  articles.value = []

  // 解析实体名/类型（必要时补拉实体列表）
  if (entitiesStore.entities.length === 0) {
    try {
      await entitiesStore.loadEntities()
    } catch {
      /* 解析失败仅是头部信息缺失，不阻断主数据加载 */
    }
  }
  const ent = entitiesStore.entities.find((e) => e.id === id)
  entityName.value = ent?.name ?? `实体 #${id}`
  entityTypeLabel.value = ent ? labelOf(ent.entity_type) : ''

  // 并行拉取五类数据；任一类失败只置该类的空值，不拖垮整体（交集价值不因单点失败归零）
  const [sent, fins, refs, arts, evs] = await Promise.allSettled([
    researchStore.getEntitySentiment(id),
    researchStore.getFinancialDataByEntity(id),
    researchStore.getCrossReference(id, 0),
    researchStore.getEntityArticles(id),
    researchStore.getEntityEvents(id, 90)
  ])

  if (sent.status === 'fulfilled') sentiment.value = sent.value
  else console.error('加载实体舆情失败:', sent.reason)

  if (fins.status === 'fulfilled') financials.value = fins.value
  else console.error('加载财务数据失败:', fins.reason)

  if (refs.status === 'fulfilled') crossRefs.value = refs.value
  else console.error('加载交叉验证失败:', refs.reason)

  if (arts.status === 'fulfilled') articles.value = arts.value
  else console.error('加载关联文章失败:', arts.reason)

  if (evs.status === 'fulfilled') events.value = evs.value
  else console.error('加载近期事件失败:', evs.reason)

  loading.value = false
}

// 弹窗常驻挂载：打开或切换实体时重新聚合五类数据；关闭态（open=false）不触发加载。
// 用 `open ? entityId : null` 作为依赖，确保只在"打开且有实体"时聚合，避免关闭瞬间空跑。
watch(
  () => (props.open ? props.entityId : null),
  (id) => {
    if (id != null) void load()
  },
  { immediate: true }
)
</script>

<style scoped>
/* 弹窗内容宿主：占满 BaseModal 面板（flex 纵向），min-height:0 让内部滚动区能收缩 */
.dash-modal {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  width: 100%;
}

/* 头部：标题 + 实体名/类型 在左，关闭钮推到最右；不随内容滚动 */
.dash-modal-head {
  flex-shrink: 0;
  display: flex;
  align-items: baseline;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-5);
  padding-right: var(--sp-2);
  border-bottom: 1px solid var(--border);
  background: var(--surface);
}
.dash-head-titles {
  display: flex;
  align-items: baseline;
  gap: var(--sp-2);
  min-width: 0;
}
.dash-head-title {
  margin: 0;
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
}
.dash-head-name {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}
.dash-head-type {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}
/* 关闭钮贴右：头部整体 flex 已把标题组推到左，这里用 margin-left:auto 顶到最右 */
.dash-modal-head .btn-icon {
  margin-left: auto;
}

/* 滚动区：五区内容在此纵向滚动；面板 max-height:86vh 由 BaseModal 约束，
   本区 flex:1 + overflow 让长内容在弹窗内滚动而非撑破面板 */
.dash-modal-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-4) var(--sp-5);
}

/* 内容居中容器：整屏太宽时把文本收窄到舒适阅读宽度，但不挤占图表/事件区 */
.dash {
  max-width: 1080px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}

.dash-state {
  text-align: center;
  padding: var(--sp-10) 0;
  color: var(--text-tertiary);
  font-size: var(--fs-base);
}
.dash-error {
  color: var(--danger);
}
.dash-section {
  display: flex;
  flex-direction: column;
  gap: var(--sp-15);
}
.dash-h {
  margin: 0;
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-secondary);
  padding-bottom: var(--sp-05);
  border-bottom: 1px solid var(--border);
}
.dash-empty {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}
.dash-sentiment {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.badge {
  font-size: var(--fs-xs);
  font-weight: 600;
  padding: var(--sp-025) var(--sp-15);
  border-radius: var(--r-sm);
  color: var(--tone-fg);
}
.badge.positive { background: var(--entity-product); }
.badge.negative { background: var(--entity-regulatory); }
.badge.neutral { background: var(--entity-other); }
.dash-stat {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

/* 财务快照 */
.dash-fin {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}
.fin-item {
  display: flex;
  align-items: baseline;
  gap: var(--sp-1);
}
.fin-k {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  min-width: 72px;
}
.fin-v {
  font-size: var(--fs-md);
  color: var(--text-primary);
  font-weight: 500;
}
.fin-v em {
  font-style: normal;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.fin-p {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  margin-left: auto;
}

/* 近期事件 */
.dash-events {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}
.dash-event {
  display: flex;
  align-items: baseline;
  gap: var(--sp-15);
  padding: var(--sp-05) 0;
  border-bottom: 1px dashed var(--border);
}
.ev-type {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  font-weight: 600;
  padding: var(--sp-025) var(--sp-2);
  border-radius: var(--r-sm);
  color: var(--text-secondary);
  background: var(--border);
}
.type-product { color: var(--tone-fg); background: var(--entity-product); }
.type-executive { color: var(--tone-fg); background: var(--entity-executive); }
.type-ma { color: var(--tone-fg); background: var(--entity-ma); }
.type-regulatory { color: var(--tone-fg); background: var(--entity-regulatory); }
.type-strategy { color: var(--tone-fg); background: var(--entity-strategy); }
.type-competition { color: var(--tone-fg); background: var(--entity-competition); }
.type-industry_signal { color: var(--tone-fg); background: var(--entity-industry_signal); }
.ev-date {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
.ev-fact {
  flex: 1;
  font-size: var(--fs-sm);
  color: var(--text-primary);
  line-height: 1.5;
}

/* 多源交叉验证 */
.dash-cross {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
}
.cross-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--sp-05) 0;
  border-bottom: 1px dashed var(--border);
}
.cross-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-05);
}
.cross-src {
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-secondary);
}
.cross-sent {
  flex-shrink: 0;
  padding: 0 var(--sp-05);
  border-radius: var(--r-sm);
  font-size: var(--fs-xs);
  color: var(--tone-fg);
}
.cross-sent.sentiment-positive { background: var(--entity-product); }
.cross-sent.sentiment-negative { background: var(--entity-regulatory); }
.cross-sent.sentiment-neutral { background: var(--entity-other); }
.cross-fact {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--text-primary);
  line-height: 1.4;
  word-break: break-all;
}

/* 关联文章 */
.dash-articles {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}
.art-item {
  padding: var(--sp-025) 0;
}
.art-link {
  font-size: var(--fs-sm);
  color: var(--primary);
  text-decoration: none;
  line-height: 1.4;
  word-break: break-all;
}
.art-link:hover { text-decoration: underline; }
</style>
