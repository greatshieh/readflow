<template>
  <div class="tab-pane">
    <!-- 摘要 / 翻译总开关 -->
    <div class="setting">
      <div class="setting-info">
        <h4>AI 摘要</h4>
        <p>使用 AI 自动生成文章摘要</p>
      </div>
      <AppSwitch v-model="summaryEnabled" label="AI 摘要总开关" />
    </div>
    <div class="setting">
      <div class="setting-info">
        <h4>AI 翻译</h4>
        <p>一键翻译外文文章内容</p>
      </div>
      <AppSwitch v-model="translationEnabled" label="AI 翻译总开关" />
    </div>

    <div class="divider"></div>

    <!-- 默认模型 -->
    <div class="form-group">
      <label class="form-label">默认模型</label>
      <input class="form-input" v-model="model" placeholder="例如 agnes-2.5-flash" />
    </div>
    <!-- API Base URL：无需带 /chat/completions，后端自动补全 -->
    <div class="form-group">
      <label class="form-label">API Base URL</label>
      <input class="form-input" v-model="baseUrl" placeholder="https://apihub.agnes-ai.com/v1" />
      <p class="form-hint">无需包含 /chat/completions，系统会自动补全端点</p>
    </div>
    <!-- API Key：密码框，仅本地存储；不回填明文，改由右侧状态标记告知是否已配置 -->
    <div class="form-group">
      <label class="form-label form-label--split">
        <span>API Key</span>
        <span class="key-status" :class="{ 'is-set': hasStoredKey }">
          {{ hasStoredKey ? '已配置' : '未配置' }}
        </span>
      </label>
      <input
        class="form-input"
        type="password"
        v-model="apiKey"
        :placeholder="hasStoredKey ? '已保存（留空则不修改）' : 'sk-...'"
        autocomplete="off"
      />
      <p class="form-hint">密钥仅本地存储于 SQLite，不会上传任何服务器</p>
    </div>

    <!-- 保存提示 -->
    <p class="save-hint" v-if="saved">已保存</p>
  </div>
</template>

<script setup lang="ts">
/**
 * 设置 · AI 设置 tab
 *
 * # 职责
 * 摘要 / 翻译总开关、默认模型、API Base URL 与 API Key 的配置。
 *
 * # 与外部的边界
 * - 「保存」按钮在模态底部（SettingsModal 的 `.m-foot`），本组件通过
 *   `defineExpose({ save, saving })` 交出保存动作与"保存中"状态，
 *   由父组件决定按钮文案与禁用态。
 * - API Key 输入框**只写不读**：打开设置时刻意不回填明文，是否已配置看 `hasStoredKey`
 *   （读 store 缓存，而非输入框），避免把"设计上不回填"误判成"上次没保存成功"。
 */
import { ref, computed, watch, onUnmounted } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import { useModalStore } from '@/stores/modal'
import AppSwitch from './AppSwitch.vue'

/** 模态是否显示（由 SettingsModal 透传，用于"打开即回填"） */
const props = defineProps<{ open: boolean }>()

/** 设置 store 实例：读写后端 settings 表 */
const settingsStore = useSettingsStore()
/** 全局弹窗 store 实例 */
const modalStore = useModalStore()

/** 表单字段：与 agnes provider 的 settings 键一一对应 */
const model = ref('')
const baseUrl = ref('')
/**
 * AI Key 输入框（**只写不读**）
 *
 * 打开设置时刻意不回填明文（见 `props.open` 监听），因此本 ref 只承载"用户本次
 * 新输入的密钥"。想知道库里到底有没有密钥，看 [`hasStoredKey`]，不要看这里。
 */
const apiKey = ref('')
/**
 * 库中是否已存在 AI 密钥
 *
 * 用于在不回填明文的前提下给出"已保存"的可见指示：否则用户每次打开设置都看到
 * 空输入框，会把"设计上不回填"误判成"上次没保存成功"。
 *
 * 读的是 settings store 的缓存值而非输入框——保存成功后 store 会同步缓存，
 * 标记立即翻转；用户在输入框打字不会影响它。
 */
const hasStoredKey = computed(
  () => (settingsStore.getSetting('ai_agnes_key') ?? '').trim().length > 0
)
/** 功能开关：默认开启 */
const summaryEnabled = ref(true)
const translationEnabled = ref(true)

/** 保存中 / 已保存 状态（saving 同时驱动模态底部按钮的文案与禁用态） */
const saving = ref(false)
const saved = ref(false)

/** 组件内在途定时器的句柄（保存提示 2 秒后自动隐藏） */
const timers: ReturnType<typeof setTimeout>[] = []

/**
 * 登记一个延时回调并返回句柄
 *
 * @param fn - 到期执行的回调
 * @param delay - 延迟毫秒数
 * @returns 定时器句柄
 */
function schedule(fn: () => void, delay: number) {
  const id = setTimeout(fn, delay)
  timers.push(id)
  return id
}

// 卸载（关闭弹窗 / 切 tab）时清空全部在途定时器
onUnmounted(() => {
  timers.forEach(clearTimeout)
  timers.length = 0
})

/**
 * 打开模态时回填表单
 *
 * 监听 props.open：由隐藏变为显示时读取 agnes 的现有配置填入各字段，并重置保存提示。
 * `immediate: true` 是因为切 tab 会重新挂载本组件，此时 open 已是 true，
 * 不走 immediate 就会显示空白。
 */
watch(
  () => props.open,
  (open) => {
    if (!open) return
    model.value = settingsStore.getSetting('ai_agnes_model') || ''
    baseUrl.value = settingsStore.getSetting('ai_agnes_url') || ''
    // API Key 出于安全考虑，打开时不回填明文，仅保留为空让用户按需修改
    apiKey.value = ''
    summaryEnabled.value = settingsStore.getSetting('ai_summary_enabled') !== '0'
    translationEnabled.value = settingsStore.getSetting('ai_translation_enabled') !== '0'
    saved.value = false
  },
  { immediate: true }
)

/**
 * 保存 AI 设置
 *
 * 将四个字段（默认模型、Base URL、API Key、两个开关）通过 settingsStore 写入后端。
 * API Key 仅在用户确实填写时才写库（为空则不动旧值，避免误清空）。
 * 写库顺序无关，用 Promise.all 并发提交即可。
 *
 * @returns 无返回值；成功后置 saved 提示并清空 key 输入框，失败仅打印日志（轻量提示）
 */
async function saveAi() {
  saving.value = true
  saved.value = false
  try {
    const tasks: Promise<void>[] = [
      settingsStore.setSetting('ai_agnes_model', model.value.trim()),
      settingsStore.setSetting('ai_agnes_url', baseUrl.value.trim()),
      settingsStore.setSetting('ai_summary_enabled', summaryEnabled.value ? '1' : '0'),
      settingsStore.setSetting('ai_translation_enabled', translationEnabled.value ? '1' : '0'),
    ]
    // 仅当用户填写了 Key 才覆盖旧值，避免清空
    if (apiKey.value.trim()) {
      tasks.push(settingsStore.setSetting('ai_agnes_key', apiKey.value.trim()))
    }
    await Promise.all(tasks)
    saved.value = true
    // 明文不留在输入框：保存状态改由标签右侧的「已配置」标记体现。
    // 既避免旁人瞥屏，也不会让用户误以为"输入框空了 = 没保存"——指示器此刻已翻转。
    apiKey.value = ''
    // 2 秒后自动隐藏提示，避免长期停留
    schedule(() => (saved.value = false), 2000)
  } catch (e) {
    console.error('保存 AI 设置失败:', e)
    await modalStore.showAlert({
      title: '保存失败',
      message: 'AI 设置保存失败，请重试',
    })
  } finally {
    saving.value = false
  }
}

// 底部「保存」按钮由 SettingsModal 调用；saving 驱动其文案与禁用态
defineExpose({ save: saveAi, saving })
</script>

<style scoped>
/* 表单原语（.form-group / .form-label / .form-input / .form-hint / .save-hint /
   .setting / .divider / .tab-pane）已提升为全局类（styles.css，被多个 tab 共用）。
   这里只保留本 tab 独有的两处：左标签右状态的布局变体与密钥状态标记。 */

/* 标签的左右分栏变体：左侧标签文字、右侧状态标记（目前仅 API Key 使用） */
.form-label--split {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-1);
}
/* 密钥的「已配置 / 未配置」状态标记：弱化字重，已配置时用成功色提示 */
.key-status {
  font-size: var(--fs-xs);
  font-weight: 400;
  color: var(--text-tertiary);
}
.key-status.is-set { color: var(--success); }
</style>
