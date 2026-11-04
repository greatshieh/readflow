<template>
  <!-- 论点面板：列表 ↔ 详情的切换容器。
       为什么用"同层切换"而不是"详情另开弹窗"：
       论点详情里最常见的事情是摘证据、跳原文、再回来接着看——
       如果详情是叠在列表上的第二个弹窗，返回时列表的滚动位置与筛选会被重排，
       用户会反复"找回我刚才看的是哪一条"。同层切换只换内容区，没有这个代价。 -->
  <ThesisDetail
    v-if="store.detail"
    ref="detailRef"
    :thesis-id="store.detail.thesis.id"
    @back="back"
    @open-article="onOpenArticle"
    @deleted="onDeleted"
  />
  <ThesisList v-else @goto-review="emit('goto-review')" />
</template>

<script setup lang="ts">
/**
 * 论点面板（页签容器）
 *
 * # 职责
 * 在「论点列表」与「论点详情」之间切换，把子组件的事件往上抛给研究工作台外壳
 * （`ResearchModal`），并在"从原文返回"时把详情与滚动位置恢复回来。
 *
 * # 切换依据
 * `store.detail` 是否为 null：null = 列表，非 null = 详情。
 * 不用本地 `selectedId`：列表项点击直接 `store.openThesis(id)`，
 * 详情里删除论点会把 `detail` 置空 —— 两处都只改 store，容器不需要再记一份。
 * 多一份本地状态就多一条"本地与 store 不一致"的路径。
 *
 * # 为什么详情用 v-if 而列表不用
 * 详情每次打开都要重新拉数据并聚焦标题，v-if 保证重新挂载（重新执行
 * `bootstrap`）。列表保持挂载，从详情返回时筛选与滚动位置都还在。
 *
 * # 为什么恢复逻辑在容器这一层
 * 恢复要按 `thesisId` 先把详情打开（这是 store 的事），再去滚详情自己的容器
 * （这是详情组件的事）。两件事分别属于两侧，只有容器同时看得见两者。
 */
import { nextTick, ref } from 'vue'
import { useThesesStore } from '@/stores/theses'
import type { OriginRestore, OriginRestoreResult } from '@/stores/originTrail'
import type { ThesisLabelInput } from '@/utils/originLabel'
import { restoreScrollTop } from '@/utils/scrollRestore'
import ThesisList from './ThesisList.vue'
import ThesisDetail from './ThesisDetail.vue'

const store = useThesesStore()

/** 详情组件实例：只为取它的滚动容器（恢复位置用） */
const detailRef = ref<InstanceType<typeof ThesisDetail> | null>(null)

const emit = defineEmits<{
  /**
   * 需要打开原文：交给外壳决定是关弹窗跳主屏，还是就地预览
   *
   * 第二个参数是"跳转现场"（滚动量 + 论点快照），原样透传：
   * 本容器不解释它，只保证它在 `closeThesis()` **之前**已经被取出来。
   */
  (e: 'open-article', articleId: number, meta: { scrollTop: number; thesis: ThesisLabelInput }): void
  /** 请求进入复盘页：透传自列表，由外壳负责关弹窗 + 切主屏视图 */
  (e: 'goto-review'): void
}>()

/**
 * 返回列表
 *
 * 必须先清 `store.detail` 再抛事件：容器是按 `detail` 决定渲染谁，
 * 只抛事件不清 store 会一直停在详情页（父组件"收下事件但界面没动"）。
 */
function back() {
  store.closeThesis()
}

/**
 * 详情里点了「打开原文」
 *
 * @param articleId - 文章 ID
 * @param meta - 详情页给出的跳转现场（滚动量 + 论点快照）
 */
function onOpenArticle(
  articleId: number,
  meta: { scrollTop: number; thesis: ThesisLabelInput },
) {
  store.closeThesis()
  emit('open-article', articleId, meta)
}

/** 论点被删除后回列表 */
function onDeleted() {
  store.closeThesis()
}

/**
 * 从快照恢复页签（外壳在弹窗打开、数据就绪后调用）
 *
 * # 为什么失败不一律判"目标不存在"
 * `openThesis` 把两种完全不同的失败收进同一句错误串：论点真的没了、IPC 挂了。
 * 判死会让"网络抖一下"变成"这条论点再也回不去"（栏转失效态、只剩 ✕）。
 * 所以这里一律按**临时故障**上报：栏留着，用户可以再点一次，也可以 ✕ 收掉。
 * 代价是"论点被删"时用户会多看到一次失败提示——比丢失整个返回入口轻得多。
 *
 * # 顺序
 * 先 `openThesis`（详情是 v-if，数据到位才有容器），再 `nextTick` 等挂载，
 * 最后写滚动量；反序会滚到一个还不存在的元素上，静默无效。
 *
 * @param restore - 来源快照里的恢复指令（`thesisId` 为 null 时无从恢复）
 * @returns `ok` 表示已恢复；`transient` 带上可直接 toast 的原因
 */
async function restoreFrom(restore: OriginRestore): Promise<OriginRestoreResult> {
  const id = restore.thesisId
  if (id === null || id === undefined) return { ok: false, kind: 'missing' }
  await store.openThesis(id)
  if (store.detail?.thesis.id !== id) {
    return { ok: false, kind: 'transient', message: store.error || '这条论点打不开，请稍后再试' }
  }
  await nextTick()
  await restoreScrollTop(() => detailRef.value?.getScrollEl() ?? null, restore.scrollTop)
  return { ok: true }
}

defineExpose({ restoreFrom })
</script>
