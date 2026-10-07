<template>
  <!-- 研究工作台弹窗外壳：四个页签（时间线 / 报告 / 实体 / 关系图）的容器与切换。
       layout="side"：固定高度弹窗，切页签不再跳变；tab 竖排在左列。 -->
  <BaseModal :open="open" title="研究工作台" layout="side" @close="close">
    <!-- 左列：标题 + 纵排页签。关闭钮由 BaseModal 的 side 布局统一放在面板右上角 -->
    <aside class="side-nav">
      <div class="side-head">
        <h3>研究工作台</h3>
      </div>
      <nav class="side-tabs">
        <button class="side-tab" :class="{ active: tab === 'timeline' }" @click="tab = 'timeline'">
          事件时间线
        </button>
        <button class="side-tab" :class="{ active: tab === 'reports' }" @click="tab = 'reports'">
          研究报告
        </button>
        <button class="side-tab" :class="{ active: tab === 'entities' }" @click="tab = 'entities'">
          实体管理
        </button>
        <button class="side-tab" :class="{ active: tab === 'graph' }" @click="tab = 'graph'">
          关系图
        </button>
      </nav>
    </aside>

    <!-- 右列：页签内容区（四个页签的根元素都带 class="m-body"，占满右列） -->
    <div class="side-main">
      <!-- 四个页签一律 v-show 常驻挂载（不是 v-if）：各页签的筛选参数
           （时间窗 / 实体筛选 / 报告类型与天数 / 关系图参数）在拆分前的单组件里
           天然跨页签、跨开关持久，v-if 会在每次切换时把这些状态连同 DOM 一起丢掉。
           页签各自的加载时机由它们 watch `active` / `open` 自行驱动，见各组件文件头。 -->
      <ResearchTimelineTab v-show="tab === 'timeline'" :open="open" @close="close" />
      <ResearchReportsTab v-show="tab === 'reports'" :active="tab === 'reports'" :open="open" />
      <ResearchEntitiesTab
        v-show="tab === 'entities'"
        @entities-changed="graphEpoch++"
      />
      <ResearchGraphTab
        v-show="tab === 'graph'"
        :active="tab === 'graph'"
        :open="open"
        :epoch="graphEpoch"
        @expand="expandGraph"
      />
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 研究工作台弹窗组件（外壳）
 *
 * # 职责
 * 只负责弹窗外壳与页签切换：BaseModal 容器、头部、tab 条，以及四个页签组件
 * （`ResearchTimelineTab` / `ResearchReportsTab` / `ResearchEntitiesTab` /
 * `ResearchGraphTab`）的编排。页签内部的表单状态、数据加载与临时提示都在
 * 各页签组件里（见各文件头的"挂载语义"一节）。
 *
 * # 父组件通信
 * - `open` prop：控制弹窗显示状态
 * - `@close` 事件：通知父组件关闭弹窗
 *
 * # 跨页签协调（外壳仅有的两处编排逻辑）
 * 1. 弹窗打开时确保实体列表已加载——时间线的筛选下拉与实体页签共用这份数据；
 * 2. 实体页签的增删/启停/评分会让已加载的关系图过期：实体页签 emit
 *    `entities-changed`，外壳以 `graphEpoch` 计数转交关系图页签，
 *    由后者在自己可见时决定是否重算（不在用户看实体列表时后台全库扫描）。
 */
import { ref, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import ResearchTimelineTab from './ResearchTimelineTab.vue'
import ResearchReportsTab from './ResearchReportsTab.vue'
import ResearchEntitiesTab from './ResearchEntitiesTab.vue'
import ResearchGraphTab from './ResearchGraphTab.vue'
import { useResearchStore } from '@/stores/research'
import { useEntitiesStore } from '@/stores/entities'
import { useUiStore } from '@/stores/ui'

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

/** 当前 tab：timeline = 事件时间线；reports = 研究报告；entities = 实体管理；graph = 关系图 */
const tab = ref<'timeline' | 'reports' | 'entities' | 'graph'>('timeline')

/** 实体变更纪元：实体页签每变更一次 +1，关系图页签据此判断"图是否过期" */
const graphEpoch = ref(0)

// 打开时确保实体列表已加载：时间线的实体筛选下拉依赖它（可能尚未加载过）
watch(
  () => props.open,
  async val => {
    if (val && entitiesStore.entities.length === 0) {
      await entitiesStore.loadEntities()
    }
  }
)

/** 关闭弹窗。页签内的临时提示/展开态由各页签在下次打开时自行重置（见各自文件头） */
function close() {
  emit('close')
  tab.value = 'timeline'
  researchStore.closeReport()
}

/**
 * 把关系图放大到主屏展示
 *
 * 关系图页签的「放大到主屏」按钮触发：切换主屏视图后必须**先收起本弹窗**——
 * 主屏的 GraphView 与本弹窗的画布是同一份数据的两个渲染位，弹窗盖在上面
 * 会让用户以为还是弹窗里那张小图。先切视图再走 close()（它会重置页签，
 * 无碍：用户下次打开研究工作台本就应从时间线开始）。
 *
 * @returns 无返回值；副作用为 mainView 切到 graph、本弹窗关闭
 */
function expandGraph() {
  uiStore.setMainView('graph')
  close()
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
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
}

/* 纵排页签：左对齐文字 + 圆角悬浮态；激活态用主题色描一层浅底 */
.side-tabs {
  display: flex;
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
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: var(--r-md);
  transition: all 0.15s;
  font-family: inherit;
}
.side-tab:hover { background: var(--fill); color: var(--text-primary); }
.side-tab.active { background: var(--primary-fg); color: var(--primary); font-weight: 500; }

/* 右列：页签内容区。页签组件的根元素（class="m-body"）会带上本组件的
   作用域属性，故这条父级 scoped 规则能命中四个页签的根容器
   （子组件根元素继承父 scope id）。弹窗已是固定高度（layout="side"），
   激活的页签靠 flex:1 占满剩余高度即可。 */
.side-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.m-body {
  /* 顶部额外预留 BaseModal side 布局右上角关闭钮的高度（--side-close-reserve），
     参数条 / 工具条等首行内容不再顶进按钮区被遮挡 */
  padding: calc(var(--sp-4) + var(--side-close-reserve, 48px)) var(--sp-5) var(--sp-4);
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  overflow-y: auto;
  flex: 1;
}
</style>
