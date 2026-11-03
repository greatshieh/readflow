<template>
  <!-- 今日概览：列表栏顶部的一行读数。
       折起来只有这一行；展开多一条今日阅读进度。数字全为 0 时整条不渲染。 -->
  <section v-if="hasAnything" class="today-overview" :class="{ collapsed: isCollapsed }">
    <!-- 整行可点：为了收起去抠右侧小箭头是多余的一步 -->
    <div
      class="to-head"
      role="button"
      tabindex="0"
      :aria-expanded="!isCollapsed"
      aria-controls="today-overview-body"
      :title="isCollapsed ? '展开今日阅读进度' : '收起今日阅读进度'"
      @click="toggle"
      @keydown.enter.prevent="toggle"
      @keydown.space.prevent="toggle"
    >
      <p class="to-line">
        <template v-for="(item, i) in stats" :key="item.key">
          <span v-if="i > 0" class="to-sep" aria-hidden="true">·</span>
          <span class="to-item">
            <span class="to-num" :class="{ accent: item.accent }">{{ item.value }}</span>
            <span class="to-unit">{{ item.label }}</span>
          </span>
        </template>
      </p>
      <span class="to-chevron" aria-hidden="true"></span>
    </div>

    <div v-show="!isCollapsed" class="to-body" id="today-overview-body">
      <template v-if="todayNew > 0">
        <span class="to-progress-label">今日已读 {{ readToday }} / {{ todayNew }}</span>
        <span
          class="to-progress"
          role="progressbar"
          :aria-valuenow="progressPercent"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-label="`今日已读 ${readToday} / ${todayNew}`"
        >
          <span class="to-progress-fill" :style="{ width: progressPercent + '%' }"></span>
        </span>
      </template>
      <p v-else class="to-progress-label">今天还没有新文章</p>
    </div>
  </section>
</template>

<script setup lang="ts">
/**
 * 今日概览
 *
 * # 职责
 * 在文章列表栏顶部给出一行读数：待读 / 今日新文 / 待处理，展开后加一条
 * 今日阅读进度。不承担任何操作，它是"现在什么状态"，与 `TodayBar`
 * （"现在要做什么"）分工不同，不要合并。
 *
 * # 这版删掉了什么，为什么
 * - **问候语**（早上好 / 下午好）：界面上的问候语是"这是一个网站"的信号，
 *   而不是"这是一个应用"。标题位置应该放事实，不是寒暄。
 * - **conic 进度环**：30px 的环里塞 9px 的数字，既是图表又不承担可读性，
 *   属于"仪表盘装饰"。进度本来就是一维的，用一条 2px 横条更贴切。
 * - **身份渐变顶线**：1px 的跨色相渐变，只让这条看起来"有归属感"，
 *   代价是在一个安静界面上多一道彩色边。
 * 现在整条只有两种材料：字形与一条横线。
 *
 * # 数据（全部为真实数据，不做估算）
 * - `unreadTotal`：订阅源 `unread_count` 之和（与侧栏同源，不另建口径）；
 * - `todayNew`：`displayArticles` 中发布时间落在**本地今日**的条数；
 * - `pendingTotal`：`workbenchStore` 的可见待办总数。
 * 今日新文只统计已加载到内存的文章（首屏 + 加载更多），这是刻意的：
 * 它回答"列表里有多少是今天的"，不是"库里有多少"。
 *
 * # 收起状态
 * 存 sessionStorage 而非 store：它只影响本组件渲染，与待办清单的
 * `collapsed` 语义不同（那个管的是清单展开）。两个键互不牵连。
 */

import { computed, ref } from 'vue'
import { useArticlesStore } from '@/stores/articles'
import { useFeedsStore } from '@/stores/feeds'
import { useWorkbenchStore } from '@/stores/workbench'

/** 收起状态的 sessionStorage 键：与待办的 `wb_todaybar_collapsed` 刻意分开 */
const COLLAPSED_KEY = 'wb_overview_collapsed'

const articlesStore = useArticlesStore()
const feedsStore = useFeedsStore()
const workbenchStore = useWorkbenchStore()

/** 是否收起：默认收起（只占一行，不抢列表空间） */
const isCollapsed = ref(sessionStorage.getItem(COLLAPSED_KEY) !== '1')

/** 收起 / 展开并落 sessionStorage */
function toggle() {
  isCollapsed.value = !isCollapsed.value
  sessionStorage.setItem(COLLAPSED_KEY, isCollapsed.value ? '0' : '1')
}

/** 全库待读数：与侧栏"全部文章"旁的数字同源 */
const unreadTotal = computed(() =>
  feedsStore.feeds.reduce((sum, f) => sum + (f.unread_count || 0), 0)
)

/**
 * 今日新文数
 *
 * 按**本地日**切窗：文章时间戳是 UTC 字符串，直接比字符串前缀会因时区
 * 在凌晨 0–8 点错判一天，所以换算成 Date 后比较年月日。
 */
const todayNew = computed(() => {
  const now = new Date()
  const y = now.getFullYear()
  const m = now.getMonth()
  const d = now.getDate()
  return articlesStore.displayArticles.filter((a) => {
    if (!a.published_at) return false
    const t = new Date(a.published_at)
    if (Number.isNaN(t.getTime())) return false
    return t.getFullYear() === y && t.getMonth() === m && t.getDate() === d
  }).length
})

/** 今日新文里还没读的条数 */
const unreadToday = computed(() => {
  const now = new Date()
  const y = now.getFullYear()
  const m = now.getMonth()
  const d = now.getDate()
  return articlesStore.displayArticles.filter((a) => {
    if (a.is_read || !a.published_at) return false
    const t = new Date(a.published_at)
    return (
      !Number.isNaN(t.getTime()) &&
      t.getFullYear() === y &&
      t.getMonth() === m &&
      t.getDate() === d
    )
  }).length
})

/** 今日已读 = 今日新文 − 今日未读 */
const readToday = computed(() => Math.max(0, todayNew.value - unreadToday.value))

/** 今日阅读进度百分比（整数）。今日无新文时不渲染进度条，故不做除零兜底。 */
const progressPercent = computed(() => {
  if (todayNew.value === 0) return 0
  return Math.round((readToday.value / todayNew.value) * 100)
})

/** 待处理数：待办 store 的可见总数（已忽略与已处理的条目不在其中） */
const pendingTotal = computed(() => workbenchStore.visibleTotal)

/**
 * 一行读数
 *
 * `accent` 交给"待处理"：整屏只有这一处用主色，因为它是唯一会催人
 * 起身做事 的数字。其余数字用正文色，靠 tabular-nums 对齐。
 */
const stats = computed(() => [
  { key: 'unread', value: unreadTotal.value, label: '篇待读', accent: false },
  { key: 'today', value: todayNew.value, label: '篇今日新增', accent: false },
  {
    key: 'pending',
    value: pendingTotal.value,
    label: '项待处理',
    accent: pendingTotal.value > 0,
  },
])

/** 三个数字全为 0 时不占用首屏空间 */
const hasAnything = computed(
  () => unreadTotal.value > 0 || todayNew.value > 0 || pendingTotal.value > 0
)
</script>

<style scoped>
/* 横带本体：只有底边一根发丝线与下面隔开。没有渐变、没有背景色块——
   一行读数不该看起来像一个模块。 */
.today-overview {
  flex-shrink: 0;
  border-bottom: 1px solid var(--shell-hair);
}

.to-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  min-height: 40px;
  padding: var(--sp-2) var(--sp-4);
  cursor: pointer;
  transition: background var(--dur) var(--ease);
}
.to-head:hover { background: var(--fill); }
.to-head:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}

/* 读数行：数字与单位同一行，用 · 分隔。数字 14px（--fs-md）而不是 22px——
   这一条是背景信息，抢占视线的应该是文章列表本身。 */
.to-line {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: var(--sp-15);
  font-size: var(--fs-md);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.to-item {
  display: inline-flex;
  align-items: baseline;
  gap: var(--sp-05);
}
.to-num {
  font-variant-numeric: tabular-nums;
  font-weight: 500;
  color: var(--text-primary);
}
/* 整屏唯一的主色数字：只有"待处理"会催人做事 */
.to-num.accent { color: var(--primary); }
.to-unit { color: var(--text-secondary); }
.to-sep { color: var(--text-tertiary); }

/* 展开箭头：两条边拼一个 V，不给 8px 的图形塞一条 SVG 路径 */
.to-chevron {
  flex-shrink: 0;
  width: 6px;
  height: 6px;
  margin-right: 2px;
  border-right: 1.5px solid var(--text-tertiary);
  border-bottom: 1.5px solid var(--text-tertiary);
  transform: rotate(45deg) translateY(-2px);
  transition: transform var(--dur) var(--ease);
}
.today-overview.collapsed .to-chevron { transform: rotate(-45deg) translateY(0); }

/* 展开区：进度条 + 一行说明。进度条 2px，实心主色，无渐变。 */
.to-body {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: 0 var(--sp-4) var(--sp-3);
}
.to-progress-label {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
.to-progress {
  flex: 1 1 auto;
  min-width: 0;
  height: 2px;
  border-radius: var(--r-pill);
  background: var(--fill-secondary);
  overflow: hidden;
}
.to-progress-fill {
  display: block;
  height: 100%;
  background: var(--primary);
  transition: width var(--dur) var(--ease);
}
</style>
