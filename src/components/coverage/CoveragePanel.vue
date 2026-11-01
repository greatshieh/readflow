<template>
  <!-- 覆盖清单：回答"我盯哪些标的、为什么盯、什么条件下不盯了"。
       与上面的实体列表是同一批数据的两副面孔——实体列表管"系统要看谁"，
       覆盖清单管"我为什么看它"。所以做成视图切换而不是第二个页签：
       两者共用一个入口，用户不会在"实体"和"覆盖"之间迷路。 -->
  <div class="cv">
    <header class="cv-head">
      <div class="cv-head-row">
        <h3 class="cv-title">覆盖清单</h3>
        <span v-if="store.rows.length > 0" class="cv-count">{{ store.rows.length }} 个标的</span>
        <span class="cv-spacer"></span>
        <button
          class="btn-primary btn-sm"
          type="button"
          :disabled="availableOptions.length === 0"
          :title="availableOptions.length === 0 ? '所有实体都已在覆盖清单里' : '把标的加入覆盖'"
          @click="toggleAdd"
        >
          {{ adding ? '取消' : '+ 加入覆盖' }}
        </button>
      </div>
      <p class="cv-tip">
        这张表的用处不是"列清单"，是事后能回答<b>上次把一只移出覆盖，是在什么条件下决定的</b>。
        没有退出条件的覆盖，等于把"盯到什么时候"交给心情。
      </p>
    </header>

    <!-- 加入表单：只让选还没在覆盖里的实体，避免"加了两遍"这种只能靠后端唯一约束兜住的事 -->
    <div v-if="adding" class="cv-add">
      <div class="cv-add-row">
        <AppSelect v-model="newEntityId" :options="availableOptions" placeholder="选择标的" />
        <AppSelect v-model="newTier" :options="TIER_OPTIONS" width="120px" />
        <button class="btn-primary btn-sm" type="button" :disabled="newEntityId === ''" @click="submitAdd">
          加入
        </button>
      </div>
      <p class="cv-add-hint">加入后可以补「为什么盯」与「退出条件」——空着也能用，但清单会退化成一份名单。</p>
    </div>

    <div v-if="store.error" class="cv-error">{{ store.error }}</div>
    <div v-if="store.loading && store.rows.length === 0" class="cv-empty">加载中…</div>

    <div v-else-if="store.rows.length === 0" class="cv-empty">
      <p class="cv-empty-title">还没有覆盖清单</p>
      <p class="cv-empty-desc">
        覆盖清单记的是"我为什么盯它、什么条件下不盯了"。先从下面已有的实体里挑几个核心的加进来，
        退出条件可以先空着——空着比编一个更诚实。
      </p>
    </div>

    <ul v-else class="cv-list">
      <li v-for="r in store.rows" :key="r.entityId" class="cv-item">
        <div class="cv-row">
          <span class="cv-tier" :class="`t-${r.tier}`">{{ tierLabel(r.tier) }}</span>
          <span class="cv-name">{{ r.entityName }}</span>
          <span class="cv-meta">{{ metaText(r) }}</span>
          <span class="cv-spacer"></span>
          <button class="btn-default btn-sm" type="button" @click="startEdit(r)">
            {{ editingId === r.entityId ? '收起' : '编辑' }}
          </button>
          <button class="btn-default btn-sm" type="button" @click="openDossier(r.entityId)">
            生成一页
          </button>
          <button class="btn-default btn-sm cv-danger" type="button" @click="removeRow(r)">移出</button>
        </div>

        <p v-if="r.reason" class="cv-line"><b>为什么盯</b>{{ r.reason }}</p>
        <!-- 没写退出条件要显式说出来，而不是留空白：
             空白看起来像"还没填"，而这条恰恰是最该填的一条 -->
        <p v-if="r.exitCondition" class="cv-line"><b>退出条件</b>{{ r.exitCondition }}</p>
        <p v-else class="cv-line warn"><b>退出条件</b>还没写——这条覆盖没有退出条件</p>
        <p v-if="r.positionNote" class="cv-line"><b>产业链位置</b>{{ r.positionNote }}</p>

        <div v-if="editingId === r.entityId" class="cv-edit">
          <div class="cv-edit-grid">
            <label class="cv-field">
              <span>档位</span>
              <AppSelect v-model="draftTier" :options="TIER_OPTIONS" />
            </label>
            <label class="cv-field">
              <span>为什么盯（一句话）</span>
              <input v-model="draftReason" class="form-input" maxlength="200" placeholder="例：储能出货拐点，产能先于价格" />
            </label>
            <label class="cv-field">
              <span>退出条件（什么情况下不盯了）</span>
              <input
                v-model="draftExit"
                class="form-input"
                maxlength="300"
                placeholder="例：连续两个季度出货低于 80GWh，或价格战导致毛利转负"
              />
            </label>
            <label class="cv-field">
              <span>产业链位置（手写，不自动推断）</span>
              <input v-model="draftPosition" class="form-input" maxlength="100" placeholder="例：电池环节，上游锂矿、下游整车" />
            </label>
          </div>
          <div class="cv-edit-actions">
            <span v-if="saveError" class="cv-edit-error">{{ saveError }}</span>
            <span class="cv-spacer"></span>
            <button class="btn-default btn-sm" type="button" @click="editingId = null">取消</button>
            <button class="btn-primary btn-sm" type="button" :disabled="saving" @click="submitEdit(r)">
              {{ saving ? '保存中…' : '保存' }}
            </button>
          </div>
        </div>
      </li>
    </ul>

    <DossierModal
      :open="dossierOpen"
      :dossier="dossier"
      :loading="dossierLoading"
      :error="dossierError"
      @close="dossierOpen = false"
    />
  </div>
</template>

<script setup lang="ts">
/**
 * 覆盖清单面板（研究工作台 · 实体页签的「覆盖」视图）
 *
 * # 职责
 * 列出覆盖条目、就地编辑（档位 / 为什么盯 / 退出条件 / 产业链位置）、
 * 移出覆盖、触发「生成一页」。
 *
 * # 为什么编辑是内联的而不是弹窗
 * 编辑覆盖最常做的事是"补一句为什么盯"。为四个短字段开一个弹窗，
 * 用户要付出"点开、填、保存、关掉"四步，而同屏已有全部上下文
 * （这条覆盖的论点数、待验判据数）。这与复盘卡片就地展开是同一个判断。
 *
 * # 退出条件为什么不与判据表合并
 * 判据挂在**判断**上（"什么情况下这条判断不成立"），退出条件挂在**标的**上
 * （"什么条件下我不再盯它"）。层级不同，但形式一样，所以这里只在
 * `metaText` 里把"待验判据"数摆出来，让"退出条件往往就是判据里的一条"可见，
 * 而不去伪造一层结构（详见 `research/coverage.rs` 模块注释）。
 */
import { computed, onMounted, ref } from 'vue'
import AppSelect, { type SelectOption } from '@/components/AppSelect.vue'
import DossierModal from './DossierModal.vue'
import { useCoverageStore, type CoverageRow, type CoverageTier, type Dossier } from '@/stores/coverage'
import { useEntitiesStore } from '@/stores/entities'
import { useToastStore } from '@/stores/toast'

const store = useCoverageStore()
const entitiesStore = useEntitiesStore()
const toast = useToastStore()

/** 档位选项（顺序即清单排序：核心 → 观察 → 本月退出） */
const TIER_OPTIONS: SelectOption[] = [
  { value: 'core', label: '核心' },
  { value: 'watch', label: '观察' },
  { value: 'exit', label: '本月退出' },
]

/** 档位中文名 */
function tierLabel(tier: CoverageTier): string {
  return TIER_OPTIONS.find(o => o.value === tier)?.label ?? tier
}

/** 加入表单开关 */
const adding = ref(false)
/** 加入表单：选中的标的（空串 = 未选） */
const newEntityId = ref<string | number>('')
/** 加入表单：档位 */
const newTier = ref<string | number>('watch')

/** 正在内联编辑的标的 ID（null = 全部收起，一次只编辑一条） */
const editingId = ref<number | null>(null)
/** 编辑草稿：四个字段 */
const draftTier = ref<string | number>('watch')
const draftReason = ref('')
const draftExit = ref('')
const draftPosition = ref('')
/** 保存中 */
const saving = ref(false)
/** 保存失败原因（就地显示，不打断编辑） */
const saveError = ref('')

/** 「研究一页」弹窗状态 */
const dossierOpen = ref(false)
const dossier = ref<Dossier | null>(null)
const dossierLoading = ref(false)
const dossierError = ref('')

/** 还没进覆盖的实体，供加入表单选择 */
const availableOptions = computed<SelectOption[]>(() => {
  const covered = new Set(store.rows.map(r => r.entityId))
  return entitiesStore.entities
    .filter(e => !covered.has(e.id))
    .map(e => ({ value: e.id, label: e.name }))
})

/**
 * 一行右侧的计数摘要
 *
 * 四个数回答"这条覆盖还活着吗"：有多少判断、有多少事在等结果、
 * 它最近有没有动静、上次被复盘是哪天。只有档位与理由的清单是静态名单。
 */
function metaText(r: CoverageRow): string {
  const parts = [`判断 ${r.thesisCount}`]
  if (r.falsifierOpen > 0) parts.push(`待验判据 ${r.falsifierOpen}`)
  if (r.eventCount30d > 0) parts.push(`30 天事件 ${r.eventCount30d}`)
  parts.push(r.lastReviewAt ? `上次复盘 ${r.lastReviewAt.slice(5, 10)}` : '还没复盘过')
  return parts.join(' · ')
}

/** 打开/收起加入表单 */
function toggleAdd() {
  adding.value = !adding.value
  newEntityId.value = ''
  newTier.value = 'watch'
}

/**
 * 提交「加入覆盖」
 *
 * 只写档位，理由与退出条件留空由用户随后补：强制必填会让加入这一步变重，
 * 而"先加进来再想清楚"是正常的思考顺序。
 */
async function submitAdd() {
  const entityId = Number(newEntityId.value)
  if (!Number.isFinite(entityId) || entityId <= 0) return
  try {
    await store.save({
      entity_id: entityId,
      tier: String(newTier.value) as CoverageTier,
      reason: '',
      exit_condition: '',
      position_note: '',
    })
    adding.value = false
    // 加完直接展开编辑：加进来之后的下一件事必然是写"为什么盯"
    startEdit(store.rows.find(r => r.entityId === entityId))
  } catch (e) {
    toast.show(`加入失败：${e instanceof Error ? e.message : String(e)}`)
  }
}

/**
 * 展开某行的内联编辑并把当前值填进草稿
 *
 * @param row - 覆盖行；不存在时什么都不做（加入后可能因筛选而取不到）
 */
function startEdit(row: CoverageRow | undefined) {
  if (!row) return
  if (editingId.value === row.entityId) {
    editingId.value = null
    return
  }
  editingId.value = row.entityId
  draftTier.value = row.tier
  draftReason.value = row.reason
  draftExit.value = row.exitCondition
  draftPosition.value = row.positionNote
  saveError.value = ''
}

/**
 * 保存编辑
 *
 * @param row - 覆盖行（取其 entityId）
 */
async function submitEdit(row: CoverageRow) {
  saving.value = true
  saveError.value = ''
  try {
    await store.save({
      entity_id: row.entityId,
      tier: String(draftTier.value) as CoverageTier,
      reason: draftReason.value.trim(),
      exit_condition: draftExit.value.trim(),
      position_note: draftPosition.value.trim(),
    })
    editingId.value = null
  } catch (e) {
    // 就地显示：这几个字段有长度上限，错误必须在填的地方出现，
    // 弹一条 toast 会消失，用户回头不知道是哪一项超了
    saveError.value = e instanceof Error ? e.message : String(e)
  } finally {
    saving.value = false
  }
}

/**
 * 移出覆盖
 *
 * 不做二次确认：移出只删覆盖行，实体、论点、证据全部保留，且变更史里留了痕。
 * 为一个可撤销、无损的动作加确认框，等于训练用户无条件点"确定"。
 */
async function removeRow(row: CoverageRow) {
  try {
    await store.remove(row.entityId)
    if (editingId.value === row.entityId) editingId.value = null
    toast.show(`已把「${row.entityName}」移出覆盖（论点与证据都还在）`)
  } catch (e) {
    toast.show(`移出失败：${e instanceof Error ? e.message : String(e)}`)
  }
}

/**
 * 生成「研究一页」并打开预览
 *
 * @param entityId - 标的 ID
 */
async function openDossier(entityId: number) {
  dossierOpen.value = true
  dossierLoading.value = true
  dossierError.value = ''
  dossier.value = null
  try {
    dossier.value = await store.dossier(entityId)
  } catch (e) {
    dossierError.value = e instanceof Error ? e.message : String(e)
  } finally {
    dossierLoading.value = false
  }
}

// 覆盖清单与实体列表是两套数据：外壳保证实体已加载，覆盖要自己拉
onMounted(() => {
  void store.load().catch(() => {
    /* 错误已写入 store.error，模板会显示；这里不再 toast，避免与外壳提示重复 */
  })
})
</script>

<style scoped>
.cv {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  padding: var(--sp-4) var(--sp-5);
  /* 列表长了要能滚：页签容器本身是滚动容器，这里不另开一层，
     只在内容超出时让父级滚（父级 .m-body 已有 overflow-y:auto） */
  min-height: 0;
}

.cv-head-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.cv-title {
  margin: 0;
  font-size: var(--fs-modal-section);
  font-weight: 500;
  color: var(--text-primary);
  flex-shrink: 0;
}
.cv-count {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  flex-shrink: 0;
  white-space: nowrap;
}
/* 行内弹性占位：把按钮组推到右侧。用 flex:1 1 auto + min-width:0 而不是
   margin-left:auto，后者在内容溢出时不会收缩，会把右侧按钮顶出容器 */
.cv-spacer {
  flex: 1 1 auto;
  min-width: 0;
}
.cv-tip {
  margin: var(--sp-2) 0 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  line-height: 1.5;
}

.cv-add {
  padding: var(--sp-3);
  border: 1px solid var(--glass-hair);
  border-radius: var(--r-md);
  background: var(--fill);
}
.cv-add-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.cv-add-row > :first-child {
  /* 名称列吃掉剩余宽度；两个下拉不抢空间 */
  flex: 1 1 auto;
  min-width: 0;
}
.cv-add-hint {
  margin: var(--sp-2) 0 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}

.cv-error {
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--r-sm);
  background: color-mix(in srgb, var(--danger) 10%, transparent);
  color: var(--danger);
  font-size: var(--fs-modal-label);
}
.cv-empty {
  padding: var(--sp-6) var(--sp-2);
  text-align: center;
}
.cv-empty-title {
  margin: 0 0 var(--sp-2);
  font-size: var(--fs-modal-body);
  color: var(--text-primary);
}
.cv-empty-desc {
  margin: 0 auto;
  max-width: 34em;
  font-size: var(--fs-modal-label);
  color: var(--text-tertiary);
  line-height: 1.6;
}

.cv-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.cv-item {
  padding: var(--sp-3);
  border: 1px solid var(--glass-hair);
  border-radius: var(--r-md);
  background: var(--fill);
}
.cv-row {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  /* 小屏时按钮组换行而不是被挤成两字一行 */
  flex-wrap: wrap;
}
.cv-tier {
  flex-shrink: 0;
  padding: var(--sp-025) var(--sp-2);
  border-radius: var(--r-sm);
  font-size: var(--fs-modal-meta);
  white-space: nowrap;
}
/* 三档语义色：核心=主色、观察=中性、本月退出=警示。
   用色区分档位而不是只靠文字，是为了让"这一列里哪些是核心"一眼可数 */
.cv-tier.t-core {
  background: color-mix(in srgb, var(--primary) 16%, transparent);
  color: var(--primary);
}
.cv-tier.t-watch {
  background: var(--hover-bg);
  color: var(--text-secondary);
}
.cv-tier.t-exit {
  background: color-mix(in srgb, var(--warning) 16%, transparent);
  color: var(--warning);
}
.cv-name {
  flex-shrink: 0;
  font-size: var(--fs-modal-body);
  font-weight: 500;
  color: var(--text-primary);
}
.cv-meta {
  flex-shrink: 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  white-space: nowrap;
}
.cv-danger {
  color: var(--danger);
}

.cv-line {
  margin: var(--sp-2) 0 0;
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
  line-height: 1.6;
}
.cv-line b {
  display: inline-block;
  min-width: 4.5em;
  margin-right: var(--sp-2);
  font-weight: 500;
  color: var(--text-tertiary);
}
.cv-line.warn {
  color: var(--warning);
}
.cv-line.warn b {
  color: var(--warning);
}

.cv-edit {
  margin-top: var(--sp-3);
  padding-top: var(--sp-3);
  border-top: 1px dashed var(--glass-hair);
}
.cv-edit-grid {
  display: grid;
  grid-template-columns: 120px 1fr;
  gap: var(--sp-2) var(--sp-3);
  align-items: center;
}
.cv-field {
  display: contents;
}
.cv-field > span {
  font-size: var(--fs-modal-label);
  color: var(--text-tertiary);
}
.cv-edit-actions {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-top: var(--sp-3);
}
.cv-edit-error {
  font-size: var(--fs-modal-label);
  color: var(--danger);
}
</style>
