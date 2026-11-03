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
      >
        <div
          class="entity-info clickable"
          role="button"
          title="查看实体仪表盘"
          @click="openDashboard(entity)"
        >
          <span class="entity-name">{{ entity.name }}</span>
          <span class="entity-type">{{ entityTypeLabel(entity.entity_type) }}</span>
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
      <button class="btn btn-soft" @click="handleScore" :disabled="entitiesStore.loading">
        {{ entitiesStore.loading ? '评分中...' : '重新评分' }}
      </button>
    </div>

    <!-- 实体级告警规则编辑弹窗（P1-F） -->
    <EntityAlertRuleModal
      :open="ruleModalOpen"
      :entity="ruleEntity"
      @close="ruleModalOpen = false"
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
import { ref } from 'vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import EntityAlertRuleModal from './EntityAlertRuleModal.vue'
import { useEntitiesStore } from '@/stores/entities'
import { showConfirm } from '@/stores/modal'
import { ENTITY_TYPE_LABELS, entityTypeLabel } from '@/utils/entityLabels'

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
 * 新实体必须立刻参与评分才对存量文章生效，否则批量提取仍然捞不到它们
 * （候选条件是评分达到阈值）。评分为纯本地字符串匹配，不消耗 AI 额度。
 */
async function handleAdd() {
  const name = newName.value.trim()
  if (!name) return
  try {
    await entitiesStore.createEntity(name, newType.value)
    newName.value = ''
    await runScore()
  } catch (e) {
    // 错误已在 store 中处理
  }
}

/**
 * 删除实体（需二次确认）
 * 删除操作本身是同步的乐观更新，重新评分是纯本地匹配，接着跑完即可。
 */
async function handleDelete(entity: typeof entitiesStore.entities[0]) {
  const ok = await showConfirm({
    title: '删除实体',
    message: `确定删除实体「${entity.name}」吗？\n删除后将重新计算文章评分。`
  })
  if (!ok) return
  try {
    await entitiesStore.deleteEntity(entity.id)
    await runScore()
  } catch (e) {
    console.error('删除实体失败:', e)
    scoreError.value = true
    scoreMsg.value = `删除实体失败：${e instanceof Error ? e.message : String(e)}`
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
</style>
