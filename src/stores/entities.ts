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

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Entity } from '@/types'

export const useEntitiesStore = defineStore('entities', () => {
  /** 实体列表缓存 */
  const entities = ref<Entity[]>([])

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

  return {
    entities,
    loading,
    error,
    loadEntities,
    createEntity,
    deleteEntity,
    toggleEntity,
    scoreArticles
  }
})
