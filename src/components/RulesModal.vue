<template>
  <!-- 过滤规则管理弹窗：列出全部规则并提供增删改。
       由「规则」入口打开，点击遮罩、× 或 Esc 关闭（Esc 与焦点陷阱由 BaseModal 提供）；
       所有写操作经 invoke 交给 Rust 后端（filters_* 命令），前端只做展示与表单收集。 -->
  <BaseModal :open="open" title="过滤规则" size="sm" @close="close">
    <div class="manage-head">
      <span>过滤规则</span>
      <button class="manage-close" @click="close" aria-label="关闭">×</button>
    </div>

    <!-- 列表与表单共用一段滚动区：头部固定，规则多时也不会把新增表单顶出视野 -->
    <div class="rules-scroll">
      <div class="manage-msg" v-if="msg">{{ msg }}</div>

      <!-- 规则列表：每条规则显示名称 / 条件 / 动作，并提供 启用开关 / 编辑 / 删除 -->
      <div class="rules-list">
        <div v-if="loading" class="hint">加载中…</div>
        <div v-else-if="rules.length === 0" class="hint">暂无规则，添加一条来自动处理新文章。</div>
        <div v-for="r in rules" :key="r.id" class="rule-row" :class="{ disabled: !r.enabled }">
          <!-- 启停开关：乐观更新 + 失败回滚由 toggleEnabled 处理 -->
          <AppSwitch
            :model-value="r.enabled"
            :label="`启用规则「${r.name}」`"
            @change="toggleEnabled(r)"
          />
          <div class="rule-info">
            <div class="rule-name">{{ r.name }}</div>
            <div class="rule-desc">
              {{ scopeText(r) }} · {{ fieldText(r) }} {{ opText(r) }} 「{{ r.value }}」 → {{ actionText(r) }}
            </div>
          </div>
          <div class="rule-ops">
            <!-- 行内小按钮走全局按钮体系的 mini 档；删除用「弱危险」变体
                 （默认中性、悬浮才透红），避免整列常显红字造成视觉噪音 -->
            <button class="btn-mini" @click="editRule(r)">编辑</button>
            <button class="btn-mini btn-danger-text" @click="removeRule(r)">删除</button>
          </div>
        </div>
      </div>

      <!-- 编辑 / 新增表单 -->
      <div class="rule-form">
        <div class="form-title">{{ editing ? '编辑规则' : '新增规则' }}</div>
        <div class="fields">
          <div class="field">
            <label>名称</label>
            <input v-model="form.name" type="text" placeholder="规则名称" />
          </div>
          <div class="field">
            <label>作用范围</label>
            <AppSelect v-model="form.scope" :options="scopeOptions" @change="onScopeChange" />
          </div>
          <div class="field" v-if="form.scope === 'feed'">
            <label>订阅源</label>
            <AppSelect v-model="form.scope_id" :options="feedOptions" />
          </div>
          <div class="field" v-if="form.scope === 'folder'">
            <label>文件夹</label>
            <AppSelect v-model="form.scope_id" :options="folderOptions" />
          </div>
          <div class="field">
            <label>匹配字段</label>
            <AppSelect v-model="form.field" :options="fieldOptions" />
          </div>
          <div class="field">
            <label>匹配方式</label>
            <AppSelect v-model="form.op" :options="opOptions" />
          </div>
          <div class="field">
            <label>匹配值</label>
            <input v-model="form.value" type="text" placeholder="关键字" />
          </div>
          <div class="field">
            <label>动作</label>
            <AppSelect v-model="form.action" :options="actionOptions" />
          </div>
          <!-- 打标签动作的参数：输入标签名，不存在时后端会自动创建；
               同时也接受从已有标签里选（datalist 提供补全），避免同名标签被写成两个 -->
          <div class="field" v-if="form.action === 'tag'">
            <label>标签名</label>
            <input
              v-model="form.action_arg"
              type="text"
              list="rule-tag-options"
              placeholder="标签名（不存在会自动创建）"
            />
            <datalist id="rule-tag-options">
              <option v-for="t in tagsStore.tags" :key="t.id" :value="t.name"></option>
            </datalist>
          </div>
          <div class="field">
            <label>优先级</label>
            <input v-model.number="form.priority" type="number" />
          </div>
        </div>
        <div class="form-ops">
          <button class="btn" v-if="editing" @click="cancelEdit">取消</button>
          <!-- 主按钮必须同时挂基类 .btn：.btn-primary 只负责配色，
               尺寸/圆角/内边距都来自基类，漏写会退化成浏览器原生按钮外观 -->
          <button class="btn btn-primary" :disabled="saving" @click="save">{{ saving ? '保存中…' : (editing ? '更新' : '添加') }}</button>
        </div>
      </div>
    </div>
  </BaseModal>

</template>

<script setup lang="ts">
/**
 * 过滤规则管理弹窗组件
 *
 * # 职责
 * 提供规则引擎的可视化管理：列出、新增、编辑、删除、启停过滤规则。
 * 所有持久化经 invoke 交给 Rust 端 filters_* 命令，前端不直连数据库。
 *
 * # 设计要点
 * - 作用范围支持 全部 / 指定订阅源 / 指定文件夹；指定范围时由下拉框选取 scope_id；
 * - 命中动作支持 自动已读 / 收藏 / 隐藏（隐藏即标记已读，使其不出现在未读列表）/ 打标签；
 * - 「打标签」的标签名存 `action_arg`：标签不存在时后端自动创建；标签被删除后，
 *   后端会清空引用它的规则参数，界面上显示为「打标签（未指定）」提示用户处理；
 * - 规则在文章入库时（rss::store_items）被求值，命中即施加动作，无需手动触发。
 *   注意：规则只作用于**新入库**的文章，不会回溯已存在的历史文章。
 */

import { ref, reactive, computed, onMounted, watch } from 'vue'
import { useFeedsStore } from '@/stores/feeds'
import { useTagsStore } from '@/stores/tags'
import { useModalStore } from '@/stores/modal'
import AppSelect from './AppSelect.vue'
import AppSwitch from './AppSwitch.vue'
import BaseModal from './BaseModal.vue'
import type { FilterRule } from '@/types'

/** 弹窗是否打开（由父组件 v-model 控制，这里用 open prop + close 事件） */
const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const feedsStore = useFeedsStore()
/** 全局弹窗 store 实例 */
const modalStore = useModalStore()
/**
 * 标签 store 实例
 *
 * 仅用于「打标签」动作的标签名补全：规则本身不持有 tag_id，只存名字
 *（这样规则可以引用尚未创建的标签，且不会因为标签被删而失效）。
 */
const tagsStore = useTagsStore()

/**
 * 作用范围下拉选项（全部 / 指定订阅源 / 指定文件夹）
 *
 * 选中后由 form.scope 驱动下方 scope_id 选择器（feed / folder）的显隐。
 */
const scopeOptions = [
  { value: 'all', label: '全部订阅源' },
  { value: 'feed', label: '指定订阅源' },
  { value: 'folder', label: '指定文件夹' },
]
/** 匹配字段下拉选项 */
const fieldOptions = [
  { value: 'title', label: '标题' },
  { value: 'content', label: '正文' },
  { value: 'author', label: '作者' },
  { value: 'feed', label: '来源名' },
]
/** 匹配方式下拉选项 */
const opOptions = [
  { value: 'contains', label: '包含' },
  { value: 'not_contains', label: '不包含' },
  { value: 'equals', label: '等于' },
  { value: 'not_equals', label: '不等于' },
]
/** 动作下拉选项 */
const actionOptions = [
  { value: 'mark_read', label: '自动标为已读' },
  { value: 'star', label: '收藏' },
  { value: 'hide', label: '隐藏（标为已读）' },
  { value: 'tag', label: '打标签' },
]
/** 订阅源下拉选项（动态，随 store 的 feeds 列表变化） */
const feedOptions = computed(() =>
  feedsStore.feeds.map((f) => ({ value: f.id, label: f.name })),
)
/** 文件夹下拉选项（动态，随 store 的 folders 列表变化） */
const folderOptions = computed(() =>
  feedsStore.folders.map((f) => ({ value: f.id, label: f.name })),
)

/** 规则列表 */
const rules = ref<FilterRule[]>([])
/** 加载中标志 */
const loading = ref(false)
/** 保存中标志 */
const saving = ref(false)
/** 操作反馈信息 */
const msg = ref('')
/** 当前编辑中的规则（null 表示新增） */
const editing = ref<FilterRule | null>(null)

/**
 * 新增 / 编辑表单的本地副本
 *
 * 与组件解耦：输入先落在副本上，只有点「添加 / 更新」才写回后端。
 */
const form = reactive({
  name: '',
  scope: 'all',
  scope_id: 0,
  field: 'title',
  op: 'contains',
  value: '',
  action: 'mark_read',
  action_arg: '',
  priority: 0
})

/** 打开弹窗时加载规则与文件夹（文件夹供 scope=folder 下拉选择） */
onMounted(() => {
  if (props.open) {
    feedsStore.loadFolders()
    // 标签目录供「打标签」动作的名称补全
    tagsStore.loadTags()
    loadRules()
  }
})
watch(
  () => props.open,
  (v) => {
    if (v) {
      feedsStore.loadFolders()
      tagsStore.loadTags()
      loadRules()
    }
  }
)

/**
 * 加载全部规则
 *
 * @returns 无返回值；结果写入 `rules`，`loading` 指示状态
 */
async function loadRules() {
  loading.value = true
  msg.value = ''
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    rules.value = await invoke<Array<FilterRule>>('filters_list')
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

/** 切换规则启用状态（乐观更新开关，失败回滚） */
async function toggleEnabled(r: FilterRule) {
  const next = !r.enabled
  r.enabled = next
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke<void>('filters_update', {
      id: r.id,
      name: r.name,
      scope: r.scope,
      scopeId: r.scope_id,
      field: r.field,
      op: r.op,
      value: r.value,
      action: r.action,
      actionArg: r.action_arg,
      enabled: next,
      priority: r.priority
    })
  } catch (e) {
    r.enabled = !next
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

/** 进入编辑态：把规则拷入表单副本 */
function editRule(r: FilterRule) {
  editing.value = r
  form.name = r.name
  form.scope = r.scope
  form.scope_id = r.scope_id
  form.field = r.field
  form.op = r.op
  form.value = r.value
  form.action = r.action
  form.action_arg = r.action_arg
  form.priority = r.priority
}

/** 取消编辑：清空编辑态与表单 */
function cancelEdit() {
  editing.value = null
  resetForm()
}

/** 删除规则（二次确认） */
async function removeRule(r: FilterRule) {
  const ok = await modalStore.showConfirm({
    title: '删除规则',
    message: `确定删除规则「${r.name}」吗？`,
  })
  if (!ok) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke<void>('filters_delete', { id: r.id })
    rules.value = rules.value.filter((x) => x.id !== r.id)
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

/** 保存（新增或更新） */
async function save() {
  if (!form.name.trim()) {
    msg.value = '请填写规则名称'
    return
  }
  // 「打标签」必须给出标签名：参数为空时后端会跳过该规则，等于保存了一条永不生效的规则
  if (form.action === 'tag' && !form.action_arg.trim()) {
    msg.value = '请填写要打的标签名'
    return
  }
  saving.value = true
  msg.value = ''
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const payload = {
      name: form.name.trim(),
      scope: form.scope,
      scopeId: form.scope === 'all' ? 0 : form.scope_id,
      field: form.field,
      op: form.op,
      value: form.value,
      action: form.action,
      // 非打标签动作一律提交空串，避免旧草稿里的标签名被写进库
      actionArg: form.action === 'tag' ? form.action_arg.trim() : '',
      enabled: true,
      priority: form.priority
    }
    if (editing.value) {
      await invoke<void>('filters_update', { id: editing.value.id, ...payload })
    } else {
      await invoke<number>('filters_create', payload)
    }
    await loadRules()
    editing.value = null
    resetForm()
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e)
  } finally {
    saving.value = false
  }
}

/** 作用范围变更时重置 scope_id（避免残留无效值） */
function onScopeChange() {
  form.scope_id = 0
}

/** 复位表单副本 */
function resetForm() {
  form.name = ''
  form.scope = 'all'
  form.scope_id = 0
  form.field = 'title'
  form.op = 'contains'
  form.value = ''
  form.action = 'mark_read'
  form.action_arg = ''
  form.priority = 0
}

/** 关闭弹窗 */
function close() {
  emit('close')
}

// ─── 文案辅助 ───────────────────────────────────────────────
function scopeText(r: FilterRule): string {
  if (r.scope === 'feed') {
    const f = feedsStore.feeds.find((x) => x.id === r.scope_id)
    return `源「${f ? f.name : r.scope_id}」`
  }
  if (r.scope === 'folder') {
    const f = feedsStore.folders.find((x) => x.id === r.scope_id)
    return `文件夹「${f ? f.name : r.scope_id}」`
  }
  return '全部'
}
function fieldText(r: FilterRule): string {
  return { title: '标题', content: '正文', author: '作者', feed: '来源' }[r.field] || r.field
}
function opText(r: FilterRule): string {
  return { contains: '包含', not_contains: '不包含', equals: '等于', not_equals: '不等于' }[r.op] || r.op
}
function actionText(r: FilterRule): string {
  if (r.action === 'tag') {
    // 参数为空说明标签已被删除（后端会在删标签时清空引用它的规则参数）——
    // 显式写成"未指定"而不是留一个空引号，让用户知道这条规则需要处理
    return `打标签「${r.action_arg || '未指定'}」`
  }
  return { mark_read: '标为已读', star: '收藏', hide: '隐藏' }[r.action] || r.action
}
</script>

<style scoped>
/* 列表 + 表单的滚动区
   头部（.manage-head）挂在滚动区之外保持固定，这里只负责"占满剩余高度并在内部滚动"。
   `min-height: 0` 是关键：flex 子项默认 `min-height: auto` 会被内容撑破父容器，
   导致滚动条落到整个面板外层、头部跟着一起滚走。 */
.rules-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}
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
.hint { padding: var(--sp-4); color: var(--text-tertiary); font-size: var(--fs-sm); }

.rules-list { padding: var(--sp-2) var(--sp-4); }
.rule-row {
  display: flex;
  align-items: center;
  gap: var(--sp-15);
  padding: var(--sp-15) 0;
  border-bottom: 1px solid var(--border);
}
.rule-row.disabled { opacity: 0.5; }
.rule-info { flex: 1; min-width: 0; }
.rule-name { font-size: var(--fs-sm); font-weight: 500; color: var(--text-primary); }
.rule-desc {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  margin-top: var(--sp-025);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.rule-ops { display: flex; gap: var(--sp-05); flex-shrink: 0; }
/* 行内按钮（编辑 / 删除）样式统一由全局按钮体系的 .btn-mini / .btn-danger-text 提供，
   此处不再重复声明，防止组件副本与全局分叉。 */

.rule-form {
  padding: var(--sp-3) var(--sp-4) var(--sp-4);
  border-top: 1px solid var(--border);
}
.form-title { font-size: var(--fs-sm); font-weight: 600; color: var(--text-primary); margin-bottom: var(--sp-15); }
.fields { display: grid; grid-template-columns: 1fr 1fr; gap: var(--sp-15); }
.field { display: flex; flex-direction: column; gap: var(--sp-1); }
.field label { font-size: var(--fs-xs); color: var(--text-tertiary); }
.field input {
  height: 34px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-2);
  font-size: var(--fs-sm);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  font-family: inherit;
  width: 100%;
}
.field input:focus { box-shadow: 0 0 0 3px var(--primary-soft); }
/* 下拉统一用 AppSelect 组件（自绘箭头 / focus 光环 / 浮层列表），
   其触发器高度为 34px，与上方的输入框等高，无需额外覆盖尺寸。 */
.form-ops { display: flex; justify-content: flex-end; gap: var(--sp-2); margin-top: var(--sp-3); }
/* 底部按钮样式统一由全局按钮体系（styles.css 的 .btn 族）提供，
   此处不再重复声明基类与 .btn-primary，避免出现"组件副本与全局分叉"。
   本弹窗的输入框为 34px，底部按钮保持全局默认 32px，形成轻微的主次差异。 */
</style>
