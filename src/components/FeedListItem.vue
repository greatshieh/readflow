<template>
  <!-- 单条订阅源行：未读点 / 首字母头像（含图标叠加回退）/ 名称截断 / 管理按钮 / 未读数。
       折叠（v-show）与 v-for 由父组件控制，本组件只负责一行的渲染与两个动作。 -->
  <div class="feed-item" :class="{ active, unread: feed.unread_count > 0 }" @click="emit('select')">
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
           未读数与间距后只剩约 130px，长标题必然被切。title 让鼠标悬浮时能读到
           全称——此处不能用"悬停时展开换行"来实现，因为管理按钮同样在 hover 时
           才出现，行高一旦变化，鼠标划过列表会让下方内容整体跳动。 -->
      <div class="feed-item-title" :title="feed.name">{{ feed.name }}</div>
    </div>
    <!-- 管理入口：悬浮在条目上时显示，点击打开订阅源管理弹窗（查看链接 / 重命名 / 删除）。
         @click.stop 阻止冒泡，避免误触"选中订阅源"逻辑而切走当前文章。 -->
    <button class="feed-manage-btn" :title="'管理订阅源'" @click.stop="emit('manage')">
      <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <circle cx="12" cy="5" r="2"></circle>
        <circle cx="12" cy="12" r="2"></circle>
        <circle cx="12" cy="19" r="2"></circle>
      </svg>
    </button>
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
 * 渲染一行订阅源（未读点 / 头像与图标回退 / 名称 / 管理按钮 / 未读数），
 * 并把两个动作上抛：点击行 → `select`，点击「⋯」→ `manage`。
 * 与 ArticleListItem 同款范式：父组件（FeedPanel）持有列表、分组与选中态，本组件无状态。
 *
 * # 与文章列表的 .feed-item 的关系
 * ArticleListItem.vue 里也有同名 scoped `.feed-item`（文章列表行），
 * 两者几何不同、互不相通；styles.css 窄屏全局规则对两者同时生效（类名契约，不可改名）。
 */

import type { Feed } from '@/types'

defineProps<{
  /** 要渲染的订阅源 */
  feed: Feed
  /** 是否为当前选中源（驱动主色指示条与文字高亮） */
  active: boolean
}>()

const emit = defineEmits<{
  /** 点击行：父组件写入选中态并收起窄屏抽屉 */
  (e: 'select'): void
  /** 点击「⋯」：父组件打开管理弹窗 */
  (e: 'manage'): void
}>()

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
  position: relative;
  transition: background var(--dur) var(--ease), transform var(--dur) var(--ease),
    box-shadow var(--dur) var(--ease);
}
.feed-item:hover {
  background: var(--fill);
  /* 轻微右移 + 抬升：让鼠标反馈"有质感"而不只是变色 */
  transform: translateX(2px);
}
.feed-item:active { transform: scale(0.985); }
.feed-item.active { background: var(--primary-soft); }
/* 选中态指示条：悬浮 drew 出主色小圆条，给"当前在读哪个源"一个明确的视觉锚点 */
.feed-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 24%;
  bottom: 24%;
  width: 3px;
  border-radius: var(--r-pill);
  background: var(--primary);
}
.feed-item.unread .feed-item-title { font-weight: 600; color: var(--text-primary); }
/* 选中态文字取主色，压过上面的未读加粗规则（同特异性靠书写顺序取胜） */
.feed-item.active .feed-item-title { color: var(--primary); font-weight: 600; }

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
  background: var(--primary-fg);
  color: var(--primary);
}

.feed-item-info { flex: 1; min-width: 0; }
.feed-item-title {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  line-height: 1.3;
}

/* 订阅源「管理」入口：悬浮在 feed 条目右侧，默认隐藏，hover 条目时显现，避免常驻干扰列表阅读 */
.feed-manage-btn {
  border: none;
  background: none;
  cursor: pointer;
  color: var(--text-tertiary);
  padding: var(--sp-1);
  border-radius: var(--r-sm);
  display: none;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
}
.feed-item:hover .feed-manage-btn { display: flex; }
.feed-manage-btn:hover { color: var(--primary); background: var(--fill); }
.feed-manage-btn svg { width: 16px; height: 16px; }
</style>
