<template>
  <!-- AI 摘要卡：生成中即提前出现（卡片内自带转圈 + 骨架屏），生成完成后骨架替换为
       摘要正文，避免进度提示游离在卡片之外。卡片自身的显隐由父组件判断
       （有摘要 或 正在生成），本组件只负责"长什么样"。 -->
  <div class="summary-card">
    <div class="summary-head">
      <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path d="M12 2l2 6.1L20 10l-6 1.9L12 18l-2-6.1L4 10l6-1.9z"></path>
        <path d="M19 14l1 3 3 1-3 1-1 3-1-3-3-1 3-1z"></path>
      </svg>
      <span class="summary-label">AI 摘要</span>
      <!-- 生成中：标题行右侧给出转圈 + 状态文字，与骨架屏共同表达"正在写" -->
      <span class="summary-pending" v-if="generating">
        <span class="ai-spinner"></span>
        正在生成摘要…
      </span>
    </div>
    <div class="summary-text" v-if="summary">{{ summary }}</div>
    <!-- 流式生成中：首字到达前仍是骨架屏，之后换成正在累积的文本，
         让"模型正在写"这件事可见，而不是整段生成期间一动不动 -->
    <div class="summary-text summary-stream" v-else-if="streamingText">{{ streamingText }}</div>
    <div class="summary-skeleton" v-else aria-hidden="true">
      <span class="sk-line"></span>
      <span class="sk-line"></span>
      <span class="sk-line sk-line-short"></span>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * AI 摘要卡片（正文滚动区顶部那张卡）
 *
 * # 职责
 * 纯展示：按「已生成 / 流式生成中 / 尚未有内容」三种情况渲染摘要卡。
 * 三个 props 恰好对应这三种情况，组件自身零状态、零 store、零 DOM 引用——
 * 这层抽离的价值就在于把 98 行样式（含暗色主题覆盖）从 2000 行的宿主里摘出来。
 *
 * # 依赖的全局类
 * `.ai-spinner`（转圈）与 `.sk-line` / `.sk-line-short`（骨架条）定义在 `styles.css`：
 * 三者都被多处复用（转圈同时出现在标题状态行、摘要卡、研究事件 FAB），
 * 因此不能留在任何单个组件的 scoped 样式里——scoped 样式跨不过组件边界。
 */

defineProps<{
  /** 已生成的摘要正文（null 表示尚未生成） */
  summary: string | null
  /** 流式累积中的摘要文本（仅属于当前文章时才非空） */
  streamingText: string
  /** 是否正在生成摘要（驱动标题行的转圈 + 状态文字） */
  generating: boolean
}>()
</script>

<style scoped>
.summary-card {
  margin: var(--sp-4) auto 0;
  max-width: var(--read-max);
  /* 内边距比正文区更松：摘要卡自带底色与描边，若留白与正文一致会显得局促、
     文字紧贴卡片边缘。22px 横向留白与 --read-max 内缩量搭配后观感更从容。 */
  padding: var(--sp-5) 22px;
  /* 主色浅底 + 主色半透明叠层：随"主题色"设置自动换色，取代原先硬编码的紫色方案。
     原先是"纯白 → 极浅主色"的渐变，平均值几乎等于白色、看不出底色，
     故改为以 --primary-soft 作实心底、再叠一层 --primary-fg（主色约 10% 不透明度），
     色感明确但不抢正文；135° 渐变只保留极轻微的深浅过渡。 */
  background:
    linear-gradient(135deg, rgba(255, 255, 255, 0.12) 0%, rgba(255, 255, 255, 0) 100%),
    linear-gradient(135deg, var(--primary-fg), var(--primary-fg)),
    var(--primary-soft);
  border: 1px solid var(--border);
  border-radius: var(--r-xl);
  flex-shrink: 0;
  box-shadow: var(--shadow-sm);
}
html[data-theme="dark"] .summary-card {
  /* 暗色下不再叠白色高光（会让卡片显脏），只保留"浅底 + 主色叠层"两层，
     与浅色主题保持同样的色感强度 */
  background:
    linear-gradient(135deg, var(--primary-fg), var(--primary-fg)),
    var(--primary-soft);
  border-color: var(--border);
  box-shadow: none;
}
html[data-theme="dark"] .summary-card .summary-label,
html[data-theme="dark"] .summary-card .summary-head svg { color: var(--primary); }
html[data-theme="dark"] .summary-card .summary-text { color: var(--text-secondary); }
/* 生成中状态：贴在"AI 摘要"标签右侧，复用工具栏同款转圈（尺寸略收小） */
.summary-pending {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: var(--sp-05);
  font-size: var(--fs-sm);
  color: var(--primary);
  font-weight: 500;
}
.summary-card .ai-spinner {
  width: 11px;
  height: 11px;
  border-width: 2px;
  border-top-color: var(--primary);
}
/* 骨架屏：三行浅色条 + 掠光动画，表达"正在写摘要"而不是"内容为空"。
   行高与行距同步放大，让骨架屏占位高度接近一行真实摘要，切换时不跳动。
   条与掠光动画本身（.sk-line / @keyframes sk-shimmer）已提升为全局工具类，
   订阅栏与文章列表的加载态共用同一套视觉语言。 */
.summary-skeleton {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.summary-skeleton .sk-line {
  height: 11px;
}
.summary-skeleton .sk-line-short { width: 62%; }

.summary-head { display: flex; align-items: center; gap: var(--sp-05); margin-bottom: var(--sp-15); }
.summary-head svg { width: 14px; height: 14px; color: var(--primary); flex-shrink: 0; }
.summary-label {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--primary);
  letter-spacing: 0.02em;
  background: var(--surface);
  border-radius: var(--r-sm);
  padding: 1px var(--sp-2);
}
.summary-text {
  font-size: calc(13px * var(--reading-scale, 1));
  line-height: 1.7;
  color: var(--text-secondary);
  word-break: break-word;
  font-family: var(--reading-font, inherit);
}
/* 流式生成中的摘要：行尾跟一个闪烁光标，明确表达"还在往下写" */
.summary-stream::after {
  content: '';
  display: inline-block;
  width: 2px;
  height: 1em;
  margin-left: 2px;
  vertical-align: text-bottom;
  background: currentColor;
  animation: ai-caret 1s step-end infinite;
}
@keyframes ai-caret {
  50% { opacity: 0; }
}
@media (prefers-reduced-motion: reduce) {
  .summary-stream::after { animation: none; }
}
</style>
