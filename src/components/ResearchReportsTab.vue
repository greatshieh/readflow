<template>
  <!-- ═══ Tab 2：研究报告 ═══ -->
  <div class="m-body">
    <!-- 手动生成工具条：不依赖自动化任务，参数当场可调。
         置于内容/列表视图之上常驻，生成后仍可继续生成下一份。 -->
    <div class="report-toolbar">
      <AppSelect v-model="genKind" :options="reportKindOptions" width="auto" title="报告类型" />
      <AppSelect v-model="genDays" :options="genDayOptions" width="auto" title="覆盖天数" />
      <AppSelect
        v-model="genChannel"
        :options="genChannelOptions"
        width="auto"
        :title="genChannel === 'app' ? '输出通道' : '输出通道（含 Obsidian 的报告不在应用内查看器显示）'"
        @change="onChannelChange"
      />
      <button class="btn btn-primary btn-sm" :disabled="generating" @click="handleGenerate">
        {{ generating ? '生成中…' : '生成报告' }}
      </button>
      <span v-if="genMsg" class="extract-msg" :class="{ error: genError }">{{ genMsg }}</span>
    </div>

    <!-- 报告内容视图 -->
    <template v-if="researchStore.reportContent">
      <button class="btn btn-secondary btn-back" @click="researchStore.closeReport">← 返回报告列表</button>
      <pre class="report-content">{{ reportBody }}</pre>
    </template>

    <!-- 报告列表视图 -->
    <template v-else>
      <div v-if="researchStore.reports.length === 0 && !reportsLoading" class="empty-state">
        <p>暂无报告</p>
        <p class="hint">
          点上方「生成报告」即刻生成一份；也可在「设置 → 自动化」中建定时任务自动产出
        </p>
      </div>
      <div v-else class="report-list">
        <div
          v-for="r in researchStore.reports"
          :key="r.name"
          class="report-item"
          :class="{ active: r.name === researchStore.activeReportName }"
          @click="handleReadReport(r.name)"
        >
          <span class="report-icon">📄</span>
          <span class="report-name">{{ r.name }}</span>
          <span class="report-meta">{{ r.modified }} · {{ formatSize(r.size) }}</span>
        </div>
      </div>
      <span v-if="reportError" class="extract-msg error">{{ reportError }}</span>
    </template>
  </div>
</template>

<script setup lang="ts">
/**
 * 研究工作台 · 研究报告页签
 *
 * 手动生成日报/周报（类型、覆盖天数、输出通道当场可调，不依赖自动化任务），
 * 并列出已生成的报告、支持读入应用内查看器。
 *
 * # 挂载语义（与外壳的契约）
 * 本页签经 `v-show` 常驻挂载，**不能用 `onMounted` 当"进入页签"信号**：
 * - 每次切入本页签（`active` 翻真）→ 清空读取错误并重拉报告列表——
 *   对应拆分前 `switchToReports` 的"每次进入都加载"；
 * - 弹窗每次打开（`open` 翻真）→ 通道偏好重新取值（可能在「设置 → 自动化」
 *   或上次使用中被改过），并清掉上一次的生成提示。
 * 生成与定时任务共用后端实现，因此无需先创建任务。
 */
import { ref, computed, watch } from 'vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import { useResearchStore } from '@/stores/research'
import { useSettingsStore } from '@/stores/settings'
import {
  REPORT_CHANNEL_KEY,
  VAULT_PATH_KEY,
  resolveReportChannel,
  isReportChannel,
  type ReportChannel
} from '@/utils/reportChannel'
import { stripFrontmatter } from '@/utils/reportMarkdown'

const props = defineProps<{
  /** 是否为当前显示的页签：切入时重拉报告列表 */
  active: boolean
  /** 弹窗是否打开：打开即重取通道偏好并清掉生成提示 */
  open: boolean
}>()

const researchStore = useResearchStore()
const settingsStore = useSettingsStore()

/** 报告列表加载中 */
const reportsLoading = ref(false)

/** 报告读取错误 */
const reportError = ref('')

/**
 * 应用内展示的报告正文
 *
 * 磁盘上的报告带 YAML frontmatter（供 Obsidian 的 dataview 与标签系统消费），
 * 而这里是等宽纯文本展示，直接显示会让一整段元数据挡在正文前面，故只在展示时裁掉，
 * 文件本身保持完整格式。
 */
const reportBody = computed(() => stripFrontmatter(researchStore.reportContent))

// ========== 手动生成报告 ==========
/** 手动生成的报告类型 */
const genKind = ref<'daily' | 'weekly'>('daily')
/** 手动生成的覆盖天数 */
const genDays = ref(1)
/** 手动生成的输出通道（both = 应用内 + Obsidian 双写）
 *  真实取值在弹窗打开时由 preferredChannel() 覆写，此处只是兜底初值 */
const genChannel = ref<ReportChannel>('app')
/** 生成进行中（按钮禁用 + 文案切换） */
const generating = ref(false)
/** 生成结果提示（成功描述或错误信息） */
const genMsg = ref('')
/** 生成提示是否为错误态 */
const genError = ref(false)

/** 报告类型选项 */
const reportKindOptions: SelectOption[] = [
  { value: 'daily', label: '日报' },
  { value: 'weekly', label: '周报' }
]

/** 覆盖天数选项：覆盖常见的补报场景（天数上限由后端校验为 90） */
const genDayOptions: SelectOption[] = [
  { value: 1, label: '近 1 天' },
  { value: 3, label: '近 3 天' },
  { value: 7, label: '近 7 天' },
  { value: 14, label: '近 14 天' },
  { value: 30, label: '近 30 天' }
]

/** 输出通道选项 */
const genChannelOptions: SelectOption[] = [
  { value: 'app', label: '应用内' },
  { value: 'obsidian', label: 'Obsidian' },
  { value: 'both', label: '应用内 + Obsidian' }
]

/**
 * 本次该用哪个通道：上次选择优先，其次按 Vault 是否配置决定
 *
 * 取值规则集中在 `resolveReportChannel`，与「设置 → 自动化」的报告任务表单
 * 共用同一份判断。设置项在 App 启动时已全量缓存，这里同步读取即可。
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
 * 记住用户选择的通道
 *
 * 在**选择时**落库而不是等生成成功：生成失败（例如 Vault 路径填错）时用户的
 * 意图同样值得记住，否则下次还得重选一遍。
 *
 * @param value - 选择器回传的新值（已由 options 限定为合法通道）
 */
function onChannelChange(value: string | number) {
  const next = String(value)
  if (!isReportChannel(next)) return
  settingsStore.setSetting(REPORT_CHANNEL_KEY, next).catch((e) => {
    console.error('保存报告通道偏好失败:', e)
  })
}

/**
 * 手动生成一份报告
 *
 * 与定时任务共用后端实现，因此无需先创建任务。成功后 store 会刷新报告列表，
 * 并在报告落入应用目录时直接把内容读进查看器，这里只负责展示结果描述与错误
 * （典型错误：Obsidian 通道未配置 Vault 路径）。
 */
async function handleGenerate() {
  generating.value = true
  genMsg.value = ''
  genError.value = false
  try {
    genMsg.value = await researchStore.generateReport(
      genKind.value,
      genDays.value,
      genChannel.value
    )
    // 仅 obsidian 通道：报告只落在 Vault，应用目录没有新文件。此时若继续展示
    // 上一份报告正文，会让人误以为"这次生成没生效"，因此收起正文回到列表，
    // 由上面的结果描述说明落盘位置。
    if (genChannel.value === 'obsidian') {
      researchStore.closeReport()
    }
    // 生成成功即说明报告页工作正常，清掉此前读取失败留下的提示
    reportError.value = ''
  } catch (e) {
    genError.value = true
    genMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    generating.value = false
  }
}

/**
 * 读取一份报告内容
 *
 * @param name - 报告文件名
 */
async function handleReadReport(name: string) {
  reportError.value = ''
  try {
    await researchStore.readReport(name)
  } catch (e) {
    reportError.value = e instanceof Error ? e.message : String(e)
  }
}

/**
 * 字节数的友好展示
 *
 * @param size - 字节数
 * @returns 如 "1.2 KB"
 */
function formatSize(size: number): string {
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / 1024 / 1024).toFixed(1)} MB`
}

// 每次切入本页签都重拉报告列表（对应拆分前 switchToReports 的行为）
watch(
  () => props.active,
  async vis => {
    if (!vis) return
    reportError.value = ''
    reportsLoading.value = true
    try {
      await researchStore.loadReports()
    } catch (e) {
      reportError.value = e instanceof Error ? e.message : String(e)
    } finally {
      reportsLoading.value = false
    }
  }
)

// 弹窗每次打开：通道偏好重新取值；生成提示是"上一次操作"的残留，一并清掉
watch(
  () => props.open,
  val => {
    if (!val) return
    genChannel.value = preferredChannel()
    genMsg.value = ''
    genError.value = false
  }
)

/**
 * 监听报告类型变化：把覆盖天数带回该类型的默认窗口
 *
 * 与后端 `ReportKind::default_days` 的口径一致（日报 1 天 / 周报 7 天）。
 * 只是省一次手改，改完仍可继续调整天数。
 */
watch(genKind, kind => {
  genDays.value = kind === 'weekly' ? 7 : 1
})
</script>

<style scoped>
/* 手动生成工具条：三个参数下拉 + 生成按钮 + 结果提示。
   窄宽度下允许换行，避免选项被挤出可视区。 */
.report-toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
  padding-bottom: var(--sp-3);
  border-bottom: 1px solid var(--border);
}

/* 报告列表 */
.report-list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}
.report-item {
  display: flex;
  align-items: center;
  gap: var(--sp-15);
  padding: var(--sp-15) var(--sp-3);
  background: var(--fill);
  border-radius: var(--r-md);
  cursor: pointer;
  transition: background 0.15s;
}
.report-item:hover {
  background: var(--hover-bg);
}
.report-item.active {
  outline: 1px solid var(--primary);
}
.report-icon { font-size: var(--fs-base); }
.report-name {
  flex: 1;
  font-size: var(--fs-md);
  color: var(--text-primary);
}
.report-meta {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

/* 报告正文：等宽 + 保留换行（无 markdown 渲染依赖的第一版展示） */
.report-content {
  margin: 0;
  padding: var(--sp-3);
  background: var(--fill);
  border-radius: var(--r-md);
  font-family: ui-monospace, 'SF Mono', Consolas, monospace;
  font-size: var(--fs-sm);
  line-height: 1.7;
  color: var(--text-secondary);
  white-space: pre-wrap;
  word-break: break-word;
}

/* 返回列表按钮：不应被 flex 拉伸居中 */
.btn-back {
  align-self: flex-start;
}
</style>
