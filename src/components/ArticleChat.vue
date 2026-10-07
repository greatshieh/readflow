<template>
  <!-- 文章追问入口：与「研究事件」FAB 同处右下角，垂直叠放在其上方。
       两者的可见条件由父组件统一控制（同一计算属性），因此不会出现"只剩一个悬空"的错位。 -->
  <div class="chat-fab-wrap" v-if="article">
    <!-- 浮层面板：消息气泡 / 预设问题 / 输入与发送停止 / 清空导出，见 ArticleChatPanel.vue。
         面板开合（panelOpen）、外点关闭与 Esc 由本组件统一持有。 -->
    <ArticleChatPanel
      ref="panelComp"
      :article="article"
      :open="panelOpen"
      @close="panelOpen = false"
    />

    <button
      ref="fabRef"
      class="chat-fab"
      :class="{ on: panelOpen, busy: chatStore.busy }"
      :title="fabTitle"
      :aria-expanded="panelOpen"
      aria-haspopup="dialog"
      aria-label="追问这篇文章"
      @click.stop="togglePanel"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <path d="M21 12a8 8 0 0 1-8 8H8l-4 3v-6.5A8 8 0 0 1 13 4a8 8 0 0 1 8 8Z"></path>
        <path d="M9 11h.01M13 11h.01M17 11h.01"></path>
      </svg>
      <span class="chat-fab-dot" v-if="chatStore.busy"></span>
    </button>
  </div>
</template>

<script setup lang="ts">
/**
 * 文章追问面板（右下角 FAB + 浮层对话）
 *
 * # 职责
 * 只负责「入口」：FAB 的可见性与徽标、面板开合、外点关闭与 Esc。
 * 对话界面本身（气泡 / 预设 / 输入 / 清空导出）在 `ArticleChatPanel.vue`，
 * 所有状态与后端调用都在 `stores/articleChat.ts`。
 *
 * # 设计意图
 * - **非模态浮层**：刻意不用 `BaseModal`——它是"停下手上的一切先处理这件事"的模态语义，
 *   而追问是伴随阅读的旁路动作，用户应当能一边看着正文一边提问。因此这里沿用阅读区
 *   「研究事件」面板的同一套交互：右下角 FAB 触发、点面板外或 Esc 关闭、不锁滚动。
 * - **外点关闭自己管**：不把 DOM ref 交给父组件维护白名单，避免每加一个浮层就要改
 *   `ContentColumn` 的 contains 判断；父组件只需挂载一行。面板根元素经
 *   `defineExpose({ rootEl })` 暴露，本组件据此判断"点在不在面板内"。
 *
 * # 无障碍
 * FAB 带 `aria-expanded` / `aria-haspopup`；面板 `role="dialog"` 并有 `aria-label`
 * （在 ArticleChatPanel 内）；Esc 关闭后焦点回到 FAB。
 */

import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { Article, ChatMessage } from '@/types'
import { useArticleChatStore } from '@/stores/articleChat'
import ArticleChatPanel from './ArticleChatPanel.vue'

const props = defineProps<{
  /** 当前打开的文章；null 时整个入口不渲染（模板首行已兜住） */
  article: Article | null
}>()

const chatStore = useArticleChatStore()

/** 面板是否展开 */
const panelOpen = ref(false)

const fabRef = ref<HTMLElement | null>(null)
/** 面板组件实例（取其暴露的根元素参与外点判断） */
const panelComp = ref<InstanceType<typeof ArticleChatPanel> | null>(null)

/** 当前文章 ID（无文章时为 null） */
const articleId = computed<number | null>(() => props.article?.id ?? null)

/** 当前文章的会话消息（仅用于 FAB 提示文案中的条数） */
const messages = computed<ChatMessage[]>(() =>
  articleId.value === null ? [] : chatStore.messages(articleId.value)
)

/** FAB 悬浮提示 */
const fabTitle = computed(() => {
  if (chatStore.busy) return '正在回答…'
  return messages.value.length > 0 ? `继续追问（已有 ${messages.value.length} 条对话）` : '就这篇文章向 AI 提问'
})

/** 展开 / 收起面板；聚焦输入框与滚动到底由面板组件 watch(open) 自理 */
function togglePanel(): void {
  panelOpen.value = !panelOpen.value
}

/**
 * 点到面板与 FAB 之外时收起面板
 *
 * 用 pointerdown 而非 click：与阅读区其它浮层一致，避免"面板内按下、外部松开"被误判为外点。
 *
 * @param e - 文档级指针事件
 */
function onDocPointerDown(e: Event): void {
  if (!panelOpen.value) return
  const target = e.target as Node | null
  if (!target) return
  if (panelComp.value?.rootEl?.contains(target) || fabRef.value?.contains(target)) return
  panelOpen.value = false
}

/**
 * Esc 收起面板
 *
 * 只在面板展开时拦截：收起状态下不干预全局快捷键（如聚焦模式退出）。
 *
 * @param e - 文档级键盘事件
 */
function onDocKeydown(e: KeyboardEvent): void {
  if (!panelOpen.value || e.key !== 'Escape') return
  e.preventDefault()
  panelOpen.value = false
  fabRef.value?.focus()
}

onMounted(() => {
  document.addEventListener('pointerdown', onDocPointerDown)
  document.addEventListener('keydown', onDocKeydown)
})

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocPointerDown)
  document.removeEventListener('keydown', onDocKeydown)
})
</script>

<style scoped>
/* ─── 定位：与研究 FAB 同处右下角，叠放在其上方（其 bottom 为 18px、高 38px） ─── */
.chat-fab-wrap {
  position: absolute;
  right: 18px;
  bottom: 68px;
  z-index: 31;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: var(--sp-15);
}

.chat-fab {
  position: relative;
  width: 38px;
  height: 38px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--surface);
  color: var(--primary);
  border: 1px solid var(--panel-border);
  box-shadow: var(--shadow-pop);
  cursor: pointer;
  transition: transform var(--dur) var(--ease), background var(--dur), color var(--dur);
}
.chat-fab svg {
  width: 18px;
  height: 18px;
}
.chat-fab:hover {
  background: var(--primary-soft);
  transform: translateY(-1px);
}
.chat-fab:active {
  transform: scale(0.96);
}
/* 展开态：填充主色，与面板形成"同源"的视觉关联 */
.chat-fab.on {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--tone-fg);
}
/* 生成中：右下角一枚小圆点，作为"回答还在流"的常驻提示（不拦截点击） */
.chat-fab-dot {
  position: absolute;
  top: -2px;
  right: -2px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--success);
  border: 2px solid var(--surface);
  pointer-events: none;
}
</style>
