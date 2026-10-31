<template>
  <!-- 论点面板：列表 ↔ 详情的切换容器。
       为什么用"同层切换"而不是"详情另开弹窗"：
       论点详情里最常见的事情是摘证据、跳原文、再回来接着看——
       如果详情是叠在列表上的第二个弹窗，返回时列表的滚动位置与筛选会被重排，
       用户会反复"找回我刚才看的是哪一条"。同层切换只换内容区，没有这个代价。 -->
  <ThesisDetail
    v-if="store.detail"
    :thesis-id="store.detail.thesis.id"
    @back="back"
    @open-article="onOpenArticle"
    @deleted="onDeleted"
  />
  <ThesisList v-else />
</template>

<script setup lang="ts">
/**
 * 论点面板（页签容器）
 *
 * # 职责
 * 在「论点列表」与「论点详情」之间切换，并把子组件的事件往上抛给
 * 研究工作台外壳（`ResearchModal`）。
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
 */
import { useThesesStore } from '@/stores/theses'
import ThesisList from './ThesisList.vue'
import ThesisDetail from './ThesisDetail.vue'

const store = useThesesStore()

const emit = defineEmits<{
  /** 需要打开原文：交给外壳决定是关弹窗跳主屏，还是就地预览 */
  (e: 'open-article', articleId: number): void
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
 */
function onOpenArticle(articleId: number) {
  store.closeThesis()
  emit('open-article', articleId)
}

/** 论点被删除后回列表 */
function onDeleted() {
  store.closeThesis()
}
</script>
