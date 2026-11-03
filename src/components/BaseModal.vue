<template>
  <!-- 模态弹窗外壳：遮罩 + 居中面板 + 无障碍语义。
       业务弹窗只负责往默认插槽里放内容（一般是「头部 + 主体」两段），
       遮罩点击、Esc 关闭、焦点陷阱与滚动锁都由本组件统一处理。

       Teleport 到 body 是必需的，不能省：订阅栏等面板带 `backdrop-filter`，
       而 backdrop-filter 会**创建包含块**，使内部的 position:fixed 退化为
       absolute —— 遮罩于是被局限在面板盒子里、并被面板的 overflow:hidden 裁掉
       （表现为弹窗被"关"在栏内）。移到 body 下即彻底脱离该坐标系。 -->
  <Teleport to="body">
  <div class="overlay" :class="{ show: open }" @mousedown.self="onOverlayMousedown">
    <div
      ref="panelRef"
      class="base-modal"
      :class="[`size-${size}`, layout === 'side' ? 'layout-side' : '', { global, 'side-wide': sideWide }]"
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
 * # 打开初期的遮罩点击会被忽略（300ms）
 * `mousedown` 解决了"拖动误判"，但解决不了"手滑"：点完打开入口后手还压在鼠标键上、
 * 双击的第二下、触控板抬起时的余震，都会在弹窗刚出现的那一瞬落到遮罩上。
 * 故 `onOverlayMousedown` 会丢掉打开后 `OVERLAY_GUARD_MS` 内的点击。
 * 这是**迟滞**而不是禁用——过了窗口行为与原来完全一致；Esc 与 ✕ 从不迟滞，
 * 它们是明确意图，给它们加延迟只会让人觉得卡。
 *
 * # 与全局确认框的优先级
 * 全局确认框（`GlobalModal`，z-index 9999）可能叠在弹窗之上（如弹窗内点删除）。
 * 此时 Esc 属于最上层，本组件检出 `modalStore.activeModal` 后主动让路，
 * 避免"按一次 Esc 关掉两层"。
 *
 * # 参数
 * * `open` - 是否打开；关闭态面板只是不可见，组件本身常驻
 * * `title` - 弹窗标题，写入 `aria-label` 供读屏播报
 * * `size` - 面板宽度档位：sm 460px / md 520px / lg 560px / xl 680px / 2xl 820px
 * * `layout` - 布局档位：`default`（默认）高度随内容自适应；
 *   `side` 固定高度（min(80vh, 720px)）+ 宽面板，供 tab 较多的弹窗
 *   （设置 / 研究工作台）把 tab 竖排在左侧——弹窗高度不再随切 tab 跳变。
 *   side 布局下宽度由本变体定（min(880px, 92vw)），size 档位不再参与。
 *   **本组件不渲染关闭钮**：模板只有 `<slot />`，关闭钮在各宿主的标题栏里
 *   （`SettingsModal` / `ResearchModal` 的 `.btn-icon`）；`--side-close-reserve`
 *   只是给宿主对齐用的占位高度参考。要"收进外壳"属功能改动，须单独排期。
 * * `sideWide` - 仅在 `layout="side"` 下生效：把面板从 880 加宽到 1000。
 *   给研究工作台的报告页用（正文 + 引用源右栏需要两列），设置弹窗保持 880。
 *   **不要直接改 `.layout-side` 的宽度**——那是两个宿主共用的，改一处会连带改另一处。
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
    size?: 'sm' | 'md' | 'lg' | 'xl' | '2xl' | '3xl'
    /** 布局：default 高度随内容自适应；side 固定高度 + 宽面板，宿主内部排「左 tab 列 + 右内容列」 */
    layout?: 'default' | 'side'
    /** side 布局下的加宽档位（880 → 1000），只有研究工作台传 */
    sideWide?: boolean
    /** 全局确认框变体（GlobalModal 专用）：z-index 压过普通弹窗，且键盘事件不向 activeModal 让路 */
    global?: boolean
  }>(),
  { size: 'lg', layout: 'default', sideWide: false },
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
 * 打开初期忽略遮罩点击的时长（ms）
 *
 * 应对的是**手滑**而非"不想关"：点完入口按钮后手还没离开鼠标键、双击的第二下、
 * 或者手指从触控板抬起时的余震，都会在弹窗刚出现时落到遮罩上。
 * 弹窗进场动画是 200ms（`--m-in`），取 300ms 覆盖"动画期间 + 落定后一瞬"这段
 * 最容易误触的窗口。
 *
 * 只拦遮罩的 `mousedown`：Esc、✕ 按钮、任何键盘路径都不受影响——那些是明确意图，
 * 给它们加迟滞只会让人觉得卡。
 */
const OVERLAY_GUARD_MS = 300

/**
 * 本次进入打开态的时刻（`performance.now()` 基准）
 *
 * 用 `performance.now()` 而不是 `Date.now()`：这里量的是时长，不该受系统时钟
 * 被校时或用户改时间影响。
 */
let openedAt = 0

/**
 * 遮罩被按下：关闭弹窗，但打开初期的手滑不算
 *
 * 语义边界要说清：这是**防手滑迟滞**，不是"禁止点遮罩关闭"。过了 {@link OVERLAY_GUARD_MS}
 * 之后，行为与改动前完全一致。
 *
 * @returns 无返回值；超过迟滞窗口时 `emit('close')`
 */
function onOverlayMousedown(): void {
  if (performance.now() - openedAt < OVERLAY_GUARD_MS) return
  emit('close')
}

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
  // 记下进入打开态的时刻，供遮罩点击的防手滑迟滞判断（见 OVERLAY_GUARD_MS）
  openedAt = performance.now()
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
  /* ── 运动尺度：4 档，按用途定义 ──
     挂在 .overlay 上：自定义属性沿 DOM 继承，面板与插槽内容都能直接消费。
     刻意**不重定义** --dur / --ease —— 它们被弹窗外 30 余处使用，改值会波及整个界面。 */
  --m-tap: 90ms;                                 /* T1 即时：悬停、按下、焦点环 */
  --m-state: 160ms;                              /* T2 状态：页签切换、选中、开关 */
  --m-in: 200ms;                                 /* T3 面板进 */
  --m-out: 120ms;                                /* T4 面板出：0.6× 于进场，退出永远比进入快 */
  --m-ease-out: cubic-bezier(0.2, 0.8, 0.2, 1);  /* 进 / 悬停 / 状态：快起慢停 */
  --m-ease-in: cubic-bezier(0.4, 0, 1, 1);       /* 退场：末段加速离开，不留尾巴 */

  position: fixed;
  /* 不用 inset:0 —— 叠了 zoom 之后 inset 会按缩放后的单位解析，遮罩比视口大、
     把面板推出中心。改成 calc 反算，缩放之后恰好铺满视口。 */
  top: 0;
  left: 0;
  width: calc(100vw / var(--ui-zoom, 1));
  height: calc(100vh / var(--ui-zoom, 1));
  /* 弹窗跟随「界面字体大小」：zoom 只挂在 .app 上，而本组件 Teleport 到 body，
     落在 zoom 容器之外。不补这行，用户把界面字号调到「大」时弹窗会小 9%。 */
  zoom: var(--ui-zoom, 1);
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--overlay-scrim);
  /* 遮罩也要压暗 + 模糊，才能让弹窗"浮起来"；4px 太浅、玻璃感出不来 */
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  opacity: 0;
  visibility: hidden;
  /* pointer-events 基础态为 none：关闭瞬间即让出点击，**不依赖 visibility 的过渡时序**。
     visibility 是离散属性，渐隐时要到整个过渡结束才跳到 hidden —— 那 120ms 里遮罩
     仍是铺满全屏的可点击层，用户关掉弹窗后立刻点别处会"第一下点空"。
     这行不能当冗余删掉；缩短时长也不等于修好（症状变轻 ≠ 病因消除）。 */
  pointer-events: none;
  transition:
    opacity var(--m-out) var(--m-ease-in),
    /* duration 0s + delay = 退场时长：淡出结束才 hidden，否则面板会在第一帧就消失。
       delay 必须与面板退场时长严格相等，两者都读同一个 --m-out，天然同步不会漂移。 */
    visibility 0s linear var(--m-out);
}
.overlay.show {
  opacity: 1;
  visibility: visible;
  pointer-events: auto;
  /* 开门方向 delay 必须是 0s：否则打开时先空白一个 --m-out 才出现 */
  transition:
    opacity var(--m-in) var(--m-ease-out),
    visibility 0s;
}
/* 全局确认框变体：必须压在普通弹窗之上（弹窗内点删除会再弹确认框）。
   原 GlobalModal 自绘外壳时的 z-index 就是 9999，迁移后语义不变。
   注意不要为了"更浮"抬高普通弹窗的 z-index：AppSelect 下拉面板同值 1000、
   靠 DOM 顺序取胜，抬到 1001 以上会让弹窗内所有下拉掉到遮罩后面。 */
.overlay.global {
  z-index: 9999;
}

/* ─── 面板 ─────────────────────────────────────────────── */
.base-modal {
  /* ── 弹窗排版尺度：六个角色，全部是既有字号阶梯的别名 ──
     用别名而不是直接写 var(--fs-xl)，是为了把「这一处是标题」留在代码里；
     阶梯变则弹窗自动跟随，而 --fs-* 的值一个字没改 ⇒ 全应用其余部分零影响。
     作用域说明：自定义属性靠 DOM 继承传到插槽内容；而 `.base-modal .m-title`
     这类后代选择器写在本文件里**选不中插槽节点**（那些元素带的是宿主的 data-v），
     所以变量只能定义在这一层，消费写在各宿主自己的 scoped 样式里。 */
  --fs-modal-title: var(--fs-xl);    /* 18  弹窗标题（每个弹窗恰好一个） */
  --fs-modal-section: var(--fs-lg);  /* 16  区块标题 */
  --fs-modal-body: var(--fs-md);     /* 14  正文：说明段落、消息、摘要 */
  --fs-modal-label: var(--fs-sm);    /* 13  标签 / 列表主字段 / 页签 */
  --fs-modal-meta: var(--fs-xs);     /* 11  元信息：时间 / 来源 / 计数 / 徽标 */
  --fs-modal-num: var(--fs-2xl);     /* 22  单值大数字 */

  --lh-modal-title: 1.35;
  --lh-modal-section: 1.5;
  --lh-modal-body: 1.65;
  --lh-modal-label: 1.4;
  --lh-modal-meta: 1.5;
  --lh-modal-num: 1.1;

  /* 排版基线：未单独设字号的文字按「正文」渲染，而不是继承 body 的 14px
     再叠上字体自带的 normal 行高（中文 ≈1.15，行与行几乎贴着）。
     行高只给这一层，各角色在自己的规则里覆盖，不要在几十个选择器里重复数字。 */
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);

  /* relative：side 布局的右上角关闭钮以面板为定位基准 */
  position: relative;
  display: flex;
  flex-direction: column;
  /* 除 zoom：面板的宽高上限是相对视口的，缩放后要按同一比例反算才不会越界 */
  max-width: calc(92vw / var(--ui-zoom, 1));
  max-height: calc(86vh / var(--ui-zoom, 1));
  /* 玻璃化：比面板玻璃更实（--glass-strong），因为弹窗是**浮在最上层**的聚焦层，
     需要比下层玻璃更明确边界；模糊半径也更大（28 vs 20px），视觉上"更靠近眼睛"。 */
  background: var(--glass-strong);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid var(--glass-hair);
  /* 弹窗圆角 16px（--r-panel）。**不要用 --shell-r**：外壳满窗后那个令牌是 0，
     用了会让弹窗变直角。 */
  border-radius: var(--r-panel);
  /* 与其余浮层同一套口径：外圈 --glass-hair 定形，顶边一道 --glass-hi 内高光
     给"厚度"，最外层 --shadow-pop 负责浮起。命令面板、右键菜单、tooltip、
     订阅栏的「更多」菜单都是这两层。

     此前这里是 `border` + `inset 0 0 0 1px var(--shell-border)`：内外两道 1px
     线紧挨着画，2px 宽的边上出现两种颜色；而且 --shell-border 是**外壳**的
     发丝线（= var(--border)），它不该出现在浮层上——浮层浮在内容之上，
     边界要读成"玻璃的边缘"，不是"窗口的分隔线"。 */
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    var(--shadow-pop);
  overflow: hidden;
  outline: none;
  /* 进出场两个方向都走 transition：基础态即「退场终点」，撤掉 .show 是一次有前值的
     属性变化，所以退场不需要任何 JS，也不存在"忘记写退出"这种失败模式
     （原先 animation 挂在 .show 上，关闭时声明整条消失 ⇒ 面板原地消失，没有形体）。
     位移取 6px 而不是 8px：它同时是进场起点与退场终点，一次开关只付一次
     28px 模糊重算的代价。
     ⚠ 新增 transform 必须与下面这行合并写在同一处声明里：将来若有人把居中改成
     translate(-50%,-50%)，分散两条会让退场直接跳到左上角。 */
  opacity: 0;
  transform: translateY(6px);
  transition:
    opacity var(--m-out) var(--m-ease-in),
    transform var(--m-out) var(--m-ease-in);
}
.overlay.show .base-modal {
  opacity: 1;
  transform: translateY(0);
  transition:
    opacity var(--m-in) var(--m-ease-out),
    transform var(--m-in) var(--m-ease-out);
}
/* 面板本身也可能拿到焦点：面板内没有任何可聚焦元素时 moveFocusIn 会落到面板上，
   此时默认 outline:none 会让键盘用户看不见焦点在哪。
   offset 取负值画在面板内侧，否则会被 overflow:hidden 裁掉。 */
.base-modal:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -4px;
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
/* 2xl：给内容密度高的聚合类弹窗（实体仪表盘：五个区块纵向排布）用，
   在 92vw / max-width 约束内尽可能宽 */
.size-2xl {
  width: 820px;
}
/* 3xl：实体仪表盘在「概览带 + 双列卡片」改版后需要更宽的水平空间，
   960px 给双列卡片与概览带留出呼吸感；仅实体仪表盘使用，不影响其他弹窗 */
.size-3xl {
  width: 960px;
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
/* 加宽档位：研究工作台的报告页要放「正文 + 引用源右栏」两列，
   880 下正文只剩 486px（含右栏就挤没了）。1000 给正文留出 600+，
   仍落在中文舒适行宽（420–630px）内；再宽只是留白，不加。
   高度同步提到 86vh：报告比设置项长，垂直空间比宽度更值钱。 */
.base-modal.layout-side.side-wide {
  /* 上界用 92vw 而不是 94vw：`.base-modal` 的 max-width 就是 92vw（按 zoom 反算），
     写更大的值只会被裁掉，看起来像"设了没用"。 */
  width: min(1000px, 92vw);
  height: min(86vh, 720px);
}
</style>
