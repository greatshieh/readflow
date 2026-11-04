<template>
  <!--
    命令面板：Ctrl/⌘+K 全局唤起的搜索与跳转。

    遮罩与面板是**两级不同的玻璃**——遮罩 blur(10px)+降饱和+压暗，让整窗退到后面；
    面板自身 blur(16px)+提饱和，是画面里最"厚"的一层玻璃，从而成为唯一清晰物。
    两级模糊量必须拉开，否则面板会和背景一样糊，读不出层级。
  -->
  <Teleport to="body">
    <transition name="pal">
      <div v-if="open" class="pal-scrim" @mousedown.self="close">
        <div
          class="pal"
          role="dialog"
          aria-modal="true"
          aria-label="搜索文章、判断与证据"
          @mousemove="onMouseMove"
        >
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
          <div ref="listRef" class="pal-body">
            <div v-if="loading" class="pal-loading">
              <span v-for="i in 3" :key="i" class="pal-skel">
                <span class="sk-line"></span>
              </span>
            </div>
            <p v-else-if="results.length === 0" class="pal-empty">
              {{ keyword.trim() ? `没有匹配「${keyword.trim()}」的文章或判断` : '输入关键词开始搜索' }}
            </p>
            <ul v-else class="pal-list">
              <li
                v-for="(item, i) in results"
                :key="rowKey(item, i)"
                class="pal-row"
                :class="{ on: i === index }"
                @mouseenter="onRowEnter(i)"
                @click="jump"
              >
                <!-- 文章行：与新增笔记行共处一条 ↑↓ 序列，所以用同一个 <li>，
                     只是内部换形状。分成两个 v-for 会得到两个下标空间。 -->
                <template v-if="item.kind === 'article'">
                  <span class="pal-av" :style="avStyle(item.article.feed_id)">
                    {{ feedInitial(item.article.feed_id) }}
                  </span>
                  <span class="pal-main">
                    <!-- 标题与摘要均按命中词切分高亮：命中位置是用户判断"是不是这篇"的依据 -->
                    <span class="pal-title" v-html="mark(item.article.title, keyword.trim())"></span>
                    <span class="pal-snippet" v-html="mark(snippet(item.article), keyword.trim())"></span>
                  </span>
                  <span class="pal-side">
                    <span class="pal-feed">{{ feedName(item.article.feed_id) }}</span>
                    <span class="pal-time">{{ formatDistance(item.article.published_at) }}</span>
                  </span>
                </template>
                <!-- 笔记行：色块里放类型首字，右侧标类型。不用打字高亮（v-html）
                     是因为这里展示的是用户自己写的句子，逐字切标意义不大，
                     而"挂在哪个判断下"才是他判断该不该点进去的依据。 -->
                <template v-else>
                  <span class="pal-av note" :class="item.note.kind">
                    {{ noteKindLabel(item.note.kind).slice(0, 1) }}
                  </span>
                  <span class="pal-main">
                    <span class="pal-title">{{ item.note.text || item.note.thesisStatement }}</span>
                    <span class="pal-snippet">{{ noteSub(item.note) }}</span>
                  </span>
                  <span class="pal-side">
                    <span class="pal-feed">{{ noteKindLabel(item.note.kind) }}</span>
                  </span>
                </template>
              </li>
            </ul>
          </div>

          <!-- 底栏：键盘约定。与其让用户猜，不如写在这里 -->
          <div class="pal-foot">
            <span><kbd class="kbd">↑</kbd><kbd class="kbd">↓</kbd>选择</span>
            <span><kbd class="kbd">↵</kbd>跳转</span>
            <span><kbd class="kbd">esc</kbd>关闭</span>
            <!-- 可点入口：`?` 键本身没人能猜到，必须有个可见地方把它带出来 -->
            <span class="pal-hint">标题无命中时自动搜全文</span>
            <button class="pal-action" type="button" @click="openResearch">研究工作台</button>
            <button class="pal-action" type="button" @click="openShortcuts">快捷键</button>
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
 * 3. **交给 `navigateToArticle`** —— 切范围与定位都收在那条编排里，事件派发放在最后：事件早于选中态更新的话，
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
import { requestThesisOpen } from '@/utils/thesisJump'
import { navigateToArticle } from '@/utils/navigateToArticle'
import type { Article } from '@/types'

/** 笔记类命中（与 Rust `research::search::NoteHit` 对齐，出参为 camelCase） */
interface NoteHit {
  kind: 'highlight' | 'thesis' | 'evidence'
  refId: number
  /** 归属论点；高亮可能还没挂到任何论点，此时为 null */
  thesisId: number | null
  thesisStatement: string
  text: string
  meta: string
}

/**
 * 结果行：文章与笔记共用同一条 ↑↓ 序列
 *
 * 用带标签的并集而不是两个数组：键盘选择、确保可见、hover 高亮都建立在
 * "一个下标对应一行"上。两个数组就要维护两套下标与两套边界判断，
 * 而"按了一次下键跨过两个列表的缝"这种边界恰恰最容易写错。
 */
type PalItem =
  | { kind: 'article'; article: Article }
  | { kind: 'note'; note: NoteHit }

/** 面板是否打开 */
const open = ref(false)
/** 输入框内容（v-model） */
const keyword = ref('')

/**
 * 上次使用的检索词（仅会话内记忆，不落盘）
 *
 * # 为什么记
 * 面板此前是严格无状态的：唤起即清空，跳转即关闭。代价是"搜一个词 → 跳去看 →
 * 想回头再看一眼其余结果"这条路径必须把关键词重输一遍。
 *
 * # 为什么只存词、不存结果
 * 存结果意味着要自己维护失效逻辑（文章可能被删、订阅可能已刷新），
 * 而重新检索一次的成本远低于维护一份可能过期的快照。show() 里直接重跑 runSearch，
 * 拿到的一定是当前真实结果。
 *
 * # 为什么不用 localStorage
 * 跨重启带出旧关键词会让人拿陈旧的检索条件去找东西，且关键词本身可能敏感。
 *
 * # 何时写入
 * 只在 jump() 里（即用户真的"用这个词去看了某篇东西"），
 * Esc 关闭或看空结果不算——那种情况下记住它只会下次唤起时莫名被预填。
 *
 * 非响应式：它只在 show() / jump() 里读写，不驱动任何渲染，故用普通 let 而非 ref。
 * （`<script setup>` 顶层的代码在每个实例上各跑一次，本组件全局只挂载一份，
 *   故此处的"模块级"与"实例级"等价。）
 */
let lastKeyword = ''
/** 检索结果 */
const results = ref<PalItem[]>([])
/** 当前高亮项下标 */
const index = ref(0)
/** 是否正在检索：用于区分"没有结果"与"还没查完" */
const loading = ref(false)
/** 当前处于标题检索还是全文检索（决定右上角标签） */
const mode = ref<'title' | 'full'>('title')
/** 输入框 DOM，唤起时聚焦 */
const inputRef = ref<HTMLInputElement | null>(null)
/**
 * 结果滚动容器 DOM
 *
 * 既是 ensureVisible 的测距基准（靠 CSS 的 position: relative 成为行的 offsetParent），
 * 也是新结果到达时要归零的那个 scrollTop 的持有者。
 */
const listRef = ref<HTMLElement | null>(null)

const articlesStore = useArticlesStore()
const feedsStore = useFeedsStore()

/** 无输入时的占位文案：把能搜的东西都说清楚，用户不必猜 */
const placeholder = computed(() => '搜索文章、判断与证据…')

/**
 * 检索结果上限
 *
 * 面板可视区最多显示 6~7 行，给到 30 条足够翻找；再多的结果在面板里也翻不到，
 * 反而拖慢后端返回。
 */
const LIMIT = 30

/**
 * 笔记类命中每类取多少条
 *
 * 比文章少一个数量级：笔记是用户自己写的东西，同一个词命中的条数天然少；
 * 给到 8 条足够覆盖"我是不是写过这条"，再多就会把文章结果挤出可视区——
 * 而文章仍然是这个面板的主要用途（找那篇文章）。
 */
const NOTE_LIMIT = 8

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
 * 是否屏蔽 watch(keyword) 的下一次触发
 *
 * # 为什么需要它
 * `watch(keyword)` 在 setup 期就注册好了，而 `show()` 的 `keyword.value = q`
 * 是一次真实赋值——只要值确实变了，Vue 就会把 watch 排进 pre 队列，
 * 150ms 后再跑一次 `runSearch(q)`。于是唤起面板会检索两次：
 * 第二次进全文段时还会把 `loading` 重新置 true，**已经出来的结果列表被骨架屏
 * 顶掉再闪回来**——比多查一次更难接受。故程序化回填时必须屏蔽这一次触发。
 *
 * # 为什么只在"值确实要变且要沿用旧词"时置位
 * 值没变时 watch 根本不会触发，标记会留下来吞掉用户之后的**第一次真实击键**
 * （那次击键就搜不出东西了）。故严格限定置位条件，让标记必定被本次回填消费掉。
 *
 * # 为什么空词路径不屏蔽
 * `q` 为空时让 watch 照常走它的空分支反而更好：那一支会作废在途请求
 * （`reqSeq++`）并撤掉待执行的防抖，正好清掉"关面板前敲了一半"留下的尾巴。
 *
 * 非响应式：仅在 show() 与 watch 回调之间同步传话，不参与渲染。
 */
let suppressKeywordWatch = false

/**
 * 结果集整体换掉时把列表滚回顶部
 *
 * 用 watch 而非在 runSearch 的两个分支里各写一次：换结果有三条路径
 * （标题命中、全文命中、清空），漏一条就会出现"新搜索从半截开始显示"。
 * index 归零与滚动归零必须同时发生，否则第一行不在视口里。
 *
 * 等 nextTick 再写：结果行要等 Vue 渲染完才在 DOM 里，早一步容器还是空的。
 */
watch(results, async () => {
  // 结果集整体换掉时同样挂起 hover：列表重排会让鼠标底下换成另一行，
  // 若不锁住，新结果刚出来的高亮会被立刻改到鼠标那一行
  hoverLocked = true
  await nextTick()
  if (listRef.value) listRef.value.scrollTop = 0
})

/**
 * 打开命令面板
 *
 * 由全局 `Ctrl/⌘+K` 触发。
 *
 * # 为什么沿用上一次的关键词，而不是清空
 * 面板此前严格无状态（唤起即清空），代价落在最常见的一条路径上：
 * 「搜一个词 → 跳过去读完 → 回头想看看其余结果」必须把词重输一遍。
 * 而"跳过去读完再回来接着挑"恰恰是命令面板区别于一次性搜索框的价值所在——
 * 一次性搜索框用完就丢是合理的，多结果浏览工具丢掉上下文则是纯粹的摩擦。
 * 故改为沿用 `lastKeyword`，让面板回到"随时回到刚才那次检索"的工具形态。
 *
 * 注意这只改**起点**，不改**结果**：结果仍然每次现搜（见 lastKeyword 的注释），
 * 因此不存在"面板里挂着过期的检索快照"这种问题。
 *
 * # 为什么要 select() 全选而不是只 focus()
 * 沿用旧词时用户大概率是要改词或直接翻结果，两者都是"覆盖式"操作：
 * 全选后直接打字即改词，直接按 ↓ 即看结果。若只聚焦，光标落在末尾，
 * 想换个词得先手动清空，多一步无谓操作。
 *
 * # 为什么 focus() 与 select() 都要调，不能二选一
 * `select()` 不保证让元素获得焦点（MDN 用词是 "not necessarily"，非"一定不聚焦"），
 * `focus()` 又只定位光标、不选中文本，故两者互补。详见 show() 内该分支的注释。
 * 同帧连续调用，不引入时延。
 *
 * # 为什么先置 loading 再开面板
 * results 已清空而检索尚未发起，这一帧若不置 loading 会闪出"没有匹配…"——
 * 那是在说谎（真相是"还没查"）。loading 先置位、骨架先出现，结果回来再替换。
 *
 * @returns 无返回值；副作用为置位 open、回填关键词、按需重跑检索、选中或聚焦输入框
 */
async function show() {
  const q = lastKeyword
  // 值确实要变才置屏蔽标记：见 suppressKeywordWatch 的注释。
  // 空词不屏蔽——让 watch 的空分支替我们作废在途请求、撤掉残留防抖。
  if (q && keyword.value !== q) suppressKeywordWatch = true
  keyword.value = q
  index.value = 0
  mode.value = 'title'
  results.value = []
  // 沿用旧词时先置loading，避免 results 已空而检索未完时闪出"没有匹配…"
  loading.value = q ? true : false
  open.value = true
  await nextTick()
  if (q) {
    // 必须先 focus() 再 select()，两者互补、不可二选一：
    //   · select() 不保证让元素获得焦点（MDN："will not necessarily focus the input"）；
    //   · focus() 只定位光标、不选中文本，全选仍要靠 select()。
    // 同帧连续调用，不引入时延，"唤起即可打字"的手感不受影响。
    //
    // 为什么要显式补 focus()：它把「不保证」变成「保证」，从而**不依赖引擎实现细节**
    // （本项目三端分别是 WebKitGTK / WKWebView / WebView2，出自不同引擎团队）。
    // 附带收益是让两个分支对称——空词分支本就调 focus()，原先沿用分支不调，本身就不一致。
    //
    // 注意：这不是修复已观测到的缺陷。Linux 端实测（WebKitGTK 2.52.6）单用 select()
    // 也能隐式聚焦，选区与覆盖行为均正常；此处只是跨平台加固，不是 Bug 修复。
    inputRef.value?.focus()
    // 全选而非仅聚焦：想改词直接打字覆盖，想看结果直接按 ↓。
    // 光标停在末尾时用户要重输还得先手动清空，多一步。
    inputRef.value?.select()
    // 撤掉残留防抖：上一轮可能"敲了几个字就 Esc"，定时器还挂着。
    // watch 那次触发已被 suppressKeywordWatch 屏蔽，它的 clearTimeout 不会跑，
    // 若不在这儿补，这个陈旧的 runSearch 会在 150ms 后把本次结果顶掉。
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = undefined
    }
    // 直调 runSearch 而不等 watch(keyword) 的 150ms 防抖——否则每次唤起都有可感知的迟滞
    void runSearch(q)
  } else {
    inputRef.value?.focus()
  }
}

/** 关闭命令面板 */
function close() {
  open.value = false
}

/**
 * 打开快捷键一览
 *
 * 弹窗由 `App.vue` 持有，故走 window 事件（与 `open-settings` 同构）。
 * **必须先关本面板**：两层浮层都在 body 下，命令面板的 z-index 更高，
 * 不关会盖住帮助弹窗的遮罩，看起来像点了没反应。
 *
 * @returns 无返回值
 */
function openShortcuts() {
  close()
  window.dispatchEvent(new CustomEvent('open-shortcuts'))
}

/**
 * 打开研究工作台
 *
 * 与 {@link openShortcuts} 同构：弹窗由 `FeedPanel` 持有，走既有的 `open-research`
 * 窗口事件（`FeedPanel.handleOpenResearch` 早在监听，无需新增接线）。
 * 同样**必须先关本面板**——两者的浮层都在 body 下，工作台弹窗的遮罩 z-index 1000
 * 高于本面板的 900，不关会看着像"点了没反应"。
 *
 * # 为什么放底栏，而不是塞进结果列表
 * 结果列表的模型是 `Article[]`：上下键、回车、`jump()` 全都假定"选中的是一篇文章"。
 * 混进一条静态命令会打破这个心智模型，也会给"标题无命中自动退全文"那套判断
 * 多开一个特例。底栏本来就是放"入口"的位置（"快捷键"就在那儿），语义正确。
 *
 * 补这个入口是为了降低"误关之后重新打开"的成本：DESIGN §4.6.2 把
 * `Ctrl/⌘K → 打开研究工作台` 列为建议后续补的入口，此前一直缺。
 *
 * @returns 无返回值
 */
function openResearch() {
  close()
  window.dispatchEvent(new CustomEvent('open-research'))
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
  // 每一轮检索都从这里"重新开始"：先作废上一轮、再把 loading 复位。
  //
  // # 为什么复位必须放在入口，而不是每条分支各自记得清
  // 下面有三条出口（标题早退 / 全文 try / 全文 catch），原先只有走完整 invoke 的
  // 那条能被 `finally` 兜住。标题命中是**同步早退**——早于本轮把 loading 置 true 的
  // 任何一次 await，于是"沿用上次关键词唤起"（show() 先置 loading=true 再调本函数）
  // 碰上标题命中时，loading 永久为 true，模板 `v-if="loading"` 会把骨架屏
  // 永久留在结果列表上——恰好废掉本次需求的核心承诺。
  //
  // 收口到入口后不变式变得可读：**loading 为 true 当且仅当本轮有 invoke 在途**。
  // 入口清零保证任何"不发请求就返回"的出口都自动收尾；真正要发请求前再置回 true。
  loading.value = false
  const lower = q.toLowerCase()

  // 第一段：标题命中。直接在已加载的列表里找——不 invoke 后端，输入零延迟
  const titleHits = articlesStore.displayArticles.filter((a) =>
    a.title.toLowerCase().includes(lower),
  )
  if (titleHits.length > 0) {
    results.value = titleHits.slice(0, LIMIT).map(articleItem)
    mode.value = 'title'
    index.value = 0
    // 笔记命中异步补在后面，**不阻塞**标题结果：标题命中是同步可得的，
    // 为一次额外往返把整列压住会让面板看起来变慢了。
    // 这里确实有一次 invoke 在途而 loading 仍为 false——与"loading ⟺ 有请求在途"
    // 的旧不变式不同，是有意的：已有结果可看时，骨架屏比结果本身更碍事。
    void fetchNoteItems(q, seq).then((notes) => {
      if (notes.length > 0) results.value = [...results.value, ...notes]
    })
    return
  }

  // 第二段：标题无命中才搜全文（FTS5，含正文与摘要）。
  loading.value = true
  try {
    const hits = await invoke<Article[]>('articles_search', { query: q, limit: LIMIT })
    // 序号已过期说明用户又改了输入，丢弃这次结果
    if (seq !== reqSeq) return
    const notes = await fetchNoteItems(q, seq)
    if (seq !== reqSeq) return
    // 文章在前、笔记在后：这个面板的主要用途仍然是"找那篇文章"，
    // 自己写的判断排在它后面（但可见——这是新增的那一类结果）。
    results.value = [...hits.map(articleItem), ...notes]
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

/**
 * 把文章包成结果行
 *
 * @param article - 文章
 * @returns 带标签的结果行
 */
function articleItem(article: Article): PalItem {
  return { kind: 'article', article }
}

/**
 * 检索用户自己写的东西（高亮 / 论点 / 证据快照）
 *
 * # 为什么失败只记日志不抛
 * 面板是高频入口，一次笔记检索失败不该把**已经拿到的文章结果**一起清掉。
 * 返回空数组＝"这一类没命中"，用户仍能正常找文章。
 *
 * # 为什么带 seq 而不是让调用方判断
 * 序号校验必须发生在这里：`await` 回来时调用方可能已被新的检索覆盖，
 * 而"把过期的笔记拼到新结果后面"比"少几条结果"难查得多。
 *
 * @param q - 已 trim 的关键词
 * @param seq - 本轮请求序号
 * @returns 笔记结果行（过期或失败时为空数组）
 */
async function fetchNoteItems(q: string, seq: number): Promise<PalItem[]> {
  try {
    const notes = await invoke<NoteHit[]>('notes_search', { query: q, limit: NOTE_LIMIT })
    if (seq !== reqSeq) return []
    return notes.map(note => ({ kind: 'note' as const, note }))
  } catch (e) {
    console.error('命令面板笔记检索失败:', e)
    return []
  }
}

/** 笔记命中类型的中文名（左侧色块里的一个字 + 右侧的类型标签） */
function noteKindLabel(kind: NoteHit['kind']): string {
  if (kind === 'thesis') return '判断'
  if (kind === 'evidence') return '证据'
  return '高亮'
}

/**
 * 笔记命中行的第二行文字
 *
 * 三种命中的"我为什么该点它"不同：
 * - 判断：它挂在哪个标的下（实体名）；
 * - 高亮 / 证据：它被挂在哪条判断下——这正是 ROADMAP 要求的
 *   "每条结果标注被挂在哪个论点下"。
 *
 * @param n - 笔记命中
 * @returns 一行说明文字
 */
function noteSub(n: NoteHit): string {
  if (n.kind === 'thesis') return n.meta || '判断'
  const owner = n.thesisStatement.trim()
  if (!owner) return `${n.meta} · 还没挂到任何判断`
  return `${n.meta} · 挂在「${owner}」`
}

/**
 * 结果行的稳定 key
 *
 * 文章与笔记的 ID 空间各自独立，直接拼在一起会让 `a-12` 与 `n-12` 撞——
 * 用类型前缀分开，避免上一轮的行被复用成另一种类型（hover 高亮会串）。
 *
 * @param item - 结果行
 * @param i - 下标（仅在高亮/证据同 ID 重复时兜底）
 * @returns 唯一 key
 */
function rowKey(item: PalItem, i: number): string {
  return item.kind === 'article'
    ? `a-${item.article.id}`
    : `n-${item.note.kind}-${item.note.refId}-${i}`
}

/** 关键词变化 → 防抖后检索。150ms 足够消掉连续击键，又不会有明显迟滞 */
watch(keyword, (v) => {
  // show() 程序化回填引发的那次触发：检索已由 show() 直调 runSearch 跑过，此处跳过。
  // 标记无条件消费——它必定由那一次回填置位，不会残留到用户输入上。
  if (suppressKeywordWatch) {
    suppressKeywordWatch = false
    return
  }
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
 * 把当前高亮行滚进可视区
 *
 * `move()` 只改 index，容器不会自己滚——结果多于一屏时，高亮会一路走出视口，
 * 表现就是"按了方向键没反应"。必须显式滚。
 *
 * # 为什么不用 scrollIntoView()
 * 它会连带滚动**所有**可滚动祖先，容器外若还有可滚层就会把整块界面一起挪。
 * 项目里 SelectCapsule 已因同样理由禁用过，这里保持一致：直接写容器 scrollTop，
 * 把影响范围锁死在结果区内。
 *
 * # 上下各留 8px 余量
 * 差一两像素也别滚，否则贴边的行会让列表频繁小幅抖动。
 *
 * @returns 无返回值；副作用为按需改写结果容器的 scrollTop
 */
function ensureVisible() {
  const box = listRef.value
  if (!box) return
  const row = box.querySelectorAll<HTMLElement>('.pal-row')[index.value]
  if (!row) return

  const PAD = 8
  const top = row.offsetTop
  const bottom = top + row.offsetHeight
  const viewTop = box.scrollTop
  const viewBottom = viewTop + box.clientHeight

  if (top - PAD < viewTop) box.scrollTop = Math.max(0, top - PAD)
  else if (bottom + PAD > viewBottom) box.scrollTop = bottom + PAD - box.clientHeight
}

/**
 * 键盘导航期间是否挂起"鼠标划过即高亮"
 *
 * # 为什么需要
 * 结果行上挂着 `@mouseenter`，鼠标移进某行就把高亮改到那行。但 mouseenter
 * 不只由"鼠标移动"触发，**元素移动到鼠标底下**同样触发。于是按方向键 → 列表滚动
 * → 鼠标没动、下方却换了一行 → 补发 mouseenter → 高亮被拽回鼠标那一行，
 * 键盘就走不动了。换结果集时列表重排，同理。
 *
 * # 何时解锁
 * 只在鼠标发生**真实位移**时解除。滚动会让 Chromium 合成 mousemove，但合成事件的
 * `movementX/Y` 恒为 0，据此过滤掉——否则滚动自己就把锁解开了。
 *
 * 非响应式：它只参与事件回调里的判断，不驱动渲染，用普通 let 即可。
 */
let hoverLocked = false

/**
 * 鼠标划过结果行时同步高亮
 *
 * 键盘导航挂起期间直接忽略，避免高亮在两种输入方式之间来回跳。
 *
 * @param i - 鼠标所在行的下标
 * @returns 无返回值
 */
function onRowEnter(i: number) {
  if (hoverLocked) return
  index.value = i
}

/**
 * 鼠标在面板内移动 → 解除 hover 挂起
 *
 * @param e - 鼠标事件；用 movementX/Y 判定是否为真实位移
 * @returns 无返回值
 */
function onMouseMove(e: MouseEvent) {
  // 位移为 0 说明是滚动合成的事件，不是用户真的动了鼠标
  if (e.movementX === 0 && e.movementY === 0) return
  hoverLocked = false
}

/**
 * 上下移动高亮项
 *
 * **不循环**：走到首项再按上、或走到末项再按下，都停在原地。此前用取模实现
 * 环绕，结果是按到底部时高亮突然翻回第一项、列表整个滚回顶部，方向感和视线
 * 都断了。命令面板的结果集通常是"从上往下找"，到头即止比绕圈更符合预期。
 *
 * 越界时仍要滚一次：用户可能先用鼠标滚轮把列表滚到别处，此时当前高亮行未必
 * 在视口内，越界"无响应"应当表现为"高亮不动、但把它带回视野"，而不是彻底静默。
 *
 * 移动后也必须滚一次：index 变了不代表那一行在视口里。
 * 等 nextTick 再量——高亮类的切换要等 Vue 渲染完，此刻 DOM 才是新的一行。
 *
 * @param d - 位移方向：1 下一项，-1 上一项
 * @returns 无返回值
 */
async function move(d: number) {
  if (results.value.length === 0) return
  // 键盘导航期间挂起 hover 抢占：本次移动会滚动列表，而滚动会让别的行滑到
  // 鼠标底下、触发 mouseenter 把高亮拽回去
  hoverLocked = true
  const next = index.value + d
  if (next < 0 || next >= results.value.length) {
    // 到头停住，但把当前高亮行滚回视野
    await nextTick()
    ensureVisible()
    return
  }
  index.value = next
  await nextTick()
  ensureVisible()
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
 * 跳转到当前高亮项
 *
 * 由 `navigateToArticle` 切范围、写选中态触发正文渲染，最后派发 `reveal-in-list`
 * 让两栏的胶囊各自播一次定位脉冲。派发而非直接调 ref，是因为两栏可能
 * 尚未渲染出目标条目（首次展开某分组等），由它们在 nextTick 后自行处理更稳。
 *
 * @returns 无返回值；副作用为记下本次检索词、写 store、关闭面板、派发定位事件
 */
async function jump() {
  const item = results.value[index.value]
  if (!item) return
  // 记住本次用的检索词，供下次唤起沿用。必须落在 close() 之前：
  // close() 只是 open=false，但它代表"这次交互结束了"——语义上先记账再收摊。
  // 空词不记：无关键词的跳转（理论上进不到这里）不该污染下次唤起的起点。
  const used = keyword.value.trim()
  if (used) lastKeyword = used
  close()

  // 笔记命中：落到「研究工作台 → 论点 → 那一条」。
  //
  // 这里不能直接切页签：工作台弹窗的打开状态由 `FeedPanel` 持有，本组件
  // 唯一能用的通道是 `open-research` 事件；而工作台打开时固定落报告页签。
  // 故先把落点写进待决意图，由它在确定打开的那一刻取走（见 `utils/thesisJump`）。
  // thesisId 为 null 的高亮还没有归属判断：只切论点页签，不打开任何详情。
  if (item.kind === 'note') {
    requestThesisOpen(item.note.thesisId)
    window.dispatchEvent(new CustomEvent('open-research'))
    return
  }

  const a = item.article

  // 切范围 → 打开 → 等条目 → 派发定位，整条编排在 utils/navigateToArticle 里，
  // 与研究工作台、实体仪表盘共用同一条路径（这里曾各写一份，且都漏了清标签筛选）。
  await navigateToArticle(a.id)
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
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
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
  backdrop-filter: blur(16px);
  /* 命令面板是浮在最上层、承载全局搜索的聚焦层，玻璃要比普通弹窗更实一档 */
  -webkit-backdrop-filter: blur(16px);
  /* 外框用 --glass-hair（冷蓝灰）而非 --glass-hi（白）：
     浅色主题下面板底已是 #f6f7f7，白描边落上去**完全看不见**，
     面板与背后界面糊成一片，正是"输入框边框是白色的、没有区分"的来源。
     玻璃厚度改由上下两道的inset 高光承担（暗色下仍主要靠 --glass-hi）。 */
  border: 1px solid var(--glass-hair);
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    inset 0 -1px 0 var(--glass-lo),
    var(--shadow-pop);
}

/* ── 输入行 ───────────────────────────────────────────── */
/* 头部 = 输入区。刻意给它一块**独立底色**与下边分隔线：
   没有这两样时，整块面板是一整片同色，输入框与结果列表之间毫无结构感，
   看起来就"像个灰盒子"。分区之后才有"上面是搜索、下面是结果"的层次。 */
.pal-head {
  display: flex;
  align-items: center;
  gap: var(--sp-15);
  padding: var(--sp-4) var(--sp-5);
  background: var(--glass-head-bg);
  border-bottom: 1px solid var(--glass-divider);
  flex-shrink: 0;
}
/* secondary 而非 tertiary：面板底近白，tertiary 在上面只有 2.7:1 */
.pal-ico { width: 18px; height: 18px; color: var(--text-secondary); flex-shrink: 0; }
.pal-head input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  font-family: inherit;
  /* --fs-md 而非 --fs-lg：输入文字不应比结果标题更大，否则视觉重心
     全落在输入框上、下方真正要浏览的结果反而被压低一等。 */
  font-size: var(--fs-md);
  font-weight: 500;
  color: var(--text-primary);
}
.pal-head input::placeholder { color: var(--text-secondary); opacity: 0.75; }
/* 检索模式标签：标题段用中性色，全文段用品牌紫罗兰（与文章列表的胶囊描边同源） */
.pal-mode {
  flex-shrink: 0;
  font-size: var(--fs-xs);
  font-weight: 600;
  padding: 2px var(--sp-2);
  border-radius: var(--r-pill);
  background: var(--primary-soft);
  color: var(--primary-text);
}
.pal-mode.full { background: var(--cap-accent-bg); color: var(--cap-accent-line); }

/* ── 结果区 ───────────────────────────────────────────── */
/* 结果区：比面板底**压深**一档（--glass-body-bg），使"输入区/ 结果区"
   除了分隔线之外还有底色差。浅色主题下靠加深而非加白——面板已接近白，
   加白等于没加。 */
.pal-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-2);
  background: var(--glass-body-bg);
  /* 必须是结果行的 offsetParent：ensureVisible 用 offsetTop 量行相对滚动容器的位置。
     不加这句，offsetTop 会以 .pal-scrim 为基准、把头部高度也算进去，滚动量就偏了。
     与 FeedPanel 的 .feed-list 是同一个约定（胶囊测距也靠它）。 */
  position: relative;
}
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
  /* secondary 而非 tertiary：同上，tertiary 在近白底上只有 2.7:1 */
  color: var(--text-secondary);
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
  /* 用 --cap-accent-bg 而非 --cap-solid-bg：后者在浅色主题下是**实色紫罗兰**
     （--brand-to），面板里的深色标题压上去会读不清。命令面板是浮层、
     不需要"实体色块"那种强锚定，描边 + 淡底已足够表达选中。 */
  /* 20% 品牌色底 + 58% 描边：面板底已是近白，用列表里的 12%/42% 会弱到看不见。 */
  background: color-mix(in srgb, var(--brand-from) 20%, transparent);
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--brand-from) 58%, transparent),
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
/* 笔记类结果没有订阅源图标，用类型色块区分：判断=主色、证据=语义色、高亮=中性。
   颜色在这里是"分类标记"而不是装饰，所以三种必须明显不同——
   否则用户在一条混排列表里分不清哪行是文章、哪行是自己写的东西。 */
.pal-av.note {
  color: var(--primary-text);
  background: color-mix(in srgb, var(--primary) 14%, transparent);
  box-shadow: none;
}
.pal-av.note.evidence {
  color: var(--tone-indigo);
  background: color-mix(in srgb, var(--tone-indigo) 14%, transparent);
}
.pal-av.note.highlight {
  color: var(--text-secondary);
  background: var(--fill-secondary);
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
/* 命中高亮：与品牌紫罗兰同源，是画面里仅有的两处饱和色之一 */
.pal-snippet :deep(mark), .pal-title :deep(mark) {
  /* 实色品牌紫罗兰底 + --cap-active-fg 文字（浅色为纯白、深色为主题文字色），
     而不是 --cap-accent-bg（淡底）：
     面板已是近白色，12% 品牌色落在上面几乎看不出"命中"，
     搜索结果里最该被一眼看到的位置反而最弱。 */
  background: var(--brand-from);
  color: var(--cap-active-fg);
  border-radius: 2px;
  box-shadow: inset 0 -1px 0 var(--cap-accent-line);
  padding: 0 1px;
}
/* 时间用 secondary 而非 tertiary：面板底已是近白，
   tertiary 在上面实测仅 2.7:1（低于 4.5:1），几乎读不出来。
   来源名已是 primary系，只需把默认色从 tertiary 提到 secondary 兜底。 */
.pal-side {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  font-size: var(--fs-xs);
  color: var(--text-secondary);
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
  border-top: 1px solid var(--glass-divider);
  background: var(--glass-head-bg);
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  flex-shrink: 0;
}
/* 键帽外观来自全局 `.kbd`。底栏只负责间距（两条键帽挨着会黏在一起）。 */
.pal-foot .kbd { margin-right: 3px; }
.pal-hint { margin-left: auto; opacity: 0.85; }

/* 底栏里的可点入口：外观贴近 .pal-hint（同为次要信息），但要有可点的反馈 */
.pal-action {
  border: none;
  background: none;
  padding: 0 var(--sp-1);
  font: inherit;
  font-size: inherit;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: var(--r-sm);
}
.pal-action:hover { color: var(--primary); background: var(--cap-feed); }

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