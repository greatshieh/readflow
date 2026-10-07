<template>
  <!-- ═══ Tab 3：实体管理 ═══ -->
  <div class="m-body">
    <!-- 实体列表 -->
    <div class="entity-list" v-if="entitiesStore.entities.length > 0">
      <div
        v-for="entity in entitiesStore.entities"
        :key="entity.id"
        class="entity-item"
      >
        <div class="entity-info">
          <span class="entity-name">{{ entity.name }}</span>
          <span class="entity-type">{{ entityTypeLabel(entity.entity_type) }}</span>
        </div>
        <div class="entity-actions">
          <!-- 启用/禁用开关 -->
          <button
            class="toggle-btn"
            :class="{ active: entity.enabled }"
            @click="handleToggle(entity)"
            :title="entity.enabled ? '禁用' : '启用'"
          >
            {{ entity.enabled ? '✓' : '○' }}
          </button>
          <!-- 删除按钮 -->
          <button
            class="delete-btn"
            @click="handleDelete(entity)"
            title="删除"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polyline points="3 6 5 6 21 6"></polyline>
              <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2"></path>
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
        <button class="add-btn" @click="handleAdd" :disabled="!newName.trim()">
          添加
        </button>
      </div>
      <div v-if="entitiesStore.error" class="error-msg">{{ entitiesStore.error }}</div>
    </div>

    <!-- 批量操作：评分结果与失败原因都落在按钮左侧的行内提示上 ——
         例行操作不该弹窗打断，失败也不该只写进控制台 -->
    <div class="batch-actions">
      <span v-if="scoreMsg" class="score-msg" :class="{ err: scoreError }">{{ scoreMsg }}</span>
      <button class="score-btn" @click="handleScore" :disabled="entitiesStore.loading">
        {{ entitiesStore.loading ? '评分中...' : '重新评分' }}
      </button>
    </div>
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
 * # 与外壳/其它页签的契约
 * - 实体列表的加载由外壳在弹窗打开时统一保证（时间线筛选下拉与这里共用）；
 * - 评分/增删/启停会让已加载的关系图过期：本页签只 emit `entities-changed`
 *   通知外壳，由关系图页签在自己可见时决定何时重算——拆分前是立即后台
 *   重算，现在改为推迟到用户切到关系图页签时，省掉一次看不见的全库扫描。
 */
import { ref } from 'vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import { useEntitiesStore } from '@/stores/entities'
import { showConfirm } from '@/stores/modal'
import { ENTITY_TYPE_LABELS, entityTypeLabel } from '@/utils/entityLabels'

const emit = defineEmits<{
  /** 实体集合或评分已变化，已加载的关系图随之过期（外壳转交给关系图页签） */
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
    scoreMsg.value = count > 0 ? `已重新评分 ${count} 篇文章` : '没有需要评分的文章'
  } catch (e) {
    scoreError.value = true
    scoreMsg.value = `评分失败：${e instanceof Error ? e.message : String(e)}`
  }
  // 关系图依赖「关注实体」集合，评分后图已过期；重算时机由关系图页签决定
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
  transition: background 0.15s;
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
.entity-name {
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.entity-type {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.entity-actions {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  flex-shrink: 0;
}
.toggle-btn {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  border: 1px solid var(--border);
  background: var(--surface);
  cursor: pointer;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s;
}
.toggle-btn.active {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--primary-fg);
}
.toggle-btn:hover {
  border-color: var(--primary);
}
.delete-btn {
  width: 28px;
  height: 28px;
  border-radius: var(--r-sm);
  border: none;
  background: none;
  cursor: pointer;
  color: var(--text-tertiary);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.15s;
}
.delete-btn:hover {
  background: var(--danger-fg);
  color: var(--danger);
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
  font-size: var(--fs-md);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: border-color 0.15s;
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
.add-btn {
  height: 34px;
  padding: 0 var(--sp-35);
  border: none;
  border-radius: var(--r-md);
  background: var(--primary);
  color: var(--tone-fg);
  font-size: var(--fs-md);
  cursor: pointer;
  transition: opacity 0.15s;
  white-space: nowrap;
}
.add-btn:hover:not(:disabled) {
  opacity: 0.85;
}
.add-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.batch-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-2);
  padding-top: var(--sp-1);
}
/* 评分结果提示：成功/空态走中性色，失败走警告色 */
.score-msg {
  font-size: var(--fs-sm);
  line-height: 1.5;
  color: var(--text-secondary);
}
.score-msg.err {
  color: var(--danger);
}
.score-btn {
  height: 32px;
  padding: 0 var(--sp-35);
  border: 1px solid var(--primary);
  border-radius: var(--r-md);
  background: var(--primary-fg);
  color: var(--primary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: all 0.15s;
}
.score-btn:hover:not(:disabled) {
  background: var(--primary);
  color: var(--tone-fg);
}
.score-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.error-msg {
  font-size: var(--fs-sm);
  color: var(--danger);
}
</style>
