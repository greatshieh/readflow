<template>
  <!-- 首启引导：3 步向导，带步骤指示与「去设置」跳转按钮。
       每步的跳转按钮派发既有 window 事件（open-add-feed / open-research / open-settings），
       由 FeedPanel / App.vue 监听打开对应弹窗；本弹窗本身保持打开、位于其下层。
       关闭（Esc / 点遮罩 / 跳过 / 完成）统一经 BaseModal 的 close 向上抛出 finish。 -->
  <BaseModal :open="open" title="欢迎使用 ReadFlow" size="md" @close="emit('finish')">
    <div class="onb">
      <!-- 步骤指示：已完成 = done，当前 = active -->
      <div class="onb-steps" role="presentation">
        <span
          v-for="(s, i) in steps"
          :key="i"
          class="dot"
          :title="s.title"
          :class="{ active: i === step, done: i < step }"
        />
      </div>

      <h2 class="onb-title">{{ steps[step].title }}</h2>
      <p class="onb-desc">{{ steps[step].desc }}</p>

      <button class="btn btn-primary onb-jump" @click="jump(steps[step].event)">
        {{ steps[step].action }}
      </button>

      <div class="onb-nav">
        <button v-if="step > 0" class="btn" @click="prev">上一步</button>
        <span class="spacer" />
        <button class="btn" @click="emit('finish')">跳过</button>
        <button v-if="step < steps.length - 1" class="btn btn-primary" @click="next">
          下一步
        </button>
        <button v-else class="btn btn-primary" @click="emit('finish')">完成</button>
      </div>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 首启引导弹窗
 *
 * # 职责
 * 首次启动（settings 表无 `onboarded` 标记）时，用 3 步向导把用户领进主路径：
 * 加订阅源 → 加关注实体 → 开自动化。每步给一个「去设置」按钮，点下去会打开对应的
 * 真实弹窗（与命令面板/菜单里的入口完全一致），避免另造一套引导专用流程。
 *
 * # 与设计边界的关系
 * - 纯前端、无后端改动：`onboarded` 标记走既有 settings 表（key-value），由 App.vue
 *   在首启判断并写入，本组件只负责展示与派发跳转事件。
 * - 复用全局 `BaseModal` 外壳：遮罩 / Esc / 焦点陷阱 / 滚动锁统一处理；`@close` 既来自
 *   Esc、点遮罩，也来自「跳过 / 完成」按钮（本组件对二者语义一致——引导结束即标记完成）。
 *
 * # 参数
 * * `open` - 是否打开
 *
 * # 事件
 * * `finish` - 引导结束（完成、跳过或 Esc/遮罩关闭）；是否真的写入 `onboarded` 由 App.vue 决定
 */

import { ref } from 'vue'
import BaseModal from './BaseModal.vue'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ (e: 'finish'): void }>()

/** 单步定义：标题 + 说明 + 跳转按钮文案 + 派发的 window 事件名 */
interface OnbStep {
  title: string
  desc: string
  action: string
  event: string
}

/** 三步主路径；顺序即引导顺序 */
const steps: OnbStep[] = [
  {
    title: '① 添加订阅源',
    desc: '订阅你关心的 RSS 源，ReadFlow 会定时帮你抓取新文章——打开就有新内容。',
    action: '去添加订阅源',
    event: 'open-add-feed',
  },
  {
    title: '② 添加关注实体',
    desc: '告诉 ReadFlow 你关注的实体（公司 / 人物 / 产品），它会自动聚合相关的舆情、财务与事件。',
    action: '去添加关注实体',
    event: 'open-research',
  },
  {
    title: '③ 开启自动化',
    desc: '配置每日研究任务，让 AI 自动抽取事件、生成日报，重要动态主动通知你。',
    action: '去设置自动化',
    event: 'open-settings',
  },
]

/** 当前步骤下标（0 起） */
const step = ref(0)

/** 进入下一步（不到末步不越界） */
function next(): void {
  if (step.value < steps.length - 1) step.value++
}

/** 回到上一步（不到首步不越界） */
function prev(): void {
  if (step.value > 0) step.value--
}

/**
 * 派发某一步对应的 window 事件，打开真实弹窗
 *
 * 事件名与命令面板/菜单里的入口同构，保证引导跳过去的就是用户之后会自己用的那个界面。
 *
 * @param event - window 事件名（如 `open-add-feed`）
 * @returns 无返回值；副作用为 dispatch 一个 CustomEvent
 */
function jump(event: string): void {
  window.dispatchEvent(new CustomEvent(event))
}
</script>

<style scoped>
/* 向导内容区：固定内边距，标题/说明/按钮纵向排布 */
.onb {
  padding: var(--sp-6);
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}

/* 步骤指示：三个圆点，当前高亮、已完成填充 */
.onb-steps {
  display: flex;
  gap: var(--sp-2);
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--glass-hair);
  transition: background var(--dur) var(--ease);
}
.dot.active {
  background: var(--accent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
}
.dot.done {
  background: color-mix(in srgb, var(--accent) 55%, transparent);
}

.onb-title {
  margin: 0;
  font-size: var(--fs-xl);
  font-weight: 650;
  color: var(--text-primary);
}
.onb-desc {
  margin: 0;
  font-size: var(--fs-base);
  line-height: 1.6;
  color: var(--text-secondary);
  min-height: 3.2em;
}

/* 跳转按钮占满宽度，作为每步的主行动点 */
.onb-jump {
  width: 100%;
  justify-content: center;
}

/* 底部导航：上一步 / 跳过 / 下一步（或完成） */
.onb-nav {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-top: var(--sp-2);
}
.spacer {
  flex: 1;
}
</style>
