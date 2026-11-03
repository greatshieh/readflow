<template>
  <!-- 复盘卡片：一条判断 + 它的证据结构 + 可做的处置。
       三个动作（保持 / 改置信 / 改状态）就地展开，不弹窗——复盘是连续处理几十条的
       场景，每点一次都弹一个框会让人放弃使用。补判据要走编辑器，故 emit 给父组件。 -->
  <article class="rv-card" :class="{ 'is-due': card.overdueDays !== null }">
    <header class="rv-head">
      <span class="rv-entity">{{ card.entityName || '未关联实体' }}</span>
      <span class="rv-badge">{{ statusLabel }}</span>
      <span class="rv-badge">置信 {{ card.confidence }}</span>
      <span v-if="card.overdueDays !== null" class="rv-badge rv-badge-due">
        {{ card.overdueDays === 0 ? '今天到期' : `逾期 ${card.overdueDays} 天` }}
      </span>
    </header>

    <p class="rv-statement">{{ card.statement }}</p>

    <div class="rv-meta">
      <span>支持 {{ card.supportCount }}</span>
      <span>反驳 {{ card.opposeCount }}</span>
      <span v-if="card.mixedCount > 0">存疑 {{ card.mixedCount }}</span>
      <span class="rv-sep">·</span>
      <span>登记 {{ card.ageDays }} 天</span>
      <span class="rv-sep">·</span>
      <span v-if="card.lastEvidenceDays !== null">最近证据 {{ card.lastEvidenceDays }} 天前</span>
      <span v-else class="rv-muted">从未挂证据</span>
      <template v-if="card.lastReviewAt">
        <span class="rv-sep">·</span>
        <span class="rv-muted">上次复盘 {{ shortDate(card.lastReviewAt) }}</span>
      </template>
    </div>

    <!-- 命中的健康规则：文案由后端拼好，这里只渲染。反驳/同源类问题常被忽略，
         所以整块用左侧色条标出，不缩成一行小字 -->
    <ul v-if="card.hits.length" class="rv-hits">
      <li v-for="h in card.hits" :key="h.rule" :class="`rv-hit rv-hit-${h.severity}`">
        <span class="rv-hit-rule">{{ h.rule }}</span>{{ h.text }}
      </li>
    </ul>

    <div v-if="card.falsifier" class="rv-falsifier">
      <span class="rv-f-label">判据</span>
      <span>{{ card.falsifier.statement }}</span>
    </div>
    <div v-else class="rv-suggest">
      <span class="rv-f-label">还没有判据，可以这样写</span>
      <button
        v-for="(s, i) in card.suggestedFalsifiers"
        :key="i"
        type="button"
        class="rv-suggest-item"
        @click="emit('edit-falsifier', s)"
      >
        {{ s }}
      </button>
    </div>

    <div class="rv-actions">
      <button type="button" class="rv-btn" :disabled="busy" @click="keep">保持</button>
      <button type="button" class="rv-btn" :disabled="busy" @click="toggle('confidence')">改置信</button>
      <button type="button" class="rv-btn" :disabled="busy" @click="toggle('status')">改状态</button>
      <button type="button" class="rv-btn" :disabled="busy" @click="emit('edit-falsifier', '')">
        {{ card.falsifier ? '改判据' : '补判据' }}
      </button>
    </div>

    <!-- 改置信：1–5 就地选，理由一句。理由必填是刻意的——半年后理由比结论有用 -->
    <div v-if="panel === 'confidence'" class="rv-panel">
      <div class="rv-conf-row">
        <button
          v-for="n in 5"
          :key="n"
          type="button"
          class="rv-conf-dot"
          :class="{ on: draftConfidence === n }"
          :aria-pressed="draftConfidence === n"
          @click="draftConfidence = n"
        >
          {{ n }}
        </button>
      </div>
      <input v-model="reason" class="rv-reason" placeholder="为什么改？（必填）" />
      <div class="rv-panel-actions">
        <button type="button" class="rv-btn rv-btn-primary" :disabled="busy" @click="submitConfidence">
          保存
        </button>
        <button type="button" class="rv-btn" @click="panel = ''">取消</button>
      </div>
    </div>

    <!-- 改状态：归档是"这条判断我不再跟了"，与"否定了"是两件事，故分开 -->
    <div v-if="panel === 'status'" class="rv-panel">
      <div class="rv-status-row">
        <button
          v-for="s in STATUS_OPTIONS"
          :key="s.value"
          type="button"
          class="rv-btn"
          :class="{ 'rv-btn-on': draftStatus === s.value }"
          @click="draftStatus = s.value"
        >
          {{ s.label }}
        </button>
      </div>
      <input v-model="reason" class="rv-reason" placeholder="为什么改？（必填）" />
      <div class="rv-panel-actions">
        <button type="button" class="rv-btn rv-btn-primary" :disabled="busy" @click="submitStatus">
          保存
        </button>
        <button type="button" class="rv-btn" @click="panel = ''">取消</button>
      </div>
    </div>
  </article>
</template>

<script setup lang="ts">
/**
 * 复盘卡片
 *
 * # 职责
 * 只渲染一张卡片并就地收集"改置信 / 改状态"的输入；补判据交给父组件的编辑器
 * （判据有三段式结构，塞进卡片会把几十张卡片都撑开）。
 *
 * # 为什么"保持"是零输入
 * 复盘里最常见的处置是"看过了，判断没变"。如果它也要求填理由，用户会直接关页面。
 * 而"看了没改"本身是重要信息——它是 `thesis_review_items` 存在的全部理由。
 */
import { computed, ref } from 'vue'
import type { MarkAction, ReviewCard } from '@/stores/review'

const props = defineProps<{
  /** 卡片数据 */
  card: ReviewCard
  /** 有处置在途（防连点） */
  busy: boolean
}>()

const emit = defineEmits<{
  /** 记一次处置 */
  (e: 'mark', action: MarkAction, payload: { confidence?: number; status?: string; reason?: string }): void
  /** 请求编辑判据（建议句可能为空串） */
  (e: 'edit-falsifier', suggestion: string): void
}>()

/** 当前展开的面板：'' = 都收起 */
const panel = ref<'' | 'confidence' | 'status'>('')
/** 草稿置信 */
const draftConfidence = ref(props.card.confidence)
/**
 * 草稿状态
 *
 * 刻意用 `string` 而不是 `ReviewCard['status']`：归档不是卡片会出现的状态
 * （后端 `WHERE status <> 'archived'` 已排除），但它是**可选的目标状态**。
 * 用卡片的联合类型会让"归档"这个选项无法赋值。
 */
const draftStatus = ref<string>(props.card.status)
/** 理由（两个面板共用一个输入框，同一时刻只会展开一个） */
const reason = ref('')

/** 状态下拉选项：归档单列，因为"不再跟了"与"判断被否定"是两件事 */
const STATUS_OPTIONS = [
  { value: 'active', label: '跟进中' },
  { value: 'confirmed', label: '确认' },
  { value: 'refuted', label: '否定' },
  { value: 'archived', label: '归档' },
] as const

const statusLabel = computed(
  () => STATUS_OPTIONS.find(s => s.value === props.card.status)?.label ?? props.card.status,
)

/**
 * 展开 / 收起某个面板
 *
 * @param next - 目标面板
 */
function toggle(next: 'confidence' | 'status'): void {
  panel.value = panel.value === next ? '' : next
  reason.value = ''
  draftConfidence.value = props.card.confidence
  draftStatus.value = props.card.status
}

/** 保持：零输入，直接记一条 unchanged */
function keep(): void {
  emit('mark', 'unchanged', {})
}

/** 提交改置信 */
function submitConfidence(): void {
  if (draftConfidence.value === props.card.confidence) {
    // 值没变就不是"改置信"，按"保持"记：否则变更史上会出现一条 from==to 的空记录
    keep()
    panel.value = ''
    return
  }
  emit('mark', 'confidence', { confidence: draftConfidence.value, reason: reason.value.trim() })
  panel.value = ''
}

/** 提交改状态 */
function submitStatus(): void {
  if (draftStatus.value === props.card.status) {
    keep()
    panel.value = ''
    return
  }
  emit('mark', 'status', { status: draftStatus.value, reason: reason.value.trim() })
  panel.value = ''
}

/**
 * 把 `YYYY-MM-DD HH:MM:SS` 截成 `MM-DD`
 *
 * 卡片上只需要"多久以前"的量级，完整时间戳会把一行挤满。
 *
 * @param ts - 时间戳字符串
 * @returns 短日期；无法解析时原样返回
 */
function shortDate(ts: string): string {
  const m = ts.match(/^(\d{4})-(\d{2})-(\d{2})/)
  return m ? `${m[2]}-${m[3]}` : ts
}
</script>

<style scoped>
.rv-card {
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-3);
  background: var(--surface);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
/* 到期的卡片用左侧色条，不整块染红：一屏可能有十几张，全染色会看不出重点 */
.rv-card.is-due {
  border-left: 3px solid var(--warning);
}

.rv-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.rv-entity {
  font-size: var(--fs-sm);
  font-weight: 500;
  color: var(--text-primary);
}
.rv-badge {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  background: var(--fill);
  border-radius: var(--r-sm);
  padding: 1px 6px;
  flex-shrink: 0;
  white-space: nowrap;
}
.rv-badge-due {
  color: var(--warning-text);
  background: color-mix(in srgb, var(--warning) 12%, transparent);
}

.rv-statement {
  margin: 0;
  font-size: var(--fs-md);
  line-height: 1.6;
  color: var(--text-primary);
}

.rv-meta {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
  font-size: var(--fs-xs);
  color: var(--text-secondary);
}
.rv-sep {
  opacity: 0.4;
}
.rv-muted {
  color: var(--text-tertiary, var(--text-secondary));
  opacity: 0.8;
}

.rv-hits {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.rv-hit {
  font-size: var(--fs-xs);
  line-height: 1.55;
  padding: 4px 8px;
  border-radius: var(--r-sm);
  border-left: 2px solid transparent;
}
.rv-hit-high {
  color: var(--danger-text);
  background: color-mix(in srgb, var(--danger) 8%, transparent);
  border-left-color: var(--danger);
}
.rv-hit-mid {
  color: var(--warning-text);
  background: color-mix(in srgb, var(--warning) 8%, transparent);
  border-left-color: var(--warning);
}
.rv-hit-low {
  color: var(--text-secondary);
  background: var(--fill);
  border-left-color: var(--border);
}
.rv-hit-rule {
  font-family: var(--font-mono, monospace);
  margin-right: 6px;
  opacity: 0.75;
}

.rv-falsifier,
.rv-suggest {
  display: flex;
  gap: var(--sp-2);
  font-size: var(--fs-xs);
  line-height: 1.55;
  color: var(--text-secondary);
}
.rv-suggest {
  flex-direction: column;
}
.rv-f-label {
  color: var(--text-tertiary, var(--text-secondary));
  flex-shrink: 0;
  opacity: 0.85;
}
.rv-suggest-item {
  text-align: left;
  border: 1px dashed var(--border);
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  padding: 5px 8px;
  cursor: pointer;
  line-height: 1.5;
}
.rv-suggest-item:hover {
  border-color: var(--primary);
  color: var(--text-primary);
}
.rv-suggest-item:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}

.rv-actions {
  display: flex;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.rv-btn {
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-secondary);
  border-radius: var(--r-sm);
  font: inherit;
  font-size: var(--fs-xs);
  padding: 5px 10px;
  cursor: pointer;
  min-height: 26px;
}
.rv-btn:hover:not(:disabled) {
  border-color: var(--primary);
  color: var(--primary);
}
.rv-btn:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.rv-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.rv-btn-primary {
  background: var(--primary);
  border-color: var(--primary);
  color: #fff;
}
.rv-btn-primary:hover:not(:disabled) {
  color: #fff;
  filter: brightness(1.06);
}
.rv-btn-on {
  border-color: var(--primary);
  color: var(--primary-text);
  background: var(--primary-soft, color-mix(in srgb, var(--primary) 10%, transparent));
}

.rv-panel {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  padding: var(--sp-2);
  border-radius: var(--r-sm);
  background: var(--fill);
}
.rv-conf-row,
.rv-status-row {
  display: flex;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.rv-conf-dot {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-secondary);
  font: inherit;
  font-size: var(--fs-xs);
  cursor: pointer;
}
.rv-conf-dot.on {
  border-color: var(--primary);
  background: var(--primary);
  color: #fff;
}
.rv-conf-dot:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
.rv-reason {
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-primary);
  font: inherit;
  font-size: var(--fs-xs);
  padding: 6px 8px;
}
.rv-reason:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -1px;
}
.rv-panel-actions {
  display: flex;
  gap: var(--sp-2);
}
</style>
