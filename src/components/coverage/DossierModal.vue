<template>
  <!-- 「研究一页」预览。

       刻意不做编辑器：这一页的终点是 Obsidian / Word，用户在那里改。
       在这里做编辑，就得处理保存、冲突、块合并（projection 那一套），
       而这页的全部价值是"把散在各处的判断拼成一页带引用东西"，
       不是"再给用户一个编辑器"（ROADMAP §6 明确不做富文本）。

       **本组件现在是壳**：预览、复制、另存为三件事全部在
       `MarkdownPreviewModal` 里。保留这层壳，是为了让 `CoveragePanel` 的
       调用点与 props 契约保持原样——覆盖域不该知道"预览长什么样"，
       也不该跟着通用预览的样式一起改。 -->
  <MarkdownPreviewModal
    :open="open"
    :title="titleText"
    :markdown="dossier?.markdown ?? ''"
    :loading="loading"
    :error="error"
    :default-name="dossier?.defaultName ?? ''"
    :hint="hint"
    @close="emit('close')"
  />
</template>

<script setup lang="ts">
/**
 * 「研究一页」预览弹窗（覆盖域）
 *
 * # 职责
 * 把 `coverage_dossier` 的结果交给通用预览壳，并补上这个域特有的标题与说明。
 *
 * # 为什么说明文字留在这里而不是通用壳里
 * "收录 N 条判断 · M 条证据"是这一种产物的统计口径，只有覆盖域知道怎么算；
 * 通用壳不该为一个调用方长出专属文案。
 *
 * # 复制为什么复制原文
 * 复制 HTML 粘到 Obsidian 里会变成富文本，用户再也没法按自己的习惯重排。
 * 这条约定现在只写在 `MarkdownPreviewModal` 一处，本组件不再重复实现。
 */
import { computed } from 'vue'
import MarkdownPreviewModal from '@/components/MarkdownPreviewModal.vue'
import type { Dossier } from '@/stores/coverage'

const props = defineProps<{
  /** 是否打开 */
  open: boolean
  /** 组装结果；为空且不在加载中时显示失败态 */
  dossier: Dossier | null
  /** 是否正在生成 */
  loading: boolean
  /** 生成失败原因 */
  error: string
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

/** 弹窗标题：组装结果自带的标题，缺省时给一个中性的名字 */
const titleText = computed(() => props.dossier?.title ?? '研究一页')

/**
 * 正文上方的一行说明
 *
 * 只在拿到结果时给：加载中给统计数字会让用户以为内容已经到了，
 * 失败时给则与失败态自相矛盾。
 */
const hint = computed(() =>
  props.dossier
    ? `收录 ${props.dossier.thesisCount} 条判断 · ${props.dossier.evidenceCount} 条证据。这一页只是骨架（标题 + 列表 + 引用），排版请在 Obsidian / Word 里做。`
    : '',
)
</script>
