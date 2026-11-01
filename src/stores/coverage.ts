/**
 * 覆盖清单与「研究一页」的 IPC 层
 *
 * # 职责
 * 覆盖条目的读写（`list` / `save` / `remove`）与组装的取数（`dossier`）。
 * **所有覆盖相关组件都通过本 store 取数与提交，没有组件自己 invoke**：
 * 一条命令两个组件各自调一次，等于两条"什么时候刷新"的逻辑，
 * 迟早出现"改完档位表没变"。
 *
 * # 为什么不缓存 dossier
 * 组装结果依赖证据、判据、变更史三类数据，任何一类在别处被改都要重取。
 * 缓存它就得维护失效，而组装本身只在用户点「生成一页」时跑一次，
 * 一次查询换掉一整套失效逻辑是划算的。
 *
 * # 序列化约定
 * 出参是 Rust 的 camelCase（`entityId` / `thesisCount` / `eventCount30d`），
 * 入参 `CoverageInput` 是前端的 **snake_case**（`entity_id` / `exit_condition`），
 * 与 `FalsifierInput`、`ReviewMarkInput` 一致。写错会硬失败
 * `invalid args for command`，不会静默写空值。
 */
import { acceptHMRUpdate, defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

/**
 * 覆盖档位
 *
 * `core` 核心 / `watch` 观察 / `exit` 本月退出。
 * 三档的中文名在组件里映射，不在这里拼——这里是契约，不是文案。
 */
export type CoverageTier = 'core' | 'watch' | 'exit'

/** 覆盖清单的一行（与 Rust `CoverageRow` 对齐） */
export interface CoverageRow {
  entityId: number
  entityName: string
  entityType: string
  tier: CoverageTier
  reason: string
  exitCondition: string
  positionNote: string
  updatedAt: string
  /** 在跟踪的论点条数（不含归档） */
  thesisCount: number
  /** 未结案的判据条数 */
  falsifierOpen: number
  /** 最近一次被复盘处置的时间，未复盘过为 null */
  lastReviewAt: string | null
  /** 近 30 天事件条数 */
  eventCount30d: number
}

/** 写入入参（内层 snake_case，见文件头） */
export interface CoverageInput {
  entity_id: number
  tier: CoverageTier
  reason: string
  exit_condition: string
  position_note: string
}

/** 「研究一页」的生成结果（与 Rust `Dossier` 对齐） */
export interface Dossier {
  entityId: number
  title: string
  /** 另存为对话框的预填名，不含扩展名 */
  defaultName: string
  /** markdown 原文：复制与导出都用它，前端不做二次拼装 */
  markdown: string
  thesisCount: number
  evidenceCount: number
}

export const useCoverageStore = defineStore('coverage', () => {
  /** 覆盖清单 */
  const rows = ref<CoverageRow[]>([])
  /** 是否正在加载 */
  const loading = ref(false)
  /** 最近一次错误（清空时机：下次成功加载） */
  const error = ref('')

  /**
   * 重新读取覆盖清单
   *
   * @returns 无返回值；结果写入 `rows`，失败写入 `error`（并抛出，供调用方决定是否 toast）
   */
  async function load() {
    loading.value = true
    error.value = ''
    try {
      rows.value = await invoke<CoverageRow[]>('coverage_list')
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      throw e
    } finally {
      loading.value = false
    }
  }

  /**
   * 新增或更新一条覆盖，成功后重新拉清单
   *
   * 不本地改 `rows` 再对齐：`list` 里带着四个计数（论点数 / 待验判据 /
   * 上次复盘 / 30 天事件），本地拼不出来。重拉一次的代价是一条查询，
   * 换掉"本地与后端算法各写一遍"的风险。
   *
   * @param input - 覆盖内容
   * @returns 标的 ID
   */
  async function save(input: CoverageInput): Promise<number> {
    const id = await invoke<number>('coverage_upsert', { input })
    await load()
    return id
  }

  /**
   * 把标的移出覆盖（只删覆盖行，不动实体与论点）
   *
   * @param entityId - 标的 ID
   */
  async function remove(entityId: number) {
    await invoke('coverage_remove', { entityId })
    await load()
  }

  /**
   * 生成「研究一页」
   *
   * 失败直接抛出、不落 `error`：这个动作有明确的发起者与即时反馈（弹窗里显示原因），
   * 写进全局 error 反而会和「清单加载失败」混在一起。
   *
   * @param entityId - 标的 ID
   * @returns 组装结果（含 markdown 原文）
   */
  async function dossier(entityId: number): Promise<Dossier> {
    return invoke<Dossier>('coverage_dossier', { entityId })
  }

  return { rows, loading, error, load, save, remove, dossier }
})

// 开发期热更新：用新定义重建 Pinia 实例。
// 直接调 `acceptHMRUpdate` 不生效（它只返回回调），必须交回 `import.meta.hot.accept`。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useCoverageStore, import.meta.hot))
}
