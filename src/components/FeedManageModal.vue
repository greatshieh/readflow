<template>
  <!-- 订阅源管理弹窗：改名、重新归类、更换文件夹、修改链接、AI 配置、删除。
       由 feed 条目上的「⋯」按钮（@click.stop）打开，外壳以 managingFeed 非空持有；
       Esc / 遮罩点击 / 焦点陷阱 / 滚动锁由 BaseModal 统一提供。
       删除为破坏性操作，先经全局弹窗二次确认。 -->
  <BaseModal :open="!!feed" title="管理订阅源" size="sm" @close="emit('close')">
    <div class="manage-head">
      <span>管理订阅源</span>
      <button class="manage-close" @click="emit('close')" aria-label="关闭">×</button>
    </div>

    <!-- 错误提示 -->
    <div class="manage-msg" v-if="manageMsg">{{ manageMsg }}</div>

    <!-- Tab 导航 -->
    <div class="manage-tabs">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        class="manage-tab"
        :class="{ active: activeTab === tab.key }"
        @click="activeTab = tab.key"
      >
        {{ tab.label }}
      </button>
    </div>

    <!-- 基础信息 tab -->
    <div v-if="activeTab === 'basic'" class="manage-body">
      <div class="field">
        <label>名称</label>
        <input v-model="editName" type="text" placeholder="订阅源名称" />
      </div>
      <div class="field">
        <label>文件夹</label>
        <AppSelect v-model="editFolderId" :options="folderOptions" />
      </div>
      <div class="field">
        <label>链接</label>
        <div class="url-row">
          <input v-model="editUrl" type="text" placeholder="https://example.com/feed.xml" />
          <button class="copy-btn" @click="copyUrl">复制</button>
        </div>
        <p class="field-hint">支持 RSS 地址，或 RSSHub 路由（如 /github/trending/daily）</p>
      </div>
    </div>

    <!-- AI 配置 tab -->
    <div v-if="activeTab === 'ai'" class="manage-body ai-config-body">
      <div class="field">
        <label>摘要指令</label>
        <textarea
          v-model="editSummaryPrompt"
          class="prompt-textarea"
          placeholder="留空使用全局默认指令"
          rows="4"
        ></textarea>
        <p class="field-hint">自定义 AI 摘要指令，覆盖全局设置。留空则使用默认。</p>
      </div>
      <div class="field">
        <label>提取指令</label>
        <textarea
          v-model="editExtractPrompt"
          class="prompt-textarea"
          placeholder="留空使用全局默认指令"
          rows="4"
        ></textarea>
        <p class="field-hint">自定义研究事件提取指令，覆盖全局设置。留空则使用默认。</p>
      </div>
      <div class="field">
        <label>语言偏好</label>
        <AppSelect v-model="editLanguage" :options="languageOptions" />
      </div>
    </div>

    <div class="manage-foot">
      <button class="btn btn-danger" @click="handleDelete">删除</button>
      <div class="foot-right">
        <button class="btn" @click="emit('close')">取消</button>
        <button
          v-if="activeTab === 'ai'"
          class="btn btn-primary"
          :disabled="savingAi"
          @click="handleSaveAi"
        >
          {{ savingAi ? '保存中…' : '保存配置' }}
        </button>
        <button
          v-else
          class="btn btn-primary"
          :disabled="saving"
          @click="handleSave"
        >
          {{ saving ? '保存中…' : '保存' }}
        </button>
      </div>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 订阅源管理弹窗
 *
 * # 职责
 * 单个订阅源的查看与维护：重命名、移动文件夹、修改链接、AI 配置、删除（二次确认）。
 * 由 FeedPanel 以 `:feed` 传入当前管理的源，`feed` 为 null 即弹窗关闭。
 */

import { ref, computed, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import AppSelect from './AppSelect.vue'
import { useFeedsStore } from '@/stores/feeds'
import { useModalStore } from '@/stores/modal'
import type { Feed, FeedAiConfig } from '@/types'

const props = defineProps<{
  /** 当前管理的订阅源；null 表示弹窗关闭 */
  feed: Feed | null
}>()

const emit = defineEmits<{
  /** 用户按 Esc / 点遮罩 / 点 × / 取消 / 操作成功后请求关闭；由父组件把 feed 置空 */
  (e: 'close'): void
}>()

const feedsStore = useFeedsStore()
const modalStore = useModalStore()

// ─── Tab 控制 ───────────────────────────────────────────────────────────────
const tabs = [
  { key: 'basic', label: '基础信息' },
  { key: 'ai', label: 'AI 配置' }
] as const
type TabKey = typeof tabs[number]['key']
const activeTab = ref<TabKey>('basic')

// ─── 基础信息表单 ───────────────────────────────────────────────────────────
const editName = ref('')
const editUrl = ref('')
const editFolderId = ref(0)
const saving = ref(false)
const manageMsg = ref('')

const folderOptions = computed(() =>
  feedsStore.folders.map((f) => ({ value: f.id, label: f.name }))
)

// ─── AI 配置表单 ────────────────────────────────────────────────────────────
const editSummaryPrompt = ref('')
const editExtractPrompt = ref('')
const editLanguage = ref('auto')
const savingAi = ref(false)
const currentAiConfig = ref<FeedAiConfig | null>(null)

const languageOptions = [
  { value: 'auto', label: '自动' },
  { value: 'zh', label: '中文' },
  { value: 'en', label: '英文' }
]

// 打开弹窗时回填数据
watch(
  () => props.feed,
  (feed) => {
    if (!feed) return
    editName.value = feed.name
    editUrl.value = feed.url
    editFolderId.value = feed.folder_id || 0
    manageMsg.value = ''
    activeTab.value = 'basic'
    // 加载该源的 AI 配置
    feedsStore.getFeedAiConfig(feed.id).then((cfg) => {
      currentAiConfig.value = cfg
      editSummaryPrompt.value = cfg.summary_prompt
      editExtractPrompt.value = cfg.extract_prompt
      editLanguage.value = cfg.language
    }).catch(() => {
      editSummaryPrompt.value = ''
      editExtractPrompt.value = ''
      editLanguage.value = 'auto'
    })
  }
)

// ─── 基础信息操作 ───────────────────────────────────────────────────────────
async function copyUrl() {
  try {
    await navigator.clipboard.writeText(editUrl.value)
  } catch (e) {
    console.error('复制链接失败:', e)
  }
}

async function handleSave() {
  if (!props.feed) return
  const url = editUrl.value.trim()
  if (!url) {
    manageMsg.value = '订阅链接不能为空'
    return
  }
  saving.value = true
  manageMsg.value = ''
  try {
    const id = props.feed.id
    const name = editName.value.trim() || props.feed.name
    await feedsStore.updateFeed(id, url, name, editFolderId.value)
    emit('close')
  } catch (e) {
    manageMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    saving.value = false
  }
}

async function handleDelete() {
  if (!props.feed) return
  const feed = props.feed
  const ok = await modalStore.showConfirm({
    title: '删除订阅源',
    message: `确定删除订阅源「${feed.name}」吗？\n该源下的所有文章也会一并删除，且不可恢复。`
  })
  if (!ok) return
  try {
    await feedsStore.deleteFeed(feed.id)
    emit('close')
  } catch (e) {
    manageMsg.value = e instanceof Error ? e.message : String(e)
  }
}

// ─── AI 配置操作 ────────────────────────────────────────────────────────────
async function handleSaveAi() {
  if (!props.feed) return
  savingAi.value = true
  manageMsg.value = ''
  try {
    const config: FeedAiConfig = {
      id: currentAiConfig.value?.id || 0,
      feed_id: props.feed.id,
      summary_prompt: editSummaryPrompt.value.trim(),
      extract_prompt: editExtractPrompt.value.trim(),
      language: editLanguage.value,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString()
    }
    const id = await feedsStore.saveFeedAiConfig(config)
    currentAiConfig.value = { ...config, id }
    manageMsg.value = 'AI 配置已保存'
  } catch (e) {
    manageMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    savingAi.value = false
  }
}
</script>

<style scoped>
/* 头部 */
.manage-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-35) var(--sp-4);
  border-bottom: 1px solid var(--border);
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}
.manage-close {
  border: none;
  background: none;
  cursor: pointer;
  font-size: 22px;
  line-height: 1;
  color: var(--text-tertiary);
  padding: 0 var(--sp-1);
}
.manage-close:hover { color: var(--text-primary); }

/* 错误提示 */
.manage-msg {
  color: var(--primary);
  background: var(--primary-fg);
  font-size: var(--fs-xs);
  padding: var(--sp-2) var(--sp-4);
  border-bottom: 1px solid var(--border);
}

/* Tab 导航 */
.manage-tabs {
  display: flex;
  border-bottom: 1px solid var(--border);
}
.manage-tab {
  flex: 1;
  padding: var(--sp-25) var(--sp-3);
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}
.manage-tab:hover { color: var(--text-primary); }
.manage-tab.active {
  color: var(--primary);
  border-bottom-color: var(--primary);
}

/* 主体区域 */
.manage-body {
  padding: var(--sp-4);
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}
.ai-config-body {
  gap: var(--sp-4);
}

/* 表单字段 */
.field { display: flex; flex-direction: column; gap: var(--sp-05); }
.field label { font-size: var(--fs-xs); color: var(--text-tertiary); }
.field input[type="text"] {
  height: 32px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-15);
  font-size: var(--fs-sm);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: box-shadow 0.15s;
}
.field input[type="text"]:focus { box-shadow: 0 0 0 1px var(--primary); }
.field input[readonly] { color: var(--text-tertiary); cursor: default; }

.url-row { display: flex; gap: var(--sp-2); }
.url-row input { flex: 1; min-width: 0; }
.copy-btn {
  border: 1px solid var(--border);
  background: var(--surface);
  border-radius: var(--r-md);
  padding: 0 var(--sp-3);
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  cursor: pointer;
  font-family: inherit;
  flex-shrink: 0;
}
.copy-btn:hover { border-color: var(--primary); color: var(--primary); }

.field-hint {
  margin: var(--sp-05) 0 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}

/* 指令文本框 */
.prompt-textarea {
  width: 100%;
  min-height: 96px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-15);
  font-size: var(--fs-sm);
  line-height: 1.6;
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  resize: vertical;
  font-family: inherit;
  transition: box-shadow 0.15s;
}
.prompt-textarea:focus { box-shadow: 0 0 0 1px var(--primary); }
.prompt-textarea::placeholder { color: var(--text-tertiary); }

/* 底部 */
.manage-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-35) var(--sp-4);
  border-top: 1px solid var(--border);
}
.foot-right { display: flex; gap: var(--sp-2); }
</style>
