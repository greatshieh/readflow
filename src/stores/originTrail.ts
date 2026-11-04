/**
 * 来源回路 Store（Pinia）
 *
 * # 职责
 * 记住"用户是从研究工作台的哪一处跳进原文的、当时停在哪"，并负责把他送回去：
 * - `current`：正文栏顶部那条上下文栏的**唯一**显隐依据；
 * - `back()`：一次返回的唯一入口（栏的主按钮 / 引用徽标 / `Alt+←` 共用）；
 * - `pendingReturn`：交给 `ResearchModal` 的凭据——"这次打开是一次返回，不是新开"。
 *
 * # 为什么是单条快照，而不是栈
 * 场景是"读报告 → 跳原文 → 回去"，不是浏览器式的前进后退历史：从某个来源跳进原文后，
 * 用户想回的是**那一个**来源；再跳一次（从另一处进原文）时旧来源已经不可能再被
 * "上一步"抵达，留着它只会让栏显示一个过期的落点。所以 `push` 是覆盖语义。
 * 栈还要额外回答"返回后要不要 pop""同一来源重复 push 算几步"，两串状态机换不来
 * 任何用户可见的收益。
 *
 * # 为什么位置快照放这里、报告的位置恢复却仍走 `reportPeriod.pendingRestore`
 * 报告域的位置恢复是一条**已经验证过**的链路（`PeriodReportReader` 的立即 watch +
 * 有界重试 + 送达即回收，见该文件 539-574 行）。重写它只会把踩过的坑再踩一遍。
 * 所以这里只持有快照，走到报告这条路上时把快照**转交**给报告域
 * （`prepareReportReturn`），由报告域自己恢复。
 *
 * # 与 `pendingRestore` / `consumeThesisOpen()` 是三条独立通道
 * `pendingReturn` 只表达"这次打开是一次返回"。一次普通打开（命令面板 / 更多菜单）
 * 永远拿不到它。混用会让"用户从命令面板点了一条判断"被当成一次返回，于是错误地
 * 恢复别人的位置、甚至清掉别人的来源。
 */

import { acceptHMRUpdate, defineStore } from 'pinia'
import { ref } from 'vue'
import { useReportPeriodStore } from '@/stores/reportPeriod'

/** 来源类别：决定栏的失效文案、徽标是否可能出现、读屏里的目的地名词 */
export type OriginKind = 'report' | 'timeline' | 'thesis' | 'coverage' | 'entity'

/** 失效原因：四个值各自对应一句面向用户的话（文案见 `utils/originLabel.ts`） */
export type OriginStale = 'report-missing' | 'thesis-missing' | 'article-missing' | 'event-missing'

/**
 * 栏上的三段文案（分工决定窄栏时谁先让位，详见 `utils/originLabel.ts` 文件头）
 *
 * - `anchor`：坐标（期号跨度 `10.26 – 11.01` / 日期 `2026-10-14` / 实体名）。**可收缩**，
 *   被压缩时带 ellipsis（用户看得见"这里还有字"）；
 * - `kind`：类别词（`周报` / `事件` / `论点`）。系统枚举、长度有界，**永不收缩、永不截断**——
 *   窄到只剩两三个字时，留下的必须是它；
 * - `desc`：用户写的东西（AI 标题 / 实体名·事件事实 / 论点陈述），长度无界，
 *   窄栏时整段 `display: none`。
 *
 * ⚠ 类别词不许折进 `anchor`：折回去会让窄栏把「10月30日 日报」压成「10月30日 日」，
 * 而栏的容器只有 `overflow: hidden`、没有省略号，用户看不出被裁过。
 */
export interface OriginLabel {
  anchor: string
  kind: string
  desc: string
}

/**
 * 回到原处要恢复什么
 *
 * 位置快照在此只**持有**，不解释：怎么落到 DOM 上由各页签自己决定（滚动容器不同）。
 */
export interface OriginRestore {
  tab: 'timeline' | 'reports' | 'entities' | 'coverage' | 'theses'
  /** 离开时页签滚动容器的 `scrollTop` */
  scrollTop: number
  /** 时间线：离开时展开的那条事件 */
  expandedEventId?: number
  /** 论点：离开时打开的那条论点详情 */
  thesisId?: number | null
  /** 报告：离开时点的那条引用编号；转交给 `reportPeriod.pendingRestore` */
  refNo?: number | null
  /**
   * 报告：要打开的那一份
   *
   * 单独一个字段而不是从 `key` 里反解：`key` 是去重键，语义上随时可以改格式
   * （本期为了"同一来源重复 push 只替换"，它就带了 `refNo`），
   * 让"打开哪份报告"依赖 `key` 的字符串格式是把两件不相干的事绑在一起。
   */
  reportId?: number
}

/** 一条来源记录 */
export interface OriginRecord {
  kind: OriginKind
  /** 去重键：重复 push 同一来源只替换不堆积（本期快照只有一条，键仍要留，便于将来判重） */
  key: string
  label: OriginLabel
  /** 跳过去的文章 ID：引用徽标只对这一篇有意义 */
  articleId: number
  restore: OriginRestore
  /** 目标已不存在（终态）；null = 正常 */
  stale: null | OriginStale
}

/** 一次返回的凭据：交给 `ResearchModal` 消费 */
export interface OriginReturnTicket {
  key: string
  restore: OriginRestore
}

/**
 * 页签恢复的结果
 *
 * `missing` 与 `transient` 必须分开：前者是终态（转失效态，只剩 ✕），
 * 后者是临时故障（栏留着，用户再点一次就好）。把临时故障标成终态
 * 就是这次改造要消掉的病——"失败即失联"。
 */
export type OriginRestoreResult =
  | { ok: true }
  | { ok: false; kind: 'missing' | 'transient'; message?: string }

/** `back()` 的结果：调用方据此决定要不要 toast */
export type OriginBackResult =
  | { ok: true }
  | { ok: false; kind: 'transient' | 'missing'; message: string }

export const useOriginTrailStore = defineStore('originTrail', () => {
  const reportStore = useReportPeriodStore()

  /** 当前来源（null = 正文栏没有上下文栏） */
  const current = ref<OriginRecord | null>(null)

  /** 待消费的返回凭据（`back()` 写入，`ResearchModal` 打开时取走） */
  const pendingReturn = ref<OriginReturnTicket | null>(null)

  /**
   * 是否有"正在路上的返回"
   *
   * # 为什么不直接写 `pendingReturn !== null`
   * `ResearchModal` 的 open watch 在**子组件的 watch 之前**执行（父组件先 flush），
   * 它取走凭据后，子页签再去读 `pendingReturn` 就永远是 null 了。
   * 所以它表达的是"有待返回意图"，生命周期到**送达或放弃**为止，而不是到被取走为止。
   */
  const returnInFlight = ref(false)

  /**
   * 在途返回是否**落在这个页签**
   *
   * 只暴露这一个判断，不另外暴露"有没有在途返回"：页签需要的从来是"这次打开是不是
   * 冲我来的"。返回报告时时间线也会收到"弹窗打开了"事件，若它只判"有在途返回"，
   * 就会把别人的返回当成自己的，那次打开不再重置展开项——用户下次切到时间线
   * 会看到一个上次的展开态。
   *
   * @param tab - 页签名
   * @returns 在途返回的落点就是这个页签时 true
   */
  function returnsTo(tab: OriginRestore['tab']): boolean {
    return returnInFlight.value && current.value?.restore.tab === tab
  }

  /**
   * 记下来源
   *
   * 覆盖语义：同一 `key` 重复调用只替换（本期快照只有一条，`key` 用于将来判重与日志）。
   * 同时作废任何在途的返回凭据——用户已经在往新来源跳了，"刚才那次返回"不再成立。
   *
   * @param record - 完整来源记录（文案由 `utils/originLabel.ts` 生成）
   */
  function push(record: OriginRecord): void {
    current.value = record
    pendingReturn.value = null
    returnInFlight.value = false
  }

  /**
   * 清空来源（栏随之消失）
   *
   * 调用点只有三处：用户点栏上的 ✕、返回**送达成功**之后、以及"来源换了别人"。
   * 失败路径一律不清——清在一个会失败的转场之前等于"失败即失联"，
   * 那是 `reportPeriod.ts` 旧实现（先清再试）留下的原文教训。
   */
  function clear(): void {
    current.value = null
    pendingReturn.value = null
    returnInFlight.value = false
  }

  /**
   * 把当前来源标记为失效（目标已不存在）
   *
   * 就地改 `stale`、**不清空记录**：栏要留在原地转失效态，用户至少还有一个可关的入口。
   * 若这里清掉，用户既没有退回目标、也没有栏可点，就退化成"失败即失联"。
   *
   * @param reason - 失效原因（界面文案由 `originStaleText` 统一给出）
   */
  function markStale(reason: OriginStale): void {
    const record = current.value
    if (!record) return
    record.stale = reason
    // 目标没了，在途的返回凭据也不可能送达；留着会让时间线以为"还在返回中"
    pendingReturn.value = null
    returnInFlight.value = false
  }

  /**
   * 若来源正是这份报告，转失效态
   *
   * 报告被删除时调用（`PeriodReports` 的删除成功分支）。这里**不清**：
   * 用户手上还按着那篇原文，抽走入口比留一条"这份报告已不存在"更糟。
   *
   * @param reportId - 被删除的报告 ID
   */
  function markStaleIfReport(reportId: number): void {
    const record = current.value
    if (!record || record.kind !== 'report') return
    if (record.restore.reportId !== reportId) return
    markStale('report-missing')
  }

  /**
   * 用户主动打开另一份报告 → 作废指向旧报告的返回入口
   *
   * 依据：用户已经换了在读的东西，"回到上一份"不再是他此刻的意图；
   * 栏上留着一个属于旧报告的期号会与正在读的那份打架。
   * 指向**同一份**报告时不作废——那正是返回路径本身（`focusReport` 会打开同一份）。
   *
   * @param reportId - 即将打开的报告 ID
   */
  function dropIfReportOtherThan(reportId: number): void {
    const record = current.value
    if (!record || record.kind !== 'report') return
    if (record.restore.reportId === reportId) return
    clear()
  }

  /**
   * 回到来源（栏的主按钮、引用徽标、`Alt+←` / `Cmd+[` 共用的唯一入口）
   *
   * 只做三件事：写返回凭据、把位置快照交给目的地、派发已有的打开意图事件。
   * **函数体内绝不清 `current`**：清除只发生在送达之后（`ResearchModal` 里恢复成功
   * 的分支）。在这里清就是把"先清再试"重演一遍。
   *
   * 顺序也不好换：`pendingReturn` 必须先写、再派发事件——`open-research` 是同步派发，
   * 接收方（`FeedPanel`）当场就把弹窗置为打开，下一次 flush 里 `ResearchModal` 的
   * open watch 就要读这份凭据。
   *
   * @returns 成功派发时 `{ ok: true }`；无来源 / 已失效 / 生成中返回失败原因
   */
  async function back(): Promise<OriginBackResult> {
    const record = current.value
    if (!record || record.stale !== null) {
      return { ok: false, kind: 'missing', message: '这个来源已不存在' }
    }
    // 生成期阻塞转场：`ResearchModal.close()` 已有同一先例（生成中不允许关弹窗）。
    // 放行的话会打开一个正在跑进度卡的页签，恢复动作与进度卡抢同一块区域
    if (reportStore.generating) {
      return { ok: false, kind: 'transient', message: '报告生成中，稍后再返回' }
    }
    pendingReturn.value = { key: record.key, restore: record.restore }
    returnInFlight.value = true

    if (record.kind === 'report' && typeof record.restore.reportId === 'number') {
      // 报告域自己负责"送达即回收"：位置快照转交 `pendingRestore`，打开意图走 `requestOpen`。
      // 它内部就会派发 `open-research`，这里**不能**再派发一次
      reportStore.prepareReportReturn(record.restore.refNo ?? null, record.restore.scrollTop)
      reportStore.requestOpen(record.restore.reportId)
    } else {
      window.dispatchEvent(new CustomEvent('open-research'))
    }
    return { ok: true }
  }

  /**
   * 取走返回凭据（取走即清空，避免重复触发）
   *
   * 刻意**不**动 `returnInFlight`：子页签的 `returnsTo()` 还要读它（凭据被取走
   * 不等于返回已经送达）。
   *
   * @returns 返回凭据；没有在途返回时为 null
   */
  function consumePendingReturn(): OriginReturnTicket | null {
    const ticket = pendingReturn.value
    pendingReturn.value = null
    return ticket
  }

  return {
    current,
    returnsTo,
    push,
    clear,
    markStale,
    markStaleIfReport,
    dropIfReportOtherThan,
    back,
    consumePendingReturn,
  }
})

// 开发期热更新：用新定义重建 Pinia 实例。
// 少了这段，Vite 热更新只重跑本模块，pinia 缓存里仍是旧 action 集合，
// 新组件调用本轮新增的方法会拿到 undefined，表现为「点了没反应」。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useOriginTrailStore, import.meta.hot))
}
