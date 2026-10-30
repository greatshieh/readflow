<template>
  <!-- ═══ Tab 2：报告（周期报告 / 研究报告 两段） ═══ -->
  <!-- 根节点保留 `.m-body`（外壳的 scoped 规则按这个类名给内边距与滚动），
       再挂 `m-body-locked` 把**滚动交给各段自己**：周期报告是"头部固定 + 双列各自滚动"，
       若外层也在滚，正文与引用源栏会各滚各的，头部还会被滚走。 -->
  <div class="m-body m-body-locked">
    <!-- 段一：周期报告 -->
    <div v-show="store.segment === 'period'" class="seg-pane">
      <PeriodReports :open="open" @close="emit('close')" />
    </div>

    <!-- 段二：原来的研究报告（逻辑一行未改，只是被包进分段容器） -->
    <div v-show="store.segment === 'research'" class="seg-pane seg-pane-scroll">
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
        <div class="report-content rc" v-html="renderedReport"></div>
      </template>

      <!-- 报告列表视图 -->
      <template v-else>
        <div v-if="researchStore.reports.length === 0 && !reportsLoading" class="empty-state">
          <p>暂无报告</p>
          <p class="hint">
            上方「周期报告」按时间自动汇总全部文章；这里的「研究报告」按研究实体聚合事件生成
          </p>
        </div>
        <div v-else class="report-list">
          <div
            v-for="r in researchStore.reports"
            :key="r.name"
            class="report-item"
            :class="{ active: r.name === researchStore.activeReportName }"
            role="button"
            tabindex="0"
            :aria-label="`打开 ${r.name}`"
            @click="handleReadReport(r.name)"
            @keydown="onRowKey($event, r.name)"
          >
            <svg class="report-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M14 3v4a1 1 0 0 0 1 1h4" />
              <path d="M5 3h9l5 5v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" />
            </svg>
            <span class="report-name">{{ r.name }}</span>
            <span class="report-meta">{{ r.modified }} · {{ formatSize(r.size) }}</span>
            <button
              class="report-export"
              type="button"
              title="导出"
              aria-label="导出这份报告"
              @click.stop="openExport(r, $event)"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                <polyline points="7 10 12 15 17 10" />
                <line x1="12" y1="15" x2="12" y2="3" />
              </svg>
            </button>
          </div>
        </div>
        <span v-if="reportError" class="extract-msg error">{{ reportError }}</span>
      </template>

      <!-- 导出菜单：与周期报告共用同一个组件（两类报告一条导出管线） -->
      <ReportExportMenu
        v-if="exportTarget && exportAnchor"
        :anchor="exportAnchor"
        :target="exportTarget"
        @close="closeExport"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 研究工作台 · 报告页签
 *
 * # 职责
 * 一个页签装**两段**：周期报告（默认）与研究报告。
 * - 周期报告段：`components/report/PeriodReports.vue`（时间驱动，汇总全部文章）；
 * - 研究报告段：本文件里的原实现（事件驱动，按实体分节），逻辑一行未改，
 *   只是被包进分段容器里，并补了两处：导出按钮、列表项的键盘可达。
 *
 * # 分段状态为什么在 store
 * 分段控件由外壳 `ResearchModal` 渲染（必须位于 `.m-body` 之外），而两段内容在这里，
 * 两者要共享同一份真源 —— 见 `stores/reportPeriod.ts` 的 `segment`。
 *
 * # 挂载语义（与外壳的契约）
 * 本页签经 `v-show` 常驻挂载，**不能用 `onMounted` 当"进入页签"信号**：
 * - 每次切入本页签（`active` 翻真）→ 清空读取错误并重拉报告列表；
 * - 弹窗每次打开（`open` 翻真）→ 通道偏好重新取值，并清掉上一次的生成提示。
 */
import { ref, computed, watch } from 'vue'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import PeriodReports from './report/PeriodReports.vue'
import ReportExportMenu from './report/ReportExportMenu.vue'
import { useResearchStore } from '@/stores/research'
import { useSettingsStore } from '@/stores/settings'
import { useReportPeriodStore } from '@/stores/reportPeriod'
import {
  REPORT_CHANNEL_KEY,
  VAULT_PATH_KEY,
  resolveReportChannel,
  isReportChannel,
  type ReportChannel
} from '@/utils/reportChannel'
import { stripFrontmatter } from '@/utils/reportMarkdown'
import { renderMarkdown } from '@/utils/markdown'
import type { ExportTarget } from '@/utils/reportExport'

const props = defineProps<{
  /** 是否为当前显示的页签：切入时重拉报告列表 */
  active: boolean
  /** 弹窗是否打开：打开即重取通道偏好并清掉生成提示 */
  open: boolean
}>()

const emit = defineEmits<{
  /** 请求关闭弹窗（周期报告跳文章时用） */
  (e: 'close'): void
}>()

const researchStore = useResearchStore()
const settingsStore = useSettingsStore()
const store = useReportPeriodStore()

/** 报告列表加载中 */
const reportsLoading = ref(false)

/** 报告读取错误 */
const reportError = ref('')

/**
 * 应用内展示的报告正文
 *
 * 磁盘上的报告带 YAML frontmatter（供 Obsidian 的 dataview 与标签系统消费），
 * 而应用内是渲染展示，直接显示会让一整段元数据挡在正文前面，故只在展示时裁掉，
 * 文件本身保持完整格式。
 */
const reportBody = computed(() => stripFrontmatter(researchStore.reportContent))

/**
 * 应用内展示的报告正文（Markdown 渲染后的安全 HTML）
 *
 * `renderMarkdown` 把标题 / 列表 / callout / 加粗等渲染为 HTML，并对所有文本转义、
 * 仅放行安全标签，可直接 `v-html`。与周期报告**同一个渲染器**，版式因此一致。
 */
const renderedReport = computed(() => renderMarkdown(reportBody.value))

// ========== 导出（两类报告共用管线的入口之一） ==========
/** 导出目标 */
const exportTarget = ref<ExportTarget | null>(null)
/** 菜单锚点 */
const exportAnchor = ref<{ x: number; y: number } | null>(null)

/**
 * 打开导出菜单
 *
 * @param r - 报告文件元信息
 * @param e - 点击事件（取按钮位置做锚点）
 */
function openExport(r: { name: string }, e: MouseEvent) {
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  exportTarget.value = {
    kind: 'research',
    name: r.name,
    label: r.name.replace(/\.md$/i, ''),
  }
  exportAnchor.value = { x: rect.right - 224, y: rect.bottom + 6 }
}

/** 关闭导出菜单 */
function closeExport() {
  exportTarget.value = null
  exportAnchor.value = null
}

/**
 * 列表行的键盘激活
 *
 * `e.target !== e.currentTarget` 早退：行内嵌着导出按钮，
 * 在按钮上按 Enter 会冒泡到行上，不拦就变成"导出的同时把报告也打开"。
 *
 * @param e - 键盘事件
 * @param name - 报告文件名
 */
function onRowKey(e: KeyboardEvent, name: string) {
  if (e.target !== e.currentTarget) return
  if (e.key !== 'Enter' && e.key !== ' ') return
  e.preventDefault()
  void handleReadReport(name)
}

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
 * 手动生成一份研究报告
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
// immediate：弹窗可能一开始就是打开态（外壳用 v-show 常驻），此时 watch 不会自己触发
watch(
  () => props.open,
  val => {
    if (!val) return
    genChannel.value = preferredChannel()
    genMsg.value = ''
    genError.value = false
  },
  { immediate: true }
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
/* 根节点锁定外层滚动，把滚动权交给各段（见模板注释）。
   选择器写两个类是为了压过外壳 `.m-body` 的 `overflow-y:auto`（同层 scoped 规则
   谁后注入不确定，靠特异性取胜才是稳的）。 */
.m-body.m-body-locked {
  overflow: hidden;
}
/* 两段共用的容器：撑满剩余高度，自身不滚动 */
.seg-pane {
  display: flex;
  flex-direction: column;
  flex: 1 1 auto;
  min-height: 0;
  gap: var(--sp-3);
}
/* 研究报告段保持"整段一起滚"——它的列表与正文是一个整体，
   拆成双列反而多一层滚动。 */
.seg-pane-scroll {
  overflow-y: auto;
  overscroll-behavior: contain;
}

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
  transition: background var(--m-tap) var(--m-ease-out);
}
.report-item:hover {
  background: var(--hover-bg);
}
.report-item:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}
.report-item.active {
  outline: 1px solid var(--primary);
}
.report-icon { width: 17px; height: 17px; flex-shrink: 0; color: var(--text-secondary); }
/* 列表行主字段 13/500（与 .entity-name / .article-title 同角色）。
   原先还有一处负字距（-0.01em）：负字距是给拉丁字母排紧的，中文压字距会让笔画粘连 → 删除。 */
.report-name {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-modal-label);
  font-weight: 500;
  line-height: var(--lh-modal-label);
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.report-meta {
  flex-shrink: 0;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  white-space: nowrap;
}
/* 导出钮：常驻但低对比，hover/focus 才显形——旧列表行高 34px，
   加个常驻高对比按钮会让整列噪声变大。 */
.report-export {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: none;
  background: none;
  border-radius: var(--r-sm);
  color: var(--text-tertiary);
  cursor: pointer;
  opacity: 0;
  transition: opacity var(--m-tap) var(--m-ease-out), background var(--m-tap) var(--m-ease-out);
}
.report-export svg { width: 13px; height: 13px; }
.report-item:hover .report-export,
.report-item:focus-within .report-export { opacity: 1; }
.report-export:hover { background: var(--surface); color: var(--text-primary); }
.report-export:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
  opacity: 1;
}

/* 报告正文：排版在 `src/styles/report-content.css`（`.rc`），
   与周期报告阅读区、导出 HTML **共用同一份**，这里只管外框。
   字号与行高也由 `.rc` 提供（全局阶梯 `--fs-md` + 行高 1.65）。 */
.report-content {
  margin: 0;
  padding: var(--sp-4);
  background: var(--fill);
  border-radius: var(--r-md);
  overflow-x: auto;
}

/* 返回列表按钮：不应被 flex 拉伸居中 */
.btn-back {
  align-self: flex-start;
}
</style>
