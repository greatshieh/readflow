<template>
  <!-- 详情卡：点击节点后浮在画布左上角，显示该实体的类型 / 命中数 / 情感倾向 / 财务数据 / 共现最强的邻居 -->
  <div class="eg-detail">
    <div class="eg-detail-head">
      <span class="eg-detail-name">{{ name }}</span>
      <button class="eg-detail-close" @click="emit('close')" title="关闭">×</button>
    </div>
    <p class="eg-detail-meta">{{ typeLabel }} · 命中 {{ articleCount }} 篇</p>

    <!-- 情感倾向（若存在） -->
    <div v-if="sentiment" class="eg-sentiment-badge" :class="sentimentClass">
      <span class="eg-sentiment-label">{{ sentimentLabel }}</span>
      <span v-if="sentimentConfidence != null" class="eg-sentiment-conf">
        置信度 {{ (sentimentConfidence * 100).toFixed(0) }}%
      </span>
    </div>

    <!-- 财务指标（若有） -->
    <div v-if="!loading && financials && financials.length > 0" class="eg-financial-section">
      <p class="eg-detail-sub">财务指标</p>
      <ul class="eg-detail-list eg-financial-list">
        <li v-for="f in financials" :key="f.id" class="eg-financial-item">
          <span class="eg-financial-key">{{ f.metric_key }}</span>
          <span class="eg-financial-val">{{ f.metric_value }}</span>
          <span v-if="f.currency" class="eg-financial-cur">{{ f.currency }}</span>
        </li>
      </ul>
    </div>
    <div v-else class="eg-detail-sub eg-detail-empty">无财务数据</div>

    <!-- 共现最强的关联实体 -->
    <p class="eg-detail-sub">共现最强的关联实体</p>
    <ul class="eg-detail-list">
      <li v-for="nb in neighbors" :key="nb.id" class="eg-detail-nb-item" @click="emit('selectNeighbor', nb.id)" title="点击查看该实体的文章">
        <span class="eg-detail-nb">{{ nb.name }}</span>
        <span class="eg-detail-w">共现 {{ nb.weight }} 篇</span>
      </li>
      <li v-if="neighbors.length === 0" class="eg-detail-empty">没有与其他实体共现</li>
    </ul>

    <!-- 选中邻居的文章列表 -->
    <div v-if="selectedArticles && selectedArticles.length > 0" class="eg-articles-section">
      <p class="eg-detail-sub">相关文章</p>
      <ul class="eg-detail-list eg-articles-list">
        <li v-for="a in selectedArticles" :key="a.id" class="eg-article-item">
          <a :href="a.link" target="_blank" class="eg-article-link">{{ a.title }}</a>
        </li>
      </ul>
    </div>
    <div v-else-if="articlesLoading" class="eg-detail-sub eg-detail-empty">加载中...</div>
    <div v-else class="eg-detail-sub eg-detail-empty">点击关联实体查看文章</div>

    <!-- 多源交叉验证：同一实体在不同订阅源中的报道并列对比 -->
    <div class="eg-cross-section">
      <p class="eg-detail-sub">来源验证（{{ crossRefs?.length ?? 0 }} 条）</p>
      <div v-if="crossRefsLoading" class="eg-detail-sub eg-detail-empty">加载中...</div>
      <ul v-else-if="crossRefs && crossRefs.length > 0" class="eg-detail-list eg-cross-list">
        <li v-for="c in crossRefs" :key="c.event_id" class="eg-cross-item">
          <div class="eg-cross-head">
            <span class="eg-cross-source" :title="c.source_name ?? ''">{{ c.source_name ?? '未知来源' }}</span>
            <span class="eg-cross-sentiment" :class="`eg-sentiment-${crossSentimentClass(c.sentiment)}`">
              {{ crossSentimentLabel(c.sentiment) }}
            </span>
          </div>
          <p class="eg-cross-fact">{{ c.fact }}</p>
          <a v-if="c.article_link" :href="c.article_link" target="_blank" class="eg-article-link">
            {{ c.article_title ?? c.article_link }}
          </a>
        </li>
      </ul>
      <div v-else class="eg-detail-sub eg-detail-empty">该实体暂无多源报道</div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 实体关系图的详情卡
 *
 * # 职责
 * 纯展示：渲染选中实体的名称 / 类型 / 命中数 / 情感倾向 / 财务指标 / 共现邻居列表。
 * 数据全部由父组件（EntityGraph）从图数据派生后以 props 传入，
 * 选中态归父组件所有——本组件只上抛「关闭」。
 *
 * # 样式边界
 * `.eg-detail*` 样式随组件搬入；画布容器（`.eg-wrap`）与定位相关的样式仍在父组件，
 * 卡片以 `position: absolute` 锚定其上（父容器的 scoped 样式管不到这里的类名，
 * 但 absolute 定位只依赖祖先有 `position: relative`，语义成立）。
 */
import { computed } from 'vue'

const props = defineProps<{
  /** 选中实体名称 */
  name: string
  /** 已格式化的类型中文标签（由父组件经 entityTypeLabel 派生） */
  typeLabel: string
  /** 命中文章数 */
  articleCount: number
  /** 共现邻居（父组件已按共现数降序） */
  neighbors: Array<{ id: number; name: string; weight: number }>
  /** 情感倾向：positive / negative / neutral / null */
  sentiment?: string | null
  /** 情感置信度 0-1 */
  sentimentConfidence?: number | null
  /** 财务指标列表 */
  financials?: Array<{
    id: number
    metric_key: string
    metric_value: string
    currency?: string
    period?: string
  }>
  /** 数据加载中 */
  loading?: boolean
  /** 选中邻居后加载的文章列表 */
  selectedArticles?: Array<{ id: number; title: string; link: string }>
  /** 文章加载中 */
  articlesLoading?: boolean
  /** 多源交叉验证条目（同一实体在不同订阅源中的报道） */
  crossRefs?: Array<{
    event_id: number
    article_id: number
    fact: string
    article_title: string | null
    article_link: string | null
    source_name: string | null
    sentiment: string
  }>
  /** 交叉验证数据加载中 */
  crossRefsLoading?: boolean
}>()

const emit = defineEmits<{
  /** 点击 ×：父组件把选中态置空 */
  (e: 'close'): void
  /** 点击邻居实体：父组件加载该实体的文章列表 */
  (e: 'selectNeighbor', entityId: number): void
}>()

/** 情感标签映射 */
const SENTIMENT_LABELS: Record<string, string> = {
  positive: '利好',
  negative: '利空',
  neutral: '中性'
}

/** 情感 CSS class 映射 */
const SENTIMENT_CLASSES: Record<string, string> = {
  positive: 'positive',
  negative: 'negative',
  neutral: 'neutral'
}

const sentimentLabel = computed(() => {
  if (!props.sentiment) return ''
  return SENTIMENT_LABELS[props.sentiment] ?? props.sentiment
})

const sentimentClass = computed(() => {
  if (!props.sentiment) return ''
  return `eg-sentiment-${SENTIMENT_CLASSES[props.sentiment] ?? 'neutral'}`
})

/**
 * 交叉验证条目的情感标签
 *
 * @param s - 情感倾向字符串
 * @returns 中文标签（未知值原样返回）
 */
function crossSentimentLabel(s: string): string {
  return SENTIMENT_LABELS[s] ?? s
}

/**
 * 交叉验证条目的情感 CSS class 后缀
 *
 * @param s - 情感倾向字符串
 * @returns positive / negative / neutral（未知值回退 neutral）
 */
function crossSentimentClass(s: string): string {
  return SENTIMENT_CLASSES[s] ?? 'neutral'
}
</script>

<style scoped>
/* 样式自 EntityGraph 原样搬入（.eg-detail 全族）。 */
.eg-detail {
  position: absolute;
  top: var(--sp-15);
  left: var(--sp-15);
  width: 210px;
  max-height: calc(100% - var(--sp-5));
  display: flex;
  flex-direction: column;
  padding: var(--sp-15);
  border: 1px solid var(--panel-border);
  border-radius: var(--r-lg);
  background: var(--surface);
  box-shadow: var(--shadow-pop);
}

.eg-detail-head {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
}

.eg-detail-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--text-primary);
}

.eg-detail-close {
  border: none;
  background: none;
  color: var(--text-tertiary);
  cursor: pointer;
  font-size: 15px;
  line-height: 1;
}

.eg-detail-close:hover {
  color: var(--text-primary);
}

.eg-detail-meta {
  margin: var(--sp-025) 0 var(--sp-1);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.eg-detail-sub {
  margin: 0 0 var(--sp-05);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.eg-detail-list {
  /* flex:1 + min-height:0：列表自己滚，卡片不被撑破 */
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 0;
  padding: 0;
  list-style: none;
  font-size: var(--fs-sm);
}

.eg-detail-list li {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-025) 0;
}

.eg-detail-nb {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
}

.eg-detail-nb-item {
  cursor: pointer;
  border-radius: var(--r-sm);
  transition: background var(--dur) var(--ease);
}

.eg-detail-nb-item:hover {
  /* 此前写 var(--surface-elevated, var(--bg-elevated))——两个 token 都未定义，
     整条声明被静默作废（var 未定义规则），悬停底色从未生效，改用既有 hover 语义色 */
  background: var(--hover-bg);
}

.eg-detail-nb-item:hover .eg-detail-nb {
  color: var(--primary);
}

.eg-detail-w {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.eg-detail-empty {
  color: var(--text-tertiary);
  font-size: var(--fs-xs);
}

/* 情感徽标 */
.eg-sentiment-badge {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-025) var(--sp-1);
  border-radius: var(--r-sm);
  font-size: var(--fs-xs);
  font-weight: 600;
  margin-bottom: var(--sp-05);
}

.eg-sentiment-positive {
  color: var(--tone-fg);
  background: var(--entity-product);
}

.eg-sentiment-negative {
  color: var(--tone-fg);
  background: var(--entity-regulatory);
}

.eg-sentiment-neutral {
  color: var(--tone-fg);
  background: var(--entity-other);
}

.eg-sentiment-conf {
  font-weight: 400;
  opacity: 0.8;
}

/* 财务指标区 */
.eg-financial-section {
  margin-bottom: var(--sp-05);
}

.eg-financial-list {
  max-height: 120px;
}

.eg-financial-item {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  padding: var(--sp-025) 0;
}

.eg-financial-key {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.eg-financial-val {
  font-size: var(--fs-sm);
  color: var(--text-primary);
  font-weight: 500;
}

.eg-financial-cur {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

/* 关联文章列表 */
.eg-articles-section {
  margin-top: var(--sp-05);
  padding-top: var(--sp-05);
  border-top: 1px solid var(--border);
}

.eg-articles-list {
  max-height: 150px;
}

.eg-article-item {
  padding: var(--sp-025) 0;
}

.eg-article-link {
  font-size: var(--fs-xs);
  color: var(--primary);
  text-decoration: none;
  word-break: break-all;
  display: block;
  line-height: 1.4;
}

.eg-article-link:hover {
  text-decoration: underline;
}

/* 多源交叉验证区：与关联文章区同样式分段，列表自己滚动 */
.eg-cross-section {
  margin-top: var(--sp-05);
  padding-top: var(--sp-05);
  border-top: 1px solid var(--border);
}

.eg-cross-list {
  max-height: 180px;
}

.eg-cross-item {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  padding: var(--sp-025) 0;
}

.eg-cross-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-05);
  width: 100%;
}

.eg-cross-source {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-secondary);
}

.eg-cross-sentiment {
  flex-shrink: 0;
  padding: 0 var(--sp-05);
  border-radius: var(--r-sm);
  font-size: var(--fs-xs);
  color: var(--tone-fg);
}

/* 复用情感徽标配色：正=产品绿 / 负=监管红 / 中性=灰 */
.eg-cross-sentiment.eg-sentiment-positive {
  background: var(--entity-product);
}

.eg-cross-sentiment.eg-sentiment-negative {
  background: var(--entity-regulatory);
}

.eg-cross-sentiment.eg-sentiment-neutral {
  background: var(--entity-other);
}

.eg-cross-fact {
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--text-primary);
  line-height: 1.4;
  word-break: break-all;
}
</style>
