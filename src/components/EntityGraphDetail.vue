<template>
  <!-- 详情卡：点击节点后浮在画布左上角，显示该实体的类型 / 命中数与共现最强的邻居 -->
  <div class="eg-detail">
    <div class="eg-detail-head">
      <span class="eg-detail-name">{{ name }}</span>
      <button class="eg-detail-close" @click="emit('close')" title="关闭">×</button>
    </div>
    <p class="eg-detail-meta">{{ typeLabel }} · 命中 {{ articleCount }} 篇</p>
    <p class="eg-detail-sub">共现最强的关联实体</p>
    <ul class="eg-detail-list">
      <li v-for="nb in neighbors" :key="nb.id">
        <span class="eg-detail-nb">{{ nb.name }}</span>
        <span class="eg-detail-w">共现 {{ nb.weight }} 篇</span>
      </li>
      <li v-if="neighbors.length === 0" class="eg-detail-empty">没有与其他实体共现</li>
    </ul>
  </div>
</template>

<script setup lang="ts">
/**
 * 实体关系图的详情卡
 *
 * # 职责
 * 纯展示：渲染选中实体的名称 / 类型 / 命中数与共现邻居列表。
 * 数据全部由父组件（EntityGraph）从图数据派生后以 props 传入，
 * 选中态归父组件所有——本组件只上抛「关闭」。
 *
 * # 样式边界
 * `.eg-detail*` 样式随组件搬入；画布容器（`.eg-wrap`）与定位相关的样式仍在父组件，
 * 卡片以 `position: absolute` 锚定其上（父容器的 scoped 样式管不到这里的类名，
 * 但 absolute 定位只依赖祖先有 `position: relative`，语义成立）。
 */

defineProps<{
  /** 选中实体名称 */
  name: string
  /** 已格式化的类型中文标签（由父组件经 entityTypeLabel 派生） */
  typeLabel: string
  /** 命中文章数 */
  articleCount: number
  /** 共现邻居（父组件已按共现数降序） */
  neighbors: Array<{ id: number; name: string; weight: number }>
}>()

const emit = defineEmits<{
  /** 点击 ×：父组件把选中态置空 */
  (e: 'close'): void
}>()
</script>

<style scoped>
/* 样式自 EntityGraph 原样搬入（.eg-detail 全族）。 */
.eg-detail {
  position: absolute;
  top: var(--sp-15);
  left: var(--sp-15);
  width: 210px;
  max-height: calc(100% - var(--sp-5));
  display: flex;
  flex-direction: column;
  padding: var(--sp-15);
  border: 1px solid var(--panel-border);
  border-radius: var(--r-lg);
  background: var(--surface);
  box-shadow: var(--shadow-pop);
}

.eg-detail-head {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
}

.eg-detail-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--text-primary);
}

.eg-detail-close {
  border: none;
  background: none;
  color: var(--text-tertiary);
  cursor: pointer;
  font-size: 15px;
  line-height: 1;
}

.eg-detail-close:hover {
  color: var(--text-primary);
}

.eg-detail-meta {
  margin: var(--sp-025) 0 var(--sp-1);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.eg-detail-sub {
  margin: 0 0 var(--sp-05);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

.eg-detail-list {
  /* flex:1 + min-height:0：列表自己滚，卡片不被撑破 */
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 0;
  padding: 0;
  list-style: none;
  font-size: var(--fs-sm);
}

.eg-detail-list li {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-025) 0;
}

.eg-detail-nb {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
}

.eg-detail-w {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.eg-detail-empty {
  color: var(--text-tertiary);
  font-size: var(--fs-xs);
}
</style>
