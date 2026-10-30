<template>
  <!-- 订阅源管理弹窗：改名、重新归类、更换文件夹、修改链接、AI 配置、删除。
       由 feed 条目上的「⋯」按钮（@click.stop）打开，外壳以 managingFeed 非空持有；
       Esc / 遮罩点击 / 焦点陷阱 / 滚动锁由 BaseModal 统一提供。
       删除为破坏性操作，先经全局弹窗二次确认。 -->
  <BaseModal :open="!!feed" title="管理订阅源" size="sm" @close="emit('close')">
    <div class="manage-head">
      <span class="m-title">管理订阅源</span>
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

/**
 * 删除当前订阅源
 *
 * 与订阅栏右键菜单的「取消订阅」走同一条路径：不弹确认框，改为「立即移除 + 6 秒撤销」。
 * 失败由 store 内部弹 toast 并自动还原条目，故这里不再回填 `manageMsg`；
 * 先关弹窗再删——弹窗留在屏幕上等一个已经可撤销的删除，只会挡住后面要看的界面。
 */
async function handleDelete() {
  if (!props.feed) return
  const feed = props.feed
  emit('close')
  await feedsStore.unsubscribeFeed(feed.id)
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
  /* 字号不再挂在容器上：原先标题 span 靠继承取容器的 14/600，于是改标题必然
     牵连同容器里的关闭钮。现在字号只属于标题这个角色。 */
  flex: 0 0 auto;
}
.m-title {
  font-size: var(--fs-modal-title);
  font-weight: 600;
  line-height: var(--lh-modal-title);
  color: var(--text-primary);
}
.manage-close {
  border: none;
  background: none;
  cursor: pointer;
  /* × 是图形不是文字：18px 字形 + 32×32 命中区（WCAG 2.5.8 下限 24×24）。
     负 margin 抵掉命中区多占的空间，头部高度不变。 */
  font-size: var(--fs-xl);
  line-height: 1;
  min-width: 32px;
  min-height: 32px;
  display: grid;
  place-items: center;
  margin: -8px -8px -8px 0;
  color: var(--text-tertiary);
}
.manage-close:hover { color: var(--text-primary); }

/* 错误提示 */
.manage-msg {
  color: var(--primary);
  background: var(--primary-fg);
  /* 这是失败原因，用户要读懂才知道怎么改 —— 属于正文，不是元信息 */
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  padding: var(--sp-2) var(--sp-4);
  border-bottom: 1px solid var(--border);
  flex: 0 0 auto;
}

/* Tab 导航 */
.manage-tabs {
  display: flex;
  border-bottom: 1px solid var(--border);
  flex: 0 0 auto;
}
.manage-tab {
  flex: 1;
  /* 原为 var(--sp-25)：该令牌从未定义（间距阶梯里只有 --sp-025 / --sp-05），
     CSS 里 var(--未定义) 会让整条 padding 声明静默作废，表现为「内间距为 0」。 */
  padding: var(--sp-05) var(--sp-3);
  font-size: var(--fs-modal-label);
  font-weight: 500;
  color: var(--text-secondary);
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  cursor: pointer;
  /* 原先只声明了 color 与 border-color 两个属性（时长 150ms）：漏了 background，
     将来给选中态加底色就会硬跳。三属性显式列出，并用 T1 档（操作反馈比面板进出快）。 */
  transition:
    color var(--m-tap) var(--m-ease-out),
    background var(--m-tap) var(--m-ease-out),
    border-color var(--m-tap) var(--m-ease-out);
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
  /* 面板是 max-height:86vh + overflow:hidden，自己并不滚。两个 tab 各有一个
     .manage-body，共用这条规则一起变成滚动容器。
     min-height:0 是关键：缺了它 flex 子项不会收缩，overflow 永远不生效。 */
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}
.ai-config-body {
  gap: var(--sp-4);
}

/* 表单字段 */
.field { display: flex; flex-direction: column; gap: var(--sp-05); }
.field label {
  font-size: var(--fs-modal-label);
  font-weight: 500;
  line-height: var(--lh-modal-label);
  color: var(--text-secondary);
}
.field input[type="text"] {
  height: 32px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-15);
  font-size: var(--fs-modal-label);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: box-shadow var(--m-tap) var(--m-ease-out);
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
  font-size: var(--fs-modal-label);
  color: var(--text-secondary);
  cursor: pointer;
  font-family: inherit;
  flex-shrink: 0;
}
.copy-btn:hover { border-color: var(--primary); color: var(--primary); }

.field-hint {
  margin: var(--sp-05) 0 0;
  /* 用户必须读懂才能正确填写 —— 属于正文 */
  font-size: var(--fs-modal-body);
  color: var(--text-secondary);
  line-height: var(--lh-modal-body);
}

/* 指令文本框 */
.prompt-textarea {
  width: 100%;
  min-height: 96px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: var(--sp-15);
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  resize: vertical;
  font-family: inherit;
  transition: box-shadow var(--m-tap) var(--m-ease-out);
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
  /* 主操作区不参与收缩，滚动时始终可见 */
  flex: 0 0 auto;
}
.foot-right { display: flex; gap: var(--sp-2); }
</style>
