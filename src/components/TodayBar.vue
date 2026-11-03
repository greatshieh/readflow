<template>
  <!-- 「今天要处理」摘要条：常驻在文章列表栏顶部（操作条之上）。
       无待办时整条不渲染——"0 项"是纯噪音，也是用户判定功能坏了的常见误因。
       收起态只有一行 40px，展开态最多 5 行，不挤压下方列表。 -->
  <section
    v-if="store.shouldRender"
    class="today-bar"
    :class="{ expanded: !store.collapsed }"
    role="region"
    aria-label="今天要处理"
  >
    <!-- 收起态整行可点：区域小、目标是"扫一眼"，把点击目标做成整行比抠箭头好点 -->
    <div
      class="today-head"
      role="button"
      tabindex="0"
      :aria-expanded="!store.collapsed"
      @click="store.toggleCollapsed()"
      @keydown.enter.prevent="store.toggleCollapsed()"
      @keydown.space.prevent="store.toggleCollapsed()"
      @keydown.esc.stop="onCollapseEsc"
    >
      <span class="today-title">今天要处理</span>
      <span class="today-total">{{ store.visibleTotal }} 项</span>
      <!-- 分类计数：只显示有数的类别。窄屏隐藏（收起态只剩总数，仍能判断轻急） -->
      <span class="today-counts">
        <span
          v-for="c in store.visibleCounts"
          :key="c.kind"
          class="today-count"
          :class="`tk-${c.kind}`"
        >
          {{ KIND_LABELS[c.kind].short }}{{ c.count }}
        </span>
      </span>
      <span class="today-chevron" :class="{ up: !store.collapsed }" aria-hidden="true"></span>
    </div>

    <!-- 展开态：最多 5 条 + 「还有 N 项」 -->
    <ul v-if="!store.collapsed" class="today-list">
      <li
        v-for="item in shownItems"
        :key="`${item.kind}:${item.targetId}`"
        class="today-item"
        :class="{ overdue: item.overdue }"
      >
        <span class="ti-kind" :class="`tk-${item.kind}`">
          {{ KIND_LABELS[item.kind].short }}
        </span>
        <span class="ti-main">
          <span class="ti-title" :title="item.title">{{ item.title }}</span>
          <span v-if="item.detail" class="ti-detail">{{ item.detail }}</span>
        </span>
        <!-- 一键处理：只有 mark_reviewed 会真正改动数据；其余是跳转，
             文案也跟着区分（「打开」而不是「处理」），避免让用户以为已完成 -->
        <button
          class="btn btn-sm ti-act"
          type="button"
          :title="actionTitle()"
          :disabled="actingOn === keyOf(item)"
          @click.stop="onAct(item)"
        >
          {{ actionLabel(item) }}
        </button>
        <!-- 本次会话内忽略：不落库，也不会永久屏蔽 -->
        <button
          class="ti-dismiss"
          type="button"
          title="本次不再显示这一条（不会删除数据）"
          aria-label="忽略这一条"
          @click.stop="store.dismiss(item)"
        >
          <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
            <path
              d="M4 4l8 8M12 4l-8 8"
              stroke="currentColor"
              stroke-width="1.5"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </li>
    </ul>

    <!-- 超出 5 条时给一条出口，把"还有更多"的表达统一到一句话上 -->
    <div v-if="!store.collapsed && overflowCount > 0" class="today-more">
      还有 {{ overflowCount }} 项，进入研究工作台查看全部
      <button class="today-more-link" type="button" @click="gotoReview">去看看</button>
    </div>
  </section>
</template>

<script setup lang="ts">
/**
 * 「今天要处理」摘要条
 *
 * # 职责
 * 只渲染 `workbench` store 里的待办 + 把用户动作转成 store 调用。
 * 取数、排序、文案全在后端与 store 里，本组件不查库、不拼判定条件。
 *
 * # 为什么最多显示 5 条
 * 这块存在于文章列表顶部，展开过高会把列表挤下去。5 条是"一眼能扫完"的上限，
 * 超出的部分给一句话出口——"要不要看全部"应该由用户决定，而不是靠滚动。
 */
import { computed, onBeforeUnmount, ref } from 'vue'
import { KIND_LABELS, useWorkbenchStore, type TodoItem } from '@/stores/workbench'
import { useUiStore } from '@/stores/ui'
import { requestThesisOpen } from '@/utils/thesisJump'

/** 最多同时展示的条数 */
const SHOW_LIMIT = 5

const store = useWorkbenchStore()
const uiStore = useUiStore()

/** 正在执行动作的条目（防连点） */
const actingOn = ref('')

const shownItems = computed(() => store.visibleItems.slice(0, SHOW_LIMIT))
const overflowCount = computed(() => Math.max(0, store.visibleTotal - SHOW_LIMIT))

function keyOf(item: TodoItem): string {
  return `${item.kind}:${item.targetId}`
}

/**
 * 按钮文案
 *
 * 所有按钮都是**跳转**，因此一律用「打开 / 查看」而不是「处理 / 完成」——
 * 让用户清楚点完只是"到了该看的地方"，事情还得自己做。
 */
function actionLabel(item: TodoItem): string {
  if (item.action === 'open_entity') return '查看'
  return '打开'
}

function actionTitle(): string {
  return '跳转过去看（不会标记为已处理）'
}

/** Esc 只收起，不做别的：本条是常驻区，不应抢全局 Esc 的语义 */
function onCollapseEsc() {
  if (!store.collapsed) store.toggleCollapsed()
}

/** 跳到研究工作台的论点页签 */
function gotoThesis(id: number) {
  requestThesisOpen(id)
  window.dispatchEvent(new CustomEvent('open-research'))
}

function gotoReview() {
  uiStore.toggleReviewView()
}

/**
 * 执行一键处理
 *
 * # 这里**不做**「标记已复盘」
 * 复盘的 `thesis_review_mark` 要求 `review_id` + `action`，且除 `unchanged` 外
 * **必须填理由**——理由正是复盘这个功能的全部产出。在待办条上点一下就"已复盘"，
 * 等于把一句有内容的理由省掉，复盘记录会退化成"我点过了"。
 * 所以这类条目这里只做**跳转**（去复盘页真正处置），不假造一条空记录。
 */
async function onAct(item: TodoItem) {
  const key = keyOf(item)
  if (actingOn.value) return
  actingOn.value = key
  try {
    await store.handleAction(
      item,
      {
        toThesis: gotoThesis,
        toEntity: (id) =>
          window.dispatchEvent(
            new CustomEvent('open-entity-dashboard', { detail: { entityId: id } })
          ),
        toCoverage: () => {
          window.dispatchEvent(new CustomEvent('open-research'))
        },
        toReview: gotoReview
      },
      // 传空实现：store 里 `mark_reviewed` 分支因此不会被触发
      // （后端目前也不会产出该 action，见 workbench.rs 的 action 取值）
      async () => {}
    )
  } catch (e) {
    console.warn('待办处理失败：', e)
  } finally {
    actingOn.value = ''
  }
}

/**
 * 组件卸载时不留悬挂引用
 *
 * 目前只用 window.dispatchEvent（无需解绑），但仍显式声明钩子：
 * 将来若加 addEventListener，忘记解绑会让"打开过后台仍收到事件"这类问题
 * 变成偶发 bug，在这里留一个明显的落点。
 */
onBeforeUnmount(() => {
  actingOn.value = ''
})
</script>

<style scoped>
/* 摘要条整体：与列表栏同宽，顶部分隔线区分于工具栏 */
.today-bar {
  flex-shrink: 0;
  border-bottom: 1px solid var(--border);
  background: var(--surface-muted);
  font-size: var(--fs-sm);
}

/* 收起态一行 40px：达到 44 触达下限的可点高度靠内边距补足 */
.today-head {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  min-height: 40px;
  cursor: pointer;
  user-select: none;
}
.today-head:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}
.today-title {
  flex-shrink: 0;
  font-weight: 500;
  color: var(--text-primary);
}
.today-total {
  flex-shrink: 0;
  color: var(--primary);
  font-variant-numeric: tabular-nums;
  font-weight: 500;
}
/* 分类计数占据剩余宽度，超宽时自己省略，不推挤右侧箭头 */
.today-counts {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  gap: var(--sp-1);
  overflow: hidden;
  white-space: nowrap;
}
.today-count {
  flex-shrink: 0;
  padding: 0 var(--sp-1);
  border-radius: var(--r-sm);
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  background: var(--surface);
}
.today-chevron {
  flex-shrink: 0;
  width: 0;
  height: 0;
  margin-left: auto;
  border-left: 4px solid transparent;
  border-right: 4px solid transparent;
  border-top: 5px solid var(--text-tertiary);
  transition: transform var(--m-tap) ease;
}
.today-chevron.up {
  transform: rotate(180deg);
}

/* 展开态列表：自身可滚，上限 240px（不超过 5 行） */
.today-list {
  list-style: none;
  margin: 0;
  padding: 0 var(--sp-3) var(--sp-2);
  max-height: 240px;
  overflow-y: auto;
}
.today-item {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-1) 0;
  border-top: 1px solid var(--border);
}
.today-item:first-child {
  border-top: none;
}
/* 类别徽标：宽度随字数变，但它处在行首且是唯一会变的元素——
   为了让正文左边缘对齐，给固定宽度 + 居中。 */
.ti-kind {
  flex-shrink: 0;
  width: 34px;
  text-align: center;
  padding: 1px 0;
  border-radius: var(--r-sm);
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  background: var(--surface);
}
/* 逾期项标红：与"已欠着"的语义一致 */
.today-item.overdue .ti-title {
  color: var(--tone-red);
}
.ti-main {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.ti-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-primary);
}
.ti-detail {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.ti-act {
  flex-shrink: 0;
}
/* 忽略按钮：常显但视觉弱化。
   刻意不做"hover 才出现"——那会让触摸设备无法发现，也少一个可聚焦目标。 */
.ti-dismiss {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: none;
  border-radius: var(--r-sm);
  background: none;
  color: var(--text-tertiary);
  cursor: pointer;
}
.ti-dismiss:hover {
  color: var(--text-primary);
  background: var(--surface);
}
.ti-dismiss:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.today-more {
  padding: 0 var(--sp-3) var(--sp-2);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.today-more-link {
  border: none;
  background: none;
  padding: 0 var(--sp-1);
  color: var(--primary);
  cursor: pointer;
  font-size: inherit;
}

/* 窄屏：分类计数与详情行隐藏，保证每行只有"类别 + 正文 + 处理"三件事 */
@media (max-width: 768px) {
  .today-counts,
  .ti-detail {
    display: none;
  }
}
</style>
