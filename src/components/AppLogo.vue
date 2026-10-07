<template>
  <!--
    应用标志：琥珀渐变徽章 + 白色流动箭头。
    徽章底色取自全局 --brand-from / --brand-to，标记为品牌锚点，不随强调色预设变化；
    深色主题下由 styles.css 的暗色段整体提亮一档。
    标题栏与正文栏空状态共用本组件，避免两处各写一份 SVG 导致改色不同步。
  -->
  <span class="app-logo" :style="logoStyle" aria-hidden="true">
    <svg viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <!-- 渐变 id 随组件实例生成，防止同页多实例互相覆盖同一 id -->
        <linearGradient :id="gradId" x1="0%" y1="0%" x2="100%" y2="100%">
          <stop offset="0%" style="stop-color: var(--brand-from)" />
          <stop offset="100%" style="stop-color: var(--brand-to)" />
        </linearGradient>
      </defs>
      <circle cx="32" cy="32" r="24" :fill="`url(#${gradId})`" />
      <!-- 流动箭头：由 R 形弧线收笔，与"ReadFlow"的流字呼应 -->
      <path
        d="M32 16 C22 16 16 24 16 32 C16 40 22 48 32 48"
        stroke="white"
        stroke-width="3.5"
        stroke-linecap="round"
        fill="none"
      />
      <path
        d="M38 28 L32 34 L38 40"
        stroke="white"
        stroke-width="3"
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
 * 提供全应用唯一的品牌标志图形，供标题栏与正文栏空状态复用。
 *
 * # 设计意图
 * - **单一来源**：此前标题栏与空状态各内联一份完整 SVG，渐变色值也各写一遍；
 *   改一次品牌色要动两个文件，极易不同步。本组件把这些收敛到一处。
 * - **与主题色解耦**：品牌色走独立的 `--brand-*` 变量，不挂在 `--primary` 上。
 *   用户在设置里切换 9 套强调色时标志保持不变——应用图标是品牌锚点，
 *   而强调色只是界面装饰色，两者语义不同。
 * - **id 唯一**：SVG `<linearGradient>` 的 id 是文档级的，同页多实例若共用一个
 *   固定 id，后挂载的实例会覆盖前者的定义。这里按实例生成唯一 id。
 */

import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    /** 标志边长（px），正方形；默认与标题栏原有尺寸一致 */
    size?: number
  }>(),
  { size: 24 },
)

/**
 * 渐变 id：组件实例内唯一，避免同页多实例互相覆盖 `<linearGradient>` 定义
 */
const gradId = `app-logo-grad-${Math.random().toString(36).slice(2, 9)}`

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
  /* 圆形裁切：徽章本就是正圆，外层再切一次可避免 SVG 边缘抗锯齿时
     在主题色背景上露出 1px 底色缝隙 */
  border-radius: 50%;
  overflow: hidden;
  /* 投影走全局 token：暗色主题下自动换成更重的浓度，不会显脏 */
  box-shadow: var(--shadow-sm);
  /* 标题栏与空状态都用它，缩小不产生布局抖动 */
  flex-shrink: 0;
  transition: transform var(--dur) var(--ease);
}

.app-logo:hover {
  transform: scale(1.08);
}

.app-logo svg {
  width: 100%;
  height: 100%;
  display: block;
}
</style>
