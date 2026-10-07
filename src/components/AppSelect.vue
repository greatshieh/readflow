<template>
  <!--
    自绘下拉选择器（AppSelect）

    为什么不用原生 select：原生 <option> 弹出层由操作系统绘制，CSS 完全无法干预，
    在深色面板 / 圆角浮岛风格的应用里会出现"系统控件混进来"的割裂感。
    本组件用「触发按钮 + 自绘浮层列表」复刻原生行为，外观完全跟随设计令牌。

    结构说明：
    - 触发器是 <button>，宽度默认撑满父容器（表单场景），可用 width prop 收敛；
    - 浮层用 <Teleport to="body"> + position: fixed，规避弹窗内 overflow:auto
      容器对 absolute 浮层的裁切（设置页 / 研究工作台的滚动容器都会裁掉它）；
      坐标由触发器的 getBoundingClientRect() 计算。
  -->
  <div ref="rootEl" class="app-select" :style="{ width: width }">
    <button
      type="button"
      class="as-trigger"
      :class="{ open: open }"
      :disabled="disabled"
      :aria-expanded="open"
      aria-haspopup="listbox"
      @click="toggle"
      @keydown.down.prevent="onTriggerArrowDown"
      @keydown.up.prevent="onTriggerArrowUp"
      @keydown.enter.prevent="toggle"
      @keydown.space.prevent="toggle"
    >
      <span class="as-value" :class="{ 'is-placeholder': !currentLabel }">
        {{ currentLabel || placeholder }}
      </span>
      <svg
        class="as-chevron"
        viewBox="0 0 12 12"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M3 4.5 6 7.5 9 4.5"></path>
      </svg>
    </button>

    <Teleport to="body">
      <div v-if="open" ref="panelEl" class="as-panel" :style="panelStyle" role="listbox">
        <div
          v-for="(opt, i) in options"
          :key="String(opt.value)"
          class="as-option"
          :class="{ active: opt.value === modelValue, highlighted: i === highlightIndex }"
          role="option"
          :aria-selected="opt.value === modelValue"
          @click="select(opt)"
          @mouseenter="highlightIndex = i"
        >
          <span class="as-option-label">{{ opt.label }}</span>
          <svg
            v-if="opt.value === modelValue"
            class="as-check"
            viewBox="0 0 12 12"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d="M2.5 6.2 4.8 8.5 9.5 3.6"></path>
          </svg>
        </div>
        <div v-if="options.length === 0" class="as-empty">暂无选项</div>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
/**
 * 自绘下拉选择器组件
 *
 * # 职责
 * 替代原生 `<select>`，提供与应用设计令牌一致的触发器与浮层列表，
 * 并保留原生下拉的核心交互（点击展开 / 点选 / 键盘导航 / Esc 关闭 / 点外部关闭）。
 *
 * # 设计意图
 * - **浮层挂到 body**：弹窗主体普遍带 `overflow-y: auto`，若浮层留在原地用
 *   `absolute` 定位，会被容器裁切或撑出滚动条；`Teleport + fixed` 彻底规避。
 * - **不设全屏遮罩**：遮罩会吞掉滚轮事件，导致浮层打开时背后的弹窗无法滚动，
 *   因此用 document 级 mousedown 判断"点击了浮层外"来关闭。
 * - **滚动 / 缩放即关闭**：浮层是 fixed 定位，跟随触发器重定位的收益低于复杂度，
 *   直接关闭更符合"轻量选择"的语义（在浮层内部滚动不会触发关闭）。
 *
 * # 用法
 * ```vue
 * <AppSelect v-model="theme" :options="[{ value: 'light', label: '浅色' }]" />
 * ```
 */

import { ref, computed, onUnmounted, nextTick } from 'vue'

/** 下拉选项的数据结构 */
export interface SelectOption {
  /** 选项值（提交给 v-model 的值） */
  value: string | number
  /** 选项展示文案 */
  label: string
}

const props = withDefaults(
  defineProps<{
    /** 当前选中值（配合 v-model 使用） */
    modelValue: string | number
    /** 可选项列表；为空时浮层显示"暂无选项" */
    options: SelectOption[]
    /** 未选中任何值时的占位文案 */
    placeholder?: string
    /** 禁用状态（触发器不可点击、不展开） */
    disabled?: boolean
    /** 组件整体宽度的 CSS 值；默认 100%（表单场景撑满），行内场景可传 'auto' 或固定值 */
    width?: string
  }>(),
  {
    placeholder: '请选择',
    disabled: false,
    width: '100%',
  },
)

const emit = defineEmits<{
  /** 选中值变化（v-model 通道） */
  'update:modelValue': [value: string | number]
  /** 用户主动选中（与 update:modelValue 同时触发，便于父组件做副作用） */
  change: [value: string | number]
}>()

/** 浮层最大高度（px）：超出后列表内部滚动 */
const PANEL_MAX_HEIGHT = 260
/** 浮层与窗口边缘的最小安全间距（px） */
const EDGE_GAP = 8
/** 触发器与浮层之间的间距（px） */
const TRIGGER_GAP = 6

/** 组件根元素（用于测量触发器位置与判断"点击是否在组件内"） */
const rootEl = ref<HTMLElement | null>(null)
/** 浮层元素（teleport 到 body 后仍带本组件的 scope id，scoped 样式生效） */
const panelEl = ref<HTMLElement | null>(null)
/** 浮层是否展开 */
const open = ref(false)
/** 键盘高亮的选项下标（-1 = 无高亮） */
const highlightIndex = ref(-1)
/** 浮层定位（fixed 坐标与宽度） */
const panelPos = ref({ top: 0, left: 0, width: 0 })

/** 当前选中项对应的展示文案 */
const currentLabel = computed(
  () => props.options.find((o) => o.value === props.modelValue)?.label ?? '',
)

/** 浮层的内联定位样式 */
const panelStyle = computed(() => ({
  top: `${panelPos.value.top}px`,
  left: `${panelPos.value.left}px`,
  width: `${panelPos.value.width}px`,
}))

/**
 * 展开或收起浮层
 */
function toggle() {
  if (open.value) {
    closePanel()
  } else {
    openPanel()
  }
}

/**
 * 展开浮层
 *
 * 顺序很关键：必须先渲染浮层（nextTick）才能量到真实高度，
 * 否则翻转判断只能靠估算行高，长列表时会算错。
 */
async function openPanel() {
  if (props.disabled || open.value) return
  open.value = true
  await nextTick()
  positionPanel()
  // 初始高亮落在当前选中项上，键盘操作的起点才符合直觉
  const idx = props.options.findIndex((o) => o.value === props.modelValue)
  highlightIndex.value = idx >= 0 ? idx : 0
  scrollHighlightIntoView()
  attachListeners()
}

/**
 * 计算并设置浮层坐标
 *
 * 优先向下展开；若下方空间不足则向上翻转（前提是上方空间更多），
 * 并把浮层夹在窗口可视范围内，避免贴边或被裁掉。
 */
function positionPanel() {
  const rect = rootEl.value?.getBoundingClientRect()
  if (!rect) return
  const height = panelEl.value?.offsetHeight ?? PANEL_MAX_HEIGHT
  const width = Math.max(rect.width, 120)

  let top = rect.bottom + TRIGGER_GAP
  if (top + height > window.innerHeight - EDGE_GAP) {
    const above = rect.top - TRIGGER_GAP - height
    if (above > EDGE_GAP) top = above
  }

  let left = rect.left
  if (left + width > window.innerWidth - EDGE_GAP) {
    left = Math.max(EDGE_GAP, window.innerWidth - EDGE_GAP - width)
  }

  panelPos.value = { top, left, width }
}

/**
 * 选中一个选项
 *
 * @param opt - 被选中的选项
 */
function select(opt: SelectOption) {
  emit('update:modelValue', opt.value)
  emit('change', opt.value)
  closePanel()
}

/**
 * 收起浮层并解绑全局监听
 */
function closePanel() {
  if (!open.value) return
  open.value = false
  highlightIndex.value = -1
  detachListeners()
}

/**
 * 把键盘高亮项滚进可视区（长列表时键盘导航才跟得上）
 */
function scrollHighlightIntoView() {
  const el = panelEl.value?.querySelector<HTMLElement>('.as-option.highlighted')
  el?.scrollIntoView({ block: 'nearest' })
}

/**
 * 键盘上下键移动高亮
 *
 * @param delta - 位移量（+1 向下 / -1 向上）
 */
function moveHighlight(delta: number) {
  if (props.options.length === 0) return
  const next = highlightIndex.value + delta
  highlightIndex.value = Math.max(0, Math.min(props.options.length - 1, next))
  scrollHighlightIntoView()
}

/**
 * 触发器上的 ↓ 键：收起状态下展开，展开状态下移动高亮
 */
function onTriggerArrowDown() {
  if (!open.value) {
    openPanel()
  } else {
    moveHighlight(1)
  }
}

/**
 * 触发器上的 ↑ 键：同 ↓，方向相反
 */
function onTriggerArrowUp() {
  if (!open.value) {
    openPanel()
  } else {
    moveHighlight(-1)
  }
}

/**
 * 文档级键盘处理
 *
 * 浮层内的选项本身不可聚焦，键盘事件统一在这里处理，
 * 避免"点击选项后焦点丢失导致按键失效"的问题。
 *
 * @param e - 键盘事件
 */
function onDocKeydown(e: KeyboardEvent) {
  if (!open.value) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    moveHighlight(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    moveHighlight(-1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    const opt = props.options[highlightIndex.value]
    if (opt) select(opt)
  } else if (e.key === 'Escape' || e.key === 'Tab') {
    closePanel()
  }
}

/**
 * 文档级点击处理：点击浮层与触发器之外的地方则收起
 *
 * @param e - 鼠标事件
 */
function onDocMouseDown(e: MouseEvent) {
  const target = e.target as Node | null
  if (!target) return
  if (rootEl.value?.contains(target) || panelEl.value?.contains(target)) return
  closePanel()
}

/**
 * 页面滚动或窗口尺寸变化时收起浮层
 *
 * 浮层自身内部的滚动要放行，否则长列表滚到底会把自己关掉。
 *
 * @param e - 事件对象
 */
function onViewportChange(e: Event) {
  const target = e.target as Node | null
  if (target && panelEl.value?.contains(target)) return
  closePanel()
}

/** 注册全局监听 */
function attachListeners() {
  document.addEventListener('mousedown', onDocMouseDown, true)
  document.addEventListener('keydown', onDocKeydown, true)
  // scroll 不冒泡，必须用捕获阶段才能收到内层容器的滚动
  window.addEventListener('scroll', onViewportChange, true)
  window.addEventListener('resize', onViewportChange)
}

/** 注销全局监听 */
function detachListeners() {
  document.removeEventListener('mousedown', onDocMouseDown, true)
  document.removeEventListener('keydown', onDocKeydown, true)
  window.removeEventListener('scroll', onViewportChange, true)
  window.removeEventListener('resize', onViewportChange)
}

/** 组件卸载时兜底解绑，避免监听泄漏 */
onUnmounted(() => {
  detachListeners()
})
</script>

<style scoped>
.app-select {
  position: relative;
  /* 行内场景（如筛选条）传 width="auto" 时给一个下限，
     避免选项文案很短时触发器缩成一个小方块 */
  min-width: 96px;
}

/* 触发器：尺寸 / 圆角 / 底色 / focus 光环与输入框保持同一套令牌 */
.as-trigger {
  position: relative;
  width: 100%;
  height: 34px;
  display: flex;
  align-items: center;
  padding: 0 28px 0 var(--sp-15);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--fill);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--fs-md);
  text-align: left;
  cursor: pointer;
  transition: border-color var(--dur) var(--ease), background-color var(--dur) var(--ease),
    box-shadow var(--dur) var(--ease);
}
.as-trigger:hover:not(:disabled) {
  background-color: var(--fill-secondary);
  border-color: var(--text-tertiary);
}
.as-trigger.open {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px var(--primary-soft);
}
.as-trigger:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
/* 全局 button:active 有 0.96 缩放，下拉触发器按下时抖动会显得廉价，这里抑制掉 */
.as-trigger:not(:disabled):active {
  transform: none;
}

.as-value {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.as-value.is-placeholder {
  color: var(--text-tertiary);
}

.as-chevron {
  position: absolute;
  right: 9px;
  width: 12px;
  height: 12px;
  color: var(--text-secondary);
  transition: transform var(--dur) var(--ease), color var(--dur) var(--ease);
}
.as-trigger.open .as-chevron {
  transform: rotate(180deg);
  color: var(--primary);
}

/* 浮层：teleport 到 body，故必须显式抬高层级（弹窗遮罩是 200） */
.as-panel {
  position: fixed;
  z-index: 1000;
  max-height: 260px;
  overflow-y: auto;
  padding: var(--sp-1);
  background: var(--surface);
  border: 1px solid var(--panel-border);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-pop);
}

.as-option {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-15);
  border-radius: var(--r-md);
  font-size: var(--fs-md);
  color: var(--text-primary);
  cursor: pointer;
  white-space: nowrap;
}
.as-option.highlighted {
  background: var(--fill);
}
.as-option.active {
  color: var(--primary);
  font-weight: 500;
}
.as-option-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.as-check {
  flex-shrink: 0;
  width: 12px;
  height: 12px;
  color: var(--primary);
}

.as-empty {
  padding: var(--sp-2) var(--sp-15);
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}
</style>
