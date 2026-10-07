<template>
  <!-- 开关控件：受控组件，自身不持有状态，只在点击时把"取反后的值"抛给父组件。
       用 <button> 而不是 <div> 是为了天然获得键盘能力（Tab 可达、空格/回车触发），
       role="switch" + aria-checked 则让读屏播报"已开启 / 已关闭"。 -->
  <button type="button" class="switch" :class="{ on: modelValue }" role="switch" :aria-checked="modelValue"
    :aria-label="label" :title="title || label" :disabled="disabled" @click="toggle"></button>
</template>

<script setup lang="ts">
/**
 * 开关（Switch）组件
 *
 * # 职责
 * 提供全应用唯一的开关外观与交互，替代此前三种各自为政的实现：
 * 设置页的 `<div class="switch" @click>`（不可 Tab、不可键盘触发、读屏不可见）、
 * 过滤规则页的原生 `<input type="checkbox">`（外观与全应用不一致）。
 *
 * # 为什么是受控组件
 * 开关的"真值"通常在后端（任务是否启用、规则是否生效）。组件只在点击时抛出
 * 取反后的值，是否立即变更是父组件的决定——这样父组件可以做乐观更新与失败回滚
 * （见 `RulesModal::toggleEnabled`），而不必和组件内部状态对齐。
 *
 * # 参数
 * * `modelValue` - 当前状态
 * * `label` - 无障碍名称（读屏播报用）；视觉标签由父组件在左侧渲染
 * * `title` - 悬浮提示，缺省时复用 `label`
 * * `disabled` - 禁用态：不响应点击，并给出原生禁用语义
 *
 * # 事件
 * * `update:modelValue` - 支持 `v-model`
 * * `change` - 值取反后触发（带新值），供"切换后还要请求后端"的场景使用
 */

const props = withDefaults(
  defineProps<{
    /** 当前状态 */
    modelValue: boolean
    /** 无障碍名称 */
    label: string
    /** 悬浮提示（缺省时用 label） */
    title?: string
    /** 是否禁用 */
    disabled?: boolean
  }>(),
  { title: '', disabled: false },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: boolean): void
  (e: 'change', value: boolean): void
}>()

/**
 * 切换状态
 *
 * 由 `<button>` 的 click 触发（键盘的空格 / 回车会转成 click，无需额外处理键盘事件）。
 *
 * @returns 无返回值；副作用为抛出取反后的值
 */
function toggle(): void {
  if (props.disabled) return
  const next = !props.modelValue
  emit('update:modelValue', next)
  emit('change', next)
}
</script>

<style scoped>
.switch {
  /* 抹掉 <button> 的默认外观，让它和此前的 <div> 版本像素级一致 */
  appearance: none;
  -webkit-appearance: none;
  border: none;
  padding: 0;
  flex-shrink: 0;

  width: 32px;
  height: 18px;
  background: var(--border);
  border-radius: 9px;
  position: relative;
  cursor: pointer;
  transition: background var(--dur) var(--ease);
}

.switch.on {
  background: var(--primary);
}

.switch::after {
  content: '';
  position: absolute;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--tone-fg);
  top: 2px;
  left: 2px;
  transition: transform var(--dur) var(--ease);
}

.switch.on::after {
  transform: translateX(14px);
}

.switch:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 键盘聚焦时给出可见焦点环：键盘用户必须能看出焦点在哪 */
.switch:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
</style>
