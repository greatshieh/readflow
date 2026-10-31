<template>
  <!-- 上下文栏：从某份报告里点「打开全文」跳到文章后，正文栏顶部出现的一条 36px 栏。
       它承载一句承诺——"你正在某份报告的阅读过程中，随时可以回到原处"——
       所以它的语汇是「栏」而不是「提示」：中性底色 + 左侧 3px 竖条 + 正文色文字，
       没有一个地方借用"选中 / 高亮"的表达（那套语汇已被 .tb-btn.on 与 .ref-item.flash 占用）。
       三个并列按钮：回到报告 / 回到第 n 条引用 / 关掉这条入口（button 不能嵌套 button）。 -->
  <div class="ctxbar" :class="{ 'is-stale': ctx.stale }" role="region" aria-label="报告来源">
    <button
      class="cb-main"
      type="button"
      :disabled="ctx.stale"
      :aria-label="mainLabel"
      :title="mainTitle"
      @click="goBack"
    >
      <svg
        class="cb-arrow"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <line x1="19" y1="12" x2="5" y2="12" />
        <polyline points="12 19 5 12 12 5" />
      </svg>
      <!-- 失效态：整条主文案替换成一句话。两段（期号 / AI 标题）都不显示——
           一份已经不在的报告，标题再准确也没有"回到哪"可言 -->
      <span v-if="ctx.stale" class="cb-stale">这份报告已不存在</span>
      <!-- 常态：两段式。期号永不截断（它是"回到哪一份"的唯一坐标），
           分隔符与 AI 标题合成一段并单独 ellipsis（title 属性里始终是标题全文） -->
      <span v-else class="cb-text">
        <span class="cb-period">{{ ctx.reportLabel }}</span>
        <span v-if="ctx.reportTitle" class="cb-title">{{ ctx.reportTitle }}</span>
      </span>
    </button>

    <!-- 引用徽标：只在"当前文章就是当初跳出去的那一篇"时可见。
         两种隐藏是两件事，所以用了两个指令：
         - `v-if`（结构性）：失效态 / 没有引用编号时，这条栏上根本不该有徽标；
         - `v-show`（状态性）：用户自己切到文章 B 之后徽标隐去，切回来立刻复原——
           留着「引用 12」是在说一件假话（那张编号属于另一篇）。 -->
    <button
      v-if="!ctx.stale && ctx.refNo !== null"
      v-show="onOriginArticle"
      class="cb-ref"
      type="button"
      :aria-label="badgeLabel"
      @click="goBack"
    >
      <span>引用 {{ ctx.refNo }}</span>
    </button>

    <button
      class="cb-x"
      type="button"
      aria-label="关闭这条返回入口"
      title="关闭这条入口。之后可在「研究工作台 → 报告」里找到这份报告"
      @click="store.clearSourceContext()"
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true">
        <line x1="18" y1="6" x2="6" y2="18" />
        <line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  </div>
</template>

<script setup lang="ts">
/**
 * 报告上下文栏
 *
 * # 职责
 * 用户从报告点「打开全文」跳到文章后，在正文栏顶部提供一个"回到刚才那个位置"的入口：
 * 主按钮回报告并恢复位置、徽标回报告并定位到那条引用、✕ 关掉这条入口。
 *
 * # 为什么清理必须发生在送达之后
 * 旧实现是"先作废、再出发"：`goBack()` 先 `clearSourceContext()` 再 `requestOpen()`，
 * 于是上下文在**会失败的转场之前**就被收走——`focusReport` 一旦找不到报告，用户既没有
 * 退回目标、也没有栏可点，被锁在文章里。病根不是"会清"，而是"清在一个会失败的转场之前"。
 * 现在：本组件只调 `store.backToReport()`（写一份位置快照 + 派发打开意图，不清 `ctx`），
 * 清除动作落在 `ResearchModal` 里 `await focusReport()` 之后的成功分支——**送达即回收**。
 * 失败也不清，本栏转 `stale` 失效态留在原地：留着才有第二次机会，✕ 是用户唯一的出口。
 *
 * # 为什么视觉上不写"回到"二字
 * `←` + 期号已建立导航语义，而"一个期号不可能是一个状态标签的内容"正是与旧视觉的关键差别；
 * 报告阅读页头部已有同构先例（`← 报告列表`）。含义由 `aria-label` 补齐。
 *
 * # 为什么不恢复主屏视图
 * 跳转前可能在推荐流，回来仍停在文章阅读：用户手上正看着文章，
 * 把他拽回卡片流是反直觉的。原视图属于"跳转前的历史"，不追。
 */
import { computed } from 'vue'
import { useReportPeriodStore } from '@/stores/reportPeriod'
import { useArticlesStore } from '@/stores/articles'

const store = useReportPeriodStore()
const articlesStore = useArticlesStore()

/** 当前来源上下文（为 null 时本组件不会渲染，见父级 v-if） */
const ctx = computed(() => store.sourceContext!)

/**
 * 当前文章是否就是当初跳出去的那一篇
 *
 * 引用编号只对那一篇有意义：用户自己切到文章 B 之后，徽标必须隐去，
 * 栏退化为纯"回报告"入口（DESIGN §4.6）。
 *
 * `Number.isFinite(ctx.articleId)` 这一半不能省：若上下文里的 `articleId` 是
 * `undefined`（后端字段漏了 camelCase 重命名 / 旧快照），而此刻又恰好没有选中文章，
 * 两边同为 `undefined` 会让 `===` 成立 → 徽标显示，等于在说一件假话。
 *
 * @returns 是同一篇时 true
 */
const onOriginArticle = computed(
  () =>
    Number.isFinite(ctx.value.articleId) &&
    ctx.value.articleId === articlesStore.selectedArticle?.id,
)

/** 主按钮的读屏文案：视觉上没有"回到"二字，这里必须有（DESIGN §5.4） */
const mainLabel = computed(() =>
  ctx.value.stale ? '这份报告已不存在' : `回到 ${ctx.value.reportLabel}：${ctx.value.reportTitle}`,
)

/** 主按钮的悬停提示：标题被 ellipsis 截断时，这里是唯一能看到全文的地方 */
const mainTitle = computed(() => (ctx.value.stale ? '这份报告已不存在' : ctx.value.reportTitle))

/** 徽标的读屏文案：说明它比主按钮多做了什么（定位到第 n 条） */
const badgeLabel = computed(() => `回到报告并定位到第 ${ctx.value.refNo} 条引用`)

/**
 * 回到来源报告
 *
 * 与徽标点击走**同一条**逻辑：`pendingRestore.refNo` 已经是 `ctx.refNo`，
 * 所以"点徽标回报告并定位引用"是 `backToReport` 的自然结果，不需要第二套代码路径。
 * 这里刻意不 `clearSourceContext()`——成败由送达方（`ResearchModal`）判定。
 */
function goBack() {
  store.backToReport()
}
</script>

<style scoped>
/* ── 容器 ───────────────────────────────────────────────────────────────
   36px（不是 32）：正文工具条由 8px × 2 padding + 约 20px 按钮 + 1px 边框
   组成 ≈ 32px，两条同高会读成"一个区块的两行"；36 差一档，读成"独立的一条栏"。
   底色用 --surface 而不是 --primary-fg：后者是"选中 / 高亮"的语汇。 */
.ctxbar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  height: 36px;
  /* 左 12 + 3px 竖条 = 视觉 15，与工具条的 16px 基本对齐 */
  padding: 0 var(--sp-4) 0 var(--sp-3);
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  /* 左竖条是整条栏唯一的主题色表达 */
  border-left: 3px solid var(--primary);
}
/* 失效态：竖条褪为中性色。不用红色——红色在本项目是 --danger（删除类操作）的语汇，
   这里只是"这条路走不通"，不是危险 */
.ctxbar.is-stale {
  border-left-color: var(--border);
}

/* ── 主按钮：← + 两段文字 ─────────────────────────────────────────────── */
.cb-main {
  flex: 1 1 auto;
  min-width: 0;
  /* 命中区吃满栏高，但不**超过**栏高：`height: 36px` 在 35px 的内容盒里会上下各溢出
     0.5px，压着容器下边框——今后谁给栏加 `overflow: hidden` 就被裁一刀。
     `align-self: stretch` 让高度等于内容盒（35px），实质上是这条栏能给的最大命中区。
     判据（§5.5 规则 4，产品经理已修订）：主按钮命中区 ≥ 34px。 */
  align-self: stretch;
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  border: none;
  background: none;
  padding: 0;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.cb-main:disabled {
  cursor: default;
}
.cb-arrow {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  /* 图标走主题色：整条栏只有它与左竖条带色，其余全是中性色 */
  color: var(--primary);
}
.cb-main:disabled .cb-arrow {
  opacity: 0.45;
}
/* 两段文字的外层：唯一的 job 是把"可收缩"这件事关在自己身上，
   否则长标题会把 ✕ 挤出去 */
.cb-text {
  display: flex;
  align-items: baseline;
  gap: var(--sp-05);
  min-width: 0;
  overflow: hidden;
}
/* 段①期号：永不收缩、永不截断（截它等于废掉这条栏） */
.cb-period {
  flex: 0 0 auto;
  font-size: var(--fs-sm);
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
}
/* 段②AI 标题：唯一可收缩的段。层级靠颜色与字重一档差，不靠字号 */
.cb-title {
  flex: 0 1 auto;
  min-width: 0;
  font-size: var(--fs-sm);
  font-weight: 400;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 分隔符挂在可截断段上：标题被完全裁掉时它跟着一起消失，
   永远不会在行尾留下一个悬空的「·」。纯装饰，故不进 aria-label */
.cb-title::before {
  content: '·';
  margin-right: var(--sp-05);
  color: var(--text-tertiary);
}
/* hover 只换颜色、不加下划线：36px 栏里下划线会贴到 1px 下边框上，
   读成"被选中的文字" */
.cb-main:hover:not(:disabled) .cb-period,
.cb-main:hover:not(:disabled) .cb-title {
  color: var(--primary);
}
.cb-main:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  border-radius: var(--r-sm);
}

/* 失效文案：整条替换两段文字，颜色降到三级（这条路由已经不通了） */
.cb-stale {
  font-size: var(--fs-sm);
  font-weight: 400;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ── 引用徽标 ───────────────────────────────────────────────────────────
   视觉药丸 20px 高（36px 栏里不该出现第二块实心块），
   但命中区必须 ≥28px —— 所以外圈按钮撑 28、只有内层 span 上底色。
   用 `getBoundingClientRect()` 量到的是按钮（28），不是药丸（20）。 */
.cb-ref {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  height: 28px;
  padding: 0;
  border: none;
  background: none;
  cursor: pointer;
  font-family: inherit;
}
.cb-ref > span {
  font-size: var(--fs-xs);
  /* line-height 直接给 20px：不加纵向 padding 也能得到 20px 高的药丸 */
  line-height: 20px;
  padding: 0 8px;
  border-radius: 999px;
  background: var(--fill);
  color: var(--text-secondary);
  /* 编号会从 1 跳到 12，等宽数字让它不左右抖 */
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.cb-ref:hover > span {
  background: var(--fill-secondary);
  color: var(--text-primary);
}
.cb-ref:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  border-radius: 999px;
}

/* ── ✕ ────────────────────────────────────────────────────────────────
   24×24（低于 24 就破项目命中区下限）；margin-right: -6px 把图标视觉
   对齐到 16px 内边距上 */
.cb-x {
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: none;
  background: none;
  margin-right: -6px;
  border-radius: var(--r-sm);
  color: var(--text-tertiary);
  cursor: pointer;
}
.cb-x:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.cb-x:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
</style>
