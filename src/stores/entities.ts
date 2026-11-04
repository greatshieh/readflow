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
import type { Entity, EntityBlock, EntityImpact } from '@/types'

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
   * 新增实体
   *
   * @param name - 实体名称（如 "腾讯"、"GOOG"）
   * @param entityType - 实体类型（company / person / product / industry / other）
   * @returns 新实体的 ID
   * @throws 后端失败时原样抛出
   */
  async function createEntity(name: string, entityType: string): Promise<number> {
    try {
      const id = await invoke<number>('entities_create', {
        name,
        entityType
      })
      await loadEntities()
      return id
    } catch (e) {
      console.error('创建实体失败:', e)
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
