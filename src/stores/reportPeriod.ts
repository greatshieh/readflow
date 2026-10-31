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
 * - **来源上下文 `B1`**（从报告点进文章后，正文栏顶部那条「上下文栏」的依据）。
 *
 * # 为什么 `B1` 必须放 Store
 * 弹窗内部状态能保留，靠的是"`BaseModal` 无 `v-if` + 三个页签 `v-show` 常驻"这个
 * **隐式契约**。哪天有人为性能给弹窗加 `v-if`，返回会静默失效且不报错。
 * `B1` 显式存 store，弹窗被销毁重建也能恢复；组件在跳走时就已卸载，放组件里必丢。
 *
 * # `B1` 的生命周期（本次改动的核心）
 * 从"用后即弃"改成"**送达即回收**"：返回动作先派发，`ctx` 的清除只发生在
 * 打开成功之后；失败不清、转 `stale` 失效态留在栏上。详见
 * `// ─── 来源上下文 B1 ───` 一节的注释与 `deliverables/design/report-backtrack/DESIGN.md` §4.4。
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
 * 来源上下文：从某份报告的第 n 条引用跳到文章后，正文栏据此显示「上下文栏」
 *
 * 它只负责**栏的显隐与文案**；阅读位置的恢复另走 `pendingRestore`（见该变量注释）。
 */
interface SourceContext {
  /** 来源报告 ID */
  reportId: number
  /**
   * 来源报告的 AI 标题**全文**（模型生成的一句话标题，最长 60 字）
   *
   * 原样照搬、不做预处理：前端截成 30 字会让 `title` 属性也变短，用户再也看不到全名。
   * 截断由 CSS（ellipsis）负责，`title` 属性里始终是全文（DESIGN §5.4）。
   */
  reportTitle: string
  /**
   * 期号标签（如「10月30日 日报」），写入上下文时算好
   *
   * 栏里不再引 `utils/periodWindow`：期号是"回到哪一份"的坐标，必须在栏渲染时零计算。
   */
  reportLabel: string
  /** 刚才点的那条引用编号；不是从引用跳过去时为 null */
  refNo: number | null
  /** 从报告跳到的那个文章 ID：当前文章换了就隐去「引用 n」徽标（徽标只对那一篇有意义） */
  articleId: number
  /** 离开报告时 `.rd-doc-wrap` 的滚动量，回退时按它恢复位置 */
  scrollTop: number
  /**
   * 目标报告已不存在（被删除 / 被覆盖重生成）
   *
   * `stale` 是**终态**：栏留在原地转失效态，用户点 ✕ 收掉它，不提供"重试"
   * （重试路径交给「研究工作台 → 报告」，那里能看到最新列表）。
   */
  stale: boolean
}

/**
 * 上下文栏用的紧凑期号标签
 *
 * # 为什么不复用 `periodLabel`
 * 后者输出「2026年10月30日 周五」（11 个字）。上下文栏只有 36px 高，还要同时放
 * AI 标题，期号多占的每个字都是从标题里抢来的（DESIGN §5.4）。
 *
 * @param kind - 周期粒度（`PeriodSummary.kind` 原样传入，后端是字符串）
 * @param periodStart - 该期起点（ISO 字符串；按**本地时区**取日期，不做 UTC 换算）
 * @returns 日报「10月30日 日报」/ 周报「10.26 – 11.01 周报」/ 月报「2026年10月 月报」；
 *          日期无法解析时退化为粒度名（如「日报」），栏里至少还有一段可读文字
 */
export function compactPeriodLabel(kind: string, periodStart: string): string {
  const start = new Date(periodStart)
  if (Number.isNaN(start.getTime())) return periodKindLabel(kind as PeriodKind)
  const p2 = (n: number) => String(n).padStart(2, '0')
  if (kind === 'weekly') {
    // 窗口左闭右开：本期最后一天 = 起点 + 6
    const last = new Date(start.getTime())
    last.setDate(last.getDate() + 6)
    return `${start.getMonth() + 1}.${p2(start.getDate())} – ${last.getMonth() + 1}.${p2(last.getDate())} 周报`
  }
  if (kind === 'monthly') {
    return `${start.getFullYear()}年${start.getMonth() + 1}月 月报`
  }
  return `${start.getMonth() + 1}月${start.getDate()}日 日报`
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
    // 打开的是**另一份**报告 → 用户已经主动换了在读的东西，"回到上一份"不再有意义，
    // 上下文随栏一起作废（清除规则 #3）。返回自身时 `id === ctx.reportId`，
    // 这条守卫不成立，不会把"正在返回的那一份"误清掉。
    if (sourceContext.value && sourceContext.value.reportId !== id) {
      clearSourceContext()
    }
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
   * 若删掉的正是上下文指向的那份：**不清 `ctx`**，只置 `stale`——栏留在原地转失效态，
   * 用户手上还按着文章，直接抽走入口等于让他在原地失联（清除规则 #5）。
   *
   * @param id - 报告 ID
   */
  async function removeReport(id: number): Promise<void> {
    await invoke('report_period_delete', { id })
    list.value = list.value.filter((r) => r.id !== id)
    if (detail.value?.summary.id === id) detail.value = null
    if (sourceContext.value?.reportId === id) markSourceStale('这份报告已被删除')
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
  //
  // # 生命周期：先回退，送达成功才回收（DESIGN §4.4）
  //
  // 旧实现是「先清再试」：`SourceBar.goBack()` 先 `clearSourceContext()` 再
  // `requestOpen()`，一次失败就永久失去入口（票在上车之前就被收走了）。
  // 现在顺序反过来——`backToReport()` 只写 `pendingRestore` 并派发打开意图，
  // **它自己绝不清 `ctx`**；清理发生在 `ResearchModal` 里
  // `await focusReport()` 之后的成功分支，语义是"送达即回收"。
  //
  // # `clearSourceContext()` 只有三个调用点（其余地方一律不许清）
  // 1. 用户点栏上的 ✕ —— 语义是"我不再需要这条路"；
  // 2. 用户打开了另一份报告（`openReport` 内 `id !== ctx.reportId`）；
  // 3. 返回送达成功（`ResearchModal` 里 `focusReport` 之后）。
  // 失败路径**不清**，只转 `stale`；关弹窗、切文章、切视图、切分段也都不动。

  /** 当前来源上下文（null = 正文栏不显示上下文栏） */
  const sourceContext = ref<SourceContext | null>(null)
  /** 待打开的周期报告 ID（上下文栏点了返回、但弹窗已关闭时用） */
  const pendingOpenId = ref<number | null>(null)

  /**
   * 待恢复的阅读位置（`backToReport()` 写入，阅读页恢复完位置后清空）
   *
   * # 为什么位置恢复的唯一数据源是它，而不是 `sourceContext`
   * 清除 `ctx` 发生在"送达之后"，位置恢复也发生在"送达之后"，两者有先后竞争：
   * 若恢复逻辑从 `ctx` 取 `scrollTop`，`ctx` 先被清就拿到 null，表现为
   * **"回来了但位置丢了"**——比原问题（回不去）更难查。
   * 所以职责切干净：`ctx` 只管栏的显隐与文案，位置只走这里。
   */
  const pendingRestore = ref<{ refNo: number | null; scrollTop: number } | null>(null)

  /**
   * 记录来源（点「打开全文」时先写，后面任何一步失败至少还回得去）
   *
   * @param ctx - 来源上下文（含期号标签、跳去的文章、离开报告的滚动量）
   */
  function setSourceContext(ctx: SourceContext) {
    sourceContext.value = ctx
  }

  /**
   * 清空来源上下文（栏随之消失）
   *
   * 调用点只有三处（见本节顶部注释）：用户点 ✕、打开另一份报告、返回送达成功。
   * 幂等：置 null 而已，重复调用无副作用——真正要防的不是重复清，而是**提前清**。
   */
  function clearSourceContext() {
    sourceContext.value = null
  }

  /**
   * 把当前来源上下文标记为失效（目标报告已不存在）
   *
   * 就地改 `stale`、**不清空对象**：栏要留在原地变成失效态。若这里清掉 `ctx`，
   * 用户既没有退回目标、也没有栏可点，就退化成"失败即失联"，正是本次要消掉的病。
   *
   * @param reason - 失效原因。界面文案固定为「这份报告已不存在」（DESIGN §4.4 明确
   *                 不提供第二种话术、不提供"重试"），原因只落控制台供排查
   */
  function markSourceStale(reason = '') {
    const ctx = sourceContext.value
    if (!ctx) return
    ctx.stale = true
    if (reason) console.warn(`[reportPeriod] 来源上下文失效：${reason}`)
  }

  /**
   * 请求打开某份报告（上下文栏返回时用）
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
   * 回到来源报告（栏的主按钮、引用徽标、`Alt+←` / `Cmd+[` 共用的唯一入口）
   *
   * 只做两件事：把"离开时的位置"记进 `pendingRestore`、派发打开意图。
   * **函数体内绝不 `clearSourceContext()`**——它是同步函数，`requestOpen` 只是派发
   * 事件，此刻根本不知道转场成败；在这里清就等于回到"先清再试"的老路。
   *
   * @returns 无返回值；`ctx` 为空或已失效时无操作（失效态没有可回的目标）
   */
  function backToReport() {
    const ctx = sourceContext.value
    if (!ctx || ctx.stale) return
    pendingRestore.value = { refNo: ctx.refNo, scrollTop: ctx.scrollTop }
    requestOpen(ctx.reportId)
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
    // 来源上下文
    sourceContext,
    setSourceContext,
    clearSourceContext,
    markSourceStale,
    backToReport,
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
