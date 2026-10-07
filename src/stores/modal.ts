/**
 * 全局弹窗状态 store
 *
 * # 职责
 * 统一管理应用内所有"确认/提示/输入"类弹窗的状态，替代原生的
 * `window.confirm` / `window.prompt` / `window.alert`，确保视觉风格与应用一致。
 *
 * # 设计意图
 * - 所有需要二次确认或提示用户的操作（删除订阅源、删除规则、保存失败等）
 *   都通过此 store 触发，避免散落在各组件中的原生弹窗；
 * - 使用 `ref` 承载当前激活的弹窗信息，`showConfirm`/`showPrompt`/`showAlert`
 *   为异步封装，返回 `Promise<boolean | string>`，调用方可 await 得到用户选择；
 * - 支持多个弹窗堆叠（confirm/prompt/alert 互斥，同一时间只展示一种）。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'

/** 弹窗类型枚举 */
export type ModalType = 'confirm' | 'prompt' | 'alert'

/** 确认弹窗的参数结构 */
export interface ConfirmOptions {
  /** 弹窗标题（显示在弹窗头部） */
  title: string
  /** 弹窗正文内容（支持换行） */
  message: string
}

/** 输入弹窗的参数结构 */
export interface PromptOptions {
  /** 弹窗标题 */
  title: string
  /** 弹窗正文 / 占位提示 */
  message: string
  /** 默认填入值（可为空字符串） */
  defaultValue?: string
}

/** 提示弹窗的参数结构 */
export interface AlertOptions {
  /** 弹窗标题 */
  title: string
  /** 弹窗正文内容 */
  message: string
}

/**
 * 弹窗共享状态：当前激活的弹窗信息
 *
 * type 字段区分三种形态，inputKey 仅 prompt 类型使用（强制 input 重新挂载以重置焦点）
 */
const activeModal = ref<{
  type: ModalType
  title: string
  message: string
  /** 仅 prompt 类型使用：默认值 */
  defaultValue?: string
  /** 仅 prompt 类型使用：输入框焦点标识 */
  inputKey?: number
} | null>(null)

/**
 * 当前正在等待的 Promise 解析器
 *
 * 使用 any 类型以兼容不同 modal 类型的返回值（boolean / string | null / void）
 */
let pendingResolve: ((value: any) => void) | null = null

/**
 * 展示确认弹窗（替代 window.confirm）
 *
 * 用户点击"确定"返回 `true`，点击"取消"或关闭遮罩返回 `false`。
 *
 * @param options - 弹窗配置（title / message）
 * @returns Promise<boolean>：true = 用户确认，false = 用户取消
 */
export async function showConfirm(options: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    activeModal.value = {
      type: 'confirm',
      title: options.title,
      message: options.message,
    }
    pendingResolve = resolve
  })
}

/**
 * 展示输入弹窗（替代 window.prompt）
 *
 * 用户输入文本后点击"确定"返回输入字符串；点击"取消"或关闭遮罩返回 `null`。
 *
 * @param options - 弹窗配置（title / message / defaultValue）
 * @returns Promise<string | null>：用户输入的内容或 null
 */
export async function showPrompt(options: PromptOptions): Promise<string | null> {
  // 用 inputKey 强制 input 重新挂载以重置焦点（内部状态，非 options 字段）
  const key = ((activeModal.value as any)?.inputKey ?? 0) + 1
  return new Promise((resolve) => {
    activeModal.value = {
      type: 'prompt',
      title: options.title,
      message: options.message,
      defaultValue: options.defaultValue,
      inputKey: key,
    }
    pendingResolve = resolve
  })
}

/**
 * 展示提示弹窗（替代 window.alert）
 *
 * 用户点击"确定"或关闭遮罩后 Promise resolve。
 *
 * @param options - 弹窗配置（title / message）
 * @returns Promise<void>
 */
export async function showAlert(options: AlertOptions): Promise<void> {
  return new Promise((resolve) => {
    activeModal.value = {
      type: 'alert',
      title: options.title,
      message: options.message,
    }
    pendingResolve = resolve
  })
}

/**
 * 关闭当前弹窗并 resolve 等待中的 Promise
 *
 * 由 GlobalModal 组件内的按钮触发；传入任何值均可（类型由调用方决定）
 *
 * @param result - 要 resolve 的值
 * @returns 无返回值
 */
function closeWithResult(result: unknown): void {
  if (pendingResolve) {
    pendingResolve(result)
    pendingResolve = null
  }
  activeModal.value = null
}

export const useModalStore = defineStore('modal', () => {
  return {
    activeModal,
    showConfirm,
    showPrompt,
    showAlert,
    closeWithResult,
  }
})
