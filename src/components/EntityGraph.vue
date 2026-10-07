<template>
  <!--
    实体关系图：零依赖力导向布局 + SVG 渲染。

    为什么自己写而不用 d3：项目至今零图形依赖（package.json 里只有 vue/pinia），
    而本图的节点规模是"关注实体数"级别（当前实测 34 个节点 / 113 条边），
    O(n²) 的斥力计算每帧不到两千次乘法——引一个几十 KB 的力导向库换不来任何
    可感知收益，只多一份长期维护与版本升级面。布局算法本身（库仑斥力 + 边弹簧
    + 向心力 + 速度阻尼）不到 40 行。
  -->
  <div class="eg-wrap" ref="wrapRef">
    <svg
      class="eg-svg"
      :viewBox="`0 0 ${size.w} ${size.h}`"
      @pointerdown="onCanvasDown"
      @wheel.prevent="onWheel"
    >
      <g :transform="`translate(${view.tx} ${view.ty}) scale(${view.k})`">
        <!-- 边：底层先画，节点覆盖其上 -->
        <line
          v-for="(l, i) in links"
          :key="`l${i}`"
          class="eg-edge"
          :class="{ 'is-active': isEdgeActive(l), 'is-dim': dimOthers && !isEdgeActive(l) }"
          :x1="nodes[l.a].x"
          :y1="nodes[l.a].y"
          :x2="nodes[l.b].x"
          :y2="nodes[l.b].y"
          :stroke-width="edgeWidth(l.weight)"
        />

        <!-- 节点：圆 + 挂在圆下方的标签 -->
        <g
          v-for="(n, i) in nodes"
          :key="n.id"
          class="eg-node"
          :class="{ 'is-dim': dimOthers && !isNeighbor(n.id) && n.id !== focusId }"
          @pointerdown.stop.prevent="onNodeDown(i)"
          @pointerenter="hoverId = n.id"
          @pointerleave="hoverId = null"
          @click.stop="toggleSelect(n.id)"
        >
          <circle
            class="eg-dot"
            :class="[`eg-type-${n.typeClass}`, { 'is-focus': n.id === focusId, 'is-pinned': n.pinned }]"
            :r="nodeRadius(n.article_count)"
            :cx="n.x"
            :cy="n.y"
          />
          <text
            class="eg-label"
            :class="{ 'is-focus': n.id === focusId }"
            :x="n.x"
            :y="n.y + nodeRadius(n.article_count) + 12"
          >
            {{ n.name }}
          </text>
        </g>
      </g>
    </svg>

    <!-- 工具条：缩放 / 适应 / 重排（浮在画布右上角） -->
    <div class="eg-tools">
      <button class="btn btn-sm" @click="zoomBy(1.25)" title="放大">+</button>
      <button class="btn btn-sm" @click="zoomBy(0.8)" title="缩小">−</button>
      <button class="btn btn-sm" @click="fitView" title="适应视图">适应</button>
      <button class="btn btn-sm" @click="rebuild" title="重新布局（清除手动拖动）">重排</button>
    </div>

    <!-- 图例：颜色 → 实体类型 -->
    <div class="eg-legend">
      <span v-for="t in ENTITY_LEGEND" :key="t.key" class="eg-legend-item">
        <i class="eg-legend-dot" :class="`eg-type-${t.key}`"></i>{{ t.label }}
      </span>
    </div>

    <!-- 详情卡：点击节点后显示其共现最强的邻居（外观与交互见 EntityGraphDetail.vue） -->
    <EntityGraphDetail
      v-if="focus"
      :name="focus.name"
      :type-label="entityTypeLabel(focus.entity_type)"
      :article-count="focus.article_count"
      :neighbors="neighbors"
      @close="selectedId = null"
    />

    <p v-if="showHint" class="eg-hint">拖拽节点可固定位置 · 拖拽空白平移 · 滚轮缩放</p>
  </div>
</template>

<script setup lang="ts">
/**
 * 实体关系图（力导向布局）
 *
 * # 职责
 * 把后端 `entity_graph` 返回的「节点 + 边」渲染成一张可交互的关系图：
 * 自写力导向布局（斥力 / 弹簧 / 向心力 / 阻尼）收敛到稳定形态后交由 SVG 绘制，
 * 并支持拖拽节点（拖过的节点会被钉住，不被物理弹回）、平移缩放、
 * 悬停高亮邻接、点击查看详情卡。
 *
 * # 设计意图
 * - **零依赖**：不引 d3/echarts。节点规模是"关注实体数"级别（实测 34 个），
 *   O(n²) 斥力每帧不到两千次运算，自写 40 行远比引库划算。
 * - **确定性初始布局**：用黄金角螺旋而非 `Math.random()` 播种，同一份数据
 *   每次打开得到同一张图，便于对照与复现问题。
 * - **数据不可变**：组件不修改传入的 `nodes/edges`，而是在内部克隆出带
 *   坐标与速度的仿真副本（`props` 是后端返回的只读载荷）。
 * - **尊重减弱动效**：`prefers-reduced-motion` 下不做逐帧动画，直接同步
 *   迭代到收敛再一次性渲染，避免眩晕。
 *
 * # 与父组件的关系
 * 由 `ResearchModal.vue` 的「关系图」页签渲染。选中状态与详情卡都在组件内部
 * 自洽（详情卡内容全部来自 `nodes`/`edges` 本身），因此不需要任何 emit。
 */

import { ref, computed, watch, onBeforeUnmount, onMounted, nextTick } from 'vue'
import type { GraphNode, GraphEdge } from '@/types'
import EntityGraphDetail from './EntityGraphDetail.vue'
import { useGraphView } from '@/composables/useGraphView'
import { ENTITY_LEGEND, entityTypeClass, entityTypeLabel } from '@/utils/entityLabels'

const props = defineProps<{
  /** 图节点（后端 `entity_graph` 返回） */
  nodes: GraphNode[]
  /** 图的边（后端 `entity_graph` 返回，保证 source < target） */
  edges: GraphEdge[]
}>()

// ─── 常量（布局与视觉参数集中在此，便于调参）────────────────────────────────

/** 初始播种角度：黄金角，保证任意节点数下都均匀铺开而不重叠 */
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))
/** 斥力系数（库仑力，与距离平方成反比） */
const REPULSION = 4200
/** 弹簧系数与静止长度（边的自然长度） */
const SPRING = 0.012
const REST_LENGTH = 130
/** 向心力系数：把所有节点往原点收，否则连通分量会互相推开到看不见 */
const CENTER_PULL = 0.006
/** 速度阻尼与单帧最大位移（防抖） */
const DAMPING = 0.85
const MAX_STEP = 12
/** 最大迭代轮数与收敛判据（单位位移总和） */
const MAX_TICKS = 500
const ENERGY_EPS = 0.35



// ─── 选中状态 ────────────────────────────────────────────────────────────────

/** 悬停 / 选中的节点 ID */
const hoverId = ref<number | null>(null)
const selectedId = ref<number | null>(null)

/** 是否处于「聚焦某个节点」的状态：此时非邻接元素淡出 */
const focusId = computed(() => hoverId.value ?? selectedId.value)
const dimOthers = computed(() => focusId.value !== null)

/** 仿真节点：在传入节点上附加坐标与速度（组件私有副本） */
interface SimNode {
  id: number
  name: string
  entity_type: string
  article_count: number
  /** 已归一化的 CSS 类后缀（未知类型回退 other，避免出现无样式的节点） */
  typeClass: string
  x: number
  y: number
  vx: number
  vy: number
  /** 是否被用户拖拽钉住（钉住的节点不再被物理力移动，只对别人施加斥力） */
  pinned: boolean
}
const nodes = ref<SimNode[]>([])
/** 仿真边：把实体 ID 换成节点数组下标，避免每帧做 Map 查找 */
const links = ref<Array<{ a: number; b: number; weight: number }>>([])
/** 邻接表：ID → 邻居 ID 集合（悬停高亮用） */
const adjacency = ref(new Map<number, Set<number>>())

// 视图变换（画布尺寸 / 平移缩放 / 适应视图）托管给 useGraphView；
// positions 传只读坐标来源，radiusOf 让包围盒计算与渲染半径同源。
const { wrapRef, size, view, fitView, zoomBy, onWheel, onCanvasDown } = useGraphView({
  positions: computed(() => nodes.value),
  radiusOf: nodeRadius,
})

/** 选中的节点对象 */
const focus = computed(() => nodes.value.find((n) => n.id === selectedId.value) ?? null)

/** 选中节点的邻居（按共现数降序） */
const neighbors = computed(() => {
  const id = selectedId.value
  if (id === null) return []
  const byId = new Map(nodes.value.map((n) => [n.id, n]))
  return props.edges
    .filter((e) => e.source === id || e.target === id)
    .map((e) => {
      const otherId = e.source === id ? e.target : e.source
      const other = byId.get(otherId)
      return { id: otherId, name: other?.name ?? String(otherId), weight: e.weight }
    })
    .sort((x, y) => y.weight - x.weight)
})

/** 是否显示操作提示（有节点时才提示） */
const showHint = computed(() => nodes.value.length > 0)

// ─── 视觉映射 ────────────────────────────────────────────────────────────────

/** 节点最大命中文章数（用于把半径归一化到可读区间） */
const maxCount = computed(() => Math.max(1, ...props.nodes.map((n) => n.article_count)))
/** 边最大共现数 */
const maxWeight = computed(() => Math.max(1, ...props.edges.map((e) => e.weight)))

/**
 * 节点半径：按命中文章数开方缩放到 5–16px
 *
 * 用开方而非线性，是因为命中数分布长尾（实测 119 与 1 相差百倍），
 * 线性映射会让主体节点全挤在下限、只剩一个巨球。
 *
 * @param count - 命中文章数
 * @returns 半径（px）
 */
function nodeRadius(count: number): number {
  const r = Math.sqrt(count / maxCount.value)
  return 5 + 11 * r
}

/**
 * 边粗细：按共现数开方缩放到 0.7–3.2px（同样为避免长尾压扁）
 *
 * @param weight - 共现文章数
 * @returns 描边宽度（px）
 */
function edgeWidth(weight: number): number {
  return 0.7 + 2.5 * Math.sqrt(weight / maxWeight.value)
}


// ─── 布局仿真 ────────────────────────────────────────────────────────────────

/** rAF 句柄（0 = 未在跑） */
let rafId = 0
/**
 * 下一次收敛后是否把视图自动适配到内容
 *
 * 只在首次布局完成时需要。拖拽节点引发的重收敛**不**重新适配，
 * 否则用户刚摆好的位置会被一次自动缩放重置掉。
 */
let autoFit = true

/**
 * 播种：用黄金角螺旋给每个节点一个确定的初始位置
 *
 * @param source - 后端返回的节点
 * @returns 带坐标与速度的仿真节点
 */
function seed(source: GraphNode[]): SimNode[] {
  const radiusStep = 26
  return source.map((n, i) => {
    const r = radiusStep * Math.sqrt(i + 1)
    const a = i * GOLDEN_ANGLE
    return {
      id: n.id,
      name: n.name,
      entity_type: n.entity_type,
      article_count: n.article_count,
      typeClass: entityTypeClass(n.entity_type),
      x: Math.cos(a) * r,
      y: Math.sin(a) * r,
      vx: 0,
      vy: 0,
      pinned: false
    }
  })
}

/**
 * 推进一帧物理仿真
 *
 * 力模型（都是"直接位移"而非"加速度"，对小图更稳、不需要时间步长调参）：
 * 1. 斥力：任意两点间 `REPULSION / d²`，把节点推散；
 * 2. 弹簧：每条边按 `(d - REST_LENGTH) * SPRING` 收拢；
 * 3. 向心：所有节点朝原点轻微收拢，防止多个连通分量飘出视野。
 * 最后乘阻尼、限幅，得到本帧位移。被钉住的节点照常参与受力计算
 * （否则它周围的节点会失去支撑而塌陷），但不会移动自己。
 *
 * @returns 本帧所有可移动节点的位移总和（用于判断是否收敛）
 */
function tick(): number {
  const ns = nodes.value
  const ls = links.value

  for (const n of ns) {
    n.vx = 0
    n.vy = 0
  }

  // 1. 斥力：O(n²) 双循环。n ≤ 数十时每帧千余次乘法，无需空间索引
  for (let i = 0; i < ns.length; i++) {
    for (let j = i + 1; j < ns.length; j++) {
      const a = ns[i]
      const b = ns[j]
      let dx = a.x - b.x
      let dy = a.y - b.y
      let d2 = dx * dx + dy * dy
      // 完全重合时给一个确定性的微小偏移，否则方向向量的除法会炸成 NaN
      if (d2 < 0.01) {
        dx = 0.1 * (i + 1)
        dy = 0.1 * (j + 1)
        d2 = dx * dx + dy * dy
      }
      const d = Math.sqrt(d2)
      const f = REPULSION / d2
      const ux = dx / d
      const uy = dy / d
      a.vx += ux * f
      a.vy += uy * f
      b.vx -= ux * f
      b.vy -= uy * f
    }
  }

  // 2. 弹簧：边自然长度 REST_LENGTH
  for (const l of ls) {
    const a = ns[l.a]
    const b = ns[l.b]
    if (!a || !b) continue
    const dx = b.x - a.x
    const dy = b.y - a.y
    const d = Math.max(0.01, Math.hypot(dx, dy))
    const f = (d - REST_LENGTH) * SPRING
    const ux = dx / d
    const uy = dy / d
    a.vx += ux * f
    a.vy += uy * f
    b.vx -= ux * f
    b.vy -= uy * f
  }

  // 3. 向心 + 阻尼 + 限幅 + 位移
  let energy = 0
  for (const n of ns) {
    if (n.pinned) {
      n.vx = 0
      n.vy = 0
      continue
    }
    n.vx -= n.x * CENTER_PULL
    n.vy -= n.y * CENTER_PULL
    n.vx *= DAMPING
    n.vy *= DAMPING
    const len = Math.hypot(n.vx, n.vy)
    if (len > MAX_STEP) {
      n.vx = (n.vx / len) * MAX_STEP
      n.vy = (n.vy / len) * MAX_STEP
    }
    n.x += n.vx
    n.y += n.vy
    energy += Math.abs(n.vx) + Math.abs(n.vy)
  }
  // 微小的浮点残差会累积成持续抖动，低于阈值时直接吸附为静止
  if (energy < ENERGY_EPS) {
    for (const n of ns) {
      n.vx = 0
      n.vy = 0
    }
  }
  return energy
}

/**
 * 逐帧循环：收敛（或达到轮数上限）后停帧
 *
 * 停帧时只有「首次布局」会自动适配视图（见 `autoFit`）；此后由用户用
 * 工具条的「适应」按钮按需调整。
 */
function run(): void {
  let ticks = 0
  const step = () => {
    const energy = tick()
    ticks++
    if (energy < ENERGY_EPS || ticks >= MAX_TICKS) {
      rafId = 0
      if (autoFit) {
        fitView()
        autoFit = false
      }
      return
    }
    rafId = requestAnimationFrame(step)
  }
  rafId = requestAnimationFrame(step)
}

/** 停止当前仿真（重新播种或卸载时调用） */
function stop(): void {
  if (rafId) cancelAnimationFrame(rafId)
  rafId = 0
}


/** 依据 `nodes` / `edges` 重建仿真状态并启动布局（「重排」按钮也走这条） */
function rebuild(): void {
  stop()
  const seeded = seed(props.nodes)
  const indexById = new Map(seeded.map((n, i) => [n.id, i]))
  nodes.value = seeded
  links.value = props.edges
    .map((e) => ({ a: indexById.get(e.source) ?? -1, b: indexById.get(e.target) ?? -1, weight: e.weight }))
    // 端点缺失的边直接丢弃，否则 tick 里会读到 undefined 而抛错
    .filter((l) => l.a >= 0 && l.b >= 0)

  const adj = new Map<number, Set<number>>()
  for (const n of seeded) adj.set(n.id, new Set())
  for (const e of props.edges) {
    adj.get(e.source)?.add(e.target)
    adj.get(e.target)?.add(e.source)
  }
  adjacency.value = adj

  hoverId.value = null
  // 选中的实体可能已不在新图里（换了时间窗），此时自动取消选中
  if (selectedId.value !== null && !indexById.has(selectedId.value)) selectedId.value = null

  autoFit = true
  const reduceMotion =
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  if (reduceMotion) {
    // 同步迭代到收敛：不做逐帧动画，直接呈现最终形态
    for (let i = 0; i < MAX_TICKS; i++) {
      if (tick() < ENERGY_EPS) break
    }
    fitView()
    autoFit = false
  } else {
    run()
  }
}

// ─── 交互 ────────────────────────────────────────────────────────────────────

/**
 * 该边是否处于"高亮"状态（任一端点是关注节点）
 *
 * @param l - 仿真边
 * @returns 高亮时为 true
 */
function isEdgeActive(l: { a: number; b: number }): boolean {
  const id = focusId.value
  if (id === null) return false
  return nodes.value[l.a]?.id === id || nodes.value[l.b]?.id === id
}

/**
 * 指定节点是否为关注节点的邻居
 *
 * @param id - 节点 ID
 * @returns 是邻居时为 true
 */
function isNeighbor(id: number): boolean {
  const focus = focusId.value
  if (focus === null) return false
  if (focus === id) return true
  return adjacency.value.get(focus)?.has(id) ?? false
}

/**
 * 点击节点：切换选中态
 *
 * @param id - 节点 ID
 */
function toggleSelect(id: number): void {
  selectedId.value = selectedId.value === id ? null : id
}

/**
 * 按下节点：进入拖拽
 *
 * 拖过的节点会被**钉住**（`pinned = true`）：既使松手后不被物理力弹回原位，
 * 也让它作为锚点支撑邻接节点重新收敛。想恢复自动布局用工具条的「重排」。
 * 起点坐标不取按下瞬间的指针位置，而是等第一次 pointermove 再赋值。
 *
 * @param index - 仿真节点下标
 */
function onNodeDown(index: number): void {
  const start = nodes.value[index]
  if (!start) return
  stop()
  start.pinned = true
  const move = (ev: PointerEvent) => {
    const rect = wrapRef.value?.getBoundingClientRect()
    if (!rect) return
    // 屏幕坐标 → 图坐标：先减去平移，再除以缩放
    start.x = (ev.clientX - rect.left - view.value.tx) / view.value.k
    start.y = (ev.clientY - rect.top - view.value.ty) / view.value.k
    start.vx = 0
    start.vy = 0
  }
  const up = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    // 松手后让它继续收敛：被拖散的邻居自然回位，而钉住的那个留在原地。
    // 这里不重开 autoFit，所以视野不会被这次重收敛重置。
    run()
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
}




// ─── 生命周期 ────────────────────────────────────────────────────────────────

// 容器尺寸观察（画布随弹窗宽度自适应）由 useGraphView 内部维护；
// 这里只在挂载时启动首次布局、卸载时停掉仿真循环。
onMounted(() => {
  nextTick(rebuild)
})

onBeforeUnmount(() => {
  stop()
})

// 数据变化（切时间窗 / 改最小共现数 / 增删实体后刷新）都重建布局
watch(() => [props.nodes, props.edges], rebuild)
</script>

<style scoped>
.eg-wrap {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
}

.eg-svg {
  display: block;
  width: 100%;
  height: 100%;
  /* 抓手光标：暗示"空白处可拖拽平移" */
  cursor: grab;
  touch-action: none;
}

.eg-svg:active {
  cursor: grabbing;
}

/* 边：默认低对比，仅作为结构底噪；高亮时提到主色 */
.eg-edge {
  stroke: var(--border);
  transition: stroke var(--dur) var(--ease);
}

.eg-edge.is-active {
  stroke: var(--primary);
}

.eg-node {
  cursor: pointer;
}

/* 实体类型配色：只在这里把类型映射成一个自定义属性 `--eg-c`，
   SVG 圆用 fill 取值、HTML 图例点用 background 取值——两处共用同一份映射，
   新增类型时不会出现"图上改了、图例没改"。色值全部来自既有语义色 token。 */
.eg-type-company {
  --eg-c: var(--tone-sky);
}
.eg-type-person {
  --eg-c: var(--tone-violet);
}
.eg-type-product {
  --eg-c: var(--tone-green);
}
.eg-type-industry {
  --eg-c: var(--tone-amber);
}
.eg-type-other {
  --eg-c: var(--tone-slate);
}

.eg-dot {
  fill: var(--eg-c, var(--tone-slate));
  transition: opacity var(--dur) var(--ease);
}

/* 选中态：描一圈底色环，在深色节点上也能看清"就是这一个" */
.eg-dot.is-focus {
  stroke: var(--text-primary);
  stroke-width: 2;
}

/* 被手动拖过并钉住的节点：虚线环，提示"它不会自己回去了" */
.eg-dot.is-pinned:not(.is-focus) {
  stroke: var(--text-tertiary);
  stroke-width: 1.5;
  stroke-dasharray: 3 2;
}

.eg-label {
  font-size: 11px;
  fill: var(--text-secondary);
  text-anchor: middle;
  paint-order: stroke;
  /* 描边用底色：节点密集时标签压住边线也读得清 */
  stroke: var(--surface);
  stroke-width: 3px;
  pointer-events: none;
  user-select: none;
}

.eg-label.is-focus {
  fill: var(--text-primary);
  font-weight: 600;
}

/* 聚焦某节点时，非邻接元素淡出（不是隐藏，保留整体结构感） */
.is-dim {
  opacity: 0.16;
}

.eg-tools {
  position: absolute;
  top: var(--sp-15);
  right: var(--sp-15);
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}

.eg-legend {
  position: absolute;
  left: var(--sp-15);
  bottom: var(--sp-15);
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-1);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.eg-legend-item {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-025);
}

.eg-legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  /* HTML 元素不吃 fill，必须用 background 取同一个 --eg-c */
  background: var(--eg-c, var(--tone-slate));
}


.eg-hint {
  position: absolute;
  right: var(--sp-15);
  bottom: var(--sp-15);
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
</style>
