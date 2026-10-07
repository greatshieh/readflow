<template>
  <!--
    全局弹窗容器：根据 activeModal.type 渲染三种不同形态的弹窗。
    - confirm：询问用户是否执行破坏性操作（删除等）
    - prompt：要求用户输入文本（添加订阅源、新建文件夹等）
    - alert：单向提示（保存失败等）

    外壳复用 BaseModal 的 `global` 变体：z-index 9999 压过普通弹窗（弹窗内点删除
    会再弹确认框），遮罩点击 / Esc / Tab 焦点陷阱 / 滚动锁统一由 BaseModal 处理。
    BaseModal 的 `close` 事件语义 = "用户以取消方式关闭"（Esc / 遮罩 / ×），
    alert 形态下遮罩与 Esc 原本就等于"确定"，由 handleClose 统一映射。
  -->
  <teleport to="body">
    <BaseModal
      :open="!!activeModal"
      :title="activeModal?.title ?? ''"
      size="sm"
      global
      @close="handleClose"
    >
      <template v-if="activeModal">
        <!-- 弹窗头部 -->
        <div class="modal-header">
          <span class="modal-title">{{ activeModal.title }}</span>
          <!-- 仅 confirm/alert 有关闭按钮；prompt 需要保留输入能力 -->
          <button
            v-if="activeModal.type !== 'prompt'"
            class="modal-close-btn"
            @click="handleClose"
            aria-label="关闭"
          >
            ×
          </button>
        </div>

        <!-- 弹窗内容 -->
        <div class="modal-body">
          <!-- 多行文本内容（支持换行符） -->
          <p class="modal-message" v-html="escapedMessage"></p>
        </div>

        <!-- prompt 专属：输入框 -->
        <div v-if="activeModal.type === 'prompt'" class="modal-input-wrap">
          <input
            :key="activeModal.inputKey"
            v-model="inputValue"
            type="text"
            :placeholder="activeModal.message"
            autocomplete="off"
            data-autofocus
            @keyup.enter="handlePromptConfirm"
          />
          <p class="input-hint">按 Enter 确认，Esc 取消</p>
        </div>

        <!-- 弹窗底部按钮区：确定按钮挂 data-autofocus，键盘用户打开即可直接 Enter -->
        <div class="modal-footer">
          <!-- confirm：确定 / 取消 -->
          <template v-if="activeModal.type === 'confirm'">
            <button class="btn btn-secondary" @click="handleCancel">取消</button>
            <button class="btn btn-primary" data-autofocus @click="handleConfirm">确定</button>
          </template>
          <!-- prompt：取消 / 确定 -->
          <template v-else-if="activeModal.type === 'prompt'">
            <button class="btn btn-secondary" @click="handlePromptCancel">取消</button>
            <button class="btn btn-primary" @click="handlePromptConfirm">确定</button>
          </template>
          <!-- alert：确定 -->
          <template v-else>
            <button class="btn btn-primary" data-autofocus @click="handleAlertConfirm">确定</button>
          </template>
        </div>
      </template>
    </BaseModal>
  </teleport>
</template>

<script setup lang="ts">
/**
 * 全局弹窗组件
 *
 * # 职责
 * 渲染 confirm / prompt / alert 三种形态的弹窗，通过 Teleport 挂到 body 下；
 * 状态由 useModalStore 统一管理，外壳复用 BaseModal 的 `global` 变体。
 *
 * # 设计要点
 * - confirm：展示消息，等待用户确认（返回 true/false）
 * - prompt：展示消息 + 输入框，支持 Enter 确认 / Esc 取消
 * - alert：展示消息，用户点确定关闭
 * - 遮罩点击 / Esc = 取消（confirm/prompt）或确定（alert），统一走 handleClose
 *
 * # 从自绘外壳迁移到 BaseModal（global 变体）
 * 原先自备遮罩、z-index 9999、window 级 Esc 监听与入场动画；迁移后这些全部由
 * BaseModal 提供，并补上了焦点陷阱与焦点归还。宽度从一次性 420px 归入 sm 档（460px）。
 * 输入框焦点改由 `data-autofocus` 声明（BaseModal 打开时优先聚焦它），
 * 原先 watch 里的手工 nextTick focus 一并删除。
 */

import { ref, watch } from 'vue'
import { useModalStore } from '@/stores/modal'
import { storeToRefs } from 'pinia'
import BaseModal from './BaseModal.vue'

const modalStore = useModalStore()
const { activeModal } = storeToRefs(modalStore)
// closeWithResult 是函数（非响应式），直接从 store 解构即可
const { closeWithResult } = modalStore

/** prompt 输入框的当前值 */
const inputValue = ref('')

/** 对消息中的 HTML 实体进行转义，防止 XSS */
const escapedMessage = ref('')

/**
 * 当弹窗激活时，重置输入值并转义消息
 *
 * 输入框聚焦交给 BaseModal 的 data-autofocus 机制，这里不再手工 focus。
 */
watch(
  () => activeModal.value,
  (modal) => {
    if (modal) {
      // 转义消息内容中的 HTML 特殊字符，防止 XSS
      escapedMessage.value = escapeHtml(modal.message)
      if (modal.type === 'prompt') {
        inputValue.value = modal.defaultValue ?? ''
      }
    } else {
      inputValue.value = ''
    }
  }
)

/** HTML 实体转义，防止 v-html 注入攻击 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;')
}

/**
 * 确认弹窗：用户点击"确定"
 *
 * @returns 无返回值；副作用为 closeWithResult(true)
 */
function handleConfirm() {
  closeWithResult(true)
}

/**
 * 确认弹窗：用户点击"取消"
 *
 * @returns 无返回值；副作用为 closeWithResult(false)
 */
function handleCancel() {
  closeWithResult(false)
}

/**
 * 输入弹窗：用户点击"确定"或按 Enter
 *
 * @returns 无返回值；副作用为 closeWithResult(inputValue)
 */
function handlePromptConfirm() {
  closeWithResult(inputValue.value)
}

/**
 * 输入弹窗：用户点击"取消"
 *
 * @returns 无返回值；副作用为 closeWithResult(null)
 */
function handlePromptCancel() {
  closeWithResult(null)
}

/**
 * 提示弹窗：用户点击"确定"
 *
 * @returns 无返回值；副作用为 closeWithResult(undefined)
 */
function handleAlertConfirm() {
  closeWithResult(undefined)
}

/**
 * 通用关闭：confirm/prompt 的取消路径（Esc / 遮罩 / × / 取消按钮），alert 的确定路径
 *
 * @returns 无返回值
 */
function handleClose() {
  if (activeModal.value?.type === 'alert') {
    closeWithResult(undefined)
  } else {
    closeWithResult(null)
  }
}
</script>

<style scoped>
/* 面板外壳（遮罩 / 圆角 / 投影 / 宽度 / 入场动画）由 BaseModal 提供，不再自绘。 */

/* ─── 弹窗头部 ──────────────────────────────────────────── */
.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-35) var(--sp-4);
  border-bottom: 1px solid var(--border);
}

.modal-title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}

.modal-close-btn {
  border: none;
  background: none;
  cursor: pointer;
  /* 22px：与其余弹窗的关闭按钮（RulesModal / ResearchModal / DigestModal /
     FeedManageModal）对齐。此前此处是 20px，同一语义元素出现两种尺寸。 */
  font-size: 22px;
  line-height: 1;
  color: var(--text-tertiary);
  padding: 0 var(--sp-1);
  transition: color 0.15s;
}

.modal-close-btn:hover {
  color: var(--text-primary);
}

/* ─── 弹窗内容 ──────────────────────────────────────────── */
.modal-body {
  padding: var(--sp-4);
}

.modal-message {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  line-height: 1.6;
  margin: 0;
  white-space: pre-wrap;
}

/* ─── prompt 输入框 ─────────────────────────────────────── */
.modal-input-wrap {
  padding: 0 var(--sp-4) var(--sp-3);
}

.modal-input-wrap input {
  width: 100%;
  height: 36px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-15);
  font-size: var(--fs-sm);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: border-color 0.15s;
  font-family: inherit;
  box-sizing: border-box;
}

.modal-input-wrap input:focus {
  border-color: var(--primary);
}

.input-hint {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  margin: var(--sp-05) 0 0;
}

/* ─── 弹窗底部按钮 ──────────────────────────────────────── */
.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-2);
  padding: var(--sp-3) var(--sp-4);
  border-top: 1px solid var(--border);
}

/* 弹窗底部按钮统一由全局按钮体系提供（styles.css 的 .btn / .btn-secondary / .btn-primary），
   本组件只保留布局（flex 容器与间距），不再复制按钮样式副本。 */
</style>
