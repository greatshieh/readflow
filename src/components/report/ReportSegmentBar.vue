<template>
  <!-- 报告页签内的两段式分段控件：周期报告 / 研究报告。
       位置在 `.m-body` **之外**（`.side-main` 层、标题栏之下）：`.m-body` 是带
       padding 与 `overflow-y:auto` 的滚动容器，分段塞进去会变成"内层再来一份
       padding + 跟着内容滚走"，而它应该是常驻的容器级导航。

       语义用「切换按钮组」而不是 `role="tablist"`：真正的 tab 语义要求
       `aria-controls` 指向 panel、且用 roving tabindex + 左右方向键切换；
       这两段的 panel 在另一个组件里（`ResearchReportsTab`），只加 `role="tab"`
       会让读屏播报"第 1 个，共 2 个页签"却按方向键毫无反应 —— 半套 tab 语义
       比不用 tab 语义更糟。`aria-pressed` 与周期报告里的粒度选择器一致。 -->
  <div class="seg-bar" role="group" aria-label="报告类型">
    <button
      v-for="opt in OPTIONS"
      :key="opt.value"
      type="button"
      class="seg-btn"
      :class="{ active: store.segment === opt.value }"
      :aria-pressed="store.segment === opt.value"
      @click="store.setSegment(opt.value)"
    >
      {{ opt.label }}
    </button>
  </div>
</template>

<script setup lang="ts">
/**
 * 报告页签内的分段控件
 *
 * # 职责
 * 在「周期报告」与「研究报告」两段之间切换。它只改 `reportPeriod` store 里的
 * `segment`，两段的内容由 `ResearchReportsTab` 各自 `v-show` 常驻渲染
 * ——不用 `v-if` 是为了保住旧段的生成参数与阅读位置（切回来还在原处）。
 *
 * # 为什么两段而不是两个页签
 * 两者是并列关系（同一件事的两种口径），用分段控件比下拉好：下拉会把另一半藏起来，
 * 用户不知道"还有研究报告"。页签数量也保持 3 个不变（决策①）。
 */
import { useReportPeriodStore } from '@/stores/reportPeriod'

const store = useReportPeriodStore()

/** 两段定义（顺序即展示顺序：周期报告在前，因为它是默认落点） */
const OPTIONS: { value: 'period' | 'research'; label: string }[] = [
  { value: 'period', label: '周期报告' },
  { value: 'research', label: '研究报告' },
]
</script>

<style scoped>
.seg-bar {
  flex-shrink: 0;
  display: flex;
  gap: 2px;
  padding: 3px;
  margin: var(--sp-3) var(--sp-5) 0;
  background: var(--fill);
  border-radius: var(--r-lg);
}
.seg-btn {
  flex: 1;
  border: none;
  background: none;
  padding: 7px 10px;
  border-radius: 7px;
  cursor: pointer;
  font-family: inherit;
  font-size: var(--fs-modal-label);
  font-weight: 500;
  color: var(--text-secondary);
  transition:
    background var(--m-state, 160ms) var(--m-ease-out, ease-out),
    color var(--m-state, 160ms) var(--m-ease-out, ease-out),
    box-shadow var(--m-state, 160ms) var(--m-ease-out, ease-out);
}
.seg-btn:hover {
  color: var(--text-primary);
}
.seg-btn.active {
  background: var(--surface);
  color: var(--text-primary);
  box-shadow: 0 1px 3px rgba(16, 24, 40, 0.1);
}
.seg-btn:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
</style>
