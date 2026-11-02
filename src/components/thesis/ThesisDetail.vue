<template>
  <!-- 论点详情：整套方案的核心界面。
       布局的最重要一条：**反驳分区在最上面**，且完整展开。
       这是刻意反直觉的——所有研究工具都让人方便地收藏"支持我的"，
       结果是论点越收集越自信而不是越准确。把反面证据顶到第一眼，
       是让"我有没有漏掉反证"成为打开论点的第一个动作。 -->
  <!-- v-if 兜底：详情加载失败时 store.detail 会被置空（openThesis 的 catch），
       若无此判断，下面的 `t.xxx` 全会在渲染期炸掉。
       加载中给一行安静的文字，不给整页骨架——论点详情打开通常只需一帧。 -->
  <div v-if="!detail" class="td td-loading">{{ store.error || '加载中…' }}</div>
  <div v-else class="td">
    <header class="td-head">
      <div class="td-crumb">
        <button class="link" type="button" @click="emit('back')">← 论点列表</button>
        <span class="spacer"></span>
        <button class="btn btn-danger-text btn-sm" type="button" @click="confirmDelete = true">删除</button>
      </div>

      <div class="td-title-row">
        <span class="td-ent">{{ t.entityName }}</span>
        <span class="st" :class="STATUS_META[t.status].tone">{{ STATUS_META[t.status].label }}</span>
      </div>
      <h1 ref="titleRef" class="td-stmt" tabindex="-1">{{ t.statement }}</h1>

      <div class="td-meta">
        <span class="td-conf">
          置信
          <button
            v-for="i in 5"
            :key="i"
            type="button"
            class="dot"
            :class="{ on: i <= t.confidence }"
            :aria-label="`置信 ${i} 分`"
            :aria-pressed="i === t.confidence"
            @click="setConfidence(i)"
          ></button>
        </span>
        <span class="dot-sep">·</span>
        <span>建于 {{ t.createdAt }}</span>
      </div>

      <!-- 状态操作：状态变更会留痕，所以放在显眼处但不做成大按钮，
           避免误点。已推翻/已验证是可逆的，随时可以改回观察中。 -->
      <div class="td-actions">
        <button
          v-for="s in STATUS_ACTIONS"
          :key="s.value"
          type="button"
          class="chip"
          :class="{ on: t.status === s.value }"
          :aria-pressed="t.status === s.value"
          @click="setStatus(s.value)"
        >
          {{ s.label }}
        </button>
        <span class="spacer"></span>
        <button class="btn btn-default btn-sm" type="button" @click="startEdit">改判断</button>
      </div>

      <!-- 改判断：单独一个小表单，不挤占详情阅读区 -->
      <div v-if="editing" class="td-edit">
        <textarea v-model="editText" class="td-edit-input" rows="3" maxlength="400"></textarea>
        <input
          v-model="editReason"
          class="td-edit-reason"
          type="text"
          maxlength="120"
          placeholder="为什么改？（可留空，但半年后你会想看到它）"
        />
        <div class="td-edit-foot">
          <button class="btn btn-default btn-sm" type="button" @click="editing = false">取消</button>
          <button class="btn btn-primary btn-sm" type="button" :disabled="!editText.trim()" @click="saveEdit">保存</button>
        </div>
      </div>
    </header>

    <!-- 变更理由：点了置信点或状态 chip 后才出现。
         放在头部之下、正文之上：它是个"打断一下"的确认条，
         不该滚进正文里被忽略。 -->
    <div v-if="pendingChange" class="td-pending">
      <span class="td-pending-what">
        {{ pendingChange.kind === 'status' ? '改状态' : `置信改为 ${pendingChange.value} 分` }}
      </span>
      <input
        v-model="pendingReason"
        class="td-pending-input"
        type="text"
        maxlength="120"
        placeholder="为什么？（可留空，但半年后你会想看到它）"
        @keydown.enter.prevent="commitPending"
        @keydown.esc.prevent="cancelPending"
      />
      <button class="btn btn-default btn-sm" type="button" @click="cancelPending">取消</button>
      <button class="btn btn-primary btn-sm" type="button" @click="commitPending">确认</button>
    </div>

    <!-- 引导：第一次打开论点时的解释。
         反驳置顶是反直觉设计，不解释会被当成 bug 反馈。 -->
    <p v-if="!guideDismissed && evidence.length > 0" class="td-guide">
      反面证据放在了最上面，这是故意的——收集支持自己的材料很容易，主动看反面才难。
      <button class="td-guide-x" type="button" aria-label="知道了" @click="dismissGuide">知道了</button>
    </p>

    <div class="td-body">
      <!-- 来源分歧：这一行回答"这条判断是不是只有一家在说"。
           放在证据分区之前——先让用户知道证据的独立性，再看证据本身，
           否则读到 5 条支持证据时会先形成一个"证据很足"的印象，
           再看那句"只有 1 家来源"就已经晚了。 -->
      <DivergenceBadge :thesis-id="t.id" />
      <!-- ═══ 证据分区：反驳 → 存疑 → 支持 ═══
           `shouldShow` 的取舍：完全还没有证据时，只保留「反驳」这一节的空态
           （它本身就是那句"这不是好消息，而是还没找"，是引导不是占位）；
           存疑/支持的空态一律折叠——否则一条证据都没有时会顶着三段空话，
           让人以为界面坏了。有任意证据后，三节全部展开，空态才重新有意义。 -->
      <section v-for="sec in sections" v-show="shouldShow(sec)" :key="sec.stance" class="td-sec">
        <div class="td-sec-head">
          <h2 :class="sec.tone">{{ sec.title }}</h2>
          <span class="td-sec-n">{{ sec.items.length }}</span>
        </div>

        <p v-if="sec.items.length === 0" class="td-sec-empty">{{ sec.empty }}</p>

        <ul v-else class="td-ev-list">
          <li v-for="ev in sec.items" :key="ev.id" class="td-ev">
            <div class="td-ev-body">
              <!-- 快照是唯一长期可靠的内容：源文章可能已被保留策略清理 -->
              <div class="td-ev-text">{{ ev.snapshotText }}</div>
              <div v-if="ev.note" class="td-ev-note">我的备注：{{ ev.note }}</div>
              <div class="td-ev-foot">
                <span class="td-ev-kind">{{ KIND_LABEL[ev.kind] }}</span>
                <template v-if="ev.articleId !== null">
                  <span class="dot-sep">·</span>
                  <button class="td-ev-open" type="button" @click="emit('open-article', ev.articleId)">
                    {{ ev.articleTitle || '打开原文' }}
                  </button>
                </template>
                <template v-else>
                  <span class="dot-sep">·</span>
                  <span class="td-ev-gone">原文已清理（证据靠快照保留）</span>
                </template>
                <span class="spacer"></span>
                <button class="td-ev-x" type="button" aria-label="摘掉这条证据" title="摘掉" @click="removeEvidence(ev.id)">
                  ✕
                </button>
              </div>
            </div>
          </li>
        </ul>
      </section>

      <!-- ═══ 变更记录：回答"我当初为什么这么判断" ═══ -->
      <section class="td-sec">
        <div class="td-sec-head">
          <h2>判断变更</h2>
          <span class="td-sec-n">{{ revisions.length }}</span>
        </div>
        <p v-if="revisions.length === 0" class="td-sec-empty">还没有变更过。</p>
        <ul v-else class="td-rev-list">
          <li v-for="r in revisions" :key="r.id" class="td-rev">
            <span class="td-rev-when">{{ r.createdAt }}</span>
            <span class="td-rev-what">{{ describeRevision(r) }}</span>
            <span v-if="r.reason" class="td-rev-reason">理由：{{ r.reason }}</span>
          </li>
        </ul>
      </section>

      <!-- ═══ 时间线供血（P5）：该实体最近事件，一键挂载 ═══ -->
      <section class="td-sec" v-if="visibleTimeline.length > 0">
        <div class="td-sec-head">
          <h2>该实体最近事件</h2>
          <span class="td-sec-n">{{ visibleTimeline.length }}</span>
        </div>
        <p class="td-sec-tip">还没挂到论点的，可以直接选立场挂上去。</p>
        <ul class="td-tl-list">
          <li v-for="ev in visibleTimeline" :key="ev.id" class="td-tl">
            <div class="td-tl-main">
              <span class="td-tl-date">{{ ev.eventDate || '—' }}</span>
              <span class="td-tl-type">{{ ev.eventType }}</span>
              <span class="td-tl-fact">{{ ev.fact }}</span>
            </div>
            <div class="td-tl-acts">
              <button class="mini" type="button" title="作为支持证据" @click="attachTimeline(ev.id, 'support')">支持</button>
              <button class="mini bad" type="button" title="作为反驳证据" @click="attachTimeline(ev.id, 'oppose')">反驳</button>
              <button class="mini" type="button" title="作为存疑证据" @click="attachTimeline(ev.id, 'mixed')">存疑</button>
            </div>
          </li>
        </ul>
      </section>
    </div>

    <!-- 删除确认：删除连带证据一起没，必须明确说 -->
    <BaseModal :open="confirmDelete" title="删除论点" size="sm" @close="confirmDelete = false">
      <p class="cf-text">
        将删除「{{ t.statement }}」以及它的 {{ evidence.length }} 条证据和 {{ revisions.length }} 条变更记录。
        这个动作不能撤销。
      </p>
      <div class="cf-foot">
        <button class="btn btn-default btn-sm" type="button" @click="confirmDelete = false">取消</button>
        <button class="btn-danger" type="button" @click="doDelete">删除</button>
      </div>
    </BaseModal>
  </div>
</template>

<script setup lang="ts">
/**
 * 论点详情
 *
 * # 职责
 * 展示一个论点的全貌：判断本体、证据四分区、变更记录、实体最近事件（供血）。
 *
 * # 三个刻意的设计
 * 1. **反驳置顶**：对抗确认偏误（见模板顶部注释）。首次打开时给一次引导。
 * 2. **证据用快照渲染，不用实时内容**：文章有保留策略、事件是级联删除，
 *    实时内容会随着时间蒸发。快照是"三个月后还能读"的唯一保证。
 * 3. **状态与置信度改动都留痕**：半年后回看，最重要的是"我什么时候改的主意、为什么"。
 */
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import BaseModal from '@/components/BaseModal.vue'
import DivergenceBadge from './DivergenceBadge.vue'
import {
  useThesesStore,
  KIND_LABEL,
  STATUS_META,
  type EvidenceStance,
  type Revision,
  type ThesisStatus,
} from '@/stores/theses'

const props = defineProps<{
  /** 论点 ID */
  thesisId: number
}>()

const emit = defineEmits<{
  (e: 'back'): void
  /** 请求打开原文（交给外壳跳主屏阅读区） */
  (e: 'open-article', articleId: number): void
  /** 论点被删除 */
  (e: 'deleted'): void
}>()

const store = useThesesStore()

/** 详情（null 时组件不该被渲染，但类型上要守一下） */
const detail = computed(() => store.detail)
const t = computed(() => detail.value!.thesis)
const evidence = computed(() => detail.value?.evidence ?? [])
const revisions = computed(() => detail.value?.revisions ?? [])

const titleRef = ref<HTMLElement | null>(null)
const editing = ref(false)
const editText = ref('')
/** 「改判断」时的理由（可空） */
const editReason = ref('')
/**
 * 待确认的置信/状态变更及其理由
 *
 * 为什么不是"直接改完再说"：置信度和状态的变化是论点台账最有价值的历史，
 * 事后补理由没人会补。改完立刻给一个输入框（可跳过），是唯一能让人真写下来的时机。
 */
const pendingChange = ref<{ kind: 'confidence' | 'status'; value: number | ThesisStatus } | null>(null)
const pendingReason = ref('')
const confirmDelete = ref(false)
/** 引导条是否已关（本次会话内关掉就不再出现，不做持久化：
 *  它只需要说一次"这是故意的"，不需要长期占位） */
const guideDismissed = ref(false)

/** 状态操作档位 */
const STATUS_ACTIONS: Array<{ value: ThesisStatus; label: string }> = [
  { value: 'active', label: '观察中' },
  { value: 'confirmed', label: '已验证' },
  { value: 'refuted', label: '已推翻' },
  { value: 'archived', label: '归档' },
]

/** 该实体最近事件（供血） */
interface TimelineEvent {
  id: number
  eventType: string
  fact: string
  eventDate: string
}
/** 供血时间线只看最近 60 天：太老的事件更可能已经把结论"打进去了"，
 *  再挂它只是凑数，反而稀释证据质量 */
const TIMELINE_DAYS = 60
/** 一次最多取 20 条：供血区是"顺手补充"的通道，不能长到压过证据区 */
const TIMELINE_LIMIT = 20
const timeline = ref<TimelineEvent[]>([])

/** 时间线里已经挂到本论点的来源 refId 集合，用来隐藏已挂条目（避免重复操作） */
const attachedRefIds = computed(() => new Set(evidence.value.filter((e) => e.kind === 'event').map((e) => e.refId)))

/** 已被挂载过的事件不再出现在时间线里 */
const visibleTimeline = computed(() => timeline.value.filter((e) => !attachedRefIds.value.has(e.id)))

/** 三档证据分区（顺序 = 显示顺序 = 认知优先级） */
const sections = computed(() => {
  const mk = (stance: EvidenceStance, title: string, tone: string, empty: string) => ({
    stance,
    title,
    tone,
    empty,
    items: evidence.value.filter((e) => e.stance === stance),
  })
  return [
    mk('oppose', '反驳', 'bad', '还没有反面证据。这不是好消息，而是还没找——试一段时间线里的事件，或者主动搜一次反方观点。'),
    mk('mixed', '存疑', 'mixed', '没有存疑的证据。'),
    mk('support', '支持', 'ok', '还没有支持证据。'),
  ]
})

/**
 * 某证据分区是否要渲染
 *
 * 规则见模板注释：空证据时只留「反驳」那一节的引导文案。
 *
 * @param sec - 分区（来自 `sections`）
 * @returns 是否渲染该分区
 */
function shouldShow(sec: { stance: EvidenceStance; items: unknown[] }): boolean {
  if (sec.items.length > 0) return true
  if (evidence.value.length > 0) return true
  return sec.stance === 'oppose'
}

/**
 * 变更记录的可读描述
 *
 * @param r - 变更行
 * @returns 一句话
 */
function describeRevision(r: Revision): string {
  if (r.field === 'confidence') return `置信度 ${r.fromValue} → ${r.toValue}`
  if (r.field === 'status') {
    const f = STATUS_META[r.fromValue as ThesisStatus]?.label ?? r.fromValue
    const to = STATUS_META[r.toValue as ThesisStatus]?.label ?? r.toValue
    return r.fromValue === '' ? `建立为「${to}」` : `状态 ${f} → ${to}`
  }
  return '判断文本已修改'
}

/** 关掉引导条 */
function dismissGuide() {
  guideDismissed.value = true
}

/** 进入"改判断"态 */
function startEdit() {
  editText.value = t.value.statement
  editReason.value = ''
  editing.value = true
}

/** 保存判断文本 */
async function saveEdit() {
  const text = editText.value.trim()
  if (!text) return
  const reason = editReason.value.trim() || '修改判断'
  await store.update(props.thesisId, { statement: text, reason })
  editing.value = false
  editReason.value = ''
}

/**
 * 改状态
 *
 * @param s - 目标状态
 */
function setStatus(s: ThesisStatus) {
  if (s === t.value.status) return
  // 不立刻写库：先问一句"为什么"，见 pendingChange 的注释
  pendingChange.value = { kind: 'status', value: s }
  pendingReason.value = ''
}

/**
 * 改置信度
 *
 * @param v - 1–5
 */
function setConfidence(v: number) {
  if (v === t.value.confidence) return
  pendingChange.value = { kind: 'confidence', value: v }
  pendingReason.value = ''
}

/** 放弃这次变更（不写库） */
function cancelPending() {
  pendingChange.value = null
  pendingReason.value = ''
}

/**
 * 提交待确认的变更（理由可空）
 *
 * 理由留空也放行：强制填写只会让人随便敲两个字，反而污染变更记录。
 */
async function commitPending() {
  const p = pendingChange.value
  if (!p) return
  const reason = pendingReason.value.trim()
  pendingChange.value = null
  if (p.kind === 'status') {
    await store.update(props.thesisId, { status: p.value as ThesisStatus, reason })
  } else {
    await store.update(props.thesisId, { confidence: p.value as number, reason })
  }
  pendingReason.value = ''
}

/**
 * 摘掉一条证据
 *
 * @param id - 证据行 ID
 */
async function removeEvidence(id: number) {
  await store.removeEvidence(id)
}

/**
 * 把时间线上的一条事件挂成证据
 *
 * @param refId - 事件 ID
 * @param stance - 立场
 */
async function attachTimeline(refId: number, stance: EvidenceStance) {
  await store.addEvidence({
    thesisId: props.thesisId,
    kind: 'event',
    refId,
    stance,
  })
}

/** 执行删除 */
async function doDelete() {
  await store.remove(props.thesisId)
  confirmDelete.value = false
  emit('deleted')
}

/**
 * 拉该实体的事件时间线
 *
 * 用现有 `research_events_timeline` 命令（按实体筛选），不新增后端接口。
 */
async function loadTimeline() {
  try {
    // 后端签名是 (days, entity_id, event_type, sentiment, keyword)，五个参数都要传全：
    // days 必填，其余传 null 表示不筛。命令返回 EventWithContext（无 serde rename），
    // 字段是 snake_case，这里逐字段收敛成前端 camelCase。
    const rows = await invoke<Array<Record<string, unknown>>>('research_events_timeline', {
      days: TIMELINE_DAYS,
      entityId: t.value.entityId,
      eventType: null,
      sentiment: null,
      keyword: null,
    })
    timeline.value = rows
      .map((r) => ({
        id: Number(r.id),
        eventType: String(r.event_type ?? ''),
        fact: String(r.fact ?? ''),
        eventDate: String(r.event_date ?? ''),
      }))
      // 后端按时间倒序返回；这里只保留最近 N 条，够"供血"且不让列表压过证据区
      .slice(0, TIMELINE_LIMIT)
  } catch {
    // 时间线只是"供血"，取不到不影响论点本身可读 → 静默降级
    timeline.value = []
  }
}

/** 组装与加载 */
async function bootstrap() {
  await store.openThesis(props.thesisId)
  await loadTimeline()
  await nextTick()
  titleRef.value?.focus()
}

watch(() => props.thesisId, () => void bootstrap(), { immediate: true })
onMounted(() => {
  void bootstrap()
})
</script>

<style scoped>
/* 变更理由条：一行动作，不换行，宽度收敛在正文行宽内 */
.td-pending {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin: 0 var(--sp-5) var(--sp-15);
  padding: var(--sp-15) var(--sp-3);
  background: var(--primary-tint);
  border-radius: var(--r-md);
  flex-wrap: wrap;
}
.td-pending-what {
  flex-shrink: 0;
  font-size: var(--fs-sm);
  font-weight: 500;
  color: var(--primary);
}
.td-pending-input {
  flex: 1 1 200px;
  min-width: 0;
  font-family: inherit;
  font-size: var(--fs-sm);
  padding: var(--sp-1) var(--sp-2);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-primary);
}
.td-pending-input:focus {
  outline: none;
  border-color: var(--primary);
}

.td-loading {
  padding: var(--sp-6) var(--sp-5);
  font-size: var(--fs-md);
  color: var(--text-tertiary);
}
.td {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
  gap: var(--sp-2);
  overflow-y: auto;
  overscroll-behavior: contain;
  padding: var(--sp-4) var(--sp-5) var(--sp-6);
}
.td-head {
  flex-shrink: 0;
}
.td-crumb {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-15);
}
.spacer {
  flex: 1 1 auto;
  min-width: 0;
}
.link {
  border: none;
  background: none;
  padding: 0;
  color: var(--primary);
  font-family: inherit;
  font-size: var(--fs-modal-label);
  cursor: pointer;
}
.link:hover {
  text-decoration: underline;
}
.title-row,
.td-title-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: 4px;
}
.td-ent {
  font-size: var(--fs-modal-label);
  font-weight: 500;
  color: var(--primary);
}
.st {
  padding: 1px 7px;
  border-radius: 999px;
  font-size: var(--fs-modal-meta);
  font-weight: 500;
}
.st.active {
  background: var(--primary-soft);
  color: var(--primary);
}
.st.ok {
  background: color-mix(in srgb, var(--success) 14%, transparent);
  color: var(--success);
}
.st.bad {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
}
.st.muted {
  background: var(--fill);
  color: var(--text-tertiary);
}
.td-stmt {
  margin: 0 0 var(--sp-2);
  font-size: var(--fs-modal-title);
  font-weight: 600;
  line-height: var(--lh-modal-title);
  color: var(--text-primary);
  outline: none;
  max-width: 34em;
}
.td-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  margin-bottom: var(--sp-3);
}
.td-conf {
  display: inline-flex;
  align-items: center;
  gap: 3px;
}
.dot {
  width: 8px;
  height: 8px;
  padding: 0;
  border-radius: 50%;
  border: 1px solid var(--border);
  background: var(--surface);
  cursor: pointer;
}
.dot.on {
  background: var(--primary);
  border-color: var(--primary);
}
.dot:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
.dot-sep {
  opacity: 0.6;
}
.td-actions {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  flex-wrap: wrap;
  margin-bottom: var(--sp-2);
}
.chip {
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--fs-modal-meta);
  cursor: pointer;
}
.chip:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.chip.on {
  background: var(--primary-soft);
  border-color: color-mix(in srgb, var(--primary) 45%, var(--border));
  color: var(--primary);
  font-weight: 500;
}
.chip:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.td-edit {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  margin-bottom: var(--sp-2);
}
.td-edit-input {
  width: 100%;
  padding: var(--sp-2);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  resize: vertical;
}
.td-edit-input:focus {
  outline: none;
  border-color: var(--primary);
}
.td-edit-reason {
  width: 100%;
  font-family: inherit;
  font-size: var(--fs-sm);
  padding: var(--sp-1) var(--sp-2);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-primary);
  margin-top: var(--sp-05);
}
.td-edit-reason:focus {
  outline: none;
  border-color: var(--primary);
}
.td-edit-foot {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-2);
}
.td-guide {
  display: flex;
  align-items: flex-start;
  gap: var(--sp-2);
  margin: 0 0 var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-md);
  border: 1px solid color-mix(in srgb, var(--primary) 28%, var(--border));
  background: var(--primary-soft);
  font-size: var(--fs-modal-meta);
  line-height: 1.6;
  color: var(--text-secondary);
}
.td-guide-x {
  flex-shrink: 0;
  margin-left: auto;
  border: none;
  background: none;
  color: var(--primary);
  font-family: inherit;
  font-size: var(--fs-modal-meta);
  cursor: pointer;
}
.td-body {
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
  max-width: 38em;
}
.td-sec {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.td-sec-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding-bottom: 2px;
  border-bottom: 1px solid var(--border);
}
.td-sec-head h2 {
  margin: 0;
  font-size: var(--fs-modal-label);
  font-weight: 600;
}
.td-sec-head h2.bad {
  color: var(--danger);
}
.td-sec-head h2.mixed {
  color: var(--warning);
}
.td-sec-head h2.ok {
  color: var(--success);
}
.td-sec-head h2 {
  color: var(--text-primary);
}
.td-sec-n {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
.td-sec-empty {
  margin: 0;
  font-size: var(--fs-modal-label);
  line-height: var(--lh-modal-body);
  color: var(--text-tertiary);
}
.td-sec-tip {
  margin: 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
.td-ev-list,
.td-rev-list,
.td-tl-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
}
.td-ev {
  padding: 9px 11px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
}
.td-ev-text {
  font-size: var(--fs-modal-body);
  line-height: 1.55;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
}
.td-ev-note {
  margin-top: 4px;
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
  font-style: italic;
}
.td-ev-foot {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: 6px;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  min-width: 0;
}
.td-ev-kind {
  flex-shrink: 0;
}
.td-ev-open {
  border: none;
  background: none;
  padding: 0;
  color: var(--primary);
  font-family: inherit;
  font-size: var(--fs-modal-meta);
  cursor: pointer;
  max-width: 22em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.td-ev-open:hover {
  text-decoration: underline;
}
.td-ev-gone {
  font-style: italic;
}
.td-ev-x {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  display: grid;
  place-items: center;
  border: none;
  background: none;
  border-radius: var(--r-sm);
  color: var(--text-tertiary);
  font-size: 11px;
  cursor: pointer;
  opacity: 0;
  transition: opacity var(--m-tap, 90ms) ease-out;
}
.td-ev:hover .td-ev-x,
.td-ev-x:focus-visible {
  opacity: 1;
}
.td-ev-x:hover {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
}
.td-rev {
  display: flex;
  align-items: baseline;
  gap: var(--sp-2);
  font-size: var(--fs-modal-meta);
  flex-wrap: wrap;
}
.td-rev-when {
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}
.td-rev-what {
  color: var(--text-secondary);
}
.td-rev-reason {
  color: var(--text-tertiary);
  font-style: italic;
}
.td-tl {
  display: flex;
  align-items: flex-start;
  gap: var(--sp-2);
  padding: 7px 9px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
}
.td-tl-main {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: 5px;
  flex-wrap: wrap;
}
.td-tl-date {
  flex-shrink: 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
.td-tl-type {
  flex-shrink: 0;
  padding: 0 5px;
  border-radius: 4px;
  background: var(--fill);
  font-size: var(--fs-modal-meta);
  color: var(--text-secondary);
}
.td-tl-fact {
  font-size: var(--fs-modal-label);
  line-height: 1.5;
  color: var(--text-primary);
  word-break: break-word;
}
.td-tl-acts {
  flex-shrink: 0;
  display: flex;
  gap: 2px;
}
.mini {
  height: 22px;
  padding: 0 7px;
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--fs-modal-meta);
  cursor: pointer;
}
.mini:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.mini.bad:hover {
  background: color-mix(in srgb, var(--danger) 10%, transparent);
  color: var(--danger);
}
.mini:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
/* 按钮一律走全局 `.btn` 族（基类 + 修饰类 + 尺寸档），本文件不再自建。
   这里曾有一份私有的 `.btn-ghost / .btn-primary / .btn-danger`：
   只有这个组件在用，且绕开了 `.btn` 基类，于是高度、内边距、圆角、字号全靠
   这几条临时声明，与弹窗底部那排按钮对不上。删掉，别再长回来。 */
.cf-text {
  margin: 0 0 var(--sp-3);
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--text-secondary);
}
.cf-foot {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-2);
}
</style>
