/**
 * 论点台账 Store（Pinia）
 *
 * # 职责
 * 承载「我的判断」这一层的前端状态：论点列表、当前打开的论点详情、
 * 新建/更新/删除、证据挂接与摘除。
 *
 * # 为什么单独一个 store 而不是塞进 research.ts
 * `research.ts` 承载的是**系统产出**（事件/报告/实体），本 store 承载的是
 * **人的判断**。两者生命周期完全不同：事件随文章增长、自动过期；
 * 论点一旦建立就应该长期存在，且"最近动过"决定列表顺序。
 *
 * # 边界
 * 不做任何语义判断——"这条证据支持还是反驳"由用户在界面上选，
 * store 只负责把它送到后端。让前端替用户猜立场，等于把确认偏误搬个地方。
 */

import { acceptHMRUpdate, defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useEntitiesStore } from './entities'

/** 论点状态 */
export type ThesisStatus = 'active' | 'confirmed' | 'refuted' | 'archived'

/** 证据来源类型 */
type EvidenceKind = 'event' | 'highlight' | 'article'

/** 证据立场：支持 / 反驳 / 存疑 */
export type EvidenceStance = 'support' | 'oppose' | 'mixed'

/** 论点（与后端 `Thesis` 一一对应，字段名必须一致） */
interface Thesis {
  id: number
  statement: string
  entityId: number
  entityName: string
  status: ThesisStatus
  confidence: number
  supportCount: number
  opposeCount: number
  mixedCount: number
  createdAt: string
  updatedAt: string
}

/** 一条证据 */
interface Evidence {
  id: number
  kind: EvidenceKind
  refId: number
  stance: EvidenceStance
  note: string
  snapshotText: string
  snapshotAt: string
  createdAt: string
  /** 可跳转的源文章 ID（源被删后为 null） */
  articleId: number | null
  articleTitle: string
}

/** 一次变更留痕 */
export interface Revision {
  id: number
  field: 'confidence' | 'status' | 'statement'
  fromValue: string
  toValue: string
  reason: string
  createdAt: string
}

/** 论点详情 */
export interface ThesisDetail {
  thesis: Thesis
  evidence: Evidence[]
  revisions: Revision[]
}

/**
 * 一条判断的证据来水构成（与 Rust `Divergence` 对齐）
 *
 * 侧别分开计数是刻意的：3 家支持 + 1 家反对不能被说成"4 家互证"。
 */
export interface Divergence {
  thesisId: number
  /** 支持证据来自几家不同来源 */
  supportSources: number
  /** 反对证据来自几家不同来源 */
  opposeSources: number
  /** 去重后的来源名 */
  sources: string[]
  /** 没有来源记录的证据条数（TASK-00 之前挂的老证据） */
  unrecorded: number
}

/** 新建论点的入参 */
interface NewThesisInput {
  statement: string
  entityId: number
  confidence?: number
}

/** 更新论点的入参（只传要改的字段） */
interface ThesisPatchInput {
  statement?: string
  status?: ThesisStatus
  confidence?: number
  reason?: string
}

/** 挂证据的入参 */
interface NewEvidenceInput {
  thesisId: number
  kind: EvidenceKind
  refId: number
  stance: EvidenceStance
  note?: string
}

/** 状态的中文名与色调（列表徽标与详情页共用，避免两处不一致） */
export const STATUS_META: Record<ThesisStatus, { label: string; tone: string }> = {
  active: { label: '观察中', tone: 'active' },
  confirmed: { label: '已验证', tone: 'ok' },
  refuted: { label: '已推翻', tone: 'bad' },
  archived: { label: '已归档', tone: 'muted' },
}

/** 证据来源类型 → 中文 */
export const KIND_LABEL: Record<EvidenceKind, string> = {
  event: '研究事件',
  highlight: '高亮',
  article: '文章',
}

export const useThesesStore = defineStore('theses', () => {
  const entities = useEntitiesStore()

  /** 列表（默认不含归档） */
  const list = ref<Thesis[]>([])
  /** 列表筛选：状态 */
  const filterStatus = ref<ThesisStatus | 'all' | 'activeOnly'>('activeOnly')
  /** 列表筛选：实体（null = 全部） */
  const filterEntityId = ref<number | null>(null)
  /** 是否正在加载列表 */
  const loading = ref(false)
  /** 当前打开的论点详情（null = 停在列表） */
  const detail = ref<ThesisDetail | null>(null)
  /** 详情加载中 */
  const detailLoading = ref(false)
  /** 最近一次错误（供界面提示） */
  const error = ref('')

  /**
   * 按论点 ID 缓存的分歧度（来源构成）
   *
   * 缓存而不是每次进详情都取：对比两条判断的证据强度是常见动作，
   * 来回切换时不该各等一次往返。键是论点 ID，删除/新增证据后重取覆盖。
   */
  const divergenceById = ref<Record<number, Divergence>>({})

  /** 顶层统计：用于列表页头部"3 个论点需要跟进"这类提示 */
  const stats = computed(() => {
    const all = list.value
    return {
      total: all.length,
      active: all.filter((t) => t.status === 'active').length,
      /** 有反驳证据的论点数量——这是"该看一下了"的最强信号 */
      challenged: all.filter((t) => t.opposeCount > 0 && t.status === 'active').length,
    }
  })

  /**
   * 拉取论点列表
   *
   * `filterStatus` 为 `'activeOnly'` 时传 `null`（后端默认隐藏归档）。
   */
  async function loadList() {
    loading.value = true
    error.value = ''
    try {
      const status =
        filterStatus.value === 'activeOnly' || filterStatus.value === 'all'
          ? null
          : filterStatus.value
      list.value = await invoke<Thesis[]>('thesis_list', {
        status,
        entityId: filterEntityId.value,
      })
      // `all` 需要归档也出现，后端 None 会隐藏归档 → 再补一次显式查询
      if (filterStatus.value === 'all') {
        list.value = await invoke<Thesis[]>('thesis_list', { status: 'all', entityId: filterEntityId.value })
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      list.value = []
    } finally {
      loading.value = false
    }
  }

  /**
   * 打开某个论点的详情
   *
   * @param id - 论点 ID
   */
  async function openThesis(id: number) {
    detailLoading.value = true
    error.value = ''
    try {
      detail.value = await invoke<ThesisDetail>('thesis_get', { id })
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      detail.value = null
    } finally {
      detailLoading.value = false
    }
  }

  /** 关闭详情，回到列表 */
  function closeThesis() {
    detail.value = null
  }

  /**
   * 重新加载当前打开的详情（增删证据、改状态后调用）
   *
   * 详情与列表是一体的：改完必须两边都刷新，否则列表里的计数会停在旧值，
   * 用户会以为"我刚挂的证据没生效"。
   */
  async function refreshCurrent() {
    const id = detail.value?.thesis.id
    if (id != null) {
      await openThesis(id)
      // 证据变了，来源构成跟着变。放在这里而不是让组件各自记得重取：
      // addEvidence / removeEvidence / update 全都汇到本函数，
      // 漏一处就是"加了证据但顶部那行还写着 1 家"
      await loadDivergence(id)
    }
    await loadList()
  }

  /**
   * 新建论点
   *
   * @param input - 判断文本 + 标的实体 + 可选初始置信
   * @returns 新论点 ID
   */
  async function create(input: NewThesisInput): Promise<number> {
    // 后端按 entity_id 查标的，前端传 entityId（Tauri 会自动映射驼峰 → 下划线）
    const id = await invoke<number>('thesis_create', {
      input: {
        statement: input.statement,
        entity_id: input.entityId,
        confidence: input.confidence ?? 3,
      },
    })
    await loadList()
    // 实体列表的"论点计数"由后端 join 得到，这里无需刷新实体本身
    void entities
    return id
  }

  /**
   * 更新论点
   *
   * @param id - 论点 ID
   * @param patch - 只传要改的字段；`reason` 会写进变更记录
   */
  async function update(id: number, patch: ThesisPatchInput) {
    await invoke<void>('thesis_update', {
      id,
      patch: {
        statement: patch.statement,
        status: patch.status,
        confidence: patch.confidence,
        reason: patch.reason,
      },
    })
    await refreshCurrent()
  }

  /**
   * 把一条论点组装成一页 markdown（「组装」预览的取数）
   *
   * # 为什么不在前端拼
   * 章节顺序、引用写法、日期截断是**内容格式**，不是渲染。拼接在同族的
   * `coverage::dossier` 里也在 Rust——搬一份到前端，同一套格式就会两头各长一个，
   * 改一处漏一处，两个出口的产出物迟早互相不认识。
   * 前端只做两件事：把原文交给 `renderMarkdown` 显示、把原文写进剪贴板。
   *
   * @param id - 论点 ID
   * @returns markdown 原文
   */
  async function composeMarkdown(id: number): Promise<string> {
    return invoke<string>('thesis_compose', { id })
  }

  /**
   * 删除论点
   *
   * @param id - 论点 ID
   */
  async function remove(id: number) {
    await invoke<void>('thesis_delete', { id })
    if (detail.value?.thesis.id === id) detail.value = null
    await loadList()
  }

  /**
   * 挂一条证据
   *
   * 同一来源重复挂载时后端会就地改立场（用户改主意是正常行为）。
   *
   * @param input - 论点 + 来源 + 立场 + 可选备注
   */
  async function addEvidence(input: NewEvidenceInput) {
    await invoke<number>('thesis_add_evidence', {
      input: {
        thesis_id: input.thesisId,
        kind: input.kind,
        ref_id: input.refId,
        stance: input.stance,
        note: input.note ?? '',
      },
    })
    await refreshCurrent()
  }

  /**
   * 摘掉一条证据
   *
   * @param evidenceId - 证据行 ID
   */
  async function removeEvidence(evidenceId: number) {
    await invoke<void>('thesis_remove_evidence', { evidenceId })
    await refreshCurrent()
  }

  /**
   * 反查：某条来源被挂到了哪些论点
   *
   * 阅读文章时用来显示"这条我评过没有"，避免重复挂载。
   *
   * @param kind - 来源类型
   * @param refId - 来源行 ID
   * @returns `[{ thesisId, stance }]`
   */
  async function evidenceOf(kind: EvidenceKind, refId: number) {
    return invoke<Array<[number, EvidenceStance]>>('thesis_evidence_of', { kind, refId })
  }

  /**
   * 一条判断的证据来水构成（按来源名去重）
   *
   * # 为什么按论点 ID 缓存成 map
   * 这个数只在详情页顶部显示一行，但详情页打开、加证据、摘证据都会变。
   * 缓存成 map 而不是单个值，是为了切回上一条判断时不用重新请求——
   * 那是最常见的动作（对比两条判断的证据强度）。
   *
   * @param thesisId - 论点 ID
   * @returns 分歧度（失败时返回 null，界面不显示这一行即可，不打断阅读）
   */
  async function loadDivergence(thesisId: number): Promise<Divergence | null> {
    try {
      const d = await invoke<Divergence>('thesis_divergence', { thesisId })
      divergenceById.value = { ...divergenceById.value, [thesisId]: d }
      return d
    } catch (e) {
      // 统计失败是"少一行信息"，不是错误：不写 error、不弹 toast，
      // 否则用户会在详情页看到一个与自己的动作无关的红字
      console.error('统计来源分歧失败:', e)
      return null
    }
  }

  return {
    list,
    filterStatus,
    filterEntityId,
    loading,
    detail,
    detailLoading,
    error,
    stats,
    loadList,
    openThesis,
    closeThesis,
    refreshCurrent,
    create,
    update,
    composeMarkdown,
    remove,
    addEvidence,
    removeEvidence,
    evidenceOf,
    divergenceById,
    loadDivergence,
  }
})

// 开发期热更新：用新定义重建 Pinia 实例。
// 少了这段，Vite 热更新只重跑本模块，而 pinia 缓存里仍是旧实例的 action 集合，
// 于是新组件调用本轮新增的方法会拿到 undefined，TypeError 被事件处理器吞掉，
// 界面表现是「点了没反应」；生产构建不会进入本分支。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useThesesStore, import.meta.hot))
}
