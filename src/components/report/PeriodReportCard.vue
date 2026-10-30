<template>
  <!-- 报告卡片：两行人读信息替代"文件名 · 大小"。
       第一行讲这期讲了什么（类型徽标 + 标题 + 未读点），第二行给规模
       （篇数 / 主题 / 源 / 生成时间）——用户据此判断"值不值得点开"。 -->
  <div
    class="card"
    :class="{ active }"
    role="button"
    tabindex="0"
    :aria-label="`打开 ${report.title}`"
    @click="emit('open')"
    @keydown="onKey"
  >
    <div class="card-top">
      <span class="card-tag" :class="tagClass">{{ kindLabel }}</span>
      <span class="card-title">{{ report.title }}</span>
      <span v-if="!report.isRead" class="card-new" aria-label="未读"></span>
    </div>
    <div class="card-sub">
      <span>{{ report.articleCount }} 篇文章</span>
      <span class="dot">·</span>
      <span>{{ report.topicCount }} 个主题</span>
      <span class="dot">·</span>
      <span>{{ report.feedCount }} 个订阅源</span>
      <span class="dot">·</span>
      <span>{{ report.createdAt }} 生成</span>
      <template v-if="report.status === 'failed'">
        <span class="dot">·</span>
        <span class="fail" :title="report.error || '生成失败'">生成失败</span>
      </template>
    </div>

    <!-- 悬浮操作：导出 / 重新生成。
         不常驻——常驻会让每行多两个视觉噪点（与订阅源列表同一取舍）。
         `opacity:0` 但**保持可聚焦**，靠 `:focus-within` 显形，
         否则键盘 Tab 会落在看不见的按钮上。 -->
    <div class="card-actions">
      <button
        v-if="report.status !== 'failed'"
        class="mini"
        type="button"
        title="导出"
        aria-label="导出这份报告"
        @click.stop="onExport"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
          <polyline points="7 10 12 15 17 10" />
          <line x1="12" y1="15" x2="12" y2="3" />
        </svg>
      </button>
      <button
        class="mini"
        type="button"
        :title="`重新生成（将覆盖本期已有的${kindLabel}）`"
        aria-label="重新生成（覆盖本期已有内容）"
        @click.stop="emit('regenerate')"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="23 4 23 10 17 10" />
          <path d="M20.5 15a9 9 0 1 1-2.1-9.4L23 10" />
        </svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 周期报告列表卡片
 *
 * # 职责
 * 展示一份报告的摘要，并给出两个即时动作：导出、重新生成。
 * 两个动作都是 `stop` 的——否则点按钮会顺带触发整行的"打开报告"。
 *
 * # 为什么导出在卡片上也有
 * 用户往往"只想存一份"而不想先打开读一遍。菜单是**就地弹出**（不跳页）：
 * 跳页再导出会打断"挑几份存下来"的连续动作（决策⑩）。
 */
import { computed } from 'vue'
import type { PeriodSummary } from '@/stores/reportPeriod'

const props = defineProps<{
  /** 报告摘要 */
  report: PeriodSummary
  /** 是否为当前打开的那一份（列表里高亮） */
  active?: boolean
}>()

const emit = defineEmits<{
  (e: 'open'): void
  (e: 'export', anchor: { x: number; y: number }): void
  (e: 'regenerate'): void
}>()

/** 类型徽标中文名 */
const kindLabel = computed(() =>
  props.report.kind === 'weekly' ? '周报' : props.report.kind === 'monthly' ? '月报' : '日报',
)

/** 周报/月报用不同色相区分粒度（与原型一致） */
const tagClass = computed(() =>
  props.report.kind === 'weekly' ? 'week' : props.report.kind === 'monthly' ? 'month' : '',
)

/**
 * 键盘激活整行
 *
 * 必须 `e.target !== e.currentTarget` 早退：行内还嵌着导出/重新生成两个按钮，
 * 在按钮上按 Enter 会同时冒泡到这里 —— 不拦就会出现"点重新生成，顺带把报告打开"。
 *
 * @param e - 键盘事件
 */
function onKey(e: KeyboardEvent) {
  if (e.target !== e.currentTarget) return
  if (e.key !== 'Enter' && e.key !== ' ') return
  e.preventDefault()
  emit('open')
}

/**
 * 导出：把按钮位置抛给上层，由上层渲染共用菜单
 *
 * @param e - 点击事件
 */
function onExport(e: MouseEvent) {
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  emit('export', { x: rect.right - 224, y: rect.bottom + 6 })
}
</script>

<style scoped>
.card {
  position: relative;
  padding: 11px 13px;
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  background: var(--surface);
  cursor: pointer;
  transition:
    border-color var(--m-state, 160ms) ease-out,
    background var(--m-state, 160ms) ease-out;
}
.card:hover {
  border-color: color-mix(in srgb, var(--primary) 40%, var(--border));
  background: color-mix(in srgb, var(--primary) 3%, var(--surface));
}
.card:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.card.active {
  border-color: var(--primary);
}
.card-top {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: 4px;
  /* 给右上角悬浮按钮留位：`.card-actions` 是绝对定位（右 10px、宽约 54px），
     标题又是 `flex:1 1 auto` 会一路撑到右边界，长标题会被按钮压住 */
  padding-right: 58px;
}
.card-tag {
  flex-shrink: 0;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: var(--fs-modal-meta);
  font-weight: 500;
  background: var(--primary-soft);
  color: var(--primary);
}
.card-tag.week {
  background: color-mix(in srgb, var(--info) 14%, transparent);
  color: var(--info);
}
.card-tag.month {
  background: color-mix(in srgb, var(--tone-violet) 14%, transparent);
  color: var(--tone-violet);
}
.card-title {
  min-width: 0;
  flex: 1 1 auto;
  font-size: var(--fs-modal-body);
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.card-new {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--primary);
  flex-shrink: 0;
}
.card-sub {
  display: flex;
  gap: var(--sp-2);
  flex-wrap: wrap;
  align-items: center;
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
}
.card-sub .dot {
  color: var(--text-tertiary);
}
.card-sub .fail {
  color: var(--danger);
}
.card-actions {
  position: absolute;
  right: 10px;
  top: 9px;
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity var(--m-tap, 90ms) ease-out;
}
.card:hover .card-actions,
.card:focus-within .card-actions {
  opacity: 1;
}
.mini {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  border: 1px solid var(--border);
  background: var(--surface);
  border-radius: var(--r-sm);
  color: var(--text-secondary);
  cursor: pointer;
}
.mini svg {
  width: 13px;
  height: 13px;
}
.mini:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.mini:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
</style>
