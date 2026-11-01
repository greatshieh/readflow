/**
 * 周复盘 store（阶段二）
 *
 * # 职责
 * 承载复盘页的全部数据与动作：四区总览、会话、处置、摘要。
 * 组件只负责渲染与收集输入，所有 IPC 都在这里。
 *
 * # 为什么会话状态放 store 而不是组件
 * 复盘页用 `v-if` 渲染（与 GraphView / RecommendView 同族，切换即卸载）。
 * "离开页面 ≠ 结束会话"（PRD-05 §4.1）：按 Esc 只是回文章栏，会话数据必须还在，
 * 用户再按 `W` 回来能接着看。所以 `overview` 与 `summary` 都不能随组件销毁而丢。
 *
 * # 加载顺序不能反
 * `overview` 靠"进行中的会话 ID"算「本次已处置」，而会话由 `start` 创建。
 * 先 overview 后 start 的话，第一次进来 `session = null`，卡片处置后不会离场
 * ——这是整个复盘唯一的静默失效点，故 `load()` 里写死了顺序。
 */

import { acceptHMRUpdate, defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

/** 一条判据 */
export interface Falsifier {
  id: number
  thesisId: number
  statement: string
  windowKind: 'date' | 'cycle'
  dueAt: string | null
  everyNDays: number | null
  keywords: string[]
  eventType: string
  entityId: number | null
  verdict: 'pending' | 'verified' | 'refuted' | 'unclear'
  verdictEvidenceId: number | null
  createdAt: string
  updatedAt: string
}

/** 命中的健康规则 */
export interface HealthHit {
  rule: string
  severity: 'high' | 'mid' | 'low'
  text: string
  push: boolean
}

/** 复盘卡片（四区共用，靠所在数组区分归属） */
export interface ReviewCard {
  thesisId: number
  statement: string
  entityId: number
  entityName: string
  status: 'active' | 'confirmed' | 'refuted'
  confidence: number
  supportCount: number
  opposeCount: number
  mixedCount: number
  ageDays: number
  lastEvidenceDays: number | null
  lastReviewAt: string | null
  falsifier: Falsifier | null
  overdueDays: number | null
  hits: HealthHit[]
  suggestedFalsifiers: string[]
  clueHitCount: number | null
}

/** 一次进行中的会话 */
export interface ReviewSession {
  id: number
  startedAt: string
  handledCount: number
}

/** 复盘摘要 */
export interface ReviewSummary {
  id: number
  startedAt: string
  finishedAt: string
  summary: string
  handledCount: number
  confidenceCount: number
  statusCount: number
  falsifierCount: number
  evidenceCount: number
  unchangedCount: number
  archivedCount: number
}

/** 四区总览 */
export interface ReviewOverview {
  session: ReviewSession | null
  due: ReviewCard[]
  health: ReviewCard[]
  missingFalsifier: ReviewCard[]
  idleCount: number
  handledCount: number
  quiet: boolean
  nextCheckAt: string | null
  extractionDisabledEntities: number
  lastReviewAt: string | null
}

/** 判据保存入参（内层字段 **snake_case**：入参结构体没有 rename_all） */
export interface FalsifierInput {
  thesis_id: number
  statement: string
  window_kind: 'date' | 'cycle'
  due_at?: string | null
  every_n_days?: number | null
  keywords?: string[]
  event_type?: string
  entity_id?: number | null
  reason?: string
}

/** 处置动作 */
export type MarkAction = 'unchanged' | 'confidence' | 'status' | 'falsifier' | 'evidence'

/** 一次处置的入参 */
export interface ReviewMarkInput {
  review_id: number
  thesis_id: number
  action: MarkAction
  confidence?: number | null
  status?: string | null
  reason?: string | null
  falsifier?: FalsifierInput | null
  evidence?: { thesis_id: number; kind: string; ref_id: number; stance: string; note?: string | null } | null
}

export const useReviewStore = defineStore('review', () => {
  /** 四区总览（null = 还没加载过） */
  const overview = ref<ReviewOverview | null>(null)
  /** 本次结束的摘要（null = 尚未结束） */
  const summary = ref<ReviewSummary | null>(null)
  /** 加载中 */
  const loading = ref(false)
  /** 处置中（按钮防连点） */
  const marking = ref(false)
  /** 最近一次错误 */
  const error = ref('')

  /**
   * 只取四区，不碰会话
   *
   * 处置完就调它：会话已经在了，再 start 一次只是多一次往返。
   */
  async function refresh(): Promise<void> {
    overview.value = await invoke<ReviewOverview>('thesis_review_overview')
  }

  /**
   * 进入复盘页：先开会话，再取四区
   *
   * 顺序**不能反**，理由见文件头。若后端返回 `session === null`（说明顺序还是错了，
   * 或会话被别处结束），补一次 start 并重取，把静默错误降级成一次多余往返。
   */
  async function load(): Promise<void> {
    loading.value = true
    error.value = ''
    try {
      const session = await invoke<ReviewSession>('thesis_review_start')
      await refresh()
      if (overview.value && overview.value.session === null) {
        await invoke<ReviewSession>('thesis_review_start', {})
        void session
        await refresh()
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      loading.value = false
    }
  }

  /**
   * 记一次处置并刷新四区
   *
   * @param input - 处置内容（内层 snake_case）
   */
  async function mark(input: ReviewMarkInput): Promise<void> {
    marking.value = true
    error.value = ''
    try {
      await invoke('thesis_review_mark', { input })
      await refresh()
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      throw e
    } finally {
      marking.value = false
    }
  }

  /**
   * 保存判据（也可脱离复盘单独调用）
   *
   * @param input - 判据内容（内层 snake_case）
   * @returns 判据行 ID
   */
  async function saveFalsifier(input: FalsifierInput): Promise<number> {
    return invoke<number>('thesis_falsifier_save', { input })
  }

  /**
   * 清空判据
   *
   * @param thesisId - 论点 ID
   * @param reason - 变更理由
   */
  async function clearFalsifier(thesisId: number, reason?: string): Promise<void> {
    await invoke('thesis_falsifier_clear', { thesisId, reason: reason ?? null })
  }

  /**
   * 结束本次复盘
   *
   * 成功后四区清空、摘要常驻（页面切到"本期结果"形态）。
   */
  async function finish(): Promise<void> {
    const id = overview.value?.session?.id
    if (id === undefined) return
    marking.value = true
    error.value = ''
    try {
      summary.value = await invoke<ReviewSummary>('thesis_review_finish', { reviewId: id })
      overview.value = null
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      marking.value = false
    }
  }

  /** 从摘要回到四区（开始新一次复盘） */
  function dismissSummary(): void {
    summary.value = null
  }

  return {
    overview,
    summary,
    loading,
    marking,
    error,
    load,
    refresh,
    mark,
    saveFalsifier,
    clearFalsifier,
    finish,
    dismissSummary,
  }
})

// store 必须带 HMR 自身替换：漏了会让新加的 action 在热更新后变成 undefined，
// 而报错被事件处理器吞掉，表现为"点了没反应"，只有重启 dev 才发现。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useReviewStore, import.meta.hot))
}
