/**
 * 今日待办（「今天要处理」）的 IPC 层与交互状态
 *
 * # 职责
 * 取聚合结果（`workbench_today`）、维护展开/收起、执行一键处理动作。
 * **所有组件都通过本 store 取数，没有组件自己 invoke**：
 * 一条命令两个组件各自调一次，等于两条"什么时候刷新"的逻辑。
 *
 * # 为什么收起状态存 sessionStorage 而不是 settings
 * "收起"是当下的意图，不是长期配置。存 settings 会让"今天收起一次"
 * 变成"以后永远不展开"，而用户第二天可能正想看到它。
 * 同理 `dismiss` 只在本次会话内隐藏：**"今天不想看"不该变成永久屏蔽**。
 *
 * # 为什么不缓存到 localStorage
 * 待办是"此刻该做什么"，跨会话缓存毫无意义——下次打开时数据早变了。
 */
import { acceptHMRUpdate, defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

/** 待办类别（与 Rust `workbench::TodoItem.kind` 一致） */
export type TodoKind = 'alert' | 'earnings' | 'thesis_due' | 'review' | 'coverage_exit'

/** 一键处理动作 */
export type TodoAction = 'mark_reviewed' | 'open_thesis' | 'open_entity' | 'open_coverage'

/** 一条待办（字段 camelCase，与 Rust 序列化对齐） */
export interface TodoItem {
  kind: TodoKind
  title: string
  detail: string
  priority: 'high' | 'mid' | 'low'
  target: 'review' | 'entity' | 'thesis' | 'coverage'
  targetId: number
  overdue: boolean
  action: TodoAction
}

/** 类别计数 */
export interface KindCount {
  kind: TodoKind
  count: number
}

/** 汇总（与 Rust `TodaySummary` 对齐） */
export interface TodaySummary {
  items: TodoItem[]
  counts: KindCount[]
  total: number
}

/** 收起状态在 sessionStorage 里的键名（本次会话内保持） */
const COLLAPSED_KEY = 'wb_todaybar_collapsed'
/** 本次会话内被手动忽略的条目（key 由 kind + targetId 组成） */
const DISMISSED_KEY = 'wb_todaybar_dismissed'

/**
 * 类别 → 展示名与短标签
 *
 * 放在 store 而不是组件：命令面板、设置说明等其它位置将来也要显示同类文案，
 * 分散写会出现"这里叫'预警'那里叫'提醒'"。
 */
export const KIND_LABELS: Record<TodoKind, { name: string; short: string }> = {
  alert: { name: '价格预警', short: '预警' },
  earnings: { name: '财报披露', short: '财报' },
  thesis_due: { name: '判据到期', short: '判据' },
  review: { name: '结构有偏', short: '偏' },
  coverage_exit: { name: '覆盖待定', short: '覆盖' }
}

export const useWorkbenchStore = defineStore('workbench', () => {
  const summary = ref<TodaySummary>({ items: [], counts: [], total: 0 })
  const loading = ref(false)
  /** 错误信息（仅用于 console 与调试，不在界面上喧哗） */
  const lastError = ref('')

  /** 收起状态：默认展开（首次使用要让人看见有这么个东西） */
  const collapsed = ref(sessionStorage.getItem(COLLAPSED_KEY) === '1')

  /** 本次会话内被忽略的条目 */
  const dismissed = ref<Set<string>>(readDismissed())

  function readDismissed(): Set<string> {
    try {
      const raw = sessionStorage.getItem(DISMISSED_KEY)
      return new Set(raw ? (JSON.parse(raw) as string[]) : [])
    } catch {
      // 存储被禁 / 内容损坏时当作空集合：忽略功能失效好过整个面板崩掉
      return new Set()
    }
  }

  function persistDismissed() {
    try {
      sessionStorage.setItem(DISMISSED_KEY, JSON.stringify([...dismissed.value]))
    } catch {
      /* 忽略存储失败：内存态仍然生效 */
    }
  }

  /** 条目的稳定身份（用于忽略） */
  function keyOf(item: TodoItem): string {
    return `${item.kind}:${item.targetId}`
  }

  /** 过滤掉本次会话内被忽略的条目 */
  const visibleItems = computed(() =>
    summary.value.items.filter((i) => !dismissed.value.has(keyOf(i)))
  )

  /** 可见条数（收起态摘要与"是否整条隐藏"都用它） */
  const visibleTotal = computed(() => visibleItems.value.length)

  /** 是否需要渲染整条（无待办时整条隐藏，不显示"0 项"这种噪音） */
  const shouldRender = computed(() => visibleTotal.value > 0)

  /** 摘要条上的分类计数（只显示有数的类别，顺序沿用后端） */
  const visibleCounts = computed(() =>
    summary.value.counts.filter((c) => c.count > 0)
  )

  /**
   * 拉取今日待办
   *
   * 失败时保留上一次结果（不清空）：网络/库偶发故障不该让已经看到的清单凭空消失。
   * 错误只记在 `lastError`，不弹窗——这块是主屏常驻区域，弹窗会很吵。
   */
  async function load(): Promise<void> {
    if (loading.value) return
    loading.value = true
    try {
      summary.value = await invoke<TodaySummary>('workbench_today')
      lastError.value = ''
    } catch (e) {
      // Tauri 的 reject 是字符串而非 Error
      lastError.value = String(e)
      console.warn('今日待办获取失败（保留上次结果）：', e)
    } finally {
      loading.value = false
    }
  }

  /** 切换展开 / 收起 */
  function toggleCollapsed() {
    collapsed.value = !collapsed.value
    try {
      sessionStorage.setItem(COLLAPSED_KEY, collapsed.value ? '1' : '0')
    } catch {
      /* 忽略 */
    }
  }

  /** 本次会话内忽略一条（不落库、不影响后端数据） */
  function dismiss(item: TodoItem) {
    dismissed.value = new Set(dismissed.value).add(keyOf(item))
    persistDismissed()
  }

  /**
   * 执行一键处理
   *
   * # 两类动作的语义必须分开
   * - `mark_reviewed`：**真正改变数据**——把论点标记为已复盘，条目随之消失；
   * - `open_*`：**只是跳转**，人还没处理完，条目必须保留。
   *   把跳转当成"已处理"是这类面板最常见的错误，会让用户以为自己
   *   完成了一堆事，其实一条都没做。
   *
   * @param item - 待办条目
   * @param go - 跳转回调（由组件注入，避免 store 依赖路由/弹窗层）
   * @param markReviewed - 标记已复盘的执行函数（由组件注入，走复盘自己的命令）
   */
  async function handleAction(
    item: TodoItem,
    go: {
      toThesis: (id: number) => void
      toEntity: (id: number) => void
      toCoverage: () => void
      toReview: () => void
    },
    markReviewed: (thesisId: number) => Promise<void>
  ): Promise<void> {
    if (item.action === 'mark_reviewed') {
      await markReviewed(item.targetId)
      dismiss(item)
      return
    }
    // 跳转类：不消除条目
    if (item.action === 'open_thesis') go.toThesis(item.targetId)
    else if (item.action === 'open_entity') go.toEntity(item.targetId)
    else if (item.action === 'open_coverage') go.toCoverage()
    else if (item.target === 'review') go.toReview()
  }

  return {
    summary,
    loading,
    lastError,
    collapsed,
    visibleItems,
    visibleTotal,
    shouldRender,
    visibleCounts,
    load,
    toggleCollapsed,
    dismiss,
    handleAction
  }
})

// 开发期热更新：少了这段，新 action 会在热更新后变成 undefined，
// 界面表现是「点了没反应」，且没有报错。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useWorkbenchStore, import.meta.hot))
}
