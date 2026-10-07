<template>
  <div class="tab-pane">
    <!-- 任务卡片列表 -->
    <div v-if="researchStore.tasks.length === 0 && !researchStore.tasksLoading" class="empty-tasks">
      <p>暂无自动化任务</p>
      <p class="form-hint">点击下方"新建任务"创建定时提取或定时报告任务</p>
    </div>
    <div
      v-for="t in researchStore.tasks"
      :key="t.id"
      class="task-card"
      :class="{ disabled: !t.enabled }"
    >
      <!-- 卡片头：名称 + 类型徽标 + 启停开关 -->
      <div class="task-head">
        <span class="task-name">{{ t.name }}</span>
        <span class="task-type-badge" :class="`badge-${t.task_type}`">
          {{ taskTypeLabel(t.task_type) }}
        </span>
        <AppSwitch
          :model-value="t.enabled"
          :label="`启用任务「${t.name}」`"
          :title="t.enabled ? '点击停用' : '点击启用'"
          @change="handleToggleTask(t)"
        />
      </div>
      <!-- 调度说明：cron 表达式转人类可读 -->
      <div class="task-schedule">调度：{{ cronDesc(t.cron_expr) }}</div>

      <!-- 健康度三态：成功 / 失败 / 未配置（含引导跳转）；从未运行显示灰态 -->
      <div v-if="t.last_status" class="task-status" :class="`status-${t.last_status}`">
        <span class="status-dot"></span>
        <span class="status-text">
          {{ statusText(t) }}
        </span>
        <!-- 未配置时的引导链接：按任务类型跳到对应配置 tab（跨 tab，故抛给父组件） -->
        <a
          v-if="t.last_status === 'unconfigured' && guideTab(t)"
          class="guide-link"
          @click="goConfigure(t)"
        >
          去配置 →
        </a>
      </div>
      <div v-else class="task-status status-never">
        <span class="status-dot"></span>
        <span class="status-text">从未运行</span>
      </div>

      <!-- 卡片操作：立即运行（手动补跑） / 编辑 / 删除 -->
      <div class="task-actions">
        <button
          class="btn btn-default btn-sm"
          :disabled="researchStore.runningTaskIds.has(t.id)"
          @click="handleRunNow(t)"
        >
          <span v-if="researchStore.runningTaskIds.has(t.id)" class="run-spinner"></span>
          {{ researchStore.runningTaskIds.has(t.id) ? '运行中…' : '立即运行' }}
        </button>
        <button class="btn btn-default btn-sm" @click="startEdit(t)">编辑</button>
        <button class="btn btn-default btn-sm btn-danger-text" @click="handleDeleteTask(t)">删除</button>
      </div>
    </div>

    <!-- 新建/编辑任务表单（内联展开，保存后收起） -->
    <div v-if="showTaskForm" class="task-form">
      <div class="form-group">
        <label class="form-label">任务名称</label>
        <input class="form-input" v-model="formName" placeholder="例如：每日研究报告" />
      </div>
      <div class="form-group">
        <label class="form-label">任务类型</label>
        <!-- 任务类型在创建后不可改（类型决定 config 结构，中途改会导致配置错位） -->
        <AppSelect
          v-model="formType"
          :options="taskTypeOptions"
          :disabled="editingTaskId !== null"
        />
      </div>
      <div class="form-group">
        <label class="form-label">调度时间（cron）</label>
        <input class="form-input" v-model="formCron" placeholder="0 30 2 * * *" />
        <p class="form-hint">六位 cron：秒 分 时 日 月 周；"0 30 2 * * *" = 每天 02:30</p>
      </div>
      <!-- 提取类配置项 -->
      <template v-if="formType === 'batch_extract'">
        <div class="form-row">
          <div class="form-group">
            <label class="form-label">AI 提供商</label>
            <input class="form-input" v-model="formProvider" placeholder="agnes" />
          </div>
          <div class="form-group">
            <label class="form-label">评分阈值</label>
            <input class="form-input" v-model.number="formMinScore" type="number" min="0" max="100" />
          </div>
          <div class="form-group">
            <label class="form-label">单次上限（篇）</label>
            <input class="form-input" v-model.number="formBatchLimit" type="number" min="1" max="200" />
          </div>
        </div>
      </template>
      <!-- 报告类配置项 -->
      <template v-else>
        <div class="form-row">
          <div class="form-group">
            <label class="form-label">输出通道</label>
            <AppSelect v-model="formChannel" :options="channelOptions" />
            <!-- 含 Obsidian 的通道都依赖「设置 → Obsidian」的 Vault 路径，
                 未配置时任务会停在"未配置"状态并给出跳转引导 -->
            <p class="form-hint" v-if="formChannel !== 'app'">
              报告将写入 Vault 的导出目录（含 Obsidian 的通道需先配好 Vault 路径）
            </p>
          </div>
          <div class="form-group">
            <label class="form-label">覆盖天数</label>
            <input class="form-input" v-model.number="formDays" type="number" min="1" max="90" />
          </div>
        </div>
      </template>
      <div class="form-actions">
        <button class="btn btn-default" @click="showTaskForm = false">取消</button>
        <button class="btn btn-primary" :disabled="savingTask" @click="handleSaveTask">
          {{ savingTask ? '保存中…' : editingTaskId !== null ? '保存修改' : '创建任务' }}
        </button>
      </div>
      <p class="form-hint task-form-error" v-if="taskFormError">{{ taskFormError }}</p>
    </div>

    <!-- 新建入口（表单展开时隐藏） -->
    <button v-if="!showTaskForm" class="btn btn-default" @click="resetForm">＋ 新建任务</button>
  </div>
</template>

<script setup lang="ts">
/**
 * 设置 · 自动化 tab
 *
 * # 职责
 * 定时任务管理（提取 / 报告）：任务即数据，支持立即运行、启停、编辑与删除，
 * 并以「成功 / 失败 / 未配置 / 从未运行」四态展示健康度。
 *
 * # 与外部的边界
 * - 任务数据在 `researchStore`，本组件挂载时拉取一次（等价于拆分前"切到本 tab 时加载"）。
 * - 「去配置 →」要跳到 AI / Obsidian tab 属于**跨 tab 动作**，本组件只 emit
 *   `switch-tab`，由 SettingsModal 决定怎么切。
 * - 本 tab 没有底部保存按钮：任务的增删改都是即点即提交。
 */
import { ref, onMounted } from 'vue'
import { useResearchStore } from '@/stores/research'
import { useModalStore } from '@/stores/modal'
import { useSettingsStore } from '@/stores/settings'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import AppSwitch from './AppSwitch.vue'
import {
  REPORT_CHANNEL_KEY,
  VAULT_PATH_KEY,
  resolveReportChannel,
  isReportChannel,
  type ReportChannel
} from '@/utils/reportChannel'
import type { AutomationTask } from '@/types'

/** 研究 store 实例：自动化任务的读写与立即运行走其后端命令 */
const researchStore = useResearchStore()
/** 全局弹窗 store 实例 */
const modalStore = useModalStore()
/** 设置 store 实例：读取报告通道偏好与 Vault 路径 */
const settingsStore = useSettingsStore()

const emit = defineEmits<{
  /** 请求父组件切到指定配置 tab（"去配置 →"引导链接用） */
  (e: 'switch-tab', tab: 'ai' | 'obsidian'): void
}>()

// ─── 自动化任务管理 ───────────────────────────────────────────────────────────

/** 任务表单展开态 */
const showTaskForm = ref(false)
/** 正在编辑的任务 ID（null = 新建模式） */
const editingTaskId = ref<number | null>(null)
/** 任务表单保存中 */
const savingTask = ref(false)
/** 任务表单校验/保存错误 */
const taskFormError = ref('')

/** 表单字段：任务名称与类型 */
const formName = ref('')
const formType = ref<'batch_extract' | 'daily_report' | 'weekly_report'>('batch_extract')
/** 表单字段：cron 表达式 */
const formCron = ref('0 30 2 * * *')
/** 提取类配置：AI 提供商 / 评分阈值 / 单次上限 */
const formProvider = ref('agnes')
const formMinScore = ref(40)
const formBatchLimit = ref(50)
/** 报告类配置：输出通道（both = 应用内 + Obsidian 双写）/ 覆盖天数 */
const formChannel = ref<ReportChannel>('app')
const formDays = ref(1)

/** 任务类型选项 */
const taskTypeOptions: SelectOption[] = [
  { value: 'batch_extract', label: '智能提取（批量）' },
  { value: 'daily_report', label: '每日报告' },
  { value: 'weekly_report', label: '每周报告' },
]
/** 报告输出通道选项 */
const channelOptions: SelectOption[] = [
  { value: 'app', label: '应用内（报告查看器）' },
  { value: 'obsidian', label: 'Obsidian Vault' },
  { value: 'both', label: '应用内 + Obsidian' },
]

/**
 * 报告通道偏好的兜底值
 *
 * 与研究工作台的手动生成工具条同源（`resolveReportChannel`）：新建报告任务时
 * 默认沿用上次选择的通道（未配置 Vault 则退回应用内），省掉每次重选。
 *
 * @returns 合法的通道取值
 */
function preferredChannel(): ReportChannel {
  return resolveReportChannel(
    settingsStore.getSetting(REPORT_CHANNEL_KEY),
    settingsStore.getSetting(VAULT_PATH_KEY)
  )
}

/**
 * 任务类型的中文徽标文案
 *
 * @param type - 任务类型枚举值
 * @returns 中文标签；未知类型原样返回
 */
function taskTypeLabel(type: string): string {
  const labels: Record<string, string> = {
    batch_extract: '提取',
    daily_report: '日报',
    weekly_report: '周报'
  }
  return labels[type] ?? type
}

/**
 * cron 表达式转人类可读描述
 *
 * 只做常见模式的友好化（每日 / 每周几 / 每小时），解析不了时原样展示
 * 表达式——宁可不翻译也不给错翻译。
 *
 * @param expr - 六位 cron（秒 分 时 日 月 周）
 * @returns 如 "每天 02:30"；无法识别时返回原表达式
 */
function cronDesc(expr: string): string {
  const parts = expr.trim().split(/\s+/)
  if (parts.length !== 6) return expr
  const [, min, hour, , , dayOfWeek] = parts
  const time = `${hour.padStart(2, '0')}:${min.padStart(2, '0')}`
  // 每周几（0/7=周日，1-6=周一到周六）
  if (dayOfWeek !== '*') {
    const names = ['周日', '周一', '周二', '周三', '周四', '周五', '周六', '周日']
    const idx = Number(dayOfWeek)
    if (Number.isInteger(idx) && idx >= 0 && idx <= 7) {
      return `每${names[idx]} ${time}`
    }
    return expr
  }
  return `每天 ${time}`
}

/**
 * 任务健康度文案（状态 + 时间 + 结果消息）
 *
 * @param t - 任务行
 * @returns 如 "✓ 成功 · 09-28 08:00 · 日报已生成：…"
 */
function statusText(t: AutomationTask): string {
  const icons: Record<string, string> = {
    success: '✓ 成功',
    failed: '✗ 失败',
    unconfigured: '● 未配置'
  }
  const icon = icons[t.last_status] ?? t.last_status
  const time = t.last_run_at ? t.last_run_at.slice(5, 16).replace('T', ' ') : ''
  return time ? `${icon} · ${time}` : icon
}

/**
 * 未配置任务的引导跳转目标
 *
 * 提取类缺 AI 配置 → "ai" tab；报告类缺 Vault → "obsidian" tab；
 * 无法判断时返回 null（模板据此不渲染链接）。
 *
 * @param t - 任务行
 * @returns 目标 tab；无引导目标时为 null
 */
function guideTab(t: AutomationTask): 'ai' | 'obsidian' | null {
  if (t.task_type === 'batch_extract') return 'ai'
  if (t.task_type === 'daily_report' || t.task_type === 'weekly_report') {
    return 'obsidian'
  }
  return null
}

/**
 * 点击「去配置 →」：把切 tab 的意图抛给父组件
 *
 * @param t - 任务行
 */
function goConfigure(t: AutomationTask) {
  const target = guideTab(t)
  if (target) emit('switch-tab', target)
}

/**
 * 立即运行任务（手动补跑入口，与 cron 走同一执行器）
 *
 * 结果以全局弹窗反馈：成功展示统计消息，失败展示原因，
 * 便于用户在关闭设置页前就知道跑成了没有。
 *
 * @param t - 任务行
 */
async function handleRunNow(t: AutomationTask) {
  try {
    const msg = await researchStore.runTaskNow(t.id)
    await modalStore.showAlert({ title: '任务完成', message: msg })
  } catch (e) {
    await modalStore.showAlert({
      title: '任务运行失败',
      message: e instanceof Error ? e.message : String(e)
    })
  }
}

/**
 * 切换任务启停
 *
 * @param t - 任务行
 */
async function handleToggleTask(t: AutomationTask) {
  try {
    await researchStore.setTaskEnabled(t.id, !t.enabled)
  } catch (e) {
    console.error('切换任务状态失败:', e)
  }
}

/**
 * 删除任务（破坏性操作，先经全局确认弹窗）
 *
 * @param t - 任务行
 */
async function handleDeleteTask(t: AutomationTask) {
  const ok = await modalStore.showConfirm({
    title: '删除任务',
    message: `确定删除任务「${t.name}」吗？该操作不可恢复。`
  })
  if (!ok) return
  try {
    await researchStore.deleteTask(t.id)
  } catch (e) {
    console.error('删除任务失败:', e)
  }
}

/**
 * 进入编辑模式：用任务当前值回填表单并展开
 *
 * config 的 JSON 结构按任务类型拆到具体输入框，解析失败回退默认值。
 *
 * @param t - 任务行
 */
function startEdit(t: AutomationTask) {
  editingTaskId.value = t.id
  formName.value = t.name
  if (t.task_type === 'batch_extract' || t.task_type === 'daily_report' || t.task_type === 'weekly_report') {
    formType.value = t.task_type
  }
  formCron.value = t.cron_expr
  try {
    const cfg = JSON.parse(t.config) as Record<string, unknown>
    formProvider.value = typeof cfg.provider === 'string' ? cfg.provider : 'agnes'
    formMinScore.value = typeof cfg.min_score === 'number' ? cfg.min_score : 40
    formBatchLimit.value = typeof cfg.batch_limit === 'number' ? cfg.batch_limit : 50
    // 通道只认三个合法值；缺失或非法时回退到"上次选择 / 按 Vault 配置推导"
    formChannel.value =
      typeof cfg.channel === 'string' && isReportChannel(cfg.channel)
        ? cfg.channel
        : preferredChannel()
    formDays.value = typeof cfg.days === 'number' ? cfg.days : 1
  } catch {
    // config 非法时保持默认值：编辑保存会用默认值覆盖，可接受
  }
  showTaskForm.value = true
  taskFormError.value = ''
}

/**
 * 重置表单为新建模式并展开
 */
function resetForm() {
  editingTaskId.value = null
  formName.value = ''
  formType.value = 'batch_extract'
  formCron.value = '0 30 2 * * *'
  formProvider.value = 'agnes'
  formMinScore.value = 40
  formBatchLimit.value = 50
  // 新建报告任务的默认通道沿用上次选择，未配置 Vault 时退回应用内
  formChannel.value = preferredChannel()
  formDays.value = 1
  showTaskForm.value = true
  taskFormError.value = ''
}

/**
 * 保存任务表单（新建或编辑）
 *
 * 校验名称与 cron 后按任务类型拼装 config JSON，走 store 提交；
 * 成功后收起表单并刷新列表（列表刷新在 store 内完成）。
 */
async function handleSaveTask() {
  taskFormError.value = ''
  if (!formName.value.trim()) {
    taskFormError.value = '请填写任务名称'
    return
  }
  if (formCron.value.trim().split(/\s+/).length !== 6) {
    taskFormError.value = 'cron 表达式须为六位（秒 分 时 日 月 周）'
    return
  }
  // 按任务类型拼装 config：提取类三参数，报告类两参数
  const config =
    formType.value === 'batch_extract'
      ? JSON.stringify({
          provider: formProvider.value.trim() || 'agnes',
          min_score: formMinScore.value,
          batch_limit: formBatchLimit.value
        })
      : JSON.stringify({
          channel: formChannel.value,
          days: formDays.value
        })

  savingTask.value = true
  try {
    if (editingTaskId.value !== null) {
      await researchStore.updateTask(editingTaskId.value, {
        name: formName.value.trim(),
        cronExpr: formCron.value.trim(),
        config
      })
    } else {
      await researchStore.createTask(
        formName.value.trim(),
        formType.value,
        formCron.value.trim(),
        config,
        true
      )
    }
    showTaskForm.value = false
  } catch (e) {
    taskFormError.value = e instanceof Error ? e.message : String(e)
  } finally {
    savingTask.value = false
  }
}

// 挂载即拉取任务列表（本组件只在切到「自动化」tab 时挂载，加载时机与拆分前一致）
onMounted(() => {
  void researchStore.loadTasks()
})
</script>

<style scoped>
/* 表单原语（.form-group / .form-label / .form-input / .form-hint / .divider /
   .tab-pane）已提升为全局类（styles.css，被多个 tab 共用）。
   这里保留本 tab 独有的任务卡片族与任务表单布局。 */

/* ─── 自动化任务卡片 ─── */
.empty-tasks {
  text-align: center;
  padding: var(--sp-6) 0;
  color: var(--text-tertiary);
}
.empty-tasks p { margin: 0 0 var(--sp-05); }
.task-card {
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-3) var(--sp-35);
  margin-bottom: var(--sp-15);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  transition: opacity 0.2s;
}
/* 停用任务整体降不透明度，视觉上与启用任务区分 */
.task-card.disabled { opacity: 0.6; }
.task-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}
.task-name {
  flex: 1;
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
}
.task-type-badge {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  font-weight: 600;
  padding: var(--sp-025) var(--sp-2);
  border-radius: var(--r-sm);
  color: var(--tone-fg);
}
.badge-batch_extract { background: var(--info); }
.badge-daily_report { background: var(--success); }
.badge-weekly_report { background: var(--tone-indigo); }
.task-schedule {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}

/* 健康度三态徽标：语义色圆点 + 文案 */
.task-status {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  font-size: var(--fs-sm);
  line-height: 1.5;
}
.status-dot {
  flex-shrink: 0;
  width: 7px;
  height: 7px;
  border-radius: 50%;
}
.status-success { color: var(--text-secondary); }
.status-success .status-dot { background: var(--success); }
.status-failed { color: var(--danger); }
.status-failed .status-dot { background: var(--danger); }
.status-unconfigured { color: var(--warning); }
.status-unconfigured .status-dot { background: var(--warning); }
.status-never { color: var(--text-tertiary); }
.status-never .status-dot { background: var(--border); }
.status-text { flex: 1; word-break: break-all; }
.guide-link {
  flex-shrink: 0;
  color: var(--primary);
  cursor: pointer;
  font-size: var(--fs-sm);
}
.guide-link:hover { text-decoration: underline; }

.task-actions {
  display: flex;
  gap: var(--sp-2);
}
/* .btn-sm / .btn-danger-text 已收敛到全局按钮体系，此处不再声明。
   危险色也不再需要 var(--danger) 兜底——styles.css 已正式定义该 token。 */

/* 运行中 spinner（立即运行按钮内） */
.run-spinner {
  display: inline-block;
  width: 10px;
  height: 10px;
  margin-right: var(--sp-1);
  border: 2px solid var(--border);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: task-spin 0.8s linear infinite;
  vertical-align: -1px;
}
@keyframes task-spin {
  to { transform: rotate(360deg); }
}

/* 任务表单 */
.task-form {
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-35);
  margin-bottom: var(--sp-15);
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
}
.form-row {
  display: flex;
  gap: var(--sp-15);
}
.form-row .form-group { flex: 1; }
.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--sp-2);
  margin-top: var(--sp-1);
}
.task-form-error {
  color: var(--danger);
}
</style>
