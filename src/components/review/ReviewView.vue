<template>
  <!-- 复盘页（阶段二）。
       独立页面而不是弹窗：复盘是 5–15 分钟、有进行状态的会话，而 `BaseModal` 让 Esc
       关弹窗、`App.vue` 的 Esc 又让非文章视图回文章栏——同一个键在"看一眼就走"的
       任务里是对的，在会话里会让用户以为已经结束了（PRD-05 §4.1）。
       本页保留订阅栏（同推荐流），切换即卸载，会话状态在 store 里，不随卸载丢失。 -->
  <section class="rv">
    <header class="rv-head">
      <div class="rv-head-main">
        <h2 class="rv-title">复盘</h2>
        <span class="rv-sub">{{ subtitle }}</span>
      </div>
      <div class="rv-head-actions">
        <button
          v-if="store.overview?.session"
          type="button"
          class="rv-btn rv-btn-primary"
          :disabled="store.marking"
          @click="onFinish"
        >
          结束本次复盘
        </button>
      </div>
    </header>

    <p v-if="store.error" class="rv-error">{{ store.error }}</p>

    <!-- 摘要态：一次复盘结束后停在这里，让人看到"刚刚做了什么" -->
    <div v-if="store.summary" class="rv-summary">
      <p class="rv-summary-text">{{ store.summary.summary }}</p>
      <p class="rv-summary-meta">{{ store.summary.finishedAt }}</p>
      <div class="rv-summary-actions">
        <button type="button" class="rv-btn rv-btn-primary" @click="onNewRound">再来一轮</button>
        <button type="button" class="rv-btn" @click="backToArticles">回到文章</button>
      </div>
    </div>

    <p v-else-if="store.loading" class="rv-loading">正在整理要复查的判断…</p>

    <template v-else-if="store.overview">
      <!-- 安静态：三区全空时不说"恭喜"，只说事实和下次时间 -->
      <div v-if="store.overview.quiet" class="rv-quiet">
        <p class="rv-quiet-main">本期无需复盘。</p>
        <p class="rv-quiet-sub">
          没有到期的判据，也没有结构异常的判断。
          <template v-if="store.overview.nextCheckAt">下次检查 {{ store.overview.nextCheckAt }}。</template>
        </p>
      </div>

      <template v-else>
        <section v-if="store.overview.due.length" class="rv-zone">
          <h3 class="rv-zone-title">
            该复查了<span class="rv-count">{{ store.overview.due.length }}</span>
          </h3>
          <p class="rv-zone-desc">判据约定的检验时间到了——这条判断该有个结论。</p>
          <ReviewCard
            v-for="c in store.overview.due"
            :key="c.thesisId"
            :card="c"
            :busy="store.marking"
            @mark="(action, p) => onMark(c, action, p)"
            @edit-falsifier="s => openEditor(c, s)"
          />
        </section>

        <section v-if="store.overview.health.length" class="rv-zone">
          <h3 class="rv-zone-title">
            结构有偏<span class="rv-count">{{ store.overview.health.length }}</span>
          </h3>
          <p class="rv-zone-desc">
            不是"你错了"，是这条判断的证据结构有问题——一边倒、同源、或没跟上变化。
          </p>
          <ReviewCard
            v-for="c in store.overview.health"
            :key="c.thesisId"
            :card="c"
            :busy="store.marking"
            @mark="(action, p) => onMark(c, action, p)"
            @edit-falsifier="s => openEditor(c, s)"
          />
        </section>

        <section v-if="store.overview.missingFalsifier.length" class="rv-zone">
          <h3 class="rv-zone-title">
            缺判据<span class="rv-count">{{ store.overview.missingFalsifier.length }}</span>
          </h3>
          <p class="rv-zone-desc">
            没有判据的判断永远不会被证伪。补一句「什么情况下我认错」，比多找三条支持证据有用。
          </p>
          <ReviewCard
            v-for="c in store.overview.missingFalsifier"
            :key="c.thesisId"
            :card="c"
            :busy="store.marking"
            @mark="(action, p) => onMark(c, action, p)"
            @edit-falsifier="s => openEditor(c, s)"
          />
        </section>

        <p class="rv-idle">
          另有 <strong>{{ store.overview.idleCount }}</strong> 条本期无变化<template
            v-if="store.overview.handledCount > 0"
          >
            ，本次已处置 <strong>{{ store.overview.handledCount }}</strong> 条</template
          >。
        </p>

        <p v-if="store.overview.extractionDisabledEntities > 0" class="rv-note">
          有 {{ store.overview.extractionDisabledEntities }} 个实体已停止抽取事件，相关判断可能看不到新动态。
        </p>
      </template>
    </template>

    <!-- 判据编辑器：从卡片上的「补判据 / 改判据」进来，就地覆盖在页面底部。
         不做弹窗是因为复盘里可能连续补好几条，每补一次弹一个框会打断节奏。 -->
    <div v-if="editing" class="rv-editor">
      <h3 class="rv-editor-title">{{ editing.existing ? '改判据' : '补一条判据' }}</h3>
      <p class="rv-editor-sub">{{ editing.card.statement }}</p>
      <label class="rv-field">
        <span class="rv-field-label">什么情况下这条判断算错？</span>
        <textarea
          v-model="editing.statement"
          class="rv-textarea"
          rows="2"
          maxlength="400"
          placeholder="例如：如果下季度出货量低于 18GWh，说明需求判断错了"
        />
      </label>
      <div class="rv-field-row">
        <label class="rv-field">
          <span class="rv-field-label">怎么检验</span>
          <select v-model="editing.windowKind" class="rv-input">
            <option value="date">到某一天回看</option>
            <option value="cycle">每隔一段时间回看</option>
          </select>
        </label>
        <label v-if="editing.windowKind === 'date'" class="rv-field">
          <span class="rv-field-label">到期日</span>
          <input v-model="editing.dueAt" type="date" class="rv-input" />
        </label>
        <label v-else class="rv-field">
          <span class="rv-field-label">每几天</span>
          <input v-model.number="editing.everyNDays" type="number" min="1" class="rv-input" />
        </label>
      </div>
      <label class="rv-field">
        <span class="rv-field-label">理由（可选）</span>
        <input v-model="editing.reason" class="rv-input" placeholder="为什么现在补这条判据" />
      </label>
      <p v-if="editorError" class="rv-error">{{ editorError }}</p>
      <div class="rv-editor-actions">
        <button type="button" class="rv-btn rv-btn-primary" :disabled="store.marking" @click="saveEditor">
          保存判据
        </button>
        <button type="button" class="rv-btn" @click="editing = null">取消</button>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
/**
 * 复盘页
 *
 * # 职责
 * 四区渲染 + 动作派发 + 判据编辑器。所有 IPC 在 `stores/review.ts`，本组件不直接
 * `invoke`。
 *
 * # 加载时机
 * 本页由 `App.vue` 用 `v-if` 渲染（与 GraphView 同族），挂载即加载。
 * 离开再回来会重新加载，但 `thesis_review_start` 幂等（有进行中的会话就返回它），
 * 所以"按 Esc 出去看一眼再回来"不会开启第二次复盘。
 *
 * # 为什么"保持"不要求填理由
 * 复盘里最常见的处置是"看过了，判断没变"。要求填理由，用户会直接关页面。
 */
import { computed, onMounted, ref } from 'vue'
import { useReviewStore, type MarkAction, type ReviewCard as ReviewCardData } from '@/stores/review'
import { useUiStore } from '@/stores/ui'
import ReviewCard from './ReviewCard.vue'

const store = useReviewStore()
const uiStore = useUiStore()

/** 判据编辑器的草稿（null = 未打开） */
const editing = ref<{
  card: ReviewCardData
  existing: boolean
  statement: string
  windowKind: 'date' | 'cycle'
  dueAt: string
  everyNDays: number
  reason: string
} | null>(null)

/** 编辑器内的错误（与全局 error 分开：它属于这次输入，不该顶在页面顶部） */
const editorError = ref('')

/** 顶部副标题：上次复盘时间 / 会话状态 */
const subtitle = computed(() => {
  if (store.summary) return '本次复盘已结束'
  const last = store.overview?.lastReviewAt
  if (last) return `上次复盘 ${last.slice(0, 10)}`
  return '还没有复盘记录——这一轮就是第一次'
})

onMounted(() => {
  // 先 start 再 overview 的顺序在 store 里写死了，这里只管调用
  void store.load()
})

/**
 * 记录一次处置
 *
 * @param card - 目标卡片
 * @param action - 动作
 * @param payload - 动作参数
 */
async function onMark(
  card: ReviewCardData,
  action: MarkAction,
  payload: { confidence?: number; status?: string; reason?: string },
): Promise<void> {
  const session = store.overview?.session
  if (!session) return
  try {
    await store.mark({
      review_id: session.id,
      thesis_id: card.thesisId,
      action,
      confidence: payload.confidence ?? null,
      status: payload.status ?? null,
      reason: payload.reason ?? null,
    })
  } catch {
    // store 已把错误写进 error，页面顶部会显示
  }
}

/**
 * 打开判据编辑器
 *
 * @param card - 目标卡片
 * @param suggestion - 点候选句式时带入的草稿（空串 = 从零写）
 */
function openEditor(card: ReviewCardData, suggestion: string): void {
  editorError.value = ''
  editing.value = {
    card,
    existing: card.falsifier !== null,
    statement: suggestion || card.falsifier?.statement || '',
    windowKind: card.falsifier?.windowKind ?? 'cycle',
    dueAt: card.falsifier?.dueAt?.slice(0, 10) ?? '',
    everyNDays: card.falsifier?.everyNDays ?? 90,
    reason: '',
  }
}

/** 保存判据：走复盘处置（会同时记 item 与 revision），没有会话时直接保存 */
async function saveEditor(): Promise<void> {
  const draft = editing.value
  if (!draft) return
  editorError.value = ''
  if (!draft.statement.trim()) {
    editorError.value = '判据不能为空'
    return
  }
  if (draft.windowKind === 'date' && !draft.dueAt) {
    editorError.value = '请选择到期日'
    return
  }
  if (draft.windowKind === 'cycle' && !(draft.everyNDays > 0)) {
    editorError.value = '周期需要正整数天'
    return
  }

  const falsifier = {
    thesis_id: draft.card.thesisId,
    statement: draft.statement.trim(),
    window_kind: draft.windowKind,
    due_at: draft.windowKind === 'date' ? draft.dueAt : null,
    every_n_days: draft.windowKind === 'cycle' ? draft.everyNDays : null,
    reason: draft.reason.trim() || undefined,
  }

  try {
    const session = store.overview?.session
    if (session) {
      await store.mark({
        review_id: session.id,
        thesis_id: draft.card.thesisId,
        action: 'falsifier',
        falsifier,
        reason: draft.reason.trim() || '复盘时补判据',
      })
    } else {
      await store.saveFalsifier(falsifier)
    }
    editing.value = null
  } catch (e) {
    editorError.value = e instanceof Error ? e.message : String(e)
  }
}

/** 结束本次复盘 */
async function onFinish(): Promise<void> {
  editing.value = null
  await store.finish()
}

/** 摘要态点「再来一轮」：清掉摘要，重新开会话 */
async function onNewRound(): Promise<void> {
  store.dismissSummary()
  await store.load()
}

/** 回到文章栏 */
function backToArticles(): void {
  uiStore.setMainView('articles')
}
</script>

<style scoped>
.rv {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-5);
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}

.rv-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sp-3);
}
.rv-head-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.rv-title {
  margin: 0;
  font-size: var(--fs-lg);
  font-weight: 500;
  color: var(--text-primary);
}
.rv-sub {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
}
.rv-head-actions {
  flex-shrink: 0;
}

.rv-error {
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--danger-text);
  background: color-mix(in srgb, var(--danger) 8%, transparent);
  border-left: 2px solid var(--danger);
  border-radius: var(--r-sm);
  padding: 6px 10px;
}
.rv-loading {
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

.rv-zone {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.rv-zone-title {
  margin: 0;
  font-size: var(--fs-md);
  font-weight: 500;
  color: var(--text-primary);
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.rv-count {
  font-size: var(--fs-xs);
  font-weight: 400;
  color: var(--text-secondary);
  background: var(--fill);
  border-radius: var(--r-sm);
  padding: 1px 6px;
}
.rv-zone-desc {
  margin: 0 0 var(--sp-1);
  font-size: var(--fs-xs);
  line-height: 1.6;
  color: var(--text-secondary);
  max-width: 34em;
}

.rv-idle,
.rv-note {
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--text-secondary);
}
.rv-note {
  opacity: 0.85;
}

.rv-quiet {
  padding: var(--sp-5) 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.rv-quiet-main {
  margin: 0;
  font-size: var(--fs-md);
  color: var(--text-primary);
}
.rv-quiet-sub {
  margin: 0;
  font-size: var(--fs-sm);
  line-height: 1.7;
  color: var(--text-secondary);
  max-width: 34em;
}

.rv-summary {
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-4);
  background: var(--surface);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  max-width: 34em;
}
.rv-summary-text {
  margin: 0;
  font-size: var(--fs-md);
  line-height: 1.7;
  color: var(--text-primary);
}
.rv-summary-meta {
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--text-secondary);
}
.rv-summary-actions {
  display: flex;
  gap: var(--sp-2);
}

.rv-btn {
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-secondary);
  border-radius: var(--r-sm);
  font: inherit;
  font-size: var(--fs-xs);
  padding: 6px 12px;
  cursor: pointer;
  min-height: 28px;
}
.rv-btn:hover:not(:disabled) {
  border-color: var(--primary);
  color: var(--primary);
}
.rv-btn:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.rv-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.rv-btn-primary {
  background: var(--primary);
  border-color: var(--primary);
  color: #fff;
}
.rv-btn-primary:hover:not(:disabled) {
  color: #fff;
  filter: brightness(1.06);
}

.rv-editor {
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-3);
  background: var(--surface);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  max-width: 40em;
}
.rv-editor-title {
  margin: 0;
  font-size: var(--fs-md);
  font-weight: 500;
  color: var(--text-primary);
}
.rv-editor-sub {
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  line-height: 1.6;
}
.rv-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.rv-field-label {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
}
.rv-field-row {
  display: flex;
  gap: var(--sp-3);
  flex-wrap: wrap;
}
.rv-textarea,
.rv-input {
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--bg, var(--surface));
  color: var(--text-primary);
  font: inherit;
  font-size: var(--fs-sm);
  padding: 6px 8px;
  line-height: 1.6;
}
.rv-textarea {
  resize: vertical;
  min-height: 56px;
}
.rv-textarea:focus-visible,
.rv-input:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -1px;
}
.rv-editor-actions {
  display: flex;
  gap: var(--sp-2);
}
</style>
