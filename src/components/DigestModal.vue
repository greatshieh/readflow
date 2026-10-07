<template>
  <!-- 每日摘要弹窗：展示高分文章按实体维度的分组概览 -->
  <BaseModal :open="open" title="智能摘要" size="lg" @close="close">
    <!-- 头部 -->
    <div class="m-head">
      <h3>智能摘要</h3>
      <button class="m-close" @click="close" aria-label="关闭">×</button>
    </div>

    <!-- 主体内容 -->
    <div class="m-body">
      <!-- 加载中 -->
      <div v-if="loading" class="loading-state">
        <p>正在加载高分文章...</p>
      </div>

      <!-- 无数据 -->
      <div v-else-if="groups.length === 0" class="empty-state">
        <p>暂无高分文章</p>
        <p class="hint">添加关注实体并触发评分后，此处将显示与你关注对象最相关的文章</p>
      </div>

      <!-- 文章列表（按实体分组，分组由后端按真实命中给出） -->
      <div v-else class="article-list">
        <div
          v-for="group in groups"
          :key="group.entity"
          class="entity-group"
        >
          <div class="group-header">
            <span class="entity-tag">{{ group.entity }}</span>
            <span class="entity-count">{{ group.articles.length }} 篇</span>
          </div>
          <div class="group-articles">
            <div
              v-for="article in group.articles"
              :key="article.id"
              class="digest-article"
              @click="handleSelectArticle(article)"
            >
              <div class="article-header">
                <span class="article-score" :class="scoreClass(article.entity_score)">
                  {{ article.entity_score }}分
                </span>
                <span class="article-source">{{ getFeedName(article.feed_id) }}</span>
              </div>
              <div class="article-title">{{ article.title }}</div>
              <div class="article-summary">{{ article.summary }}</div>
              <div class="article-time">{{ formatTime(article.published_at) }}</div>
            </div>
          </div>
        </div>
      </div>

      <!-- 操作栏 -->
      <div class="actions">
        <button class="refresh-btn" @click="loadHighScore" :disabled="loading">
          {{ loading ? '加载中...' : '刷新摘要' }}
        </button>
        <button
          class="score-all-btn"
          @click="handleScoreAll"
          :disabled="entitiesStore.entities.length === 0 || loading"
        >
          全库评分
        </button>
      </div>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 智能摘要弹窗组件
 *
 * # 职责
 * 展示「关注实体 → 高分文章」的分组概览，帮助用户快速发现与关注对象相关的重点内容。
 * 点击文章即跳转到阅读模式（由父组件接管选中态）。
 *
 * # 数据来源
 * 后端 `high_score_articles`：分组（哪篇文章命中了哪个实体）在 Rust 侧按与
 * `entity_score` 完全相同的规则算好，前端不再自行推断命中关系。
 *
 * # 父组件通信
 * - `open` prop：控制弹窗显示状态
 * - `@close` 事件：通知父组件关闭弹窗
 * - `@selectArticle` 事件：把被点击的文章交给父组件去选中
 */

import { ref, watch } from 'vue'
import { useEntitiesStore } from '@/stores/entities'
import { useFeedsStore } from '@/stores/feeds'
import { invoke } from '@tauri-apps/api/core'
import BaseModal from './BaseModal.vue'
import type { Article, EntityArticleGroup } from '@/types'

/**
 * props 必须接住返回值：`<script setup>` 只会把 props 暴露给模板，
 * 脚本里（下面的 watch）要引用 `open` 就必须通过 `props.open`，
 * 否则运行时会以 "open is not defined" 直接抛错。
 */
const props = defineProps<{
  /** 弹窗是否打开 */
  open: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'selectArticle', article: Article): void
}>()

const entitiesStore = useEntitiesStore()
const feedsStore = useFeedsStore()

/** 按实体分组的高分文章（后端返回，直接渲染） */
const groups = ref<EntityArticleGroup[]>([])
/** 加载状态 */
const loading = ref(false)

/**
 * 获取订阅源名称
 *
 * @param feedId - 订阅源 ID
 * @returns 订阅源名称，不存在时返回 '未知源'
 */
function getFeedName(feedId: number): string {
  const feed = feedsStore.feeds.find(f => f.id === feedId)
  return feed?.name || '未知源'
}

/**
 * 格式化时间显示
 *
 * @param time - ISO 时间字符串
 * @returns 格式化后的时间字符串（如 "3小时前"）
 */
function formatTime(time: string): string {
  const date = new Date(time)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffHours = Math.floor(diffMs / (1000 * 60 * 60))
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))

  if (diffHours < 1) return '刚刚'
  if (diffHours < 24) return `${diffHours}小时前`
  if (diffDays < 7) return `${diffDays}天前`
  return date.toLocaleDateString('zh-CN')
}

/**
 * 根据分数返回对应的样式类名
 *
 * @param score - 文章评分（0-100）
 * @returns CSS 类名
 */
function scoreClass(score: number): string {
  if (score >= 80) return 'score-high'
  if (score >= 60) return 'score-medium'
  return 'score-low'
}

/**
 * 加载按实体分组的高分文章
 *
 * @returns 无返回值；结果写入 `groups`
 */
async function loadHighScore() {
  loading.value = true
  try {
    groups.value = await invoke<EntityArticleGroup[]>('high_score_articles', {
      days: 7,
      threshold: 60
    })
  } catch (e) {
    console.error('加载高分文章失败:', e)
    groups.value = []
  } finally {
    loading.value = false
  }
}

/**
 * 选中文章并跳转至阅读模式
 *
 * @param article - 目标文章
 * @returns 无返回值；父组件负责切换选中态，本组件随后关闭
 */
function handleSelectArticle(article: Article) {
  emit('selectArticle', article)
  emit('close')
}

/** 关闭弹窗（模板里的遮罩点击与右上角关闭按钮共用） */
function close() {
  emit('close')
}

/**
 * 触发全库文章评分
 *
 * @returns 无返回值；评分完成后重新加载分组
 */
async function handleScoreAll() {
  try {
    await entitiesStore.scoreArticles()
    // 评分完成后重新加载高分文章
    await loadHighScore()
  } catch (e) {
    console.error('评分失败:', e)
  }
}

/** 监听 open 变化，打开时加载数据 */
watch(() => props.open, async (val) => {
  if (!val) return
  // 实体列表可能尚未加载（用户没进过研究工作台），而"全库评分"按钮要在
  // 实体为空时禁用，因此打开时顺手补一次
  if (entitiesStore.entities.length === 0) {
    await entitiesStore.loadEntities().catch(() => {})
  }
  await loadHighScore()
})
</script>

<style scoped>
/* 头部 */
.m-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-4) var(--sp-5);
  border-bottom: 1px solid var(--border);
}
.m-head h3 {
  margin: 0;
  font-size: var(--fs-lg);
  font-weight: 600;
  color: var(--text-primary);
}
.m-close {
  border: none;
  background: none;
  cursor: pointer;
  font-size: 22px;
  line-height: 1;
  color: var(--text-tertiary);
  padding: 0 var(--sp-1);
}
.m-close:hover { color: var(--text-primary); }

/* 主体 */
.m-body {
  padding: var(--sp-4) var(--sp-5);
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
  overflow-y: auto;
  flex: 1;
}

/* 加载/空状态 */
.loading-state,
.empty-state {
  text-align: center;
  padding: var(--sp-10) 0;
  color: var(--text-tertiary);
}
.loading-state p,
.empty-state p {
  margin: 0 0 var(--sp-2);
  font-size: var(--fs-base);
}
.empty-state .hint {
  font-size: var(--fs-sm);
}

/* 文章列表 */
.article-list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-5);
}

/* 实体分组 */
.entity-group {
  display: flex;
  flex-direction: column;
  gap: var(--sp-15);
}
.group-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-bottom: var(--sp-2);
  border-bottom: 1px solid var(--border);
}
.entity-tag {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--primary);
  background: var(--primary-fg);
  padding: var(--sp-1) var(--sp-15);
  border-radius: var(--r-sm);
}
.entity-count {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}

/* 文章条目 */
.group-articles {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.digest-article {
  padding: var(--sp-3);
  background: var(--fill);
  border-radius: var(--r-md);
  cursor: pointer;
  transition: background 0.15s;
}
.digest-article:hover {
  background: var(--hover-bg);
}
.article-header {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  margin-bottom: var(--sp-05);
}
.article-score {
  font-size: var(--fs-sm);
  font-weight: 600;
  padding: var(--sp-025) var(--sp-2);
  border-radius: var(--r-sm);
}
.score-high {
  color: var(--tone-fg);
  background: var(--danger);
}
.score-medium {
  color: var(--tone-fg);
  background: var(--warning);
}
.score-low {
  color: var(--text-secondary);
  background: var(--border);
}
.article-source {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.article-title {
  font-size: var(--fs-base);
  font-weight: 500;
  color: var(--text-primary);
  line-height: 1.4;
  margin-bottom: var(--sp-1);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.article-summary {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  margin-bottom: var(--sp-05);
}
.article-time {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

/* 操作栏 */
.actions {
  display: flex;
  gap: var(--sp-15);
  padding-top: var(--sp-3);
  border-top: 1px solid var(--border);
}
.refresh-btn,
.score-all-btn {
  flex: 1;
  height: 36px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
  color: var(--text-secondary);
  font-size: var(--fs-md);
  cursor: pointer;
  transition: all 0.15s;
}
.refresh-btn:hover:not(:disabled),
.score-all-btn:hover:not(:disabled) {
  border-color: var(--primary);
  color: var(--primary);
}
.refresh-btn:disabled,
.score-all-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.score-all-btn {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--tone-fg);
}
.score-all-btn:hover:not(:disabled) {
  filter: brightness(0.95);
}
</style>
