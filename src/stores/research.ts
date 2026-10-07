/**
 * 研究流水线 Store（Pinia）
 *
 * # 职责
 * 管理结构化研究事件（行业/公司动态）与自动化任务（定时提取 / 定时报告）：
 * - 事件时间线查询（按时间窗 / 按实体筛选）
 * - 单篇 / 批量手动提取（补跑场景）
 * - 自动化任务的增删改查、启停、立即运行
 * - 应用内报告（日报 / 周报 Markdown）的**手动生成**、列表与读取
 *   （手动生成与定时任务共用后端实现，无需先创建任务）
 *
 * # 设计意图
 * - **数据落库在 Rust 侧**：所有读写通过 `invoke()` 调用后端命令，
 *   前端仅负责状态管理与 UI 交互（规则 1）。
 * - **任务状态回写展示**：任务的 last_status / last_message 由后端执行器维护，
 *   前端每次操作后重新拉取列表即可看到最新健康度。
 *
 * # Tauri invoke 参数约定
 * 前端传参一律使用 **camelCase**，Rust 端以 snake_case 接收，Tauri 自动转换。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  ResearchEvent,
  ResearchEventWithContext,
  AutomationTask,
  ReportMeta,
  ReportGenResult,
  EntityGraph,
  EventSentiment,
  FinancialData,
  CrossReferenceEntry
} from '@/types'

export const useResearchStore = defineStore('research', () => {
  // ─── 事件时间线 ────────────────────────────────────────────────────────────

  /** 带上下文的事件列表（含实体名与来源文章标题/链接） */
  const events = ref<ResearchEventWithContext[]>([])

  /** 事件加载中 */
  const eventsLoading = ref(false)

  // ─── 自动化任务 ────────────────────────────────────────────────────────────

  /** 任务列表缓存 */
  const tasks = ref<AutomationTask[]>([])

  /** 任务加载中 */
  const tasksLoading = ref(false)

  /** 正在"立即运行"的任务 ID 集合（按钮 spinner 状态） */
  const runningTaskIds = ref<Set<number>>(new Set())

  // ─── 报告 ──────────────────────────────────────────────────────────────────

  /** 报告文件列表 */
  const reports = ref<ReportMeta[]>([])

  /** 当前打开的报告内容（空串 = 未打开任何报告） */
  const reportContent = ref('')

  /** 当前打开的报告文件名（用于高亮列表项） */
  const activeReportName = ref('')

  /**
   * 拉取事件时间线
   *
   * @param days - 时间窗口天数（0 = 全部）
   * @param entityId - 实体筛选（不传 = 全部实体）
   */
  async function loadEvents(days: number, entityId?: number) {
    eventsLoading.value = true
    try {
      events.value = await invoke<ResearchEventWithContext[]>('research_events_timeline', {
        days,
        entityId: entityId ?? null
      })
    } finally {
      eventsLoading.value = false
    }
  }

  /**
   * 获取某篇文章已提取的事件（阅读区展示用）
   *
   * @param articleId - 文章 ID
   * @returns 该文章的事件列表
   */
  async function loadEventsByArticle(articleId: number): Promise<ResearchEvent[]> {
    return invoke<ResearchEvent[]>('research_events_by_article', { articleId })
  }

  /**
   * 对单篇文章手动提取研究事件（阅读场景即时触发）
   *
   * @param articleId - 文章 ID
   * @returns 新增事件条数
   * @throws 后端失败时原样抛出
   */
  async function extractArticle(articleId: number): Promise<number> {
    return invoke<number>('research_extract_article', { articleId })
  }

  /**
   * 删除单条研究事件
   *
   * 后端做物理删除且幂等（事件不存在也返回成功），因此前端无需处理
   * "记录已被删掉"的分支。删除成功后就地从 `events` 中移除该条，
   * 而不是重新查询整个时间线——后者会让已展开的分组闪烁重排。
   *
   * @param id - 事件 ID
   * @throws 后端删除失败时原样抛出
   */
  async function deleteEvent(id: number) {
    await invoke('research_event_delete', { eventId: id })
    events.value = events.value.filter((e) => e.id !== id)
  }

  /**
   * 手动补跑一次批量提取（程序未全天运行时的补漏入口）
   *
   * @returns 结果描述（处理篇数与新增事件数）
   * @throws 后端失败时原样抛出
   */
  async function extractBatch(): Promise<string> {
    return invoke<string>('research_extract_batch')
  }

  /**
   * 拉取全部自动化任务（含最近运行状态）
   */
  async function loadTasks() {
    tasksLoading.value = true
    try {
      tasks.value = await invoke<AutomationTask[]>('automation_tasks_list')
    } finally {
      tasksLoading.value = false
    }
  }

  /**
   * 新建自动化任务
   *
   * @param name - 任务名称
   * @param taskType - 任务类型（batch_extract / daily_report / weekly_report）
   * @param cronExpr - cron 表达式
   * @param config - JSON 配置字符串
   * @param enabled - 是否启用
   */
  async function createTask(
    name: string,
    taskType: string,
    cronExpr: string,
    config: string,
    enabled: boolean
  ) {
    await invoke('automation_tasks_create', {
      name,
      taskType,
      cronExpr,
      config,
      enabled
    })
    await loadTasks()
  }

  /**
   * 更新任务的可编辑字段（None 的字段保持不变）
   *
   * @param id - 任务 ID
   * @param patch - 待更新字段（camelCase 键名）
   */
  async function updateTask(id: number, patch: { name?: string; cronExpr?: string; config?: string }) {
    await invoke('automation_tasks_update', { id, ...patch })
    await loadTasks()
  }

  /**
   * 删除任务
   *
   * @param id - 任务 ID
   */
  async function deleteTask(id: number) {
    await invoke('automation_tasks_delete', { id })
    await loadTasks()
  }

  /**
   * 启用 / 停用任务
   *
   * @param id - 任务 ID
   * @param enabled - 目标状态
   */
  async function setTaskEnabled(id: number, enabled: boolean) {
    await invoke('automation_tasks_set_enabled', { id, enabled })
    await loadTasks()
  }

  /**
   * 立即运行一个任务（手动触发，与 cron 走同一执行器）
   *
   * 运行中状态由 `runningTaskIds` 标记，供按钮显示 spinner；
   * 完成后重新拉取任务列表以刷新健康度徽标。
   *
   * @param id - 任务 ID
   * @returns 执行结果描述
   * @throws 后端失败时原样抛出（任务不存在 / 已在运行 / 执行失败）
   */
  async function runTaskNow(id: number): Promise<string> {
    runningTaskIds.value.add(id)
    // 用新 Set 触发响应式更新（Set 的 add 不被 Vue3 ref 深度侦测）
    runningTaskIds.value = new Set(runningTaskIds.value)
    try {
      const msg = await invoke<string>('automation_run_now', { taskId: id })
      return msg
    } finally {
      runningTaskIds.value.delete(id)
      runningTaskIds.value = new Set(runningTaskIds.value)
      await loadTasks()
    }
  }

  /**
   * 拉取应用内报告文件列表
   */
  async function loadReports() {
    reports.value = await invoke<ReportMeta[]>('research_reports_list')
  }

  /**
   * 读取指定报告的 Markdown 内容
   *
   * @param name - 报告文件名
   * @returns 报告全文
   */
  async function readReport(name: string): Promise<string> {
    const content = await invoke<string>('research_report_read', { name })
    reportContent.value = content
    activeReportName.value = name
    return content
  }

  /**
   * 手动生成一份研究报告（不依赖任何自动化任务）
   *
   * 后端与定时任务共用同一份生成逻辑，因此内容与格式和自动生成的报告一致。
   * 生成后刷新报告列表，并在报告落入应用目录时直接把内容读进查看器——
   * Obsidian 单通道的文件不在应用目录内，只刷新列表、不尝试读取。
   *
   * @param kind - 报告类型：daily / weekly
   * @param days - 覆盖天数（1-90）
   * @param channel - 输出通道：app（应用内）/ obsidian（仅 Vault）/
   *                  both（应用内 + Vault 双写）
   * @returns 后端的结果描述（含落盘位置与事件统计）
   * @throws 参数非法、Vault 未配置或生成失败时原样抛出
   */
  async function generateReport(
    kind: 'daily' | 'weekly',
    days: number,
    channel: 'app' | 'obsidian' | 'both'
  ): Promise<string> {
    const res = await invoke<ReportGenResult>('research_report_generate', {
      kind,
      days,
      channel
    })
    await loadReports()
    if (res.file_name) {
      await readReport(res.file_name)
    }
    return res.message
  }

  /** 关闭当前打开的报告 */
  function closeReport() {
    reportContent.value = ''
    activeReportName.value = ''
  }

  // ─── 实体关系图 ────────────────────────────────────────────────────────────

  /**
   * 当前关系图（null = 尚未加载过，用于区分「空图」与「还没查」）
   *
   * 空图（`nodes` 为空数组）也代表一次有效结果，界面应显示引导语而非骨架屏，
   * 所以这里用 `null` 而不是空数组表示未加载状态。
   */
  const graph = ref<EntityGraph | null>(null)

  /** 关系图加载中 */
  const graphLoading = ref(false)

  /**
   * 关系图时间窗（天数，0 = 全部文章），默认「全部」
   *
   * 参数放 store 而非组件本地：关系图有两处展示位（研究工作台的图页签与
   * 主屏关系图视图），两处必须用同一份参数与数据，否则会互相漂移。
   * 默认「全部」而不是 30 天：关系图的价值在于看长期结构，窗口太窄会让
   * 大部分实体失去共现对象，图上只剩零星几条线，反而看不出东西。
   */
  const graphDays = ref<number>(0)

  /** 关系图最小共现数（低于该值的边不画），默认 1 = 保留全部共现关系 */
  const graphMinWeight = ref<number>(1)

  /** 关系图计算错误消息（空串 = 无错误），写入方是 reloadGraph */
  const graphError = ref('')

  /** 关系图时间窗选项（弹窗页签与主屏视图共用） */
  const graphDayOptions: { value: number; label: string }[] = [
    { value: 7, label: '近 7 天' },
    { value: 30, label: '近 30 天' },
    { value: 90, label: '近 90 天' },
    { value: 0, label: '全部' },
  ]

  /** 关系图最小共现数选项（弹窗页签与主屏视图共用） */
  const graphWeightOptions: { value: number; label: string }[] = [
    { value: 1, label: '共现 ≥ 1 篇' },
    { value: 2, label: '共现 ≥ 2 篇' },
    { value: 3, label: '共现 ≥ 3 篇' },
    { value: 5, label: '共现 ≥ 5 篇' },
  ]

  /**
   * 拉取实体共现图
   *
   * @param days - 时间窗天数（0 = 全部文章）
   * @param minWeight - 最小共现数（1 = 保留全部共现关系）
   * @returns 无返回值；结果写入 `graph`
   * @throws 后端调用失败时向上抛出，由调用方提示（此处不吞异常，
   *         否则界面会停在旧图上，让人误以为"点了没反应"）
   */
  async function loadGraph(days: number, minWeight: number): Promise<void> {
    graphLoading.value = true
    try {
      graph.value = await invoke<EntityGraph>('entity_graph', { days, minWeight })
    } finally {
      graphLoading.value = false
    }
  }

  /**
   * 按当前共享参数重算关系图（弹窗页签与主屏视图统一走这里）
   *
   * 参数读 `graphDays` / `graphMinWeight`，失败信息写入 `graphError`——
   * 吞掉异常会让用户看到一张不动的旧图却不知为何。
   *
   * @returns 无返回值；成功刷新 `graph`，失败刷新 `graphError`
   */
  async function reloadGraph(): Promise<void> {
    graphError.value = ''
    try {
      await loadGraph(graphDays.value, graphMinWeight.value)
    } catch (e) {
      graphError.value = `关系图计算失败：${e instanceof Error ? e.message : String(e)}`
    }
  }

  // ─── 情感分析与财务数据 ──────────────────────────────────────────────────

  /**
   * 获取事件的情感分析数据
   *
   * @param eventId - 事件 ID
   * @returns 情感分析数据，不存在时返回 null
   */
  async function getEventSentiment(eventId: number): Promise<EventSentiment | null> {
    const result = await invoke<EventSentiment | null>('event_sentiment_get', { eventId })
    return result
  }

  /**
   * 获取文章关联的财务数据
   *
   * @param articleId - 文章 ID
   * @returns 财务指标列表
   */
  async function getFinancialData(articleId: number): Promise<FinancialData[]> {
    return invoke<FinancialData[]>('financial_data_list', { articleId })
  }

  /**
   * 获取实体关联的财务数据
   *
   * @param entityId - 实体 ID
   * @returns 财务指标列表（按时间倒序）
   */
  async function getFinancialDataByEntity(entityId: number): Promise<FinancialData[]> {
    return invoke<FinancialData[]>('financial_data_list_by_entity', { entityId })
  }

  /**
   * 获取实体的最新情感分析数据
   *
   * @param entityId - 实体 ID
   * @returns 情感分析数据，不存在时返回 null
   */
  async function getEntitySentiment(entityId: number): Promise<EventSentiment | null> {
    const result = await invoke<EventSentiment | null>('entity_sentiment', { entityId })
    return result
  }

  /**
   * 获取某实体的多源交叉验证条目（同一实体在不同订阅源中的报道）
   *
   * @param entityId - 实体 ID
   * @param days - 时间窗天数（0 = 全部）
   * @returns 按事件入库时间倒序的交叉验证条目列表
   */
  async function getCrossReference(
    entityId: number,
    days: number
  ): Promise<CrossReferenceEntry[]> {
    return invoke<CrossReferenceEntry[]>('research_cross_reference', { entityId, days })
  }

  return {
    events,
    eventsLoading,
    tasks,
    tasksLoading,
    runningTaskIds,
    reports,
    reportContent,
    activeReportName,
    graph,
    graphLoading,
    graphDays,
    graphMinWeight,
    graphError,
    graphDayOptions,
    graphWeightOptions,
    loadEvents,
    loadEventsByArticle,
    extractArticle,
    extractBatch,
    deleteEvent,
    loadTasks,
    createTask,
    updateTask,
    deleteTask,
    setTaskEnabled,
    runTaskNow,
    loadReports,
    generateReport,
    readReport,
    closeReport,
    loadGraph,
    reloadGraph,
    getEventSentiment,
    getFinancialData,
    getFinancialDataByEntity,
    getEntitySentiment,
    getCrossReference
  }
})
