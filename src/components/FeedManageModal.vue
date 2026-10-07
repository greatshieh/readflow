<template>
  <!-- 订阅源管理弹窗：改名、重新归类、更换文件夹、修改链接、删除。
       由 feed 条目上的「⋯」按钮（@click.stop）打开，外壳以 managingFeed 非空持有；
       Esc / 遮罩点击 / 焦点陷阱 / 滚动锁由 BaseModal 统一提供。
       删除为破坏性操作，先经全局弹窗二次确认。 -->
  <BaseModal :open="!!feed" title="管理订阅源" size="sm" @close="emit('close')">
    <div class="manage-head">
      <span>管理订阅源</span>
      <button class="manage-close" @click="emit('close')" aria-label="关闭">×</button>
    </div>
    <!-- 操作反馈：保存失败时内联提示（删除走 confirm，不在此处） -->
    <div class="manage-msg" v-if="manageMsg">{{ manageMsg }}</div>
    <div class="manage-body">
      <div class="field">
        <label>名称</label>
        <!-- 重命名：v-model 绑定到副本 editName，保存时交给后端更新 -->
        <input v-model="editName" type="text" placeholder="订阅源名称" />
      </div>
      <div class="field">
        <label>文件夹</label>
        <!-- 移动到文件夹（folder_id）；选择"未分类"即移出当前文件夹 -->
        <AppSelect v-model="editFolderId" :options="folderOptions" />
      </div>
      <div class="field">
        <label>链接</label>
        <!-- 可编辑：改地址即让该订阅指向新源，已抓取的历史文章与书签保留；
             保存后若链接确有变化，后端会异步重抓并回填真实标题 / 图标 / 文章。
             支持裸 RSSHub 路由（如 /github/trending/daily），后端自动补实例前缀。 -->
        <div class="url-row">
          <input v-model="editUrl" type="text" placeholder="https://example.com/feed.xml" />
          <button class="copy-btn" @click="copyUrl">复制</button>
        </div>
        <p class="field-hint">支持 RSS 地址，或 RSSHub 路由（如 /github/trending/daily）</p>
      </div>
    </div>
    <div class="manage-foot">
      <!-- 删除：级联删除该源下全部文章，需二次确认。走全局按钮体系的危险变体 -->
      <button class="btn btn-danger" @click="handleDelete">删除</button>
      <div class="foot-right">
        <button class="btn" @click="emit('close')">取消</button>
        <!-- 主按钮必须同时挂基类 .btn（.btn-primary 只负责配色，尺寸/圆角来自基类，
             漏写会退化成浏览器原生按钮外观） -->
        <button class="btn btn-primary" :disabled="saving" @click="handleSave">
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
 * 单个订阅源的查看与维护：重命名、移动文件夹、修改链接（含复制）、删除（二次确认）。
 * 由 FeedPanel 以 `:feed` 传入当前管理的源，`feed` 为 null 即弹窗关闭。
 *
 * # 迁移自 FeedPanel 的手写 .modal-overlay 弹窗
 * 迁到 BaseModal 后补齐了 role="dialog" / 焦点陷阱 / Esc 关闭 / 滚动锁（遗留 #2 前半）。
 * 宽度从旧的一次性 360px 归入 BaseModal 的 sm 档（460px），与 RulesModal 同档对齐。
 *
 * # 回填时机
 * BaseModal 常驻挂载（关闭只是隐藏而非卸载），原「v-if 卸载后重建、openManage 手工回填」
 * 的时机不再成立，改为 watch(feed)：feed 变为非空时把名称 / 链接 / 文件夹拷进可编辑副本
 * 并清空旧错误提示；「取消」直接丢弃副本，不污染 store。
 */

import { ref, computed, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import AppSelect from './AppSelect.vue'
import { useFeedsStore } from '@/stores/feeds'
import { useModalStore } from '@/stores/modal'
import type { Feed } from '@/types'

const props = defineProps<{
  /** 当前管理的订阅源；null 表示弹窗关闭 */
  feed: Feed | null
}>()

const emit = defineEmits<{
  /** 用户按 Esc / 点遮罩 / 点 × / 取消 / 操作成功后请求关闭；由父组件把 feed 置空 */
  (e: 'close'): void
}>()

const feedsStore = useFeedsStore()
/** 全局弹窗 store 实例：用于替代原生 confirm/prompt/alert */
const modalStore = useModalStore()

/** 弹窗内可编辑的名称副本（与 store 解耦，「取消」直接丢弃） */
const editName = ref('')
/** 弹窗内可编辑的链接副本：保存时与原件比对，只有真的变了才让后端重抓 */
const editUrl = ref('')
/** 弹窗内可编辑的文件夹副本（folder_id），实现"移动到文件夹" */
const editFolderId = ref(0)
/** 保存中状态：禁用「保存」按钮避免重复提交 */
const saving = ref(false)
/** 保存失败的错误提示（内联展示，不弹窗；删除走 confirm） */
const manageMsg = ref('')

/**
 * 文件夹下拉选项
 *
 * 直接复用 store 的 folders 列表映射为 AppSelect 需要的 {value,label} 结构。
 */
const folderOptions = computed(() =>
  feedsStore.folders.map((f) => ({ value: f.id, label: f.name })),
)

// 打开时回填副本：feed 为 null（关闭态）时跳过，下次打开必然重新回填
watch(
  () => props.feed,
  (feed) => {
    if (!feed) return
    editName.value = feed.name
    editUrl.value = feed.url
    editFolderId.value = feed.folder_id || 0
    manageMsg.value = ''
  },
)

/**
 * 复制输入框中的订阅源链接到剪贴板
 *
 * 取 `editUrl` 而非原件：用户改了地址后想先粘到浏览器验证是否可用，复制到的应是新值。
 * 浏览器预览环境下 clipboard API 可能不可用，失败仅打印日志、不阻塞流程。
 */
async function copyUrl() {
  try {
    await navigator.clipboard.writeText(editUrl.value)
  } catch (e) {
    console.error('复制链接失败:', e)
  }
}

/**
 * 保存订阅源改动（名称 / 链接 / 文件夹）
 *
 * 名称留空时回退为原名称；链接留空则直接拦下并内联提示——"空链接"不是一种订阅。
 * 链接变更后由后端异步重抓该源，抓取失败经 `feed-updated` 的 error 字段回来，
 * 由 FeedPanel 的 watch 弹出提示（改错地址时唯一的可见反馈）。
 */
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
 * 删除订阅源（含其下全部文章）
 *
 * 删除是破坏性且级联（后端 ON DELETE CASCADE 一并删文章），先经全局弹窗二次确认，
 * 文案明确告知"不可恢复"。确认后委托 feedsStore.deleteFeed（会重载列表并清空可能的选中态），
 * 成功后关闭弹窗；失败把错误内联提示。
 */
async function handleDelete() {
  if (!props.feed) return
  const feed = props.feed
  const ok = await modalStore.showConfirm({
    title: '删除订阅源',
    message: `确定删除订阅源「${feed.name}」吗？\n该源下的所有文章也会一并删除，且不可恢复。`,
  })
  if (!ok) return
  try {
    await feedsStore.deleteFeed(feed.id)
    emit('close')
  } catch (e) {
    manageMsg.value = e instanceof Error ? e.message : String(e)
  }
}
</script>

<style scoped>
/* 头部 / 消息条 / 主体 / 底部的分段样式与旧手写弹窗逐字一致；
   面板外壳（遮罩 / 圆角 / 投影 / 宽度）由 BaseModal 提供，不再自绘。 */
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
.manage-msg {
  color: var(--primary);
  background: var(--primary-fg);
  font-size: var(--fs-xs);
  padding: var(--sp-2) var(--sp-4);
  border-bottom: 1px solid var(--border);
}
.manage-body {
  padding: var(--sp-4);
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}
.field { display: flex; flex-direction: column; gap: var(--sp-05); }
.field label { font-size: var(--fs-xs); color: var(--text-tertiary); }
.field input {
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
.field input:focus { box-shadow: 0 0 0 1px var(--primary); }
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
/* 字段说明：只在"链接该填什么"这类需要举例的字段下出现，压低存在感 */
.field-hint {
  margin: var(--sp-05) 0 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}
.manage-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-35) var(--sp-4);
  border-top: 1px solid var(--border);
}
.foot-right { display: flex; gap: var(--sp-2); }
/* 弹窗底部按钮（取消 / 保存 / 删除）样式统一由全局按钮体系提供
   （styles.css 的 .btn / .btn-primary / .btn-danger），此处不再复制副本。 */
</style>
