<template>
  <!-- 主屏关系图视图：占满订阅栏右侧的全部宽度（App.vue 在 mainView === 'graph'
       时以本组件替换文章列表栏 + 正文栏）。参数、数据与重算动作都收在
       researchStore，与研究工作台的图页签共享同一份，两处展示不会互相漂移。 -->
  <div class="graph-view">
    <!-- 参数条：与弹窗图页签同一组控件，改参数两处同步生效 -->
    <div class="graph-bar">
      <h3 class="graph-title">实体关系图</h3>
      <AppSelect v-model="researchStore.graphDays" :options="researchStore.graphDayOptions" width="auto" title="时间窗" />
      <AppSelect
        v-model="researchStore.graphMinWeight"
        :options="researchStore.graphWeightOptions"
        width="auto"
        title="最小共现数"
      />
      <span class="graph-stat">{{ graphStatText }}</span>
      <button
        class="btn btn-secondary btn-sm"
        @click="researchStore.reloadGraph()"
        :disabled="researchStore.graphLoading"
      >
        {{ researchStore.graphLoading ? '计算中…' : '重新计算' }}
      </button>
      <!-- spacer 把返回按钮推到最右：参数在左、退出在右，视觉上分成两组 -->
      <span class="graph-bar-spacer"></span>
      <button class="btn btn-default btn-sm" @click="backToArticles">返回文章</button>
    </div>

    <p v-if="researchStore.graphError" class="graph-error">{{ researchStore.graphError }}</p>

    <!-- 画布：加载态分支必须先于空态，否则拉取期间会闪一下"暂无关系" -->
    <div class="graph-stage">
      <div v-if="researchStore.graphLoading" class="state-block">
        <div class="skeleton-list">
          <div class="sk-line" v-for="i in 6" :key="i"></div>
        </div>
      </div>
      <EntityGraph
        v-else-if="hasGraph"
        :nodes="researchStore.graph!.nodes"
        :edges="researchStore.graph!.edges"
      />
      <div v-else class="state-block">
        <p>暂无可展示的实体关系</p>
        <p class="state-hint">
          两个实体必须同时出现在同一篇文章的标题或摘要里才会连线。可在研究工作台的「实体管理」里
          多添加几个关注实体，或把时间窗放宽到「全部」后重试。
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 主屏关系图视图
 *
 * # 职责
 * 把实体共现关系图以全宽展示——弹窗里的画布太小，看不出长程结构。
 * 这是主屏的一种"内容模式"（与文章阅读互斥）：App.vue 在 `uiStore.mainView`
 * 为 `'graph'` 时以本组件替换文章列表栏与正文栏；`'articles'` 时恢复三栏。
 *
 * # 与研究工作台图页签的关系
 * 两者是**同一份数据的两个渲染位**：参数（时间窗 / 最小共现数）、图数据与
 * 重算动作全部收在 `researchStore`，改哪一处的参数两处都同步。页签侧在
 * 主屏模式激活时会用提示替代画布——避免两套 D3 力导向仿真同时运行。
 *
 * # 数据加载时机
 * 挂载时图尚未加载过（`graph === null`）才重算一次；已加载过则直接复用
 * store 里的现成结果（从弹窗「放大到主屏」过来时必然已加载）。
 * 参数变化的监听用 `watch` + `immediate: false`：挂载时的首次加载由
 * onMounted 显式触发，二者不重叠。
 */
import { computed, onMounted, watch } from 'vue'
import AppSelect from './AppSelect.vue'
import EntityGraph from './EntityGraph.vue'
import { useResearchStore } from '@/stores/research'
import { useUiStore } from '@/stores/ui'

const researchStore = useResearchStore()
const uiStore = useUiStore()

/** 关系图是否有可渲染的内容（节点非空才算有图） */
const hasGraph = computed<boolean>(() => (researchStore.graph?.nodes.length ?? 0) > 0)

/** 关系图统计文案：节点数 / 边数 / 参与扫描的文章数，截断时追加说明 */
const graphStatText = computed<string>(() => {
  const g = researchStore.graph
  if (!g) return ''
  const base = `${g.nodes.length} 个实体 · ${g.edges.length} 条关系 · 已分析 ${g.scanned_articles} 篇`
  return g.truncated ? `${base}（仅显示共现最强的一部分）` : base
})

/** 回到文章阅读（App.vue 恢复三栏渲染） */
function backToArticles() {
  uiStore.setMainView('articles')
}

// 挂载时首次加载：只在本会话从未算过图时触发，避免重复全库扫描
onMounted(() => {
  if (researchStore.graph === null) researchStore.reloadGraph()
})

// 参数变化即重算：本视图必然可见（它挂载着才会改参数），无需额外守卫
watch(
  () => [researchStore.graphDays, researchStore.graphMinWeight],
  () => researchStore.reloadGraph()
)
</script>

<style scoped>
/* 视图容器：占满 .app-body 中 FeedPanel 右侧的全部空间（对齐 ArticleColumn
   的 flex 布局约定）；overflow hidden 与弹窗内的画布页签同理——平移缩放
   由画布内部处理，外层滚动条只会打架 */
.graph-view {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  padding: var(--sp-4) var(--sp-5);
  overflow: hidden;
  background: var(--surface);
}

.graph-title {
  margin: 0;
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
}

/* 把「返回文章」推到参数条最右端 */
.graph-bar-spacer {
  flex: 1;
}

/* 主屏空间充裕，画布的 min-height 兜底可以比弹窗里更宽松
   （全局 .graph-stage 的 260px 在此足够，无需覆盖） */
</style>
