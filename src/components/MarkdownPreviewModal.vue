<template>
  <!-- 只读 markdown 预览与两个出口（复制原文 / 另存为 .md）。

       这是「研究一页」与「论点组装」共用的**唯一**预览壳：两者产出的都是一段
       markdown，区别只在后端怎么拼。之所以抽成一个组件，是因为"复制走原文、
       渲染只用于看"这条约定必须只有一处实现——两个壳各写一遍，迟早一个改成
       复制渲染后的文本、另一个还在复制原文，用户拿到的东西就不一样了。 -->
  <BaseModal :open="open" :title="title" size="xl" @close="emit('close')">
    <div class="pv-bar">
      <span class="m-title">{{ title }}</span>
      <span class="spacer"></span>
      <button
        class="btn btn-default btn-sm"
        type="button"
        :disabled="!markdown || !!error"
        @click="copyMarkdown"
      >
        复制 Markdown
      </button>
      <button
        class="btn btn-default btn-sm"
        type="button"
        :disabled="!markdown || !!error"
        @click="saveAs"
      >
        另存为 .md
      </button>
      <button class="pv-x" type="button" aria-label="关闭" @click="emit('close')">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      </button>
    </div>

    <!-- 三态互斥：失败 / 加载 / 内容。
         失败态不复用空态文案——"取数失败"和"这份东西本来就是空的"是两件事，
         混成一句"暂无内容"会让用户以为自己的证据丢了 -->
    <div v-if="error" class="pv-state error">
      <p class="pv-state-title">生成失败</p>
      <p class="pv-state-msg">{{ error }}</p>
    </div>
    <div v-else-if="loading" class="pv-state">正在拼装…</div>
    <div v-else class="pv-body">
      <p v-if="hint" class="pv-hint">{{ hint }}</p>
      <!-- 渲染的是 markdown，复制的是原文：粘到 Obsidian 里仍可重新排版 -->
      <article class="rc pv-doc" v-html="html"></article>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 只读 markdown 预览弹窗
 *
 * # 职责
 * 把后端拼好的 markdown 渲染出来，并提供两个出口：复制原文、另存为 .md。
 *
 * # 为什么复制的是原文而不是渲染后的 DOM
 * 复制 HTML 粘到 Obsidian 里会变成一段富文本，用户再也没法按自己的习惯重排。
 * 复制原文，粘到哪里都是可编辑的 markdown。
 *
 * # 为什么不做「写入 Obsidian」
 * 写 Vault 需要解析 `obsidian_vault_path` + `obsidian_export_folder` 两个设置，
 * 这套逻辑已在 `obsidian.rs`（文章导出）与 `report.rs`（报告落盘）各有一份。
 * 再抄第三份，将来改目录规则就要改三处、漏一处就写错地方。
 * 「另存为」由用户自己选路径，可以直接存进 Vault，零新增耦合。
 *
 * # 参数
 * * `open` - 是否打开
 * * `title` - 弹窗标题（同时用于无障碍播报）
 * * `markdown` - 后端拼好的原文；空串时表示"确实没有内容"
 * * `loading` - 是否正在生成
 * * `error` - 生成失败原因；非空时优先于内容展示
 * * `defaultName` - 另存为时的默认文件名（含扩展名，由后端补全）
 * * `hint` - 正文上方的一行说明，可为空
 *
 * # 事件
 * * `close` - 用户按 Esc、点遮罩或 ✕
 */
import { computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import BaseModal from '@/components/BaseModal.vue'
import { useToastStore } from '@/stores/toast'
import { renderMarkdown } from '@/utils/markdown'
// 正文排版（`.rc`）与周期报告共用一份，别在这里重写
import '@/styles/report-content.css'

const props = withDefaults(
  defineProps<{
    /** 是否打开 */
    open: boolean
    /** 弹窗标题 */
    title: string
    /** markdown 原文 */
    markdown: string
    /** 是否正在生成 */
    loading?: boolean
    /** 生成失败原因 */
    error?: string
    /** 另存为的默认文件名 */
    defaultName?: string
    /** 正文上方的一行说明 */
    hint?: string
  }>(),
  { loading: false, error: '', defaultName: '', hint: '' },
)

const emit = defineEmits<{
  (e: 'close'): void
}>()

const toast = useToastStore()

/** 渲染后的正文 */
const html = computed(() => (props.markdown ? renderMarkdown(props.markdown) : ''))

/**
 * 复制 markdown 原文
 *
 * 失败必须让用户知道：剪贴板权限在部分桌面环境会被拒，静默失败的表现是
 * "点了没反应、粘出来还是旧内容"，比直接报错更难查。
 */
async function copyMarkdown() {
  if (!props.markdown) return
  try {
    await navigator.clipboard.writeText(props.markdown)
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
  if (!props.markdown) return
  try {
    const path = await invoke<string | null>('report_export_write', {
      defaultName: props.defaultName,
      content: props.markdown,
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
.pv-bar {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-4) var(--sp-5);
  border-bottom: 1px solid var(--glass-hair);
  flex-shrink: 0;
}
.pv-bar .spacer {
  flex: 1 1 auto;
  min-width: 0;
}
.pv-x {
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
.pv-x:hover {
  background: var(--hover-bg);
  color: var(--text-primary);
}
.pv-x:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}

/* 正文滚动容器三件套：flex:1 1 auto + min-height:0 + overflow-y:auto。
   少了 min-height:0，内容会把弹窗撑高而不是内部滚动 */
.pv-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-4) var(--sp-5) var(--sp-5);
}
.pv-hint {
  margin: 0 0 var(--sp-3);
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
.pv-doc {
  /* 正文舒适行宽上限见设计约定：这一页是给人读的，不是给屏占比的 */
  max-width: 34em;
}

.pv-state {
  padding: var(--sp-8) var(--sp-5);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--fs-modal-body);
}
.pv-state.error {
  color: var(--danger);
}
.pv-state-title {
  margin: 0 0 var(--sp-2);
  font-weight: 500;
  color: var(--text-primary);
}
.pv-state-msg {
  margin: 0;
  font-size: var(--fs-modal-label);
}
</style>
