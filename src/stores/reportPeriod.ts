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
 * - **报告这一侧的"回到原处"**：`pendingRestore` 位置快照 + `focusReport` 打开落点。
 *
 * # 来源记录的归属已经搬走（本次改造）
 * 「从哪跳进原文的、栏上写什么」原本也在这里（旧的来源上下文一节）。现在五种来源
 * （报告 / 时间线 / 论点 / 覆盖 / 实体）共用一条栏，那份快照与 `kind` 强相关，
 * 留在报告域会变成"报告的字段 + 四个别人的字段"混装。所以栏的数据搬到
 * `stores/originTrail.ts`；**报告域的这套位置恢复机制原样保留**，只是入口从
 * "自己读来源快照"变成"由 `originTrail` 把快照递进来"（见 `prepareReportReturn`）。
 *
 * # 边界
 * 这里只包 `invoke` 与状态编排，不写业务规则：窗口计算在 `utils/periodWindow.ts`，
 * 文章筛选 / AI 归纳 / 校验全在 Rust（`research/period.rs`）。
 */

import { acceptHMRUpdate, defineStore } from 'pinia'
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

/** 一条「本期值得跟进」（与后端 `PeriodFollowup` 一一对应，字段名必须一致） */
interface PeriodFollowup {
  id: number
  entityName: string
  reason: string
  entityId: number | null
  tracked: boolean
}

/** 阅读页载荷 */
export interface PeriodDetail {
  summary: PeriodSummary
  markdown: string
  sources: PeriodSource[]
  /** 本期值得跟进（可能为空数组；为空时整段不渲染） */
  followups: PeriodFollowup[]
}

/**
 * 把后端传来的粒度字符串收口成枚举
 *
 * 未知值按日报处理：旧实现里"日"分支是 `return ...日报` 的兜底分支，
 * 收口一次能让下面两个函数对同一个输入给出一致的类别词。
 *
 * @param kind - 后端 `PeriodSummary.kind`（字符串）
 * @returns 三档之一
 */
function normalizePeriodKind(kind: string): PeriodKind {
  return kind === 'weekly' ? 'weekly' : kind === 'monthly' ? 'monthly' : 'daily'
}

/**
 * 期号的**跨度部分**（不含类别词）
 *
 * # 为什么要把它单独拆出来
 * 上下文栏的主文案是「跨度 + 类别词 + 描述」三段，窄栏时**先弃跨度、保类别词**：
 * 「10.26 – 11.01 周报」被压窄时，用户必须还能看出这是周报。
 * 若把类别词拼在跨度里、再靠字符串切割拆开，任何一次格式调整（换分隔符、
 * 改月份写法）都会让切割悄悄失效，且没有编译错误兜底。
 *
 * @param kind - 周期粒度（`PeriodSummary.kind` 原样传入，后端是字符串）
 * @param periodStart - 该期起点（ISO 字符串；按**本地时区**取日期，不做 UTC 换算）
 * @returns 日报「10月30日」/ 周报「10.26 – 11.01」/ 月报「2026年10月」；
 *          日期无法解析时返回空串（调用方据此只显示类别词）
 */
export function compactPeriodSpan(kind: string, periodStart: string): string {
  const start = new Date(periodStart)
  if (Number.isNaN(start.getTime())) return ''
  const p2 = (n: number) => String(n).padStart(2, '0')
  const grain = normalizePeriodKind(kind)
  if (grain === 'weekly') {
    // 窗口左闭右开：本期最后一天 = 起点 + 6
    const last = new Date(start.getTime())
    last.setDate(last.getDate() + 6)
    return `${start.getMonth() + 1}.${p2(start.getDate())} – ${last.getMonth() + 1}.${p2(last.getDate())}`
  }
  if (grain === 'monthly') {
    return `${start.getFullYear()}年${start.getMonth() + 1}月`
  }
  return `${start.getMonth() + 1}月${start.getDate()}日`
}

/**
 * 上下文栏用的紧凑期号标签（跨度 + 类别词）
 *
 * # 为什么不复用 `periodLabel`
 * 后者输出「2026年10月30日 周五」（11 个字）。上下文栏只有 36px 高，还要同时放
 * AI 标题，期号多占的每个字都是从标题里抢来的（DESIGN §5.4）。
 *
 * # 与 `compactPeriodSpan` 的关系
 * 就是"跨度 + 空格 + 类别词"的组合，**三档输出与拆分前逐字一致**
 * （`10月30日 日报` / `10.26 – 11.01 周报` / `2026年10月 月报`）。
 * 栏本身不再用它（它要两段分开控制收缩），保留下来的原因是它是这份格式的
 * 公共出口：导出 / 文档 / 将来的列表卡片引用的是"一整句期号"这件事。
 *
 * @param kind - 周期粒度（`PeriodSummary.kind` 原样传入，后端是字符串）
 * @param periodStart - 该期起点（按**本地时区**取日期）
 * @returns 见上；日期无法解析时只返回类别词（如「日报」）
 */
export function compactPeriodLabel(kind: string, periodStart: string): string {
  const span = compactPeriodSpan(kind, periodStart)
  const word = periodKindLabel(normalizePeriodKind(kind))
  return span ? `${span} ${word}` : word
}

/** `focusReport` 失败的两种类别 */
type FocusReportFailure = 'missing' | 'list'

/**
 * `focusReport` 抛出的错误（带可判定的类别）
 *
 * # 为什么要有类型，而不是让调用方比字符串
 * 调用方（`ResearchModal`）要按失败类别分流：报告**真的**不存在才允许把来源上下文
 * 转成终态 `stale`；列表加载失败这类临时故障绝不能标死（标死等于"失败即失联"，
 * 正是本次改造要消掉的病）。文案会改、类别不会，所以用 `kind` 而不是 `message` 判定。
 */
export class FocusReportError extends Error {
  /** 失败类别：`missing` = 报告已不存在；`list` = 列表加载失败（临时故障） */
  readonly kind: FocusReportFailure

  /**
   * @param kind - 失败类别
   * @param message - 面向用户的文案（会被直接 toast 出来）
   */
  constructor(kind: FocusReportFailure, message: string) {
    super(message)
    this.name = 'FocusReportError'
    this.kind = kind
  }
}

/** 生成阶段（与后端 `report:progress` 的 stage 取值对应） */
type GenStage = 'collect' | 'cluster' | 'compose' | 'sources'

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
   * 把一条「值得跟进」落成真实体（用户点了跟踪）
   *
   * 就地更新 `detail.followups` 里的这一条，不整份重拉：
   * 重拉会让阅读区滚动位置跳动，而用户此刻正停在跟进段落上。
   *
   * @param followupId - `report_followups` 行 ID
   * @returns 实体 ID
   */
  async function trackFollowup(followupId: number): Promise<number> {
    const entityId = await invoke<number>('report_followup_track', { followupId })
    const row = detail.value?.followups.find((f) => f.id === followupId)
    if (row) {
      row.entityId = entityId
      row.tracked = true
    }
    return entityId
  }

  /**
   * 删除一份报告
   *
   * 若删掉的正是某条来源指向的报告：这里**不做任何来源处理**——来源记录的
   * "转失效态"由调用方（`PeriodReports` 的删除分支）调 `originTrail.markStaleIfReport`
   * 完成。放在这里会把报告域与来源域绑成一个环（两者互相 import store），
   * 而删除成功后的提示与界面刷新本来就发生在组件层。
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

  // ─── 返回报告的落点 ───────────────────────────────────────────────────────
  //
  // # 生命周期：先回退，送达成功才回收
  //
  // 旧实现是「先清再试」：栏的返回按钮先清上下文再 `requestOpen()`，一次失败就永久
  // 失去入口（票在上车之前就被收走了）。现在顺序反过来——返回动作只写 `pendingRestore`
  // 并派发打开意图，**它自己绝不清任何记录**；清除发生在 `ResearchModal` 里
  // `await focusReport()` 之后的成功分支，语义是"送达即回收"。
  //
  // # 来源记录的清除规则已搬到 `originTrail`
  // 三个清除点（用户点 ✕ / 打开另一份报告 / 送达成功）随数据一起搬走了。本文件只保留
  // "报告这一侧怎么被打开、位置怎么恢复"，它对"这次打开是不是一次返回"没有意见。

  /** 待打开的周期报告 ID（返回报告、但弹窗已关闭时用） */
  const pendingOpenId = ref<number | null>(null)

  /**
   * 待恢复的阅读位置（返回报告时写入，阅读页恢复完位置后清空）
   *
   * # 为什么位置恢复的唯一数据源是它，而不是来源记录
   * 来源记录的清除发生在"送达之后"，位置恢复也发生在"送达之后"，两者有先后竞争：
   * 若恢复逻辑从来源记录取 `scrollTop`，记录先被清就拿到 null，表现为
   * **"回来了但位置丢了"**——比原问题（回不去）更难查。
   * 所以职责切干净：来源记录只管栏的显隐与文案，位置只走这里。
   */
  const pendingRestore = ref<{ refNo: number | null; scrollTop: number } | null>(null)

  /**
   * 准备一次"回到报告"：把位置快照交给报告域
   *
   * 由 `originTrail.back()` 在派发打开意图**之前**调用。参数化而不是让来源域直接写
   * `pendingRestore`：这份快照的生命周期（何时消费、消费不掉怎么办）只有报告域清楚，
   * 外部写入等于把内部时序契约公开出去。
   *
   * @param refNo - 离开时点的那条引用编号（null = 不是从引用跳出去的）
   * @param scrollTop - 离开时 `.rd-doc-wrap` 的滚动量
   */
  function prepareReportReturn(refNo: number | null, scrollTop: number) {
    pendingRestore.value = { refNo, scrollTop }
  }

  /**
   * 请求打开某份报告（返回报告时用）
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
   * 消费一次待恢复位置（读后立即清空，避免下一次打开报告时被重复应用）
   *
   * @returns `{ refNo, scrollTop }`；没有待恢复内容时为 null
   */
  function consumeRestore(): { refNo: number | null; scrollTop: number } | null {
    const snapshot = pendingRestore.value
    pendingRestore.value = null
    return snapshot
  }

  /**
   * 打开报告页签并定位到指定报告（供外壳在弹窗打开后调用）
   *
   * # 回退 vs 重放
   * 弹窗里**已经是这份报告**时直接短路：只把浏览周期对齐，然后返回。
   * 不 `loadList()`、不 `openReport()`（即不重发 `report_period_get`）——
   * "回退"是回到我刚才停的地方，"重放"是重新拉一遍，后者既慢又会让位置与
   * 会话感丢失（DESIGN §4.3 第 2 步）。
   *
   * # 两种失败必须可区分
   * 「列表没拉成功」和「列表里没有这一份」是完全不同的两件事：前者是临时故障，
   * 用户再点一次就好；后者才是终态。调用方据此决定要不要把来源上下文标成 `stale`
   * （终态只有 ✕ 能收），所以这里抛带 `kind` 的 {@link FocusReportError}。
   *
   * @param id - 报告 ID
   * @throws {FocusReportError} `kind === 'list'` 表示列表加载失败（可重试）；
   *         `kind === 'missing'` 表示列表正常但没有这一份（降级重放也救不回来）
   */
  async function focusReport(id: number): Promise<void> {
    const open = detail.value
    if (open && open.summary.id === id) {
      // 短路分支必须在 loadList 之前返回：走重放会白跑一次列表查询与一次报告拉取
      kind.value = (open.summary.kind as PeriodKind) ?? kind.value
      windowMode.value = (open.summary.windowMode as WindowMode) ?? 'natural'
      anchor.value = new Date(open.summary.periodStart)
      return
    }
    await loadList()
    // `loadList` 把失败写进 `listError` 且不抛（见其注释），所以这里要显式判一次：
    // 不判就会把"列表没拉到"误判成"报告不存在"，那是一个不可撤销的错误结论
    if (listError.value) {
      throw new FocusReportError('list', '报告列表加载失败，请稍后重试')
    }
    const target = list.value.find((r) => r.id === id)
    if (!target) {
      // 报告已被删除或覆盖：不跳空白页，退到列表并提示
      throw new FocusReportError('missing', '该报告已不存在')
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
    trackFollowup,
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
    // 来源记录
    prepareReportReturn,
    pendingRestore,
    consumeRestore,
    requestOpen,
    consumePendingOpen,
    refresh,
  }
})

// 开发期热更新：用新定义重建 Pinia 实例。
// 少了这段，Vite 热更新只重跑本模块，而 pinia 缓存里仍是旧实例的 action 集合，
// 于是新组件调用本轮新增的方法会拿到 undefined，TypeError 被事件处理器吞掉，
// 界面表现是「点了没反应」；生产构建不会进入本分支。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useReportPeriodStore, import.meta.hot))
}
