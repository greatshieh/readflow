/**
 * 标签 Store（Pinia）
 *
 * # 职责
 * 管理文章标签这一「用户侧整理维度」：标签目录的增删改查、给单篇文章整组设置标签，
 * 以及标签颜色的取值约定。对外暴露 `useTagsStore()`，被 `FeedPanel.vue`（侧边栏标签分区）、
 * `ArticleColumn.vue`（列表项 chip）与 `ContentColumn.vue`（正文打标面板）消费。
 *
 * # 设计意图
 * - **数据落库在 Rust 侧**：所有读写经 `invoke('tags_*' / 'article_tags_set')`，
 *   前端不直连数据库（本地优先架构的前后端边界，见项目铁律）。
 * - **与 articles store 分工明确**：本 store 只持有「标签目录」；文章身上的 `tags`
 *   是文章状态的一部分，因此设置成功后由本 store 反向同步 `useArticlesStore()` 的
 *   内存（`selectedArticle` 与列表项），避免界面上出现「库里改了、界面没变」。
 * - **标签 ≠ 实体**：`entities` 服务于自动评分与研究提取，本 store 与之完全独立。
 *
 * # Tauri invoke 参数约定
 * 前端传参一律使用 **camelCase**（`articleId`、`tagIds`），Rust 端以 snake_case 接收，
 * Tauri 自动完成转换。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Tag } from '@/types'
import { useArticlesStore } from './articles'

/**
 * 标签可选颜色
 *
 * 取值与 `styles.css` 的 `--tone-*` 变量一一对应：库里存**颜色名**而非色值，
 * 色值只在 CSS 里定义一处，日后调整主题或替换色板无需迁移数据。
 */
export const TAG_COLORS = ['green', 'indigo', 'amber', 'red', 'sky', 'violet', 'slate'] as const
export type TagColor = (typeof TAG_COLORS)[number]

/**
 * 把标签颜色名转成可直接写进样式的 CSS 变量引用
 *
 * 空串（新建时未选色）与未知取值统一回退到中性 slate，保证 chip 永远有一个确定外观，
 * 而不是因为 `var(--tone-) ` 这种残缺变量让整条声明静默作废。
 *
 * @param color - 标签的 color 字段
 * @returns 形如 `var(--tone-green)` 的 CSS 变量引用
 */
export function tagColorVar(color: string): string {
  return TAG_COLORS.includes(color as TagColor)
    ? `var(--tone-${color})`
    : 'var(--tone-slate)'
}

export const useTagsStore = defineStore('tags', () => {
  /** 标签目录（含各自文章数），由 `loadTags` 填充 */
  const tags = ref<Tag[]>([])
  /** 加载状态 */
  const loading = ref(false)
  /** 最近一次操作的失败原因（成功时置 null） */
  const error = ref<string | null>(null)

  /**
   * 从后端加载全部标签（带文章数）
   *
   * @returns 无返回值；结果写入 `tags`，失败写入 `error` 并打印日志
   */
  async function loadTags(): Promise<void> {
    loading.value = true
    error.value = null
    try {
      tags.value = await invoke<Array<Tag>>('tags_list')
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      console.error('加载标签失败:', e)
    } finally {
      loading.value = false
    }
  }

  /**
   * 新建标签；后端对同名标签做复用（不报错），因此返回值可能是既有标签的 ID
   *
   * @param name - 标签名（会 trim；空名会被后端拒绝）
   * @param color - 颜色名（见 {@link TAG_COLORS}）
   * @returns 标签 ID（新建的或既有的）
   * @throws 后端失败时原样抛出，由调用方决定是否提示
   */
  async function createTag(name: string, color: string = ''): Promise<number> {
    const id = await invoke<number>('tags_create', { name, color })
    // 立刻重载目录：新建/复用的差异只有后端知道，本地推测容易与库不一致
    await loadTags()
    return id
  }

  /**
   * 重命名标签或改颜色
   *
   * 后端会把引用旧名的过滤规则一并改指新名，故重载目录即可反映最终状态。
   *
   * @param id - 标签 ID
   * @param name - 新名称
   * @param color - 新颜色名
   * @throws 重名或名称为空时后端返回可读错误，原样抛出
   */
  async function updateTag(id: number, name: string, color: string): Promise<void> {
    await invoke<void>('tags_update', { id, name, color })
    await loadTags()
  }

  /**
   * 删除标签
   *
   * 后端级联删除文章关联、并清空引用该标签的规则参数。这里同步清理前端内存：
   * 目录、已加载文章身上的该标签、以及正在生效的标签筛选（否则会停留在
   * 一个已不存在的标签范围上，列表永远为空）。
   *
   * @param id - 标签 ID
   * @throws 后端失败时原样抛出
   */
  async function deleteTag(id: number): Promise<void> {
    await invoke<void>('tags_delete', { id })
    tags.value = tags.value.filter((t) => t.id !== id)

    const articlesStore = useArticlesStore()
    if (articlesStore.tagFilterId === id) {
      articlesStore.setTagFilter(null)
    }
    // 已加载的文章内存里也要摘掉该标签，否则 chip 会残留到下次重载
    for (const a of articlesStore.articles) {
      if (a.tags?.some((t) => t.id === id)) {
        a.tags = a.tags.filter((t) => t.id !== id)
      }
    }
    const sel = articlesStore.selectedArticle
    if (sel?.tags?.some((t) => t.id === id)) {
      sel.tags = sel.tags.filter((t) => t.id !== id)
    }
  }

  /**
   * 整组覆盖某篇文章的标签
   *
   * 前端提交「最终勾选集合」而非增删差集：后端 `article_tags_set` 是覆盖语义，
   * 天然幂等，不必让前端计算差集（差集算错会静默丢标签）。
   *
   * @param articleId - 文章 ID
   * @param tagIds - 该文章最终应有的标签 ID 集合（空数组表示清空）
   * @returns 无返回值；成功后同步 `selectedArticle.tags` 与列表项
   * @throws 后端失败时原样抛出
   */
  async function setArticleTags(articleId: number, tagIds: number[]): Promise<void> {
    await invoke<void>('article_tags_set', { articleId, tagIds })
    // 以目录为准构造顺序稳定的标签数组（按 tagIds 给出的顺序）
    const next: Tag[] = []
    for (const id of tagIds) {
      const t = tags.value.find((x) => x.id === id)
      if (t) next.push(t)
    }
    const articlesStore = useArticlesStore()
    if (articlesStore.selectedArticle?.id === articleId) {
      articlesStore.selectedArticle.tags = next
    }
    const idx = articlesStore.articles.findIndex((a) => a.id === articleId)
    if (idx !== -1) {
      articlesStore.articles[idx].tags = next
    }
    // 文章数变了（增/减关联），重载目录让侧边栏计数与列表 chip 同步
    await loadTags()
  }

  return {
    tags,
    loading,
    error,
    loadTags,
    createTag,
    updateTag,
    deleteTag,
    setArticleTags
  }
})
