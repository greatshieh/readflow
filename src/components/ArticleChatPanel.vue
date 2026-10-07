<template>
  <!-- 追问浮层面板：消息列表 + 输入区。展开 / 收起由父组件（FAB 宿主）以 `open` 控制；
       面板内部点击不关闭（@click.stop），外点与 Esc 由宿主的 document 监听统一处理 -->
  <div class="chat-panel" v-if="open" ref="rootEl" role="dialog" aria-label="文章追问" @click.stop>
    <div class="cp-head">
      <span class="cp-title">追问这篇文章</span>
      <div class="cp-head-actions">
        <button
          class="cp-icon-btn"
          :disabled="!hasMessages"
          title="导出为 Markdown"
          aria-label="导出为 Markdown"
          @click.stop="handleExport"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 3v12m0 0 4-4m-4 4-4-4"></path>
            <path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"></path>
          </svg>
        </button>
        <button
          class="cp-icon-btn"
          :disabled="!hasMessages"
          title="清空本次对话"
          aria-label="清空本次对话"
          @click.stop="handleClear"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"></path>
            <path d="M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12"></path>
          </svg>
        </button>
        <button class="cp-icon-btn" title="关闭" aria-label="关闭" @click.stop="emit('close')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
            <path d="M6 6l12 12M18 6L6 18"></path>
          </svg>
        </button>
      </div>
    </div>

    <p class="cp-error" v-if="chatStore.error">{{ chatStore.error }}</p>

    <div class="cp-body" ref="bodyRef">
      <!-- 空态：预设问题药丸。首次使用的引导成本主要在这里 -->
      <div class="cp-empty" v-if="!hasMessages && !streaming">
        <p class="cp-empty-hint">
          就这篇文章提问，回答只依据文章正文、命中的关注实体与已提取的研究事件。
        </p>
        <div class="cp-presets">
          <button
            v-for="q in PRESET_QUESTIONS"
            :key="q"
            class="cp-preset"
            @click.stop="ask(q)"
          >
            {{ q }}
          </button>
        </div>
      </div>

      <template v-else>
        <div v-for="(m, i) in messages" :key="i" class="cp-turn" :class="m.role">
          <div class="cp-bubble" :class="{ error: m.error }">
            <span class="cp-text">{{ m.content }}</span>
            <span class="cp-note" v-if="m.stopped">（已停止生成）</span>
          </div>
        </div>

        <!-- 流式中：回答完成前用独立气泡承载逐字文本，不提前写进会话记录 -->
        <div class="cp-turn assistant" v-if="streaming">
          <div class="cp-bubble">
            <span class="cp-text" v-if="streamText">{{ streamText }}</span>
            <span class="cp-typing" v-else aria-label="正在生成">
              <i></i><i></i><i></i>
            </span>
          </div>
        </div>
      </template>
    </div>

    <div class="cp-input-row">
      <textarea
        ref="inputRef"
        v-model="draft"
        class="cp-input"
        rows="2"
        placeholder="追问这篇文章…"
        :disabled="chatStore.busy"
        @click.stop
        @keydown.enter.exact.prevent="ask()"
      ></textarea>
      <div class="cp-input-side">
        <button v-if="chatStore.busy" class="btn btn-sm cp-send" @click.stop="chatStore.stop()">
          停止
        </button>
        <button
          v-else
          class="btn btn-primary btn-sm cp-send"
          :disabled="!canSend"
          @click.stop="ask()"
        >
          发送
        </button>
        <span class="cp-input-hint">Enter 发送 · Shift+Enter 换行</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 文章追问的浮层面板（消息气泡 + 预设问题 + 输入与发送/停止 + 清空/导出）
 *
 * # 职责
 * 渲染对话界面并直连 `stores/articleChat.ts`：发送、停止、清空、导出都在这里完成。
 * 面板的开合归父组件（ArticleChat 的 FAB 宿主）——它要同时管外点关闭与 Esc，
 * 因此本组件只接收 `open` prop 并上抛「关闭」。
 *
 * # 流式文本不提前入会话
 * 逐字期间渲染的是合成气泡，`invoke` 返回后才把整段写进会话记录。
 * 这样"停止"只需丢弃合成气泡即可，不会在记录里留下半条脏数据。
 *
 * # 与宿主的 DOM 契约
 * 宿主的外点关闭需要判断"点在不在面板内"，因此暴露根元素 `rootEl`；
 * 展开时的输入框聚焦与滚动到底也在这里做（`watch(open)`），宿主不再插手。
 */

import { computed, nextTick, ref, watch } from 'vue'
import type { Article, ChatMessage } from '@/types'
import { useArticleChatStore } from '@/stores/articleChat'
import { useModalStore } from '@/stores/modal'

const props = defineProps<{
  /** 当前打开的文章；null 时内部会话相关计算全部退化为空 */
  article: Article | null
  /** 面板是否展开（由宿主控制） */
  open: boolean
}>()

const emit = defineEmits<{
  /** 用户点 ×；宿主把 open 置 false */
  (e: 'close'): void
}>()

const chatStore = useArticleChatStore()
const modalStore = useModalStore()

/** 预设问题：覆盖"读懂一篇文章"最常用的四个角度 */
const PRESET_QUESTIONS = [
  '核心论点是什么？',
  '有哪些关键数据或事实？',
  '作者的立场与倾向如何？',
  '这对我意味着什么？'
]

/** 面板根元素（宿主外点关闭的白名单成员，经 defineExpose 暴露） */
const rootEl = ref<HTMLElement | null>(null)
/** 输入框草稿 */
const draft = ref('')
const bodyRef = ref<HTMLElement | null>(null)
const inputRef = ref<HTMLTextAreaElement | null>(null)

/** 当前文章 ID（无文章时为 null） */
const articleId = computed<number | null>(() => props.article?.id ?? null)

/** 当前文章的会话消息 */
const messages = computed<ChatMessage[]>(() =>
  articleId.value === null ? [] : chatStore.messages(articleId.value)
)

/** 当前文章是否正在生成回答 */
const streaming = computed(
  () => articleId.value !== null && chatStore.streamArticleId === articleId.value
)

/** 流式累积的文本 */
const streamText = computed(() => chatStore.streamText)

/** 是否已有对话内容（决定导出 / 清空按钮可用性） */
const hasMessages = computed(() => messages.value.length > 0)

/** 是否可以发送（有内容且没有在途回答） */
const canSend = computed(() => draft.value.trim().length > 0 && !chatStore.busy)

/**
 * 滚动消息区到底部
 *
 * @param force - 为 true 时无视用户当前位置强制拉到底（用户刚发出提问时使用）；
 *                为 false 时仅在用户本就在底部附近才跟随，避免把他从正在翻阅的
 *                历史消息里拽走
 */
function scrollToBottom(force = false): void {
  const el = bodyRef.value
  if (!el) return
  const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80
  if (force || nearBottom) el.scrollTop = el.scrollHeight
}

// 新消息与流式增量都跟随滚动（用户上翻时不打断）
watch([() => messages.value.length, streamText], () => {
  nextTick(() => scrollToBottom())
})

// 展开时把焦点交给输入框，省去一次点击；immediate 覆盖"挂载时已处于展开态"的情况
watch(
  () => props.open,
  (open) => {
    if (!open) return
    nextTick(() => {
      inputRef.value?.focus()
      scrollToBottom(true)
    })
  },
  { immediate: true }
)

/**
 * 发送一次提问
 *
 * @param preset - 点选预设问题时传入该问题文本；从输入框发送时不传，
 *                 此时读取并清空草稿
 * @returns 无返回值；提问与回答由 store 维护
 */
async function ask(preset?: string): Promise<void> {
  const id = articleId.value
  if (id === null || chatStore.busy) return

  const text = (preset ?? draft.value).trim()
  if (!text) return
  // 只有"从输入框发送"才清空草稿：点预设药丸不该把用户已经打了一半的问题抹掉
  if (preset === undefined) draft.value = ''

  await nextTick()
  scrollToBottom(true)
  await chatStore.send(id, text)
  await nextTick()
  scrollToBottom()
}

/** 清空当前文章的对话（先确认，再清） */
async function handleClear(): Promise<void> {
  const id = articleId.value
  if (id === null) return
  const ok = await modalStore.showConfirm({
    title: '清空对话',
    message: '确定清空当前文章的全部追问记录吗？此操作不可撤销（可先导出为 Markdown 留存）。'
  })
  if (!ok) return
  chatStore.clear(id)
}

/**
 * 导出当前文章的对话为 Markdown 文件
 *
 * 文件读写都在 Rust 侧（系统「另存为」对话框），此处只负责把结果反馈给用户。
 *
 * @returns 无返回值；用户取消对话框时静默返回
 */
async function handleExport(): Promise<void> {
  const id = articleId.value
  const title = props.article?.title ?? '文章'
  if (id === null) return
  try {
    const path = await chatStore.exportMarkdown(id, title)
    if (path === null) return
    // 用全局提示而不是原生 alert：与设置页导出 OPML 的反馈方式保持一致
    await modalStore.showAlert({ title: '导出完成', message: `对话已保存到：\n${path}` })
  } catch (e) {
    await modalStore.showAlert({
      title: '导出失败',
      message: e instanceof Error ? e.message : String(e)
    })
  }
}

defineExpose({
  /** 面板根元素：宿主据此判断外点关闭时是否点在面板内 */
  rootEl
})
</script>

<style scoped>
/* 样式自 ArticleChat 原样搬入（.chat-panel 与全部 .cp-*）；
   FAB 与浮层定位（.chat-fab-wrap / .chat-fab*）仍属宿主组件。 */
.chat-panel {
  width: 380px;
  height: min(440px, 62vh);
  display: flex;
  flex-direction: column;
  background: var(--surface);
  border: 1px solid var(--panel-border);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-pop);
  overflow: hidden;
}
.cp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  padding: var(--sp-15) var(--sp-3);
  border-bottom: 1px solid var(--border);
}
.cp-title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}
.cp-head-actions {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
}
.cp-icon-btn {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background var(--dur), color var(--dur);
}
.cp-icon-btn svg {
  width: 15px;
  height: 15px;
}
.cp-icon-btn:hover:not(:disabled) {
  background: var(--hover-bg);
  color: var(--primary);
}
.cp-icon-btn:disabled {
  color: var(--text-tertiary);
  cursor: default;
}
.cp-error {
  margin: 0;
  padding: var(--sp-1) var(--sp-3);
  background: var(--danger);
  color: var(--danger-fg);
  font-size: var(--fs-xs);
  line-height: 1.5;
}

/* 消息区：flex:1 + min-height:0 是让内部滚动条生效的关键
   （默认 min-height:auto 会撑破容器，滚动条落到外层） */
.cp-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-3);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}

.cp-empty {
  margin: auto 0;
  text-align: center;
}
.cp-empty-hint {
  margin: 0 0 var(--sp-3);
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-secondary);
}
.cp-presets {
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
}
.cp-preset {
  padding: var(--sp-1) var(--sp-15);
  border: 1px solid var(--border);
  border-radius: var(--r-pill);
  background: var(--surface);
  color: var(--text-primary);
  font-size: var(--fs-sm);
  text-align: left;
  cursor: pointer;
  transition: border-color var(--dur), color var(--dur), background var(--dur);
}
.cp-preset:hover {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--primary-soft);
}

.cp-turn {
  display: flex;
}
.cp-turn.user {
  justify-content: flex-end;
}
.cp-turn.assistant {
  justify-content: flex-start;
}
.cp-bubble {
  max-width: 84%;
  padding: var(--sp-15) var(--sp-2);
  border-radius: var(--r-md);
  font-size: var(--fs-sm);
  line-height: 1.65;
  color: var(--text-primary);
  background: var(--surface-muted);
  border: 1px solid var(--border);
  white-space: pre-wrap;
  word-break: break-word;
}
.cp-turn.user .cp-bubble {
  background: var(--primary-soft);
  border-color: transparent;
}
.cp-bubble.error {
  background: var(--danger);
  color: var(--danger-fg);
  border-color: transparent;
}
.cp-note {
  color: var(--text-tertiary);
  font-size: var(--fs-xs);
}
.cp-bubble.error .cp-note {
  color: var(--danger-fg);
}

/* 等待首字：三点呼吸，明确表达"模型正在写"而不是界面卡死 */
.cp-typing {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-05);
}
.cp-typing i {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--text-tertiary);
  animation: cp-blink 1.2s infinite ease-in-out;
}
.cp-typing i:nth-child(2) {
  animation-delay: 0.2s;
}
.cp-typing i:nth-child(3) {
  animation-delay: 0.4s;
}
/* @keyframes 必须与使用它的规则同处一份样式表（scoped 下会加作用域后缀） */
@keyframes cp-blink {
  0%,
  80%,
  100% {
    opacity: 0.25;
  }
  40% {
    opacity: 1;
  }
}
@media (prefers-reduced-motion: reduce) {
  .cp-typing i {
    animation: none;
    opacity: 0.6;
  }
}

/* ─── 输入区 ─── */
.cp-input-row {
  display: flex;
  align-items: flex-end;
  gap: var(--sp-1);
  padding: var(--sp-2) var(--sp-3) var(--sp-15);
  border-top: 1px solid var(--border);
}
.cp-input {
  flex: 1;
  min-width: 0;
  padding: var(--sp-1) var(--sp-15);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-primary);
  font-size: var(--fs-sm);
  line-height: 1.6;
  resize: none;
}
.cp-input:focus {
  outline: none;
  border-color: var(--primary);
}
.cp-input:disabled {
  color: var(--text-tertiary);
  background: var(--surface-muted);
}
.cp-input-side {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--sp-05);
  flex-shrink: 0;
}
.cp-send {
  min-width: 52px;
}
.cp-input-hint {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}
</style>
