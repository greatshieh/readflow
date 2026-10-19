<!--
  ToastHost —— 全局轻提示容器

  # 职责
  把 `stores/toast.ts` 里的提示队列渲染成屏幕底部居中的一串浮条，每条可带一个动作按钮
  （撤销 / 重试）。全局只挂一次，在 `App.vue` 根容器内。

  # 为什么 Teleport 到 body
  Toast 必须能盖在**任何**浮层之上——包括设置弹窗（z-index 1000）。
  若留在 `.app` 内，会被 `overflow: hidden` 的栏容器裁掉，也会被 `.app` 的 zoom 缩放
  （文字大小和圆角跟着界面缩放在小档位下会失真）。挂到 body 后使用视口坐标，
  与 zoom 解耦。z-index 取 1100：高于 BaseModal(1000)、右键菜单/悬浮提示(950)。

  # 无障碍
  - 容器 `aria-live="polite"` + `role="status"`：屏幕阅读器会播报新增提示，
    但不打断当前朗读（撤销的机会窗口很短，用 assertive 太粗暴）。
  - 动作按钮是原生 `<button>`，天然可 Tab 到达；关闭按钮带 `aria-label`。
-->
<template>
  <Teleport to="body">
    <transition-group name="toast" tag="div" class="toast-host" role="status" aria-live="polite">
      <div v-for="item in toastStore.items" :key="item.id" class="toast">
        <span class="toast-text" :title="item.text">{{ item.text }}</span>
        <button
          v-if="item.action"
          class="toast-action"
          type="button"
          @click="toastStore.runAction(item.id)"
        >{{ item.action.label }}</button>
        <button
          class="toast-close"
          type="button"
          aria-label="关闭提示"
          @click="toastStore.dismiss(item.id)"
        >
          <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
            <path d="M4 4l8 8M12 4l-8 8" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
          </svg>
        </button>
      </div>
    </transition-group>
  </Teleport>
</template>

<script setup lang="ts">
import { useToastStore } from '@/stores/toast'

const toastStore = useToastStore()
</script>

<style scoped>
/* 容器本身不拦截鼠标：空白区域（提示之间、屏幕中下部）必须能点到底下的界面。
   fixed + 底部居中，自下而上堆叠（后出现的在上），与"新消息从下方推入"的直觉一致 */
.toast-host {
  position: fixed;
  left: 50%;
  bottom: var(--sp-6);
  transform: translateX(-50%);
  z-index: 1100;
  display: flex;
  flex-direction: column-reverse;
  align-items: center;
  gap: var(--sp-2);
  pointer-events: none;
}
.toast {
  /* 单条提示恢复鼠标交互（撤销要能点） */
  pointer-events: auto;
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  max-width: min(560px, 80vw);
  padding: 10px 10px 10px var(--sp-4);
  border-radius: var(--r-pill);
  background: var(--glass-strong);
  border: 1px solid var(--glass-hair);
  box-shadow: var(--shadow-pop);
  backdrop-filter: blur(var(--blur)) saturate(1.6);
  -webkit-backdrop-filter: blur(var(--blur)) saturate(1.6);
}
.toast-text {
  /* 长源名会撑破胶囊：单行截断，完整内容靠 title 兜底 */
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--fs-sm);
  color: var(--text-primary);
}
.toast-action {
  flex-shrink: 0;
  padding: 4px 10px;
  border: none;
  border-radius: var(--r-pill);
  background: var(--primary-soft);
  color: var(--primary-hover);
  font-size: var(--fs-sm);
  font-weight: 600;
  cursor: pointer;
  transition: background var(--dur) var(--ease), color var(--dur) var(--ease);
}
.toast-action:hover { background: var(--primary); color: #fff; }
.toast-action:focus-visible { outline: 2px solid var(--primary); outline-offset: 2px; }
.toast-close {
  flex-shrink: 0;
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: background var(--dur) var(--ease), color var(--dur) var(--ease);
}
.toast-close:hover { background: var(--fill-secondary); color: var(--text-primary); }
.toast-close:focus-visible { outline: 2px solid var(--primary); outline-offset: 1px; }

/* 进出场：淡入 + 轻微上移，尊重系统的"减少动效"偏好 */
.toast-enter-active,
.toast-leave-active { transition: opacity var(--dur) var(--ease), transform var(--dur) var(--ease); }
.toast-enter-from,
.toast-leave-to { opacity: 0; transform: translateY(8px); }
@media (prefers-reduced-motion: reduce) {
  .toast-enter-active,
  .toast-leave-active { transition: opacity var(--dur) var(--ease); }
  .toast-enter-from,
  .toast-leave-to { transform: none; }
}
</style>
