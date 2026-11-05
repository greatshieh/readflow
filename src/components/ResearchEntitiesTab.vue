<template>
  <!-- ═══ Tab：实体管理 ═══
       只做一件事：系统要盯谁（增删 / 启停 / 重评分 / 告警规则）。
       「覆盖」曾是本页签内的第二个视图，已拆成平级页签——两个任务是不同的心智
       （这里是**配置**，覆盖是**台账/决策记录**），理由见 `ResearchModal` 侧栏注释。 -->
  <div class="m-body">
    <!-- 实体列表 -->
    <div class="entity-list" v-if="entitiesStore.entities.length > 0">
      <div
        v-for="entity in entitiesStore.entities"
        :key="entity.id"
        class="entity-item"
        :class="{ 'is-disabled': !entity.enabled }"
      >
        <div
          class="entity-info clickable"
          role="button"
          title="查看实体仪表盘"
          @click="openDashboard(entity)"
        >
          <span class="entity-name">{{ entity.name }}</span>
          <!-- 类型与状态同占一行：`.entity-info` 是纵向两行（名字 / 元信息），
               如果「已禁用」再单独占一行，每一行会变成三行高，列表密度骤降 -->
          <span class="entity-meta">
            <span class="entity-type">{{ entityTypeLabel(entity.entity_type) }}</span>
            <!-- 禁用态必须给文字标记，不能只靠降色：降色在低对比与色觉差异下都读不出
                 "它为什么不参与评分"。列表改成含禁用项之后，这一行才第一次可见。 -->
            <span v-if="!entity.enabled" class="entity-off">已禁用</span>
          </span>
        </div>
        <div class="entity-actions">
          <!-- 启用/禁用开关：一个圆点，开=实心主色，关=空心灰。
               不用 ✓ / ○ 字形——它们是字体里的字符，字号、基线、粗细都不可控，
               而且和旁边三个 SVG 图标不是同一套语言。
               状态色由 `.enabled` 类承载，按钮外观仍是全局 `.btn .btn-icon .btn-round`。 -->
          <button
            class="btn btn-icon btn-round entity-switch"
            :class="{ enabled: entity.enabled }"
            :aria-pressed="entity.enabled"
            @click="handleToggle(entity)"
            :title="entity.enabled ? '已启用，点击禁用' : '已禁用，点击启用'"
            :aria-label="entity.enabled ? '禁用' : '启用'"
          >
            <span class="entity-switch-dot" aria-hidden="true"></span>
          </button>
          <!-- 删除按钮：`.btn-quiet-danger` = 幽灵底 + 悬停透出危险色 -->
          <button
            class="btn btn-icon btn-quiet btn-quiet-danger"
            @click="handleDelete(entity)"
            title="删除"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="3 6 5 6 21 6"></polyline>
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"></path>
            </svg>
          </button>
          <!-- 名称与合并：别名维护 + 把重复条目并走。
               与「删除」的区别是它不丢历史——合并只是搬家，删的是那一行。 -->
          <button
            class="btn btn-icon btn-quiet"
            @click="openIdentity(entity)"
            title="名称与合并"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M4 7h9a4 4 0 0 1 0 8H8"></path>
              <polyline points="11 12 8 15 11 18"></polyline>
              <path d="M20 17h-9"></path>
              <path d="M3 4v4M6 4v4"></path>
            </svg>
          </button>
          <!-- 告警规则按钮（实体级告警 P1-F） -->
          <button
            class="btn btn-icon btn-quiet"
            @click="openRuleModal(entity)"
            title="告警规则"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"></path>
              <path d="M13.73 21a2 2 0 0 1-3.46 0"></path>
            </svg>
          </button>
        </div>
      </div>
    </div>

    <!-- 空状态 -->
    <div v-else class="empty-state">
      <p>暂无关注的实体</p>
      <p class="hint">添加你关心的公司、人物或产品，系统会据此智能评分文章</p>
    </div>

    <!-- 添加新实体 -->
    <div class="add-entity">
      <div class="input-row">
        <input
          v-model="newName"
          type="text"
          placeholder="输入实体名称（如 腾讯）"
          class="entity-input"
          @keyup.enter="handleAdd"
        />
        <AppSelect
          v-model="newType"
          :options="entityTypeOptions"
          width="auto"
          class="type-select"
        />
        <button class="btn btn-primary btn-form" @click="handleAdd" :disabled="!newName.trim()">
          添加
        </button>
      </div>
      <div v-if="entitiesStore.error" class="error-msg">{{ entitiesStore.error }}</div>
    </div>

    <!-- 批量操作：评分结果与失败原因都落在按钮左侧的行内提示上 ——
         例行操作不该弹窗打断，失败也不该只写进控制台 -->
    <div class="batch-actions">
      <span v-if="scoreMsg" class="score-msg" :class="{ err: scoreError }">{{ scoreMsg }}</span>
      <div class="batch-buttons">
        <button class="btn btn-soft" @click="handleFindDuplicates" :disabled="dedupeLoading">
          {{ dedupeLoading ? '查重中...' : 'AI 查重' }}
        </button>
        <button class="btn btn-soft" @click="handleScore" :disabled="entitiesStore.loading">
          {{ entitiesStore.loading ? '评分中...' : '重新评分' }}
        </button>
      </div>
    </div>

    <!-- AI 查重结果：只出候选，合并要逐组点。
         不做"一键全并"——合并会搬七张表的引用、撞唯一约束的还会丢数据，
         让模型连点确认一起替人做掉是不负责任的。 -->
    <div v-if="duplicates.length" class="dup-box">
      <div class="dup-head">
        <span class="dup-title">可能重复的实体（{{ duplicates.length }} 组）</span>
        <button class="btn btn-quiet btn-sm" @click="duplicates = []">收起</button>
      </div>
      <p class="dup-hint">
        模型认为同一条目有多个写法。点右边的按钮把重复的那行并到第一个上，
        会先显示影响面再让你确认。
      </p>
      <div v-for="(group, gi) in duplicates" :key="gi" class="dup-group">
        <div class="dup-names">{{ group.names.join(' / ') }}</div>
        <p class="dup-reason">{{ group.reason }}</p>
        <div class="dup-actions">
          <button
            v-for="name in group.names.slice(1)"
            :key="name"
            class="btn btn-soft btn-mini"
            :disabled="group.entity_ids.length < 2"
            @click="mergeSuggestion(group.entity_ids[0], name)"
          >
            「{{ name }}」并到「{{ group.names[0] }}」
          </button>
        </div>
      </div>
    </div>

    <!-- 已删除（屏蔽）的实体：删除的"持久"部分就落在这里。
         用原生 <details>：summary 天然可聚焦、可键盘展开、读屏会播报展开状态，
         自己写一套折叠要额外补 aria-expanded 与键盘处理，还容易半套。 -->
    <details v-if="entitiesStore.blocked.length" class="blocked-box">
      <summary class="blocked-summary">
        已删除的实体（{{ entitiesStore.blocked.length }}）
      </summary>
      <p class="blocked-hint">
        这些名字不会被系统自动添加回来。点「恢复」只是解除拦截，
        并不会把实体重新建好——下次文章里再出现这个名字时才会重新建立。
      </p>
      <div
        v-for="item in entitiesStore.blocked"
        :key="item.name"
        class="blocked-item"
      >
        <span class="blocked-name">{{ item.name }}</span>
        <button class="btn btn-soft btn-sm" @click="handleUnblock(item)">恢复</button>
      </div>
    </details>

    <!-- 实体级告警规则编辑弹窗（P1-F） -->
    <EntityAlertRuleModal
      :open="ruleModalOpen"
      :entity="ruleEntity"
      @close="ruleModalOpen = false"
    />

    <!-- 名称与合并：别名维护 + 把重复条目并走 -->
    <EntityIdentityModal
      :open="identityOpen"
      :entity="identityEntity"
      :suggested-target="identityTarget"
      @close="identityOpen = false"
      @changed="handleIdentityChanged"
    />
  </div>
</template>

<script setup lang="ts">
/**
 * 研究工作台 · 实体管理页签
 *
 * 增删关注实体、启停与重新评分。评分是批量提取与研究报告的上游闸门
 * （提取的候选条件是 `entity_score >= 阈值`），因此增删/启停后都紧跟一次评分，
 * 让新状态立刻对存量文章生效（评分为纯本地字符串匹配，不消耗 AI 额度）。
 *
 * # 职责边界（不要再把覆盖塞回来）
 * 本页签只回答"系统要盯谁"。「覆盖」（为什么盯、何时退出）是**平级页签**，
 * 不在这里，也不再受 `entityView` 这类二级状态驱动——那套结构曾让同一个
 * 二选一在侧栏和页签内各出现一次，且两处都在宣称覆盖从属于实体。
 *
 * # 与外壳/其它页签的契约
 * - 实体列表的加载由外壳在弹窗打开时统一保证（时间线筛选下拉、覆盖页签共用）；
 * - 评分/增删/启停会让已加载的关系图过期：本页签只 emit `entities-changed`
 *   通知外壳，由外壳据当前主屏视图决定「立即重算」还是「作废待重算」
 *   （见 `ResearchModal.onEntitiesChanged`），省掉一次看不见的全库扫描。
 */
import { onMounted, ref } from 'vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import EntityAlertRuleModal from './EntityAlertRuleModal.vue'
import EntityIdentityModal from './EntityIdentityModal.vue'
import { useEntitiesStore } from '@/stores/entities'
import { showConfirm } from '@/stores/modal'
import { ENTITY_TYPE_LABELS, entityTypeLabel } from '@/utils/entityLabels'
import type { EntityBlock, EntityImpact, MergeCandidate } from '@/types'

const emit = defineEmits<{
  /** 实体集合或评分已变化，已加载的关系图随之过期（外壳转交失效处理） */
  (e: 'entities-changed'): void
}>()

const entitiesStore = useEntitiesStore()

/** 新实体的名称输入 */
const newName = ref('')
/** 新实体的类型选择 */
const newType = ref('company')

/** 评分提示：成功条数 / 无待评分文章 / 失败原因 */
const scoreMsg = ref('')

/** 评分提示是否为错误态 */
const scoreError = ref(false)

/** 告警规则弹窗开关 */
const ruleModalOpen = ref(false)
/** 当前正在编辑规则的实体（弹窗关闭前保持引用） */
const ruleEntity = ref<(typeof entitiesStore.entities[0]) | null>(null)

/** 「名称与合并」弹窗开关 */
const identityOpen = ref(false)
/** 正在编辑身份的实体（别名主体 / 并入方） */
const identityEntity = ref<(typeof entitiesStore.entities[0]) | null>(null)
/** 预选的并入目标：从 AI 查重结果点进来时直接填好，省掉再选一次 */
const identityTarget = ref<number | null>(null)

/** AI 查重结果（只出候选，合并要逐组确认） */
const duplicates = ref<MergeCandidate[]>([])
/** 查重请求进行中 */
const dedupeLoading = ref(false)

/** 实体类型下拉选项：直接从全局映射派生，新增类型只改 utils/entityLabels.ts 一处 */
const entityTypeOptions: SelectOption[] = Object.entries(ENTITY_TYPE_LABELS).map(
  ([value, label]) => ({ value, label }),
)

/**
 * 触发一次重新评分，并把结果或失败原因落到界面提示上
 *
 * 失败时若只写控制台，整条研究链路的表现就是"什么都没发生"——`entity_score`
 * 曾经长期全为 NULL 却无人察觉，正是这个原因，因此这里必须让失败可见。
 *
 * @returns 无返回值；结果写入 `scoreMsg` / `scoreError`
 */
async function runScore() {
  scoreError.value = false
  try {
    const count = await entitiesStore.scoreArticles()
    scoreMsg.value = count > 0 ? `已重新评分 ${count} 篇文章` : '所有文章评分已是最新'
  } catch (e) {
    scoreError.value = true
    scoreMsg.value = `评分失败：${e instanceof Error ? e.message : String(e)}`
  }
  // 关系图依赖「关注实体」集合，评分后图已过期；失效处理时机由外壳决定
  emit('entities-changed')
}

/**
 * 添加新实体
 *
 * 后端会先判"这个名字是不是已有实体的另一种写法"：是则并入并返回 `merged_into`，
 * 否则新建并补全别名。所以提示语必须按返回值分三种写——用户点完「添加」
 * 发现列表没多出一行时，只有明确告诉他"已并入「谷歌」"才不会以为没生效。
 *
 * 新实体必须立刻参与评分才对存量文章生效，否则批量提取仍然捞不到它们
 * （候选条件是评分达到阈值）。评分为纯本地字符串匹配，不消耗 AI 额度。
 */
async function handleAdd() {
  const name = newName.value.trim()
  if (!name) return
  try {
    const outcome = await entitiesStore.createEntity(name, newType.value)
    newName.value = ''
    // 实体集合变了，上一轮查重结果已经不作数
    duplicates.value = []
    const message = outcome.merged_into
      ? `「${name}」已并入已有实体「${outcome.merged_into}」，并记为它的别名`
      : outcome.aliases.length
        ? `已添加「${outcome.name}」，同时补全 ${outcome.aliases.length} 个别名`
        : `已添加「${outcome.name}」`
    await runScore()
    // runScore 会写自己的评分提示，这里覆盖成更有信息量的那句
    scoreError.value = false
    scoreMsg.value = message
  } catch (e) {
    // 错误已在 store 中处理
  }
}

/**
 * 打开「名称与合并」弹窗（别名维护 + 并入其他实体）
 *
 * @param entity - 被点击的实体
 */
function openIdentity(entity: typeof entitiesStore.entities[0]) {
  identityEntity.value = entity
  identityTarget.value = null
  identityOpen.value = true
}

/**
 * 身份弹窗报告变化：重新评分并刷新提示
 *
 * 别名与合并都会改变"哪篇文章算命中这个实体"，所以评分必须重算；
 * 关系图的失效交给外壳（emit `entities-changed`，由 `runScore` 一并触发）。
 *
 * @param message - 可展示的结果描述
 */
async function handleIdentityChanged(message: string) {
  // 合并后上一轮候选里可能有已被删掉的实体，继续展示会点到空
  duplicates.value = []
  await runScore()
  scoreError.value = false
  scoreMsg.value = message
}

/**
 * 让模型找出可能重复的实体（只出候选，不合并）
 */
async function handleFindDuplicates() {
  dedupeLoading.value = true
  scoreError.value = false
  scoreMsg.value = ''
  try {
    const found = await entitiesStore.suggestMerges()
    duplicates.value = found
    if (!found.length) scoreMsg.value = '没有发现疑似重复的实体'
  } catch (e) {
    duplicates.value = []
    scoreError.value = true
    scoreMsg.value = `查重失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    dedupeLoading.value = false
  }
}

/**
 * 按 AI 建议发起一次合并：把「sourceName」并到 targetEntityId
 *
 * 只负责把弹窗摆好（选中并入方、预选目标），真正的确认与执行在弹窗里——
 * 影响面必须先展示，不能在这里直接合并。
 *
 * @param targetEntityId - 保留方实体 ID（候选组里的第一个）
 * @param sourceName - 并入方实体名
 */
function mergeSuggestion(targetEntityId: number, sourceName: string) {
  const source = entitiesStore.entities.find(e => e.name === sourceName)
  if (!source) {
    scoreError.value = true
    scoreMsg.value = `找不到实体「${sourceName}」，可能已被合并或删除`
    return
  }
  identityEntity.value = source
  identityTarget.value = targetEntityId
  identityOpen.value = true
}

/**
 * 删除实体（先算影响面，再二次确认）
 *
 * 删除是不可逆的：该实体的研究事件、论点（含手写正文与反驳证据）、覆盖、
 * 市场标的、财务数据会被外键级联清空。所以顺序是**先查再问**——
 * 不能在用户不知道要丢什么的情况下让他点"确定"。
 *
 * 同时后端会把名字写进屏蔽表，此后归一化不再为它自动创建实体；
 * 否则删完下一轮 AI 提取就把它建回来了，等于没删。
 */
async function handleDelete(entity: typeof entitiesStore.entities[0]) {
  // 影响面查失败不阻断删除（删除本身仍要能进行），但文案要退回到"不列数字"的写法，
  // 绝不能因为查不到就假装"没什么可丢的"
  let impact: EntityImpact | null = null
  try {
    impact = await entitiesStore.fetchImpact(entity.id)
  } catch {
    impact = null
  }

  const lines = [`确定删除实体「${entity.name}」吗？`]
  if (impact) {
    const parts: string[] = []
    if (impact.events) parts.push(`${impact.events} 条研究事件`)
    if (impact.theses) parts.push(`${impact.theses} 条论点`)
    if (impact.coverage) parts.push(`${impact.coverage} 条覆盖记录`)
    if (impact.market_symbols) parts.push(`${impact.market_symbols} 条市场标的`)
    if (impact.financial) parts.push(`${impact.financial} 条财务数据`)
    if (parts.length) lines.push(`将同时删除：${parts.join('、')}。`)
  } else {
    lines.push('它名下的研究事件、论点与覆盖记录也会一并删除。')
  }
  lines.push('该名字以后不会再被自动添加，且此操作不可撤销。')

  const ok = await showConfirm({ title: '删除实体', message: lines.join('\n') })
  if (!ok) return
  try {
    await entitiesStore.deleteEntity(entity.id)
    await entitiesStore.loadBlocked()
    await runScore()
  } catch (e) {
    console.error('删除实体失败:', e)
    scoreError.value = true
    scoreMsg.value = `删除实体失败：${e instanceof Error ? e.message : String(e)}`
  }
}

/**
 * 解除屏蔽：该名字之后又能被自动创建
 *
 * 不会把实体建回来——恢复是"不再拦着"，而不是替用户决定重新关注。
 */
async function handleUnblock(item: EntityBlock) {
  try {
    await entitiesStore.unblockEntity(item.name)
  } catch (e) {
    scoreError.value = true
    scoreMsg.value = `恢复失败：${e instanceof Error ? e.message : String(e)}`
  }
}

/**
 * 切换实体启用/禁用状态
 */
async function handleToggle(entity: typeof entitiesStore.entities[0]) {
  try {
    await entitiesStore.toggleEntity(entity.id)
    // 状态变更后重新评分：停用的实体不应再贡献分数
    await runScore()
  } catch (e) {
    // 错误已在 store 中处理
  }
}

/**
 * 触发全库文章重新评分
 */
async function handleScore() {
  scoreMsg.value = ''
  await runScore()
}

/**
 * 打开「实体仪表盘」聚合页
 *
 * 走 window 事件 `open-entity-dashboard`（与 open-settings 同构的跨组件开弹窗模式），
 * 由 App.vue 监听后切换主屏到实体仪表盘全屏视图。实体信息区可点击，让"看某实体的
 * 舆情/财务/事件"从"钻进关系图找节点"变成一步直达。
 *
 * @param entity - 被点击的实体
 */
function openDashboard(entity: typeof entitiesStore.entities[0]) {
  window.dispatchEvent(
    new CustomEvent('open-entity-dashboard', { detail: { entityId: entity.id } })
  )
}

/**
 * 打开「实体告警规则」编辑弹窗（P1-F）
 *
 * 把被点击实体透传给弹窗；弹窗内部自行加载并保存该实体的 `alert_rules`，
 * 本页签不持有规则状态，避免与列表产生第二份真相。
 *
 * @param entity - 被点击的实体
 */
function openRuleModal(entity: typeof entitiesStore.entities[0]) {
  ruleEntity.value = entity
  ruleModalOpen.value = true
}

// 已屏蔽列表只在"删除/恢复"时变化，不随实体列表一起刷新。
// 挂载时取一次即可；取失败在 store 内已降级（不抛），页签不会因此报错。
onMounted(() => {
  void entitiesStore.loadBlocked()
})
</script>

<style scoped>
/* ═══ 实体管理（列表 + 新增表单）═══ */
.entity-list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.entity-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-15) var(--sp-3);
  background: var(--fill);
  border-radius: var(--r-md);
  transition: background var(--m-tap) var(--m-ease-out);
}
.entity-item:hover {
  background: var(--hover-bg);
}
.entity-info {
  display: flex;
  flex-direction: column;
  gap: var(--sp-025);
  min-width: 0;
}
.entity-info.clickable {
  cursor: pointer;
  border-radius: var(--r-sm);
}
.entity-info.clickable:hover .entity-name {
  color: var(--primary);
}
/* 列表行主字段：13/500 —— 与 DigestModal 的 .article-title 同一角色（密集列表用标签档）。
   卡片式条目（如设置页任务卡）才用 14/600，这里的行只有两行信息，不需要那么重。 */
.entity-name {
  font-size: var(--fs-modal-label);
  font-weight: 500;
  line-height: var(--lh-modal-label);
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.entity-type {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
/* 类型 + 状态挤在同一行：两者都是元信息，不该各占一行 */
.entity-meta {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  min-width: 0;
}
/* 「已禁用」用描边小胶囊而不是实色：它只是状态注记，不是当前选中项，
   抢主色会让人误以为要操作它。面积远小于阈值，故不加底色。 */
.entity-off {
  padding: 0 var(--sp-05);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  white-space: nowrap;
}
/* 禁用行整体降一档：名字从主文降到次级，但「已禁用」文字标记仍在，不靠颜色单独表意 */
.entity-item.is-disabled .entity-name {
  color: var(--text-secondary);
}
.entity-actions {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  flex-shrink: 0;
}
/* 行内三个图标钮（启停 / 删除 / 告警）的尺寸与外观全部来自全局
   `.btn .btn-icon`（32×32）与 `.btn-quiet`，本文件不再持有任何按钮配色——
   此前这里有三份逐字复制的 32×32 + 显式 transition，改一个就要改三处。 */

/* 启用开关里的圆点：只有"点"是状态，按钮本体仍是标准图标钮。
   开 = 10px 实心主色；关 = 10px 空心，1.5px 描边。 */
.entity-switch-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  border: 1.5px solid var(--text-tertiary);
  background: transparent;
  transition: background var(--dur) var(--ease), border-color var(--dur) var(--ease);
}
.entity-switch.enabled .entity-switch-dot {
  background: var(--primary);
  border-color: var(--primary);
}
.add-entity {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  padding-top: var(--sp-3);
  border-top: 1px solid var(--border);
}
.input-row {
  display: flex;
  gap: var(--sp-2);
}
.entity-input {
  flex: 1;
  height: 34px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-15);
  font-size: var(--fs-modal-body);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: border-color var(--m-state) var(--m-ease-out);
}
.entity-input:focus {
  border-color: var(--primary);
}
.entity-input::placeholder {
  color: var(--text-tertiary);
}
.type-select {
  flex-shrink: 0;
}
/* 「添加」与输入框同一行，所以用 `.btn-form`（34px）而不是默认 32px——
   这一行的 `.entity-input` / `.type-select` 都是 34px，按钮矮 2px 会读成没对齐。 */
.batch-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  padding-top: var(--sp-1);
}
/* 右侧两个批量动作成组：与左侧的行内提示分开，提示再长也只挤自己 */
.batch-buttons {
  display: flex;
  gap: var(--sp-1);
  flex-shrink: 0;
}

/* ═══ AI 查重结果 ═══
   一组一块淡底卡片：候选是"待决定的事"，不是日常配置，
   用块面与上面的实体列表区分开，扫一眼就知道这块是一次性清理任务。 */
.dup-box {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  padding: var(--sp-3);
  border-radius: var(--r-md);
  background: var(--fill);
}
.dup-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
}
.dup-title {
  font-size: var(--fs-modal-label);
  font-weight: 500;
  line-height: var(--lh-modal-label);
  color: var(--text-primary);
}
.dup-hint {
  margin: 0;
  font-size: var(--fs-modal-meta);
  line-height: var(--lh-modal-meta);
  color: var(--text-tertiary);
}
.dup-group {
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
  padding-top: var(--sp-2);
  border-top: 1px solid var(--border);
}
.dup-names {
  font-size: var(--fs-modal-label);
  color: var(--text-primary);
  word-break: break-word;
}
.dup-reason {
  margin: 0;
  font-size: var(--fs-modal-meta);
  line-height: var(--lh-modal-meta);
  color: var(--text-tertiary);
}
.dup-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-1);
  padding-top: var(--sp-05);
}
/* 评分结果提示：成功/空态走中性色，失败走警告色。整句 → 正文档 14 / 1.65 */
.score-msg {
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--text-secondary);
}
.score-msg.err {
  color: var(--danger-text);
}
/* 错误提示是必读句子，与成功提示同档 14 */
.error-msg {
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  color: var(--danger-text);
}

/* ═══ 已删除（屏蔽）的实体 ═══
   用原生 <details>：summary 天然可聚焦、可键盘展开、读屏会播报展开状态。
   默认收起、只占一行——它是"删过什么"的账本，不是日常操作区。 */
.blocked-box {
  margin-top: var(--sp-3);
  padding-top: var(--sp-2);
  border-top: 1px solid var(--border);
}
.blocked-summary {
  padding: var(--sp-05) 0;
  border-radius: var(--r-sm);
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
  cursor: pointer;
}
.blocked-summary:hover {
  color: var(--text-primary);
}
.blocked-summary:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
.blocked-hint {
  margin: var(--sp-1) 0 var(--sp-2);
  font-size: var(--fs-modal-meta);
  line-height: var(--lh-modal-meta);
  color: var(--text-tertiary);
}
.blocked-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  padding: var(--sp-05) 0;
}
.blocked-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
}
/* 行内按钮不参与收缩：名字再长也只截名字 */
.blocked-item .btn {
  flex-shrink: 0;
}
</style>
