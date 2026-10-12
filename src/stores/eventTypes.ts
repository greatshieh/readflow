/**
 * 事件类型 Store（Pinia）
 *
 * # 职责
 * 聚合「生效事件类型枚举」所需的两类数据：
 * 1. 启用的预设领域（settings.enabled_event_domains，JSON 数组）；
 * 2. 用户自定义类型（custom_event_types 表，经 list_custom_event_types 命令读取）。
 * 二者合并为 `effectiveTypes`，供时间线筛选、告警规则、仪表盘等统一消费。
 *
 * # 设计意图
 * - **数据落库在 Rust 侧**：启用领域写 settings 表，自定义类型写 custom_event_types 表；
 *   前端仅通过 `invoke()` 读取与变更，并维护本地缓存。
 * - **单一事实来源**：各处原本散落的硬编码 `TYPE_LABELS` 改为读取本 store 的
 *   `effectiveTypes`，新增类型时只需改后端 PRESET_DOMAINS + 本文件 PRESET_EVENT_TYPES。
 *
 * # Tauri invoke 参数约定
 * 前端传参一律使用 camelCase，Rust 端以 snake_case 接收，Tauri 自动转换。
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from './settings'
import {
  mergeEffectiveEventTypes,
  type CustomEventTypeItem,
  type EventTypeItem,
} from '@/utils/eventTypeLabels'

export const useEventTypesStore = defineStore('eventTypes', () => {
  const settingsStore = useSettingsStore()

  /** 用户启用的预设领域 key 列表（来自 settings.enabled_event_domains） */
  const enabledDomains = ref<string[]>([])
  /** 自定义事件类型列表（来自 custom_event_types 表） */
  const customTypes = ref<CustomEventTypeItem[]>([])
  /** 加载状态 */
  const loading = ref(false)

  /** 生效事件类型枚举 = 启用预设领域 ∪ 自定义类型（去重，数据化渲染的唯一输入） */
  const effectiveTypes = computed<EventTypeItem[]>(() =>
    mergeEffectiveEventTypes(enabledDomains.value, customTypes.value),
  )

  /**
   * 从 settings 读取并解析启用的预设领域
   *
   * @returns 无返回值；解析失败（非法 JSON）时退化为空列表
   */
  function loadEnabledDomains() {
    const raw = settingsStore.getSetting('enabled_event_domains')
    if (!raw) {
      enabledDomains.value = []
      return
    }
    try {
      const parsed = JSON.parse(raw)
      enabledDomains.value = Array.isArray(parsed) ? (parsed as string[]) : []
    } catch {
      enabledDomains.value = []
    }
  }

  /**
   * 从后端加载自定义事件类型
   *
   * @returns 无返回值；失败时清空列表（仅影响可选项数量，不阻断页面）
   */
  async function loadCustomTypes() {
    try {
      customTypes.value = await invoke<CustomEventTypeItem[]>('list_custom_event_types')
    } catch (e) {
      console.error('加载自定义事件类型失败:', e)
      customTypes.value = []
    }
  }

  /**
   * 全量加载（启用领域 + 自定义类型）
   *
   * @returns 无返回值
   */
  async function loadAll() {
    loading.value = true
    try {
      loadEnabledDomains()
      await loadCustomTypes()
    } finally {
      loading.value = false
    }
  }

  /**
   * 写回启用的预设领域到 settings（JSON 数组）
   *
   * @param domains - 选中的领域 key 列表
   * @throws 后端失败时原样抛出
   */
  async function saveEnabledDomains(domains: string[]) {
    enabledDomains.value = domains
    await settingsStore.setSetting('enabled_event_domains', JSON.stringify(domains))
  }

  /**
   * 新增一个自定义事件类型并刷新列表
   *
   * @param value - 事件类型取值（唯一，会落库到 research_events.event_type）
   * @param label - 展示文案（可选，为空则取 value）
   * @throws 后端失败时原样抛出
   */
  async function addCustomType(value: string, label?: string) {
    // 归一化：value 作为匹配键统一小写 + 去空格，避免 "Clinical" ≠ "clinical"
    // 导致抽取时 validate_events 静默丢弃；展示文案 label 保留用户原样（缺省取原 value）
    const normalized = value.trim().toLowerCase()
    const displayLabel = label && label.trim() ? label.trim() : value.trim()
    await invoke('add_custom_event_type', { value: normalized, label: displayLabel })
    await loadCustomTypes()
  }

  /**
   * 删除一个自定义事件类型并刷新列表
   *
   * @param id - 类型主键
   * @throws 后端失败时原样抛出
   */
  async function removeCustomType(id: number) {
    await invoke('remove_custom_event_type', { id })
    await loadCustomTypes()
  }

  return {
    enabledDomains,
    customTypes,
    loading,
    effectiveTypes,
    loadAll,
    loadEnabledDomains,
    loadCustomTypes,
    saveEnabledDomains,
    addCustomType,
    removeCustomType,
  }
})
