/**
 * 关系图视图变换 composable
 *
 * # 职责
 * 承担 EntityGraph 中与「画布几何」相关的全部状态与交互：
 * - 容器尺寸跟随（ResizeObserver）；
 * - 平移 / 缩放（滚轮以画布中心为锚点、空白处拖拽平移、按钮按比例缩放）；
 * - 「适应视图」：从当前节点坐标计算包围盒，把视图调成刚好装下整张图。
 *
 * 与它解耦的是**物理仿真**（seed / tick / run，仍在 EntityGraph 内）——
 * 仿真只管节点坐标，本模块只管"这些坐标如何被映射到屏幕"，二者通过
 * `positions`（只读坐标来源）与 `radiusOf`（与渲染半径同源的包围盒留白）单向衔接。
 *
 * @param options.positions - 仿真节点数组（只读，取 x / y / article_count）
 * @param options.radiusOf - 节点半径映射；包围盒按"圆 + 下方标签"留白，必须与渲染半径一致
 * @returns 视图状态与交互处理器；`wrapRef` 须绑到画布容器元素上
 */

import { ref, onMounted, onBeforeUnmount, type Ref } from 'vue'

/** 视图变换所需的最小节点形状（结构化匹配 SimNode，避免引入组件私有类型） */
export interface GraphViewNode {
  x: number
  y: number
  article_count: number
}

/** 视口内边距（适应视图时留出标签空间） */
const FIT_PADDING = 46

export function useGraphView(options: {
  positions: Ref<ReadonlyArray<GraphViewNode>>
  radiusOf: (count: number) => number
}) {
  const { positions, radiusOf } = options

  /** 画布容器引用（测量尺寸与拖拽坐标换算的锚点） */
  const wrapRef = ref<HTMLElement | null>(null)
  /** 画布尺寸（跟随容器，由 ResizeObserver 维护） */
  const size = ref({ w: 640, h: 420 })
  /** 平移 + 缩放（作用在 `<g>` 的 transform 上，不逐节点改坐标） */
  const view = ref({ tx: 320, ty: 210, k: 1 })

  /** 容器尺寸观察器（画布随弹窗宽度自适应） */
  let observer: ResizeObserver | null = null

  /**
   * 从当前节点坐标计算包围盒，把视图平移缩放成"刚好装下整张图"
   *
   * 收敛后由仿真自动调用一次（首次布局），也由「适应」按钮手动触发；
   * 不依赖初始猜测的缩放系数，节点规模变化（34 → 100）时也不会跑出视野。
   */
  function fitView(): void {
    const ns = positions.value
    if (ns.length === 0) {
      view.value = { tx: size.value.w / 2, ty: size.value.h / 2, k: 1 }
      return
    }
    let minX = Infinity
    let maxX = -Infinity
    let minY = Infinity
    let maxY = -Infinity
    for (const n of ns) {
      // 标签挂在节点下方，包围盒下沿要多留一行字的高度，否则底部标签会被裁掉
      const pad = radiusOf(n.article_count)
      minX = Math.min(minX, n.x - pad)
      maxX = Math.max(maxX, n.x + pad)
      minY = Math.min(minY, n.y - pad)
      maxY = Math.max(maxY, n.y + pad + 14)
    }
    const bw = Math.max(1, maxX - minX)
    const bh = Math.max(1, maxY - minY)
    const k = Math.min(
      (size.value.w - FIT_PADDING * 2) / bw,
      (size.value.h - FIT_PADDING * 2) / bh,
      // 上限 1.4：节点很少时不要让三两个圆点被放大到占满整屏
      1.4
    )
    const cx = (minX + maxX) / 2
    const cy = (minY + maxY) / 2
    view.value = {
      k,
      tx: size.value.w / 2 - cx * k,
      ty: size.value.h / 2 - cy * k
    }
  }

  /**
   * 按比例缩放，并让画布中心保持在原位
   *
   * @param factor - 缩放倍数
   */
  function zoomBy(factor: number): void {
    const v = view.value
    const k = Math.min(3, Math.max(0.25, v.k * factor))
    const rect = wrapRef.value?.getBoundingClientRect()
    const cx = (rect?.width ?? size.value.w) / 2
    const cy = (rect?.height ?? size.value.h) / 2
    // 保持中心点在图坐标下的位置不变：新平移 = 中心 - 图坐标 * 新缩放
    const gx = (cx - v.tx) / v.k
    const gy = (cy - v.ty) / v.k
    view.value = { k, tx: cx - gx * k, ty: cy - gy * k }
  }

  /**
   * 滚轮缩放（以画布中心为锚点）
   *
   * @param e - 滚轮事件
   */
  function onWheel(e: WheelEvent): void {
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1
    zoomBy(factor)
  }

  /**
   * 按下空白：平移画布
   *
   * @param e - 指针事件
   */
  function onCanvasDown(e: PointerEvent): void {
    const startX = e.clientX
    const startY = e.clientY
    const { tx, ty } = view.value
    const move = (ev: PointerEvent) => {
      view.value = { ...view.value, tx: tx + (ev.clientX - startX), ty: ty + (ev.clientY - startY) }
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  onMounted(() => {
    const el = wrapRef.value
    if (el) {
      size.value = { w: el.clientWidth || size.value.w, h: el.clientHeight || size.value.h }
      observer = new ResizeObserver(() => {
        if (!wrapRef.value) return
        size.value = {
          w: wrapRef.value.clientWidth || size.value.w,
          h: wrapRef.value.clientHeight || size.value.h
        }
      })
      observer.observe(el)
    }
  })

  onBeforeUnmount(() => {
    observer?.disconnect()
    observer = null
  })

  return { wrapRef, size, view, fitView, zoomBy, onWheel, onCanvasDown }
}
