<template>
  <!-- 上下文栏：用户从研究工作台的某处跳进原文后，正文栏顶部出现的一条 36px 栏。
       它承载一句承诺——"你正在某处的阅读过程中，随时可以回到原处"——
       所以它的语汇是「栏」而不是「提示」：中性底色 + 左侧 3px 竖条 + 正文色文字，
       没有一个地方借用"选中 / 高亮"的表达（那套语汇已被 .tb-btn.on 与 .ref-item.flash 占用）。
       三个并列按钮：回到来源 / 回到第 n 条引用（仅报告）/ 关掉这条入口（button 不能嵌套 button）。 -->
  <div class="ctxbar" :class="{ 'is-stale': record.stale !== null }" role="region" aria-label="来源">
    <button
      class="cb-main"
      type="button"
      :disabled="record.stale !== null"
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
      <!-- 失效态：整条主文案替换成一句话。锚点与描述都不显示——
           一个已经不在的目标，描述再准确也没有"回到哪"可言 -->
      <span v-if="record.stale !== null" class="cb-stale">{{ staleText }}</span>
      <!-- 常态：坐标 + 类别词 + 描述。收缩顺序是"描述 → 坐标 → 类别词"：
           坐标可 ellipsis（看得见省略号），类别词永不收缩——
           把「10月30日 日报」压成「10月30日 日」而不给省略号，是本轮设计评审
           专门攻过的缺陷，别再折回去。分隔符挂在描述上，描述隐藏时它一起消失 -->
      <span v-else class="cb-text">
        <span v-if="record.label.anchor" class="cb-anchor">{{ record.label.anchor }}</span>
        <span class="cb-kind">{{ record.label.kind }}</span>
        <span v-if="record.label.desc" class="cb-desc">{{ record.label.desc }}</span>
      </span>
    </button>

    <!-- 引用徽标：只在"这条来源是报告引用、且当前文章就是当初跳出去的那一篇"时可见。
         两种隐藏是两件事，所以用了两个指令：
         - `v-if`（结构性）：非报告来源 / 没有引用编号时，这条栏上根本不该有徽标；
         - `v-show`（状态性）：用户自己切到文章 B 之后徽标隐去，切回来立刻复原——
           留着「引用 12」是在说一件假话（那张编号属于另一篇）。 -->
    <button
      v-if="showBadge"
      v-show="onOriginArticle"
      class="cb-ref"
      type="button"
      :aria-label="`回到报告并定位到第 ${refNo} 条引用`"
      @click="goBack"
    >
      <!-- 「引用」二字在窄栏（< 279px）里让位给数字：那时整条栏已经在跟描述抢宽度，
           而"这个数字是什么"由 aria-label 与它旁边的报告期号共同交代 -->
      <span><span class="cb-ref-word">引用 </span>{{ refNo }}</span>
    </button>

    <!-- 键帽：这条栏的键盘等价物。独立槽位、不可点、不参与 hover 变色——
         它说明的是"还有一条路"，不是"点我"。
         用全局 `.kbd`（20px 标准键帽），**不用** `.kbd-block`：后者 44px 的
         min-width 在 36px 栏里会吃掉一整段描述。 -->
    <kbd v-if="record.stale === null" class="kbd cb-kbd" aria-hidden="true">{{ backKeyLabel }}</kbd>

    <button
      class="cb-x"
      type="button"
      aria-label="关闭这条返回入口"
      title="关闭这条入口。之后仍可在「研究工作台」里找到这个来源"
      @click="trail.clear()"
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
 * 来源上下文栏
 *
 * # 职责
 * 用户从研究报告 / 事件时间线 / 论点证据跳进原文后，在正文栏顶部提供一个
 * "回到刚才那个位置"的入口：主按钮回来源并恢复位置、引用徽标回来源并定位到那条引用
 * （仅报告）、✕ 关掉这条入口。
 *
 * # 数据源只有一个：`originTrail.current`
 * 换句话说，本组件不判断"该不该显示"——父级的 `v-if` 只看 `originTrail.current` 是否为空，
 * **绝不看是否选中了文章**：文章被清理 / 被筛选掉 / 用户取消选中时正文栏回到空态，
 * 但用户仍然要能回到来源，所以那时这条栏还在。
 *
 * # 为什么清理必须发生在送达之后
 * 旧实现（本组件的前身，那个只服务报告的场景栏）是"先作废、再出发"：`goBack()` 先清上下文
 * 再派发打开意图，于是上下文在**会失败的转场之前**就被收走——目标一旦找不到，用户既没有
 * 退回目标、也没有栏可点，被锁在文章里。病根不是"会清"，而是"清在一个会失败的转场之前"。
 * 现在：本组件只调 `trail.back()`（写凭据 + 派发打开意图，不清 `current`），
 * 清除动作落在 `ResearchModal` 里恢复成功之后——**送达即回收**。
 * 失败也不清：栏转 `stale` 失效态留在原地，留着才有第二次机会，✕ 是用户唯一的出口。
 *
 * # 为什么视觉上不写"回到"二字
 * `←` + 锚点已建立导航语义，而"一个锚点不可能是一个状态标签的内容"正是与普通标签的
 * 关键差别；报告阅读页头部已有同构先例（`← 报告列表`）。含义由 `aria-label` 补齐。
 *
 * # 为什么不恢复主屏视图
 * 跳转前可能在推荐流，回来仍停在文章阅读：用户手上正看着文章，
 * 把他拽回卡片流是反直觉的。原视图属于"跳转前的历史"，不追。
 */
import { computed } from 'vue'
import { useOriginTrailStore } from '@/stores/originTrail'
import { useArticlesStore } from '@/stores/articles'
import { useToastStore } from '@/stores/toast'
import { originAriaLabel, originStaleText } from '@/utils/originLabel'

const trail = useOriginTrailStore()
const articlesStore = useArticlesStore()
const toast = useToastStore()

/** 当前来源记录（为 null 时本组件不会渲染，见父级 `v-if`） */
const record = computed(() => trail.current!)

/**
 * 徽标是否可能出现
 *
 * 两个前提缺一不可：来源是"报告引用"（只有它带编号；`refNo === null` 表示不是从引用
 * 跳出去的）**且**这条栏没有失效——失效态上整条栏只剩一句话与 ✕，
 * 留着「引用 12」会让用户以为还能定位到那一条。
 */
const showBadge = computed(
  () => record.value.stale === null && record.value.kind === 'report' && refNo.value !== null,
)

/** 引用编号（`refNo` 的类型是 `number | null | undefined`，这里收口成数字或 null） */
const refNo = computed(() => record.value.restore.refNo ?? null)

/**
 * 当前文章是否就是当初跳出去的那一篇
 *
 * 引用编号只对那一篇有意义：用户自己切到文章 B 之后，徽标必须隐去，
 * 栏退化为纯"回来源"入口。
 *
 * `Number.isFinite(record.articleId)` 这一半不能省：若记录里的 `articleId` 是
 * `undefined`（上游漏了赋值 / 旧快照），而此刻又恰好没有选中文章，
 * 两边同为 `undefined` 会让 `===` 成立 → 徽标显示，等于在说一件假话。
 */
const onOriginArticle = computed(
  () =>
    Number.isFinite(record.value.articleId) &&
    record.value.articleId === articlesStore.selectedArticle?.id,
)

/** 主按钮的读屏文案：视觉上没有"回到"二字，这里必须有（含描述全文） */
const mainLabel = computed(() =>
  record.value.stale !== null
    ? originStaleText(record.value.stale)
    : originAriaLabel(record.value),
)

/** 主按钮的悬停提示：描述被隐藏或截断时，这里是唯一能看到全文的地方 */
const mainTitle = computed(() => {
  const { anchor, kind, desc } = record.value.label
  if (record.value.stale !== null) return originStaleText(record.value.stale)
  return desc || [anchor, kind].filter(Boolean).join(' ')
})

/** 失效态那一句话（主语是目的地，句式一律「已不存在」） */
const staleText = computed(() =>
  record.value.stale !== null ? originStaleText(record.value.stale) : '',
)

/**
 * 键帽内容：按平台给同一次"后退"的两种写法
 *
 * 与 `ContentColumn` 空态卡、`App.vue` 的 `isBackToReportKey` 用同一条判据
 * （`navigator.platform || navigator.userAgent` 命中 Mac/iOS 即视为 macOS 键位）；
 * 三处若各写一套，用户看到的键帽与真正生效的组合键就会对不上。
 */
const backKeyLabel = computed(() =>
  /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent) ? '⌘[' : 'Alt+←',
)

/**
 * 回到来源
 *
 * 与徽标点击走**同一条**逻辑：`restore.refNo` 已经在记录里，
 * 所以"点徽标回报告并定位引用"是 `back()` 的自然结果，不需要第二套代码路径。
 * 这里刻意不清 `current`——成败由送达方（`ResearchModal`）判定。
 *
 * @returns 无返回值；临时故障（报告生成中）会 toast 一次，终态故障不会走到这里（按钮已禁用）
 */
async function goBack() {
  const result = await trail.back()
  if (!result.ok && result.kind === 'transient') toast.show(result.message)
}
</script>

<style scoped>
/* ── 容器 ───────────────────────────────────────────────────────────────
   36px（不是 32）：正文工具条由 8px × 2 padding + 约 20px 按钮 + 1px 边框
   组成 ≈ 32px，两条同高会读成"一个区块的两行"；36 差一档，读成"独立的一条栏"。
   底色用 --surface 而不是 --primary-tint：后者是"选中 / 高亮"的语汇。

   `container-type: inline-size`：本栏的收缩分档必须按**栏自己**的宽度算，
   而不是视口宽度——栏宽由左侧两栏的拖拽分隔条决定，与窗口大小无关。
   ⚠ 刻意**不**把它挂到 `.content-column` 上：`container-type` 会附带
   `contain: layout`，那会让 `.content-column` 成为 `position: fixed` 子孙的包含块，
   而正文里的图片灯箱（`.lightbox`，`position: fixed; inset: 0`）正挂在那儿——
   灯箱会从"铺满窗口"退化成"铺满正文栏并被 overflow 裁掉"。挂在本栏自己的根节点上，
   测量宽度完全一样（本栏横向撑满），风险为零。
   WebKit < 16 不支持容器查询，退化表现是"永远不收缩"（键帽与徽标始终显示），
   栏本身照常可用。 */
.ctxbar {
  flex-shrink: 0;
  container-type: inline-size;
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

/* ── 主按钮：← + 消歧词 + 锚点 + 描述 ─────────────────────────────────── */
.cb-main {
  flex: 1 1 auto;
  min-width: 0;
  /* 命中区吃满栏高，但不**超过**栏高：`height: 36px` 在 35px 的内容盒里会上下各溢出
     0.5px，压着容器下边框——今后谁给栏加 `overflow: hidden` 就被裁一刀。
     `align-self: stretch` 让高度等于内容盒（35px），是这条栏能给的最大命中区。 */
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
/* 三段文字的外层：唯一的 job 是把"可收缩"这件事关在自己身上，
   否则长描述会把徽标与 ✕ 挤出去 */
.cb-text {
  display: flex;
  align-items: baseline;
  gap: var(--sp-05);
  min-width: 0;
  overflow: hidden;
}
/* 坐标（期号跨度 / 日期 / 实体名）：**可收缩**，且被压窄时给省略号——
   用户必须看得见"这里还有字被省略了"。只给 `min-width: 0`，
   **不许给任何正值的 min-width**：那会把右边的类别词挤出边界，
   把"类别词被静默裁掉"这个缺陷从左边反向复现一次。 */
.cb-anchor {
  flex: 0 1 auto;
  min-width: 0;
  font-size: var(--fs-sm);
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
/* 类别词（日报 / 周报 / 月报 / 事件 / 论点）：**永不收缩**。
   窄到极限时留下的就是它——「周报」两个字本身不足以定位"哪一期"，
   但至少不会让用户把周报读成日报。 */
.cb-kind {
  flex: 0 0 auto;
  font-size: var(--fs-sm);
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
}
/* 描述：用户写的东西，长度无界。层级靠颜色与字重一档差，不靠字号 */
.cb-desc {
  flex: 0 1 auto;
  min-width: 0;
  font-size: var(--fs-sm);
  font-weight: 400;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 分隔符挂在描述上：描述被隐藏或裁掉时它跟着一起消失，
   永远不会在行尾留下一个悬空的「·」。纯装饰，故不进 aria-label */
.cb-desc::before {
  content: '·';
  margin-right: var(--sp-05);
  color: var(--text-tertiary);
}
/* hover 只换颜色、不加下划线：36px 栏里下划线会贴到 1px 下边框上，
   读成"被选中的文字" */
.cb-main:hover:not(:disabled) .cb-anchor,
.cb-main:hover:not(:disabled) .cb-kind,
.cb-main:hover:not(:disabled) .cb-desc {
  color: var(--primary);
}
.cb-main:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  border-radius: var(--r-sm);
}

/* 失效文案：整条替换三段文字，颜色降到三级（这条路由已经不通了） */
.cb-stale {
  font-size: var(--fs-sm);
  font-weight: 400;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ── 键帽 ───────────────────────────────────────────────────────────────
   尺寸、圆角、边框全部来自全局 `.kbd`（20px 标准键帽），这里只补两点：
   不参与收缩（它是个固定宽度的提示，压窄后字形会挤在一起），
   以及 `aria-hidden` 由模板给——键盘路径的语义已由 `aria-label` 表达，
   重复播报一次"Alt+←"反而打断。 */
.cb-kbd {
  flex-shrink: 0;
  color: var(--text-tertiary);
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
   对齐到 16px 内边距上。任何宽度下都不收缩、不隐藏：失效态里它是唯一出口。 */
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

/* ── 收缩分档（按容器宽度，不按视口）──────────────────────────────────
   阈值 320 / 279 / 239 是设计稿标的三档，这里按"整数像素边界"写成
   `max-width: 319 / 278 / 238`——CSS 的 `max-width: 319px` 等价于"宽度 < 320px"，
   写成 320 会把恰好 320px 那一档也算进收缩侧。

   收缩只做两件事，且都只是 `display` 开关：让**整段**消失，不靠 ellipsis 把它缩到 0
   （缩到 0 会在原位留一段 6px 的 gap，看起来像破图）。档位之外的
   ✕ / 栏高 / 左竖条 / 箭头在任何宽度都不动；栏永不整体隐藏——
   一条会自己消失的返回入口，比没有返回入口更难解释。 */
@container (max-width: 319px) {
  /* 键帽只是"还有一条路"的提示，最先让位 */
  .cb-kbd {
    display: none;
  }
  /* 描述整段隐藏：这一档之后只剩下"回到哪一份"的坐标，描述挤成一条缝没有意义 */
  .cb-desc {
    display: none;
  }
}
@container (max-width: 278px) {
  /* 「引用」二字让位给数字，药丸只剩编号 */
  .cb-ref-word {
    display: none;
  }
}
@container (max-width: 238px) {
  /* 徽标整个让位：回来源比"回到第 12 条"更基础 */
  .cb-ref {
    display: none;
  }
  /* 坐标让位，只留类别词。这是本轮的硬要求：宁可只剩「周报」，
     也不要把「10月30日 日报」压成「10月30日 日」 */
  .cb-anchor {
    display: none;
  }
}
</style>
