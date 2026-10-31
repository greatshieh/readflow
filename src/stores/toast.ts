/**
 * 全局轻提示（Toast）状态
 *
 * # 职责
 * 承载"操作已完成、且可能还有一次反悔机会"这类短暂反馈；一条提示最多挂一个动作按钮
 * （撤销 / 查看 / 重试）。渲染由 `components/ToastHost.vue` 负责，本文件只管状态与计时。
 *
 * # 设计意图
 * - **与 modalStore 分工**：Modal 打断用户并要求回答（确认/输入），Toast 不打断、事后补救。
 *   可撤销的操作（取消订阅）走 Toast —— 用撤销兜底，就不必给每一次操作都插一个确认框。
 * - **队列上限**：同时最多 3 条，超出时最旧的一条立即出队（其动作随之失效），
 *   避免连点几次后屏幕上堆满提示、也避免"待撤销"状态无限堆积。
 * - **计时器不入响应式**：`setTimeout` 句柄放模块级 Map，它只是实现细节，
 *   放进 `ref` 只会让 Vue 去代理一个不需要追踪的对象。
 *
 * # 边界
 * - 动作按钮点击后提示立即关闭，回调在关闭后执行（回调抛错不影响提示消失）。
 * - 不提供 hover 暂停计时：撤销窗口只有数秒，暂停逻辑带来的复杂度大于收益。
 */

import { acceptHMRUpdate, defineStore } from 'pinia'
import { ref } from 'vue'

/** 提示上的动作按钮 */
interface ToastAction {
  /** 按钮文案，如"撤销" */
  label: string
  /** 点击后执行的回调；执行完提示即关闭 */
  onClick: () => void
}

/** 一条提示的展示数据 */
interface ToastItem {
  /** 自增 ID，兼作列表 key */
  id: number
  /** 正文文案 */
  text: string
  /** 可选动作按钮 */
  action?: ToastAction
  /** 停留时长（毫秒） */
  duration: number
}

/** `show()` 的可选参数 */
interface ToastOptions {
  /** 停留时长（毫秒），默认 4000 */
  duration?: number
  /** 挂载的动作按钮 */
  action?: ToastAction
}

/** 同屏最多保留的提示条数，超出时最旧的立即出队 */
const MAX_VISIBLE = 3

/** 默认停留时长（毫秒）：纯提示读得完又不碍事 */
const DEFAULT_DURATION = 4000

/* 下面两项必须放在模块作用域（而非 store 的 setup 内）：
   setup 里的顶层变量会随 store 实例存在，但计时器句柄需要被"出队"等操作跨调用访问，
   放模块级更直白，也避免 HMR 重载时旧定时器与新状态错位。 */
let idSeed = 0
const timers = new Map<number, ReturnType<typeof setTimeout>>()

export const useToastStore = defineStore('toast', () => {
  /** 当前屏幕上的提示，按加入顺序排列（渲染顺序即此顺序） */
  const items = ref<ToastItem[]>([])

  /**
   * 关闭一条提示并清掉它的定时器
   *
   * @param id - 提示 ID；不存在时静默返回（重复关闭是合法调用）
   */
  function dismiss(id: number): void {
    const timer = timers.get(id)
    if (timer !== undefined) {
      clearTimeout(timer)
      timers.delete(id)
    }
    const at = items.value.findIndex((t) => t.id === id)
    if (at !== -1) items.value.splice(at, 1)
  }

  /**
   * 弹出一条提示
   *
   * @param text - 正文文案，应是一句完整的话（"已取消订阅「少数派」"）
   * @param options - 时长与动作按钮
   * @returns 该条提示的 ID，调用方通常无需使用
   */
  function show(text: string, options: ToastOptions = {}): number {
    // 队列已满：先挤掉最旧的一条，保证屏幕与"可达动作"都只有一个定量
    while (items.value.length >= MAX_VISIBLE) {
      const oldest = items.value[0]
      if (!oldest) break
      dismiss(oldest.id)
    }

    const id = ++idSeed
    const duration = options.duration ?? DEFAULT_DURATION
    items.value.push({ id, text, action: options.action, duration })
    timers.set(
      id,
      setTimeout(() => dismiss(id), duration)
    )
    return id
  }

  /**
   * 触发某条提示的动作按钮
   *
   * 先关闭提示再执行回调：回调里常会弹新的提示（如"已恢复订阅"），
   * 若先执行回调，新提示可能被"关闭旧提示"的动作波及。
   *
   * @param id - 提示 ID
   */
  function runAction(id: number): void {
    const item = items.value.find((t) => t.id === id)
    if (!item?.action) return
    const action = item.action
    dismiss(id)
    action.onClick()
  }

  return { items, show, dismiss, runAction }
})

// 开发期热更新：用新定义重建 Pinia 实例。
// 少了这段，Vite 热更新只重跑本模块，而 pinia 缓存里仍是旧实例的 action 集合，
// 于是新组件调用本轮新增的方法会拿到 undefined，TypeError 被事件处理器吞掉，
// 界面表现是「点了没反应」；生产构建不会进入本分支。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useToastStore, import.meta.hot))
}
