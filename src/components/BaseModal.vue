<template>
  <!-- 模态弹窗外壳：遮罩 + 居中面板 + 无障碍语义。
       业务弹窗只负责往默认插槽里放内容（一般是「头部 + 主体」两段），
       遮罩点击、Esc 关闭、焦点陷阱与滚动锁都由本组件统一处理。

       Teleport 到 body 是必需的，不能省：订阅栏等面板带 `backdrop-filter`，
       而 backdrop-filter 会**创建包含块**，使内部的 position:fixed 退化为
       absolute —— 遮罩于是被局限在面板盒子里、并被面板的 overflow:hidden 裁掉
       （表现为弹窗被"关"在栏内）。移到 body 下即彻底脱离该坐标系。 -->
  <Teleport to="body">
  <div class="overlay" :class="{ show: open }" @mousedown.self="emit('close')">
    <div
      ref="panelRef"
      class="base-modal"
      :class="[`size-${size}`, layout === 'side' ? 'layout-side' : '', { global }]"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
      tabindex="-1"
    >
      <slot />
    </div>
  </div>
  </Teleport>
</template>

<script setup lang="ts">
/**
 * 模态弹窗外壳组件
 *
 * # 职责
 * 提供所有模态弹窗**唯一**的遮罩与面板实现，并补齐此前全应用缺失的无障碍能力：
 * - 语义：`role="dialog"` + `aria-modal="true"` + `aria-label`，读屏能正确播报"这是个对话框"；
 * - 键盘：Esc 关闭；Tab 在面板内循环（焦点陷阱），不会跑到被遮住的页面上；
 * - 焦点：打开时记住触发元素并把焦点移入面板，关闭时归还（键盘用户的操作起点不丢）；
 * - 滚动锁：打开时锁定背景滚动，关闭时恢复。
 *
 * # 为什么点击关闭监听 `mousedown` 而非 `click`
 * 用户从面板内部按下鼠标、拖到遮罩上松开时，`click` 的 target 是二者共同祖先
 * （即遮罩），会被误判成"点了遮罩"而关闭弹窗、丢掉正在填的内容。按下点才是真实意图。
 *
 * # 与全局确认框的优先级
 * 全局确认框（`GlobalModal`，z-index 9999）可能叠在弹窗之上（如弹窗内点删除）。
 * 此时 Esc 属于最上层，本组件检出 `modalStore.activeModal` 后主动让路，
 * 避免"按一次 Esc 关掉两层"。
 *
 * # 参数
 * * `open` - 是否打开；关闭态面板只是不可见，组件本身常驻
 * * `title` - 弹窗标题，写入 `aria-label` 供读屏播报
 * * `size` - 面板宽度档位：sm 460px / md 520px / lg 560px / xl 680px
 * * `layout` - 布局档位：`default`（默认）高度随内容自适应；
 *   `side` 固定高度（min(80vh, 720px)）+ 宽面板，供 tab 较多的弹窗
 *   （设置 / 研究工作台）把 tab 竖排在左侧——弹窗高度不再随切 tab 跳变。
 *   side 布局下宽度由本变体定（min(880px, 92vw)），size 档位不再参与；
 *   且自带面板右上角关闭钮（双列布局没有贯通头部，关闭钮由外壳统一提供）。
 *
 * # 事件
 * * `close` - 用户按下 Esc 或点击遮罩空白处；是否真的关闭由父组件决定
 */

import { nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useModalStore } from '@/stores/modal'
import { acquireDialog, releaseDialog } from '@/utils/dialogStack'

const props = withDefaults(
  defineProps<{
    /** 是否打开 */
    open: boolean
    /** 弹窗标题（仅用于无障碍播报，视觉标题由各弹窗自己渲染） */
    title: string
    /** 面板宽度档位 */
    size?: 'sm' | 'md' | 'lg' | 'xl'
    /** 布局：default 高度随内容自适应；side 固定高度 + 宽面板，宿主内部排「左 tab 列 + 右内容列」 */
    layout?: 'default' | 'side'
    /** 全局确认框变体（GlobalModal 专用）：z-index 压过普通弹窗，且键盘事件不向 activeModal 让路 */
    global?: boolean
  }>(),
  { size: 'lg', layout: 'default' },
)

const emit = defineEmits<{ (e: 'close'): void }>()

/** 全局弹窗 store：用于判断"最上层是不是全局确认框" */
const modalStore = useModalStore()

/** 面板元素引用：焦点移入 / 焦点陷阱的边界 */
const panelRef = ref<HTMLElement | null>(null)

/** 打开前的焦点所在元素，关闭时归还给它 */
let lastActive: HTMLElement | null = null

/** 本实例当前是否持有滚动锁（用于配对 acquire / release，避免多余调用错位计数） */
let locked = false

/**
 * 面板内可聚焦元素的选择器
 *
 * 只覆盖浏览器默认可 Tab 到达的元素；`tabindex="-1"` 被排除（那是程序化聚焦用的）。
 */
const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

/**
 * 取面板内当前可见的可聚焦元素
 *
 * @returns 按 DOM 顺序排列的元素数组；面板未渲染时为空数组
 */
function focusables(): HTMLElement[] {
  const root = panelRef.value
  if (!root) return []
  // 过滤掉 `display:none` 的分支（如未激活的 tab 内容），否则焦点会被送进看不见的元素
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => el.offsetParent !== null,
  )
}

/**
 * 把焦点移入面板
 *
 * 优先 `[data-autofocus]`（表单类弹窗希望落在输入框上），否则第一个可聚焦元素，
 * 都没有就聚焦面板本身——面板带 `tabindex="-1"`，可被程序化聚焦。
 *
 * @returns Promise：在 DOM 更新完成（nextTick）后完成聚焦
 */
async function moveFocusIn(): Promise<void> {
  await nextTick()
  const root = panelRef.value
  if (!root) return
  const preferred = root.querySelector<HTMLElement>('[data-autofocus]')
  const target = preferred ?? focusables()[0] ?? root
  target.focus()
}

/**
 * 键盘处理：Esc 关闭 + Tab 焦点陷阱
 *
 * 在**捕获阶段**监听 `document`：这样事件还没传到页面里的其他处理器
 * （正文栏的 Esc 关闭浮层、应用级快捷键等）就被拦下，符合"模态优先"的语义。
 *
 * @param e - 键盘事件
 * @returns 无返回值
 */
function onKeydown(e: KeyboardEvent): void {
  if (!props.open) return

  // 全局确认框叠在本弹窗之上时，键盘事件属于它；本弹窗保持打开。
  // Esc 与 Tab 都要让路：Tab 若不让，两个焦点陷阱会互相把焦点抢来抢去。
  // `global` 变体（GlobalModal 自己）不在此列——activeModal 指的就是它自己。
  if (modalStore.activeModal && !props.global) return

  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    emit('close')
    return
  }

  if (e.key !== 'Tab') return

  const root = panelRef.value
  if (!root) return

  const items = focusables()
  // 没有任何可聚焦元素时，把 Tab 扣在面板上，避免焦点跑到背景页面
  if (items.length === 0) {
    e.preventDefault()
    root.focus()
    return
  }

  const first = items[0]
  const last = items[items.length - 1]
  const active = document.activeElement as HTMLElement | null
  const inside = active !== null && root.contains(active)

  // 焦点已跑到面板之外（例如背景按钮被点过）：拉回边界
  if (!inside) {
    e.preventDefault()
    ;(e.shiftKey ? last : first).focus()
    return
  }
  // 走到两端时循环，形成闭环
  if (!e.shiftKey && active === last) {
    e.preventDefault()
    first.focus()
  } else if (e.shiftKey && active === first) {
    e.preventDefault()
    last.focus()
  }
}

/**
 * 进入打开态：记住触发元素、加锁、开始监听键盘、移入焦点
 *
 * @returns 无返回值
 */
function activate(): void {
  if (locked) return
  locked = true
  lastActive = document.activeElement as HTMLElement | null
  acquireDialog()
  document.addEventListener('keydown', onKeydown, true)
  void moveFocusIn()
}

/**
 * 离开打开态：停止监听、归还焦点、解锁
 *
 * @returns 无返回值；副作用为把焦点还给打开弹窗的那个元素（若它还在文档里）
 */
function deactivate(): void {
  if (!locked) return
  locked = false
  document.removeEventListener('keydown', onKeydown, true)
  releaseDialog()

  const target = lastActive
  lastActive = null
  // 触发元素可能已随列表刷新被移除，故先确认它仍在文档中
  if (target && document.contains(target)) target.focus()
}

// immediate：组件可能一开始就以 open=true 挂载（父组件用 v-if 控制时）
watch(() => props.open, (isOpen) => (isOpen ? activate() : deactivate()), { immediate: true })

// 组件被销毁时同样要解绑监听与解锁，否则会泄漏全局状态
onBeforeUnmount(deactivate)
</script>

<style scoped>
/* ─── 遮罩 ─────────────────────────────────────────────── */
.overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--overlay-scrim);
  /* 遮罩也要压暗+ 模糊，才能让弹窗"浮起来"；4px 太浅、玻璃感出不来 */
  backdrop-filter: blur(10px) saturate(0.92);
  -webkit-backdrop-filter: blur(10px) saturate(0.92);
  /* visibility 一起参与过渡：淡出期间就不会再挡住底层的点击 */
  opacity: 0;
  visibility: hidden;
  transition: opacity var(--dur) var(--ease), visibility var(--dur) var(--ease);
}
.overlay.show {
  opacity: 1;
  visibility: visible;
}
/* 全局确认框变体：必须压在普通弹窗之上（弹窗内点删除会再弹确认框）。
   原 GlobalModal 自绘外壳时的 z-index 就是 9999，迁移后语义不变。 */
.overlay.global {
  z-index: 9999;
}

/* ─── 面板 ─────────────────────────────────────────────── */
.base-modal {
  /* relative：side 布局的右上角关闭钮以面板为定位基准 */
  position: relative;
  display: flex;
  flex-direction: column;
  max-width: 92vw;
  max-height: 86vh;
  /* 玻璃化：比面板玻璃更实（--glass-strong），因为弹窗是**浮在最上层**的聚焦层，
     需要比下层玻璃更明确边界；模糊半径也更大（28 vs 20px），视觉上"更靠近眼睛"。 */
  background: var(--glass-strong);
  backdrop-filter: blur(28px) saturate(1.7);
  -webkit-backdrop-filter: blur(28px) saturate(1.7);
  border: 1px solid var(--glass-hair);
  border-radius: var(--r-xl);
  /* 上边缘内高光是玻璃厚度的来源；--shadow-pop 提供浮起感 */
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    inset 0 -1px 0 var(--glass-lo),
    var(--shadow-pop);
  overflow: hidden;
  outline: none;
}
/* 动画挂在 .show 上而不是面板本身：面板常驻于 DOM，创建时就播放的话
   等用户真正打开时动画早已结束；跟随 class 变化才能每次打开都播一遍。 */
.overlay.show .base-modal {
  animation: base-modal-in var(--dur) var(--ease);
}
@keyframes base-modal-in {
  from {
    opacity: 0;
    transform: scale(0.97) translateY(8px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

/* 宽度档位：把原先各弹窗写死的 width 收敛成四档梯队 */
.size-sm {
  width: 460px;
}
.size-md {
  width: 520px;
}
.size-lg {
  width: 560px;
}
.size-xl {
  width: 680px;
}

/* 侧栏 tab 布局：固定高度 + 宽面板。高度固定是本变体的核心诉求——
   tab 多的弹窗（设置 / 研究工作台）此前高度随内容变化，切一次 tab 跳一次。
   宽度同样在这里定死（size 档位是按"内容单列"设计的，对双列布局太窄）；
   写在 size-* 之后，靠同特异性后来居上的规则覆盖宽度。
   flex 方向改为 row：宿主在默认插槽里放「aside 左列 + 主区列」两个直接子元素。 */
.base-modal.layout-side {
  /* 关闭钮占位高度：由宿主（SettingsModal / ResearchModal）的标题栏高度决定。
     宿主的内容滚动区须把这个变量加进 padding-top，否则首行内容会被标题栏遮挡。 */
  --side-close-reserve: 48px;
  width: min(880px, 92vw);
  height: min(80vh, 720px);
  flex-direction: row;
}
</style>
