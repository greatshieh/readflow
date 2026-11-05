/**
 * 实体管理 Store（Pinia）
 *
 * # 职责
 * 管理用户关注的实体（公司、人物、产品等），这些实体用于文章智能评分：
 * 文章标题和摘要中出现的实体越多，评分越高。
 *
 * # 设计意图
 * - **数据落库在 Rust 侧**：所有读写操作通过 `invoke()` 调用 Rust 后端命令，
 *   前端仅负责状态管理与 UI 交互。
 * - **实体与评分联动**：当实体列表变化时，自动触发 `score_articles` 命令
 *   重新计算所有文章评分，保持数据一致性。
 * - **乐观更新**：切换实体启用状态时，先更新本地状态再等待后端确认，
 *   提供流畅的用户体验。
 *
 * # Tauri invoke 参数约定
 * 前端传参一律使用 **camelCase**，Rust 端以 snake_case 接收，Tauri 自动转换。
 */

import { acceptHMRUpdate, defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  Entity,
  EntityBlock,
  EntityCreateOutcome,
  EntityImpact,
  MergeCandidate,
  MergeReport,
} from '@/types'

export const useEntitiesStore = defineStore('entities', () => {
  /** 实体列表缓存 */
  const entities = ref<Entity[]>([])

  /** 被删除（屏蔽）的实体名。删除 = 从库里移除 + 名字进屏蔽表，AI 不再自动重建 */
  const blocked = ref<EntityBlock[]>([])

  /** 加载状态 */
  const loading = ref(false)

  /** 错误信息 */
  const error = ref<string | null>(null)

  /**
   * 从后端加载所有实体列表
   *
   * @returns 无返回值，结果写入 `entities`
   */
  async function loadEntities() {
    loading.value = true
    error.value = null
    try {
      const data = await invoke<Entity[]>('entities_list')
      entities.value = data
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      console.error('加载实体列表失败:', e)
    } finally {
      loading.value = false
    }
  }

  /**
   * 新增实体（自动完成命名解析与别名补全）
   *
   * 后端会先判"这个名字是不是已有实体的另一种写法"：是则**并入**（返回
   * `merged_into`），否则新建并把模型给的中英文写法作为初始别名。
   * 调用方必须看返回值而不是看"列表是否变长"——并入时列表只多了一个别名。
   *
   * @param name - 实体名称（如 "腾讯"、"Google"）
   * @param entityType - 实体类型（company / person / product / industry / other）
   * @returns 新建或并入的结果
   * @throws 后端失败时原样抛出
   */
  async function createEntity(name: string, entityType: string): Promise<EntityCreateOutcome> {
    try {
      const outcome = await invoke<EntityCreateOutcome>('entities_create', {
        name,
        entityType
      })
      await loadEntities()
      return outcome
    } catch (e) {
      console.error('创建实体失败:', e)
      throw e
    }
  }

  /**
   * 把一个实体并入另一个（不可逆）
   *
   * 后端在一个事务里搬七张表的引用，撞唯一约束的丢并入方一侧。
   * 调用方必须先让用户看到两侧影响面并确认——这不是"改个名"。
   *
   * @param keepId - 保留方实体 ID
   * @param mergeId - 并入方实体 ID（完成后该行被删除）
   * @returns 搬家与丢弃的明细
   * @throws 后端失败时原样抛出
   */
  async function mergeEntities(keepId: number, mergeId: number): Promise<MergeReport> {
    try {
      const report = await invoke<MergeReport>('entities_merge', { keepId, mergeId })
      // 保留方的别名表变了（并入方名字成为别名），必须重新拉取才对得上；
      // 并入方那一行已不存在，随这次拉取一起消失
      await loadEntities()
      return report
    } catch (e) {
      console.error('合并实体失败:', e)
      throw e
    }
  }

  /**
   * 覆盖某个实体的别名表（人工编辑）
   *
   * @param entityId - 实体 ID
   * @param aliases - 完整别名表
   * @returns 实际写入的别名表（后端已去空、去重、去掉与主名重复的项）
   * @throws 后端失败时原样抛出
   */
  async function setAliases(entityId: number, aliases: string[]): Promise<string[]> {
    try {
      const written = await invoke<string[]>('entities_set_aliases', { entityId, aliases })
      // 本地列表里的别名字符串同步更新，免得弹窗关掉后行内还是旧值
      const entity = entities.value.find(e => e.id === entityId)
      if (entity) {
        entity.aliases = JSON.stringify(written)
      }
      return written
    } catch (e) {
      console.error('保存别名失败:', e)
      throw e
    }
  }

  /**
   * 让模型为某个实体补全别名（只返回建议，不落库）
   *
   * @param entityId - 实体 ID
   * @returns 建议新增的别名
   * @throws AI 未配置或后端失败时原样抛出
   */
  async function suggestAliases(entityId: number): Promise<string[]> {
    try {
      return await invoke<string[]>('entities_suggest_aliases', { entityId })
    } catch (e) {
      console.error('AI 补全别名失败:', e)
      throw e
    }
  }

  /**
   * 全库查重：让模型找出可能指代同一实体的条目（只出候选）
   *
   * @returns 候选组；没有可疑项时为空数组
   * @throws AI 未配置或后端失败时原样抛出
   */
  async function suggestMerges(): Promise<MergeCandidate[]> {
    try {
      return await invoke<MergeCandidate[]>('entities_suggest_merges')
    } catch (e) {
      console.error('AI 查重失败:', e)
      throw e
    }
  }

  /**
   * 设置实体的归属（产品 / 子公司 / 人物 → 公司）
   *
   * 归属只影响展示与分组，不改变评分与事件归属。后端会拒绝"挂到自己"
   * "上级不是公司"和任何成环的写法，错误原样抛给调用方展示。
   *
   * @param entityId - 被设置的实体
   * @param parentId - 上级公司 ID；null 表示取消归属
   * @throws 后端拒绝（成环 / 非公司 / 不存在）时原样抛出
   */
  async function setParent(entityId: number, parentId: number | null): Promise<void> {
    try {
      await invoke<void>('entities_set_parent', { entityId, parentId })
      // 本地列表同步，避免关闭弹窗后行内仍显示旧归属
      const entity = entities.value.find(e => e.id === entityId)
      if (entity) {
        entity.parent_id = parentId
      }
    } catch (e) {
      console.error('设置实体归属失败:', e)
      throw e
    }
  }

  /**
   * 删除实体
   *
   * 后端在同一次调用里做两件事：把名字写进屏蔽表、删掉实体行（关联表由
   * 外键级联清除）。只删行的话，下一轮 AI 提取会把它当新实体建回来。
   *
   * 删除不可逆，调用方应先用 {@link fetchImpact} 取影响面并让用户确认。
   *
   * @param id - 实体 ID
   * @throws 后端失败时原样抛出
   */
  async function deleteEntity(id: number) {
    try {
      await invoke<void>('entities_delete', { id })
      // 乐观更新：从本地列表移除
      entities.value = entities.value.filter(e => e.id !== id)
    } catch (e) {
      console.error('删除实体失败:', e)
      throw e
    }
  }

  /**
   * 切换实体启用状态
   *
   * @param id - 实体 ID
   * @returns 切换后的启用状态
   */
  async function toggleEntity(id: number): Promise<boolean> {
    try {
      const isEnabled = await invoke<boolean>('entities_toggle', { id })
      // 乐观更新
      const entity = entities.value.find(e => e.id === id)
      if (entity) {
        entity.enabled = isEnabled
      }
      return isEnabled
    } catch (e) {
      console.error('切换实体状态失败:', e)
      throw e
    }
  }

  /**
   * 触发文章智能评分
   *
   * 遍历所有已启用实体，统计每篇文章标题和摘要中的命中数，
   * 将结果写入 `articles.entity_score`。
   *
   * @param articleIds - 指定文章 ID 列表（为空时全库评分）
   * @returns 实际更新的文章条数
   */
  async function scoreArticles(articleIds?: number[]): Promise<number> {
    try {
      const count = await invoke<number>('score_articles', {
        articleIds: articleIds ?? []
      })
      return count
    } catch (e) {
      console.error('评分失败:', e)
      throw e
    }
  }

  /**
   * 读取实体的告警规则 JSON（实体级告警 P1-F）
   *
   * @param id - 实体 ID
   * @returns 规则 JSON 字符串；未配置时返回 null
   * @throws 后端失败时原样抛出
   */
  async function getAlertRules(id: number): Promise<string | null> {
    try {
      return await invoke<string | null>('entities_get_alert_rules', { entityId: id })
    } catch (e) {
      console.error('读取实体告警规则失败:', e)
      throw e
    }
  }

  /**
   * 写入实体的告警规则 JSON（实体级告警 P1-F）
   *
   * @param id - 实体 ID
   * @param rulesJson - 规则 JSON 字符串（空串表示清空规则）
   * @throws 后端失败时原样抛出
   */
  async function setAlertRules(id: number, rulesJson: string): Promise<void> {
    try {
      await invoke<void>('entities_set_alert_rules', { entityId: id, rulesJson })
    } catch (e) {
      console.error('保存实体告警规则失败:', e)
      throw e
    }
  }

  /**
   * 统计删除某实体会连带清掉多少数据
   *
   * 删除不可逆（研究事件 / 论点 / 覆盖 / 市场标的 / 财务会一并清除），
   * 所以确认框必须先拿到这些数字再说"确定删除"。
   *
   * @param id - 实体 ID
   * @returns 各关联表条数；实体不存在时全为 0
   * @throws 后端失败时原样抛出
   */
  async function fetchImpact(id: number): Promise<EntityImpact> {
    try {
      return await invoke<EntityImpact>('entities_impact', { id })
    } catch (e) {
      console.error('统计实体影响面失败:', e)
      throw e
    }
  }

  /**
   * 加载被删除（屏蔽）的实体名
   *
   * @returns 无返回值，结果写入 `blocked`
   */
  async function loadBlocked(): Promise<void> {
    try {
      blocked.value = await invoke<EntityBlock[]>('entities_blocked')
    } catch (e) {
      console.error('加载已屏蔽实体失败:', e)
      // 这里刻意不抛：屏蔽列表是次要信息，取不到不该让整个页签报错
    }
  }

  /**
   * 解除屏蔽：该名字之后又能被自动创建
   *
   * 只取消拦截，**不会**把实体建回来——恢复是"不再拦着"，
   * 而不是替用户决定重新关注。
   *
   * @param name - 实体名（后端负责归一化）
   * @throws 后端失败时原样抛出
   */
  async function unblockEntity(name: string): Promise<void> {
    try {
      await invoke<void>('entities_unblock', { name })
      blocked.value = blocked.value.filter((b) => b.name !== name)
    } catch (e) {
      console.error('解除屏蔽失败:', e)
      throw e
    }
  }

  return {
    entities,
    blocked,
    loading,
    error,
    loadEntities,
    createEntity,
    mergeEntities,
    setAliases,
    setParent,
    suggestAliases,
    suggestMerges,
    deleteEntity,
    toggleEntity,
    scoreArticles,
    getAlertRules,
    setAlertRules,
    fetchImpact,
    loadBlocked,
    unblockEntity
  }
})

// 开发期热更新：用新定义重建 Pinia 实例。
// 少了这段，Vite 热更新只重跑本模块，而 pinia 缓存里仍是旧实例的 action 集合，
// 于是新组件调用本轮新增的方法会拿到 undefined，TypeError 被事件处理器吞掉，
// 界面表现是「点了没反应」；生产构建不会进入本分支。
if (import.meta.hot) {
  import.meta.hot.accept(acceptHMRUpdate(useEntitiesStore, import.meta.hot))
}
