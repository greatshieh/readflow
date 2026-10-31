/**
 * 周期窗口计算（日报 / 周报 / 月报）
 *
 * # 职责
 * 把"哪一期报告"翻译成一组 UTC 时间戳，供后端按 `published_at` 过滤文章。
 * 前端算窗口、后端只认时间戳，是因为"今天""本周"依赖用户本地时区与当前时刻，
 * 后端（尤其定时任务）看不到用户正在看哪一期。
 *
 * # 两端语义不同（最容易踩的一处）
 * `end` 是**排他**的：截止到 10 月 30 日的那一期，落库终点是 10 月 31 日 00:00。
 * 若用"含"的上界，10-30 23:00 的文章会被算进 10-31 的日报。
 *
 * # 为什么这里可以用 `toISOString()`
 * 项目其他地方（日期输入框）禁用 `toISOString()`，因为它取的是 UTC 日期，
 * 东八区凌晨会差一天。而这里是**时间戳**语义：本地 00:00 转成 UTC 瞬间正是我们要的，
 * 唯一会出错的是拿它去拼"YYYY-MM-DD"字符串——本文件不做这件事（展示走 formatLocal*）。
 */

/** 周期粒度 */
export type PeriodKind = 'daily' | 'weekly' | 'monthly'

/** 窗口口径 */
export type WindowMode = 'natural' | 'custom'

/** 一个时间窗口：`start` 含、`end` 不含 */
interface PeriodRange {
  /** 起点（含） */
  start: Date
  /** 终点（不含） */
  end: Date
}

/** 星期中文名（周报标题用） */
const WEEKDAYS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

/**
 * 取本地时区某天的 00:00
 *
 * @param d - 任意时刻
 * @returns 同一天的本地零点（新对象，不改原值）
 */
function startOfLocalDay(d: Date): Date {
  const out = new Date(d.getTime())
  out.setHours(0, 0, 0, 0)
  return out
}

/**
 * 取某一周周一的 00:00（周一为一周之始，与后端 `natural_window` 一致）
 *
 * @param d - 任意时刻
 * @returns 本周一本地零点
 */
function weekStart(d: Date): Date {
  const day = startOfLocalDay(d)
  // getDay()：周日 0、周一 1 …… 周六 6 → 回退天数为 (day + 6) % 7
  const back = (day.getDay() + 6) % 7
  day.setDate(day.getDate() - back)
  return day
}

/**
 * 计算某一期（自然周期）的时间窗口
 *
 * @param kind - 周期粒度
 * @param anchor - 该期内的任意一天（翻页时传目标日期）
 * @returns 左闭右开的本地时间窗口（调用方按需 `toISOString()`）
 */
export function naturalRange(kind: PeriodKind, anchor: Date): PeriodRange {
  if (kind === 'daily') {
    const start = startOfLocalDay(anchor)
    const end = new Date(start.getTime())
    end.setDate(end.getDate() + 1)
    return { start, end }
  }
  if (kind === 'weekly') {
    const start = weekStart(anchor)
    const end = new Date(start.getTime())
    end.setDate(end.getDate() + 7)
    return { start, end }
  }
  const start = startOfLocalDay(anchor)
  start.setDate(1)
  const end = new Date(start.getTime())
  // setMonth(month + 1) 在 12 月会自动进位到下一年，无需特判
  end.setMonth(end.getMonth() + 1)
  return { start, end }
}

/**
 * 按周期跨度前后翻页
 *
 * @param kind - 周期粒度
 * @param anchor - 当前锚点日期
 * @param delta - 翻几期（负数向前）
 * @returns 新锚点（同一期的代表性时刻，统一取该期起点）
 */
export function shiftPeriod(kind: PeriodKind, anchor: Date, delta: number): Date {
  const { start } = naturalRange(kind, anchor)
  const next = new Date(start.getTime())
  if (kind === 'daily') next.setDate(next.getDate() + delta)
  else if (kind === 'weekly') next.setDate(next.getDate() + delta * 7)
  else next.setMonth(next.getMonth() + delta)
  return next
}

/**
 * 判断锚点是否落在"当前这一期"
 *
 * 用于禁用「下一期」箭头：未来还没有文章，翻过去只会看到空态。
 *
 * @param kind - 周期粒度
 * @param anchor - 待判断的锚点
 * @param now - 当前时刻（测试注入用）
 * @returns 是当前期则 true
 */
export function isCurrentPeriod(
  kind: PeriodKind,
  anchor: Date,
  now: Date = new Date(),
): boolean {
  const a = naturalRange(kind, anchor)
  const b = naturalRange(kind, now)
  return a.start.getTime() === b.start.getTime()
}

/**
 * 周期的展示标签
 *
 * @param kind - 周期粒度
 * @param anchor - 该期内的任意一天
 * @returns 日报「2026年10月30日 周五」/ 周报「10月26日 – 11月1日」/ 月报「2026年10月」
 */
export function periodLabel(kind: PeriodKind, anchor: Date): string {
  const { start, end } = naturalRange(kind, anchor)
  if (kind === 'daily') {
    return `${start.getFullYear()}年${start.getMonth() + 1}月${start.getDate()}日 ${WEEKDAYS[start.getDay()]}`
  }
  if (kind === 'weekly') {
    // 上界排他 → 展示用最后一天
    const last = new Date(end.getTime())
    last.setDate(last.getDate() - 1)
    return `${start.getMonth() + 1}月${start.getDate()}日 – ${last.getMonth() + 1}月${last.getDate()}日`
  }
  return `${start.getFullYear()}年${start.getMonth() + 1}月`
}

/**
 * 粒度中文名
 *
 * @param kind - 周期粒度
 * @returns 日报 / 周报 / 月报
 */
export function periodKindLabel(kind: PeriodKind): string {
  return kind === 'daily' ? '日报' : kind === 'weekly' ? '周报' : '月报'
}

/**
 * 把时间戳格式化为「2026-10-30 12:04」
 *
 * @param iso - RFC3339 / ISO 字符串
 * @returns 本地时区的展示串；无法解析时原样返回
 */
export function formatLocalStamp(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}
