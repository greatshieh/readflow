<template>
  <div class="tab-pane">
    <!-- Vault 根目录：必填，文章将写到 {vault}/导出目录/ 下；未填时导出会提示去设置 -->
    <div class="form-group">
      <label class="form-label">Vault 路径</label>
      <input class="form-input" v-model="vaultPath" placeholder="例如 /Users/you/Documents/MyVault" />
      <p class="form-hint">Obsidian 仓库的根目录绝对路径；文章会导出到该目录下的「导出目录」中</p>
    </div>
    <!-- 导出目录：默认 ReadFlow，留空则用默认 -->
    <div class="form-group">
      <label class="form-label">导出目录</label>
      <input class="form-input" v-model="exportFolder" placeholder="ReadFlow" />
      <p class="form-hint">Vault 内的子文件夹名，留空默认为 ReadFlow</p>
    </div>
    <!-- 译文开关：开启且文章已有 AI 译文时，在原文之后追加「译文」节，二者同文件导出 -->
    <div class="setting">
      <div class="setting-info">
        <h4>同时导出译文</h4>
        <p>原文始终导出；开启且已有 AI 译文时，在同一篇笔记中追加「译文」节</p>
      </div>
      <AppSwitch v-model="useTranslation" label="同时导出译文" />
    </div>

    <!-- 保存提示 -->
    <p class="save-hint" v-if="savedObs">{{ savedObs }}</p>
  </div>
</template>

<script setup lang="ts">
/**
 * 设置 · Obsidian tab
 *
 * # 职责
 * 把文章导出为 Markdown 写入用户配置的 Vault 目录：Vault 路径 / 导出目录 / 译文开关。
 *
 * # 与外部的边界
 * 「保存」按钮在模态底部（SettingsModal 的 `.m-foot`），本组件通过
 * `defineExpose({ save })` 交出 `saveObsidian` 供其调用；表单状态完全自持。
 */
import { ref, watch, onUnmounted } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import { useModalStore } from '@/stores/modal'
import AppSwitch from './AppSwitch.vue'

/** 模态是否显示（由 SettingsModal 透传，用于"打开即回填"） */
const props = defineProps<{ open: boolean }>()

/** 设置 store 实例：读写后端 settings 表 */
const settingsStore = useSettingsStore()
/** 全局弹窗 store 实例 */
const modalStore = useModalStore()

/** Obsidian 导出配置：与 settings 表的 obsidian_* 键对应 */
const vaultPath = ref('')
const exportFolder = ref('')
/** 是否同时导出 AI 译文（默认关闭：仅导出原文，开启则在原文后追加译文节） */
const useTranslation = ref(false)
/** 保存反馈 */
const savedObs = ref('')

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
 * `immediate: true` 是因为切 tab 会重新挂载本组件，此时 open 已是 true，
 * 不走 immediate 就会显示空白。
 */
watch(
  () => props.open,
  (open) => {
    if (!open) return
    // Obsidian 配置：回填 Vault 路径 / 导出目录（目录可为空，用默认 ReadFlow）；
    // 译文开关默认关（'0' 或缺失都视为关，即仅导出原文）
    vaultPath.value = settingsStore.getSetting('obsidian_vault_path') || ''
    exportFolder.value = settingsStore.getSetting('obsidian_export_folder') || ''
    useTranslation.value = settingsStore.getSetting('obsidian_use_translation') === '1'
    savedObs.value = ''
  },
  { immediate: true }
)

/**
 * 保存 Obsidian 导出配置
 *
 * 将 Vault 路径、导出目录、译文开关写入后端 settings 表。
 * Vault 路径为空时也照常写入（空字符串），由导出命令在用户实际导出时再校验并提示去设置；
 * 不强制要求先填路径才能关掉设置面板。
 *
 * @returns 无返回值；成功后置 savedObs 提示，失败仅打印日志并弹窗
 */
async function saveObsidian() {
  try {
    const tasks: Promise<void>[] = [
      settingsStore.setSetting('obsidian_vault_path', vaultPath.value.trim()),
      // 导出目录留空时写入默认 ReadFlow，避免后端反复判断默认
      settingsStore.setSetting(
        'obsidian_export_folder',
        exportFolder.value.trim() || 'ReadFlow'
      ),
      settingsStore.setSetting('obsidian_use_translation', useTranslation.value ? '1' : '0'),
    ]
    await Promise.all(tasks)
    savedObs.value = '已保存'
    schedule(() => (savedObs.value = ''), 2000)
  } catch (e) {
    console.error('保存 Obsidian 设置失败:', e)
    await modalStore.showAlert({
      title: '保存失败',
      message: 'Obsidian 设置保存失败，请重试',
    })
  }
}

// 底部「保存」按钮由 SettingsModal 调用（表单状态在本组件，按钮在模态底部）
defineExpose({ save: saveObsidian })
</script>

<style scoped>
/* 本 tab 的表单排版全部复用全局原语（styles.css 的 .form-group / .form-input /
   .form-hint / .setting / .save-hint / .tab-pane），无需额外样式。 */
</style>
