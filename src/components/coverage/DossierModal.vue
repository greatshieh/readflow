<template>
  <!-- 「研究一页」只读预览。
       刻意不做编辑器：这一页的终点是 Obsidian / Word，用户在那里改。
       在这里做编辑，就得处理保存、冲突、块合并（projection 那一套），
       而这页的全部价值是"把散在各处的判断拼成一页带引用东西"，
       不是"再给用户一个编辑器"（ROADMAP §6 明确不做富文本）。 -->
  <BaseModal :open="open" :title="titleText" size="xl" @close="emit('close')">
    <div class="dm-bar">
      <span class="m-title">{{ titleText }}</span>
      <span class="spacer"></span>
      <button class="btn btn-default btn-sm" type="button" @click="copyMarkdown" :disabled="!dossier">
        复制 Markdown
      </button>
      <button class="btn btn-default btn-sm" type="button" @click="saveAs" :disabled="!dossier">
        另存为 .md
      </button>
      <button class="dm-x" type="button" aria-label="关闭" @click="emit('close')">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      </button>
    </div>

    <!-- 失败态：不复用空态文案。取数失败与"这个标的没内容"是两件事，
         混成一句"暂无内容"会让用户以为自己的证据丢了 -->
    <div v-if="error" class="dm-state error">
      <p class="dm-state-title">生成失败</p>
      <p class="dm-state-msg">{{ error }}</p>
    </div>
    <div v-else-if="loading" class="dm-state">正在拼装…</div>
    <div v-else-if="dossier" class="dm-body">
      <p class="dm-hint">
        收录 {{ dossier.thesisCount }} 条判断 · {{ dossier.evidenceCount }} 条证据。
        这一页只是骨架（标题 + 列表 + 引用），排版请在 Obsidian / Word 里做。
      </p>
      <article class="rc dm-doc" v-html="html"></article>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 「研究一页」预览弹窗
 *
 * # 职责
 * 把后端拼好的 markdown 原文渲染出来，并提供两个出口：复制原文、另存为 .md。
 *
 * # 为什么复制用的是原文而不是渲染后的 DOM
 * 复制 HTML 粘到 Obsidian 里会变成一段富文本，用户再也没法按自己的习惯重新排版。
 * 复制原文，粘到哪里都是可编辑的 markdown——这才符合"只出骨架"的定位。
 *
 * # 为什么没有「写入 Obsidian」
 * 写 Vault 需要解析 `obsidian_vault_path` + `obsidian_export_folder` 两个设置并
 * 处理文件名清洗，目前这套逻辑在两处（`obsidian.rs` 的文章导出、`report.rs` 的
 * 报告落盘）。再抄第三份，将来改目录规则就要改三处、漏一处就写错地方。
 * 「另存为」由用户自己选路径（可以直接存进 Vault），零新增耦合。
 */
import { computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import BaseModal from '@/components/BaseModal.vue'
import { useToastStore } from '@/stores/toast'
import { renderMarkdown } from '@/utils/markdown'
import type { Dossier } from '@/stores/coverage'
// 报告正文的排版规则（`.rc` 容器）与周期报告共用一份，别在这里重写
import '@/styles/report-content.css'

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

const toast = useToastStore()

/** 弹窗标题：组装结果自带的标题，缺省时给一个中性的名字 */
const titleText = computed(() => props.dossier?.title ?? '研究一页')

/** 渲染后的正文 */
const html = computed(() => (props.dossier ? renderMarkdown(props.dossier.markdown) : ''))

/**
 * 复制 markdown 原文
 *
 * 失败必须让用户知道：剪贴板权限在部分桌面环境会被拒，
 * 静默失败的表现是"点了没反应、粘出来是旧内容"，比报错更难查。
 */
async function copyMarkdown() {
  if (!props.dossier) return
  try {
    await navigator.clipboard.writeText(props.dossier.markdown)
    toast.show('已复制 markdown 原文')
  } catch (e) {
    toast.show(`复制失败：${e instanceof Error ? e.message : String(e)}`)
  }
}

/**
 * 另存为 .md
 *
 * 由后端弹系统保存对话框（`report_export_write`，与两类报告共用同一出口）：
 * 文件类型过滤器、扩展名补全都在那里统一处理，前端不自己拼路径。
 */
async function saveAs() {
  if (!props.dossier) return
  try {
    const path = await invoke<string | null>('report_export_write', {
      defaultName: props.dossier.defaultName,
      content: props.dossier.markdown,
      format: 'md',
    })
    // null = 用户取消，不该提示成功也不该提示失败
    if (path) toast.show(`已保存到 ${path}`)
  } catch (e) {
    toast.show(`保存失败：${e instanceof Error ? e.message : String(e)}`)
  }
}
</script>

<style scoped>
/* 头部一行：标题在左，两个出口与关闭钮在右。
   绝对定位按钮组的老毛病是压住标题，这里用 flex + 标题 flex:1 避掉 */
.dm-bar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-4) var(--sp-5);
  border-bottom: 1px solid var(--glass-hair);
  flex-shrink: 0;
}
.dm-bar .spacer {
  flex: 1 1 auto;
  min-width: 0;
}
.dm-x {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  border: none;
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}
.dm-x:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}
.dm-x:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}

/* 正文滚动容器三件套：flex:1 1 auto + min-height:0 + overflow-y:auto。
   少了 min-height:0，内容会把弹窗撑高而不是内部滚动 */
.dm-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-4) var(--sp-5) var(--sp-5);
}
.dm-hint {
  margin: 0 0 var(--sp-3);
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
.dm-doc {
  /* 正文舒适行宽上限见设计约定；这一页是给人读的，不是给屏占比的 */
  max-width: 34em;
}

.dm-state {
  padding: var(--sp-8) var(--sp-5);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--fs-modal-body);
}
.dm-state.error {
  color: var(--danger);
}
.dm-state-title {
  margin: 0 0 var(--sp-2);
  font-weight: 500;
  color: var(--text-primary);
}
.dm-state-msg {
  margin: 0;
  font-size: var(--fs-modal-label);
}
</style>
