<template>
  <!-- 研究事件悬浮入口：常驻正文栏右下角，点击在按钮上方浮出面板查看/提取事件。
       从正文中抽离是为了让阅读区保持"标题 → 摘要 → 正文"的干净流，
       避免每篇文章顶部都被一块无关卡片撑开。

       整个 FAB + 面板 + 面板的开关/外点关闭/Esc 都由本组件自持：
       它是"右下角一个可以自己开合的小浮层"，没有需要宿主参与的状态。
       是否显示由父组件用 v-if 决定（见 `showReadingFabs` 的注释：
       两个 FAB 共用同一个条件，避免条件分歧时留下悬空按钮）。 -->
  <div class="research-fab-wrap">
    <!-- 浮层面板：事件列表 + 提取入口。点击面板内部不关闭（@click.stop） -->
    <div class="research-panel" v-if="panelOpen" ref="panelRef" @click.stop>
      <div class="rp-head">
        <span class="rp-title">研究事件</span>
        <button class="rp-action" :disabled="extracting" @click.stop="extractEvents">
          {{ extracting ? '提取中…' : extractActionLabel }}
        </button>
      </div>
      <p class="rp-msg" :class="{ err: extractError }" v-if="extractMsg">{{ extractMsg }}</p>
      <div class="rp-body" v-if="events.length > 0">
        <div v-for="ev in events" :key="ev.id" class="rp-event">
          <span class="rp-dot" :class="`et-${ev.event_type}`" :title="eventTypeLabel(ev.event_type)"></span>
          <span class="rp-fact">{{ ev.fact }}</span>
        </div>
      </div>
      <p class="rp-empty" v-else>{{ extractEmptyHint }}</p>
    </div>
    <button
      ref="fabRef"
      class="research-fab"
      :class="{ on: panelOpen }"
      :disabled="extracting"
      :title="fabTitle"
      @click.stop="panelOpen = !panelOpen"
    >
      <span class="ai-spinner" v-if="extracting"></span>
      <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
        <circle cx="12" cy="6" r="2.2"></circle>
        <circle cx="6" cy="16" r="2.2"></circle>
        <circle cx="18" cy="16" r="2.2"></circle>
        <path d="M10.4 7.7 7.6 14.3M13.6 7.7l2.8 6.6M8.2 16h7.6"></path>
      </svg>
      <span class="research-fab-badge" v-if="events.length > 0 && !extracting">
        {{ events.length > 99 ? '99+' : events.length }}
      </span>
    </button>
  </div>
</template>

<script setup lang="ts">
/**
 * 研究事件悬浮入口（右下角 FAB + 上方浮层面板）
 *
 * # 职责
 * 「查看本文已提取的研究事件」与「手动触发提取」的全部界面与状态：事件列表、
 * 提取结果提示、面板开合、外点关闭、Esc 关闭。
 *
 * # 为什么把 document 监听也搬进来
 * 面板的"点空白处 / 按 Esc 收起"只与本组件有关。留在宿主里会让宿主持有
 * `panelRef` / `fabRef` 两个它并不使用的元素引用；搬进来后宿主对这两个浮层
 * 一无所知，浮层之间也不会互相干扰（各判各的状态）。
 *
 * # 两个易错点（原实现踩过，勿回退）
 * - **`extractAttempted`（是否问过模型）与 `events`（问出了几条）是两件事**：
 *   `ai_extracted_at` 有值即表示"问过"，只看事件条数会把"提取过但本文无事件"
 *   显示成"尚未提取"。初值取自文章的 `ai_extracted_at`，跨重启有效。
 * - **提取命令的返回值（实际新增条数）必须用起来**：后端成功但抽到 0 条时不报错，
 *   若丢弃返回值，面板会停在"尚未提取"，用户无法分辨成功 / 空 / 坏掉。
 */

import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { useResearchStore } from '@/stores/research'
import type { Article, ResearchEvent } from '@/types'

const props = defineProps<{
  /** 当前正在阅读的文章（本组件的所有操作都作用于它） */
  article: Article
}>()

const researchStore = useResearchStore()

/** 当前文章已提取的研究事件 */
const events = ref<ResearchEvent[]>([])

/** 提取进行中（按钮 loading 态） */
const extracting = ref(false)

/**
 * 当前文章是否已尝试过提取
 *
 * 初值取自库里的 `ai_extracted_at`（跨重启有效），提取动作成功后本地置位。
 * 它和 `events` 是两件事：前者表示"问过模型了"，后者表示"问出了东西"——
 * 只按事件条数判断，就会把"提取过但本文没有事件"显示成"尚未提取"。
 */
const extractAttempted = ref(false)

/** 提取结果提示：新增条数 / 无事件说明 / 失败原因 */
const extractMsg = ref('')

/** 提示是否为错误态（错误走警告色，空结果走中性提示色） */
const extractError = ref(false)

/** 浮层面板展开态 */
const panelOpen = ref(false)

/** 浮层面板元素引用（用于判断"点击面板内部"时不关闭） */
const panelRef = ref<HTMLElement | null>(null)

/** 悬浮按钮元素引用（点击按钮自身由 toggle 处理，不触发外部关闭） */
const fabRef = ref<HTMLElement | null>(null)

/** 悬浮按钮的悬浮提示文案（随事件数变化） */
const fabTitle = computed(() => {
  if (extracting.value) return '正在提取…'
  return events.value.length > 0
    ? `已提取 ${events.value.length} 条研究事件`
    : '从本文提取结构化研究事件'
})

/**
 * 提取按钮文案
 *
 * 已抽出事件、或已尝试过提取（含抽到 0 条）时都显示"重新提取"：
 * 两者再点一次都是重跑模型，文案应当一致，也顺带告诉用户"这篇问过了"。
 */
const extractActionLabel = computed(() =>
  events.value.length > 0 || extractAttempted.value ? '重新提取' : '提取'
)

/**
 * 面板空态文案
 *
 * 区分「从未提取」与「提取过但本文没有事件」。后者此前与"功能没反应"长得
 * 一模一样，是用户以为提取坏了的直接原因——原文只有导语（部分订阅源不提供正文）
 * 时模型必然抽出 0 条，属于正常结论，需要说清楚。
 */
const extractEmptyHint = computed(() =>
  extractAttempted.value
    ? '本文已提取过，模型没有抽到可抽取的事件。常见原因是原文过短（该订阅源可能只提供导语）或内容与公司/行业动态无关。'
    : '尚未提取。点击右上角「提取」，事件会进入研究工作台的时间线与报告'
)

/** 事件类型枚举 → 中文标签（与后端 EVENT_TYPES 对齐） */
const EVENT_LABELS: Record<string, string> = {
  product: '产品',
  executive: '高管',
  ma: '并购',
  regulatory: '监管',
  strategy: '战略',
  competition: '竞争',
  industry_signal: '行业信号'
}

/**
 * 事件类型的中文标签
 *
 * @param type - 事件类型枚举值
 * @returns 中文标签；未知类型原样返回
 */
function eventTypeLabel(type: string): string {
  return EVENT_LABELS[type] ?? type
}

/**
 * 加载指定文章的事件列表
 *
 * @param articleId - 文章 ID
 * @returns 无返回值；失败时清空列表（事件加载失败不应影响阅读主流程）
 */
async function loadArticleEvents(articleId: number) {
  try {
    events.value = await researchStore.loadEventsByArticle(articleId)
  } catch (e) {
    events.value = []
    console.error('加载研究事件失败:', e)
  }
}

/**
 * 手动提取当前文章的研究事件
 *
 * 后端返回的是**实际新增条数**，它必须被用起来：0 条意味着"模型认为本文没有
 * 可抽取的事实"（正常结论，原文只有导语时必然如此），但后端不会报错，前端若
 * 丢弃返回值，面板就停在"尚未提取"上，用户无法分辨是成功、是空、还是坏了。
 *
 * 失败不打断阅读：错误写进面板提示行，同时保留一份控制台记录。
 */
async function extractEvents() {
  if (extracting.value) return
  const articleId = props.article.id
  extracting.value = true
  extractMsg.value = ''
  extractError.value = false
  try {
    const inserted = await researchStore.extractArticle(articleId)
    // 走 loadArticleEvents 而非直接 invoke：它内部吞掉异常并清空列表，
    // 避免"刷新事件列表失败"被误报成"提取失败"
    await loadArticleEvents(articleId)
    extractAttempted.value = true
    extractMsg.value =
      inserted > 0 ? `已提取 ${inserted} 条研究事件` : '本文没有可抽取的事件'
  } catch (e) {
    extractError.value = true
    extractMsg.value = `提取失败：${e instanceof Error ? e.message : String(e)}`
    console.error('提取研究事件失败:', e)
  } finally {
    extracting.value = false
  }
}

// 换文章即重置本组件随文章变化的全部状态：面板收起、事件清空、提示清空，
// 并按新文章的 ai_extracted_at 恢复"是否问过模型"。immediate 保证挂载时也执行一次。
watch(
  () => props.article.id,
  (id) => {
    panelOpen.value = false
    events.value = []
    extractMsg.value = ''
    extractError.value = false
    extracting.value = false
    extractAttempted.value = !!props.article.ai_extracted_at
    void loadArticleEvents(id)
  },
  { immediate: true }
)

/**
 * 点击面板与按钮之外的地方时关闭面板
 *
 * 用 document 级监听而非全屏透明遮罩：遮罩会吞掉滚轮事件导致正文无法滚动，
 * 而监听只判断点击目标，不影响任何滚动或文本选择行为。
 *
 * @param e - 鼠标事件
 */
function onDocumentClick(e: MouseEvent) {
  if (!panelOpen.value) return
  const target = e.target as Node | null
  if (!target) return
  if (panelRef.value?.contains(target) || fabRef.value?.contains(target)) return
  panelOpen.value = false
}

/**
 * Esc 关闭面板
 *
 * @param e - 键盘事件
 */
function onDocumentKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  if (panelOpen.value) panelOpen.value = false
}

// 挂在 document 的冒泡阶段：面板与按钮内部的点击由各自的 handler 处理，
// 冒泡上来时已被上面的 contains 判断放行，不会误关。
onMounted(() => {
  document.addEventListener('click', onDocumentClick)
  document.addEventListener('keydown', onDocumentKeydown)
})

// 与上面的注册成对解绑，否则组件卸载后 document 仍持有本组件的回调
onUnmounted(() => {
  document.removeEventListener('click', onDocumentClick)
  document.removeEventListener('keydown', onDocumentKeydown)
})
</script>

<style scoped>
/* ─── 研究事件悬浮入口（右下角 FAB + 上方浮层面板） ─── */
/* 定位上下文：FAB 相对正文栏右下角固定，不随 .reading-scroll 滚动 */
.research-fab-wrap {
  position: absolute;
  right: 18px;
  bottom: 18px;
  z-index: 30;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: var(--sp-15);
}
.research-fab {
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
  transition: transform var(--dur) var(--ease), background var(--dur), color var(--dur),
    box-shadow var(--dur);
}
.research-fab svg {
  width: 18px;
  height: 18px;
}
.research-fab:hover {
  background: var(--primary-soft);
  transform: translateY(-1px);
}
.research-fab:active {
  transform: scale(0.96);
}
/* 展开态：按钮填充主色，与面板形成"同源"的视觉关联 */
.research-fab.on {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--tone-fg);
}
.research-fab:disabled {
  cursor: default;
}
/* 提取中：图标位换成转圈并压到与图标相近的尺寸，按钮不跳动 */
.research-fab .ai-spinner {
  width: 14px;
  height: 14px;
  border-width: 2px;
}
/* 数量徽标：2px 描边用面板底色"抠"出一圈间隙，避免与按钮圆边糊在一起 */
.research-fab-badge {
  position: absolute;
  top: -4px;
  right: -4px;
  min-width: 17px;
  height: 17px;
  padding: 0 var(--sp-1);
  border-radius: 9px;
  background: var(--primary);
  border: 2px solid var(--surface);
  color: var(--tone-fg);
  font-size: var(--fs-xs);
  font-weight: 600;
  line-height: 15px;
  text-align: center;
}
.research-fab.on .research-fab-badge {
  border-color: var(--primary);
}
.research-panel {
  width: 260px;
  max-height: 320px;
  overflow-y: auto;
  padding: var(--sp-15) var(--sp-3);
  background: var(--surface);
  border: 1px solid var(--panel-border);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-pop);
}
.rp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  margin-bottom: var(--sp-2);
}
.rp-title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}
.rp-action {
  flex-shrink: 0;
  padding: var(--sp-025) var(--sp-2);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--primary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: all 0.15s;
}
.rp-action:hover:not(:disabled) {
  border-color: var(--primary);
  background: var(--primary-soft);
}
.rp-action:disabled {
  color: var(--text-tertiary);
  cursor: default;
}
.rp-body {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.rp-event {
  display: flex;
  align-items: flex-start;
  gap: var(--sp-2);
}
.rp-dot {
  flex-shrink: 0;
  margin-top: 5px;
  width: 6px;
  height: 6px;
  border-radius: 3px;
}
.rp-fact {
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-secondary);
  word-break: break-word;
}
/* 提取结果提示行：与空态同为小字，空结果用中性色、失败用警告色 */
.rp-msg {
  margin: 0 0 var(--sp-1);
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-secondary);
}
.rp-msg.err {
  color: var(--danger);
}
.rp-empty {
  margin: 0;
  font-size: var(--fs-sm);
  line-height: 1.6;
  color: var(--text-tertiary);
}
/* 事件类型配色：与 ResearchModal 的时间线语义一致 */
.et-product { background: var(--entity-product); }
.et-executive { background: var(--entity-executive); }
.et-ma { background: var(--entity-ma); }
.et-regulatory { background: var(--entity-regulatory); }
.et-strategy { background: var(--entity-strategy); }
.et-competition { background: var(--entity-competition); }
.et-industry_signal { background: var(--entity-industry_signal); }
</style>
