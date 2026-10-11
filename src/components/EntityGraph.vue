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
          @pointerdown.stop.prevent="onNodeDown(i, $event)"
          @pointerenter="hoverId = n.id"
          @pointerleave="hoverId = null"
        >
          <circle
          class="eg-dot"
          :class="[nodeColorClass(n), { 'is-focus': n.id === focusId, 'is-pinned': n.pinned }]"
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

    <!-- 工具条：导出 / 高亮维度 / 缩放（浮在画布右上角） -->
    <div class="eg-tools">
      <button class="btn btn-sm" @click="exportPng" title="导出为 PNG 图片">导出</button>
      <div class="eg-mode">
        <button class="eg-mode-btn" :class="{ active: highlightMode === 'type' }" @click="highlightMode = 'type'">类型</button>
        <button class="eg-mode-btn" :class="{ active: highlightMode === 'sentiment' }" @click="highlightMode = 'sentiment'">情感</button>
      </div>
      <button class="btn btn-sm" @click="zoomBy(1.25)" title="放大">+</button>
      <button class="btn btn-sm" @click="zoomBy(0.8)" title="缩小">−</button>
      <button class="btn btn-sm" @click="fitView" title="适应视图">适应</button>
      <button class="btn btn-sm" @click="rebuild" title="重新布局（清除手动拖动）">重排</button>
    </div>

    <!-- 图例：随高亮维度切换（类型 / 情感） -->
    <div class="eg-legend">
      <template v-if="highlightMode === 'type'">
        <span v-for="t in ENTITY_LEGEND" :key="t.key" class="eg-legend-item">
          <i class="eg-legend-dot" :class="`eg-type-${t.key}`"></i>{{ t.label }}
        </span>
      </template>
      <template v-else>
        <span v-for="s in SENTIMENT_LEGEND" :key="s.key" class="eg-legend-item">
          <i class="eg-legend-dot" :class="`eg-sent-${s.key}`"></i>{{ s.label }}
        </span>
      </template>
    </div>

    <p v-if="showHint" class="eg-hint">点击节点打开仪表盘 · 拖拽节点固定位置 · 拖拽空白平移 · 滚轮缩放</p>
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
 * 悬停高亮邻接、点击节点直接打开完整仪表盘弹窗。
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
 * 由主屏全屏视图 `GraphView.vue` 渲染。悬停高亮的选中态在组件内部自洽；
 * 点开仪表盘走 window 事件 `open-entity-dashboard`（由 App.vue 打开弹窗），
 * 因此不需要任何 emit。
 */

import { ref, computed, watch, onBeforeUnmount, onMounted, nextTick } from 'vue'
import type { GraphNode, GraphEdge } from '@/types'
import { useGraphView } from '@/composables/useGraphView'
import { ENTITY_LEGEND, entityTypeClass } from '@/utils/entityLabels'

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

/** 悬停的节点 ID（用于高亮邻接元素；点击节点直接开仪表盘弹窗，不再有"选中"态） */
const hoverId = ref<number | null>(null)

/** 是否处于「聚焦某个节点」的状态：此时非邻接元素淡出 */
const focusId = computed(() => hoverId.value)
const dimOthers = computed(() => focusId.value !== null)

/** 仿真节点：在传入节点上附加坐标与速度（组件私有副本） */
interface SimNode {
  id: number
  name: string
  entity_type: string
  article_count: number
  /** 已归一化的 CSS 类后缀（未知类型回退 other，避免出现无样式的节点） */
  typeClass: string
  /** 聚合情感（positive / negative / neutral），情感高亮模式使用 */
  sentiment: string
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

/** 是否显示操作提示（有节点时才提示） */
const showHint = computed(() => nodes.value.length > 0)

// ─── 高亮维度（节点着色模式）────────────────────────────────────────────────────

/**
 * 节点配色维度：'type' 按实体类型（默认），'sentiment' 按聚合情感。
 * 切换后节点圆与图例同步改变，边与底图结构不变。
 */
const highlightMode = ref<'type' | 'sentiment'>('type')

/** 情感图例（与类型图例互斥，随高亮维度切换显示） */
const SENTIMENT_LEGEND = [
  { key: 'positive', label: '正面' },
  { key: 'negative', label: '负面' },
  { key: 'neutral', label: '中性' },
] as const

/**
 * 节点圆的配色类：按当前高亮维度返回 `eg-type-*` 或 `eg-sent-*`
 *
 * @param n - 仿真节点
 * @returns CSS 类名（供 `<circle>` 的 `:class` 使用）
 */
function nodeColorClass(n: SimNode): string {
  if (highlightMode.value === 'sentiment') {
    return `eg-sent-${n.sentiment || 'neutral'}`
  }
  return `eg-type-${n.typeClass}`
}

// ─── 打开实体仪表盘 ────────────────────────────────────────────────────────

/**
 * 从关系图节点打开「实体仪表盘」弹窗
 *
 * 由节点点击（pointerup 判定为点击、非拖拽）直接调用。走 window 事件
 * `open-entity-dashboard`（与 open-settings 同构），由 App.vue 监听后调
 * uiStore.openEntityDashboard 弹出完整仪表盘弹窗——不再经过中间的节点详情卡。
 *
 * @param entityId - 实体 ID
 */
function openEntityDashboard(entityId: number): void {
  window.dispatchEvent(
    new CustomEvent('open-entity-dashboard', { detail: { entityId } })
  )
}

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
      sentiment: n.sentiment ?? 'neutral',
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
 * 按下节点：进入「点击 / 拖拽」判定
 *
 * - 指针位移 ≤ 4px → 视为**点击**：松手时打开该实体的完整仪表盘弹窗；
 * - 位移超过阈值 → 视为**拖拽**：节点被**钉住**（`pinned = true`），松手后不被
 *   物理力弹回原位，并作为锚点支撑邻接节点重新收敛（想恢复自动布局用「重排」）。
 *
 * 钉住只发生在"真的拖动过"时——单纯点开弹窗不再把节点钉死。
 * 起点坐标不取按下瞬间的指针位置，而是等第一次 pointermove 再赋值。
 *
 * @param index - 仿真节点下标
 * @param ev - 触发按下事件的指针事件（记录起始坐标，用于区分点击与拖拽）
 */
function onNodeDown(index: number, ev: PointerEvent): void {
  const start = nodes.value[index]
  if (!start) return
  stop()
  const downX = ev.clientX
  const downY = ev.clientY
  /** 是否已判定为拖拽（位移超过阈值） */
  let dragging = false
  const move = (pev: PointerEvent) => {
    // 首次超过阈值即锁定为拖拽，并此时才钉住节点
    if (!dragging && Math.hypot(pev.clientX - downX, pev.clientY - downY) > 4) {
      dragging = true
      start.pinned = true
    }
    if (!dragging) return
    const rect = wrapRef.value?.getBoundingClientRect()
    if (!rect) return
    // 屏幕坐标 → 图坐标：先减去平移，再除以缩放
    start.x = (pev.clientX - rect.left - view.value.tx) / view.value.k
    start.y = (pev.clientY - rect.top - view.value.ty) / view.value.k
    start.vx = 0
    start.vy = 0
  }
  const up = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    // 松手后让仿真继续收敛（pointerdown 时已 stop）：拖拽者被拖散的邻居自然回位、
    // 钉住的留在原地；点击者则恢复此前被暂停的收敛。这里不重开 autoFit，
    // 所以视野不会被这次重收敛重置。
    run()
    // 未移动 = 点击：直接打开完整仪表盘弹窗
    if (!dragging) openEntityDashboard(start.id)
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

// ─── 导出 PNG ────────────────────────────────────────────────────────────────

/**
 * 把当前关系图导出为 PNG 并触发下载
 *
 * SVG 用的颜色来自 CSS 变量（`var(--eg-c)` 等），而 `canvas` 绘制时不解析 CSS 变量，
 * 直接序列化会因 `fill: var(...)` 失效而得到透明/黑块。故导出前遍历所有元素，
 * 用 `getComputedStyle` 取**已解析**的实际色值内联为样式，规避该坑。
 *
 * 流程：克隆 svg → 内联计算样式 → 注入 `--surface` 背景 → 序列化 → Image 载入 canvas
 * （按 `devicePixelRatio` 放大保高清）→ `toDataURL('image/png')` → `<a download>` 触发下载。
 */
function exportPng(): void {
  const svgEl = wrapRef.value?.querySelector('svg.eg-svg') as SVGSVGElement | null
  if (!svgEl) return
  const clone = svgEl.cloneNode(true) as SVGSVGElement
  const origEls = svgEl.querySelectorAll('*')
  const cloneEls = clone.querySelectorAll('*')
  // 内联计算样式：clone 未挂载，必须对照原元素取 computed 值
  const INLINE_PROPS = ['fill', 'stroke', 'stroke-width', 'opacity', 'font-size', 'font-weight', 'text-anchor']
  origEls.forEach((orig, i) => {
    const cs = getComputedStyle(orig)
    const c = cloneEls[i] as SVGElement
    for (const p of INLINE_PROPS) {
      const v = cs.getPropertyValue(p)
      if (v) c.style.setProperty(p, v)
    }
  })
  // 背景：取容器底色，避免透明
  const surface = getComputedStyle(wrapRef.value!).backgroundColor || '#ffffff'
  const bg = document.createElementNS('http://www.w3.org/2000/svg', 'rect')
  bg.setAttribute('x', '0')
  bg.setAttribute('y', '0')
  bg.setAttribute('width', '100%')
  bg.setAttribute('height', '100%')
  bg.setAttribute('fill', surface)
  clone.insertBefore(bg, clone.firstChild)
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
  const serializer = new XMLSerializer()
  const svgStr = '<?xml version="1.0" encoding="UTF-8" standalone="no"?>\n' + serializer.serializeToString(clone)

  const blob = new Blob([svgStr], { type: 'image/svg+xml;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const img = new Image()
  img.onload = () => {
    const scale = window.devicePixelRatio || 1
    const w = size.value.w
    const h = size.value.h
    const canvas = document.createElement('canvas')
    canvas.width = Math.round(w * scale)
    canvas.height = Math.round(h * scale)
    const ctx = canvas.getContext('2d')
    if (ctx) {
      ctx.scale(scale, scale)
      ctx.fillStyle = surface
      ctx.fillRect(0, 0, w, h)
      ctx.drawImage(img, 0, 0, w, h)
      const pngUrl = canvas.toDataURL('image/png')
      const a = document.createElement('a')
      a.href = pngUrl
      a.download = 'entity-graph.png'
      a.click()
    }
    URL.revokeObjectURL(url)
  }
  img.onerror = () => URL.revokeObjectURL(url)
  img.src = url
}
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

/* 情感配色（「情感」高亮维度使用）：复用既有语义色 token，与列表 / 时间线情感色一致 */
.eg-sent-positive {
  --eg-c: var(--entity-product);
}
.eg-sent-negative {
  --eg-c: var(--entity-regulatory);
}
.eg-sent-neutral {
  --eg-c: var(--entity-other);
}

/* 高亮维度切换控件（类型 / 情感） */
.eg-mode {
  display: flex;
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  overflow: hidden;
}
.eg-mode-btn {
  flex: 1;
  padding: 2px 8px;
  font-size: var(--fs-xs);
  border: none;
  background: var(--fill);
  color: var(--text-secondary);
  cursor: pointer;
  transition: background var(--dur) var(--ease), color var(--dur) var(--ease);
}
.eg-mode-btn.active {
  background: var(--primary);
  color: #fff;
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
