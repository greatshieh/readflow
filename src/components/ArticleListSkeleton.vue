<template>
  <!-- 文章列表加载态骨架屏：按真实条目（ArticleListItem）几何铺排——
       左侧「标题行 + meta 行」，右侧 52px 方形缩略图位。 -->
  <div class="skeleton-list" aria-hidden="true">
    <div v-for="i in rows" :key="i" class="skeleton-row">
      <div class="sk-info">
        <span class="sk-line"></span>
        <span class="sk-meta">
          <span class="sk-line sk-line-short"></span>
        </span>
      </div>
      <span class="sk-thumb"></span>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 文章列表骨架屏
 *
 * # 职责
 * 纯展示：文章列表加载期间按真实条目几何铺出占位行，避免加载完成时列表跳版。
 * 容器级的滚动 / 对位样式（flex:1、overflow、滚动条配重内边距）仍由
 * ArticleColumn 的 scoped 规则提供（子组件根元素继承父作用域属性，选择器能命中）。
 *
 * # 几何契约
 * padding / gap / 缩略图尺寸必须与 ArticleListItem.vue 的真实条目完全一致；
 * 改动一侧时必须同改另一侧（两侧均有注释标注）。
 */

withDefaults(
  defineProps<{
    /** 占位行数 */
    rows?: number
  }>(),
  { rows: 7 },
)
</script>

<style scoped>
/* 样式自 ArticleColumn 原样搬入。.sk-line / .sk-line-short / @keyframes sk-shimmer
   是全局工具类（styles.css），此处不重复定义；
   .skeleton-list 的容器布局基座也是全局类，这里只补"行"的几何。 */
.skeleton-row {
  gap: var(--sp-4);
  padding: 11px var(--sp-4) 11px var(--sp-3);
  margin: 0 var(--sp-05) var(--sp-05);
}
.sk-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.sk-meta {
  display: flex;
  align-items: center;
}
.sk-meta .sk-line {
  height: 8px;
  width: 40%;
}
.sk-thumb {
  width: 52px;
  height: 52px;
  flex-shrink: 0;
  border-radius: var(--r-md);
  box-shadow: var(--ring-inset);
  background: linear-gradient(
    90deg,
    var(--fill) 0%,
    var(--fill-secondary) 50%,
    var(--fill) 100%
  );
  background-size: 200% 100%;
  animation: sk-shimmer 1.4s ease-in-out infinite;
}
</style>
