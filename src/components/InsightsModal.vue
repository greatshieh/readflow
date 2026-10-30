<template>
  <!-- 阅读洞察看板：纯前端聚合，数据源为 db_stats 与 stats_daily_read 两个既有命令 -->
  <BaseModal :open="open" title="阅读洞察" size="xl" @close="emit('close')">
    <!-- 补视觉标题：BaseModal 的 title 属性只写进 aria-label，屏幕上原本一个字都没有，
         于是这个弹窗没有"它是什么"的第一行。标题放在 .ins 之外，滚动时不跟着走。 -->
    <div class="ins-head">
      <span class="m-title">阅读洞察</span>
    </div>
    <div class="ins">
      <div v-if="loading && !stats" class="ins-state">加载中…</div>
      <div v-else-if="!stats" class="ins-state ins-error">数据加载失败，请稍后重试</div>
      <template v-else>
        <!-- 全库指标卡 -->
        <section class="ins-metrics">
          <div v-for="m in metricCards" :key="m.label" class="ins-card">
            <span class="ins-num">{{ m.value }}</span>
            <span class="ins-label">{{ m.label }}</span>
          </div>
        </section>

        <!-- 阅读趋势图 -->
        <section class="ins-chart-block">
          <div class="ins-chart-head">
            <h3 class="ins-h">阅读趋势</h3>
            <div class="ins-range">
              <button
                v-for="d in ranges"
                :key="d"
                class="ins-range-btn"
                :class="{ active: days === d }"
                @click="days = d; load()"
              >
                {{ d }} 天
              </button>
            </div>
          </div>

          <div v-if="series.length" class="ins-chart">
            <svg :viewBox="`0 0 ${chart.W} ${chart.H}`" class="ins-svg">
              <!-- 横向网格 + Y 轴刻度 -->
              <line
                v-for="(t, i) in chart.yTicks"
                :key="'grid' + i"
                :x1="padX"
                :x2="chart.W - padX"
                :y1="t.y"
                :y2="t.y"
                class="ins-grid"
              />
              <text
                v-for="(t, i) in chart.yTicks"
                :key="'yt' + i"
                :x="padX - 6"
                :y="t.y + 3"
                class="ins-axis"
                text-anchor="end"
              >{{ t.text }}</text>

              <!-- 面积 + 折线 -->
              <path :d="chart.areaPath" class="ins-area" />
              <path :d="chart.linePath" class="ins-line" />

              <!-- X 轴日期标签 -->
              <text
                v-for="(l, i) in chart.labels"
                :key="'xl' + i"
                :x="l.x"
                :y="chart.H - 6"
                class="ins-axis"
                text-anchor="middle"
              >{{ l.text }}</text>
            </svg>
          </div>
          <div v-else class="ins-state">所选区间内没有已读记录</div>

          <!-- 由趋势派生的指标 -->
          <div class="ins-derived">
            <span>区间总阅读 <b>{{ totalRead }}</b></span>
            <span>日均 <b>{{ avgRead }}</b></span>
            <span>峰值 <b>{{ peak.count }}</b>（{{ peak.day }}）</span>
            <span>连续阅读 <b>{{ streak }}</b> 天</span>
          </div>
        </section>
      </template>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 阅读洞察看板
 *
 * # 职责
 * 把既有的 `db_stats`（全库计数）与 `stats_daily_read`（逐日已读）聚合成可视化看板：
 * 指标卡 + 内联 SVG 阅读趋势图 + 由趋势派生的区间统计。
 *
 * # 设计约束
 * - **纯前端、零后端改动**：不新增任何命令，也不引入图表库（用内联 SVG 手绘）。
 * - 数据全来自现有命令，符合实施计划对 P1-C「无后端改动」的约定。
 * - 注：计划文档原列的「高分文章占比 / 实体覆盖数」当前无对应后端命令，v1 暂不实现
 *   （若要做需另立后端命令，走单独确认）。
 *
 * # 数据来源
 * - `db_stats` → 全库文章 / 已读 / 未读 / 收藏 / 今日已读 / 订阅源计数；
 * - `stats_daily_read(days)` → 窗口内每天的已读数，用于画趋势与派生指标。
 */

import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import BaseModal from './BaseModal.vue'
import type { Stats } from '@/types'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

/** 全库统计 */
const stats = ref<Stats | null>(null)
/** 趋势窗口（天） */
const days = ref(14)
/** 可选窗口档位 */
const ranges = [7, 14, 30]
/** 逐日已读序列 */
const series = ref<{ day: string; count: number }[]>([])
/** 加载态 */
const loading = ref(false)

/** SVG 绘图区内边距（X 轴留给刻度文字） */
const padX = 36
const PAD_Y = 22
const CHART_W = 640
const CHART_H = 200

/**
 * 拉取看板数据
 *
 * 两个命令并行请求，失败仅打日志，由模板的空态 / 错误态兜底。
 */
async function load() {
  loading.value = true
  try {
    const [s, raw] = await Promise.all([
      invoke<Stats>('db_stats'),
      invoke<[string, number][]>('stats_daily_read', { days: days.value }),
    ])
    stats.value = s
    series.value = raw.map(([day, count]) => ({ day, count }))
  } catch (e) {
    console.error('加载阅读洞察失败:', e)
    stats.value = null
  } finally {
    loading.value = false
  }
}

// 每次打开弹窗重新拉取（天数切换时也会主动调用 load）
watch(
  () => props.open,
  (v) => {
    if (v) void load()
  },
)

/** 指标卡数据（全库计数） */
const metricCards = computed(() => {
  const s = stats.value
  if (!s) return []
  return [
    { label: '文章总数', value: s.articles },
    { label: '已读', value: s.read },
    { label: '未读', value: s.unread },
    { label: '收藏', value: s.bookmarks },
    { label: '今日已读', value: s.read_today },
    { label: '订阅源', value: s.feeds },
  ]
})

/** SVG 折线 / 面积 / 坐标轴几何 */
const chart = computed(() => {
  const data = series.value
  const n = data.length
  const innerW = CHART_W - padX * 2
  const innerH = CHART_H - PAD_Y * 2
  const maxY = Math.max(1, ...data.map((d) => d.count))
  const xAt = (i: number) => (n <= 1 ? padX + innerW / 2 : padX + (i / (n - 1)) * innerW)
  const yAt = (v: number) => PAD_Y + innerH - (v / maxY) * innerH
  const pts = data.map((d, i) => ({ x: xAt(i), y: yAt(d.count), d }))

  const linePath = pts
    .map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x.toFixed(1)},${p.y.toFixed(1)}`)
    .join(' ')
  const baseline = (CHART_H - PAD_Y).toFixed(1)
  const areaPath = pts.length
    ? `M${pts[0].x.toFixed(1)},${baseline} ` +
      pts.map((p) => `L${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(' ') +
      ` L${pts[pts.length - 1].x.toFixed(1)},${baseline} Z`
    : ''

  // X 轴最多 6 个标签，避免拥挤
  const step = Math.max(1, Math.ceil(n / 6))
  const labels = pts
    .filter((_, i) => i % step === 0)
    .map((p) => ({ x: p.x, text: p.d.day.slice(5) })) // MM-DD
  const yTicks = [0, maxY / 2, maxY].map((v) => ({ y: yAt(v), text: String(Math.round(v)) }))

  return { W: CHART_W, H: CHART_H, linePath, areaPath, labels, yTicks }
})

/** 区间总阅读 */
const totalRead = computed(() => series.value.reduce((a, d) => a + d.count, 0))
/** 区间日均 */
const avgRead = computed(() =>
  series.value.length ? Math.round(totalRead.value / series.value.length) : 0,
)
/** 阅读峰值（天数 + 数量） */
const peak = computed(() =>
  series.value.reduce((m, d) => (d.count > m.count ? d : m), { day: '—', count: 0 }),
)
/** 从末日回溯的连续阅读天数 */
const streak = computed(() => {
  let s = 0
  for (let i = series.value.length - 1; i >= 0; i--) {
    if (series.value[i].count > 0) s++
    else break
  }
  return s
})
</script>

<style scoped>
.ins-head {
  display: flex;
  align-items: center;
  padding: var(--sp-35) var(--sp-6);
  border-bottom: 1px solid var(--border);
  flex: 0 0 auto;
}
.m-title {
  font-size: var(--fs-modal-title);
  font-weight: 600;
  line-height: var(--lh-modal-title);
  color: var(--text-primary);
}
.ins {
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
  padding: var(--sp-5) var(--sp-6);
  /* 面板是 max-height:86vh + overflow:hidden，自己并不滚。图表 + 图例 + 明细在
     小屏或放大字号后必然超出，超出部分原本直接消失且滚不到。
     min-height:0 是关键：缺了它 flex 子项不会收缩，overflow 永远不生效。 */
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}
.ins-state {
  padding: var(--sp-5);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
}
.ins-error { color: var(--danger, #e5484d); }

/* 指标卡 */
.ins-metrics {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--sp-2);
}
.ins-card {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--sp-3) var(--sp-3);
  background: var(--fill);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
}
.ins-num {
  /* 原为 1.5rem（=24px，阶梯里没有这一档，也无法随界面缩放联动） */
  font-size: var(--fs-modal-num);
  font-weight: 700;
  line-height: var(--lh-modal-num);
  color: var(--text-primary);
  /* 负字距是给拉丁字母排紧的手法；汉字字面本身满格，压字距会让笔画粘连 —— 删除 */
  font-variant-numeric: tabular-nums;
}
.ins-label {
  font-size: var(--fs-modal-meta);
  line-height: var(--lh-modal-meta);
  color: var(--text-tertiary);
}

/* 趋势图 */
.ins-chart-block {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.ins-chart-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.ins-h {
  margin: 0;
  /* 区块标题：与弹窗标题拉开一级（16 vs 18），而不是靠 700 硬顶 */
  font-size: var(--fs-modal-section);
  font-weight: 600;
  line-height: var(--lh-modal-section);
  color: var(--text-primary);
}
.ins-range {
  display: flex;
  gap: var(--sp-1);
}
.ins-range-btn {
  padding: 4px 10px;
  font-size: var(--fs-xs);
  border-radius: var(--r-sm);
  border: 1px solid var(--border);
  background: var(--fill);
  color: var(--text-secondary);
  cursor: pointer;
  /* 原先用 all 过渡 —— 会连带过渡布局属性（每帧重排），且整条覆盖了全局
     button 基线的六属性过渡。只列真正会变的三个。 */
  transition:
    background var(--m-tap) var(--m-ease-out),
    color var(--m-tap) var(--m-ease-out),
    border-color var(--m-tap) var(--m-ease-out);
}
.ins-range-btn:hover { background: var(--hover-bg); }
.ins-range-btn.active {
  background: var(--primary);
  border-color: var(--primary);
  color: #fff;
}
.ins-chart {
  width: 100%;
  background: var(--fill);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-2);
}
.ins-svg {
  display: block;
  width: 100%;
  height: auto;
}
.ins-grid {
  stroke: var(--border);
  stroke-width: 1;
  stroke-dasharray: 3 3;
}
.ins-axis {
  fill: var(--text-tertiary);
  /* 原为裸写 11px：轴刻度属于元信息，走角色 token 才不会再"多一个 11px" */
  font-size: var(--fs-modal-meta);
  font-variant-numeric: tabular-nums;
}
.ins-area {
  fill: color-mix(in srgb, var(--primary) 14%, transparent);
  stroke: none;
}
.ins-line {
  fill: none;
  stroke: var(--primary);
  stroke-width: 2;
  stroke-linejoin: round;
  stroke-linecap: round;
}
.ins-derived {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-4);
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--text-secondary);
}
.ins-derived b {
  color: var(--text-primary);
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
</style>
