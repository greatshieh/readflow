<template>
  <!-- 研究工作台弹窗外壳：四个页签（时间线 / 报告 / 实体 / 论点）的容器与切换。
       layout="side"：固定高度弹窗，切页签不再跳变；tab 竖排在左列。
       关系图已不在此处——弹窗画布太小，只保留主屏全宽展示（入口见 FeedPanel 工具栏与 D 键）。 -->
  <BaseModal :open="open" title="研究工作台" layout="side" side-wide @close="close">
    <!-- 左列：标题 + 纵排页签。关闭钮由 BaseModal 的 side 布局统一放在面板右上角 -->
    <aside class="side-nav">
      <div class="side-head">
        <h3>研究工作台</h3>
      </div>
      <nav class="side-tabs">
        <p class="side-group">台账</p>
        <button class="side-tab" :class="{ active: tab === 'timeline' }" @click="tab = 'timeline'">
          事件时间线
        </button>
        <button class="side-tab" :class="{ active: tab === 'reports' }" @click="tab = 'reports'">
          报告
        </button>
        <button class="side-tab" :class="{ active: tab === 'entities' }" @click="tab = 'entities'">
          实体管理
        </button>
        <!-- 「覆盖」与「实体管理」**平级**（这次改版的结论，别再拆回去）。
             曾被做成实体页签内的第二个视图 + 侧栏子项，理由是"两者共用同一批实体数据"。
             那个理由在数据层成立、在界面层不成立：覆盖只能从已有实体里选，只说明它
             需要一个"去加标的"的空态出口，不说明它该和实体管理挤在一个页签里。
             两个任务是不同的心智——实体管理是**配置**（系统盯谁，低频一次），
             覆盖是**台账/决策记录**（为什么盯、何时退出，反复回看）。
             把记录藏在设置的第二个视图里，等于每次回看都要先进设置；
             而且论点已是独立页签，覆盖与它同族（都是"我写下的判断"），
             旧的从属结构把同族的东西拆在了两个层级。
             ⚠ 「覆盖」的 active 只看 tab，不再有 entityView 这个二级状态。 -->
        <button class="side-tab" :class="{ active: tab === 'coverage' }" @click="tab = 'coverage'">
          覆盖
        </button>
        <!-- 论点放在最后而不是最前：时间线是全应用的入口（每天开机先看它），
             论点是自己写下的判断，频率低但深度高。它紧邻「覆盖」是拆分后的意外收益——
             两者同族（都是"我写下的判断"：覆盖记标的的持有逻辑，论点记判断本身），
             现在它们终于相邻了，而旧结构里覆盖藏在实体管理下面就做不到 -->
        <button class="side-tab" :class="{ active: tab === 'theses' }" @click="tab = 'theses'">
          论点
          <span v-if="thesesStore.stats.challenged > 0" class="side-dot" aria-hidden="true"></span>
        </button>

        <!-- 「工作流」不是页签组：复盘是独立主屏视图（有会话状态，PRD-05 §1），
             组装是只读预览弹窗（§0）。列在这里是因为用户会到工作台里找它们；
             它们不高亮、不带 active 态——这正是"它们不装在这个容器里"的诚实表达。
             与其让用户翻遍页签找不到，不如摆出来并在点击时送到正确的地方。 -->
        <p class="side-group">工作流</p>
        <button class="side-tab action" type="button" @click="onGotoReview">
          复盘
          <span class="side-jump" aria-hidden="true">↗</span>
        </button>
        <button class="side-tab action" type="button" @click="openCompose()">组装</button>
      </nav>
    </aside>

    <!-- 右列：固定标题栏 + 滚动内容区 -->
    <div class="side-main">
      <!-- 固定标题栏：页签名称居中，关闭按钮在右侧，不随内容滚动 -->
      <div class="m-titlebar">
        <span class="m-titlebar-title">{{ tabTitle }}</span>
        <button class="btn btn-icon btn-quiet" @click="close" aria-label="关闭" title="关闭">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
          </svg>
        </button>
      </div>

      <!-- 报告页签内的两段式分段控件。
           位置在 `.m-body` 之外（`.side-main` 层、标题栏之下）：`.m-body` 自己带
           padding 与 overflow-y:auto，分段塞进去会变成"内层再来一份 padding + 跟着内容滚走"。
           只有报告页签显示它，且两段内容常驻（v-show），切回来生成参数不丢。 -->
      <ReportSegmentBar v-if="tab === 'reports'" />

      <!-- 右列：页签内容区（页签根元素都带 class="m-body"，占满右列；
           覆盖的根是 `.cv`，由外壳套一层 `.m-body` 容器，见下） -->
      <!-- 五个页签一律 v-show 常驻挂载（不是 v-if）：各页签的筛选参数
           （时间窗 / 实体筛选 / 报告类型与天数）在拆分前的单组件里
           天然跨页签、跨开关持久，v-if 会在每次切换时把这些状态连同 DOM 一起丢掉。
           页签各自的加载时机由它们 watch `active` / `open` 自行驱动，见各组件文件头。
           注：覆盖内部曾用 `v-if` 而不是 `v-show`，理由是"没打开过就不该白跑一次查询"。
           现在它自己就是一个页签，这个取舍由它自己的 watch 承担（见 CoveragePanel 文件头）。 -->
      <ResearchTimelineTab v-show="tab === 'timeline'" :open="open" @close="close" />
      <ResearchReportsTab
        v-show="tab === 'reports'"
        :active="tab === 'reports'"
        :open="open"
        @close="close"
      />
      <ResearchEntitiesTab v-show="tab === 'entities'" @entities-changed="onEntitiesChanged" />
      <!-- 覆盖：与实体管理平级的页签。根节点套 `.m-body` 是本弹窗的约定——
           外壳的 scoped 规则按这个类名给内边距与滚动，页签组件自身的根元素
           通常直接带它；CoveragePanel 的根是 `.cv`（它还要能被单独复用），
           所以这里由外壳补一层容器 -->
      <div v-show="tab === 'coverage'" class="m-body">
        <CoveragePanel :active="tab === 'coverage'" @goto-entities="tab = 'entities'" />
      </div>
      <ThesisPanel
        v-show="tab === 'theses'"
        @open-article="onOpenThesisArticle"
        @goto-review="onGotoReview"
      />
    </div>

    <!-- 组装预览：叠在本弹窗之上（BaseModal 的 dialogStack 支持多层，Esc 先关上层）。
         内容是一页 markdown，复制的是原文——与「研究一页」共用同一个预览壳。
         放在 `side-main` 之外、`BaseModal` 之内：它是本弹窗的附属产物，
         不该受右列滚动容器约束，也不该在切页签时被重建。 -->
    <MarkdownPreviewModal
      :open="composeOpen"
      :title="composeTitle"
      :markdown="composeMarkdown"
      :loading="composeLoading"
      :error="composeError"
      :default-name="composeDefaultName"
      hint="这一页是骨架：标题 + 判据 + 正反证据 + 变更记录。复制的是 markdown 原文，粘到 Obsidian 里仍可重新排版。"
      @close="composeOpen = false"
    />
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 研究工作台弹窗组件（外壳）
 *
 * # 职责
 * 只负责弹窗外壳与页签切换：BaseModal 容器、头部、tab 条，以及四个页签组件
 * （`ResearchTimelineTab` / `ResearchReportsTab` / `ResearchEntitiesTab` /
 * `ThesisPanel`）的编排。
 * 页签内部的表单状态、数据加载与临时提示都在各页签组件里（见各文件头的"挂载语义"一节）。
 *
 * # 关系图为何不在这里
 * 弹窗内的画布太小，看不出长程结构，故关系图只在主屏全宽展示（`GraphView`，
 * 由 `uiStore.mainView` 驱动）；入口是订阅栏底部工具栏按钮与全局 `D` 键。
 *
 * # 父组件通信
 * - `open` prop：控制弹窗显示状态
 * - `@close` 事件：通知父组件关闭弹窗
 *
 * # 跨页签协调（外壳仅有的两处编排逻辑）
 * 1. 弹窗打开时确保实体列表已加载——时间线的筛选下拉与实体页签共用这份数据；
 * 2. 实体页签的增删/启停/评分会让已加载的关系图过期：实体页签 emit
 *    `entities-changed`，外壳据当前主屏视图决定"立即重算"还是"作废待重算"
 *    （见 {@link onEntitiesChanged}）。
 */
import { ref, watch, computed } from 'vue'
import BaseModal from './BaseModal.vue'
import ResearchTimelineTab from './ResearchTimelineTab.vue'
import ResearchReportsTab from './ResearchReportsTab.vue'
import ResearchEntitiesTab from './ResearchEntitiesTab.vue'
import CoveragePanel from './coverage/CoveragePanel.vue'
import ThesisPanel from './thesis/ThesisPanel.vue'
import ReportSegmentBar from './report/ReportSegmentBar.vue'
import MarkdownPreviewModal from './MarkdownPreviewModal.vue'
import { useResearchStore } from '@/stores/research'
import { useEntitiesStore } from '@/stores/entities'
import { useUiStore } from '@/stores/ui'
import { useReportPeriodStore, FocusReportError } from '@/stores/reportPeriod'
import { useThesesStore } from '@/stores/theses'
import { consumeThesisOpen } from '@/utils/thesisJump'
import { useArticlesStore } from '@/stores/articles'
import { useToastStore } from '@/stores/toast'

const props = defineProps<{
  /** 弹窗是否打开 */
  open: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const researchStore = useResearchStore()
const entitiesStore = useEntitiesStore()
const uiStore = useUiStore()
const reportStore = useReportPeriodStore()
const thesesStore = useThesesStore()
const articlesStore = useArticlesStore()
const toast = useToastStore()

/** 当前 tab：timeline = 事件时间线；reports = 报告（含周期报告与研究报告两段）；
 *  entities = 实体管理（配置：盯谁）；coverage = 覆盖（台账：为什么盯、何时退出）；
 *  theses = 论点台账 */
const tab = ref<'timeline' | 'reports' | 'entities' | 'coverage' | 'theses'>('timeline')

/** 组装预览：是否打开、内容、加载与失败态 */
const composeOpen = ref(false)
const composeMarkdown = ref('')
const composeLoading = ref(false)
const composeError = ref('')
const composeTitle = ref('组装')
/** 另存为的默认文件名；实体名可能含 `/` `:` 等不能进文件名的字符，落盘前先净化 */
const composeDefaultName = ref('论点.md')

/** 标题栏显示的页签名称 */
const tabTitle = computed(() => {
  const titles: Record<string, string> = {
    timeline: '事件时间线',
    reports: '报告',
    entities: '实体管理',
    // 覆盖页签内部不再自带标题（CoveragePanel 已删掉那行 h3），页签名只在这里出现一次
    coverage: '覆盖',
    theses: '论点',
  }
  return titles[tab.value] ?? '研究工作台'
})

// 打开时确保实体列表已加载：时间线的实体筛选下拉依赖它（可能尚未加载过）
// 同时消费"上下文栏点了返回"的待办意图（跨组件跳转，见 DESIGN §4.4）。
watch(
  () => props.open,
  async val => {
    if (!val) return
    // ── ① 这两步不依赖任何 IPC，必须排在所有 await 之前 ────────────────
    // `consumePendingOpen()` + `tab = 'reports'` 一旦排到下面 `loadEntities()` 之后，
    // 就会晚一个真 IPC（宏任务量级）。而同一次 pre-flush 里，子组件
    // `PeriodReportReader` 的恢复 watch 已经在等 DOM 了——它这时读到的报告页签仍是
    // `v-show=false`（`display: none`）：
    //   · `.rd-doc-wrap` 没有布局盒 → `scrollTop = N` 被钳成 0（空操作）；
    //   · 两个 rect 全 0 → "是否在视野内"恒为真 → 也不会补滚。
    // 结果就是"报告回来了，但停在顶部、焦点没落 `[n]`"，且快照已被消费，无从补救。
    // 触发条件是 `entitiesStore.entities.length === 0`（实体库为空的用户 / 列表加载
    // 失败过的用户）——少数人 100% 复现，正是最难查的那类。
    // 排到 await 之前，页签就在**当前这次 flush 内**变成可见，恢复动作随后一定能拿到
    // 有布局的 DOM（推理见 `PeriodReportReader` 的恢复 watch 注释）。
    const pending = reportStore.consumePendingOpen()
    if (pending !== null) {
      tab.value = 'reports'
      reportStore.setSegment('period')
    }

    // 命令面板点了「判断 / 证据」命中：落到论点页签并打开那一条。
    // 与上面的报告意图同构，也必须在 await 之前把页签切好——
    // 晚一步的话用户先看到报告列表再"跳"过去，闪一下。
    // 与报告二选一：一次打开只有一个落点，同时来两个时报告优先（它是显式返回动作）。
    const pendingThesis = pending === null ? consumeThesisOpen() : null
    if (pendingThesis !== null) {
      tab.value = 'theses'
    }

    if (entitiesStore.entities.length === 0) {
      await entitiesStore.loadEntities()
    }
    // 论点页签的小红点依赖 stats，而 stats 来自列表 → 打开时先拉一次，
    // 否则用户看不到"有反证要处理"这个信号（列表页签他自己会再拉，成本很小）
    if (thesesStore.list.length === 0) void thesesStore.loadList()

    // ── ② 真正打开报告：这里才需要列表与实体就绪 ──────────────────────
    if (pending !== null) {
      try {
        await reportStore.focusReport(pending)
        // ── 送达即回收（本次改动的唯一清除落点）──
        // `pendingOpenId` 全仓库只有 `backToReport()` 一个生产者，"消费了待打开意图
        // 并成功打开"与"一次返回被送达"是一对一的。清除放在这里而不是
        // `backToReport`（同步、不知道成败）或 `focusReport`（不该知道"返回"这个概念），
        // 才既不会"先清再试"，也不会误伤将来别的调用者。
        // `reportId === pending` 是防御：哪天多一个 `requestOpen` 的生产者，
        // 这道判断保证它不会顺手清掉别人的上下文。
        if (reportStore.sourceContext?.reportId === pending) {
          reportStore.clearSourceContext()
        }
      } catch (e) {
        // 位置快照必须就地消费：这条路径上阅读页没被打开（或已被 `closeReport`
        // 卸载），没有人会去消费它。留在 store 里会被下一次打开**任意**报告的
        // `immediate` 恢复命中 → 用户看到"打开报告 9 却被滚到 480、还闪了第 12 条"。
        reportStore.consumeRestore()
        // 两类失败必须分开处理，不能一律判死：
        // · 报告**真的**不存在（被删 / 被覆盖重生成）→ 转终态失效，栏留在原地；
        // · 列表加载失败等临时故障 → **不标 stale**，`ctx` 保留，用户还能再点一次重试。
        //   `stale` 是终态、只有 ✕ 能收，把"网络抖了"标成终态等于把病根换个形式重演。
        if (isReportMissing(e)) {
          // 关键：**不清 `ctx`**，只把它切成失效态——栏留在原地（disabled + ✕），
          // 用户至少还有一个"这条入口"可点、可关；清掉就等于"失败即失联"，
          // 正是本次要消掉的病（DESIGN §4.4 规则 #4）。三者并存：
          // 退回列表 + toast 说明原因 + 栏转失效态。
          reportStore.markSourceStale('这份报告已不存在')
        }
        // 退回列表：两种失败下都让用户能自己重选一份（报告不在时尤其必要）
        reportStore.closeReport()
        toast.show(e instanceof Error ? e.message : '该报告已不存在')
      }
    }

    // ── ③ 命令面板要落到的判断（页签已在 ① 切好）──
    // 放在全部 await 之后：列表与实体已就绪，`openThesis` 拉详情的成功率最高，
    // 且此时页签可见，详情一渲染就有布局盒（不会量出 0 高度）
    // thesisId 为 null 表示"只切页签"（取自一条还没挂到任何论点的高亮），
    // 此时不打开任何详情——用 0 去打开会打一条查询不存在的记录
    if (pendingThesis !== null && pendingThesis.thesisId !== null) {
      void thesesStore.openThesis(pendingThesis.thesisId)
    }
  },
  // immediate：组件若在 `open=true` 的状态下挂载（外壳改成 v-if 就会发生），
  // 不带 immediate 的 watch 不会为"挂载时已成立的状态"补一次，待打开的意图会丢掉
  { immediate: true },
)

/**
 * 这次失败是不是"报告真的没了"
 *
 * 优先读错误类型上的 `kind`（`FocusReportError` 由 `focusReport` 抛出）——
 * 文案会变、类别不会。字符串比对只作兜底：将来若有别处抛同样的旧文案，行为不变。
 *
 * @param e - `focusReport` 抛出的异常
 * @returns 报告已不存在时 true；列表加载失败等临时故障返回 false
 */
function isReportMissing(e: unknown): boolean {
  if (e instanceof FocusReportError) return e.kind === 'missing'
  return e instanceof Error && e.message === '该报告已不存在'
}

/**
 * 关闭弹窗
 *
 * 生成期间**拦下关闭**（决策⑬：P0–P2 先阻塞），但必须给一次反馈——
 * 三条关闭路径（Esc / 点遮罩 / 点 ✕）全被静默吃掉时，用户会以为应用卡死。
 *
 * # 这里**只关弹窗**，不重置任何内部状态
 * 不要顺手写 `tab = 'timeline'` 或 `closeReport()`：DESIGN §4.6.2 明确要求
 * 「上次停留处」保留（"刚关掉又打开，期望回到同一份；每次都回列表等于每次重新找"），
 * §4.6.4 的退出表里 Esc / 点遮罩 / 点 ✕ 三者的副作用只有「焦点归还触发元素」，
 * 不含任何状态清理。
 *
 * 页签与报告面板的状态本来就能跨开关存活（`BaseModal` 无 `v-if` + 三个页签 `v-show`
 * 常驻，见 §4.6.1 的那条隐式契约），**清掉才是破坏**。误点遮罩的代价因此只剩
 * "重新打开一次"，而不是"页签归零 + 报告正文清空 + 找回原来那份"。
 *
 * 配套改动：报告列表的刷新时机原本挂在「切入页签」（`active` 翻真）上，页签不再
 * 复位后重开弹窗时 `active` 不变、不会触发刷新。故 `ResearchReportsTab` 的刷新
 * 条件已扩为 `active || open`，见该组件对应 watch 的注释。
 */
function close() {
  if (reportStore.generating) {
    toast.show('正在生成报告，请稍候')
    return
  }
  emit('close')
}

/**
 * 实体页签变更实体（增删 / 启停 / 评分）后，让已加载的关系图失效
 *
 * 关系图现在只在主屏全宽展示（弹窗内已无图页签），故这里按当前主屏视图分流：
 * - 主屏正显示关系图（`mainView === 'graph'`）→ 立即重算，用户关掉弹窗即看到
 *   与新实体集合一致的图；
 * - 否则把已加载的图作废（`invalidateGraph`）→ 下次进入主屏图视图时由
 *   `GraphView` 的 onMounted 重算，避免用户没在看图时白跑一次全库扫描。
 *
 * @returns 无返回值；副作用为触发一次重算或作废图数据
 */
function onEntitiesChanged() {
  if (uiStore.mainView === 'graph') void researchStore.reloadGraph()
  else researchStore.invalidateGraph()
}

/**
 * 论点详情里点了「打开原文」
 *
 * 复刻报告页签的做法：关弹窗 → 切主屏阅读区 → 选中该文章。
 * 不在这里做"就地预览"是因为论点详情已经占满右列，再叠一层预览
 * 会把"证据快照"和"原文"挤在同一个窄栏里，两边都不好看。
 *
 * @param articleId - 文章 ID
 */
async function onOpenThesisArticle(articleId: number) {
  emit('close')
  uiStore.setMainView('articles')
  await articlesStore.selectArticle(articleId)
}

/**
 * 论点页签里点了「本周复盘」
 *
 * 复盘是独立主屏视图（PRD-05：它有会话状态，放弹窗里会让 Esc 语义错配），
 * 所以这里必须**先关弹窗、再切视图**：
 * 顺序反了会短暂出现"弹窗浮在复盘页上"的中间态，而那个中间态里
 * 卡片既在弹窗遮罩下不可点、又不在侧栏路径上，用户只能按 Esc——
 * 而 Esc 同时是"关弹窗"与"退出视图"，一次按键退两层。
 *
 * 用 `setMainView('review')` 而非 `toggleReviewView()`：这里表达的是确定意图，
 * 若用户恰好在复盘页打开研究工作台再点这个按钮，toggle 会把他送回文章列表
 * ——"点了去复盘，结果到了文章列表"是最难解释的一类 bug。
 *
 * @returns 无返回值；副作用为关弹窗并切到复盘视图（会话状态由 review store 持有，不重置）
 */
function onGotoReview() {
  emit('close')
  uiStore.setMainView('review')
}

/**
 * 打开「组装」预览
 *
 * # 为什么不做"先选一条论点"的选择器
 * 组装的对象几乎总是**刚看过的那条**判断。多一层选择弹窗，等于为了低频的
 * "换一条导出"给高频路径加一次点击。这里的目标顺序是：
 * 显式传入的 id（将来论点详情里的按钮） → 最近打开的论点 → 列表第一条
 * （列表已按"观察中优先、最近动作倒序"，第一条就是最该处理的）。
 *
 * 预览是只读的，选错了代价只是关掉重来——这个代价远小于多一层弹窗。
 *
 * @param thesisId - 可选的显式目标；不传时按上面的顺序推导
 * @returns 无返回值；副作用为打开预览并异步取回 markdown
 */
async function openCompose(thesisId?: number) {
  const target = thesisId ?? thesesStore.detail?.thesis.id ?? thesesStore.list[0]?.id
  if (target === undefined) {
    toast.show('还没有论点：先在「论点」页签写一条判断，再来组装。')
    return
  }

  composeOpen.value = true
  composeLoading.value = true
  composeError.value = ''
  composeMarkdown.value = ''

  // 标题先按已知信息给出来，别等 IPC 回来再改——否则弹窗标题会跳一下
  const known =
    thesesStore.detail?.thesis.id === target
      ? thesesStore.detail.thesis
      : thesesStore.list.find(t => t.id === target)
  composeTitle.value = known ? `组装：${known.entityName}` : '组装'
  // 文件名只保留实体名：判断原文可能很长且带标点，做文件名会变成一长串乱码
  composeDefaultName.value = known
    ? `论点-${known.entityName.replace(/[\\/:*?"<>|]/g, '_')}.md`
    : '论点.md'

  try {
    composeMarkdown.value = await thesesStore.composeMarkdown(target)
  } catch (e) {
    // Tauri 的 reject 是**字符串**不是 Error，两种都要接住
    composeError.value = e instanceof Error ? e.message : String(e)
  } finally {
    composeLoading.value = false
  }
}
</script>

<style scoped>
/* 左列：标题 + 纵排页签（与 SettingsModal 的 side 布局同构） */
.side-nav {
  width: 148px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}
.side-head {
  display: flex;
  align-items: center;
  padding: var(--sp-4);
}
.side-head h3 {
  margin: 0;
  font-size: var(--fs-modal-title);
  font-weight: 600;
  line-height: var(--lh-modal-title);
  color: var(--text-primary);
  /* 原先还有一处负字距（-0.01em）—— 负字距是给拉丁字母排紧的手法，
     汉字字面本身满格，压字距只会让笔画粘连，已删。 */
}

/* 纵排页签：左对齐文字 + 圆角悬浮态；激活态用主题色描一层浅底。
   flex + min-height: 0 让页签列自己成为可滚区（与 SettingsModal 同构） */
.side-tabs {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
  flex-direction: column;
  gap: var(--sp-05);
  padding: 0 var(--sp-2) var(--sp-3);
  overflow-y: auto;
}
.side-tab {
  text-align: left;
  border: none;
  background: none;
  padding: var(--sp-15) var(--sp-3);
  font-size: var(--fs-modal-label);
  font-weight: 500;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: var(--r-md);
  font-family: inherit;
  /* 原先用 all 过渡 —— 会连带过渡布局属性（padding 一变就演一遍重排），
     且整条覆盖了全局 button 基线的六属性过渡，于是同一屏里关闭钮 220ms、页签 150ms。
     只列真正会变的两个属性，并用 T1 档。 */
  transition:
    background var(--m-tap) var(--m-ease-out),
    color var(--m-tap) var(--m-ease-out);
}
.side-tab:hover { background: var(--fill); color: var(--text-primary); }
.side-tab.active { background: var(--primary-tint); color: var(--primary); font-weight: 500; }
/* 分组标题（「台账」/「工作流」）：只切分语义，不该看起来像能点。
   比页签小一号、无 hover、不可聚焦——它没有 role，本来也不是控件 */
.side-group {
  margin: var(--sp-2) 0 0;
  padding: 0 var(--sp-3);
  font-size: var(--fs-modal-meta);
  font-weight: 500;
  color: var(--text-tertiary);
}
.side-group:first-child { margin-top: 0; }
/* ── 已删除：`.side-tab.sub`（「覆盖」的子项样式）──
   它存在的前提是"覆盖从属于实体管理"，而那个前提是错的——两者是平级页签，
   现在都是普通 `.side-tab`。留着这段会是一条永不命中的死规则。
   历史（同一处改过四版，别再往回走）：
   · `padding-left: 24px` → 破坏左对齐轴，读作"被甩出队列"；
   · L 形引导线 → 竖段被 6px 行距切成短横杠；
   · 2px 圆头标识竖线 → 被读成"这行有左边框"（其他行都没有，更突兀）；
   · 无论哪种，根因都在"想在一个 148px 的窄栏里用横向手段表达层级"。
   最终结论不是换个图形，而是**这个层级本就不该存在**，拆成平级页签。 */
/* 跳转项（「复盘」「组装」）：它们不在本容器里，故**没有** active 态。
   hover 时给主色，暗示"点了会去别处"而不是"切到本站的另一个视图" */
.side-tab.action:hover { color: var(--primary); }
.side-jump {
  margin-left: var(--sp-05);
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
/* 页签上的"有待处理反证"提示点。
   不用数字：这个数只是"该去看一眼"的信号，具体几条在列表里更清楚，
   而左列只有 148px，挂数字会把页签文字挤到换行。 */
.side-dot {
  display: inline-block;
  width: 6px;
  height: 6px;
  margin-left: var(--sp-05);
  border-radius: 50%;
  background: var(--danger);
  vertical-align: middle;
}

/* 右列：页签内容区。页签组件的根元素（class="m-body"）会带上本组件的
   作用域属性，故这条父级 scoped 规则能命中各页签的根容器（子组件根元素继承
   父 scope id）。覆盖的根是 `.cv`，由外壳在模板里套一层 `.m-body`。
   弹窗已是固定高度（layout="side"），激活的页签靠 flex:1 占满剩余高度即可。 */
.side-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
/* 固定标题栏：不随内容滚动，关闭按钮在右侧 */
.m-titlebar {
  height: 40px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 var(--sp-5);
  border-bottom: 1px solid var(--border);
}
.m-titlebar-title {
  /* 右列栏头是"区块标题"级别：原为 14/600，与正文 14/400 同号，
     靠字重撑层级在中文里读不出来（500/600 常回落 400）。 */
  font-size: var(--fs-modal-section);
  font-weight: 600;
  line-height: var(--lh-modal-section);
  color: var(--text-primary);
}
/* 右上角关闭钮：`.btn .btn-icon .btn-quiet`（32×32 幽灵图标钮）。
   这里曾有两份逐字相同的私有 `.btn-icon`（本文件与 ResearchModal 各一份），
   而 `EntityDashboardModal` 只写了 class 没有定义——那个关闭钮一直退化成
   浏览器原生按钮。现在三处共用全局角色类，缺定义这种事不会再发生。 */
.m-body {
  /* 顶部额外预留 BaseModal side 布局右上角关闭钮的高度（--side-close-reserve），
     参数条 / 工具条等首行内容不再顶进按钮区被遮挡 */
  padding: var(--sp-2) var(--sp-5) var(--sp-4);
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  overflow-y: auto;
  overscroll-behavior: contain;
  flex: 1;
  /* min-height: 0 缺了 flex:1 就白给：flex 子项默认 min-height:auto，
     内容比容器高时它不会收缩，overflow 永远不触发，多出来的行直接被
     .base-modal 的 overflow:hidden 切掉且滚不到。放大字号后最容易暴露。 */
  min-height: 0;
}
</style>
