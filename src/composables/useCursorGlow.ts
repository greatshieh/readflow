import { onBeforeUnmount, onMounted, type Ref } from 'vue'

/**
 * 面板光标跟随效果
 *
 * # 职责
 * 让一块玻璃面板"感应"指针：底层泛起一团跟随指针的柔光（视频里的「整个后台都亮了」），
 * 同时靠近指针的那一侧边框亮起一段（「靠近的那段边框跟着亮」）。
 *
 * # 实现要点
 * 两层效果都**不含 JS 动画**，JS 只负责写三个 CSS 变量：
 * - `--mx` / `--my`：指针相对面板左上角的百分比，供面板 `::before` 的
 *   `radial-gradient(... at var(--mx) var(--my))` 定位光斑圆心；
 * - `--ang`：指针相对面板**中心**的极角（度），供 `.pane-frame` 的
 *   `conic-gradient(from calc(var(--ang) - 70deg), ...)` 定位亮边起点。
 *
 * 为什么角度要用 `atan2(dy, dx)` 而非 x/y 偏移：conic-gradient 的起点本来就要角度，
 * 直接给角度省掉一次三角函数。dx/dy 各取一半宽/高，使正右方为 0°。
 *
 * # 性能
 * `mousemove` 的触发频率可高于屏幕刷新率，逐事件写样式等于做了无用功。
 * 这里用一个 rAF 句柄把同一帧内的多个事件合并，只写一次样式；
 * 指针不动时不会有后续 rAF，天然停止。
 *
 * # 约束
 * 宿主面板须设 `position: relative`（光斑层是 absolute），并 `overflow: hidden`
 * —— 光斑半径故意大于面板，靠裁剪得到"光溢出边界"的效果，而非"面板内有个圆点"。
 *
 * @param paneRef 面板根元素的 ref（需带 `.pane-glow-host` 类）
 * @param frameRef 跟随边框元素的 ref（`.pane-frame`）。省略时只点亮光斑、不做跟随边框。
 * @returns 无返回值；副作用为写 `--mx` / `--my` / `--ang`，并在两个元素上切 `.is-lit`
 */
export function useCursorGlow(
  paneRef: Ref<HTMLElement | null>,
  frameRef?: Ref<HTMLElement | null>,
): void {
  /** 在途的 rAF 句柄；null 表示当前没有待写样式的帧 */
  let raf: number | null = null
  /** 最近一次事件的指针坐标（相对面板左上角，px） */
  let px = 0
  let py = 0

  /**
   * 把累积的指针位置写入 CSS 变量
   *
   * 只读一次 `getBoundingClientRect`：该方法会触发强制布局，在 mousemove 里逐次调用
   * 容易掉帧。rAF 已保证每帧最多一次，此处再取一次 rect 是可接受的。
   */
  function apply(): void {
    raf = null
    const pane = paneRef.value
    if (!pane) return
    const rect = pane.getBoundingClientRect()
    if (rect.width === 0 || rect.height === 0) return

    pane.style.setProperty('--mx', `${((px / rect.width) * 100).toFixed(1)}%`)
    pane.style.setProperty('--my', `${((py / rect.height) * 100).toFixed(1)}%`)

    // 相对面板中心的极角；+90° 让 0° 指向正上方，顺时针为正，与 conic-gradient 一致
    const dx = px - rect.width / 2
    const dy = py - rect.height / 2
    const deg = (Math.atan2(dy, dx) * 180) / Math.PI + 90
    pane.style.setProperty('--ang', `${deg.toFixed(1)}deg`)
  }

  /** 指针进入面板：点亮光斑与边框 */
  function onEnter(): void {
    paneRef.value?.classList.add('is-lit')
    frameRef?.value?.classList.add('is-lit')
  }

  /**
   * 指针离开面板：熄灭两层效果
   *
   * 不复位 `--mx/--my`——下次进入时第一帧就写入新坐标，残留值不会被看见。
   */
  function onLeave(): void {
    paneRef.value?.classList.remove('is-lit')
    frameRef?.value?.classList.remove('is-lit')
  }

  /**
   * 记录指针位置并合并到下一帧
   *
   * @param e - 鼠标事件
   */
  function onMove(e: MouseEvent): void {
    const pane = paneRef.value
    if (!pane) return
    const rect = pane.getBoundingClientRect()
    px = e.clientX - rect.left
    py = e.clientY - rect.top
    if (raf === null) raf = requestAnimationFrame(apply)
  }

  onMounted(() => {
    const pane = paneRef.value
    if (!pane) return
    pane.addEventListener('mouseenter', onEnter)
    pane.addEventListener('mouseleave', onLeave)
    pane.addEventListener('mousemove', onMove)
  })

  onBeforeUnmount(() => {
    // 必须在卸载时取消在途 rAF：否则它会在组件已销毁后写样式
    if (raf !== null) cancelAnimationFrame(raf)
    raf = null
    const pane = paneRef.value
    if (!pane) return
    pane.removeEventListener('mouseenter', onEnter)
    pane.removeEventListener('mouseleave', onLeave)
    pane.removeEventListener('mousemove', onMove)
  })
}