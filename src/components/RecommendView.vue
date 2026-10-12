<script setup lang="ts">
/**
 * 智能推荐流（主屏内容模式之一）
 *
 * # 职责
 * 把"以关注实体为中心的高分文章"聚合成一屏可读的卡片流，作为继「文章阅读」「实体关系图」
 * 之后的第三种主屏视图。入口：订阅栏「···更多」菜单的「智能推荐」项、全局 `T` 键
 * （均走 `uiStore.toggleRecommendView`，与 `D` 键切换关系图对称）。
 *
 * # 数据来源
 * 直接复用后端 `high_score_articles(days, threshold)` 命令（与智能摘要同源），按实体分组返回
 * `EntityArticleGroup[]`：每个分组是一个关注实体 + 命中它的高分文章。前端只做渲染与 like/skip
 * 重排，不重新计算评分。
 *
 * # like / skip 语义
 * 每张卡片的 👍/👎 走既有 `articlesStore.setPreference`（底层 `set_user_preference`），
 * skip 即时从本地列表移除（实时重排），like 仅高亮；再点一次取消回 null。跨会话的加权排序
 * 留作后续 `recommend_articles` 后端命令，本轮不做。
 *
 * # 与 displayArticles 兜底纪律的关系
 * 本视图是独立的 `mainView`，数据自 `invoke` 取，不依赖文章视图的 `displayArticles` 兜底，互不影响。
 */
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useArticlesStore } from '@/stores/articles'
import { useEntitiesStore } from '@/stores/entities'
import type { Article, EntityArticleGroup } from '@/types'
import { entityTypeLabel } from '@/utils/entityLabels'

const articlesStore = useArticlesStore()
const entitiesStore = useEntitiesStore()

/** 实体分组数据（评分已在后端降序，前端再按组内分数兜底排序） */
const groups = ref<EntityArticleGroup[]>([])
/** 加载状态 */
const loading = ref(false)
/** 错误信息（加载失败时展示） */
const error = ref<string | null>(null)
/** 回溯天数 / 评分阈值（与智能摘要保持一致） */
const DAYS = 7
const THRESHOLD = 20

/**
 * 加载按实体分组的高分文章
 *
 * @returns 无返回值；结果写入 `groups`，失败写入 `error`
 */
async function loadHighScore(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    const raw = await invoke<EntityArticleGroup[]>('high_score_articles', {
      days: DAYS,
      threshold: THRESHOLD
    })
    // 组内再按分数兜底降序（后端已排，这里防御性重排），保证 skip 后剩余项顺序稳定
    for (const g of raw) {
      g.articles = [...g.articles].sort((a, b) => (b.entity_score ?? 0) - (a.entity_score ?? 0))
    }
    groups.value = raw
  } catch (e) {
    console.error('加载高分文章失败:', e)
    error.value = String(e)
    groups.value = []
  } finally {
    loading.value = false
  }
}

/**
 * 应用 like / skip 偏好并即时重排
 *
 * @param article - 目标文章
 * @param pref - 目标偏好：'like' | 'skip' | null（null = 取消标记）
 * @returns 无返回值；副作用为回写后端偏好、更新本地标记、skip 时从列表移除该卡片
 */
async function applyPreference(article: Article, pref: 'like' | 'skip' | null): Promise<void> {
  // 已处于该偏好则视为取消（再次点击切换回 null），否则置为新偏好
  const next = article.user_preference === pref ? null : pref
  try {
    await articlesStore.setPreference(article.id, next)
    article.user_preference = next
    // skip 后从本地分组移除该卡片，实现"实时重排"；取消 skip 时数据已不在本地，
    // 需重新拉取才能恢复——这里直接触发整表刷新以保持简单一致。
    if (next === 'skip') {
      removeArticle(article.id)
    } else if (article.user_preference === null && pref === 'skip') {
      // 取消 skip：重新加载恢复候选
      void loadHighScore()
    }
  } catch (e) {
    console.error('设置偏好失败:', e)
  }
}

/**
 * 从本地分组中移除指定文章（skip 时调用）
 *
 * @param id - 文章 ID
 */
function removeArticle(id: number): void {
  for (const g of groups.value) {
    const idx = g.articles.findIndex((a) => a.id === id)
    if (idx !== -1) {
      g.articles.splice(idx, 1)
      break
    }
  }
  // 空分组直接剔除，避免留下无文章的实体标题
  groups.value = groups.value.filter((g) => g.articles.length > 0)
}

/** 整个推荐流是否为空（无分组） */
const isEmpty = (): boolean => !loading.value && groups.value.length === 0

/**
 * 格式化发布时间为短日期
 *
 * @param iso - ISO 时间字符串
 * @returns 形如 "10-09" 的短日期；解析失败回退原串
 */
function formatDate(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return `${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

onMounted(async () => {
  // 进入推荐流先补评分：评分命令此前只由摘要弹窗/研究页手动触发，
  // 推荐流若直接查 entity_score 会永远为空。这里先确保评分已跑
  // （batch_score_entities 仅更新 NULL/0，幂等，可重复进入）。
  try {
    await entitiesStore.scoreArticles()
  } catch (e) {
    console.error('推荐流预评分失败:', e)
  }
  await loadHighScore()
})
</script>

<template>
  <section class="recommend-view">
    <header class="rv-head">
      <div>
        <h2 class="rv-title">智能推荐</h2>
        <p class="rv-sub">基于关注实体的相关文章 · 近 {{ DAYS }} 天 · 命中即纳入（评分 ≥ {{ THRESHOLD }}）</p>
      </div>
      <button class="rv-refresh" :disabled="loading" @click="loadHighScore">
        {{ loading ? '加载中…' : '刷新' }}
      </button>
    </header>

    <div v-if="loading" class="rv-state">正在为你聚合关注实体的高分文章…</div>

    <div v-else-if="error" class="rv-state rv-error">加载失败：{{ error }}</div>

    <div v-else-if="isEmpty()" class="rv-state">
      近 {{ DAYS }} 天没有命中你关注实体的文章。去关注更多实体，或调长回溯天数，推荐就会出现在这里。
    </div>

    <div v-else class="rv-groups">
      <section v-for="g in groups" :key="g.entity" class="rv-group">
        <div class="rv-group-head">
          <span class="rv-entity" :title="entityTypeLabel(g.entity_type)">{{ g.entity }}</span>
          <span class="rv-type">{{ entityTypeLabel(g.entity_type) }}</span>
          <span class="rv-count">{{ g.articles.length }} 篇</span>
        </div>

        <ul class="rv-cards">
          <li v-for="a in g.articles" :key="a.id" class="rv-card">
            <div class="rv-card-main">
              <a class="rv-card-title" :href="a.link" target="_blank" rel="noopener">{{ a.title }}</a>
              <div class="rv-card-meta">
                <span v-if="a.entity_score" class="rv-score" :class="{ hot: a.entity_score >= 80 }">
                  {{ a.entity_score }}分
                </span>
                <span class="rv-date">{{ formatDate(a.published_at) }}</span>
                <span v-for="t in a.tags" :key="t.id" class="rv-tag" :style="t.color ? { color: t.color, borderColor: t.color } : undefined">
                  {{ t.name }}
                </span>
              </div>
            </div>
            <div class="rv-card-actions">
              <button
                class="rv-act"
                :class="{ active: a.user_preference === 'like' }"
                title="喜欢"
                @click="applyPreference(a, 'like')"
              >👍</button>
              <button
                class="rv-act"
                :class="{ active: a.user_preference === 'skip' }"
                title="跳过"
                @click="applyPreference(a, 'skip')"
              >👎</button>
            </div>
          </li>
        </ul>
      </section>
    </div>
  </section>
</template>

<style scoped>
.recommend-view {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--surface, #fff);
}
.rv-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px 12px;
  border-bottom: 1px solid var(--border, #eee);
}
.rv-title {
  margin: 0;
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary, #111);
}
.rv-sub {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-tertiary, #888);
}
.rv-refresh {
  border: 1px solid var(--border, #ddd);
  background: var(--surface, #f6f6f6);
  color: var(--text-primary, #111);
  border-radius: 8px;
  padding: 6px 14px;
  font-size: 13px;
  cursor: pointer;
}
.rv-refresh:disabled {
  opacity: 0.6;
  cursor: default;
}
.rv-state {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 40px;
  text-align: center;
  color: var(--text-tertiary, #888);
  font-size: 14px;
  line-height: 1.7;
}
.rv-error {
  color: var(--danger);
}
.rv-groups {
  flex: 1;
  overflow-y: auto;
  padding: 12px 20px 28px;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 16px;
  align-content: start;
}
.rv-group {
  background: var(--surface, #fafafa);
  border: 1px solid var(--border, #eee);
  border-radius: 12px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
}
.rv-group-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-bottom: 10px;
  margin-bottom: 8px;
  border-bottom: 1px dashed var(--border, #eee);
}
.rv-entity {
  font-weight: 700;
  font-size: 15px;
  color: var(--text-primary, #111);
}
.rv-type {
  font-size: 11px;
  color: var(--primary, #3b82f6);
  border: 1px solid var(--primary, #3b82f6);
  border-radius: 999px;
  padding: 1px 8px;
}
.rv-count {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-tertiary, #999);
}
.rv-cards {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.rv-card {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  background: var(--surface, #fff);
  border: 1px solid var(--border, #eee);
  border-radius: 10px;
  padding: 10px 12px;
}
.rv-card-main {
  flex: 1;
  min-width: 0;
}
.rv-card-title {
  display: block;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary, #111);
  text-decoration: none;
  line-height: 1.5;
}
.rv-card-title:hover {
  color: var(--primary, #3b82f6);
}
.rv-card-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  font-size: 12px;
  color: var(--text-tertiary, #999);
}
.rv-score {
  font-weight: 600;
  color: var(--text-tertiary, #999);
}
.rv-score.hot {
  color: var(--tone-amber);
}
.rv-tag {
  font-size: 11px;
  border: 1px solid var(--border, #ddd);
  border-radius: 999px;
  padding: 0 7px;
  color: var(--text-tertiary, #888);
}
.rv-card-actions {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.rv-act {
  border: 1px solid var(--border, #eee);
  background: var(--surface, #fff);
  border-radius: 8px;
  width: 32px;
  height: 28px;
  font-size: 14px;
  cursor: pointer;
  opacity: 0.55;
  transition: all 0.15s ease;
}
.rv-act:hover {
  opacity: 1;
}
.rv-act.active {
  opacity: 1;
  border-color: var(--primary, #3b82f6);
  background: color-mix(in srgb, var(--primary, #3b82f6) 12%, var(--surface, #fff));
}
</style>
