<template>
  <!--
    命令面板：Ctrl/⌘+K 全局唤起的搜索与跳转。

    遮罩与面板是**两级不同的玻璃**——遮罩 blur(10px)+降饱和+压暗，让整窗退到后面；
    面板自身 blur(34px)+提饱和，是画面里最"厚"的一层玻璃，从而成为唯一清晰物。
    两级模糊量必须拉开，否则面板会和背景一样糊，读不出层级。
  -->
  <Teleport to="body">
    <transition name="pal">
      <div v-if="open" class="pal-scrim" @mousedown.self="close">
        <div class="pal" role="dialog" aria-modal="true" aria-label="搜索文章">
          <!-- 输入行：放大镜 + 输入框 + 当前检索模式标签 -->
          <div class="pal-head">
            <svg class="pal-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
              <circle cx="11" cy="11" r="7" /><line x1="20" y1="20" x2="16.2" y2="16.2" />
            </svg>
            <input
              ref="inputRef"
              v-model="keyword"
              type="text"
              :placeholder="placeholder"
              autocomplete="off"
              spellcheck="false"
              @keydown.down.prevent="move(1)"
              @keydown.up.prevent="move(-1)"
              @keydown.enter.prevent="jump"
              @keydown.esc.prevent="close"
            />
            <!-- 检索模式：标题优先，无命中自动退到全文。此标签如实反映当前在搜什么 -->
            <span class="pal-mode" :class="{ full: mode === 'full' }">{{
              mode === 'title' ? '标题' : '全文'
            }}</span>
          </div>

          <!-- 结果区：加载中 / 空态 / 结果列表 三者互斥。
               加载态必须先于空态判断，否则请求在途时会闪出"没有结果"。 -->
          <div class="pal-body">
            <div v-if="loading" class="pal-loading">
              <span v-for="i in 3" :key="i" class="pal-skel">
                <span class="sk-line"></span>
              </span>
            </div>
            <p v-else-if="results.length === 0" class="pal-empty">
              {{ keyword.trim() ? `没有匹配「${keyword.trim()}」的文章` : '输入关键词开始搜索' }}
            </p>
            <ul v-else class="pal-list">
              <li
                v-for="(a, i) in results"
                :key="a.id"
                class="pal-row"
                :class="{ on: i === index }"
                @mouseenter="index = i"
                @click="jump"
              >
                <span class="pal-av" :style="avStyle(a.feed_id)">
                  {{ feedInitial(a.feed_id) }}
                </span>
                <span class="pal-main">
                  <!-- 标题与摘要均按命中词切分高亮：命中位置是用户判断"是不是这篇"的依据 -->
                  <span class="pal-title" v-html="mark(a.title, keyword.trim())"></span>
                  <span class="pal-snippet" v-html="mark(snippet(a), keyword.trim())"></span>
                </span>
                <span class="pal-side">
                  <span class="pal-feed">{{ feedName(a.feed_id) }}</span>
                  <span class="pal-time">{{ formatDistance(a.published_at) }}</span>
                </span>
              </li>
            </ul>
          </div>

          <!-- 底栏：键盘约定。与其让用户猜，不如写在这里 -->
          <div class="pal-foot">
            <span><kbd>↑</kbd><kbd>↓</kbd>选择</span>
            <span><kbd>↵</kbd>跳转</span>
            <span><kbd>esc</kbd>关闭</span>
            <span class="pal-hint">标题无命中时自动搜全文</span>
          </div>
        </div>
      </div>
    </transition>
  </Teleport>
</template>

<script setup lang="ts">
/**
 * 命令面板组件
 *
 * # 职责
 * `Ctrl/⌘+K` 唤起，用一个输入框完成「找文章 → 跳过去」这条路径：
 * 1. 输入关键词做检索（标题优先，无命中时自动退到全文）；
 * 2. `↑` `↓` 选择、`↵` 跳转、`esc` 关闭；
 * 3. 跳转后写全局 store，让订阅栏与文章列表各自把选中胶囊滑到目标位置。
 *
 * # 为什么复用现有的 `articles_search` 而不加新命令
 * 后端那条命令已经是 FTS5 全文检索（标题 / 正文 / 摘要，含中文短查询的 LIKE 回退），
 * 与面板需求完全重合。**不新增任何 Rust 命令**，也因此无需改动 `lib.rs` 的
 * 命令清单与模块文档。
 *
 * # 标题 / 全文两段式检索
 * 一次性搜全文时，用户输"AI"会得到大量正文里含该词的无关文章——这些标题根本
 * 不含该词，看不出为什么被召回。因此先只搜标题（结果精准、量少），
 * 一个都没有才退到全文（宁多勿缺）。右上角的模式标签如实标注当前处于哪一段。
 *
 * # 跳转后的三步顺序（顺序错一步就定位不到）
 * 1. **切订阅源** —— 不切的话两栏都停在当前范围，目标条目根本不在列表里；
 * 2. **等列表就绪** —— 切源触发 ArticleColumn 经 scopeKey watch 异步重载，
 *    `nextTick` 只覆盖 Vue 渲染、覆盖不到后端往返，故用 rAF 轮询等元素进 DOM；
 * 3. **派发 `command-palette-jump`** —— 必须放在最后：事件早于选中态更新的话，
 *    两栏量到的还是旧位置，胶囊停在原地不动。
 *
 * 定位用事件而非直接跨组件调 ref，是让两栏各自在 nextTick 后处理
 * "列表可能尚未渲染完成"这类时序差异。
 */

import { ref, watch, computed, nextTick, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useArticlesStore } from '@/stores/articles'
import { useFeedsStore } from '@/stores/feeds'
import { formatDistance } from '@/utils/format'
import type { Article } from '@/types'

/** 面板是否打开 */
const open = ref(false)
/** 输入框内容（v-model） */
const keyword = ref('')
/** 检索结果 */
const results = ref<Article[]>([])
/** 当前高亮项下标 */
const index = ref(0)
/** 是否正在检索：用于区分"没有结果"与"还没查完" */
const loading = ref(false)
/** 当前处于标题检索还是全文检索（决定右上角标签） */
const mode = ref<'title' | 'full'>('title')
/** 输入框 DOM，唤起时聚焦 */
const inputRef = ref<HTMLInputElement | null>(null)

const articlesStore = useArticlesStore()
const feedsStore = useFeedsStore()

/** 无输入时的占位文案：把两种检索都说清楚，用户不必猜 */
const placeholder = computed(() => '搜索标题与全文…')

/**
 * 检索结果上限
 *
 * 面板可视区最多显示 6~7 行，给到 30 条足够翻找；再多的结果在面板里也翻不到，
 * 反而拖慢后端返回。
 */
const LIMIT = 30

/**
 * 在途请求的序号
 *
 * 用户连续击键会连发多次请求，后返回的未必是最后发的那次。用自增序号标记
 * 请求发起顺序，回来时若序号已过期就丢弃结果——否则会出现"打完最后一个字，
 * 却被上一次输入的结果覆盖"。
 */
let reqSeq = 0

/** 组件内在途定时器句柄（输入防抖用），卸载时统一清理 */
let debounceTimer: ReturnType<typeof setTimeout> | undefined

/**
 * 打开命令面板
 *
 * 由全局 `Ctrl/⌘+K` 触发。打开时清空上一次的关键词与结果——
 * 面板是"一次性"的，重新唤起应当是干净起点。
 *
 * @returns 无返回值；副作用为置位 open、聚焦输入框
 */
async function show() {
  keyword.value = ''
  results.value = []
  index.value = 0
  mode.value = 'title'
  open.value = true
  await nextTick()
  inputRef.value?.focus()
}

/** 关闭命令面板 */
function close() {
  open.value = false
}

/**
 * 执行检索
 *
 * 两段式：先按标题子串过滤本地列表（订阅源文章的标题较短，前端过滤足够快、
 * 无需往返）；标题无命中才走后端 `articles_search` 做全文检索。
 *
 * @param q - 已 trim 的关键词
 * @returns 无返回值；结果写入 results / mode / index
 */
async function runSearch(q: string) {
  if (!q) {
    results.value = []
    mode.value = 'title'
    return
  }
  const seq = ++reqSeq
  const lower = q.toLowerCase()

  // 第一段：标题命中。直接在已加载的列表里找——不 invoke 后端，输入零延迟
  const titleHits = articlesStore.displayArticles.filter((a) =>
    a.title.toLowerCase().includes(lower),
  )
  if (titleHits.length > 0) {
    results.value = titleHits.slice(0, LIMIT)
    mode.value = 'title'
    index.value = 0
    return
  }

  // 第二段：标题无命中才搜全文（FTS5，含正文与摘要）
  loading.value = true
  try {
    const hits = await invoke<Article[]>('articles_search', { query: q, limit: LIMIT })
    // 序号已过期说明用户又改了输入，丢弃这次结果
    if (seq !== reqSeq) return
    results.value = hits
    mode.value = 'full'
    index.value = 0
  } catch (e) {
    if (seq !== reqSeq) return
    // 检索失败降级为"无结果"而非弹错误：命令面板是高频入口，
    // 一次失败不该用模态框打断
    console.error('命令面板检索失败:', e)
    results.value = []
  } finally {
    if (seq === reqSeq) loading.value = false
  }
}

/** 关键词变化 → 防抖后检索。150ms 足够消掉连续击键，又不会有明显迟滞 */
watch(keyword, (v) => {
  if (debounceTimer) clearTimeout(debounceTimer)
  const q = v.trim()
  if (!q) {
    // 清空时作废在途请求，避免旧结果在空输入下回填
    reqSeq++
    loading.value = false
    results.value = []
    return
  }
  debounceTimer = setTimeout(() => void runSearch(q), 150)
})

/**
 * 上下移动高亮项
 *
 * @param d - 位移方向：1 下一项，-1 上一项
 * @returns 无返回值
 */
function move(d: number) {
  if (results.value.length === 0) return
  index.value = (index.value + d + results.value.length) % results.value.length
}

/**
 * 取某个订阅源的显示名
 *
 * @param feedId - 订阅源 ID
 * @returns 源名；查不到时退回 `Feed {id}`，保证不出现空白来源
 */
function feedName(feedId: number): string {
  return feedsStore.feeds.find((f) => f.id === feedId)?.name ?? `Feed ${feedId}`
}

/**
 * 取某个订阅源的首字母（结果行左侧的小方块）
 *
 * @param feedId - 订阅源 ID
 * @returns 名称首字符；源名为空时返回 '?'
 */
function feedInitial(feedId: number): string {
  const n = feedsStore.feeds.find((f) => f.id === feedId)?.name
  return n ? n.charAt(0) : '?'
}

/**
 * 结果行左侧小方块的内联样式
 *
 * 复用订阅源自己的 icon：有图标就显示图标（与左侧列表一致），
 * 没有则退回首字母底色。不写死颜色——与 .feed-item-avatar 用同一套观感。
 *
 * @param feedId - 订阅源 ID
 * @returns CSSProperties；无图标时给出渐变底与内描边
 */
function avStyle(feedId: number) {
  const icon = feedsStore.feeds.find((f) => f.id === feedId)?.icon
  if (icon) return { backgroundImage: `url("${icon}")`, backgroundSize: 'cover' }
  return {}
}

/** HTML 转义：命中的关键词会被插成 <mark>，正文片段必须先转义再拼 */
const ESCAPES: Record<string, string> = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
}
function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ESCAPES[c] ?? c)
}

/**
 * 在文本中标出命中片段
 *
 * 先把整段文本转义，再按原串的命中位置切插 `<mark>`——顺序不能反：
 * 先插标签再转义会把标签本身也转义掉。
 *
 * @param text - 原始文本
 * @param q - 关键词（已 trim，空串表示不高亮）
 * @returns 带 `<mark>` 的安全 HTML 字符串
 */
function mark(text: string, q: string): string {
  const safe = esc(text)
  if (!q) return safe
  const i = text.toLowerCase().indexOf(q.toLowerCase())
  if (i < 0) return safe
  return esc(text.slice(0, i)) + '<mark>' + esc(text.slice(i, i + q.length)) + '</mark>' + esc(text.slice(i + q.length))
}

/**
 * 取结果行的摘要片段
 *
 * 优先取**命中位置前后**的一小段：用户判断"是不是这篇"靠的就是命中上下文，
 * 截开头很可能什么都没命中，等于没给信息。取不到（标题阶段）则用 summary 前段。
 *
 * @param a - 文章
 * @returns 摘要片段字符串
 */
function snippet(a: Article): string {
  const src = a.summary || a.content || ''
  const plain = src.replace(/<[^>]*>/g, ' ').replace(/\s+/g, ' ').trim()
  const q = keyword.value.trim()
  const i = q ? plain.toLowerCase().indexOf(q.toLowerCase()) : -1
  if (i < 0) return plain.slice(0, 70)
  return (i > 16 ? '…' : '') + plain.slice(Math.max(0, i - 16), i + 54)
}

/**
 * 轮询等待某个条目出现在 DOM 中
 *
 * 切源 / 换范围会触发列表异步重载（后端 invoke），而 `nextTick` 只等一次
 * Vue 渲染，无法覆盖后端往返。定位胶囊需要**元素真的在 DOM 里**才能量到
 * 几何，故这里以 rAF 轮询直到出现或超时。
 *
 * 超时是正常路径而非异常：命令面板能跳到未加载的文章（正文照样渲染），
 * 此时定位本就不成立，不该阻塞跳转、更不该报错。
 *
 * @param attr - 目标元素上的数据属性值，形如 `data-article-id="12"`
 * @param itemClass - 条目类名（订阅源 `.feed-item` / 文章 `.article-item`）
 * @param timeoutMs - 最长等待；超时后静默返回
 * @returns 无返回值
 */
async function waitForItem(attr: string, itemClass: string, timeoutMs = 1200): Promise<void> {
  const deadline = performance.now() + timeoutMs
  while (performance.now() < deadline) {
    if (document.querySelector(`${itemClass}${attr}`)) return
    await new Promise<void>((r) => requestAnimationFrame(() => r()))
  }
}

/**
 * 跳转到当前高亮项
 *
 * 写 `selectedArticle` 触发正文区渲染；再派发 `command-palette-jump`
 * 让两栏的胶囊各自播一次定位脉冲。派发而非直接调 ref，是因为两栏可能
 * 尚未渲染出目标条目（首次展开某分组等），由它们在 nextTick 后自行处理更稳。
 *
 * @returns 无返回值；副作用为写 store、关闭面板、派发定位事件
 */
async function jump() {
  const a = results.value[index.value]
  if (!a) return
  close()

  // ① 先切到该文所属的订阅源 —— 不切的话，两栏都停在当前范围，
  //    目标条目根本不在列表里，后续定位自然无从谈起。
  const target = feedsStore.feeds.find((f) => f.id === a.feed_id)
  if (target && feedsStore.selectedFeed?.id !== a.feed_id) {
    feedsStore.selectFeed(target)
    // 切源会经ArticleColumn 的 scopeKey watch 触发列表重载（异步）。
    // 这里既不重复调 loadArticles（会多打一次后端），也不能只等一个
    // nextTick —— 那时列表还没回来。故轮询等目标条目真正出现在 DOM 里，
    // 超时则放弃定位（正文区已渲染，定位只是锦上添花，不该阻塞跳转）。
    await waitForItem(`[data-feed-id="${a.feed_id}"]`, `.feed-item`)
    await waitForItem(`[data-article-id="${a.id}"]`, `.article-item`)
  }

  // ② 再写入选中态。乐观更新已让它同步生效，故此刻 active 类已就位。
  await articlesStore.selectArticle(a.id)

  // ③ 最后派发定位事件：两栏各自把胶囊滑到目标并播一次脉冲。
  //    放在最后是必须的——事件早于选中态更新的话，宿主量到的还是旧位置。
  window.dispatchEvent(
    new CustomEvent('command-palette-jump', { detail: { feedId: a.feed_id, articleId: a.id } }),
  )
}

/**
 * 全局快捷键：Ctrl/⌘ + K
 *
 * 用 `e.key.toLowerCase()` 兼容不同键盘布局（有些布局下 K 的 key 需配合修饰键才可打出）。
 * 与 Tauri 的全局快捷键无关——那是系统级注册，而面板仅在窗口内可用，
 * 走 DOM 监听更简单，也避免多占一个系统快捷位。
 *
 * @param e - 键盘事件
 * @returns 无返回值
 */
function onKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    open.value ? close() : void show()
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  if (debounceTimer) clearTimeout(debounceTimer)
})
</script>

<style scoped>
/* ── 遮罩：压暗 + 降饱和 + 轻模糊，让整窗退到面板之后 ────────────── */
.pal-scrim {
  position: fixed;
  inset: 0;
  z-index: 900;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 12vh;              /* 面板落在视觉中线偏上，符合命令面板惯例 */
  /* 用 --overlay-scrim（0.4 黑）而非 --scrim-heavy（0.82）：
     后者是给**全屏灯箱**（图片预览）设计的，那种场景画面只有一个主体、
     必须彻底压暗环境。而玻璃面板自身就是视觉主角，若底下压 0.82 黑，
     面板的白色玻璃（--glass-strong 74% 白）叠上去也只有约 #e4e4e4 的灰，
     深色标题压在上面虽仍可读，但整块玻璃失去"亮"的质感、看起来像脏灰塑料。 */
  background: var(--overlay-scrim);
  backdrop-filter: blur(8px) saturate(0.95);
  -webkit-backdrop-filter: blur(8px) saturate(0.95);
}

/* ── 面板：比遮罩更模糊、更饱和 = 画面里最"厚"的一层玻璃 ────────── */
.pal {
  width: 620px;
  max-width: calc(100vw - 48px);
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: var(--r-panel);
  background: var(--glass-strong);
  backdrop-filter: blur(34px) saturate(1.8);
  /* 命令面板是浮在最上层、承载全局搜索的聚焦层，玻璃要比普通弹窗更实一档 */
  -webkit-backdrop-filter: blur(34px) saturate(1.8);
  border: 1px solid var(--glass-hi);
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    var(--shadow-pop);
}

/* ── 输入行 ───────────────────────────────────────────── */
.pal-head {
  display: flex;
  align-items: center;
  gap: var(--sp-15);
  padding: var(--sp-4) var(--sp-5);
  border-bottom: 1px solid var(--glass-hair);
  flex-shrink: 0;
}
.pal-ico { width: 18px; height: 18px; color: var(--text-tertiary); flex-shrink: 0; }
.pal-head input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  font-family: inherit;
  font-size: var(--fs-lg);
  color: var(--text-primary);
}
.pal-head input::placeholder { color: var(--text-tertiary); }
/* 检索模式标签：标题段用中性色，全文段用琥珀（与文章列表的胶囊描边同源） */
.pal-mode {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  font-weight: 600;
  padding: 2px var(--sp-2);
  border-radius: var(--r-pill);
  background: var(--primary-soft);
  color: var(--primary);
}
.pal-mode.full { background: var(--cap-accent-bg); color: var(--cap-accent-line); }

/* ── 结果区 ───────────────────────────────────────────── */
.pal-body { flex: 1; min-height: 0; overflow-y: auto; padding: var(--sp-2); }
.pal-body::-webkit-scrollbar { width: var(--sb-w); }

.pal-loading { display: flex; flex-direction: column; gap: var(--sp-3); padding: var(--sp-2); }
.pal-skel { display: block; }
.pal-skel .sk-line { height: 12px; }
.pal-skel:nth-child(2) .sk-line { width: 78%; }
.pal-skel:nth-child(3) .sk-line { width: 90%; }

.pal-empty {
  padding: var(--sp-10) var(--sp-4);
  text-align: center;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
}

.pal-list { list-style: none; }
/* 高亮项：比 .pal-row 再实一档的白，让"当前选中"在玻璃上有明确落点 */
.pal-row {
  display: flex;
  align-items: flex-start;
  gap: var(--sp-3);
  padding: var(--sp-15) var(--sp-3);
  border-radius: var(--r-md);
  cursor: pointer;
  transition: background var(--dur) var(--ease);
}
/* 两态都用**半透明**色，不要用 --surface（不透明纯白）：
   面板底已是 74% 白玻璃，再叠不透明白块会把它"挖穿"一块，
   高亮行反而比周围更亮、显得脏，且下方文字对比度骤降。 */
.pal-row:hover { background: var(--pal-row-hover); }
.pal-row.on {
  /* 用--cap-accent-bg 而非 --cap-solid-bg：后者在浅色主题下是**实色琥珀**
     （--brand-from），面板里的深色标题压上去会读不清。命令面板是浮层、
     不需要"实体色块"那种强锚定，描边 + 淡底已足够表达选中。 */
  background: var(--cap-accent-bg);
  box-shadow:
    inset 0 0 0 1px var(--cap-accent-line),
    inset 0 1px 0 var(--glass-hi);
}

.pal-av {
  width: 26px; height: 26px; flex-shrink: 0;
  border-radius: var(--r-sm);
  display: flex; align-items: center; justify-content: center;
  font-size: var(--fs-sm); font-weight: 600;
  color: var(--text-secondary);
  background: var(--fill-secondary);
  box-shadow: var(--ring-inset);
  background-size: cover;          /* 有 icon 时由内联 backgroundImage 铺满 */
  background-position: center;
}
.pal-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.pal-title {
  font-size: var(--fs-md);
  line-height: 1.45;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pal-snippet {
  font-size: var(--fs-xs);
  line-height: 1.5;
  /* 用 secondary 而非 tertiary：面板底是浅色玻璃，tertiary 在上面余光一扫
     几乎读不出来（不是对比度不达标，而是字太小、颜色太淡）。 */
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 命中高亮：与品牌琥珀同源，是画面里仅有的两处饱和色之一 */
.pal-snippet :deep(mark), .pal-title :deep(mark) {
  background: var(--cap-accent-bg);
  color: var(--primary);
  border-radius: 2px;
  box-shadow: inset 0 -1px 0 var(--cap-accent-line);
  padding: 0 1px;
}
.pal-side {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  padding-top: 2px;
}
.pal-feed { color: var(--primary); white-space: nowrap; }
.pal-time { font-variant-numeric: tabular-nums; white-space: nowrap; }

/* ── 底栏 ─────────────────────────────────────────────── */
.pal-foot {
  display: flex;
  align-items: center;
  gap: var(--sp-4);
  padding: var(--sp-15) var(--sp-5);
  border-top: 1px solid var(--glass-hair);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
}
.pal-foot kbd {
  font-family: inherit;
  font-size: var(--fs-xs);
  padding: 1px 5px;
  margin-right: 3px;
  border-radius: var(--r-sm);
  background: var(--fill);
  box-shadow: var(--ring-inset);
}
.pal-hint { margin-left: auto; opacity: 0.85; }

/* ── 进出过渡：面板比遮罩晚一点到位，读起来像"从背景里浮起" ────────── */
.pal-enter-active .pal, .pal-leave-active .pal { transition: transform var(--dur-slide) var(--ease-slide), opacity var(--dur) var(--ease); }
.pal-enter-active, .pal-leave-active { transition: opacity var(--dur) var(--ease); }
.pal-enter-from, .pal-leave-to { opacity: 0; }
.pal-enter-from .pal, .pal-leave-to .pal { transform: translateY(-10px) scale(0.985); opacity: 0; }

/* 无障碍：关掉动效后仍是"遮罩变深 + 面板出现"，层级表达不受影响 */
@media (prefers-reduced-motion: reduce) {
  .pal-enter-active .pal, .pal-leave-active .pal,
  .pal-enter-active, .pal-leave-active { transition: none; }
}
</style>