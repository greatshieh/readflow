<template>
  <!-- ═══ Tab 4：关系图 ═══ -->
  <div class="m-body graph-body">
    <!-- 参数条：时间窗与最小共现数都会触发后端重算；参数与数据都收在 researchStore，
         与主屏关系图视图（GraphView）共享同一份，两处展示不会互相漂移 -->
    <div class="graph-bar">
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
      <!-- 放大到主屏：关系图撑满中右两栏，弹窗里画布太小看不清结构 -->
      <button class="btn btn-secondary btn-sm" @click="emit('expand')" title="在主屏全宽展示关系图">
        放大到主屏
      </button>
    </div>

    <p v-if="researchStore.graphError" class="graph-error">{{ researchStore.graphError }}</p>

    <!-- 画布：加载态分支必须先于空态，否则拉取期间会闪一下"暂无关系"。
         主屏视图（mainView === 'graph'）已在渲染同一份数据的画布实例，
         这里若再挂一个会有两套 D3 仿真同时跑，故以提示替代。 -->
    <div class="graph-stage">
      <div v-if="researchStore.graphLoading" class="state-block">
        <div class="skeleton-list">
          <div class="sk-line" v-for="i in 4" :key="i"></div>
        </div>
      </div>
      <div v-else-if="uiStore.mainView === 'graph'" class="state-block">
        <p>关系图正在主屏全宽展示</p>
        <p class="state-hint">关闭主屏视图后这里会恢复显示。</p>
      </div>
      <EntityGraph
        v-else-if="hasGraph"
        :nodes="researchStore.graph!.nodes"
        :edges="researchStore.graph!.edges"
      />
      <div v-else class="state-block">
        <p>暂无可展示的实体关系</p>
        <p class="state-hint">
          两个实体必须同时出现在同一篇文章的标题或摘要里才会连线。可在「实体管理」多添加几个关注实体，
          或把时间窗放宽到「全部」后重试。
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 研究工作台 · 关系图页签
 *
 * 把「同时出现在同一篇文章里的实体」画成一张力导向共现图（画布本体是
 * 独立的 `EntityGraph` 组件），时间窗与最小共现数可调，二者变化即重算。
 * 参数、数据与重算动作都收在 `researchStore`（与主屏关系图视图共享），
 * 本页签只负责弹窗内的展示与入口（放大到主屏）。
 *
 * # 挂载语义（与外壳的契约）
 * 本页签经 `v-show` 常驻挂载，**不能用 `onMounted` 当"进入页签"信号**：
 * - 首次切入（`active` 翻真且从未加载过）→ 重算——对应拆分前 `switchToGraph`
 *   的"只在首次进入时拉数据，来回点页签不该反复触发全库扫描"；
 * - 实体页签做过增删/启停/评分（外壳转来的 `epoch` 递增）且图已加载过 →
 *   下次切入时重算。拆分前是评分后立即后台重算，现在推迟到用户真正
 *   切到本页签时——省掉一次看不见的全库扫描，图上的统计文案同步更新；
 * - 参数变化 → 仅在本页签可见时重算（拆分前用 `tab === 'graph'` 守卫，
 *   现在等价于 `active` 守卫）。
 */
import { ref, computed, watch } from 'vue'
import AppSelect from './AppSelect.vue'
import EntityGraph from './EntityGraph.vue'
import { useResearchStore } from '@/stores/research'
import { useUiStore } from '@/stores/ui'

const props = defineProps<{
  /** 是否为当前显示的页签：首次切入或图已过期时重算 */
  active: boolean
  /** 弹窗是否打开（重算守卫的一部分） */
  open: boolean
  /** 实体变更纪元：实体页签每次增删/启停/评分后 +1，图随之过期 */
  epoch: number
}>()

const emit = defineEmits<{
  /** 用户点击「放大到主屏」：外壳切换主屏视图并关闭弹窗 */
  (e: 'expand'): void
}>()

const researchStore = useResearchStore()
const uiStore = useUiStore()

/** 实体变更导致的过期标记：只在图已加载过后才有"过期"可言 */
const stale = ref(false)

/** 关系图是否有可渲染的内容（节点非空才算有图） */
const hasGraph = computed<boolean>(() => (researchStore.graph?.nodes.length ?? 0) > 0)

/** 关系图统计文案：节点数 / 边数 / 参与扫描的文章数，截断时追加说明 */
const graphStatText = computed<string>(() => {
  const g = researchStore.graph
  if (!g) return ''
  const base = `${g.nodes.length} 个实体 · ${g.edges.length} 条关系 · 已分析 ${g.scanned_articles} 篇`
  return g.truncated ? `${base}（仅显示共现最强的一部分）` : base
})

// 实体变更 → 图过期（图从未加载过则不标：首次切入本来就会拉）
watch(
  () => props.epoch,
  () => {
    if (researchStore.graph !== null) stale.value = true
  }
)

// 切入本页签：首次加载，或图已过期时重算
watch(
  () => props.active,
  vis => {
    if (!vis || !props.open) return
    if (researchStore.graph === null || stale.value) {
      stale.value = false
      researchStore.reloadGraph()
    }
  }
)

/**
 * 监听关系图参数变化：仅在本页签可见时重算
 *
 * 加 `active` 守卫对应拆分前的 `tab === 'graph'` 判断：避免用户根本没在看图时
 * 也触发一次全库扫描。参数控件只存在于本页签，变化必然发生在可见时，
 * 守卫属于防御性保留。
 */
watch([() => researchStore.graphDays, () => researchStore.graphMinWeight], () => {
  if (props.active && props.open) researchStore.reloadGraph()
})
</script>

<style scoped>
/* 覆盖 .m-body 的纵向滚动：画布自己占满剩余高度，由内部平移缩放处理视野，
   外层再出现滚动条会让「拖拽平移」和「滚动页面」互相打架。
   用双类选择器压过外壳 scoped 的 `.m-body`（二者特异性相同，靠书写确定性取胜）。
   高度不用再写死：外壳迁到 layout="side" 固定高度弹窗后，本页签的根元素
   以 flex:1 占满右列剩余空间（.side-main 为固定高度的纵向 flex），
   画布自然有可分配的高度。参数条 / 统计 / 错误 / 舞台样式已提升为全局
   （styles.css 的「关系图舞台共用原语」，与主屏 GraphView 共用）。 */
.m-body.graph-body {
  overflow: hidden;
}
</style>
