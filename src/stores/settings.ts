/**
 * 应用设置 Store（Pinia）
 *
 * # 职责
 * 管理 ReadFlow 的全局配置项（AI 模型配置、主题偏好等），是前端读写"设置"的**唯一入口**。
 * 对外暴露 `useSettingsStore()`，消费方可拿到 `settings` 列表与
 * `loadSettings` / `setSetting` / `getSetting` 三个操作。
 *
 * # 设计意图
 * - **数据落库在 Rust 侧**：本 store 自身不做任何持久化，所有读写都通过
 *   `invoke()` 交给 Rust 后端（对应 `db.rs` 中 `settings` 表的 key-value 结构，
 *   值统一为字符串，复杂配置由调用方自行 JSON 序列化后存入）。
 *   这正是"前端只做展示 + 必要数据处理"这一核心架构规则的体现。
 * - **内存缓存优先**：`settings` 在 `loadSettings()` 后被完整缓存到内存，
 *   `getSetting()` 直接从缓存读取，避免每次渲染都穿透到后端。
 * - **写后同步**：`setSetting()` 采取"先落库成功、再更新本地缓存"的顺序，
 *   保证内存与数据库不会出现"本地显示已保存但后端失败"的假成功状态。
 *
 * # 使用方
 * 设置面板（AI 提供商配置、主题切换等需要持久化偏好的界面）。
 *
 * # 注意事项
 * - Tauri invoke 参数约定：前端一律传 **camelCase**（如 `feedId`、`articleId`），
 *   Rust 端用 snake_case 接收，Tauri 自动完成转换。本文件的 `key` / `value`
 *   为单词名无需转换，但若后续新增多词参数，务必保持 camelCase。
 * - 与 `feeds.ts` / `articles.ts` 不同，本文件是**静态** `import { invoke }`，
 *   因此没有浏览器预览模式的 mock 回退：`npm run dev` 下调用本 store 会直接抛错。
 *   真实 Tauri 桌面应用中不存在该问题（内建环境检测见 `@tauri-apps/api/core`）。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
// `@/types` 导出的是单数形式的 `Setting`（对应 settings 表中的一行 key-value 记录），
// 因此 `settings` 列表的类型为 `Setting[]`
import type { Setting } from '@/types'

export const useSettingsStore = defineStore('settings', () => {
  /** 全量设置项缓存（key-value 列表，与 Rust 侧 settings 表一一对应） */
  const settings = ref<Setting[]>([])

  /**
   * 从 Rust 后端加载全部设置项并覆盖本地缓存
   *
   * 采用"整体覆盖"而非"增量合并"，原因：设置项数量很小（几十条以内），
   * 全量拉取的成本远低于维护增量同步逻辑带来的复杂度与不一致风险。
   *
   * @returns 无返回值，结果直接写入 store 的 `settings`
   * @throws 后端 `settings_get_all` 命令失败时，Promise 会 reject（本函数不吞异常，
   *         由调用方决定是否提示用户）；失败时 `settings` 保持调用前的旧值
   */
  async function loadSettings() {
    settings.value = await invoke<Setting[]>('settings_get_all')
  }

  /**
   * 写入单个设置项（先落库，成功后同步本地缓存）
   *
   * 先 await 后端写入再改内存，是为了避免"后端失败但前端显示已保存"的假成功；
   * 更新缓存而非重新拉取全量，是为了让设置面板的连续修改保持低延迟。
   *
   * @param key - 设置键（如 `ai_openai_url`、`theme`），与 Rust 侧主键一致
   * @param value - 设置值（原始字符串；复杂结构请调用方先 `JSON.stringify`）
   * @returns 无返回值；副作用为更新 store 中 `settings` 对应项（不存在则追加）
   * @throws 后端 `settings_set` 命令失败时 reject，此时本地缓存不会被修改
   */
  async function setSetting(key: string, value: string) {
    // 参数名 key / value 与 Rust 端 snake_case 形参同名，无需 camelCase 转换
    await invoke<void>('settings_set', { key, value })
    const setting = settings.value.find(s => s.key === key)
    if (setting) {
      setting.value = value
    } else {
      // 新增项：后端已写入成功，这里补进缓存以免下次渲染取不到
      settings.value.push({ key, value })
    }
  }

  /**
   * 从本地缓存读取单个设置项（同步、不触网、不触后端）
   *
   * 设计为同步读取是因为它常被模板/计算属性高频调用；调用前请确保
   * 至少执行过一次 `loadSettings()`，否则会拿到 undefined。
   *
   * @param key - 设置键
   * @returns 对应的设置值；缓存未命中（键不存在或尚未加载）时返回 `undefined`
   */
  function getSetting(key: string): string | undefined {
    return settings.value.find(s => s.key === key)?.value
  }

  return { settings, loadSettings, setSetting, getSetting }
})
