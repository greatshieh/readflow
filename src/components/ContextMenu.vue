<script lang="ts">
/**
 * 右键菜单的数据契约
 *
 * 用独立普通 `<script>` 块导出接口：`<script setup>` 内的声明无法被外部 import。
 *
 * # 字段语义
 * - `separator: true` 的分隔线自己占一行，此时只需填 `key`（供 v-for 做 key）
 * - `children` 非空即成为带右侧箭头的父项，展开为飞出的子菜单（目前只有「移动到文件夹」用）
 * - `icon` 是 SVG 的 `d` 属性值（可为多段子路径），不是整段 SVG 字符串——
 *   组件内统一用 `<path :d>` 渲染，既避免 `v-html` 注入面，也保证描边风格一致
 */
export interface ContextMenuItem {
  /** 唯一标识；子项的 key 与父项共用同一命名空间（回调里靠它分派动作） */
  key: string
  /** 显示文案；分隔线可省略 */
  label?: string
  /** SVG path 的 d 值（24×24 视图框，描边风格） */
  icon?: string
  /** 危险项（如取消订阅）：hover 与文字取 danger 语义色 */
  danger?: boolean
  /** 禁用项：不可点、键盘导航跳过 */
  disabled?: boolean
  /** 渲染为分隔线而非菜单项 */
  separator?: boolean
  /** 子菜单项 */
  children?: ContextMenuItem[]
}
</script>

<script setup lang="ts">
/**
 * ContextMenu —— 通用右键菜单
 *
 * # 职责
 * 在给定视口坐标处弹出菜单，承载「点击某项执行动作」这一件事：
 * 视口边缘自动收拢、子菜单飞出、键盘导航、Esc / 点击外部 / 滚动 / 窗口失焦关闭。
 *
 * # 为什么 Teleport 到 body
 * 订阅栏 `.feed-panel` 设有 `overflow: hidden`，且带 `backdrop-filter`
 * （会让内部 `position: fixed` 退化为 absolute）。浮层若留在容器内会被裁掉或错位，
 * 故必须 Teleport 到 body 脱离该坐标系——与 AppTooltip / 「更多」下拉同一处理。
 *
 * # 定位与 zoom 的前提（改动前必读）
 * `.app` 设有 `zoom: var(--ui-zoom)`。本菜单 Teleport 到 body，位于该 zoom 容器**之外**；
 * 调用方传入的 x / y 取自鼠标事件的视口坐标，与 body 级 `fixed` 参照系一致，故可直接定位。
 * 若日后取消 Teleport（菜单回到 zoom 容器内），坐标须先除以 zoom 换算回布局像素，
 * 否则在非 100% 界面缩放下会整体偏移（同 AppTooltip 的注释）。
 *
 * # 关闭策略
 * 不用全屏遮罩层，而是用 document 上的**捕获阶段**监听：
 * 遮罩会吞掉落在其上的右键，导致「在另一个订阅源上再点右键」只是关掉菜单、需要点第二次。
 * 用监听则事件照常抵达目标元素——先关旧菜单、再由目标行的处理函数开新菜单，一次到位。
 */
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue'

const props = defineProps<{
  /** 菜单项（含分隔线与子菜单） */
  items: ContextMenuItem[]
  /** 弹出位置：视口坐标（鼠标事件的 clientX / clientY） */
  x: number
  /** 弹出位置：视口坐标（鼠标事件的 clientX / clientY） */
  y: number
}>()

const emit = defineEmits<{
  /** 某个可执行项被激活（点击或回车）；子菜单项同样以此上抛 */
  (e: 'select', key: string): void
  /** 请求关闭（Esc / 点击外部 / 滚动 / 失焦 / 执行完动作由父组件调用） */
  (e: 'close'): void
}>()

/** 菜单主体（Teleport 到 body，故需单独 ref） */
const menuRef = ref<HTMLElement | null>(null)
/** 子菜单主体 */
const subRef = ref<HTMLElement | null>(null)
/** 菜单落点（fixed 的 left / top，由 place() 算出并收拢进视口） */
const pos = ref<Record<string, string>>({ left: `${props.x}px`, top: `${props.y}px` })
/** 子菜单落点 */
const subPos = ref<Record<string, string>>({})
/** 当前键盘高亮项（鼠标 hover 也写它，两套输入共用同一份高亮） */
const activeKey = ref('')
/** 当前展开的子菜单父项 key；空串表示没有展开 */
const openSubKey = ref('')

/** 菜单距视口边缘的最小安全边距（px） */
const EDGE = 8
/** 子菜单相对父项的水平间距（px） */
const SUB_GAP = 4

/** 可被键盘选中的项（跳过分隔线与禁用项） */
const selectable = computed(() => props.items.filter((i) => !i.separator && !i.disabled))

/** 当前展开的子菜单里可选中的项 */
const subSelectable = computed(() =>
  props.items.find((i) => i.key === openSubKey.value)?.children?.filter((c) => !c.disabled) ?? [],
)

/**
 * 全部项的扁平视图（父项 + 子项，跳过分隔线）
 *
 * 键盘高亮要跨两级查找：只看主菜单的话，箭头移进子菜单后 activeItem() 会找不到对象。
 *
 * @returns 扁平后的菜单项数组
 */
const flatItems = computed<ContextMenuItem[]>(() => {
  const out: ContextMenuItem[] = []
  for (const item of props.items) {
    if (item.separator) continue
    out.push(item)
    if (item.children?.length) out.push(...item.children)
  }
  return out
})

/**
 * 量出菜单尺寸后收拢落点
 *
 * 与触发点相比，右侧/下方越界时向左/向上收，始终保留 EDGE 边距——
 * 贴合屏幕右下角的订阅源右键时，菜单不会有一半在视口外。
 *
 * @returns 无返回值；结果写入 `pos`
 */
async function place(): Promise<void> {
  await nextTick()
  const el = menuRef.value
  if (!el) return
  const vw = window.innerWidth
  const vh = window.innerHeight
  // 菜单在 body 下、不受 .app 的 zoom 影响，offsetWidth 即视口像素
  const w = el.offsetWidth
  const h = el.offsetHeight
  let left = props.x
  let top = props.y
  if (left + w > vw - EDGE) left = Math.max(EDGE, vw - EDGE - w)
  if (top + h > vh - EDGE) top = Math.max(EDGE, vh - EDGE - h)
  pos.value = { left: `${left}px`, top: `${top}px` }
}

/**
 * 展开某个父项的子菜单
 *
 * 默认贴在父项右侧；右侧放不下时翻到左侧。纵向同样收拢进视口。
 *
 * @param item - 被展开的父项；无 children 时收起当前子菜单
 * @returns 无返回值
 */
async function openSub(item: ContextMenuItem): Promise<void> {
  if (!item.children?.length) {
    openSubKey.value = ''
    return
  }
  openSubKey.value = item.key
  // 高亮交给子菜单首项：否则键盘用户按 → 展开后高亮仍停在父项上，
  // 接着按 ↓ 会从他处开始走，看起来像"展开了但进不去"
  const entered = subSelectable.value[0]
  if (entered) activeKey.value = entered.key
  await nextTick()
  const wrap = menuRef.value?.querySelector<HTMLElement>(`.ctx-item-wrap[data-key="${item.key}"]`)
  const sub = subRef.value
  if (!wrap || !sub) return
  const r = wrap.getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const w = sub.offsetWidth
  const h = sub.offsetHeight
  let left = r.right + SUB_GAP
  if (left + w > vw - EDGE) left = Math.max(EDGE, r.left - w - SUB_GAP)
  let top = r.top - 6
  if (top + h > vh - EDGE) top = Math.max(EDGE, vh - EDGE - h)
  subPos.value = { left: `${left}px`, top: `${top}px` }
}

/**
 * 激活一个菜单项
 *
 * 叶子项上抛 key 交给父组件执行；带 children 的父项在鼠标路径下切换子菜单开合
 * （键盘路径由 onKeydown 直接展开，不走这里）。
 *
 * @param item - 被激活的菜单项
 * @returns 无返回值
 */
function activate(item: ContextMenuItem): void {
  if (item.separator || item.disabled) return
  if (item.children?.length) {
    if (openSubKey.value === item.key) openSubKey.value = ''
    else void openSub(item)
    return
  }
  emit('select', item.key)
}

/**
 * 键盘上下移动高亮
 *
 * 高亮落在子菜单里就只在子菜单内走，否则走主菜单——两级各自成环，
 * 这样 ↑↓ 不会在用户看不见的层级间跳。
 *
 * @param delta - +1 向下、-1 向上
 * @returns 无返回值
 */
function moveActive(delta: number): void {
  const list = currentList()
  if (!list.length) return
  const cur = list.findIndex((i) => i.key === activeKey.value)
  const next = cur < 0 ? (delta > 0 ? 0 : list.length - 1) : (cur + delta + list.length) % list.length
  activeKey.value = list[next].key
}

/**
 * 当前键盘作用的那一层
 *
 * 高亮落在子菜单里就用子菜单、否则用主菜单。↑↓ 与 Home/End 都读它，
 * 两处若各判一次，很容易改了一处漏一处。
 *
 * @returns 当前层级的可选项数组
 */
function currentList(): ContextMenuItem[] {
  return subSelectable.value.some((i) => i.key === activeKey.value) ? subSelectable.value : selectable.value
}

/**
 * 找到当前高亮项对应的数据对象
 *
 * @returns 高亮项；无高亮或已失效时返回 null
 */
function activeItem(): ContextMenuItem | null {
  return flatItems.value.find((i) => i.key === activeKey.value) ?? null
}

/**
 * 收起子菜单并把高亮交还父项
 *
 * @returns 无返回值
 */
function exitSub(): void {
  const parent = openSubKey.value
  openSubKey.value = ''
  if (parent) activeKey.value = parent
}

/**
 * 菜单级键盘处理
 *
 * Esc 先收子菜单、再关菜单（两级返回，与文件管理器一致）；
 * ← / → 进出子菜单；↑ / ↓ / Home / End 移动；Enter / Space 激活。
 *
 * @param e - keydown 事件
 * @returns 无返回值
 */
function onKeydown(e: KeyboardEvent): void {
  switch (e.key) {
    case 'Escape':
      e.preventDefault()
      if (openSubKey.value) exitSub()
      else emit('close')
      return
    case 'ArrowDown':
      e.preventDefault()
      moveActive(1)
      return
    case 'ArrowUp':
      e.preventDefault()
      moveActive(-1)
      return
    case 'Home':
      e.preventDefault()
      activeKey.value = currentList()[0]?.key ?? ''
      return
    case 'End': {
      e.preventDefault()
      const list = currentList()
      activeKey.value = list[list.length - 1]?.key ?? ''
      return
    }
    case 'ArrowRight': {
      const cur = activeItem()
      if (cur?.children?.length) {
        e.preventDefault()
        void openSub(cur)
      }
      return
    }
    case 'ArrowLeft':
      if (openSubKey.value) {
        e.preventDefault()
        exitSub()
      }
      return
    case 'Enter':
    case ' ':
    case 'Spacebar': {
      const cur = activeItem()
      if (!cur) return
      e.preventDefault()
      if (cur.children?.length) void openSub(cur)
      else activate(cur)
      return
    }
    default:
      return
  }
}

/** 目标是否落在菜单内（含子菜单） */
function isInside(target: Node): boolean {
  // 子菜单与主菜单在模板里是**平级**节点（都挂在 body 下），不是主菜单的子元素，
  // 只判 menuRef 会漏——后果是按下子菜单项时先被当作"点击外部"关闭，
  // 元素随即卸载，随后的 click 根本不会触发，子菜单项整片失效。
  return !!menuRef.value?.contains(target) || !!subRef.value?.contains(target)
}

/**
 * document 捕获阶段的「点在菜单外」判定
 *
 * 捕获阶段先于目标元素自身的处理函数执行，因此「先关旧菜单、再由目标行开新菜单」
 * 能在同一次右键里完成，用户只需点一次。
 *
 * @param e - mousedown / contextmenu 事件
 * @returns 无返回值
 */
function onDocDown(e: Event): void {
  const t = e.target as Node | null
  if (!t) return
  if (isInside(t)) return
  emit('close')
}

/** 滚动会让 fixed 菜单与内容脱节，直接关闭；菜单自身内部滚动不在此列 */
function onScroll(e: Event): void {
  const t = e.target as Node | null
  if (t && isInside(t)) return
  emit('close')
}

/** 窗口尺寸变化后原坐标不再可靠，关闭即可（用户重新右键一次成本极低） */
function onResize(): void {
  emit('close')
}

onMounted(async () => {
  await place()
  menuRef.value?.focus()
  document.addEventListener('mousedown', onDocDown, true)
  document.addEventListener('contextmenu', onDocDown, true)
  window.addEventListener('scroll', onScroll, true)
  window.addEventListener('resize', onResize)
  window.addEventListener('blur', onResize)
})

onUnmounted(() => {
  document.removeEventListener('mousedown', onDocDown, true)
  document.removeEventListener('contextmenu', onDocDown, true)
  window.removeEventListener('scroll', onScroll, true)
  window.removeEventListener('resize', onResize)
  window.removeEventListener('blur', onResize)
})
</script>

<template>
  <Teleport to="body">
    <div
      ref="menuRef"
      class="ctx-menu"
      role="menu"
      tabindex="-1"
      :style="pos"
      @contextmenu.prevent
      @keydown="onKeydown"
    >
      <template v-for="(item, i) in items" :key="item.key || `sep-${i}`">
        <div v-if="item.separator" class="ctx-sep" role="separator"></div>
        <div
          v-else
          class="ctx-item-wrap"
          :data-key="item.key"
          @mouseenter="activeKey = item.key; openSub(item)"
        >
          <button
            class="ctx-item"
            :class="{ danger: item.danger, active: activeKey === item.key }"
            role="menuitem"
            :disabled="item.disabled"
            :aria-haspopup="item.children?.length ? 'menu' : undefined"
            :aria-expanded="item.children?.length ? openSubKey === item.key : undefined"
            @click="activate(item)"
          >
            <svg
              v-if="item.icon"
              class="ctx-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path :d="item.icon"></path>
            </svg>
            <span v-else class="ctx-icon"></span>
            <span class="ctx-label">{{ item.label }}</span>
            <svg
              v-if="item.children?.length"
              class="ctx-caret"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <polyline points="9 6 15 12 9 18"></polyline>
            </svg>
          </button>
        </div>
      </template>
    </div>

    <!-- 子菜单：与主菜单平级渲染（而非嵌在父项内），这样它同样是 body 下的 fixed 层，
         定位直接用父项的视口矩形换算，不受主菜单滚动或圆角裁剪影响 -->
    <div
      v-if="openSubKey && items.find((i) => i.key === openSubKey)?.children?.length"
      ref="subRef"
      class="ctx-menu ctx-sub"
      role="menu"
      :style="subPos"
      @contextmenu.prevent
    >
      <button
        v-for="child in items.find((i) => i.key === openSubKey)?.children"
        :key="child.key"
        class="ctx-item"
        :class="{ danger: child.danger, active: activeKey === child.key }"
        role="menuitem"
        :disabled="child.disabled"
        @mouseenter="activeKey = child.key"
        @click="activate(child)"
      >
        <span class="ctx-icon"></span>
        <span class="ctx-label">{{ child.label }}</span>
      </button>
    </div>
  </Teleport>
</template>

<style scoped>
/* 与「更多」下拉同一套视觉语言：玻璃底 + 玻璃描边 + 弹起阴影。
   z-index 取 950：高于面板光斑等装饰层，低于 BaseModal 的 1000——
   点菜单项打开的弹窗必须盖住菜单，否则菜单会浮在弹窗之上。 */
.ctx-menu {
  position: fixed;
  z-index: 950;
  min-width: 172px;
  max-height: 70vh;
  overflow-y: auto;
  background: var(--glass-strong);
  backdrop-filter: blur(24px) saturate(1.7);
  -webkit-backdrop-filter: blur(24px) saturate(1.7);
  border: 1px solid var(--glass-hair);
  border-radius: var(--r-lg);
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    var(--shadow-pop);
  padding: var(--sp-05);
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
  /* 菜单本体可聚焦（tabindex=-1）：去掉默认焦点环，键盘操作的高亮由 .active 表达 */
  outline: none;
}
/* 分隔线：一条极细的玻璃割线，比纯色边框更贴合玻璃底 */
.ctx-sep {
  height: 1px;
  margin: var(--sp-05) var(--sp-2);
  background: var(--border);
  flex-shrink: 0;
}
.ctx-item-wrap {
  display: block;
}
.ctx-item {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  width: 100%;
  padding: var(--sp-2) var(--sp-3);
  border: none;
  background: none;
  border-radius: var(--r-md);
  font-size: var(--fs-sm);
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
  font-family: inherit;
  transition: background 0.15s;
}
/* hover 与键盘高亮共用同一份样式：鼠标与键盘是同一套语言，不该出现两种高亮 */
.ctx-item:hover,
.ctx-item.active {
  background: var(--cap-hover-bg);
  color: var(--primary);
}
.ctx-item:disabled {
  color: var(--text-tertiary);
  cursor: default;
  background: none;
}
.ctx-item.danger:hover,
.ctx-item.danger.active {
  background: var(--danger-fg, var(--cap-hover-bg));
  color: var(--danger);
}
/* 图标列恒占宽：子菜单项没有图标，靠这个占位与父菜单文字左对齐 */
.ctx-icon {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}
.ctx-label {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ctx-caret {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  opacity: 0.6;
}
.ctx-sub {
  min-width: 148px;
}
</style>
