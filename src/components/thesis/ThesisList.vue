<template>
  <!-- 论点列表：判断的台账。
       排序由后端定（观察中在前、按最近动作倒序），这里不重排——
       列表顺序一旦在渲染期可重排，用户就再也记不住"我上次看到它在哪"。 -->
  <div class="tl">
    <header class="tl-head">
      <div class="tl-title-row">
        <h2>论点</h2>
        <span v-if="store.stats.challenged > 0" class="tl-alert" :title="`${store.stats.challenged} 个观察中的论点已经有反驳证据`">
          {{ store.stats.challenged }} 个有待处理的反证
        </span>
      </div>
      <div class="tl-filters">
        <button
          v-for="f in STATUS_FILTERS"
          :key="f.value"
          type="button"
          class="btn btn-mini btn-round chip"
          :class="{ 'btn-soft': store.filterStatus === f.value }"
          :aria-pressed="store.filterStatus === f.value"
          @click="setFilter(f.value)"
        >
          {{ f.label }}
        </button>
        <span class="spacer"></span>
        <!-- 复盘入口放在论点列表，不放在设置里：用户会想到"回头看一眼"的时刻，
             正是他盯着这张表的时候。复盘是独立主屏视图（不在弹窗里），
             故这里只抛意图，由外壳负责关弹窗 + 切视图。

             这两个按钮一律走全局 .btn 族（基类 + 修饰类 + 尺寸档），
             不在本文件另写一套——本组件曾经自带一份 .btn-primary/.btn-ghost，
             结果「本周复盘」用的 .btn-default 落进了没有基类的空档，
             退化成浏览器原生控件（方角、系统内边距），而「+ 新论点」命中的
             私有定义又把 --primary-tint 当文字色，紫底上的紫字几乎看不见。 -->
        <button
          class="btn btn-default btn-sm"
          type="button"
          title="按 W 随时进入复盘"
          @click="emit('goto-review')"
        >
          本周复盘
        </button>
        <button
          class="btn btn-primary btn-sm"
          type="button"
          :disabled="entities.entities.length === 0"
          @click="openCreate"
        >
          + 新论点
        </button>
      </div>
      <!-- 没有实体就没法立论：标的悬空的论点无法被事件供血，只会退化成普通笔记。
           这里直接把原因写出来，而不是给个禁用按钮让用户猜。 -->
      <p v-if="entities.entities.length === 0" class="tl-hint">
        还没有跟踪任何实体。论点必须挂在一个实体上——先去「实体管理」加一个，或从周期报告的「本期值得跟进」里点「跟踪」。
      </p>
    </header>

    <div v-if="store.error" class="tl-error">{{ store.error }}</div>

    <div v-if="store.loading && store.list.length === 0" class="tl-empty">加载中…</div>

    <div v-else-if="store.list.length === 0" class="tl-empty">
      <p class="tl-empty-title">还没有论点</p>
      <p class="tl-empty-desc">
        论点是「你对某个标的的一句话判断」。写下来之后，读文章、看事件时都能随手把证据挂上去，
        支持的和反驳的分开记——判断就不容易越收越自信。
      </p>
    </div>

    <ul v-else class="tl-list">
      <li
        v-for="t in store.list"
        :key="t.id"
        class="tl-item"
        role="button"
        tabindex="0"
        :aria-label="`打开论点：${t.statement}`"
        @click="store.openThesis(t.id)"
        @keydown="onItemKey($event, t.id)"
      >
        <div class="tl-item-top">
          <span class="st" :class="STATUS_META[t.status].tone">{{ STATUS_META[t.status].label }}</span>
          <span class="ent">{{ t.entityName }}</span>
          <span class="dots" :title="`置信 ${t.confidence}/5`">
            <i v-for="i in 5" :key="i" :class="{ fill: i <= t.confidence }"></i>
          </span>
        </div>
        <div class="tl-item-stmt">{{ t.statement }}</div>
        <div class="tl-item-foot">
          <!-- 反驳计数放前面：这是"该看一下"的最强信号 -->
          <span v-if="t.opposeCount > 0" class="cnt bad">反驳 {{ t.opposeCount }}</span>
          <span v-if="t.mixedCount > 0" class="cnt mixed">存疑 {{ t.mixedCount }}</span>
          <span v-if="t.supportCount > 0" class="cnt ok">支持 {{ t.supportCount }}</span>
          <span v-if="t.supportCount + t.opposeCount + t.mixedCount === 0" class="cnt none">还没有证据</span>
          <span class="spacer"></span>
          <span class="when">{{ t.updatedAt }}</span>
        </div>
      </li>
    </ul>

    <!-- 新建论点弹层（就地，不跳页） -->
    <BaseModal :open="createOpen" title="新论点" size="md" @close="createOpen = false">
      <div class="cf">
        <!-- 一句说明：列表空态里那段话这里也要有。
             打开弹窗的人正在做"立论"这个动作，此刻最需要知道的是
             "为什么要挂实体"与"置信度拿来干什么"，而不是三个空控件。 -->
        <p class="cf-lede">
          一句能被证据推翻的判断。挂上标的后，读到的文章才能挂成支持或反驳。
        </p>

        <div class="cf-field">
          <!-- 计数器与标签同一行：单独占一行时，它紧贴下一个字段，
               视觉上会被读成"标的实体"的附属物 -->
          <div class="cf-label-row">
            <span class="cf-label">一句话判断</span>
            <span class="cf-count" :class="{ near: form.statement.length > 360 }">
              {{ form.statement.length }} / 400
            </span>
          </div>
          <textarea
            v-model="form.statement"
            class="cf-input"
            rows="3"
            maxlength="400"
            placeholder="如：宁德时代 2027 年储能出货会超预期"
          ></textarea>
        </div>

        <div class="cf-field">
          <span class="cf-label">标的实体</span>
          <AppSelect
            v-model="form.entityId"
            :options="entityOptions"
            placeholder="选一个实体"
          />
          <!-- 没有实体时把原因写出来，而不是只留一个禁用按钮让用户猜 -->
          <p v-if="entities.entities.length === 0" class="cf-note">
            还没有跟踪任何实体。论点必须挂在一个标的上——先去「实体管理」加一个。
          </p>
        </div>

        <div class="cf-field">
          <span class="cf-label">初始置信</span>
          <div class="cf-conf">
            <button
              v-for="i in 5"
              :key="i"
              type="button"
              class="btn btn-icon btn-mini btn-round cf-conf-dot"
              :class="{ on: i <= form.confidence }"
              :aria-label="`${i} 分 · ${CONFIDENCE_HINT[i]}`"
              :aria-pressed="i === form.confidence"
              @click="form.confidence = i"
            >
              {{ i }}
            </button>
            <!-- 光有刻度没有语义，用户不知道 3 分意味着什么。
                 文案只在当前档位给一句，不做成 5 行说明 -->
            <span class="cf-conf-text">{{ CONFIDENCE_HINT[form.confidence] }}</span>
          </div>
        </div>

        <p v-if="createError" class="cf-error">{{ createError }}</p>
        <div class="cf-foot">
          <button class="btn btn-default" type="button" @click="createOpen = false">取消</button>
          <button
            class="btn btn-primary"
            type="button"
            :disabled="!canCreate || creating"
            @click="submitCreate"
          >
            {{ creating ? '创建中…' : '创建论点' }}
          </button>
        </div>
      </div>
    </BaseModal>
  </div>
</template>

<script setup lang="ts">
/**
 * 论点列表
 *
 * # 职责
 * 台账首页：筛选、列表、新建。点击条目进入详情（由父组件 `ThesisPanel` 切换）。
 *
 * # 为什么新建用弹窗而不是内联表单
 * "一句话判断"需要认真想，内联表单会诱导用户随手敲几个字就存。
 * 弹窗 + 实体必选 + 置信度选择，是把"立论"当成一个需要停顿的动作。
 */
import { computed, onMounted, reactive, ref } from 'vue'
import BaseModal from '@/components/BaseModal.vue'
import AppSelect from '@/components/AppSelect.vue'
import { useEntitiesStore } from '@/stores/entities'
import { useThesesStore, STATUS_META, type ThesisStatus } from '@/stores/theses'

const store = useThesesStore()
const entities = useEntitiesStore()

/**
 * 向外交互
 *
 * `goto-review` 只表达"用户想复盘"，不在这里直接改 `uiStore.mainView`：
 * 列表在弹窗里，切视图之前必须先把弹窗关掉，而"关弹窗"是外壳的能力。
 * 子组件越权改全局视图会留下一个"模态开着但主屏已经换了"的中间态。
 */
const emit = defineEmits<{
  (e: 'goto-review'): void
}>()

/** 状态筛选档位。`activeOnly` 是默认：归档的与已推翻的平时不用占视线 */
const STATUS_FILTERS: Array<{ value: ThesisStatus | 'all' | 'activeOnly'; label: string }> = [
  { value: 'activeOnly', label: '在跟踪' },
  { value: 'confirmed', label: '已验证' },
  { value: 'refuted', label: '已推翻' },
  { value: 'archived', label: '已归档' },
  { value: 'all', label: '全部' },
]

/**
 * 置信度档位的人话
 *
 * 光给 5 个刻度没有语义：用户不知道该填几分，也不知道 3 分意味着什么。
 * 只在当前档位显示一句，不做 5 行说明——立论时不该先读一篇帮助。
 */
const CONFIDENCE_HINT: Record<number, string> = {
  1: '只是直觉',
  2: '有零星信号',
  3: '有依据，但可能错',
  4: '把握较大',
  5: '很确定',
}

const createOpen = ref(false)
const creating = ref(false)
const createError = ref('')

const form = reactive({
  statement: '',
  /** 空串 = 未选（AppSelect 的 v-model 只接受 string | number，不吃 null） */
  entityId: '' as string | number,
  confidence: 3,
})

/** 实体下拉选项 */
const entityOptions = computed(() =>
  entities.entities.map((e) => ({ value: e.id, label: e.name })),
)

const canCreate = computed(() => form.statement.trim().length > 0 && form.entityId !== '')

/**
 * 切换状态筛选
 *
 * @param v - 目标筛选值
 */
function setFilter(v: ThesisStatus | 'all' | 'activeOnly') {
  store.filterStatus = v
  void store.loadList()
}

/** 打开新建弹层并重置表单 */
function openCreate() {
  form.statement = ''
  form.entityId = entities.entities[0]?.id ?? ''
  form.confidence = 3
  createError.value = ''
  createOpen.value = true
}

/**
 * 列表项的键盘激活
 *
 * @param e - 键盘事件
 * @param id - 论点 ID
 */
function onItemKey(e: KeyboardEvent, id: number) {
  if (e.key !== 'Enter' && e.key !== ' ') return
  e.preventDefault()
  void store.openThesis(id)
}

/** 提交新建 */
async function submitCreate() {
  if (!canCreate.value || form.entityId === '') return
  creating.value = true
  createError.value = ''
  try {
    const id = await store.create({
      statement: form.statement.trim(),
      // AppSelect 的 v-model 类型是 string | number，实体 ID 一定是数字 → 显式收敛
      entityId: Number(form.entityId),
      confidence: form.confidence,
    })
    createOpen.value = false
    // 建完直接进详情：用户下一步通常就是挂第一条证据
    await store.openThesis(id)
  } catch (e) {
    createError.value = e instanceof Error ? e.message : String(e)
  } finally {
    creating.value = false
  }
}

onMounted(() => {
  if (entities.entities.length === 0) void entities.loadEntities()
  void store.loadList()
})
</script>

<style scoped>
.tl {
  display: flex;
  flex-direction: column;
  min-height: 0;
  flex: 1;
  gap: var(--sp-3);
  padding: var(--sp-4) var(--sp-5);
  overflow-y: auto;
  overscroll-behavior: contain;
}
.tl-head {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  flex-shrink: 0;
}
.tl-title-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.tl-title-row h2 {
  margin: 0;
  font-size: var(--fs-modal-section);
  font-weight: 600;
  color: var(--text-primary);
}
.tl-alert {
  padding: 1px 8px;
  border-radius: 999px;
  font-size: var(--fs-modal-meta);
  font-weight: 500;
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger-text);
  border: 1px solid color-mix(in srgb, var(--danger) 35%, transparent);
}
.tl-filters {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  flex-wrap: wrap;
}
.spacer {
  flex: 1 1 auto;
  min-width: 0;
}
/* 状态筛选 chip：外观来自 `.btn .btn-mini .btn-round`，选中态用 `.btn-soft`。
   此前这里与 `ThesisDetail.vue` 各持一份 26px 药丸定义，且这份漏了焦点环。 */
.tl-hint {
  margin: 0;
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-md);
  background: var(--fill);
  font-size: var(--fs-modal-meta);
  line-height: 1.6;
  color: var(--text-secondary);
}
.tl-error {
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-md);
  background: color-mix(in srgb, var(--danger) 8%, transparent);
  color: var(--danger-text);
  font-size: var(--fs-modal-label);
}
.tl-empty {
  padding: var(--sp-8) var(--sp-4);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--fs-modal-label);
}
.tl-empty-title {
  margin: 0 0 var(--sp-2);
  font-size: var(--fs-modal-body);
  font-weight: 600;
  color: var(--text-secondary);
}
.tl-empty-desc {
  margin: 0 auto;
  max-width: 34em;
  line-height: var(--lh-modal-body);
}
.tl-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
}
.tl-item {
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  background: var(--surface);
  cursor: pointer;
  transition:
    border-color var(--m-state, 160ms) ease-out,
    background var(--m-state, 160ms) ease-out;
}
.tl-item:hover {
  border-color: color-mix(in srgb, var(--primary) 40%, var(--border));
  background: color-mix(in srgb, var(--primary) 3%, var(--surface));
}
.tl-item:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.tl-item-top {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: 4px;
}
.st {
  flex-shrink: 0;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: var(--fs-modal-meta);
  font-weight: 500;
}
.st.active {
  background: var(--primary-soft);
  color: var(--primary-text);
}
.st.ok {
  background: color-mix(in srgb, var(--success) 14%, transparent);
  color: var(--success-text);
}
.st.bad {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger-text);
}
.st.muted {
  background: var(--fill);
  color: var(--text-tertiary);
}
.ent {
  min-width: 0;
  flex: 1 1 auto;
  font-size: var(--fs-modal-meta);
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 置信度用五个小格而不是数字：一眼看出高低，不用读文字 */
.dots {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}
.dots i {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--border);
}
.dots i.fill {
  background: var(--primary);
}
.tl-item-stmt {
  font-size: var(--fs-modal-body);
  font-weight: 600;
  line-height: 1.5;
  color: var(--text-primary);
  margin-bottom: 5px;
}
.tl-item-foot {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  font-size: var(--fs-modal-meta);
}
.cnt {
  flex-shrink: 0;
}
.cnt.bad {
  color: var(--danger-text);
}
.cnt.mixed {
  color: var(--warning-text);
}
.cnt.ok {
  color: var(--success-text);
}
.cnt.none {
  color: var(--text-tertiary);
}
.when {
  flex-shrink: 0;
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

/* ── 新建表单 ─────────────────────────────────────────────────────────
   这一块的要点是间距的**分配方向**：组内紧、组间松。
   旧版是反的（标签↔控件 6px、字段之间 12px），三个字段连成一片，
   看起来像"三个控件堆在一起"而不是"三件事"。 */
.cf {
  display: flex;
  flex-direction: column;
  gap: var(--sp-6);
}
.cf-lede {
  margin: 0;
  font-size: var(--fs-modal-meta);
  line-height: 1.6;
  color: var(--text-tertiary);
}
.cf-field {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  position: relative;
}
.cf-label-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--sp-2);
}
.cf-label {
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
}
.cf-input {
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
.cf-input:focus {
  outline: none;
  border-color: var(--primary);
}
/* 计数与标签同行右对齐；接近上限时转警示色，而不是等提交才报错 */
.cf-count {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}
.cf-count.near {
  color: var(--warning-text);
}
/* 字段级说明（如"还没有实体"）：与错误分开——它是引导，不是失败 */
.cf-note {
  margin: 0;
  font-size: var(--fs-modal-meta);
  line-height: 1.6;
  color: var(--warning-text);
}
.cf-conf {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
}
/* 刻度里写数字：只有 5 个圆点时，用户既不知道点到了几，也不知道一共几档。
   外观来自 `.btn .btn-icon .btn-mini .btn-round`（24px 圆形图标钮）；
   这里只留数字所需的等宽数字排版。 */
.cf-conf-dot {
  font-variant-numeric: tabular-nums;
}
/* 实心主色块上的文字一律用 --tone-fg，不要用 --primary-tint（那是底色令牌）。
   这是全按钮族里唯一保留"选中即实心"的地方：1–5 的置信度是一组刻度，
   选中的那一格必须是一枚实心棋子，浅底在和其它四颗未选中棋子并列时读不出"选定"。 */
.cf-conf-dot.on {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--tone-fg);
}
.cf-conf-text {
  margin-left: var(--sp-2);
  font-size: var(--fs-modal-meta);
  color: var(--text-secondary);
}
.cf-error {
  margin: 0;
  font-size: var(--fs-modal-label);
  color: var(--danger-text);
}
/* 按钮区与内容之间要有一条线：否则整块白到底，取消/创建看起来像第 4 个字段 */
.cf-foot {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-2);
  padding-top: var(--sp-4);
  border-top: 1px solid var(--border);
}
</style>
