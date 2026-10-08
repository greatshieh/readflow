<template>
  <!-- 侧边栏根容器：isOpen 仅在窄屏（配合 .open 样式）下产生抽屉效果，宽屏常驻显示 -->
  <aside class="feed-panel pane-glow-host" :class="{ open: isOpen }" ref="paneRef">
    <!-- 跟随边框：靠近指针的那一侧亮起一段。必须是面板的直接子元素才能铺满整栏。
         亮灭由 useCursorGlow 在面板上切 .is-lit，这里同步给边框层（它自身
         定位在面板内，需要自己的类名来控制 opacity）。 -->
    <div class="pane-frame" ref="frameRef" aria-hidden="true"></div>
    <!-- 订阅源搜索区：本地过滤用，不请求后端 -->
    <div class="feed-search-wrap">
      <div class="feed-search">
        <svg class="s-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="8"></circle>
          <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
        </svg>
        <input type="text" placeholder="搜索订阅源..." v-model="searchQuery" />
      </div>
    </div>
    <!-- 订阅源列表：错误 / 加载中 / 空 三种状态互斥展示，正常时渲染过滤后的列表。
         加载中用骨架屏而非纯文字：占位块按真实 .feed-item 几何（28px 头像 + 标题行）铺排，
         加载完成切换时列表不跳版；空态与错误态复用全局 .state-block。

         SelectCapsule 是选中态的唯一视觉载体：条目自身只留极弱 hover 底色。
         它必须与条目同处 .feed-list 这个定位基准内，故作为列表容器的子节点渲染在条目之前。 -->
    <div class="feed-list" ref="feedListRef"
      @mouseover="onListMouseOver"
      @mouseout="onListMouseOut"
    >
      <SelectCapsule ref="feedCapsule" variant="solid" :container="() => feedListRef" />
      <div v-if="feedsStore.feedError" class="state-block is-error">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
          <circle cx="12" cy="12" r="10"></circle>
          <line x1="12" y1="8" x2="12" y2="12"></line>
          <line x1="12" y1="16" x2="12.01" y2="16"></line>
        </svg>
        <span>{{ feedsStore.feedError }}</span>
      </div>
      <div v-else-if="feedsStore.loading" class="skeleton-list" aria-hidden="true">
        <div v-for="i in 6" :key="i" class="skeleton-row">
          <span class="sk-avatar"></span>
          <span class="sk-lines">
            <span class="sk-line"></span>
            <span class="sk-line sk-line-short"></span>
          </span>
        </div>
      </div>
      <!-- 空态：只在"加载完成且确实没有订阅源"时出现。给品牌标志位做视觉锚点，
           配合下方「添加订阅」按钮形成"引导 → 操作"的阅读顺序。 -->
      <div v-else-if="feedsStore.feeds.length === 0" class="state-block">
        <span class="state-logo"><AppLogo :size="48" /></span>
        <span>还没有订阅源</span>
        <span class="state-hint">添加一个 RSS 地址开始订阅</span>
      </div>
      <!-- 文件夹分组：按 folder_id 把订阅源归入对应文件夹，无文件夹的归入"未分类"。
           每个分组可折叠（点击分组头），分组未读总数显示在分组头右侧。 -->
      <template v-for="group in groupedFeeds" :key="group.key">
        <!-- 文件夹分组：折叠结构外包一层 .folder-body（grid-template-rows: 1fr→0fr），
             高度动画无需测量真实内容高度即可生效；.folder-caret 点击时在该点分裂出水滴。 -->
        <div class="folder-group" :class="{ collapsed: collapsed.has(group.key) }" v-if="group.feeds.length">
          <div class="folder-head" @click="toggleFolder(group, $event)">
            <span class="folder-caret">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="6 9 12 15 18 9"></polyline>
              </svg>
            </span>
            <!-- 分组名单行截断（见 .folder-name 的 ellipsis）：title 让悬浮时能读到全称 -->
            <span class="folder-name" :title="folderLabel(group)">{{ folderLabel(group) }}</span>
            <span class="feed-item-count" v-if="group.unread > 0">{{ group.unread }}</span>
          </div>
          <div class="folder-body">
            <div>
            <FeedListItem
              v-for="feed in group.feeds"
              :key="feed.id"
              :feed="feed"
              :active="feedsStore.selectedFeed?.id === feed.id"
              :data-feed-id="feed.id"
              @select="selectFeed(feed)"
              @manage="openManage(feed)"
            />
            </div>
          </div>
        </div>
      </template>

      <!-- 标签分区：与「文件夹」并列的第二类整理维度。
           点击标签进入标签视图（所有源中带该标签的文章），再次点击退出。
           标签与订阅源是**互斥范围**，故选中标签时会清空源选择（见 selectTag）。
           搜索订阅源时隐藏，避免与"过滤订阅源"的搜索结果混在一起造成误读。 -->
      <div class="folder-group" :class="{ collapsed: tagSectionCollapsed }" v-if="!searchQuery.trim() && tagsStore.tags.length > 0">
        <div class="folder-head" @click="toggleTagSection">
          <span class="folder-caret">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="6 9 12 15 18 9"></polyline>
            </svg>
          </span>
          <span class="folder-name">标签</span>
        </div>
        <div class="folder-body">
          <div>
            <div
              v-for="tag in tagsStore.tags"
              :key="tag.id"
              class="tag-row"
              :class="{ active: articlesStore.tagFilterId === tag.id }"
              @click="selectTag(tag.id)"
            >
              <span class="tag-row-dot" :style="{ background: tagColorVar(tag.color) }"></span>
              <span class="tag-row-name" :title="tag.name">{{ tag.name }}</span>
              <span class="feed-item-count" v-if="tag.article_count > 0">{{ tag.article_count }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
    <!-- 底部工具条（单行图标，替代原先 4 个整行按钮省出的 ~130px 纵向空间）：
         左侧「＋添加订阅」（打开 FeedAddModal：地址 + 可选标题 + 分组，忙碌态由弹窗 saving 自持）
         与「···更多」（新建文件夹 / 过滤规则 / 研究工作台 / 智能摘要 / 实体关系图，均为低频操作）；
         右侧「⟳刷新全部」，刷新进行中图标旋转并显示后端逐源广播的 n/m 进度。 -->
    <div class="feed-toolbar">
      <button class="tool-btn" @click="showAdd = true" title="添加订阅">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="12" y1="5" x2="12" y2="19"></line>
          <line x1="5" y1="12" x2="19" y2="12"></line>
        </svg>
      </button>
      <div class="more-menu-wrap" ref="moreMenuRef">
        <button
          class="tool-btn"
          :class="{ active: showMoreMenu }"
          @click="showMoreMenu = !showMoreMenu"
          title="更多功能"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
            <circle cx="12" cy="5" r="2"></circle>
            <circle cx="12" cy="12" r="2"></circle>
            <circle cx="12" cy="19" r="2"></circle>
          </svg>
        </button>
        <div v-if="showMoreMenu" class="more-dropdown">
          <!-- 新建文件夹：低频结构整理操作，从整行按钮降级收进本菜单 -->
          <button class="dropdown-item" @click="showMoreMenu = false; createFolder()">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
            </svg>
            新建文件夹
          </button>
          <button class="dropdown-item" @click="showRules = true; showMoreMenu = false">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"></polygon>
            </svg>
            过滤规则
          </button>
          <button class="dropdown-item" @click="showResearch = true; showMoreMenu = false">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M3 3v18h18"></path>
              <path d="M18.7 8l-5.1 5.2-2.8-2.8L7 14.3"></path>
            </svg>
            研究工作台
          </button>
          <!-- 智能摘要：按关注实体分组看高分文章，属"挑着读"的低频入口 -->
          <button class="dropdown-item" @click="showDigest = true; showMoreMenu = false">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M12 2l2 6.1L20 10l-6 1.9L12 18l-2-6.1L4 10l6-1.9z"></path>
              <path d="M19 14l1 3 3 1-3 1-1 3-1-3-3-1 3-1z"></path>
            </svg>
            智能摘要
          </button>
          <!-- 实体关系图：切到主屏全宽展示（与文章阅读互斥的另一种主屏内容） -->
          <button class="dropdown-item" @click="showGraphView">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="5" cy="6" r="3"></circle>
              <circle cx="19" cy="6" r="3"></circle>
              <circle cx="12" cy="18" r="3"></circle>
              <line x1="7.5" y1="8" x2="10.5" y2="15.5"></line>
              <line x1="16.5" y1="8" x2="13.5" y2="15.5"></line>
              <line x1="8" y1="6" x2="16" y2="6"></line>
            </svg>
            实体关系图
          </button>
        </div>
      </div>
      <span class="tool-spacer"></span>
      <button class="tool-btn" :class="{ refreshing: refreshing }" @click="refreshFeeds" title="刷新全部订阅源">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="23 4 23 10 17 10"></polyline>
          <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"></path>
        </svg>
        <span v-if="progressText" class="refresh-progress">{{ progressText.trim() }}</span>
      </button>
    </div>

    <!-- 订阅源管理弹窗：改名、重新归类、更换文件夹、修改链接、删除。
         由 feed 条目上的「⋯」按钮打开（managingFeed 非空即为打开态）；
         遮罩 / Esc / 焦点陷阱 / 滚动锁由 BaseModal 提供，表单与删除确认在组件内自持。 -->
    <FeedManageModal :feed="managingFeed" @close="closeManage" />
    <!-- 添加订阅弹窗：地址 + 可选标题 + 分组（showAdd 为 true 即打开） -->
    <FeedAddModal :open="showAdd" @close="showAdd = false" />

    <!-- 过滤规则管理弹窗：由底部「过滤规则」按钮打开 -->
    <RulesModal :open="showRules" @close="showRules = false" />
    <!-- 智能摘要弹窗：由「更多 → 智能摘要」打开，点击其中的文章直接跳去阅读 -->
    <DigestModal
      :open="showDigest"
      @close="showDigest = false"
      @select-article="openArticleFromDigest"
    />
    <!-- 研究工作台弹窗：由底部「研究工作台」按钮打开 -->
    <ResearchModal :open="showResearch" @close="showResearch = false" />
  </aside>
</template>

<script setup lang="ts">
/**
 * 订阅源侧边栏组件
 *
 * # 职责
 * 左侧常驻（窄屏为抽屉）的订阅源面板，负责：
 * 1. 展示订阅源列表，并支持按名称本地过滤；
 * 2. 呈现每个源的未读数、选中态与加载 / 错误 / 空三种状态；
 * 3. 提供"添加订阅"与"刷新全部订阅源"两个写入入口；
 * 4. 弹窗编排：添加订阅（FeedAddModal）/ 管理订阅源（FeedManageModal）/ 过滤规则 /
 *    智能摘要 / 研究工作台。
 *
 * # 在整体布局中的位置
 * 由 App.vue 作为 `.app` 的第一个子节点渲染，位于 `.main`（标题栏 + 工具栏 + 时间线）左侧；
 * 通过 `padding-top: var(--tb-h)` 为固定定位的 TitleBar 让出顶部空间。
 *
 * # 与 Pinia store 的关系
 * 数据源头是 `useFeedsStore()`：
 * - 列表、加载态、错误全部读 `feedsStore.feeds / loading / error`，
 *   这些字段由 store 内部 `invoke('feeds_list')` 等 Tauri command 填充；
 * - 选中态以 `feedsStore.selectedFeed` 为唯一事实来源，ArticleColumn 通过 watch 它来加载文章，
 *   因此这里是"改 store 即驱动全局"的入口；
 * - 写操作（添加订阅 / refreshFeeds）全部委托弹窗组件或 store，本组件只负责入口与反馈。
 *
 * # 跨组件通信
 * 除 store 外，还用 window 自定义事件与兄弟组件协作：
 * - 监听 `toggle-feed-panel`（由标题栏的菜单按钮派发）来开合抽屉。
 * 选中订阅源不再派发事件：选中态直接写入 feedsStore.selectedFeed，
 * 由 ArticleColumn 通过 watch 被动响应，无需事件广播。
 */

import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { useFeedsStore } from '@/stores/feeds'
import { useArticlesStore } from '@/stores/articles'
import { useTagsStore, tagColorVar } from '@/stores/tags'
import { useModalStore } from '@/stores/modal'
import { useUiStore } from '@/stores/ui'
import RulesModal from './RulesModal.vue'
import DigestModal from './DigestModal.vue'
import ResearchModal from './ResearchModal.vue'
import FeedListItem from './FeedListItem.vue'
import FeedAddModal from './FeedAddModal.vue'
import FeedManageModal from './FeedManageModal.vue'
import SelectCapsule from './SelectCapsule.vue'
import AppLogo from './AppLogo.vue'
import { useCursorGlow } from '@/composables/useCursorGlow'
import type { Article, Feed, Folder } from '@/types'

/**
 * 抽屉是否展开
 *
 * 仅用于窄屏（<=768px）下的开合动画；宽屏侧边栏常驻，该值不影响样式表现。
 */
const isOpen = ref(false)

/**
 * 更多菜单是否展开
 *
 * 点击「更多」按钮切换开合；点击外部区域或菜单内任意一项时自动收起。
 */
const showMoreMenu = ref(false)
/** 更多菜单根元素（用于计算下拉位置，预留扩展） */
const moreMenuRef = ref<HTMLElement | null>(null)

/**
 * 过滤规则弹窗是否打开
 *
 * 由「更多」菜单中的「过滤规则」项置 true，弹窗关闭时复位为 false。
 */
const showRules = ref(false)

/**
 * 智能摘要弹窗是否打开
 *
 * 由「更多」菜单中的「智能摘要」项置 true，弹窗关闭时复位为 false。
 */
const showDigest = ref(false)

/**
 * 研究工作台弹窗是否打开
 *
 * 由「更多」菜单中的「研究工作台」项置 true，弹窗关闭时复位为 false。
 */
const showResearch = ref(false)

/**
 * 订阅源过滤关键字（输入框 v-model）
 *
 * 过滤在前端内存里完成：订阅源数量通常只有几十个，
 * 没必要为一次输入去 invoke 后端，也能保证输入即时响应。
 */
const searchQuery = ref('')

/**
 * 是否正在刷新
 *
 * 独立于 store 的 refreshing 字段，是因为本组件需要在刷新期间给图标加旋转动画，
 * 而动画状态属于纯 UI 反馈，放在组件内最直观。
 */
const refreshing = ref(false)

/**
 * 当前正在管理的订阅源（管理弹窗的数据来源）
 *
 * 为 null 时弹窗关闭（BaseModal 隐藏面板）；名称 / 链接 / 文件夹的可编辑副本
 * 由 FeedManageModal 自持并在打开时回填，这样"取消"不会污染原始数据。
 */
const managingFeed = ref<Feed | null>(null)

/**
 * 订阅源 store 实例
 *
 * 所有订阅源数据与该组件对数据的写操作都经由它完成，
 * 组件自身不持有任何 feeds 副本，避免出现两份不一致的列表。
 */
const feedsStore = useFeedsStore()
/** 全局弹窗 store 实例：用于替代原生 confirm/prompt/alert */
const modalStore = useModalStore()
/** UI 状态 store：实体关系图菜单项要切主屏视图 */
const uiStore = useUiStore()
/**
 * 文章 store 实例
 *
 * 仅用于「智能摘要」里点击文章后把选中态交给正文栏——本面板不渲染任何文章列表，
 * 因此不持有其它文章数据，避免侧边栏跟着列表一起重渲染。
 */
const articlesStore = useArticlesStore()

/**
 * 标签 store 实例
 *
 * 侧边栏只消费标签目录（名称 / 颜色 / 文章数）并把选中态写进 articlesStore，
 * 标签本身的增删改在正文页的打标面板里完成，本面板不重复提供入口。
 */
const tagsStore = useTagsStore()

/** 标签分区是否折叠（与文件夹分组同一交互范式） */
const tagSectionCollapsed = ref(false)

/** 订阅源列表容器（滑动胶囊与水滴的定位基准） */
const feedListRef = ref<HTMLElement | null>(null)
/** 侧栏面板根元素（光斑与跟随边框的宿主） */
const paneRef = ref<HTMLElement | null>(null)
/** 跟随边框层：与光斑层分离，便于各自控制显隐 */
const frameRef = ref<HTMLElement | null>(null)
/** 启用面板光斑跟随 + 靠近侧边框亮起 */
useCursorGlow(paneRef, frameRef)
/** 选中胶囊组件实例：负责把胶囊滑到当前选中的订阅源 */
const feedCapsule = ref<InstanceType<typeof SelectCapsule> | null>(null)

/**
 * 在折叠按钮处播放水滴分裂
 *
 * 折叠是本组件唯一使用水滴动效的地方：点击时从按钮中分裂两枚小珠坠落淡出，
 * 与折叠/展开的高度动画同步发生。刻意保持"全文只此一处"——水滴用在多处会变成噱头，
 * 抢走内容注意力。
 *
 * @param e - 点击事件的 MouseEvent，用于取被点击的具体按钮（多分组共用同一个 ref 数组）
 * @returns 无返回值
 */
function playDropSplit(e: MouseEvent) {
  const caret = e.currentTarget as HTMLElement | null
  if (!caret) return
  for (let i = 0; i < 2; i++) {
    const drop = document.createElement('span')
    drop.className = 'folder-drop'
    // 左右各偏一点并给随机量，避免两次点击的轨迹完全相同而显得机械
    drop.style.setProperty('--dx', `${(i === 0 ? -1 : 1) * (7 + Math.random() * 5)}px`)
    drop.style.setProperty('--dy', `${9 + Math.random() * 7}px`)
    drop.addEventListener('animationend', () => drop.remove())
    caret.appendChild(drop)
  }
}

/**
 * 把选中胶囊滑到当前选中的订阅源
 *
 * 胶囊测的是条目几何，而分组折叠 / 筛选搜索都会让条目重排，故这三处都要重新同步。
 * `noTransition` 用于这些"布局已变、位置直接跳过去"的场景——只有用户主动点击
 * 才需要看到滑动。
 *
 * 查询同时涵盖 `.feed-item.active` 与 `.tag-row.active`：标签视图与订阅源视图
 * 是互斥范围，但两者共用这一个面板，胶囊必须跟着当前生效的那类走。
 *
 * @param noTransition - 是否跳过过渡直接落位
 * @returns 无返回值
 */
/**
 * 把选中胶囊同步到当前选中的订阅源
 *
 * @param sliding - true 表示这是一次用户主动点击，胶囊**滑动**过去（0.26s）。
 *   默认 false：布局变化远比点击频繁（折叠、换范围、搜索都会触发），
 *   那些场景下若带过渡，会看到胶囊从旧位置慢慢飞过来，像卡顿。
 * @returns 无返回值
 */
function syncCapsule(sliding = false) {
  const list = feedListRef.value
  const capsule = feedCapsule.value
  if (!list || !capsule) return
  const active = list.querySelector<HTMLElement>('.feed-item.active, .tag-row.active')
  capsule.sync(active, sliding)
}

/**
 * 指针移动时把 hover 胶囊跟到所在条目上
 *
 * 用事件委托而非逐条绑定：条目列表会随折叠 / 筛选重排，逐条绑定需要反复解绑重绑。
 * 捕获 `mouseover`（会冒泡）而非 `mouseenter`（不冒泡，无法委托）。
 *
 * @param e - 鼠标事件
 * @returns 无返回值
 */
function onListMouseOver(e: MouseEvent) {
  const list = feedListRef.value
  const capsule = feedCapsule.value
  if (!list || !capsule) return
  const item = (e.target as HTMLElement | null)?.closest<HTMLElement>('.feed-item, .tag-row')
  capsule.hover(item ?? null)
}

/**
 * 指针离开列表时隐藏 hover 胶囊
 *
 * 必须处理：若只在 mouseover 里维护，指针从列表移到面板空白处后，胶囊会
 * 停在最后一项上不消失。
 *
 * @param e - 鼠标事件；用 relatedTarget 判断是真的离开，还是在列表内部移动
 * @returns 无返回值
 */
function onListMouseOut(e: MouseEvent) {
  const list = feedListRef.value
  const capsule = feedCapsule.value
  if (!list || !capsule) return
  // relatedTarget 非空说明只是移到了列表内部的另一个条目上，交给 mouseover 处理
  const to = e.relatedTarget as Node | null
  if (to && list.contains(to)) return
  capsule.hover(null)
}

/**
 * 折叠 / 展开某个文件夹分组
 *
 * 从「v-show 隐藏条目」改为「grid-template-rows 1fr→0fr 的高度动画」，故不再需要
 * 整体替换 collapsed 集合来触发渲染——但 Set 的原地 mutate 不会被 Vue 追踪到，
 * 仍必须整体替换一次才能让折叠态与高度动画生效。展开 / 折叠互为逆过程，复用同一条声明。
 *
 * @param group - 被切换的分组
 * @param e - 点击事件（用于播放水滴）
 * @returns 无返回值；副作用为翻转该分组的折叠态并重算胶囊位置
 */
function toggleFolder(group: { key: string }, e: MouseEvent) {
  playDropSplit(e)
  const next = new Set(collapsed.value)
  if (next.has(group.key)) next.delete(group.key)
  else next.add(group.key)
  collapsed.value = next
  // 等高度动画的样式落位后再同步胶囊，否则测到的还是动画开始前的几何
  void nextTick(() => syncCapsule())
}

/**
 * 折叠 / 展开标签分区
 *
 * 与文件夹分组同一套结构与动效，只是持久化位置不同（本地 state 而非 collapsed 集合）。
 *
 * @param e - 点击事件（用于播放水滴）
 * @returns 无返回值
 */
function toggleTagSection(e: MouseEvent) {
  playDropSplit(e)
  tagSectionCollapsed.value = !tagSectionCollapsed.value
  void nextTick(() => syncCapsule())
}

/**
 * 刷新进度文案（如 " 3/31"；无进度时为空串）
 *
 * 数据来自后端逐源广播的 `refresh-progress` 事件（经 `feedsStore.refreshProgress` 暴露），
 * 让用户在整轮刷新进行中就能看到"刷到第几个源"；用 computed 派生可保证
 * 进度消失（store 复位为 null）时按钮文案自动回到纯"刷新"二字。
 */
const progressText = computed(() => {
  const p = feedsStore.refreshProgress
  return p ? ` ${p.done}/${p.total}` : ''
})

/**
 * 按关键字过滤后的订阅源列表
 *
 * 大小写不敏感匹配 `feed.name`；无关键字时直接返回 store 中的原始数组引用，
 * 这样零输入场景不会产生额外拷贝，且 store 更新后能立即反映到视图。
 *
 * @returns 过滤后的 Feed 数组；无关键字时为 feedsStore.feeds 本身
 */
const filteredFeeds = computed(() => {
  if (searchQuery.value) {
    return feedsStore.feeds.filter((f: Feed) => f.name.toLowerCase().includes(searchQuery.value.toLowerCase()))
  }
  return feedsStore.feeds
})

/**
 * 折叠的文件夹分组集合
 *
 * 记录当前被折叠的分组 key（'f<id>' 表示文件夹，'uncat' 表示未分类），
 * 渲染时据此隐藏其下订阅源。用 Set 保存，折叠切换时整体替换以触发响应式。
 */
const collapsed = ref<Set<string>>(new Set())

/**
 * 按文件夹分组的订阅源结构（用于侧边栏分组渲染）
 *
 * 在 `filteredFeeds`（已按名称过滤）之上做分组：每个文件夹一组，无文件夹的归入"未分类"组；
 * 同时计算每个分组的未读总数，供分组头角标显示。分组顺序跟随 `feedsStore.folders`。
 *
 * @returns 分组数组，每项含唯一 key、文件夹对象（未分类为 null）、该组订阅源列表与未读总数
 */
const groupedFeeds = computed(() => {
  const feeds = filteredFeeds.value
  const groups: { key: string; folder: Folder | null; feeds: Feed[]; unread: number }[] = []
  for (const folder of feedsStore.folders) {
    const fs = feeds.filter((f: Feed) => f.folder_id === folder.id)
    groups.push({
      key: 'f' + folder.id,
      folder,
      feeds: fs,
      unread: fs.reduce((s: number, f: Feed) => s + (f.unread_count || 0), 0)
    })
  }
  // 未归入任何文件夹（folder_id 为 0 或指向已删除文件夹）的源，统一进入"未分类"组
  const unc = feeds.filter((f: Feed) => !f.folder_id)
  if (unc.length) {
    groups.push({
      key: 'uncat',
      folder: null,
      feeds: unc,
      unread: unc.reduce((s: number, f: Feed) => s + (f.unread_count || 0), 0)
    })
  }
  return groups
})

/**
 * 取文件夹分组的显示名
 *
 * 分组头里既要渲染文本、又要作为 `title`（悬浮显示全称）的取值，两处若各写一遍
 * 三元表达式，改文案时极易漏改其中一处，故收敛成单一来源。
 *
 * @param group - 分组对象；`folder` 为 null 表示"未分类"组
 * @returns 文件夹名，未分类时返回"未分类"
 */
function folderLabel(group: { folder: Folder | null }): string {
  return group.folder ? group.folder.name : '未分类'
}

/**
 * 刷新全部订阅源
 *
 * 委托 feedsStore.refreshFeeds()（内部 `invoke('feeds_refresh')` 拉取新文章并重载列表）。
 * 用 try/finally 包裹是为了：即便刷新失败或 store 内部已吞掉异常，
 * 图标的旋转动画也必须停止，否则会一直停留在 refreshing 态。
 * 失败原因由 store 写入 `refreshError`，这里显式弹一次——后端会拒掉与定时刷新
 * 重叠的请求（"已有刷新任务在进行中"），不提示的话用户只会觉得"点了没反应"。
 *
 * @returns 无返回值（Promise 在完成时 resolve）
 * @副作用 置位 refreshing 驱动动画；store 刷新成功后 feeds 列表与各源未读数会被更新
 */
async function refreshFeeds() {
  refreshing.value = true
  try {
    await feedsStore.refreshFeeds()
    if (feedsStore.refreshError) {
      await modalStore.showAlert({ title: '刷新订阅源', message: feedsStore.refreshError })
    }
  } catch (e) {
    console.error('Failed to refresh:', e)
  } finally {
    refreshing.value = false
  }
}

/**
 * 选中某个订阅源
 *
 * 先把选中态写入 store（ArticleColumn 的 watch 会据此 `invoke('articles_list')` 加载文章），
 * 再关闭窄屏抽屉让出正文区域。同时 fire-and-forget 触发一次静默自动刷新
 * （内部 5 分钟节流、全量刷新进行中跳过，见 feedsStore.autoRefreshOnSelect），
 * 让"点开一个源就看到它的最新文章"成为默认行为。
 *
 * @param feed - 被点击的订阅源对象
 * @returns 无返回值
 * @副作用 修改 feedsStore.selectedFeed；可能触发一次后台单源刷新；翻转 isOpen
 */
function selectFeed(feed: Feed) {
  // 标签与订阅源互斥：选源即退出标签视图，否则两个范围同时生效会让列表少掉一半内容
  articlesStore.setTagFilter(null)
  feedsStore.selectFeed(feed)
  // 刻意不 await：刷新是后台动作，选中的界面反馈（列表切换 / 抽屉收起）不能等它
  void feedsStore.autoRefreshOnSelect(feed.id)
  isOpen.value = false
  // 用户主动点击 → 胶囊逐格滑动过去。
  // 同时置标记：selectedFeed 变化会触发下面的 watch，而那条路径传的是"瞬时"，
  // 若不避开就会 cancelWalk() 把刚开始的逐格掐断（表现为"点了还是瞬移"）。
  // 胶囊同步统一交给下面的 watch（选中态的唯一监听点）：此处若自行同步，
  //读到的可能还是上一次的选中项，高亮会滞后一次点击。
  clickedRecently = true
}

/**
 * 点击后短期为真：**只用于决定动效，不用于跳过同步**
 *
 * 用户点击要"滑过去"，而折叠、命令面板跳转等变化只需瞬时落位。
 * watch 消费后立即复位，故不会把后续的真实变化误判为点击。
 *
 * 曾经的坑：早先用它让 watch **直接 return 跳过**，而点击回调里又自行同步——
 * 两边正好互换（点击回调早于 store 的 await、读到旧值），导致高亮滞后一次。
 * 现改为「只有 watch 一条路径 + 标记只决定动效」，选中态与胶囊永远同源。
 */
let clickedRecently = false

/**
 * 选中 / 取消选中一个标签（进入或退出标签视图）
 *
 * 再次点击当前标签即退出（回到"全部文章"），与「仅未读 / 书签」的开关式交互一致，
 * 避免用户只能靠顶部的 × 才能退出。
 *
 * @param tagId - 标签 ID
 * @returns 无返回值；副作用为清空源选择并更新 articlesStore.tagFilterId
 */
function selectTag(tagId: number) {
  if (articlesStore.tagFilterId === tagId) {
    articlesStore.setTagFilter(null)
  } else {
    // 先退出源选择再设标签：顺序反了会先触发一次"源 + 旧标签"的中间态加载
    feedsStore.clearSelectedFeed()
    articlesStore.setTagFilter(tagId)
  }
  isOpen.value = false
  // 与选订阅源同处理：标签行也是胶囊指示的对象，点击同样要滑过去。
  // 只登记意图，胶囊同步由 watch 统一完成（见 selectFeed 处的说明）。
  clickedRecently = true
}

/**
 * 添加订阅弹窗是否打开
 *
 * 弹窗自身的忙碌态（探测 RSSHub 实例可能数秒）由 FeedAddModal 的 saving 自持，
 * 宿主只负责开关；表单状态也在弹窗内，「取消」直接丢弃副本。
 */
const showAdd = ref(false)

/**
 * 新建文件夹
 *
 * 用全局 prompt modal 收集名称后交给 feedsStore.createFolder()（内部 `invoke('folders_create')`
 * 并 reload 文件夹列表）。名称缺省时不创建，直接返回。
 *
 * @returns 无返回值；可能新增一个文件夹并刷新侧边栏分组
 */
async function createFolder() {
  const name = await modalStore.showPrompt({
    title: '新建文件夹',
    message: '请输入文件夹名称：',
  })
  if (name && name.trim()) {
    feedsStore.createFolder(name.trim())
  }
}

/**
 * 切到主屏实体关系图视图
 *
 * 「更多」菜单里的入口：主屏（订阅栏右侧）从文章阅读切换为全宽关系图，
 * 与文章阅读互斥（见 uiStore.mainView）。与其它菜单项不同的是它**不打开弹窗**
 * ——视图切换立即生效，菜单先收起即可。
 *
 * @returns 无返回值；副作用为 uiStore.mainView = 'graph'、收起「更多」菜单
 */
function showGraphView() {
  showMoreMenu.value = false
  uiStore.setMainView('graph')
}

/**
 * 从智能摘要里打开一篇文章
 *
 * 直接委托 `articlesStore.selectArticle`（按 id 取回详情并标记已读），
 * 刻意**不**顺带切换订阅源：文章可能来自任意源，而切源会连带重置文章列表与排序，
 * 用户只是想读这一篇，不该看到中间那栏被换掉。
 *
 * @param article - 智能摘要中被点击的文章
 * @returns 无返回值；副作用为更新选中态，正文栏随即渲染该文章
 */
async function openArticleFromDigest(article: Article) {
  await articlesStore.selectArticle(article.id)
}

/**
 * 打开订阅源管理弹窗
 *
 * 只需装入目标 Feed：名称 / 链接 / 文件夹副本的回填由 FeedManageModal
 * watch(:feed) 完成。由 feed 条目上的「⋯」按钮以 `@click.stop` 触发，
 * 故不会连带触发"选中订阅源"。
 */
function openManage(feed: Feed) {
  managingFeed.value = feed
}

/**
 * 关闭订阅源管理弹窗
 *
 * 置空 `managingFeed` 即让 FeedManageModal 的 `open` 不成立（BaseModal 隐藏面板）；
 * 副本与错误提示由弹窗组件在下一次打开时自行重置。
 */
function closeManage() {
  managingFeed.value = null
}

/**
 * 响应菜单按钮的面板开合请求
 *
 * 作为 `toggle-feed-panel` 事件的处理器：面板开合是纯展示态，
 * 放到 store 里会成为无业务含义的全局状态，故留在组件内。
 *
 * @returns 无返回值
 * @副作用 翻转 isOpen
 */
function handleToggleFeedPanel() {
  isOpen.value = !isOpen.value
}

/**
 * 响应命令面板的跳转：把胶囊滑到目标订阅源并播一次定位脉冲
 *
 * 目标源可能落在**已折叠**的分组里，故必须先展开那一组再测位置——
 * 否则量到的是 0 高度，胶囊会滑到列表顶部。
 *
 * @param e - `command-palette-jump` 事件，detail 含 feedId
 * @returns 无返回值
 */
async function onPaletteJump(e: Event) {
  const feedId = (e as CustomEvent<{ feedId: number }>).detail?.feedId
  if (typeof feedId !== 'number') return

  // 找出目标源所属分组，展开它（若已展开则 next 保持原样，Set 替换仍会触发一次重算）
  const target = feedsStore.feeds.find((f) => f.id === feedId)
  if (target) {
    const key = target.folder_id ? 'f' + target.folder_id : 'uncat'
    if (collapsed.value.has(key)) {
      const next = new Set(collapsed.value)
      next.delete(key)
      collapsed.value = next
    }
    // 展开动画要跑完（280ms），胶囊才量得到最终位置；这里等一小段再落位
    await new Promise((r) => setTimeout(r, 300))
  }

  const list = feedListRef.value
  const capsule = feedCapsule.value
  if (!list || !capsule) return
  const el = list.querySelector<HTMLElement>(`.feed-item[data-feed-id="${feedId}"]`)
  if (!el) return
  capsule.sync(el)
  capsule.pulse()
}

/**
 * 注册面板开合的事件监听
 *
 * 监听挂在 window 上（而非常规的模板事件），是因为派发方（标题栏菜单按钮）是互不关联的兄弟组件。
 * 选择在 onMounted 注册而非模块顶层执行，可确保组件销毁前能成对移除。
 *
 * @returns 无返回值
 * @副作用 在 window 上注册 `toggle-feed-panel` 监听
 */
onMounted(() => {
  // 侧边栏分组依赖文件夹列表，挂载时拉取一次（失败仅记日志，不阻塞订阅源展示）
  feedsStore.loadFolders()
  // 标签分区同样需要在挂载时取一次目录；失败仅在 store 里记 error，不影响订阅源列表
  tagsStore.loadTags()
  window.addEventListener('toggle-feed-panel', handleToggleFeedPanel)
  window.addEventListener('command-palette-jump', onPaletteJump)

  // 列表内容是异步到达的（feeds/folders 都走 invoke），首帧没有条目可测量。
  // watch 选中态而非一次性调用：无论是挂载时的首绘、还是启动即带选中态的恢复场景，
  // 都会在数据到达那一次触发同步。
  watch(
    () => [feedsStore.feeds, feedsStore.selectedFeed, collapsed.value, articlesStore.tagFilterId] as const,
    () => {
      // 统一在此同步胶囊：此时选中态与 DOM 的 active 类都已更新，
      // 量到的必然是本次真正选中/变化的那一项，故不会滞后也不会错位。
      const sliding = clickedRecently
      clickedRecently = false
      void nextTick(() => syncCapsule(sliding))
    },
    { immediate: true, deep: false },
  )
})

/**
 * 清理事件监听
 *
 * 必须与 onMounted 中注册的监听成对移除，否则组件卸载后处理器仍持有组件闭包，
 * 既会造成内存泄漏，也会在重复挂载时让一次点击触发多次翻转。
 *
 * @returns 无返回值
 * @副作用 从 window 上移除 `toggle-feed-panel` 监听
 */
/**
 * 处理文档级点击：点击更多菜单外部时关闭下拉
 *
 * @param e - 鼠标事件
 */
function onDocClick(e: MouseEvent) {
  if (!showMoreMenu.value) return
  const target = e.target as Node | null
  if (!target) return
  // 点击菜单根或其子元素时不关闭
  if (moreMenuRef.value?.contains(target)) return
  showMoreMenu.value = false
}

onMounted(() => {
  document.addEventListener('click', onDocClick)
})

/**
 * 订阅源后台抓取失败提示
 *
 * 修改链接后后台重抓失败（地址写错 / 站点不可达）只会体现在事件里，
 * 不弹提示的话用户完全无从察觉链接没生效。用 watch 而非在 store 内直接弹窗，
 * 是为了让 store 保持纯状态、提示形式统一由界面决定。
 */
watch(
  () => feedsStore.feedError,
  async (msg) => {
    if (!msg) return
    await modalStore.showAlert({ title: '订阅源抓取失败', message: msg })
    feedsStore.clearFeedError()
  }
)

onUnmounted(() => {
  window.removeEventListener('toggle-feed-panel', handleToggleFeedPanel)
  window.removeEventListener('command-palette-jump', onPaletteJump)
  document.removeEventListener('click', onDocClick)
})
</script>

<style scoped>
.feed-panel {
  /* 列宽由拖拽分隔条控制（App.vue 写入 --feed-w 并持久化到 localStorage）。
     不能写死像素值：scoped 选择器带 data-v 属性，特异性压过 styles.css 的
     全局 `.feed-panel { width: var(--feed-w) }`，硬编码会让拖拽形同虚设。 */
  width: var(--feed-w, 260px);
  /* 玻璃化：半透明 + 背景模糊 + 上边缘内高光。厚度感靠 --glass-hi 那条内阴影，
     不靠投影——栏间窄缝会同时承接相邻面板的投影，投影稍重就在缝里叠成深色凹槽。 */
  background: var(--glass);
  backdrop-filter: blur(var(--blur)) saturate(1.5);
  -webkit-backdrop-filter: blur(var(--blur)) saturate(1.5);
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  /* 顶部让位由父级 .app-body 的 padding 统一负责，此处不再重复添加，
     否则会形成两层叠加的内边距（两处各一个 --tb-h，视觉上出现大片空白） */
  height: 100%;
  overflow: hidden;
  border-radius: var(--r-panel);
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    inset 0 -1px 0 var(--glass-lo),
    0 2px 6px rgba(31, 45, 70, 0.05),
    0 10px 30px rgba(31, 45, 70, 0.07);
  border: 1px solid var(--glass-hair);
}

.feed-search-wrap {
  padding: var(--sp-3) var(--sp-3) var(--sp-2);
}
.feed-search { position: relative; }
.feed-search input {
  width: 100%;
  height: 30px;
  border: none;
  border-radius: var(--r-md);
  padding: 0 var(--sp-3) 0 30px;
  font-size: var(--fs-sm);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: background 0.15s;
}
.feed-search input:focus {
  background: var(--cap-feed);
  box-shadow: 0 0 0 1px var(--primary);
}
.feed-search input::placeholder { color: var(--text-tertiary); }
.feed-search .s-icon {
  position: absolute;
  left: 8px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-tertiary);
  pointer-events: none;
}

/* 订阅列表滚动区：右侧被滚动条占掉 --sb-w，故左侧补同等宽度做配重，
   让订阅源项的 hover / 选中背景在左右两侧留出等宽空白（否则视觉偏左）。
   滚动条风格统一收敛到 styles.css 全局规则，此处不再单独重写。

   position: relative 是滑动胶囊的定位基准：胶囊测的是条目的 offsetTop，
   两者必须在同一坐标系里，否则胶囊会按错误的基准偏移。 */
.feed-list {
  position: relative;
  flex: 1;
  overflow-y: auto;
  padding: var(--sp-1) var(--sp-2) var(--sp-1) calc(var(--sp-2) + var(--sb-w, var(--sp-1)));
  /* 胶囊默认左右各留 8px，与条目自身 padding 对齐 */
  --cap-left: var(--sp-2);
  --cap-right: var(--sp-2);
}

/* 加载态骨架屏：按真实 .feed-item 的几何铺排（28px 圆形头像 + 右侧两行文字），
   间距与 .feed-item 的 padding / gap 完全一致，保证加载完成时列表不跳版。
   两行文字模拟「订阅源名 + 未读数」，第二行用短条制造长度差。 */
.skeleton-row {
  /* 行的 display/align 来自全局基座（styles.css），这里只补与 .feed-item 对齐的几何 */
  gap: var(--sp-4);
  padding: var(--sp-2) var(--sp-15);
  /* 与 .feed-item 的 margin-bottom 保持一致：骨架屏行间距若比真实行松，
     加载完成时整列会往上收，肉眼可见跳动 */
  margin-bottom: var(--sp-025);
}
.sk-avatar {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  border-radius: 8px;
  background: var(--fill-secondary);
  animation: sk-shimmer 1.4s ease-in-out infinite;
  background-size: 200% 100%;
  background-image: linear-gradient(
    90deg,
    var(--fill) 0%,
    var(--fill-secondary) 50%,
    var(--fill) 100%
  );
}
.sk-lines {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
}

/* ─── 文件夹分组 ─────────────────────────────────────────── */
.folder-group { margin-bottom: var(--sp-025); }
/* 折叠容器：用 grid-template-rows: 1fr→0fr 做高度动画，
   无需测量真实内容高度（groupedFeeds 每次都会产出新数组，测高度既贵又易错）。
   内层 div 必须有 overflow:hidden 才会被 0fr 真正压扁。
   展开 / 折叠互为逆过程，复用同一条声明。 */
.folder-body {
  display: grid;
  grid-template-rows: 1fr;
  transition: grid-template-rows 0.28s var(--ease-slide);
}
.folder-body > div { overflow: hidden; min-height: 0; }
.folder-group.collapsed .folder-body { grid-template-rows: 0fr; }
/* 折叠时子项淡出：单纯压扁会让文字"被挤扁"，与淡出叠加才像收起 */
.folder-body > div > * { transition: opacity 0.16s linear; }
.folder-group.collapsed .folder-body > div > * { opacity: 0; }

.folder-head {
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  padding: var(--sp-15) var(--sp-2) var(--sp-1);
  cursor: pointer;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.03em;
}
.folder-head:hover { color: var(--text-secondary); }
/* 折叠箭头：改用 SVG 图标（原先用 ▸ / ▾ 字符，宽高随字体渲染不稳定，
   水滴需要相对它定位）。折叠时旋转 90° —— 用 transition 而非换字符，跳变更像物理开合。 */
.folder-caret {
  position: relative;
  width: 12px; height: 12px; flex-shrink: 0;
  display: flex; align-items: center; justify-content: center;
}
.folder-caret svg {
  width: 9px; height: 9px;
  transition: transform var(--dur) var(--ease);
}
.folder-group.collapsed .folder-caret svg { transform: rotate(-90deg); }

/* 水滴：从折叠点分裂后坠落淡出。与高度动画同步发生，是全文唯一使用该动效的地方。 */
.folder-drop {
  position: absolute;
  left: 50%; top: 50%;
  width: 5px; height: 5px;
  margin: -2.5px 0 0 -2.5px;
  border-radius: 50%;
  background: var(--primary);
  pointer-events: none;
  animation: folder-drop-split 0.46s var(--ease-slide) forwards;
}
@keyframes folder-drop-split {
  0%   { transform: translate(0, 0) scale(0.3); opacity: 0; }
  22%  { opacity: 0.9; }
  100% { transform: translate(var(--dx), var(--dy)) scale(0.85); opacity: 0; }
}
/* 无障碍：关掉动效后水滴不播，但折叠本身仍有过渡，交互不受影响 */
@media (prefers-reduced-motion: reduce) {
  .folder-drop { animation: none; opacity: 0; }
}
.folder-name { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

/* 标签行：与 .feed-item 同构，但首元素是可着色的色点而不是头像
   —— 标签没有图标，用自身颜色做视觉锚点最直接。
   选中态底色与指示条已移除：改由滑动胶囊统一表达，此处只留文字主色。 */
.tag-row {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-15);
  margin-bottom: var(--sp-025);
  border-radius: var(--r-md);
  cursor: pointer;
  transition: background var(--dur) var(--ease);
}
/* hover 底色已移除：与 .feed-item 同理由 hover 胶囊接管（见 SelectCapsule.vue） */
.tag-row:hover { background: transparent; }
.tag-row.active { background: transparent; }
.tag-row-dot {
  width: 8px;
  height: 8px;
  border-radius: var(--r-pill);
  flex-shrink: 0;
}
.tag-row-name {
  flex: 1;
  min-width: 0;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tag-row.active .tag-row-name { color: var(--cap-active-fg); font-weight: 600; }
/* 选中态未读点取琥珀，与订阅源条目的选中文字同源 */
.tag-row.active .tag-row-dot { box-shadow: 0 0 0 2px color-mix(in srgb, var(--brand-from) 28%, transparent); }

/* ─── 底部工具条（单行图标） ─────────────────────────────────────────────────
   替代原先 4 个整行按钮（~170px）：一行 ~46px，列表区多出约 130px 纵向空间。
   图标按钮全部带 title 悬浮提示弥补去掉的文字标签。 */
.feed-toolbar {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-2) var(--sp-3);
  border-top: 1px solid var(--glass-hair);
  /* 透明：面板已是玻璃，实色底会把玻璃截断成上下两段。
     上边缘补一道内高光，代替原先靠 --surface 拉开的那层关系。 */
  box-shadow: inset 0 1px 0 var(--glass-hi);
  background: transparent;
}
.tool-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--sp-1);
  min-width: 30px;
  height: 30px;
  padding: 0 var(--sp-2);
  border: none;
  border-radius: var(--r-md);
  background: none;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all 0.15s;
  font-family: inherit;
}
.tool-btn:hover {
  background: var(--fill);
  color: var(--text-primary);
}
/* 「···」菜单展开态：填充高亮 + 主色，提示菜单当前打开（再点收起） */
.tool-btn.active {
  background: var(--fill-secondary);
  color: var(--primary);
}
.tool-btn svg {
  width: 15px;
  height: 15px;
  flex-shrink: 0;
}
.tool-spacer {
  flex: 1;
}
/* 刷新进行中：图标旋转 + n/m 进度小字（进度来自后端逐源广播，见 progressText） */
.tool-btn.refreshing svg {
  animation: spin 1s linear infinite;
}
@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
.refresh-progress {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  /* 进度数字用等宽数字：n/m 跳动时宽度稳定不抖 */
  font-variant-numeric: tabular-nums;
}

/* ─── 「更多」下拉菜单 ─────────────────────────────────────────────────────── */
.more-menu-wrap {
  position: relative;
}
.more-dropdown {
  position: absolute;
  bottom: calc(100% + 8px);
  /* 左对齐而不是居中：触发按钮「···」贴近面板左缘，160px 宽的菜单若以按钮
     为中心会向左溢出面板（.feed-panel overflow:hidden 直接裁掉） */
  left: 0;
  min-width: 160px;
  background: var(--surface);
  border: 1px solid var(--panel-border);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-pop);
  padding: var(--sp-05);
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
  z-index: 300;
}
.dropdown-item {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
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
  width: 100%;
}
.dropdown-item:hover {
  background: var(--fill-secondary);
  color: var(--primary);
}
.dropdown-item svg {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
}
</style>
