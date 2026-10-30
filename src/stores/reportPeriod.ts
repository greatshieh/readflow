/**
 * 周期报告 Store（Pinia）
 *
 * # 职责
 * 承载「周期报告」（日报 / 周报 / 月报）的全部前端状态与后端调用：
 * - 报告页签内的**分段**（周期报告 / 研究报告）——放在 store 而不是组件里，
 *   因为分段控件由外壳 `ResearchModal` 渲染（它必须位于 `.m-body` 之外，
 *   否则会与内容区争同一个滚动容器），而内容由页签组件渲染，两者要共享同一份真源；
 * - 当前浏览器周期（粒度 / 口径 / 锚点日期）与列表、阅读中的报告；
 * - 生成流程（进度、取消、失败原因）；
 * - **来源上下文 `B1`**（从报告点进文章后，正文栏顶部那条返回条的依据）。
 *
 * # 为什么 `B1` 必须放 Store
 * 弹窗内部状态能保留，靠的是"`BaseModal` 无 `v-if` + 三个页签 `v-show` 常驻"这个
 * **隐式契约**。哪天有人为性能给弹窗加 `v-if`，返回会静默失效且不报错。
 * `B1` 显式存 store，弹窗被销毁重建也能恢复；组件在跳走时就已卸载，放组件里必丢。
 *
 * # 边界
 * 这里只包 `invoke` 与状态编排，不写业务规则：窗口计算在 `utils/periodWindow.ts`，
 * 文章筛选 / AI 归纳 / 校验全在 Rust（`research/period.rs`）。
 */

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { newRequestId } from '@/utils/requestId'
import {
  isCurrentPeriod,
  naturalRange,
  periodKindLabel,
  periodLabel,
  shiftPeriod,
  type PeriodKind,
  type WindowMode,
} from '@/utils/periodWindow'

/** 列表/阅读共用的报告摘要（与后端 `PeriodSummary` 一一对应） */
export interface PeriodSummary {
  id: number
  kind: string
  windowMode: string
  periodStart: string
  periodEnd: string
  title: string
  lead: string
  articleCount: number
  topicCount: number
  feedCount: number
  isRead: boolean
  status: string
  error: string
  createdAt: string
  model: string
}

/** 引用源条目（与后端 `PeriodSource` 一一对应） */
export interface PeriodSource {
  refNo: number
  articleId: number | null
  title: string
  feedName: string
  published: string
  url: string
  topicNo: number | null
}

/** 阅读页载荷 */
export interface PeriodDetail {
  summary: PeriodSummary
  markdown: string
  sources: PeriodSource[]
}

/** 来源上下文：从某份报告的第 n 条引用跳到文章后，正文栏据此显示返回条 */
export interface SourceContext {
  /** 来源报告 ID */
  reportId: number
  /** 来源报告的展示名（如「10月30日 · 日报」） */
  reportTitle: string
  /** 刚才点的那条引用编号；直接跳文章时为 null */
  refNo: number | null
}

/** 生成阶段（与后端 `report:progress` 的 stage 取值对应） */
export type GenStage = 'collect' | 'cluster' | 'compose' | 'sources'

/** 阶段中文文案 */
const STAGE_LABELS: Record<GenStage, string> = {
  collect: '收集窗口内文章',
  cluster: '归并主题',
  compose: '撰写要点与主题',
  sources: '生成引用源清单',
}

/** 阶段展示顺序（进度条按此推进） */
const STAGE_ORDER: GenStage[] = ['collect', 'cluster', 'compose', 'sources']

export const useReportPeriodStore = defineStore('reportPeriod', () => {
  // ─── 页签内分段 ────────────────────────────────────────────────────────────

  /** 报告页签内的分段：`period` = 周期报告（默认）；`research` = 原来的研究报告 */
  const segment = ref<'period' | 'research'>('period')

  /**
   * 切到指定分段
   *
   * @param next - 目标分段
   */
  function setSegment(next: 'period' | 'research') {
    segment.value = next
  }

  // ─── 浏览周期 ──────────────────────────────────────────────────────────────

  /** 当前粒度 */
  const kind = ref<PeriodKind>('daily')

  /** 当前窗口口径 */
  const windowMode = ref<WindowMode>('natural')

  /** 自然周期的锚点日期（该期内的任意一天，翻页只改它） */
  const anchor = ref<Date>(new Date())

  /** 自定义区间的起止（date 输入框的 `YYYY-MM-DD`） */
  const customStart = ref('')
  const customEnd = ref('')

  /** 当前窗口的展示标签 */
  const windowLabel = computed(() =>
    windowMode.value === 'natural'
      ? periodLabel(kind.value, anchor.value)
      : customStart.value && customEnd.value
        ? `${customStart.value} – ${customEnd.value}`
        : '未选择区间',
  )

  /** 「下一期」是否可用：已在当前期就禁用（未来还没有文章） */
  const canGoNext = computed(
    () => windowMode.value === 'natural' && !isCurrentPeriod(kind.value, anchor.value),
  )

  /**
   * 当前窗口的实际起止时间戳（RFC3339）
   *
   * 自定义区间按**本地日**解释：`start` 取当天 00:00，`end` 取次日 00:00（排他）。
   * 这里不用 `toISOString().slice(0,10)` 拼日期——那是 UTC 日期，东八区会差一天。
   *
   * @returns `{ start, end }` 字符串；自定义区间没填全时返回 null
   */
  function currentRange(): { start: string; end: string } | null {
    if (windowMode.value === 'natural') {
      const r = naturalRange(kind.value, anchor.value)
      return { start: r.start.toISOString(), end: r.end.toISOString() }
    }
    if (!customStart.value || !customEnd.value) return null
    const [ys, ms, ds] = customStart.value.split('-').map(Number)
    const [ye, me, de] = customEnd.value.split('-').map(Number)
    if (!ys || !ms || !ds || !ye || !me || !de) return null
    const start = new Date(ys, ms - 1, ds, 0, 0, 0, 0)
    // 用户选的是"截止到哪一天"，落库上界要加一天（排他）
    const end = new Date(ye, me - 1, de + 1, 0, 0, 0, 0)
    return { start: start.toISOString(), end: end.toISOString() }
  }

  /**
   * 翻页
   *
   * @param delta - 翻几期（负数为向前）
   */
  function shift(delta: number) {
    if (windowMode.value !== 'natural') return
    anchor.value = shiftPeriod(kind.value, anchor.value, delta)
    void loadCoverage()
  }

  /**
   * 切换粒度
   *
   * 切粒度时把锚点重置回"当前期"：从月报切回日报时若保留上个月的锚点，
   * 用户会看到 30 天前的日报，与"切粒度"的意图不符。
   *
   * @param next - 目标粒度
   */
  function setKind(next: PeriodKind) {
    kind.value = next
    anchor.value = new Date()
    void refresh()
  }

  /**
   * 切换窗口口径
   *
   * @param next - natural / custom
   */
  function setWindowMode(next: WindowMode) {
    windowMode.value = next
    void refresh()
  }

  // ─── 列表 ──────────────────────────────────────────────────────────────────

  /** 当前粒度的报告列表 */
  const list = ref<PeriodSummary[]>([])
  /** 列表加载中 */
  const listLoading = ref(false)
  /** 列表错误 */
  const listError = ref('')

  /**
   * 拉取列表
   *
   * @throws 不向外抛出：错误写入 `listError`
   */
  async function loadList(): Promise<void> {
    listLoading.value = true
    listError.value = ''
    try {
      list.value = await invoke<PeriodSummary[]>('report_period_list', {
        kind: kind.value,
        limit: 50,
      })
    } catch (e) {
      listError.value = e instanceof Error ? e.message : String(e)
    } finally {
      listLoading.value = false
    }
  }

  /** 当前窗口内的素材量（空态用） */
  const coverage = ref<{ articles: number; feeds: number } | null>(null)

  /**
   * 查询当前窗口的素材量
   *
   * 自定义区间没填全时置空——不拿默认窗口的数字冒充。
   */
  async function loadCoverage(): Promise<void> {
    const range = currentRange()
    if (!range) {
      coverage.value = null
      return
    }
    try {
      coverage.value = await invoke<{ articles: number; feeds: number }>(
        'report_period_coverage',
        {
          kind: kind.value,
          windowMode: windowMode.value,
          start: range.start,
          end: range.end,
        },
      )
    } catch {
      // 素材量只是提示，失败时静默（真正的门槛在后端 generate，那里会给出明确错误）
      coverage.value = null
    }
  }

  /** 当前窗口已存在的报告（没有则走空态） */
  const currentReport = computed<PeriodSummary | null>(() => {
    const range = currentRange()
    if (!range) return null
    const start = new Date(range.start).getTime()
    return (
      list.value.find(
        (r) =>
          r.kind === kind.value &&
          r.windowMode === windowMode.value &&
          new Date(r.periodStart).getTime() === start,
      ) ?? null
    )
  })

  // ─── 阅读 ──────────────────────────────────────────────────────────────────

  /** 正在阅读的报告（null = 在列表/空态） */
  const detail = ref<PeriodDetail | null>(null)
  /** 阅读页加载中 */
  const detailLoading = ref(false)

  /**
   * 打开一份报告
   *
   * @param id - 报告 ID
   */
  async function openReport(id: number): Promise<void> {
    detailLoading.value = true
    try {
      detail.value = await invoke<PeriodDetail>('report_period_get', { id })
      // 打开即已读：与文章"打开即已读"一致，省掉一次手动标记
      if (!detail.value.summary.isRead) {
        const target = list.value.find((r) => r.id === id)
        if (target) target.isRead = true
        await invoke('report_period_set_read', { id, read: true }).catch(() => {
          // 已读失败不阻塞阅读（下次打开还会再标一次）
        })
      }
    } finally {
      detailLoading.value = false
    }
  }

  /** 回到列表（不清锚点，用户翻页后回来还在同一期） */
  function closeReport() {
    detail.value = null
  }

  /**
   * 删除一份报告
   *
   * @param id - 报告 ID
   */
  async function removeReport(id: number): Promise<void> {
    await invoke('report_period_delete', { id })
    list.value = list.value.filter((r) => r.id !== id)
    if (detail.value?.summary.id === id) detail.value = null
  }

  // ─── 生成 ──────────────────────────────────────────────────────────────────

  /** 生成中（阻塞态：期间弹窗不可关闭，见 §4.5） */
  const generating = ref(false)
  /** 当前阶段 */
  const stage = ref<GenStage>('collect')
  /** 阶段内已完成 / 总数（0 表示不确定） */
  const stageDone = ref(0)
  const stageTotal = ref(0)
  /** 生成失败原因 */
  const genError = ref('')
  /** 本次生成的请求 ID（取消键） */
  let genRequestId = ''

  /** 阶段文案 */
  const stageLabel = computed(() => STAGE_LABELS[stage.value])
  /** 进度百分比（阶段数为主、阶段内计数为辅） */
  const progressPercent = computed(() => {
    const base = STAGE_ORDER.indexOf(stage.value)
    if (base < 0) return 0
    const within = stageTotal.value > 0 ? stageDone.value / stageTotal.value : 0
    return Math.min(100, Math.round(((base + within) / STAGE_ORDER.length) * 100))
  })

  /** 进度事件解绑函数（只在生成期间挂监听） */
  let unlisten: UnlistenFn | null = null

  /**
   * 订阅后端进度事件
   *
   * 只在生成期间订阅：常驻监听会让"别的窗口/上一次请求"的事件也打到当前界面。
   */
  async function subscribeProgress(): Promise<void> {
    if (unlisten) return
    unlisten = await listen<{ stage: string; done: number; total: number }>(
      'report:progress',
      (ev) => {
        const next = ev.payload.stage as GenStage
        if (!STAGE_ORDER.includes(next)) return
        stage.value = next
        stageDone.value = ev.payload.done
        stageTotal.value = ev.payload.total
      },
    )
  }

  /**
   * 取消订阅
   */
  function unsubscribeProgress(): void {
    unlisten?.()
    unlisten = null
  }

  /**
   * 生成当前窗口的报告
   *
   * 成功后直接打开新报告（不要用户再点一次列表）。
   *
   * @returns 生成的报告 ID
   * @throws 窗口非法、无文章、AI 未配置或模型失败时抛出；错误同时写入 `genError`
   */
  async function generate(): Promise<number> {
    const range = currentRange()
    if (!range) throw new Error('请先选择完整的自定义区间')
    generating.value = true
    genError.value = ''
    stage.value = 'collect'
    stageDone.value = 0
    stageTotal.value = 0
    genRequestId = newRequestId()
    await subscribeProgress()
    try {
      const id = await invoke<number>('report_period_generate', {
        kind: kind.value,
        windowMode: windowMode.value,
        start: range.start,
        end: range.end,
        providerId: '',
        requestId: genRequestId,
      })
      await loadList()
      await openReport(id)
      return id
    } catch (e) {
      genError.value = e instanceof Error ? e.message : String(e)
      // 失败/取消都要重拉列表：失败时库里可能留了一行 `status='failed'`
      // （占位行在调模型前就已写入），不刷新用户看不到它，下次点生成会莫名其妙被覆盖
      await loadList().catch(() => {})
      throw e
    } finally {
      unsubscribeProgress()
      generating.value = false
    }
  }

  /**
   * 取消进行中的生成
   *
   * 只发取消信号；真正的收尾（置 failed）由被取消的那次 generate 自己完成。
   */
  async function cancelGenerate(): Promise<void> {
    if (!genRequestId) return
    await invoke('report_period_cancel', { requestId: genRequestId }).catch(() => {
      // 取消信号丢失不至于让界面卡死：请求本身仍会自然结束
    })
  }

  // ─── 来源上下文 B1 ─────────────────────────────────────────────────────────

  /** 当前来源上下文（null = 正文栏不显示返回条） */
  const sourceContext = ref<SourceContext | null>(null)
  /** 待打开的周期报告 ID（来源条点了返回、但弹窗已关闭时用） */
  const pendingOpenId = ref<number | null>(null)

  /**
   * 记录来源（点「打开全文」时先写，后面任何一步失败至少还回得去）
   *
   * @param ctx - 来源上下文
   */
  function setSourceContext(ctx: SourceContext) {
    sourceContext.value = ctx
  }

  /** 清空来源上下文（点 ✕ 或返回成功后） */
  function clearSourceContext() {
    sourceContext.value = null
  }

  /**
   * 请求打开某份报告（来源条返回时用）
   *
   * 弹窗可能是关着的，这里只登记意图并派发与「研究工作台」同一个 window 事件，
   * 由 `FeedPanel` 负责把弹窗打开、`ResearchModal` 负责切到报告页签并读这份。
   *
   * @param id - 报告 ID
   */
  function requestOpen(id: number) {
    pendingOpenId.value = id
    segment.value = 'period'
    window.dispatchEvent(new CustomEvent('open-research'))
  }

  /** 消费一次待打开意图（读取后立即清空，避免重复触发） */
  function consumePendingOpen(): number | null {
    const id = pendingOpenId.value
    pendingOpenId.value = null
    return id
  }

  /**
   * 打开报告页签并定位到指定报告（供外壳在弹窗打开后调用）
   *
   * @param id - 报告 ID
   */
  async function focusReport(id: number): Promise<void> {
    await loadList()
    const target = list.value.find((r) => r.id === id)
    if (!target) {
      // 报告已被删除或覆盖：不跳空白页，退到列表并提示
      throw new Error('该报告已不存在')
    }
    // 让浏览周期跟上这份报告的窗口，否则返回后"当前期"与正在读的那期不一致
    kind.value = (target.kind as PeriodKind) ?? kind.value
    windowMode.value = (target.windowMode as WindowMode) ?? 'natural'
    anchor.value = new Date(target.periodStart)
    await openReport(id)
  }

  /** 当前粒度对应的一期（供按钮文案「生成 10月30日 日报」） */
  const generateLabel = computed(() => {
    if (windowMode.value === 'custom') {
      return `生成 ${customStart.value || '?'} 至 ${customEnd.value || '?'} 的${periodKindLabel(kind.value)}`
    }
    return `生成 ${periodLabel(kind.value, anchor.value)} ${periodKindLabel(kind.value)}`
  })

  /** 刷新列表 + 素材量（切粒度/口径、打开页签时调用） */
  async function refresh(): Promise<void> {
    await Promise.all([loadList(), loadCoverage()])
  }

  return {
    // 分段
    segment,
    setSegment,
    // 浏览周期
    kind,
    windowMode,
    anchor,
    customStart,
    customEnd,
    windowLabel,
    canGoNext,
    currentRange,
    shift,
    setKind,
    setWindowMode,
    generateLabel,
    // 列表
    list,
    listLoading,
    listError,
    loadList,
    coverage,
    loadCoverage,
    currentReport,
    // 阅读
    detail,
    detailLoading,
    openReport,
    closeReport,
    removeReport,
    focusReport,
    // 生成
    generating,
    stage,
    stageLabel,
    progressPercent,
    stageDone,
    stageTotal,
    genError,
    generate,
    cancelGenerate,
    // 来源上下文
    sourceContext,
    setSourceContext,
    clearSourceContext,
    requestOpen,
    consumePendingOpen,
    refresh,
  }
})
