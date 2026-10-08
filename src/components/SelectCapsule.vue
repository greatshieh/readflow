<template>
  <!--
    滑动选中胶囊：hover 与选中**各一个**，几何完全重合。

    为什么要两个而不是一个：
    若 hover 仍画在条目自身的 background 上，它就位于胶囊之上（条目 z-index:1 >
    胶囊 z-index:0）。于是"在 active 项上 hover"会叠出一层灰，与胶囊底色重影。
    改成 hover 也用胶囊后，两个胶囊位置恒等，active 更实且压在上层，
    重影从结构上不存在——而不是靠调 z-index 或改颜色去掩盖。

    交互分工：**hover 瞬移**（指针扫过是高频连续动作，带过渡会永远追不上、
    像在拖队）；**active 滑动**（点击是一次明确的、有意图的动作，值得看到它滑过去）。
    点击瞬间会暂时压住 hover 胶囊——否则 active 滑过去时落点上已经有个
    几乎同色的胶囊，滑动过程会被完全掩盖，读起来仍是"瞬移"。

    坐标约定：外层用 display:contents，不产生盒子，故两个胶囊作为列表容器的
    直接子元素仍落在同一坐标系内（本组件要求宿主 position: relative）。
  -->
  <div ref="rootEl" class="capsule-root">
  <div
    class="capsule hover"
    :class="[variant, { instant: hoverInstant }]"
    :style="hoverBox"
    aria-hidden="true"
  ></div>
  <div
    class="capsule active"
    :class="[variant, { instant: activeInstant, pulse: pulsing }]"
    :style="activeBox"
    aria-hidden="true"
  ></div>
  </div>
</template>

<script setup lang="ts">
/**
 * 滑动高亮指示器（hover + 选中双胶囊）
 *
 * # 职责
 * 把「指针悬停在哪一项」与「当前选中的是哪一项」都表达成胶囊，二者几何重合。
 * 订阅栏（FeedPanel）与文章列表（ArticleColumn）共用本组件。
 *
 * # 为什么 hover 也要做成胶囊
 * 原本 hover 是条目自身的 background，而条目 z-index 高于胶囊。用户在 active 项上
 * 移动鼠标时，两层底色叠加成灰，与胶囊底色不一致，看起来像"重影"。
 * 让 hover 与选中都是胶囊后：位置恒等、选中更实且在上，重影不再可能发生。
 * 附带收益：两处hover 的几何自动一致，不存在"宽度不匹配"的可能。
 *
 * # 约束
 * - 宿主容器必须 `position: relative`；
 * - 本组件是宿主的**直接子元素**，且渲染在条目之前；
 * - 折叠 / 展开 / 窗口缩放后需再次调用 `sync()`，否则胶囊停在旧位置。
 */

import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'

const props = withDefaults(
  defineProps<{
    /**
     * 视觉变体
     * - `solid`：实玻璃底（订阅栏用）——承载"当前在看哪个"这条强信息，需要更实；
     * - `accent`：琥珀描边 + 淡琥珀底（文章列表用）——弱信息，品牌琥珀只此一处。
     */
    variant?: 'solid' | 'accent'
    /** 宿主容器的 ref，用于观察尺寸变化（折叠 / 缩放后重算位置） */
    container: () => HTMLElement | null
  }>(),
  { variant: 'solid' },
)

/** 根容器：仅用于 querySelector 拿到 active 胶囊元素（监听 transitionend） */
const rootEl = ref<HTMLElement | null>(null)

/**
 * 滑过过程中用于枚举条目序列的选择器
 *
 * 从容器里取出全部可见条目，才能算出"起点到终点跨了几条"，据此按距离决定
 * 滑动时长（见 walkTo）。取不到时（宿主用了自定义条目类名）退化为直接落位，
 * 不影响基本行为。
 */
const ITEM_SELECTOR = '.feed-item, .tag-row, .article-item'

/**
 * 主体滑动时**每跨一条**所用的时长（ms）
 *
 * 主体是一条过渡走完全程，故这个值直接决定"每格感"：55ms 跨一条时，
 * 6条约 330ms 完成，节奏紧凑但不急促。
 *
 * 它比早期版本的"每格 130ms"小得多——不是因为偷工，而是因为**不再有停顿**：
 * 旧方案主体每格都要停稳，那种"走一下停一下"的顿挫本身就是卡带来源，
 * 缩短间隔只会变成快放的卡带，并不会变顺。
 */
const SLIDE_PER_STEP_MS = 55
/**
 * 主体滑动总时长上限（ms）
 *
 * 与条目数成正比但设上限：点到很远的条目时，若不设限会等好几秒（像卡住）。
 * 取 620ms —— 跨 11 条以内都是线性时长，超过则压到这个上限。
 */
const MAX_WALK_MS = 620
/**
 * 走完后额外等待（ms）才补回 hover 胶囊
 *
 * 主体是**一条过渡走完全程**，finishWalk 已在同一时长上触发，故这里只需留一点
 * 缓冲让最后那点收尾完全落下，不必再等一个完整的长过渡时长。
 */
const SETTLE_PAD_MS = 90
/**
 * 一帧的时长（ms）
 *
 * 滑动用 rAF 启动（先落起点、下一帧再滑），故收尾计时要比滑动时长多一帧，
 * 否则会在过渡刚开始时就解除走动态，胶囊立刻切回普通过渡曲线、看起来又顿一下。
 */
const FRAME_MS = 16

/** hover 胶囊的位置；null 表示指针不在任何条目上，胶囊隐藏 */
const hoverTop = ref<number | null>(null)
const hoverHeight = ref(0)
/** 选中胶囊的位置；null 表示无选中项 */
const activeTop = ref<number | null>(null)
const activeHeight = ref(0)
/**
 * 隐藏状态下仍要显示的位置（px）
 *
 * 必须是响应式的：它被 `box()` 读出并写进 computed 的产物里，若是普通变量，
 * 在 top 置null 的那一帧 computed 不会重算，胶囊就会跳回原点。单独存一份是为了
 * 让"隐藏时沿用最后位置"这件事有个明确的来源，而不是靠 top 的旧值侥幸留存。
 */
const hoverLastY = ref(0)
const activeLastY = ref(0)
/** 首帧 / 折叠重算 / 布局变化时跳过渡直接落位 */
const hoverInstant = ref(false)
const activeInstant = ref(false)
/** 一次性定位脉冲（命令面板跳转后播一次，不常驻） */
const pulsing = ref(false)
/** 主体滑动进行中：期间隐藏 hover 胶囊，免得与主体叠在一起 */
const walking = ref(false)
/** 本次主体滑动的总时长（ms）——写进 --walk-dur 供样式表过渡使用 */
const walkDur = ref(SLIDE_PER_STEP_MS)
/** hover 胶囊根元素：走动期间整体隐藏，避免逐个改样式 */
const hovering = ref<HTMLElement | null>(null)
/** 在途的走动定时器；非 null 表示走动尚未结束 */
let walkTimer: ReturnType<typeof setTimeout> | null = null

/**
 * 生成胶囊的内联样式：用 transform 表达位置与高度
 *
 * 位置用 `translateY`（合成器属性，不触发重排），高度用 `height`（胶囊内无内容，
 * 不影响条目文字排版）。曾用 scaleY 拉伸固定盒高来模拟高度，那会把圆角拉成椭圆、
 * 且让内容看起来没在框里居中——两者都是过度设计的产物。见样式表内的长注释。
 *
 * @param top - 条目相对定位基准的顶端偏移（px）；null 表示隐藏
 * @param height - 条目高度（px）
 * @param lastY - 上一次可见的位置，用于隐藏时保持原地淡出（避免闪跳）
 * @returns 可直接绑定的 CSSProperties
 */
function box(top: number | null, height: number, lastY: number) {
  // 隐藏时**仍须写出 transform 与 height**：只给 opacity 的话，Vue 会把上一次
  // 内联的这两项一并移除，胶囊在淡出的那一帧会跳回原点并缩成 0 高，
  // 表现为"消失时闪一下"。故隐藏时沿用最后的值，让它原地淡出。
  const y = top ?? lastY
  return {
    transform: `translateY(${y}px)`,
    height: `${height}px`,
    opacity: top === null ? '0' : '1',
  }
}
const hoverBox = computed(() => box(hoverTop.value, hoverHeight.value, hoverLastY.value))

const activeBox = computed(() => {
  const style = box(activeTop.value, activeHeight.value, activeLastY.value)
  // 把本次滑动的总时长交给样式表：JS 用的 walkDur 与 CSS 的
  // transition-duration 必须是同一个值，否则 JS 已在收尾、CSS 还在走，
  // 两者错位会让主体看起来"到位后又顿一下"。
  return { ...style, '--walk-dur': `${walkDur.value}ms` }
})

/** 当前分别被两个胶囊指示的条目 */
let currentHover: HTMLElement | null = null
let currentActive: HTMLElement | null = null
let observer: ResizeObserver | null = null

/**
 * 把 hover 胶囊移到指定条目——**始终瞬时落位，不做过渡**
 *
 * 为什么不给 hover 加滑动：指针扫过列表是连续的高频动作，胶囊若带 0.26s 过渡，
 * 会永远"追不上"指针，看起来像有几只胶囊在后面拖队。瞬时落位才是"跟手"的，
 * 也与视频里的表现一致（"整个后台都亮了"是即时的，只有点击才带位移感）。
 *
 * 需要滑动的只有选中胶囊——那是一次明确的、有意图的点击，见 `sync()`。
 *
 * @param el - 目标条目；传 null 表示指针不在条目上，胶囊淡出
 * @returns 无返回值
 */
function hover(el: HTMLElement | null): void {
  if (!el) {
    hoverTop.value = null
    // 同时清掉 currentHover：否则下一次 mouseover 同一个条目会被下面的
    // 短路挡住，hover 胶囊再也回不来（点击压住过它之后就会这样）。
    currentHover = null
    return
  }
  // 指针已在该条目上时不重写样式：避免重复 setProperty 触发无谓的样式重算
  if (el === currentHover) return
  const base = props.container()
  if (!base) return
  currentHover = el
  hoverInstant.value = true
  const { top, height } = measure(el, base)
  hoverTop.value = top
  hoverLastY.value = top
  hoverHeight.value = height
  // 下一帧解除 instant：本次已瞬时落位，后续若因布局变化需要重算仍能正常过渡
  requestAnimationFrame(() => (hoverInstant.value = false))
}

/**
 * 把选中胶囊移到指定条目
 *
 * @param el - 目标条目；传 null 表示无选中项，胶囊淡出
 * @param sliding - true 表示**滑过去**（用户主动点击）；
 *   false 表示瞬时落位（首帧 / 折叠重算 / 换范围等布局变化）。
 *   默认 false：布局变化远多于点击，那些场景下带过渡会被看成卡顿。
 * @returns 无返回值
 */
function sync(el: HTMLElement | null, sliding = false): void {
  if (!el) {
    activeTop.value = null
    cancelWalk()
    return
  }

  // 起点必须在改写 currentActive **之前**取：sync 的第一件事就是
  // currentActive = el，若放到之后再取，起点会等于终点，滑动会被误判为"无需移动"。
  const previous = currentActive
  currentActive = el

  // 无论滑动与否都先打断在途的走动：快速连点时，旧定时器不该继续影响新目标。
  cancelWalk()

  if (!sliding) {
    // 瞬时落位（折叠重算、命令面板跳转等）：不要位移过渡
    activeInstant.value = true
    placeActive(el)
    requestAnimationFrame(() => (activeInstant.value = false))
    return
  }

  walkTo(el, previous)
}

/**
 * 测量条目相对定位基准的几何（结果已换算回 **未缩放**的 CSS 像素）
 *
 * # 为什么要除以 zoom
 * 应用根容器 `.app` 上有 `zoom: var(--ui-zoom)`（界面字体缩放，默认 1）。
 * `getBoundingClientRect()` 返回的是**缩放后**的视觉尺寸，而胶囊的
 * `top` / `height` 是内联 CSS 值，处于未缩放的坐标系里。zoom ≠ 1 时两者
 * 直接相减会得到偏大的值，胶囊画得比实际条目大一号——表现为高亮框与内容
 * 错位（订阅源尤其明显，因为它条目矮、误差占的比例大）。
 *
 * 因此这里把差值除以 zoom 换算回去。用 `offsetTop` / `offsetHeight`
 * 也能避开缩放，但它们返回**整数**且不含 margin，在小高度条目上误差同样明显；
 * rect 是浮点、含边框、除以 zoom 后即为精确值。
 *
 * @param el - 目标条目
 * @param base - 定位基准容器（列表滚动区）
 * @returns 相对基准的 top 与 height（未缩放的 CSS 像素）
 */
function measure(el: HTMLElement, base: HTMLElement): { top: number; height: number } {
  const r = el.getBoundingClientRect()
  const b = base.getBoundingClientRect()
  // 取基准自身的 zoom：它是缩放的实际施加者（与 el 的一致），
  // 故用它换算比读全局变量更可靠——即便将来局部区域有不同缩放也能正确工作。
  const zoom = b.height > 0 ? b.height / base.offsetHeight : 1
  const z = zoom > 0 ? zoom : 1
  return {
    top: (r.top - b.top) / z + base.scrollTop,
    height: r.height / z,
  }
}

/**
 * 把选中胶囊的落位写到某个条目上
 *
 * 统一入口：`walkTo` 的整块滑动与各处瞬时落位都走这里，保证 lastY 与 top 同步更新。
 *
 * @param el - 目标条目
 * @returns 无返回值
 */
function placeActive(el: HTMLElement): void {
  const base = props.container()
  if (!base) return
  const { top, height } = measure(el, base)
  activeTop.value = top
  activeLastY.value = top
  activeHeight.value = height
}

/**
 * 让胶囊滑向目标条目：**一条过渡连续滑完全程**
 *
 * # 为什么强调"连续"
 * 早期版本让主体"跳一格、停一下"逐格点亮，那种阶梯节奏**本身就是卡带来源**——
 * 无论每格多短都顿挫，缩短间隔只会变成快放的卡带。故改为一条过渡走完全程、
 * 全程匀速无停顿。顺滑由此而来，"经过了哪些中间项"交给滑过本身的视觉表达。
 *
 * @param target - 目标条目
 * @param from - 起点条目；null 表示此前没有选中项（首次点击）
 * @returns 无返回值
 */
function walkTo(target: HTMLElement, from: HTMLElement | null): void {
  cancelWalk()
  const box = props.container()
  if (!box) {
    // 拿不到容器：无法测量，直接瞬时落位
    activeInstant.value = true
    placeActive(target)
    requestAnimationFrame(() => (activeInstant.value = false))
    return
  }

  const items = [...box.querySelectorAll<HTMLElement>(ITEM_SELECTOR)]
  // 首次点击（from 为 null）时以"列表首项"为起点，让胶囊从顶部滑下来，
  // 而不是凭空出现在目标上——否则第一次点击完全没有位移反馈，
  // 看起来就像"要点第二次才生效"。
  const to = items.indexOf(target)
  // 起点下标。from 为 null（首次点击）时以首项为起点。
  //
  // **fromIdx 可能是 -1**：from 是上一次选中的那个 DOM 元素，而列表在
  // 加载更多 / 换排序 / 删条目后会重渲染，旧元素已脱离文档、不在 items 里。
  // 此时不能退化成"以首项为起点"——那会让胶囊从列表顶部一路滑下来，
  // 与用户心里的"从上一条移过去"完全不符。正确做法是**直接瞬时落位**。
  const fromIdx = from ? items.indexOf(from) : 0
  const origin = fromIdx >= 0 ? items[fromIdx] : null
  const distance = to - fromIdx

  // 无法确定起点（from 已失效）或目标本身异常：瞬时落位。
  // 点同一个条目（distance === 0）也走这里——本来就是原地，无可滑。
  // 相邻条目（|distance| === 1）**要滑**：点第 N 条后马上点 N+1 条时，
  // 若判成"距离太近不值得滑"，这一次就完全没有位移反馈、看起来像"没反应"。
  if (origin === null || to < 0 || distance === 0) {
    activeInstant.value = false
    placeActive(target)
    return
  }

  // 滑动期间压住 hover 胶囊：点击必然发生在指针已悬停的目标条目上，
  // hover 胶囊已瞬移到同一位置，留着它会与主体叠在一起、糊成一团。
  hoverTop.value = null
  hovering.value?.classList.add('is-walking')

  const steps = Math.abs(distance)
  // 主体总时长：与条目数成正比，但设上限，避免点很远时等太久。
  // 每格基准 55ms ——比旧的 130ms 快一倍以上，因为不再有"停顿"需要留时间。
  const totalMs = Math.min(MAX_WALK_MS, steps * SLIDE_PER_STEP_MS)

  // 给 walking 态挂 --walk-dur，让样式表按本次时长过渡（一条走完，不分段）
  walking.value = true
  walkDur.value = totalMs

  // 先把胶囊**瞬时**放到起点，再在下一帧滑向终点。
  // 这一步不能省：胶囊当前停在"上一次选中的位置"（首次点击时停在 top:0），
  // 若直接写终点位置，浏览器会拿这个旧值做过渡起点——首次点击时等于从
  // top:0 滑到目标（正常），但连续点击时新旧两帧的过渡会串在一起，
  // 表现为"要点第二次才滑"。每帧各走一步、互不干扰。
  //
  // 关键：必须分两步、且第一步要**等 DOM 真的更新完**再进第二步。
  // 浏览器的 transition 靠"两次样式计算之间的差异"触发，所以起点状态必须
  // 真正被渲染出来一次。若像原先那样在同一个 rAF 里既解除 instant 又写终点，
  // Vue 会把两处样式变更合并成一次 patch —— 浏览器从未见过"起点"这个状态，
  // transition 不会启动，表现就是"点了没反应/ 第二次才动"。
  // 少一个 await nextTick，动画就会静默失效。
  activeInstant.value = true
  placeActive(origin)
  void nextTick(() => {
    // 若这期间用户又点了别的（cancelWalk 已跑、walking 已置回 false），
    // 就不要再写终点——那次点击已经把胶囊安排到它的新目标上了，
    // 这里写会与之打架，表现为"快速连点时胶囊抖一下"。
    if (!walking.value) return
    activeInstant.value = false
    placeActive(target)
  })

  // 收尾计时与主体同长，另加一帧：上面用了 rAF 才启动过渡，
  // 若按totalMs 计时，会在过渡刚开始时就解除走动态，胶囊会立刻切回
  // 普通过渡曲线，看起来像"到位后又顿一下"。
  walkTimer = setTimeout(finishWalk, totalMs + FRAME_MS)
}

/**
 * 走完：解除走动态，并恢复 hover 胶囊
 *
 * @returns 无返回值
 */
function finishWalk(): void {
  walking.value = false
  hovering.value?.classList.remove('is-walking')
  walkTimer = setTimeout(restoreHoverAfterWalk, SETTLE_PAD_MS)
}

/**
 * 取消进行中的走动
 *
 * 快速连续点击时必须能打断：否则旧的定时器会继续把胶囊推向它的目标，
 * 与新的一次点击打架。
 *
 * @returns 无返回值
 */
function cancelWalk(): void {
  if (walkTimer !== null) {
    clearTimeout(walkTimer)
    walkTimer = null
  }
  walking.value = false
  hovering.value?.classList.remove('is-walking')
}

/**
 * 滑完后恢复 hover 胶囊
 *
 * 点击必然发生在指针已悬停的目标条目上，而指针通常不会再动（不会再来一次
 * mouseover）。若不在此补回，hover 胶囊会一直缺席到用户移开再移回。
 *
 * @returns 无返回值
 */
function restoreHoverAfterWalk(): void {
  const base = props.container()
  if (!base || !currentHover || currentHover !== currentActive) return
  const { top, height } = measure(currentHover, base)
  hoverTop.value = top
  hoverHeight.value = height
}

/**
 * 播一次定位脉冲
 *
 * 用于「从别处跳过来」的定位反馈：命令面板回车后，用户需要确认落在了哪一项。
 * 是一次性动效而非常驻状态。
 *
 * @returns 无返回值
 */
function pulse(): void {
  if (!currentActive) return
  pulsing.value = false
  // 强制重启动画：置 false 后等一帧再置 true，否则连续两次跳转不会重播
  requestAnimationFrame(() => (pulsing.value = true))
}

/** 布局变化（折叠 / 展开 / 缩放）后重算两个胶囊的位置 */
function reposition(): void {
  // 布局变了但选中项没变（折叠 / 展开 / 窗口缩放），两个胶囊都瞬时跟上——
  // 若带过渡，折叠时会看到胶囊从旧位置慢慢飞过来，读起来像卡顿而非跟随。
  if (currentHover) hover(currentHover)
  if (currentActive) sync(currentActive)
}

onMounted(() => {
  const box = props.container()
  if (box) {
    observer = new ResizeObserver(reposition)
    observer.observe(box)
  }
  hovering.value = rootEl.value?.querySelector<HTMLElement>('.capsule.hover') ?? null
})

onBeforeUnmount(() => {
  // 必须清掉在途定时器：否则它会在组件销毁后继续写样式
  cancelWalk()
  observer?.disconnect()
  observer = null
})

defineExpose({ hover, sync, pulse, reposition })
</script>

<style scoped>
/* 根容器不产生盒子：display:contents 让两个胶囊在 DOM 上"透明"，
   于是它们仍是宿主列表容器的直接子元素、落在同一坐标系内。
   若改成普通 div，它会成为 offsetParent，胶囊的 offsetTop 参照系就变了。 */
.capsule-root { display: contents; }

/* 位置用 transform 而非 top —— 见文件末尾「为什么不用 top」的长注释。
   transform 由合成器处理，不触发重排；而面板上有 backdrop-filter: blur，
   任何重排都会连带重算整块模糊区域，动画就会卡成"瞬移感"。 */
.capsule {
  position: absolute;
  left: var(--cap-left, 8px);
  right: var(--cap-right, 8px);
  /* 高度由 JS 按目标条目的 offsetHeight 写入，胶囊内没有任何内容，
     故改变它**不会让条目内的文字重排**——这是与 scaleY 方案的关键差别。
     （曾用 scaleY 拉伸固定盒高来"模拟"高度，但那会连圆角一起拉伸，
     且文字位置与被拉伸的盒子中心不一致，导致内容看起来没在框里居中。） */
  border-radius: var(--r-md);
  pointer-events: none;      /* 绝不拦截点击：胶囊只是视觉覆盖层 */
  z-index: 0;                /* 压在条目之下，条目文字始终在上层 */
  /* 位移只过渡 transform（合成器属性，不触发重排）；
     高度也过渡，让胶囊在跨到不同高度的条目时平滑改变形状。 */
  transition: transform var(--dur-slide) var(--ease-slide),
    height var(--dur-slide) var(--ease-slide),
    opacity 0.12s linear;
  /* 提升为独立合成层，让位移走 GPU 而不是每帧重绘 */
  will-change: transform;
  backface-visibility: hidden;
}

/* 位移为什么用 transform 而不用 top：
   top 是**布局属性**，每一帧都会触发重排。面板自身带 backdrop-filter: blur(20px)，
   重排会让它重新采样并模糊背后的像素——这部分开销与动画同频发生，列表稍长就
   明显掉帧，读起来是"没动直接到了"。transform 走合成器（GPU），不触发重排、
   也不重绘背景模糊，动画始终跟手。

   高度为什么用 height 而不用 scaleY：
   胶囊是空盒子（内部无内容），改它的 height **不会让条目里的文字重排**——
   条目是独立的兄弟节点，两者互不影响。而 scaleY 是把固定盒高"拉伸"来模拟高度，
   会同时把圆角一起拉成椭圆；更要紧的是被拉伸的盒子与条目文字的**中心基准不一致**，
   表现为"内容没在高亮框里居中"。曾用 scaleY 兜过，正是这个问题的来源。 */

/* 瞬时落位（首帧 / 折叠重算 / hover 跟手）：不产生滑动。
   只关掉 transform 的过渡，保留 opacity —— 否则胶囊显隐时会闪一下。 */
.capsule.instant { transition: opacity 0.12s linear; }

/* ── 主体滑动（点击后高亮连续滑向目标）─────────────────────────────
   walking 期间覆盖 transition-duration 为本次滑动的总时长（--walk-dur），
   使主体成为**一条连续的过渡**：全程匀速、无任何停顿。
   这一点是"顺滑"的全部关键——早期版本让主体每格跳一格、停一下，
   那种阶梯节奏无论间隔多短都像卡带。 */
.capsule.active.walking {
  transition-duration: var(--walk-dur, 320ms);
  /* 滑动途中稍微提亮，与静止时区分开 */
  filter: brightness(1.06);
}

/* ── hover 胶囊：只是"路过"，故两套主题同值、只给淡底 ────────────── */
.capsule.hover.solid,
.capsule.hover.accent {
  background: var(--cap-hover-bg);
}

/* ── 选中胶囊：必须明显强于 hover，否则"当前在哪"读不出来 ──
   浅色走实色琥珀（--cap-solid-bg = --brand-from）、深色走淡底 + 描边，
   两套取值都在 styles.css 里，这里不感知主题。 */
.capsule.active.solid,
.capsule.active.accent {
  background: var(--cap-solid-bg);
  box-shadow:
    inset 0 0 0 1px var(--cap-line),
    0 1px 3px var(--cap-shadow),
    0 6px 16px var(--cap-shadow);
}
/* 文章列表的选中胶囊用独立的琥珀档位（描边更亮一档），与订阅栏拉开一点差异 */
.capsule.active.accent {
  background: var(--cap-accent-bg);
  box-shadow:
    inset 0 0 0 1px var(--cap-accent-line),
    0 1px 3px var(--cap-accent-shadow),
    0 4px 12px var(--cap-accent-shadow);
}

/* 定位脉冲：一次性，播完即止。用琥珀描边扩散，表达"就是这一项" */
.capsule.pulse { animation: capsule-pulse 1.05s var(--ease-slide); }
@keyframes capsule-pulse {
  0% {
    box-shadow:
      inset 0 0 0 1px var(--cap-line),
      0 0 0 0 var(--cap-accent-line),
      0 4px 12px var(--cap-shadow);
  }
  55% {
    box-shadow:
      inset 0 0 0 1px var(--cap-line),
      0 0 0 9px transparent,
      0 4px 12px var(--cap-shadow);
  }
  100% {
    box-shadow:
      inset 0 0 0 1px var(--cap-line),
      0 0 0 0 transparent,
      0 4px 12px var(--cap-shadow);
  }
}

/* 无障碍：关掉动效后两个胶囊仍是静态色块，位置与强弱关系不受影响 */
@media (prefers-reduced-motion: reduce) {
  .capsule { transition: opacity 0.12s linear; }
  .capsule.pulse { animation: none; }
}
</style>