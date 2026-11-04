/**
 * 滚动位置恢复（带回下一帧重试）
 *
 * # 职责
 * 把某个滚动容器恢复到离开时的 `scrollTop`，并且**只在容器真的有布局盒时**才写。
 *
 * # 为什么必须等布局盒，而不是拿到元素就写
 * `v-show` 常驻的页签在 `display: none` 时：`scrollTop = N` 会被**钳成 0 且不可逆**
 * （写入被丢弃，而且不会因为之后显示出来自动补上），同时 `getBoundingClientRect()`
 * 全 0 会让"元素是否可见"这类判定恒为真。于是"恢复"看起来执行了、实际什么都没做，
 * 快照还被消费掉——永久静默失效，比"回不去"更难查。
 * 判据用 `getClientRects().length > 0`，不用 `offsetParent`：后者对 `position: fixed`
 * 元素是 null，判据不干净。
 *
 * # 为什么阈值只有 3 帧
 * `getClientRects()` 会强制同步布局：只要祖先不是 `display: none`，调用当场就有盒。
 * 于是"内容大、渲染慢"表现为这一帧算得久，而不是"连续多帧都拿不到盒"。真正会返回
 * 空列表的只有两类情形：元素在 `display: none` 的子树里、元素还没进文档。两类都会在
 * 2 帧内必然分出结果。调大阈值只会延长快照滞留窗口，并不会让恢复更可靠
 * （同 `PeriodReportReader` 的 `RESTORE_MAX_FRAMES`，那边是同一套推理）。
 */

import { nextTick } from 'vue'

/** 最多尝试几帧（含首帧） */
const MAX_FRAMES = 3

/**
 * 把滚动量恢复到目标容器
 *
 * 时序固定为「`nextTick`（等本次渲染提交）→ `rAF`（等样式与布局算完）→ 判有盒 → 写」：
 * 在渲染提交之前写，容器即使有盒也还是旧内容的高度；在布局算完之前写，位置会被随后的
 * 内容撑高再钳一次。
 *
 * @param getEl - 取容器；由调用方提供（各页签的滚动容器不同，容器可能被 `v-if` 换掉）
 * @param top - 目标滚动量（像素）
 * @returns 真正写下去时为 true；容器始终无布局盒时为 false（调用方据此决定要不要重试整条恢复）
 */
export async function restoreScrollTop(
  getEl: () => HTMLElement | null,
  top: number,
): Promise<boolean> {
  await nextTick()
  return new Promise<boolean>(resolve => {
    let frames = 0
    /**
     * 试一次；没有布局盒就排到下一帧再试（有界）
     *
     * @returns 无返回值；成功或放弃时都会 resolve
     */
    const attempt = (): void => {
      frames += 1
      const el = getEl()
      if (el && el.getClientRects().length > 0) {
        el.scrollTop = top
        resolve(true)
        return
      }
      if (frames >= MAX_FRAMES) {
        // 放弃前留痕：用户的阅读位置在这里被吞掉，线上出问题总得有线索可查
        console.warn('[origin] 位置恢复放弃：滚动容器始终无布局盒', { top })
        resolve(false)
        return
      }
      requestAnimationFrame(attempt)
    }
    requestAnimationFrame(attempt)
  })
}
