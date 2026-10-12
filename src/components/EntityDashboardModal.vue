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
  <BaseModal :open="open" size="3xl" title="实体仪表盘" @close="onClose">
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
          <!-- Hero 头：实体名大标题 + 类型徽标 -->
          <header class="tech-hero">
            <h3 class="tech-hero-title">{{ entityName }}</h3>
            <div class="tech-hero-chips">
              <span v-if="entityTypeLabel" class="badge">{{ entityTypeLabel }}</span>
              <span class="badge" :class="sentimentClass">{{ sentimentLabel }}</span>
            </div>
          </header>

          <!-- 关键指标四宫格：数据全部来自已加载的聚合结果，零新增查询 -->
          <div class="tech-kpi-grid">
            <div class="tech-kpi">
              <div class="tech-kpi-label">财务</div>
              <div class="tech-kpi-value">{{ financials.length }}<em>项</em></div>
            </div>
            <div class="tech-kpi">
              <div class="tech-kpi-label">事件</div>
              <div class="tech-kpi-value">{{ events.length }}</div>
            </div>
            <div class="tech-kpi">
              <div class="tech-kpi-label">来源验证</div>
              <div class="tech-kpi-value">{{ crossRefs.length }}</div>
            </div>
            <div class="tech-kpi">
              <div class="tech-kpi-label">关联文章</div>
              <div class="tech-kpi-value">{{ articles.length }}</div>
            </div>
          </div>

          <!-- 行情面板（外部数据源能力，突出显示） -->
          <section class="dash-card">
            <h4 class="dash-h">行情面板（外部数据源）</h4>
            <MarketPanel :entity-id="entityId" />
          </section>

          <!-- 双列紧凑区：综合舆情 + 财务快照 -->
          <div class="dash-row">
            <section class="dash-card">
              <h4 class="dash-h">综合舆情</h4>
              <div class="dash-sentiment">
                <span class="badge" :class="sentimentClass">{{ sentimentLabel }}</span>
                <span class="dash-stat">
                  利好 {{ counts.positive }} · 利空 {{ counts.negative }} · 中性 {{ counts.neutral }}
                </span>
              </div>
            </section>

            <section class="dash-card">
              <h4 class="dash-h">财务快照</h4>
              <ul v-if="financials.length" class="dash-fin">
                <li v-for="f in financials" :key="f.id" class="fin-item">
                  <span class="fin-k">{{ metricLabel(f.metric_key) }}</span>
                  <span class="fin-v">{{ f.metric_value }}<em v-if="f.currency"> {{ f.currency }}</em></span>
                  <span v-if="f.period" class="fin-p">{{ f.period }}</span>
                </li>
              </ul>
              <p v-else class="dash-empty">暂无财务数据</p>
            </section>
          </div>

          <!-- 近期事件：按事件类型分组，组内按日期倒序（零新增查询，纯前端分组） -->
          <section class="dash-card">
            <h4 class="dash-h">近期事件</h4>
            <div v-if="groupedEvents.length" class="tech-groups">
              <section
                v-for="(grp, gi) in groupedEvents"
                :key="grp.type"
                class="tech-group"
              >
                <!-- 分组头：序号 chip + 类型名 + 计数徽标 -->
                <header class="tech-group-head">
                  <span class="tech-index">{{ pad2(gi + 1) }}</span>
                  <span class="tech-group-title">{{ grp.label }}</span>
                  <span class="tech-count">{{ grp.items.length }} 条</span>
                </header>

                <!-- 组内卡片流 -->
                <div class="tech-timeline">
                  <article
                    v-for="(ev, ei) in grp.items"
                    :key="ev.id"
                    class="tech-tl-item"
                  >
                    <div class="tech-card">
                      <span class="tech-card-num">{{ pad2(ei + 1) }}</span>
                      <div class="tech-card-body">
                        <h5 class="tech-card-title">{{ ev.fact }}</h5>
                        <div class="tech-meta">
                          <span class="tech-meta-time">{{ ev.event_date || '日期未知' }}</span>
                          <span class="badge" :class="`type-${ev.event_type}`">
                            {{ typeLabel(ev.event_type) }}
                          </span>
                          <span class="badge" :class="sentClass(ev.sentiment ?? 'neutral')">
                            {{ sentLabel(ev.sentiment ?? 'neutral') }}
                          </span>
                          <span v-if="ev.entity_name" class="tech-meta-src">
                            {{ ev.entity_name }}
                          </span>
                        </div>
                        <p v-if="ev.evidence" class="tech-summary">{{ ev.evidence }}</p>
                        <!-- source 跳转：点则收缩弹窗并在主区域打开文章；无链接时禁用 -->
                        <button
                          class="tech-link"
                          :disabled="!ev.article_link"
                          @click="openArticle(ev)"
                          :title="ev.article_link ? '在主区域打开来源文章' : '来源文章已被清理，无法跳转'"
                        >
                          查看原文
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <line x1="7" y1="17" x2="17" y2="7"></line>
                            <polyline points="7 7 17 7 17 17"></polyline>
                          </svg>
                        </button>
                      </div>
                    </div>
                  </article>
                </div>
              </section>
            </div>
            <p v-else class="dash-empty">暂无相关事件</p>
          </section>

          <!-- 多源交叉验证 -->
          <section class="dash-card">
            <h4 class="dash-h">来源验证（{{ crossRefs.length }}）</h4>
            <div v-if="crossRefs.length" class="tech-grid">
              <div v-for="c in crossRefs" :key="c.event_id" class="tech-item">
                <div class="tech-meta">
                  <span class="tech-meta-src">{{ c.source_name ?? '未知来源' }}</span>
                  <span class="badge" :class="sentClass(c.sentiment)">{{ sentLabel(c.sentiment) }}</span>
                </div>
                <p class="tech-summary">{{ c.fact }}</p>
              </div>
            </div>
            <p v-else class="dash-empty">该实体暂无多源报道</p>
          </section>

          <!-- 关联文章 -->
          <section class="dash-card">
            <h4 class="dash-h">关联文章</h4>
            <div v-if="hasNameMatch" class="dash-reextract">
              <button
                class="btn btn-secondary btn-sm"
                :disabled="reextracting"
                @click="handleReextract"
              >
                {{ reextracting ? '抽取中...' : '重新抽取历史事件' }}
              </button>
              <span class="dash-reextract-hint">「名称匹配」文章尚未经 AI 抽取，点击补齐历史事件</span>
            </div>
            <div v-if="articles.length" class="tech-grid">
              <div v-for="a in articles" :key="a.id" class="tech-item">
                <div class="tech-meta">
                  <span v-if="a.match_type === 'name'" class="badge">名称匹配</span>
                  <button
                    class="tech-link"
                    :disabled="!a.link"
                    @click="openArticleById(a)"
                  >
                    {{ a.title }}
                  </button>
                </div>
              </div>
            </div>
            <p v-else class="dash-empty">暂无关联文章</p>
          </section>
        </div>
      </div>
    </div>

    <!-- 收缩态：右上角悬浮按钮（Teleport 到 body，避开面板 overflow 裁剪）
         v-if 额外要求 props.open：父组件真正关闭弹窗后（关闭钮/Esc）必须一并消失，
         否则会留下一个"召回已关闭仪表盘"的孤儿按钮。 -->
    <Teleport to="body">
      <button
        v-if="open && collapsed"
        class="tech-collapse-fab"
        @click="expand"
        title="返回实体仪表盘"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="15 18 9 12 15 6"></polyline>
        </svg>
        <span>实体仪表盘</span>
        <span class="tech-fab-badge">{{ events.length }}</span>
      </button>
    </Teleport>
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
import { ref, watch, computed, nextTick, onMounted, onBeforeUnmount } from 'vue'
import BaseModal from './BaseModal.vue'
import MarketPanel from './MarketPanel.vue'
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
/** 重新抽取（补历史事件）按钮 loading */
const reextracting = ref(false)
/** 是否存在"名称匹配"文章（尚未经过 AI 抽取，可触发补抽） */
const hasNameMatch = computed(() => articles.value.some((a) => a.match_type === 'name'))

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

/** 序号补零：分组序号与卡片序号统一两位（01 / 02），对齐科技风的数字锚点语汇 */
function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n)
}

/**
 * 近期事件按「事件类型」分组，组内按日期倒序
 *
 * # 为什么在前端分组
 * `events` 已是 `getEntityEvents` 拉回的完整列表（含 event_type / event_date），
 * 分组只是换个展示顺序，**不需要任何新增查询**——这与项目「零新增请求」的边界一致。
 *
 * # 排序规则
 * 组间：保持后端返回的顺序（即按该类型的最新事件聚在一起，语义稳定，
 * 不做二次排序以免与用户对"主要类型"的直觉错位）；组内：`event_date` 倒序，
 * 日期为空的事件排在最后（空值统一兜底成空串，避免 `''` 排在最前）。
 *
 * # 日期解析
 * `event_date` 形如 `YYYY-MM-DD`（可能为空）。用字符串比较即可——
 * 该格式字典序与时间序一致，无需 `new Date()`（避免时区把日期偏移一天）。
 */
const groupedEvents = computed(() => {
  const map = new Map<string, ResearchEventWithContext[]>()
  for (const ev of events.value) {
    const key = ev.event_type || 'other'
    const list = map.get(key)
    if (list) list.push(ev)
    else map.set(key, [ev])
  }
  return Array.from(map.entries()).map(([type, items]) => ({
    type,
    label: typeLabel(type),
    items: items.slice().sort((a, b) => (b.event_date ?? '').localeCompare(a.event_date ?? ''))
  }))
})

/** 是否处于收缩态（弹窗已隐藏但状态保留，由右上角悬浮按钮召回） */
const collapsed = ref(false)

/** 展开（从收缩态恢复弹窗）。nextTick 等 DOM 回来后再聚焦，键盘用户不错位。 */
async function expand() {
  collapsed.value = false
  await nextTick()
}

/**
 * 打开某条事件的来源文章：收缩弹窗 + 在主区域正文区打开 + 高亮订阅与列表
 *
 * # 交互契约（需求：source 跳转但不关闭仪表盘）
 * 1. 先收缩（`collapsed=true`）——弹窗只是隐藏，组件不销毁，
 *    数据与滚动位置原样保留，用户可随时用右上角按钮召回；
 * 2. 派发 `dashboard-jump` 事件，由 ArticleColumn 消费：
 *    切订阅源 → 展开对应折叠分组 → 打开文章 → 滚动并高亮该条目；
 * 3. 真正关闭仍只走关闭钮或 Esc（收缩态下 BaseModal 已出栈、
 *    它自身的 Esc 失效，故由本组件的 keydown 监听接管，见 `onKeydown`）。
 *
 * # 为什么传 articleId 而不是链接
 * `ResearchEvent` 自带 `article_id`（事件与来源文章的关联键），
 * 直接用 id 让订阅栏能精确定位所属源、用列表能精确定位条目；
 * 若传链接则订阅源无从得知，"高亮对应订阅"这条需求就无法满足。
 *
 * 用 window CustomEvent 而非直接调 store：仪表盘可能来自研究工作台/关系图等多入口，
 * 事件解耦后各订阅方只关心自己那部分，避免仪表盘硬依赖三栏布局的实现细节
 * （与命令面板的 `command-palette-jump` 同一思路）。
 */
function openArticle(ev: ResearchEventWithContext) {
  if (!ev.article_link) return
  collapsed.value = true
  window.dispatchEvent(
    new CustomEvent('dashboard-jump', { detail: { articleId: ev.article_id, articleLink: ev.article_link } })
  )
}

/**
 * 打开关联文章区的一条文章（同 `openArticle` 的跳转契约，只是入参形态不同）
 *
 * 关联文章走的是 `ArticlePreview`（含文章 id 与原文链接，但**不含 feed_id**——
 * 后端 `list_articles_by_entity` 只回预览字段）。因此这里只派发文章 id：
 * 由 ArticleColumn 在列表里定位该文章，订阅源则由 `selectArticle` 内部
 * 依据文章自身的 feed_id 自动切换（见 stores/articles.ts），
 * 避免为一次跳转在预览结构上新增字段（预览结构是后端 SQL 定死的）。
 */
function openArticleById(a: ArticlePreview) {
  if (!a.link) return
  collapsed.value = true
  window.dispatchEvent(new CustomEvent('dashboard-jump', { detail: { articleId: a.id } }))
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
 * 财务指标键 → 中文标签
 *
 * `metric_key` 由 AI 抽取时自由生成（英文 snake_case，无固定枚举），为保证财务快照区
 * 对中文用户可读，这里维护一份常见键的中文映射。命中即显示中文；未命中（模型产出
 * 的非预期键）回退原键，避免空白或乱码。新增常见键只需在此表补充。
 */
const METRIC_LABELS: Record<string, string> = {
  revenue: '营收',
  revenue_growth: '营收增速',
  net_revenue: '净营收',
  sales: '销售额',
  income: '收入',
  gross_profit: '毛利',
  gross_margin: '毛利率',
  operating_profit: '营业利润',
  operating_margin: '营业利润率',
  net_profit: '净利润',
  net_margin: '净利率',
  profit: '利润',
  eps: '每股收益',
  roe: '净资产收益率',
  roa: '总资产收益率',
  roi: '投资回报率',
  market_cap: '市值',
  market_value: '市值',
  total_assets: '总资产',
  total_liabilities: '总负债',
  total_equity: '净资产',
  debt_ratio: '资产负债率',
  pe: '市盈率',
  pb: '市净率',
  ps: '市销率',
  pct: '市现率',
  growth_rate: '增长率',
  growth: '增长',
  acquisition_price: '收购价格',
  valuation: '估值',
  funding: '融资额',
  investment: '投资额',
  cost: '成本',
  expense: '费用',
  rd_spend: '研发费用',
  rd_expense: '研发费用',
  rd_intensity: '研发费用率',
  cash_flow: '现金流',
  free_cash_flow: '自由现金流',
  dividend: '股息',
  dividend_yield: '股息率',
  users: '用户数',
  subscribers: '订阅用户数',
  mau: '月活用户',
  dau: '日活用户',
  orders: '订单数',
  shipments: '出货量',
  production: '产量',
  guidance: '业绩指引'
}
function metricLabel(k: string): string {
  return METRIC_LABELS[k] ?? k
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

/**
 * 为当前实体重抽历史事件（补齐既往未识别的 AI 事件）
 *
 * 仅当存在"名称匹配"文章时可用：这些文章已被 `list_articles_by_entity` 零成本关联，
 * 但尚无 AI 事件。点击后逐篇补抽，完成后重新聚合（文章关联 / 近期事件都会刷新）。
 */
async function handleReextract() {
  if (reextracting.value || props.entityId == null) return
  reextracting.value = true
  try {
    await researchStore.reextractEntity(props.entityId)
    // 补抽完成后重新聚合五类数据（文章关联 / 近期事件都会刷新）
    await load()
  } catch (e) {
    console.error('重新抽取历史事件失败:', e)
  } finally {
    reextracting.value = false
  }
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

/**
 * 父组件真正关闭弹窗时清掉收缩态
 *
 * 若不复位，`collapsed` 会残留为 true，下次打开弹窗时 BaseModal 直接可见
 * （`:class="{show: open}"` 只看 open），但悬浮按钮的 `v-if="open && collapsed"`
 * 也会同时成立——于是弹窗和"召回按钮"一起出现，状态错乱。
 */
watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) collapsed.value = false
  }
)

/**
 * 收缩态下的 Esc = 彻底关闭仪表盘
 *
 * # 为什么需要自己监听
 * 收缩态下 BaseModal 已因 `open=false` 出栈，它的 Esc 监听随之失效（`dialogStack`
 * 里也不再登记本弹窗），用户按 Esc 不会有任何反应——但需求明确"关闭只走关闭钮或
 * Esc"，所以这里补一个仅在「打开且已收缩」时生效的监听，把 Esc 解释为关闭。
 *
 * # 为什么不与 BaseModal 抢
 * 展开态下 BaseModal 自己处理 Esc（会先走 dialogStack 判定与全局确认框让路逻辑）；
 * 本监听在展开态直接 return，两者不会同时响应。
 */
function onKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  if (!props.open || !collapsed.value) return
  onClose()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
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
  max-width: 100%;
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
/* 分区卡片化：每区包成浅色卡片，整体从「设置页列表」变「仪表盘」 */
.dash-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  padding: var(--sp-3) var(--sp-4);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}

/* 顶部关键指标概览带：5 块指标自适应排布，窄屏自动换行 */

/* 双列紧凑区：综合舆情 + 财务快照 并排；窄屏自动塌成单列 */
.dash-row {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--sp-4);
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
  min-width: 100px;
  flex-shrink: 0;
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
.type-product { color: var(--tone-fg); background: var(--entity-product); }
.type-executive { color: var(--tone-fg); background: var(--entity-executive); }
.type-ma { color: var(--tone-fg); background: var(--entity-ma); }
.type-regulatory { color: var(--tone-fg); background: var(--entity-regulatory); }
.type-strategy { color: var(--tone-fg); background: var(--entity-strategy); }
.type-competition { color: var(--tone-fg); background: var(--entity-competition); }
.type-industry_signal { color: var(--tone-fg); background: var(--entity-industry_signal); }

/* 多源交叉验证 */

/* 关联文章 */

/* 文章项：链接 + 标签同行，标签不挤压链接 */

/* 名称匹配标签：区分「AI 已归类」与「仅文本命中」两类关联文章 */

/* 重新抽取历史事件操作条 */
.dash-reextract {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
  margin-bottom: var(--sp-2);
  padding: var(--sp-2);
  background: var(--fill);
  border-radius: var(--r-md);
}
.dash-reextract-hint {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
</style>
