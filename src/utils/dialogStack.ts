/**
 * 弹窗栈与滚动锁
 *
 * # 职责
 * 为所有基于 `BaseModal` 的模态弹窗提供两份**全局**状态：
 * 1. [`anyDialogOpen`]：当前是否有弹窗打开，供全局快捷键判断"该不该让路"；
 * 2. body 滚动锁：打开弹窗时禁止背景滚动，关闭时恢复。
 *
 * # 为什么单独成模块
 * 弹窗是可以**叠加**的（例如「设置」里点删除会再弹出全局确认框）。若由每个
 * 弹窗实例各自把 `body.style.overflow` 置空，先关闭的那个就会把后开的那把锁
 * 提前释放，背景又能滚动了。因此这里用**计数**而不是布尔：只有最后一个弹窗
 * 关闭时才真正解锁。
 *
 * 同理，`App.vue` 的键盘处理与弹窗组件都需要"是否有弹窗打开"，各自维护一份
 * 状态必然漂移，统一读写同一个 ref 是唯一不会不同步的做法。
 */

import { computed, ref } from 'vue'

/** 当前处于打开态的模态弹窗数量 */
const openCount = ref(0)

/** 是否至少有一个模态弹窗打开（全局快捷键据此让路） */
export const anyDialogOpen = computed(() => openCount.value > 0)

/**
 * 登记一个弹窗进入打开态
 *
 * @returns 无返回值；副作用为计数 +1 并锁定 body 滚动
 */
export function acquireDialog(): void {
  openCount.value += 1
  document.body.style.overflow = 'hidden'
}

/**
 * 登记一个弹窗离开打开态
 *
 * 计数归零才恢复 body 滚动；多余调用（计数已是 0）被夹住，不会出现负数。
 *
 * @returns 无返回值；副作用为计数 -1，必要时恢复 body 滚动
 */
export function releaseDialog(): void {
  openCount.value = Math.max(0, openCount.value - 1)
  if (openCount.value === 0) {
    document.body.style.overflow = ''
  }
}
