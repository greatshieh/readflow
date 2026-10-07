<!--
  ArticleToc.vue — 正文目录浮层

  职责：把父组件从正文 DOM 里提取出的标题渲染成一份可跳转的目录，
  并在阅读过程中标记「当前章节」。

  为什么条目由父组件提取、本组件只负责呈现与跳转：
  标题元素只能在正文容器的 DOM 上取，而那个容器（`contentBodyRef`）住在
  `ContentColumn` 里。把观察与滚动也放回父组件会让本已 1900+ 行的
  `ContentColumn` 继续膨胀，因此这里只保留「面板内部」的职责。

  三个实现要点：
  1. **跳转不用 `scrollIntoView`**：它会连带滚动所有可滚祖先，而这次点击
     只应影响正文滚动容器。改为自行换算相对位移后设 `scrollTop`。
  2. **当前章节靠 IntersectionObserver**，不接管父组件的 `@scroll`
     （那是阅读进度上报的 passive 处理器，不该被塞进额外计算）。
  3. **点击后短暂锁住高亮**：平滑滚动期间观察器仍在报告旧位置，
     不锁会让高亮先跳到目标、再被旧位置拽回去、最后又跳回来。
-->
<template>
  <div class="toc-body">
    <div class="toc-head">
      <span>目录</span>
      <span class="toc-count">{{ items.length }}</span>
    </div>
    <div class="toc-list" ref="listRef">
      <button
        v-for="(item, i) in items"
        :key="i"
        class="toc-item"
        :class="{ active: i === activeIndex }"
        :style="{ paddingLeft: `calc(var(--sp-15) + var(--sp-2) * ${item.level - baseLevel})` }"
        :title="item.text"
        @click="goTo(i)"
      >
        <span class="toc-item-text">{{ item.text }}</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { TocItem } from '@/utils/articleToc'

const props = defineProps<{
  /** 目录条目，由父组件从已渲染的正文 DOM 中提取 */
  items: TocItem[]
  /** 正文滚动容器：既是观察器的 root，也是跳转的滚动目标 */
  scrollEl: HTMLElement | null
}>()

/** 点击跳转后锁住高亮的时长（毫秒）：需覆盖平滑滚动，否则高亮会来回跳 */
const SCROLL_LOCK_MS = 600
/** 跳转落点距容器顶部的留白，避免标题紧贴上沿 */
const SCROLL_OFFSET = 8
/** 探测带高度占比：只有落在容器顶部这条带里的标题才算「当前章节」 */
const BAND_BOTTOM_MARGIN = '-58%'

const listRef = ref<HTMLElement | null>(null)
/** 当前高亮的条目下标 */
const activeIndex = ref(0)

/** 缩进基准取文章内最浅的层级：有的文章直接从 h2 起，从 1 起算会凭空多缩一级 */
const baseLevel = computed(() =>
  props.items.length ? Math.min(...props.items.map((it) => it.level)) : 1
)

/** 每个条目最近一次的观测状态；`states` 用普通 Map，不参与响应式 */
const states = new Map<Element, 'passed' | 'band' | 'below'>()
let observer: IntersectionObserver | null = null
let lockTimer: ReturnType<typeof setTimeout> | undefined

/**
 * 当前是否处于「减弱动效」偏好下
 *
 * @returns true 表示应跳转即时完成、不做平滑滚动
 */
function reducedMotion(): boolean {
  return (
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  )
}

/**
 * 依据观测状态反推当前章节
 *
 * 取「已经到达过」的最靠后一条——即在探测带内、或已滚到带上方。
 * 只看「带内」会在滚到文章底部时丢掉高亮：那时没有任何标题落在顶部带里。
 *
 * 跳转锁定期间直接返回，交由解锁时的补算收敛。
 */
function recompute(): void {
  if (lockTimer !== undefined) return
  let best = -1
  props.items.forEach((item, i) => {
    const s = states.get(item.el)
    if (s === 'passed' || s === 'band') best = i
  })
  if (best >= 0) activeIndex.value = best
}

/** 建立观察器；条目或滚动容器变化时需先 `stop()` 再重建 */
function start(): void {
  stop()
  const root = props.scrollEl
  if (!root || typeof IntersectionObserver === 'undefined') return
  states.clear()
  const io = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        const rootTop = entry.rootBounds?.top ?? 0
        if (entry.isIntersecting) {
          states.set(entry.target, 'band')
        } else {
          // 观察器不区分「在上方」与「在下方」，用与 root 的相对位置自行判断：
          // 少了这一步就无法在滚过最后一条标题后继续高亮它
          states.set(entry.target, entry.boundingClientRect.top < rootTop ? 'passed' : 'below')
        }
      }
      recompute()
    },
    { root, rootMargin: `0px 0px ${BAND_BOTTOM_MARGIN} 0px`, threshold: 0 }
  )
  for (const item of props.items) io.observe(item.el)
  observer = io
}

/** 断开观察器并释放引用 */
function stop(): void {
  observer?.disconnect()
  observer = null
}

/**
 * 跳转到第 `i` 条
 *
 * @param i - 目录条目下标；越界或容器缺失时为空操作
 */
function goTo(i: number): void {
  const item = props.items[i]
  const container = props.scrollEl
  if (!item || !container) return

  const relative =
    item.el.getBoundingClientRect().top - container.getBoundingClientRect().top
  container.scrollTo({
    top: Math.max(0, container.scrollTop + relative - SCROLL_OFFSET),
    behavior: reducedMotion() ? 'auto' : 'smooth'
  })

  // 先乐观高亮，再锁住观察器直到滚动结束，避免高亮被途中的旧位置拽回去
  activeIndex.value = i
  if (lockTimer !== undefined) clearTimeout(lockTimer)
  lockTimer = setTimeout(() => {
    lockTimer = undefined
    recompute()
  }, SCROLL_LOCK_MS)
}

/**
 * 让高亮条目在目录自身的滚动区里保持可见
 *
 * 长文目录实测最多 53 条，滚动正文时高亮条目很容易滑出可视区。
 * 自行比较 `offsetTop` 而不用 `scrollIntoView`：后者同样会波及祖先滚动容器。
 */
function keepActiveVisible(): void {
  const list = listRef.value
  const el = list?.children[activeIndex.value]
  if (!list || !(el instanceof HTMLElement)) return
  const top = el.offsetTop
  const bottom = top + el.offsetHeight
  if (top < list.scrollTop) list.scrollTop = top
  else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight
}

watch(activeIndex, keepActiveVisible)

// 条目或滚动容器换了（切文章、切原文/译文）就重建观察器
watch(
  () => [props.items, props.scrollEl],
  () => {
    activeIndex.value = 0
    start()
  }
)

onMounted(start)

onBeforeUnmount(() => {
  stop()
  if (lockTimer !== undefined) clearTimeout(lockTimer)
})
</script>

<style scoped>
.toc-body {
  display: flex;
  flex-direction: column;
  min-width: 200px;
  max-width: min(280px, 70vw);
}

.toc-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  padding: var(--sp-05) var(--sp-15) var(--sp-1);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.toc-count {
  font-variant-numeric: tabular-nums;
}

.toc-list {
  /* 定位上下文：条目的 offsetTop 需相对本容器，可见性判断才准确 */
  position: relative;
  max-height: 52vh;
  overflow-y: auto;
  padding-bottom: var(--sp-1);
}

.toc-item {
  display: block;
  width: 100%;
  padding: var(--sp-05) var(--sp-15);
  border: 0;
  border-radius: var(--r-sm);
  background: transparent;
  font-family: inherit;
  font-size: var(--fs-sm);
  line-height: 1.4;
  color: var(--text-secondary);
  text-align: left;
  cursor: pointer;
}

.toc-item:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}

.toc-item.active {
  background: var(--primary-soft);
  color: var(--primary);
  font-weight: 500;
}

.toc-item-text {
  display: block;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
</style>
