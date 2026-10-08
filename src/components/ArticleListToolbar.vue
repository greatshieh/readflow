<template>
  <!-- 列表顶部固定操作条：对齐 Folo 的文章列表头部。
       搜索框吃掉按钮之外的全部宽度（flex:1），其余图标按钮被推到最右。 -->
  <div class="list-toolbar">
    <div class="tb-actions">
      <!-- 文章搜索：后端全文检索（覆盖标题 / 正文 / 摘要，含中文）。
           防抖与结果消费在父组件（ArticleColumn）——本组件只负责 v-model 与清空入口。
           类名 .tb-search 被 App.vue 的「/」快捷键按选择器查找，不可改名。 -->
      <div class="tb-search">
        <svg class="s-icon" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="8"></circle>
          <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
        </svg>
        <input type="text" placeholder="搜索文章（标题/正文）..." v-model="articlesStore.search" />
        <!-- 清空搜索：点击后回到本地列表，避免残留后端结果 -->
        <button v-if="searchActive" class="tb-search-clear" @click="emit('clear-search')" aria-label="清空">×</button>
      </div>
      <!-- 标签范围指示：侧边栏选中标签后出现。标签是范围而非叠加筛选，
           必须有一个显式的"当前处于某标签视图"指示与退出入口，否则用户会误以为
           列表变空是 bug（实际是被标签过滤了） -->
      <button
        v-if="activeTag"
        class="tb-tag"
        :style="{ '--tag-c': tagColorVar(activeTag.color) }"
        @click="emit('clear-tag-scope')"
        :title="`退出标签视图：${activeTag.name}`"
      >
        <span class="tb-tag-dot"></span>
        <span class="tb-tag-name">{{ activeTag.name }}</span>
        <span class="tb-tag-x">×</span>
      </button>
      <!-- 仅显示未读：在 全部 / 未读 间切换（active 表示该筛选已开启）。
           只写 store，重载由父组件的范围键 watcher 兜住 -->
      <button class="tb-icon" :class="{ active: articlesStore.viewFilter === 'unread' }" @click="toggleUnread" title="仅显示未读">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
          <circle cx="12" cy="12" r="3"></circle>
        </svg>
      </button>
      <!-- 仅显示书签（Folo 的 Starred 视图入口，附加在"仅未读"旁不破坏三个主按钮的语义） -->
      <button class="tb-icon" :class="{ active: articlesStore.viewFilter === 'bookmarked' }" @click="toggleStarred" title="仅显示书签">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"></path>
        </svg>
      </button>
      <!-- 仅显示喜欢的文章（心形，与条目内偏好标记同图标） -->
      <button class="tb-icon" :class="{ active: articlesStore.viewFilter === 'liked' }" @click="articlesStore.setViewFilter(articlesStore.viewFilter === 'liked' ? 'all' : 'liked')" title="仅显示喜欢的文章">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"></path>
        </svg>
      </button>
      <!-- 仅显示跳过的文章（划掉的眼睛 = 「不看」，与喜欢对称） -->
      <button class="tb-icon" :class="{ active: articlesStore.viewFilter === 'skipped' }" @click="articlesStore.setViewFilter(articlesStore.viewFilter === 'skipped' ? 'all' : 'skipped')" title="仅显示跳过的文章">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"></path>
          <line x1="1" y1="1" x2="23" y2="23"></line>
        </svg>
      </button>
      <!-- 刷新当前源：单源视图只刷该源（feeds_refresh_one），全部视图则刷全部（feeds_refresh） -->
      <button class="tb-icon" @click="emit('refresh')" :title="feedsStore.selectedFeed ? '刷新当前订阅源' : '刷新全部'" :disabled="feedsStore.refreshing">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" :class="{ spinning: feedsStore.refreshing }">
          <polyline points="23 4 23 10 17 10"></polyline>
          <polyline points="1 20 1 14 7 14"></polyline>
          <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
        </svg>
      </button>
      <!-- 全部已读：把当前范围（单源或全部）的未读一次性标记，并切回"全部"视图避免空列表困惑 -->
      <button class="tb-icon" @click="emit('mark-all')" :disabled="unreadInScope === 0" title="全部标记为已读">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="9 11 12 14 22 4"></polyline>
          <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"></path>
        </svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 文章列表顶部操作条（ArticleColumn 的头部控件区）
 *
 * # 职责
 * 渲染列表顶端那一排控件：搜索框、标签范围指示、仅未读 / 仅书签切换、刷新、全部已读。
 *
 * # 为什么是「直接写 store + 事件回调」的混合边界
 * - 搜索词、视图筛选（viewFilter）**只改 store 状态**：父组件的范围键 watcher 会据此重新
 *   拉取列表，因此这里直接读写 store 更短，也避免把两个双向绑定再透传一层；
 * - 刷新、全部已读、清空搜索、退出标签视图**要触发父组件的动作**（重载列表 / 弹提示 /
 *   复位后端结果），一律 emit 出去，由父组件决定怎么做——组件不掌握"重新取数"这件事。
 *
 * # 依赖的全局选择器
 * `.tb-search input` 被 `App.vue` 的「/」快捷键用 `document.querySelector` 查找并聚焦，
 * 类名不可改名（改名不会报错，只是快捷键静默失效）。
 */

import { computed } from 'vue'
import { useArticlesStore } from '@/stores/articles'
import { useFeedsStore } from '@/stores/feeds'
import { tagColorVar } from '@/stores/tags'
import type { Tag } from '@/types'

const props = defineProps<{
  /** 当前生效的标签范围（null 表示不按标签筛选）；由父组件从 tags 目录反查后传入 */
  activeTag: Tag | null
  /** 当前范围内的未读总数，决定「全部已读」是否可用 */
  unreadInScope: number
}>()

const emit = defineEmits<{
  /** 请求清空搜索（复位搜索框与后端结果） */
  (e: 'clear-search'): void
  /** 请求退出标签视图（回到全部文章） */
  (e: 'clear-tag-scope'): void
  /** 请求刷新当前范围 */
  (e: 'refresh'): void
  /** 请求把当前范围全部标记为已读 */
  (e: 'mark-all'): void
}>()

const articlesStore = useArticlesStore()
const feedsStore = useFeedsStore()

/** 是否处于后端全文搜索模式（搜索框非空），决定清空按钮是否显示 */
const searchActive = computed<boolean>(() => articlesStore.search.trim().length > 0)

/** 在 全部 / 未读 视图间切换（仅未读按钮） */
function toggleUnread() {
  articlesStore.setViewFilter(articlesStore.viewFilter === 'unread' ? 'all' : 'unread')
}

/** 在 全部 / 书签 视图间切换（书签按钮） */
function toggleStarred() {
  articlesStore.setViewFilter(articlesStore.viewFilter === 'bookmarked' ? 'all' : 'bookmarked')
}
</script>

<style scoped>
/* 顶部固定操作条：吸顶在列表之上，宽度与列表一致，不随列表滚动。
   仅剩一组操作按钮（tb-actions），flex-end 使其整体靠右对齐 */
.list-toolbar {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--sp-2);
  padding: var(--sp-15) var(--sp-3);
  flex-shrink: 0;
  border-bottom: 1px solid var(--border);
  /* 透明：面板已是玻璃，这里若铺 --surface 实色会在顶部形成一条不透明横带，
     把玻璃"截断"成上下两段。只需要一条分隔线表达边界。 */
  background: transparent;
}
.tb-actions {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  /* 占满操作条剩余宽度：搜索框吃掉按钮之外的全部空间，图标按钮被推到最右 */
  flex: 1;
  min-width: 0;
}
/* 搜索框容器：flex:1 让输入框尽可能宽（"尽量大"的诉求在此落地），
   min-width:0 允许其在极窄列宽下正常收缩而不撑破工具条 */
.tb-search {
  position: relative;
  display: flex;
  align-items: center;
  flex: 1;
  min-width: 0;
}
.tb-search input {
  width: 100%;
  height: 28px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-2) 0 28px;
  font-size: var(--fs-sm);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: border-color 0.15s, background 0.15s;
}
.tb-search input:focus { border-color: var(--primary); background: var(--cap-feed); }
.tb-search input::placeholder { color: var(--text-tertiary); }
.tb-search .s-icon {
  position: absolute;
  left: 9px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-tertiary);
  pointer-events: none;
}
/* 搜索清空按钮：搜索激活时显示在输入框右侧，点击复位搜索 */
.tb-search-clear {
  position: absolute;
  right: 6px;
  top: 50%;
  transform: translateY(-50%);
  border: none;
  background: none;
  cursor: pointer;
  color: var(--text-tertiary);
  font-size: var(--fs-lg);
  line-height: 1;
  padding: 0 var(--sp-1);
}
.tb-search-clear:hover { color: var(--text-primary); }
/* 操作条图标按钮：仿 Folo 的轻量图标按钮 */
.tb-icon {
  width: 30px;
  height: 28px;
  border-radius: var(--r-md);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: var(--text-tertiary);
  transition: all 0.15s;
  border: none;
  background: none;
  flex-shrink: 0;
}
.tb-icon:hover { background: var(--fill); color: var(--text-primary); }
.tb-icon.active { background: var(--primary-fg); color: var(--primary); }
.tb-icon:disabled { opacity: 0.5; cursor: default; }
.tb-icon svg { width: 15px; height: 15px; }
/* 标签范围指示条：表明"列表正在按标签过滤"，点击即退出。
   色彩取该标签自身的颜色（内联 --tag-c），与列表项里的 chip 呼应 */
.tb-tag {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
  height: 24px;
  max-width: 150px;
  padding: 0 var(--sp-15);
  border-radius: var(--r-pill);
  border: 1px solid var(--border);
  background: var(--fill);
  color: var(--text-secondary);
  font-size: var(--fs-xs);
  cursor: pointer;
  flex-shrink: 0;
}
.tb-tag:hover { border-color: var(--border); color: var(--text-primary); background: var(--fill-secondary); }
.tb-tag-dot {
  width: 7px;
  height: 7px;
  border-radius: var(--r-pill);
  background: var(--tag-c, var(--tone-slate));
  flex-shrink: 0;
}
.tb-tag-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tb-tag-x { color: var(--text-tertiary); font-size: 13px; line-height: 1; }
/* 刷新中图标旋转动画（与 feedsStore.refreshing 联动） */
.tb-icon svg.spinning { animation: spin 0.8s linear infinite; }
@keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
</style>
