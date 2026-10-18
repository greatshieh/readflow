<template>
  <!-- 通用轻量浮层：包裹触发元素（默认插槽），hover / focus 时在下方弹出提示文本。
       定位用 fixed + 触发元素的视口坐标（getBoundingClientRect），空间不足时翻到上方。
       Teleport 到 body：宿主面板（如 .feed-panel）有 overflow:hidden，浮层留在栏内会被裁；
       且面板的 backdrop-filter 会让内部 position:fixed 退化为 absolute——两者叠加会把浮层
       关进栏内，故必须移出该坐标系（项目已有此教训，见 FeedPanel 的「更多」菜单）。 -->
  <span
    ref="triggerRef"
    v-bind="$attrs"
    class="app-tooltip-trigger"
    :tabindex="focusable ? 0 : undefined"
    :aria-describedby="tipId"
    @mouseenter="show"
    @mouseleave="hide"
    @focusin="show"
    @focusout="hide"
  >
    <slot />
  </span>
  <Teleport to="body">
    <div v-if="visible" :id="tipId" ref="tipRef" class="app-tooltip" role="tooltip" :style="tipStyle">
      <!-- 内容优先取 content prop，需要富内容时可用具名插槽覆盖 -->
      <slot name="content">{{ content }}</slot>
    </div>
  </Teleport>
</template>

<!-- 普通 <script> 块（非 setup）：Vue 会把它原样保留在**模块作用域**。
     浮层 id 的自增计数器必须放在这里。<script setup> 的顶层代码会被 SFC 编译器整体
     放进 setup()，即每个组件实例都会重新执行 → 计数器被重置为 0 → 所有实例的 tipId
     恒为 app-tooltip-1（重复 DOM id，aria-describedby 指向歧义）。放在本块中才是真正的
     模块级单例，跨实例、跨挂载持续自增。
     不用 Vue 3.5 的 useId()：package.json 声明 "vue": "^3.4.0"，useId 是 3.5 才引入的
     API，在 3.4 运行时上会直接抛错；模块级计数器跨版本安全（本项目纯客户端，无 SSR）。 -->
<script lang="ts">
let tipIdSeed = 0
</script>

<script setup lang="ts">
/**
 * AppTooltip —— 面向图标 / 短标识的通用悬浮提示
 *
 * # 职责
 * 包裹任意触发元素（默认插槽），在 hover / focus 时于触发元素下方弹出简短文本提示，
 * 空间不足时自动翻到上方。仅承载"补充说明"，不承担交互——浮层本身 pointer-events:none，
 * 鼠标移出触发元素即隐藏。
 *
 * # 为什么 Teleport 到 body
 * 订阅栏等宿主容器设有 overflow:hidden，且带 backdrop-filter（会让内部 position:fixed
 * 退化为 absolute）。浮层若留在容器内会被裁切或错位，故必须 Teleport 到 body 脱离该坐标系。
 *
 * # 定位与 zoom 的前提（改动前必读）
 * 应用根 `.app` 设有 `zoom: var(--ui-zoom)`。本浮层 Teleport 到 body，位于该 zoom 容器
 * **之外**、不受其缩放；而 getBoundingClientRect() 返回的正是缩放后的视口坐标，
 * 二者参照系一致，故直接用 fixed + 视口坐标定位即可对齐。
 * 若日后取消 Teleport（浮层重回 zoom 容器内），这些坐标需先除以 zoom 换算回布局像素，
 * 否则在非 100% 界面缩放下会整体偏移。
 */

import { nextTick, onUnmounted, ref, withDefaults } from 'vue'

const props = withDefaults(
  defineProps<{
    /** 提示文本（也可改用 #content 具名插槽提供富内容） */
    content?: string
    /**
     * 是否让触发元素进入键盘 Tab 顺序
     *
     * 默认 false：本组件多为纯装饰/鼠标态的补充说明（如文本省略号的全称提示），
     * 这类元素本就不该抢走 Tab 焦点——默认保持现状，不改变组件对其它潜在使用方的语义。
     * 仅当提示承载"必须让键盘用户也读到的信息"（如错误原因）时，才由使用方显式传 true。
     */
    focusable?: boolean
  }>(),
  { content: '', focusable: false },
)

/** 本实例浮层的唯一 id（供 trigger 的 aria-describedby 引用浮层） */
const tipId = `app-tooltip-${++tipIdSeed}`

/** 触发元素根节点 */
const triggerRef = ref<HTMLElement | null>(null)
/** 浮层根节点（Teleport 到 body，故需单独 ref） */
const tipRef = ref<HTMLElement | null>(null)
/** 浮层可见性 */
const visible = ref(false)
/** 浮层定位样式（fixed 的 top / left，由 place() 计算） */
const tipStyle = ref<Record<string, string>>({})

/** 浮层与触发元素的间距（px） */
const GAP = 8
/** 浮层距视口边缘的最小安全边距（px） */
const EDGE = 8

/**
 * 量出浮层尺寸后计算落点：优先放在下方，下方空间不足且上方够放时翻到上方
 *
 * @returns 无返回值；结果写入 tipStyle
 */
function place(): void {
  const trigger = triggerRef.value
  if (!trigger) return
  const r = trigger.getBoundingClientRect()
  const tipH = tipRef.value?.offsetHeight ?? 0
  const tipW = tipRef.value?.offsetWidth ?? 0
  const vh = window.innerHeight
  const vw = window.innerWidth
  const belowTop = r.bottom + GAP
  const aboveTop = r.top - GAP - tipH
  // 下方放不下、且上方放得下时才翻转，避免两侧都挤时来回跳
  const flipAbove = belowTop + tipH > vh && aboveTop >= EDGE
  const top = flipAbove ? aboveTop : belowTop
  // 左对齐触发元素；右侧越界时向左收，始终保留 EDGE 边距
  let left = r.left
  if (left + tipW > vw - EDGE) left = Math.max(EDGE, vw - EDGE - tipW)
  tipStyle.value = { top: `${top}px`, left: `${left}px` }
}

/** 显示浮层：等渲染出元素后再量尺寸定位 */
async function show(): Promise<void> {
  visible.value = true
  await nextTick()
  place()
}

/** 隐藏浮层 */
function hide(): void {
  visible.value = false
}

/** 视口尺寸变化时重新定位（浮层需跟随触发元素，但无需重新挂载） */
function onResize(): void {
  if (visible.value) place()
}

/** 滚动时直接隐藏：fixed 浮层不会跟随内容滚动，留着会与触发元素错位 */
function onScroll(): void {
  if (visible.value) hide()
}

window.addEventListener('resize', onResize)
// 捕获阶段监听：宿主容器的内部滚动也要能触发（scroll 事件默认不冒泡到 window）
window.addEventListener('scroll', onScroll, true)
onUnmounted(() => {
  window.removeEventListener('resize', onResize)
  window.removeEventListener('scroll', onScroll, true)
})
</script>

<style scoped>
/* 触发元素容器：inline-flex 保证包裹图标时不改变行内布局，且不参与父级挤压 */
.app-tooltip-trigger {
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
}
/* 仅当使用方传入 focusable、trigger 进入 Tab 顺序后，键盘聚焦时才显示焦点环。
   用 :focus-visible 而非 :focus：鼠标点击同样会触发 :focus，屏幕上会多出一个只在
   键盘操作时才有意义的焦点框，视觉噪音大；:focus-visible 由浏览器判定"这次聚焦是否
   来自键盘"，只在键盘导航时出现。样式对齐 AppSwitch 的焦点环写法（outline + offset），
   颜色走 --primary token，避免硬编码。 */
.app-tooltip-trigger:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  border-radius: var(--r-sm);
}
/* Teleport 到 body 后改用 fixed，由 JS 按触发元素的视口坐标定位（见 place()）。
   视觉语言与 FeedPanel 的「更多」下拉菜单一致：玻璃底 + 描边 + 弹起阴影。 */
.app-tooltip {
  position: fixed;
  /* 高于 BaseModal(1000)：提示层需压过任何弹窗 */
  z-index: 1100;
  max-width: 260px;
  padding: var(--sp-2) var(--sp-3);
  background: var(--glass-strong);
  backdrop-filter: blur(24px) saturate(1.7);
  -webkit-backdrop-filter: blur(24px) saturate(1.7);
  border: 1px solid var(--glass-hair);
  border-radius: var(--r-md);
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    var(--shadow-pop);
  color: var(--text-primary);
  font-size: var(--fs-xs);
  line-height: 1.5;
  /* 多行文本：允许换行并在长串（如 URL）处强制断行，避免撑破 max-width */
  white-space: normal;
  overflow-wrap: anywhere;
  word-break: break-word;
  /* 纯提示层：不拦截任何指针事件，鼠标移出触发元素即隐藏 */
  pointer-events: none;
}
</style>
