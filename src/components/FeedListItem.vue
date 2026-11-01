<template>
  <!-- 单条订阅源行：未读点 / 首字母头像（含图标叠加回退）/ 名称截断 / 未读数。
       折叠（v-show）与 v-for 由父组件控制，本组件只负责一行的渲染与三个动作
       （选中 / 右键开配置菜单 / 键盘的同名操作）。

       本行可聚焦（tabindex=0）：配置入口已由"hover 才出现的「⋯」按钮"改为右键菜单，
       若不给键盘留出等价入口，键盘用户将完全够不到订阅源的配置功能——
       故补 Shift+F10 / 菜单键开菜单、Enter / Space 选中，与鼠标语义一一对应。 -->
  <div
    class="feed-item"
    :class="{ active, unread: feed.unread_count > 0 }"
    tabindex="0"
    @click="emit('select')"
    @contextmenu.prevent="emit('contextmenu', $event)"
    @keydown="onKeydown"
  >
    <div class="feed-item-dot"></div>
    <div class="feed-item-avatar">
      {{ feed.name.charAt(0) }}
      <!-- 图标叠加层：源有图标时覆盖首字母（仿 Folo 侧边栏 logo）；
           加载失败（favicon 404 等）时隐藏自身、回退为首字母头像。
           :key 绑定图标地址，图标被刷新流程更新后强制重挂载、清除上次的隐藏状态 -->
      <img
        v-if="feed.icon"
        class="feed-item-avatar-img"
        :key="feed.icon"
        :src="feed.icon"
        loading="lazy"
        alt=""
        @error="hideBrokenIcon"
      />
    </div>
    <div class="feed-item-info">
      <!-- 名称单行截断（见 .feed-item-title 的 ellipsis）：订阅栏扣掉缩进、头像、
           未读数与间距后只剩约 130px，长标题必然被切。title 让鼠标悬浮时能读到全称——
           此处不能用"悬停时展开换行"来实现：行高一旦变化，鼠标划过列表会让下方内容整体跳动。 -->
      <div class="feed-item-title" :title="feed.name">{{ feed.name }}</div>
      <!-- 源级错误图标：抓取失败的源旁常驻显示感叹号，hover / focus 查看失败原因。
           常驻可见是刻意的——否则用户永远不知道这个源已经坏了。
           传 focusable：让图标进入 Tab 顺序，键盘用户可以聚焦它、读到失败原因，
           而不是只能靠鼠标 hover。aria-label 挂在 trigger 上（经 $attrs 透传），
           为焦点落点提供文字说明；内层 svg 保持 aria-hidden，避免重复朗读。 -->
      <AppTooltip
        v-if="feed.last_error"
        :content="feed.last_error"
        focusable
        aria-label="订阅源抓取失败，查看详情"
      >
        <svg
          class="feed-error-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path>
          <line x1="12" y1="9" x2="12" y2="13"></line>
          <line x1="12" y1="17" x2="12.01" y2="17"></line>
        </svg>
      </AppTooltip>
    </div>
    <!-- 未读数：直接以彩色数字显示在最右端（非 pill/tag 样式）；
         仅在有未读时渲染，避免满屏 0。样式是全局类（styles.css 的 .feed-item-count），
         因为文件夹分组头与标签行也用同一个类。 -->
    <span class="feed-item-count" v-if="feed.unread_count > 0">{{ feed.unread_count }}</span>
  </div>
</template>

<script setup lang="ts">
/**
 * 订阅源侧边栏的单行条目
 *
 * # 职责
 * 渲染一行订阅源（未读点 / 头像与图标回退 / 名称 / 错误图标 / 未读数），
 * 并把三个动作上抛：点击行 → `select`，右键或 Shift+F10 → `contextmenu`（父组件据此开配置菜单）。
 * 与 ArticleListItem 同款范式：父组件（FeedPanel）持有列表、分组与选中态，本组件无状态。
 *
 * # 与文章列表的 .feed-item 的关系
 * ArticleListItem.vue 里也有同名 scoped `.feed-item`（文章列表行），
 * 两者几何不同、互不相通；styles.css 窄屏全局规则对两者同时生效（类名契约，不可改名）。
 */

import AppTooltip from '@/components/AppTooltip.vue'
import type { Feed } from '@/types'

defineProps<{
  /** 要渲染的订阅源 */
  feed: Feed
  /** 是否为当前选中源（驱动主色指示条与文字高亮） */
  active: boolean
}>()

const emit = defineEmits<{
  /** 点击行 / 键盘 Enter·Space：父组件写入选中态并收起窄屏抽屉 */
  (e: 'select'): void
  /** 右键 / 键盘 Shift+F10·菜单键：父组件在事件坐标处弹出配置菜单 */
  (e: 'contextmenu', event: MouseEvent | KeyboardEvent): void
}>()

/**
 * 行级键盘处理
 *
 * 把鼠标的两条路径补齐到键盘上，缺一不可：
 * - 菜单键 / Shift+F10 → 开配置菜单（Windows 与多数 Linux 桌面的通用约定）
 * - Enter / Space → 选中该源（与点击等价）
 *
 * 不改用 `@keydown.enter` 这类修饰符：菜单键在 Vue 里对应的是 `ContextMenu` 这个
 * key 值，hyphenate 之后是 `context-menu`，写成 `.contextmenu` 静默匹配不上。
 *
 * 只在本行自身持有焦点时生效：行内还嵌着可聚焦的错误图标（AppTooltip 的 trigger），
 * 若把从它冒泡上来的按键也当成本行的语义，用户 Tab 到图标后按 Space 会意外切换订阅源
 * （列表被重置，阅读位置也一起丢），而预期只是看错误原因。
 *
 * @param e - keydown 事件
 * @returns 无返回值
 */
function onKeydown(e: KeyboardEvent) {
  if (e.target !== e.currentTarget) return
  if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
    e.preventDefault()
    emit('contextmenu', e)
    return
  }
  if (e.key === 'Enter' || e.key === ' ' || e.key === 'Spacebar') {
    // 阻止 Space 的默认滚动：本行的 Space 语义是"选中"，不应连同滚动页面
    e.preventDefault()
    emit('select')
  }
}

/**
 * 订阅源图标加载失败时的回退
 *
 * 直接把出错的 `<img>` 隐藏，露出下层的首字母头像（仿 Folo 的站点 logo）。
 * 之所以改 DOM 而不清空 `feed.icon`：后者会污染 store 数据，且图标地址一旦为空，
 * 后续刷新再也拿不回来。模板上的 `:key="feed.icon"` 会在图标被刷新替换时重挂载
 * 该节点，从而自动清掉这里留下的隐藏状态。
 *
 * @param e - `<img>` 的 error 事件
 */
function hideBrokenIcon(e: Event) {
  const img = e.target as HTMLImageElement | null
  if (img) img.style.display = 'none'
}
</script>

<style scoped>
/* 样式自 FeedPanel 原样搬入。注意 .feed-item-count 已提升为全局类（styles.css），
   此处不重复定义；.feed-panel / .feed-list 等容器样式仍属父组件。 */
.feed-item {
  display: flex;
  align-items: center;
  /* 加大子元素间距：dot / avatar / info / count 四段依次排列，
     8px 视觉上偏紧，调大让点与文字信息之间有呼吸感 */
  gap: var(--sp-4);
  padding: var(--sp-2) var(--sp-15);
  margin-bottom: var(--sp-025);
  border-radius: var(--r-md);
  cursor: pointer;
  /* z-index: 1 把条目抬到滑动胶囊（z-index: 0）之上：胶囊是 absolute，
     若不加这层，胶囊会盖住条目文字。 */
  position: relative;
  z-index: 1;
  transition: background var(--dur) var(--ease), transform var(--dur) var(--ease),
    box-shadow var(--dur) var(--ease);
}
/* transform 在 :active 下生效；hover 不再有位移，避免与 hover 胶囊产生位差 */
.feed-item:active { transform: scale(0.985); }
/* 选中态的底色与左侧指示条已移除：改由 FeedPanel 里的滑动胶囊统一表达。
   左侧竖条与胶囊会重叠成两道标记，且竖条只对源有效、对标签行无效；
   胶囊则是同一个视觉语言，能在两类条目间连续滑动。 */
.feed-item.unread .feed-item-title { font-weight: 600; color: var(--text-primary); }
/* 选中态文字取 --cap-active-fg：与胶囊底配套。
   浅色主题下胶囊是实色紫罗兰（--cap-solid-bg = --brand-to）、文字为纯白；
   深色主题下胶囊是淡底、文字保持 --text-primary。 */
.feed-item.active .feed-item-title { color: var(--cap-active-fg); font-weight: 600; }

.feed-item-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--primary);
  flex-shrink: 0;
  opacity: 0;
  transition: opacity 0.15s;
}
.feed-item.unread .feed-item-dot { opacity: 1; }

.feed-item-avatar {
  width: 28px;
  height: 28px;
  border-radius: 8px;
  /* 轻微渐变 + 内描边：让头像在白面板上有实体感，而不是一块灰色矩形 */
  background: linear-gradient(135deg, var(--fill-secondary), var(--fill));
  box-shadow: var(--ring-inset);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-secondary);
  flex-shrink: 0;
  overflow: hidden;
  /* 相对定位：图标叠加层（.feed-item-avatar-img）以此为锚铺满覆盖 */
  position: relative;
}
/* 订阅源图标叠加层：绝对定位铺满头像框，加载成功时盖住首字母，
   失败时由 hideBrokenIcon 隐藏、露出字母（仿 Folo 的站点 logo 头像） */
.feed-item-avatar-img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.feed-item.unread .feed-item-avatar {
  background: var(--primary-tint);
  color: var(--primary);
}

.feed-item-info {
  flex: 1;
  min-width: 0;
  /* 改为 flex 行：标题占满剩余空间（flex:1 + min-width:0 保住 ellipsis），
     错误图标固定在右端不被压缩 */
  display: flex;
  align-items: center;
  gap: var(--sp-1);
}
.feed-item-title {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  line-height: 1.3;
}
/* 源级错误图标：抓取失败时常驻显示，取 danger 语义色（styles.css 的 --danger） */
.feed-error-icon {
  width: 14px;
  height: 14px;
  color: var(--danger);
  flex-shrink: 0;
}

/* 键盘聚焦环：订阅行现在可聚焦（配置入口改走右键菜单后，键盘必须有一条路能抵达）。
   用 :focus-visible 而非 :focus——鼠标点击同样会产生焦点，若用 :focus，
   每次点订阅源都会多出一个只在键盘操作时才有意义的方框。
   offset 取负值把环压在行内：面板是 overflow:hidden，向外扩的环会被栏边缘裁掉。 */
.feed-item:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}
</style>
