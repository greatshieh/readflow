<template>
  <!-- 排序栏：切换后由父组件按当前范围重新从第 0 页加载（排序由后端在全库范围内完成）。
       书签视图额外露出「分组」维度——默认按标签（收藏夹），可切回时间。 -->
  <div class="sort-bar">
    <span>排序：</span>
    <button
      v-for="opt in SORT_OPTIONS"
      :key="opt.key"
      class="sort-btn"
      :class="{ active: articlesStore.sortMode === opt.key }"
      @click="emit('change-sort', opt.key)"
    >
      {{ opt.label }}
    </button>
    <template v-if="articlesStore.viewFilter === 'bookmarked'">
      <span class="sort-sep" aria-hidden="true"></span>
      <span>分组：</span>
      <button
        class="sort-btn"
        :class="{ active: groupMode === 'tag' }"
        @click="emit('update:groupMode', 'tag')"
        title="把标签当作收藏夹分组"
      >
        标签
      </button>
      <button
        class="sort-btn"
        :class="{ active: groupMode === 'time' }"
        @click="emit('update:groupMode', 'time')"
        title="按发布时间分组"
      >
        时间
      </button>
    </template>
  </div>
</template>

<script setup lang="ts">
/**
 * 文章列表的排序栏
 *
 * # 职责
 * 纯展示 + 上抛：渲染排序选项与（书签视图下的）分组维度切换。
 * 排序动作（清缩略图缓存 + 重新加载）与分组维度状态都归父组件（ArticleColumn）所有，
 * 因为 `groupedArticles` 与分组头渲染依赖同一份 `bookmarkGroupMode`。
 *
 * # 与 store 的关系
 * 当前排序与视图筛选直接读 `articlesStore`（只读展示，不写），
 * 与 ArticleListToolbar 的边界约定一致：展示态读 store，动作 emit 回父组件。
 */

import { useArticlesStore, type ArticleSortKey } from '@/stores/articles'

defineProps<{
  /** 书签视图的分组维度（父组件持有，v-model:group-mode） */
  groupMode: 'tag' | 'time'
}>()

const emit = defineEmits<{
  /** 切换排序方式；重载逻辑在父组件 */
  (e: 'change-sort', key: ArticleSortKey): void
  /** 切换书签视图的分组维度 */
  (e: 'update:groupMode', mode: 'tag' | 'time'): void
}>()

const articlesStore = useArticlesStore()

/**
 * 排序栏的可选项（顺序即展示顺序）
 *
 * `key` 直接透传给后端 `articles_list` 的 `sort` 参数，同时用于高亮当前项——
 * 用一份数据驱动「渲染 + 选中态 + 请求参数」，避免三个地方各写一遍中文标签。
 */
const SORT_OPTIONS: { key: ArticleSortKey; label: string }[] = [
  { key: 'latest', label: '最新' },
  { key: 'score', label: '重要' },
  { key: 'title', label: '名称' }
]
</script>

<style scoped>
/* 样式自 ArticleColumn 原样搬入。 */
.sort-bar {
  display: flex;
  align-items: center;
  /* 书签视图会多出「分组：标签 / 时间」三项，窄窗口下允许换行而不是把按钮压扁 */
  flex-wrap: wrap;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  /* Aurora：与列表栏其它分区一致改用发丝线，--border 在玻璃面上偏重 */
  border-bottom: 1px solid var(--shell-hair);
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  flex-shrink: 0;
  /* 去掉 --surface-muted 实底：三条同色实底叠在一起是"单薄"的来源之一。
     改为极淡的主色，让排序栏读作"列表栏里的一个次级带"而非另一块面板。 */
  background: color-mix(in srgb, var(--primary) 3%, transparent);
}
/* 隔开「排序」与「分组」两组控件，让它们看起来是两件事而非一串 */
.sort-sep {
  width: 1px;
  align-self: stretch;
  margin: var(--sp-025) var(--sp-05);
  background: var(--border);
}
.sort-btn {
  padding: 3px var(--sp-2);
  border-radius: var(--r-sm);
  cursor: pointer;
  transition: background var(--dur) var(--ease), border-color var(--dur) var(--ease), color var(--dur) var(--ease);
  border: 1px solid transparent;
  background: none;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  font-family: inherit;
}
.sort-btn:hover { background: var(--fill); color: var(--text-primary); border-color: var(--border); }
/* 选中：从"紫底紫字"改为**淡底 + 描边 + 加粗**。
   紫色文字压在紫色淡底上是最容易显脏的组合（对比 3.4:1 上下），
   改用中性主文字色 + 主色描边后对比 12:1，且"选中"由描边而非色块承担，
   与全局的胶囊体系（浅色实底、深色描边）保持一致。 */
.sort-btn.active {
  background: var(--primary-soft);
  color: var(--primary);
  border-color: color-mix(in srgb, var(--primary) 40%, transparent);
  font-weight: 600;
}
</style>
