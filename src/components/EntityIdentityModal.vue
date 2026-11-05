<template>
  <!-- ═══ 实体身份：别名 + 合并 ═══
       两个操作放在同一个弹窗里，因为它们回答的是同一个问题：**"这一行到底是谁"**。
       别名是"还有哪些写法算我"，合并是"另起的那一行其实也是我"。
       分开做会让人以为别名只是标签；而实际上两者都直接改写归一化的匹配表。 -->
  <BaseModal
    :open="open"
    :title="`名称与合并 · ${entity?.name ?? ''}`"
    size="lg"
    @close="emit('close')"
  >
    <div class="eid-bar" v-if="entity">
      <span class="m-title">名称与合并</span>
    </div>

    <div class="eid" v-if="entity">
      <!-- ① 别名 -->
      <section class="eid-section">
        <h4>别名</h4>
        <p class="eid-desc">
          AI 抽取时，正文里出现这些写法都会记到「{{ entity.name }}」名下。
          系统会自动累积，这里可以手动增删——写错的别名会让别的实体的历史混进来。
        </p>

        <div class="eid-chips">
          <span v-for="alias in aliases" :key="alias" class="eid-chip">
            {{ alias }}
            <button
              class="eid-chip-x"
              type="button"
              :aria-label="`移除别名 ${alias}`"
              @click="removeAlias(alias)"
            >
              ✕
            </button>
          </span>
          <span v-if="!aliases.length" class="eid-empty">暂无别名</span>
        </div>

        <div class="eid-add-row">
          <input
            v-model="newAlias"
            type="text"
            class="eid-input"
            placeholder="添加别名（如 Google、谷歌）"
            @keyup.enter="addAlias"
          />
          <button class="btn btn-soft btn-form" @click="addAlias" :disabled="!newAlias.trim()">
            添加
          </button>
          <button class="btn btn-soft btn-form" @click="handleSuggest" :disabled="aiLoading">
            {{ aiLoading ? '识别中…' : 'AI 补全' }}
          </button>
        </div>

        <p v-if="hint" class="eid-hint">{{ hint }}</p>
        <div class="eid-actions">
          <button class="btn btn-primary btn-sm" @click="handleSaveAliases" :disabled="saving">
            {{ saving ? '保存中…' : '保存别名' }}
          </button>
        </div>
      </section>

      <!-- ② 合并 -->
      <section class="eid-section">
        <h4>并入其他实体</h4>
        <p class="eid-desc">
          如果这一行其实是另一个实体的重复条目，把它并过去：事件、论点、覆盖、市场标的、
          财务数据都会搬到对方名下，这一行的名字变成对方的别名，以后自动收敛。
          <strong>操作不可撤销。</strong>
        </p>

        <AppSelect
          v-model="targetId"
          :options="targetOptions"
          placeholder="选择要并入的实体"
          :disabled="!targetOptions.length"
        />

        <p v-if="targetId" class="eid-impact">
          <span class="eid-impact-row">
            保留「{{ target?.name }}」：{{ describeImpact(targetImpact) }}
          </span>
          <span class="eid-impact-row">
            并入「{{ entity.name }}」：{{ describeImpact(selfImpact) }}
          </span>
          <span v-if="coverageConflict" class="eid-warn">
            两边都有覆盖记录（为什么盯、何时退出），合并后只保留「{{ target?.name }}」那份。
          </span>
          <span class="eid-warn">
            同一篇文章里同类型、同事实的事件会被判为重复，保留「{{ target?.name }}」那条。
          </span>
        </p>

        <div class="eid-actions">
          <button
            class="btn btn-danger btn-sm"
            @click="handleMerge"
            :disabled="!targetId || merging"
          >
            {{ merging ? '合并中…' : '合并' }}
          </button>
        </div>
      </section>

      <p v-if="error" class="eid-error">{{ error }}</p>
    </div>
    <div v-else class="eid-empty-panel">未选择实体</div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 实体身份编辑弹窗：别名维护 + 并入其他实体
 *
 * # 为什么两件事在一个弹窗里
 * 别名与合并改的是同一张匹配表（`entities.aliases` + 实体行本身）。
 * 别名回答"还有哪些写法算我"，合并回答"另起的那一行其实也是我"。
 * 分成两个入口会让人误以为别名只是展示用的标签。
 *
 * # 自动 vs 手动
 * 别名平时由抽取链路自动累积（见 Rust `research::aliases`），
 * 这里只处理它认错的两种情况：写错的别名要能删，漏认的重复实体要能合。
 *
 * # 与外壳的契约
 * - 父组件传 `entity`（当前行）与 `open`；
 * - 合并方向固定为「当前实体 → 选中的目标实体」，目标是保留方。不做"保留哪个"
 *   的单选，因为从某一行点进来时，"这一行要去掉"本身已经表达了方向；
 * - 保存别名或合并成功后 emit `changed`（携带一句可展示的结果），由外壳负责
 *   重新评分与关系图失效——本组件不碰那两件事。
 */
import { computed, ref, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import { useEntitiesStore } from '@/stores/entities'
import { showConfirm } from '@/stores/modal'
import type { Entity, EntityImpact } from '@/types'

const props = defineProps<{
  /** 弹窗显隐 */
  open: boolean
  /** 当前实体（别名主体 / 并入方） */
  entity: Entity | null
  /** 预选的并入目标（来自「AI 查重」的建议，省掉用户再选一次） */
  suggestedTarget?: number | null
}>()

const emit = defineEmits<{
  /** 关闭弹窗（取消 / 保存完成 / 合并完成） */
  (e: 'close'): void
  /** 实体已变化：携带一句可展示的结果，外壳据此重新评分并作废关系图 */
  (e: 'changed', message: string): void
}>()

const entitiesStore = useEntitiesStore()

/** 编辑中的别名表（保存前只在本组件里生效） */
const aliases = ref<string[]>([])
/** 新别名输入 */
const newAlias = ref('')
/** 保存中 */
const saving = ref(false)
/** AI 补全请求中 */
const aiLoading = ref(false)
/** 合并进行中 */
const merging = ref(false)
/** 行内提示（AI 补全结果等中性信息） */
const hint = ref('')
/** 错误提示 */
const error = ref('')
/** 并入目标（保存实体 ID 的字符串形式，AppSelect 的值是 string | number） */
const targetId = ref('')
/** 目标实体的影响面 */
const targetImpact = ref<EntityImpact | null>(null)
/** 当前实体的影响面 */
const selfImpact = ref<EntityImpact | null>(null)

/** 并入目标实体对象 */
const target = computed(() => entitiesStore.entities.find(e => String(e.id) === targetId.value) ?? null)

/**
 * 可选的并入目标：除自己以外的全部实体
 *
 * 用「名字（类型）」而不是「名字」：库里同名的两家不同公司（同名不同类型）
 * 在这行下拉里必须能分辨，否则用户会照着自己想删的那行选错目标。
 */
const targetOptions = computed<SelectOption[]>(() =>
  entitiesStore.entities
    .filter(e => e.id !== props.entity?.id)
    .map(e => ({ value: String(e.id), label: `${e.name}（${e.entity_type}）` })),
)

/** 两边都有覆盖记录时必然冲突：`coverage.entity_id` 是主键，只能留一份 */
const coverageConflict = computed(
  () => (targetImpact.value?.coverage ?? 0) > 0 && (selfImpact.value?.coverage ?? 0) > 0,
)

// 打开时从实体行回填。关弹窗不重置是陷阱（下次打开会带着上次的编辑），
// 所以每次打开都按当前实体重建一遍。
watch(
  () => [props.open, props.entity?.id] as const,
  ([isOpen]) => {
    if (!isOpen) return
    aliases.value = parseAliases(props.entity?.aliases ?? '[]')
    newAlias.value = ''
    hint.value = ''
    error.value = ''
    // 预选目标：「AI 查重」点进来时直接落在建议的那一侧，仍可改
    targetId.value = props.suggestedTarget ? String(props.suggestedTarget) : ''
    targetImpact.value = null
    selfImpact.value = null
    // 预选时 targetId 的变化发生在下面那个 watcher 注册之前，不会触发它，
    // 所以这里必须自己拉一次，否则预选状态下影响面一直显示"查询失败"
    if (targetId.value) void loadImpacts()
  },
  { immediate: true },
)

// 选中目标后才查影响面：两个实体各一次，合起来就是"到底要搬多少东西"
watch(targetId, () => {
  void loadImpacts()
})

/**
 * 拉取两侧影响面（保留方 + 并入方）
 *
 * 查询失败置 `null` 而不是零：界面上"影响面查询失败，无法预估"比
 * "没有任何数据"诚实得多——后者会让用户以为合并不丢东西。
 */
async function loadImpacts() {
  targetImpact.value = null
  selfImpact.value = null
  if (!targetId.value || !props.entity) return
  const [targetData, selfData] = await Promise.all([
    entitiesStore.fetchImpact(Number(targetId.value)).catch(() => null),
    entitiesStore.fetchImpact(props.entity.id).catch(() => null),
  ])
  targetImpact.value = targetData
  selfImpact.value = selfData
}

/**
 * 解析别名 JSON
 *
 * 后端存的是 JSON 数组字符串；损坏时按空表处理（与 Rust 侧同一口径）。
 *
 * @param raw - 别名 JSON 字符串
 * @returns 别名列表
 */
function parseAliases(raw: string): string[] {
  try {
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((a): a is string => typeof a === 'string') : []
  } catch {
    return []
  }
}

/**
 * 把影响面说成一句人话
 *
 * 查不到时明确说"无法预估"，不能装作没有数据——那是让用户闭眼点确认。
 *
 * @param impact - 影响面；`null` 表示查询失败
 * @returns 可展示的一句话
 */
function describeImpact(impact: EntityImpact | null): string {
  if (!impact) return '影响面查询失败，无法预估'
  const parts: string[] = []
  if (impact.events) parts.push(`${impact.events} 条事件`)
  if (impact.theses) parts.push(`${impact.theses} 条论点`)
  if (impact.coverage) parts.push(`${impact.coverage} 条覆盖记录`)
  if (impact.market_symbols) parts.push(`${impact.market_symbols} 条市场标的`)
  if (impact.financial) parts.push(`${impact.financial} 条财务数据`)
  return parts.length ? parts.join('、') : '没有关联数据'
}

/**
 * 追加一个别名（大小写不敏感去重，与后端同一口径）
 */
function addAlias() {
  const value = newAlias.value.trim()
  if (!value) return
  if (aliases.value.some(a => a.toLowerCase() === value.toLowerCase())) {
    newAlias.value = ''
    return
  }
  aliases.value = [...aliases.value, value]
  newAlias.value = ''
  hint.value = '别名已加入列表，点「保存别名」才会生效'
}

/**
 * 从列表里移除一个别名（只改本地，保存后才落库）
 *
 * @param alias - 待移除的别名
 */
function removeAlias(alias: string) {
  aliases.value = aliases.value.filter(a => a !== alias)
  hint.value = '别名已移除，点「保存别名」才会生效'
}

/**
 * 让模型补全别名（只进列表，不落库）
 *
 * 提示"确认后保存"而不是自动保存：模型给的名字质量参差，
 * 自动落库等于让它直接改写匹配表。
 */
async function handleSuggest() {
  if (!props.entity) return
  aiLoading.value = true
  error.value = ''
  hint.value = ''
  try {
    const suggested = await entitiesStore.suggestAliases(props.entity.id)
    const merged = [...aliases.value]
    for (const item of suggested) {
      const value = item.trim()
      if (!value) continue
      if (merged.some(a => a.toLowerCase() === value.toLowerCase())) continue
      merged.push(value)
    }
    const added = merged.length - aliases.value.length
    aliases.value = merged
    hint.value = added > 0 ? `AI 补充了 ${added} 个写法，确认后点「保存别名」` : 'AI 没有找到新的写法'
  } catch (e) {
    error.value = `AI 补全失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    aiLoading.value = false
  }
}

/**
 * 保存别名表
 */
async function handleSaveAliases() {
  if (!props.entity) return
  saving.value = true
  error.value = ''
  try {
    const written = await entitiesStore.setAliases(props.entity.id, aliases.value)
    emit('changed', `已保存「${props.entity.name}」的别名（${written.length} 个）`)
    emit('close')
  } catch (e) {
    error.value = `保存失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    saving.value = false
  }
}

/**
 * 把当前实体并入选中的目标实体
 *
 * 顺序：先持久化别名（用户可能刚在上面加了没保存）→ 再确认 → 再合并。
 * 先存别名的原因是合并会把两边别名取并集；不先存，用户刚编的别名就白编了。
 */
async function handleMerge() {
  const current = props.entity
  const targetEntity = target.value
  if (!current || !targetEntity) return

  error.value = ''
  try {
    await entitiesStore.setAliases(current.id, aliases.value)
  } catch (e) {
    error.value = `别名保存失败，合并已取消：${e instanceof Error ? e.message : String(e)}`
    return
  }

  const lines = [`把「${current.name}」并入「${targetEntity.name}」？`]
  lines.push(`「${targetEntity.name}」保留：${describeImpact(targetImpact.value)}`)
  lines.push(`「${current.name}」并入：${describeImpact(selfImpact.value)}`)
  if (coverageConflict.value) {
    lines.push(`两边都有覆盖记录，合并后只保留「${targetEntity.name}」那份。`)
  }
  lines.push('并入方的名字会成为对方的别名，以后自动收敛。此操作不可撤销。')

  const ok = await showConfirm({ title: '合并实体', message: lines.join('\n') })
  if (!ok) return

  merging.value = true
  try {
    const report = await entitiesStore.mergeEntities(targetEntity.id, current.id)
    const parts: string[] = [`已把「${report.merged_name}」并入「${report.keep_name}」`]
    if (report.events_moved) parts.push(`搬走 ${report.events_moved} 条事件`)
    if (report.events_dropped) parts.push(`去重 ${report.events_dropped} 条事件`)
    if (report.theses_moved) parts.push(`搬走 ${report.theses_moved} 条论点`)
    if (report.symbols_moved) parts.push(`搬走 ${report.symbols_moved} 条市场标的`)
    if (report.financial_dropped) parts.push(`丢弃 ${report.financial_dropped} 条重复财务数据`)
    if (report.aliases_skipped) {
      parts.push(`${report.aliases_skipped} 个别名因被其他实体占用而跳过`)
    }
    emit('changed', parts.join('，'))
    emit('close')
  } catch (e) {
    error.value = `合并失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    merging.value = false
  }
}
</script>

<style scoped>
.eid-bar {
  display: flex;
  align-items: center;
  padding: var(--sp-35) var(--sp-4);
  border-bottom: 1px solid var(--border);
  flex: 0 0 auto;
}
.m-title {
  font-size: var(--fs-modal-title);
  font-weight: 600;
  line-height: var(--lh-modal-title);
  color: var(--text-primary);
}
.eid {
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
  padding: var(--sp-4);
  /* 面板是 max-height + overflow:hidden，自己并不滚：min-height:0 缺了
     flex 子项就不会收缩，overflow 永远不生效，超出的内容直接消失 */
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}
.eid-section {
  display: flex;
  flex-direction: column;
  gap: var(--sp-15);
  padding-bottom: var(--sp-3);
  border-bottom: 1px solid var(--border);
}
.eid-section:last-of-type {
  border-bottom: none;
  padding-bottom: 0;
}
.eid-section h4 {
  margin: 0;
  font-size: var(--fs-modal-section);
  font-weight: 600;
  line-height: var(--lh-modal-section);
  color: var(--text-primary);
}
.eid-desc {
  margin: 0;
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--text-secondary);
}
.eid-chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-1);
}
/* 别名是"已存在的值"而不是可点选项：淡底 + 描边，不带主色。
   带主色会读成"当前选中"，而这里没有选中态可言。 */
.eid-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-05);
  padding: 3px 4px 3px 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--fill);
  font-size: var(--fs-modal-label);
  line-height: var(--lh-modal-label);
  color: var(--text-secondary);
}
.eid-chip-x {
  width: 16px;
  height: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--text-tertiary);
  font-size: 10px;
  cursor: pointer;
  transition:
    background var(--m-tap) var(--m-ease-out),
    color var(--m-tap) var(--m-ease-out);
}
.eid-chip-x:hover {
  background: var(--danger-fg);
  color: var(--danger-text);
}
.eid-chip-x:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.eid-empty {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
.eid-add-row {
  display: flex;
  gap: var(--sp-2);
}
/* 与两个 `.btn-form`(34px) 同行，输入框必须同高，否则读成没对齐 */
.eid-input {
  flex: 1 1 auto;
  min-width: 0;
  height: 34px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-15);
  font-size: var(--fs-modal-body);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: border-color var(--m-tap) var(--m-ease-out);
}
.eid-input:focus {
  border-color: var(--primary);
}
.eid-input::placeholder {
  color: var(--text-tertiary);
}
.eid-add-row .btn {
  flex-shrink: 0;
}
.eid-impact {
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
  margin: 0;
  padding: var(--sp-2);
  border-radius: var(--r-md);
  background: var(--fill);
  font-size: var(--fs-modal-meta);
  line-height: var(--lh-modal-meta);
  color: var(--text-secondary);
}
.eid-impact-row {
  color: var(--text-secondary);
}
/* 冲突提示是"要丢东西"的警告，用危险色文字而不是底：面积小，靠色相就够 */
.eid-warn {
  color: var(--danger-text);
}
.eid-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-2);
}
.eid-hint {
  margin: 0;
  font-size: var(--fs-modal-meta);
  line-height: var(--lh-modal-meta);
  color: var(--text-tertiary);
}
.eid-error {
  margin: 0;
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--danger-text);
}
.eid-empty-panel {
  padding: var(--sp-4);
  font-size: var(--fs-modal-body);
  color: var(--text-tertiary);
}
</style>
