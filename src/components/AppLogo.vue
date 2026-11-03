<template>
  <!--
    应用标志：一个平涂的单色符号（不是渐变圆形徽章）。
    「一道弧 + 一个指向左的回钩」＝ 把内容拉回来自己读，这是本地优先阅读器的形状。
  -->
  <span class="app-logo" :style="logoStyle" aria-hidden="true">
    <svg viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
      <!-- 弧：从左上起笔，绕过右侧，落回下方 -->
      <path
        d="M32 14 C21 14 14 23 14 32 C14 41 21 50 32 50"
        stroke="currentColor"
        stroke-width="6"
        stroke-linecap="round"
        fill="none"
      />
      <!-- 回钩：指向左，与弧的收笔同一点出发 -->
      <path
        d="M40 27 L32 34 L40 41"
        stroke="currentColor"
        stroke-width="6"
        stroke-linecap="round"
        stroke-linejoin="round"
        fill="none"
      />
    </svg>
  </span>
</template>

<script setup lang="ts">
/**
 * 应用标志组件
 *
 * # 职责
 * 提供全应用唯一的标志图形，供标题栏与正文栏空状态复用。
 *
 * # 为什么从「渐变圆徽章」改成一个平涂符号
 * 旧版是 64px 圆 + 斜向紫罗兰渐变填充 + 白色描边箭头，等于一个**渐变球**。
 * 它是"一眼 AI"清单里排在最前的形状之一：圆、渐变、白图形，
 * 三个特征叠加后在任何产品上都成立，因此不携带任何产品信息。
 * 现在去掉圆底与渐变，只留两道 6px 圆头笔画，用 `--primary` 单一色。
 * 在标题栏 24px、空态 40px 两个尺寸下都仍然清楚。
 *
 * # 与主题色的关系
 * 走 `currentColor` + `--primary`：标志不再单独持有一套品牌色变量
 * （原 `--brand-from` / `--brand-to` 已随之退化为同值，保留仅为兼容引用）。
 */

import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    /** 标志边长（px），正方形；默认与标题栏原有尺寸一致 */
    size?: number
  }>(),
  { size: 24 },
)

/** 外层容器的尺寸样式：size 单一来源，SVG 内部靠 viewBox 自适应 */
const logoStyle = computed(() => ({
  width: `${props.size}px`,
  height: `${props.size}px`,
}))
</script>

<style scoped>
.app-logo {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--primary);
  flex-shrink: 0;
}

.app-logo svg {
  width: 100%;
  height: 100%;
  display: block;
}
</style>
