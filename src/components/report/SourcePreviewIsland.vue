<template>
  <!-- 引用源预览浮岛：非模态（不锁滚动、不抢焦点、无焦点陷阱）。
       正因为非模态，用户可以一边看浮岛一边滚正文核对 —— 这是它比"直接跳转"强的地方。
       也正因如此它不进 dialogStack，Esc 得自己处理（见脚本里的 window 捕获监听）。 -->
  <Teleport to="body">
    <div
      ref="islandRef"
      class="island"
      role="dialog"
      aria-label="引用源预览"
      :style="{ left: `${pos.x}px`, top: `${pos.y}px` }"
    >
      <div class="island-head">
        <div class="island-title">{{ source.title }}</div>
        <button class="island-x" type="button" aria-label="关闭预览" @click="emit('close')">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>
      <p class="island-meta">{{ metaText }}</p>
      <div class="island-sep"></div>
      <p class="island-body">{{ snippet }}</p>
      <div class="island-foot">
        <button
          class="island-open"
          type="button"
          :disabled="source.articleId === null"
          :title="source.articleId === null ? '原文已不在库中（可能被清理任务删除）' : ''"
          @click="emit('open')"
        >
          {{ source.articleId === null ? '原文已不在库中' : '打开全文 →' }}
        </button>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
/**
 * 引用源预览浮岛
 *
 * # 职责
 * 在报告正文里点某条引用源的 `↗` 时，就地展示这篇文章的标题、来源与摘要，
 * 并给两个出口：关掉（继续读报告）或「打开全文」（跳到文章阅读）。
 *
 * # 为什么摘要要现取
 * `report_sources` 只存标题/来源/时间/链接的快照——摘要存进来会让报告体积翻倍，
 * 而它只在这个浮岛里用一次。点开时向 `article_get` 取一次即可。
 * 文章已被清理（`article_id` 为 NULL）时不给"打开全文"，避免跳到空白页。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { PeriodSource } from '@/stores/reportPeriod'

const props = defineProps<{
  /** 引用源快照 */
  source: PeriodSource
  /** 触发按钮的视口坐标 */
  anchor: { x: number; y: number }
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'open'): void
}>()

const islandRef = ref<HTMLElement | null>(null)

/** 从库里取到的摘要（空串 = 取不到，展示引导语） */
const fetched = ref('')

/** 浮岛尺寸（与 CSS 一致，用于 clamp） */
const ISLAND_W = 380
const ISLAND_H = 240

/**
 * 位置：贴按钮右侧；右边放不下就翻到左侧，再不行就贴边
 *
 * 浮岛是"点谁看谁"，位置必须锚在那个 `↗` 上，不能像弹窗一样居中——
 * 居中会让人找不到它是从哪冒出来的。
 */
const pos = computed(() => {
  const gap = 8
  const rightX = props.anchor.x + gap
  const x = rightX + ISLAND_W > window.innerWidth - 8 ? props.anchor.x - ISLAND_W - gap : rightX
  const y = Math.min(Math.max(8, props.anchor.y - 8), window.innerHeight - ISLAND_H - 8)
  return { x: Math.max(8, x), y }
})

/** 元信息行：来源 · 时间 */
const metaText = computed(() => {
  const parts = [props.source.feedName, props.source.published].filter(Boolean)
  return parts.length ? parts.join(' · ') : '来源未知'
})

/** 摘要正文：取不到时给一句明确说明，而不是留白 */
const snippet = computed(() => {
  if (props.source.articleId === null) return '这篇原文已被清理，只剩报告里的标题快照。'
  return fetched.value || '这篇文章没有摘要，可打开全文查看正文。'
})

/**
 * 取摘要
 *
 * 失败静默：浮岛本身仍有标题与来源可看，不值得为一次摘要请求弹错误。
 */
async function loadSnippet() {
  if (props.source.articleId === null) return
  try {
    const article = await invoke<{ summary?: string } | null>('article_get', {
      id: props.source.articleId,
    })
    const text = (article?.summary ?? '').trim()
    if (text) fetched.value = text
  } catch {
    // 忽略：保留"打开全文"这条主路径
  }
}

/**
 * Esc 关闭浮岛（**只关浮岛，不关弹窗**）
 *
 * 挂在 `window` 捕获阶段：`BaseModal` 在 `document` 捕获阶段监听 Esc，
 * 而捕获顺序是 window → document，故这里先拿到事件并 `stopPropagation`。
 *
 * @param e - 键盘事件
 */
function onWindowKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  e.preventDefault()
  e.stopPropagation()
  emit('close')
}

/**
 * 点浮岛之外任意位置关闭
 *
 * 非模态浮岛不能靠遮罩承接"点外部"，只能自己判。用 `mousedown` 与
 * `BaseModal` 的遮罩判定同源（避免"在里面按下、拖到外面松开"被误判）。
 * 浮岛是"点谁看谁"的临时视图，不主动关掉就会一直挡着正文。
 *
 * @param e - 鼠标事件
 */
function onDocMouseDown(e: MouseEvent) {
  const el = islandRef.value
  if (!el || el.contains(e.target as Node)) return
  emit('close')
}

onMounted(() => {
  window.addEventListener('keydown', onWindowKeydown, true)
  document.addEventListener('mousedown', onDocMouseDown)
  void loadSnippet()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onWindowKeydown, true)
  document.removeEventListener('mousedown', onDocMouseDown)
})
</script>

<style scoped>
/* 浮层视觉与 ContextMenu / 导出菜单同款；z-index 1060 高于弹窗遮罩，低于 Toast。
   注意：本组件 Teleport 到 body，落在 `.overlay` 之外，**拿不到** BaseModal 挂在
   .base-modal 上的排版/运动变量，故这里自带一份（值与 BaseModal 相同）。
   漏了字号别名不会报错，只会让 `font-size` 整条静默作废、全部退回继承值。 */
.island {
  --m-in: 200ms;
  --m-ease-out: cubic-bezier(0.2, 0.8, 0.2, 1);
  --fs-modal-label: var(--fs-sm);
  --fs-modal-meta: var(--fs-xs);
  position: fixed;
  width: min(380px, 92vw);
  max-height: 420px;
  z-index: 1060;
  display: flex;
  flex-direction: column;
  padding: 12px 12px 10px;
  background: var(--glass-strong);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid var(--glass-hair);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-pop);
  animation: island-in var(--m-in) var(--m-ease-out);
}
@keyframes island-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
}
.island-head {
  display: flex;
  align-items: flex-start;
  gap: 6px;
}
.island-title {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-modal-label);
  font-weight: 600;
  line-height: 1.45;
  color: var(--text-primary);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.island-x {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  display: grid;
  place-items: center;
  border: none;
  background: none;
  border-radius: 5px;
  color: var(--text-tertiary);
  cursor: pointer;
  margin: -2px -3px 0 0;
}
.island-x:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.island-x:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.island-meta {
  margin: 5px 0 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
.island-sep {
  height: 1px;
  background: var(--border);
  margin: 10px 0;
}
.island-body {
  margin: 0;
  font-size: var(--fs-modal-label);
  line-height: 1.68;
  color: var(--text-secondary);
  display: -webkit-box;
  -webkit-line-clamp: 4;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.island-foot {
  margin-top: 11px;
  display: flex;
  justify-content: flex-end;
}
.island-open {
  border: none;
  background: none;
  padding: 2px 0;
  color: var(--primary);
  cursor: pointer;
  font-family: inherit;
  font-size: var(--fs-modal-label);
  font-weight: 500;
}
.island-open:hover:not(:disabled) {
  text-decoration: underline;
}
.island-open:disabled {
  color: var(--text-tertiary);
  cursor: default;
}
.island-open:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  border-radius: 3px;
}
</style>
