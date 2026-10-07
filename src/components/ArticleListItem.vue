<template>
  <!-- 文章条目卡片：列表栏的最小渲染单元，自身就是一个可点击的卡片。
       未读圆点内联在标题行的行首（绝对定位到信息块左侧的预留位），取代原先漂浮在
       卡片边缘、与内容无对齐关系的做法；元素常驻、靠 opacity 切换，
       使已读行的标题不被"顶格"而破坏整体左对齐。 -->
  <div
    class="article-item"
    :class="{ active, unread: !article.is_read }"
    @click="emit('select', article)"
  >
    <div class="feed-item-info">
      <div class="feed-item-dot"></div>
      <div class="feed-item-title">{{ article.title }}</div>
      <div class="feed-item-meta">
        <span class="feed-item-source">
          <span class="source-logo" v-if="feedIcon">
            <img class="source-logo-img" :src="feedIcon" @error="hideBrokenIcon" alt="" />
          </span>
          <span class="source-logo source-letter" v-else>{{ feedName.charAt(0) }}</span>
          {{ feedName }}
        </span>
        <span class="article-time">{{ formatDistance(article.published_at) }}</span>
      </div>
      <!-- 标签 chip：最多直显 3 个，其余折叠为 +N。
           限行不换行是为了让所有条目保持等高，避免"带标签的条目突然变高"
           破坏列表的扫描节奏 -->
      <div class="feed-item-tags" v-if="article.tags && article.tags.length">
        <span
          v-for="t in article.tags.slice(0, 3)"
          :key="t.id"
          class="tag-chip"
          :style="{ '--tag-c': tagColorVar(t.color) }"
          :title="t.name"
          >{{ t.name }}</span
        >
        <span class="tag-chip tag-chip-more" v-if="article.tags.length > 3"
          >+{{ article.tags.length - 3 }}</span
        >
      </div>
    </div>
    <div class="feed-item-thumb" v-if="thumb">
      <img :src="thumb" loading="lazy" alt="" />
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 文章列表条目（ArticleColumn 的列表行）
 *
 * # 职责
 * 渲染单篇文章的一行：未读圆点 + 标题 + 来源 / 时间 + 标签 chip + 缩略图。
 * 组件只回答「长什么样」与「被点了」，选中之后的副作用（标记已读、窄屏收起抽屉）
 * 由父组件处理——保持无状态，列表滚动重渲染才不会牵动全局。
 *
 * # 为什么独立成组件
 * 这段模板 + 样式此前占了 ArticleColumn 近三分之一：一次条目样式微调要在 1200 行里翻找。
 *
 * # 依赖的全局类（改名会静默失效）
 * - `.tag-chip` / `.tag-chip-more`：定义在 `styles.css`，三处列表共用；
 * - `.article-item` / `.feed-item-*`：被 `styles.css` 的窄屏规则
 *   `.feed-item, .article-item { padding; min-height }` 命中，类名不可改。
 */

import { computed } from 'vue'
import { useFeedsStore } from '@/stores/feeds'
import { tagColorVar } from '@/stores/tags'
import { formatDistance } from '@/utils/format'
import { getThumbnail } from '@/utils/articleThumb'
import type { Article } from '@/types'

const props = defineProps<{
  /** 要渲染的文章 */
  article: Article
  /** 是否为当前选中（正在阅读）的条目，决定高亮态 */
  active: boolean
}>()

const emit = defineEmits<{
  /** 条目被点击：由父组件决定「标记已读 + 窄屏收起抽屉」 */
  (e: 'select', article: Article): void
}>()

const feedsStore = useFeedsStore()

/** 来源名称：订阅源列表里查不到时退回 `Feed {id}`，保证不出现空白来源 */
const feedName = computed<string>(() => {
  const feed = feedsStore.feeds.find((f) => f.id === props.article.feed_id)
  return feed ? feed.name : `Feed ${props.article.feed_id}`
})

/** 来源图标 URL（空串表示无图标，模板据此退回首字母兜底） */
const feedIcon = computed<string>(() => {
  const feed = feedsStore.feeds.find((f) => f.id === props.article.feed_id)
  return feed?.icon || ''
})

/** 缩略图 URL（空串表示该文无图，此时整个图片位不渲染） */
const thumb = computed<string>(() => getThumbnail(props.article))

/**
 * 图标加载失败时隐藏 img，露出底下的首字母兜底
 *
 * @param e - img 的 error 事件
 * @returns 无返回值；直接改写当前元素的 display
 */
function hideBrokenIcon(e: Event) {
  ;(e.currentTarget as HTMLImageElement).style.display = 'none'
}
</script>

<style scoped>
.article-item {
  display: flex;
  align-items: center;
  /* 与订阅栏 .feed-item 保持一致：加大间距让缩略图 / 标题 / 时间戳之间更透气 */
  gap: var(--sp-4);
  /* 左右内边距 12 → 16：卡片被撑得更饱满，标题与卡片边缘之间留出呼吸感。
     左内边距收回到 12px 以抵消标题区 14px 的圆点悬挂缩进，
     使"卡片左边缘 → 标题文字"的距离与右侧保持相近 */
  padding: 11px var(--sp-4) 11px var(--sp-3);
  /* 外边距 4 → 6：卡片之间的间隙与两侧留白同步放大 */
  margin: 0 var(--sp-05) var(--sp-05);
  cursor: pointer;
  transition: background var(--dur) var(--ease), box-shadow var(--dur) var(--ease),
    transform var(--dur) var(--ease);
  position: relative;
  /* 卡片化：圆角取缔原先贯穿整行的 1px 分隔线 */
  border-radius: var(--r-lg);
}
.article-item:hover {
  background: var(--fill);
  transform: translateY(-1px);
  box-shadow: var(--shadow-sm);
}
/* 选中项浮起成一张小卡：比背景色变化更能表达"正在读这一篇" */
.article-item.active {
  background: var(--surface);
  box-shadow: var(--shadow-hover);
}
.article-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 22%;
  bottom: 22%;
  width: 3px;
  border-radius: var(--r-pill);
  background: var(--primary);
}
.article-item.unread .feed-item-title { font-weight: 600; }
.article-item:not(.unread) .feed-item-title { color: var(--text-secondary); }

/* 未读圆点：定位到 .feed-item-info 左侧的预留位（14px = 圆点 6px + 间距 8px），
   垂直方向与标题首行居中对齐（标题行高约 19px，故 top 取 6px）。
   元素常驻、靠 opacity 切换，使已读行的标题不被"顶格"而破坏整体左对齐。 */
.feed-item-dot {
  position: absolute;
  left: 0;
  top: 6px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--primary);
  opacity: 0;
  transition: opacity 0.15s;
}
.article-item.unread .feed-item-dot { opacity: 1; }

.feed-item-info {
  position: relative;
  flex: 1;
  min-width: 0;
  /* 圆点预留位：标题与元信息整体右移，给行首圆点留出悬挂缩进 */
  padding-left: var(--sp-35);
}
.feed-item-title {
  font-size: var(--fs-md);
  color: var(--text-primary);
  font-weight: 400;
  line-height: 1.45;
  /* 轻微收紧字距：中英混排的现代阅读器观感（负字距让标题更像"标题"） */
  letter-spacing: -0.01em;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.feed-item-meta {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  margin-top: var(--sp-1);
}
.feed-item-source {
  color: var(--primary);
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
  min-width: 0;
}
/* 列表项标签行：单行不换行，超出部分由 chip 自身的 max-width + 省略号收束，
   保证"有标签"与"无标签"的条目高度一致（换行会让列表节奏忽高忽低） */
.feed-item-tags {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  margin-top: var(--sp-05);
  overflow: hidden;
  min-width: 0;
}

/* 来源小 logo：14px 圆角方块，字母兜底 + 图标叠加 */
.source-logo {
  width: 14px;
  height: 14px;
  border-radius: 3px;
  background: var(--fill);
  color: var(--text-secondary);
  font-size: 9px;
  font-weight: 600;
  line-height: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  overflow: hidden;
  position: relative;
}
.source-letter { background: var(--fill); }
.source-logo-img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
/* 来源名过长时截断，避免把右侧的时间戳挤出卡片 */
.feed-item-meta span:first-child {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.feed-item-thumb {
  width: 52px;
  height: 52px;
  border-radius: var(--r-md);
  overflow: hidden;
  flex-shrink: 0;
  background: var(--fill);
  box-shadow: var(--ring-inset);
}
.feed-item-thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
</style>
