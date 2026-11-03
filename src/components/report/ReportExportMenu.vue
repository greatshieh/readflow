<template>
  <!-- 导出格式菜单：两类报告、两个触发位（列表卡片 / 阅读页）共用同一个组件。
       首行写明"这份是哪一份"，避免在列表里点导出时导错对象。 -->
  <Teleport to="body">
    <div
      ref="menuRef"
      class="export-menu"
      role="menu"
      :style="{ left: `${pos.x}px`, top: `${pos.y}px` }"
      @mousedown.stop
    >
      <div class="em-head">导出「{{ target.label }}」</div>
      <button
        v-for="opt in FORMATS"
        :key="opt.format"
        type="button"
        class="em-item"
        role="menuitem"
        :disabled="busy"
        @click="doExport(opt.format)"
      >
        <span class="em-name">{{ busy ? '正在准备…' : opt.label }}</span>
        <span class="em-ext">{{ opt.ext }}</span>
      </button>
      <div class="em-note">Markdown 保留原文；HTML 与应用内阅读同色同版式</div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
/**
 * 报告导出菜单
 *
 * # 职责
 * 两个触发位（报告卡片、阅读页工具条）共用同一个菜单：先问后端要「源正文 + 默认名」，
 * 再按用户选的格式决定"是否多走一步渲染"，最后交给后端弹系统对话框落盘。
 *
 * # 为什么 md 与 html 只差一步
 * 两类报告都只有一份 Markdown 真相：选 md 直接写原文；选 html 先用 `renderMarkdown`
 * 渲染、再补上主题变量拼成独立文件。**没有第二条渲染链路**，所以
 * "研究报告导出"与"周期报告导出"不可能出现版式分叉。
 *
 * # 定位
 * 由调用方给触发按钮的视口坐标，这里 clamp 到窗口内（菜单贴底时向上翻）。
 * 关闭靠外层点击与 Esc（Esc 在 window 捕获阶段拦下，弹窗不会跟着关）。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { renderMarkdown } from '@/utils/markdown'
import { buildExportHtml } from '@/utils/exportHtml'
import { useToastStore } from '@/stores/toast'
import type { ExportTarget } from '@/utils/reportExport'
import reportCss from '@/styles/report-content.css?raw'

const props = defineProps<{
  /** 触发按钮的视口坐标（菜单贴它下方） */
  anchor: { x: number; y: number }
  /** 导出对象 */
  target: ExportTarget
}>()

const emit = defineEmits<{ (e: 'close'): void }>()

const toast = useToastStore()

/** 菜单根节点（点外部判定的边界） */
const menuRef = ref<HTMLElement | null>(null)

/** 正在导出（按钮禁用，防连点导出两份） */
const busy = ref(false)

/** 两种格式 */
const FORMATS = [
  { format: 'md' as const, label: 'Markdown', ext: '.md' },
  { format: 'html' as const, label: 'HTML', ext: '.html' },
]

/** 菜单宽高（与 CSS 一致；用于 clamp） */
const MENU_W = 224
const MENU_H = 150

/** 实际渲染位置：贴按钮下方，越界时上翻 / 左移 */
const pos = computed(() => {
  const x = Math.min(props.anchor.x, window.innerWidth - MENU_W - 8)
  const below = props.anchor.y + 6
  const y = below + MENU_H > window.innerHeight - 8 ? Math.max(8, props.anchor.y - MENU_H - 6) : below
  return { x: Math.max(8, x), y }
})

/**
 * 点击菜单之外任意位置关闭
 *
 * 用 `mousedown` 而不是 `click`：与 `BaseModal` 的遮罩判定同源，
 * 避免"在菜单内按下、拖到外面松开"被误判成点了外部。
 *
 * @param e - 鼠标事件
 */
function onDocMouseDown(e: MouseEvent) {
  const el = menuRef.value
  if (!el) return
  if (!el.contains(e.target as Node)) emit('close')
}

/**
 * Esc 关闭菜单（**只关菜单，不关弹窗**）
 *
 * 监听挂在 `window` 的捕获阶段：`BaseModal` 的 Esc 监听挂在 `document` 捕获阶段，
 * 而捕获顺序是 window → document，所以在 window 上拦下并 `stopPropagation`
 * 才能抢在弹窗之前。挂在 document 会与弹窗按注册顺序抢，结果不可预测。
 *
 * @param e - 键盘事件
 */
function onWindowKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  e.preventDefault()
  e.stopPropagation()
  emit('close')
}

onMounted(() => {
  document.addEventListener('mousedown', onDocMouseDown)
  window.addEventListener('keydown', onWindowKeydown, true)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onDocMouseDown)
  window.removeEventListener('keydown', onWindowKeydown, true)
})

/**
 * 执行导出
 *
 * 取消（用户关掉系统对话框）不提示、不报错——"什么都没发生"就是正确结果。
 *
 * @param format - `md` 或 `html`
 */
async function doExport(format: 'md' | 'html') {
  busy.value = true
  try {
    const payload = await invoke<{ markdown: string; document?: string; defaultName: string }>(
      'report_export_prepare',
      { kind: props.target.kind, id: props.target.id ?? null, name: props.target.name ?? null },
    )
    // md 走 document：它带投影标记，落盘时按块合并，用户上次写的批注不会被覆盖。
    // html 走 markdown：渲染器会转义所有 `<`，标记混进去会变成一串可见的乱码文本。
    const content =
      format === 'md'
        ? payload.document ?? payload.markdown
        : buildExportHtml(props.target.label, renderMarkdown(payload.markdown), reportCss)
    const path = await invoke<string | null>('report_export_write', {
      defaultName: payload.defaultName,
      content,
      format,
    })
    if (path) toast.show(`已导出到 ${path}`, { duration: 5000 })
    emit('close')
  } catch (e) {
    toast.show(`导出失败：${e instanceof Error ? e.message : String(e)}`, { duration: 6000 })
  } finally {
    busy.value = false
  }
}
</script>

<style scoped>
/* 浮层视觉与 ContextMenu / AppTooltip 同款（--glass-strong + --glass-hair + --shadow-pop）。
   z-index 1060：必须高于弹窗遮罩（1000），低于 Toast（1100）。
   字号别名自成一份：本组件 Teleport 到 body，继承不到 `.base-modal` 上的局部令牌。 */
.export-menu {
  --fs-modal-label: var(--fs-sm);
  --fs-modal-meta: var(--fs-xs);
  position: fixed;
  width: 224px;
  z-index: 1060;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 5px;
  background: var(--glass-strong);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid var(--glass-hair);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-pop);
  animation: em-in 120ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
@keyframes em-in {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
}
.em-head {
  padding: 5px 8px 4px;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.em-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  border: none;
  background: none;
  border-radius: var(--r-sm);
  padding: 7px 8px;
  cursor: pointer;
  font-family: inherit;
  font-size: var(--fs-modal-label);
  color: var(--text-primary);
  text-align: left;
  transition: background 90ms ease-out;
}
.em-item:hover:not(:disabled) {
  background: var(--fill);
}
.em-item:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}
.em-item:disabled {
  opacity: 0.55;
  cursor: default;
}
.em-ext {
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
  flex-shrink: 0;
}
.em-note {
  padding: 4px 8px 3px;
  border-top: 1px solid var(--border);
  margin-top: 3px;
  font-size: var(--fs-modal-meta);
  line-height: 1.5;
  color: var(--text-tertiary);
}
</style>
