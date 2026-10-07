<template>
  <!-- 添加订阅弹窗：URL + 自定义标题 + 手动选择文件夹。
       由 FeedPanel 底部「添加订阅」按钮打开；Esc / 遮罩点击 / 焦点陷阱 /
       滚动锁由 BaseModal 统一提供。表单为编辑副本，取消直接丢弃。 -->
  <BaseModal :open="open" title="添加订阅源" size="sm" @close="emit('close')">
    <div class="add-head">
      <span>添加订阅源</span>
      <button class="add-close" @click="emit('close')" aria-label="关闭">×</button>
    </div>
    <!-- 操作反馈：后端失败（如 URL 重复）时内联提示，不弹全局弹窗 -->
    <div class="add-msg" v-if="addMsg">{{ addMsg }}</div>
    <div class="add-body">
      <div class="field">
        <label>订阅地址</label>
        <!-- data-autofocus：打开弹窗后焦点直接落在这里（BaseModal 的约定） -->
        <input
          v-model="url"
          type="text"
          data-autofocus
          placeholder="https://example.com/feed.xml"
          @keydown.enter="handleAdd"
        />
        <p class="field-hint">支持 RSS/Atom 地址，或 RSSHub 路由（如 /github/trending/daily，自动探测可用实例）</p>
      </div>
      <div class="field">
        <label>显示标题（可选）</label>
        <!-- 留空时后端先用域名兜底名落库，真实标题由后台抓取后经 feed-updated 回填 -->
        <input v-model="name" type="text" placeholder="留空则自动抓取源标题" />
      </div>
      <div class="field">
        <label>分组（可选）</label>
        <!-- 显式指定文件夹后，后端将跳过"按来源自动归组"，尊重手工分类 -->
        <AppSelect v-model="folderId" :options="folderOptions" />
      </div>
    </div>
    <div class="add-foot">
      <button class="btn" @click="emit('close')">取消</button>
      <!-- 主按钮必须同时挂基类 .btn（.btn-primary 只负责配色，尺寸/圆角来自基类） -->
      <button class="btn btn-primary" :disabled="saving" @click="handleAdd">
        {{ saving ? '检测实例中…' : '订阅' }}
      </button>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 添加订阅源弹窗
 *
 * # 职责
 * 收集订阅地址、可选的显示标题与归属分组，交给 `feedsStore.addFeed()`（内部
 * `invoke('feeds_add')`，成功后 store 自行重载列表与文件夹）。替代原先的
 * GlobalModal 单输入 prompt——那是"只填一个 URL"年代的产物，如今后端
 * `feeds_add` 本就接受 `name` 与 `folder_id`，缺的只是这块表单 UI。
 *
 * # 回填时机
 * BaseModal 常驻挂载（关闭只是隐藏而非卸载），故 watch(open)：每次打开都把
 * 三个字段清空——添加是新动作，不该残留上一次的输入。
 */

import { ref, computed, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import AppSelect from './AppSelect.vue'
import { useFeedsStore } from '@/stores/feeds'

const props = defineProps<{
  /** 弹窗是否打开 */
  open: boolean
}>()

const emit = defineEmits<{
  /** 用户按 Esc / 点遮罩 / 点 × / 取消 / 订阅成功后请求关闭 */
  (e: 'close'): void
}>()

const feedsStore = useFeedsStore()

/** 订阅地址（必填）：完整 URL 或 RSSHub 裸路由 */
const url = ref('')
/** 显示标题（可选）：留空交由后端抓取真实标题 */
const name = ref('')
/** 归属分组（folder_id），0 = 未分类 */
const folderId = ref(0)
/** 订阅中状态：裸路由要先探测 RSSHub 实例，可能等数秒，按钮必须给忙碌态 */
const saving = ref(false)
/** 订阅失败的错误提示（内联展示，不弹全局弹窗） */
const addMsg = ref('')

/**
 * 分组下拉选项
 *
 * 首项固定「未分类」（value 0），其后是 store 的 folders 列表；
 * 与 FeedManageModal 的 folderOptions 命名一致但多了兜底首项（管理场景
 * 里 feed 必有归属，添加场景里"不选"是合法默认）。
 */
const folderOptions = computed(() => [
  { value: 0, label: '未分类' },
  ...feedsStore.folders.map((f) => ({ value: f.id, label: f.name })),
])

// 打开时清空表单：添加是新动作，不残留上一次的输入与错误提示
watch(
  () => props.open,
  (open) => {
    if (!open) return
    url.value = ''
    name.value = ''
    folderId.value = 0
    addMsg.value = ''
  },
)

/**
 * 提交订阅
 *
 * 地址留空直接内联拦下——"空地址"不是一种订阅。成功后关闭弹窗（列表由
 * store 重载，新源立即出现在侧边栏，真实标题稍后经 feed-updated 回填）；
 * 失败把后端错误写进内联提示，弹窗保持打开让用户修改后重试。
 *
 * @returns 无返回值；成功时副作用为新增一条订阅并重载 feeds/folders
 */
async function handleAdd() {
  const trimmed = url.value.trim()
  if (!trimmed) {
    addMsg.value = '订阅地址不能为空'
    return
  }
  saving.value = true
  addMsg.value = ''
  try {
    await feedsStore.addFeed(trimmed, name.value.trim(), folderId.value)
    emit('close')
  } catch (e) {
    addMsg.value = e instanceof Error ? e.message : String(e)
  } finally {
    saving.value = false
  }
}
</script>

<style scoped>
/* 分段样式与 FeedManageModal 同构（头部 / 消息条 / 主体 / 底部）；
   面板外壳（遮罩 / 圆角 / 投影 / 宽度）由 BaseModal 提供，不再自绘。 */
.add-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-35) var(--sp-4);
  border-bottom: 1px solid var(--border);
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}
.add-close {
  border: none;
  background: none;
  cursor: pointer;
  font-size: 22px;
  line-height: 1;
  color: var(--text-tertiary);
  padding: 0 var(--sp-1);
}
.add-close:hover { color: var(--text-primary); }
.add-msg {
  color: var(--primary);
  background: var(--primary-fg);
  font-size: var(--fs-xs);
  padding: var(--sp-2) var(--sp-4);
  border-bottom: 1px solid var(--border);
}
.add-body {
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
/* 字段说明：只在"地址该填什么"这类需要举例的字段下出现 */
.field-hint {
  margin: var(--sp-05) 0 0;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}
.add-foot {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--sp-2);
  padding: var(--sp-35) var(--sp-4);
  border-top: 1px solid var(--border);
}
/* 底部按钮样式统一由全局按钮体系提供（styles.css 的 .btn / .btn-primary）。 */
</style>
