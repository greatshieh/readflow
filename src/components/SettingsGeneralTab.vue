<template>
  <div class="tab-pane">
    <!-- 主题：写入 settings.theme（light/dark/system），保存后立即切换 -->
    <div class="form-group">
      <label class="form-label">主题</label>
      <AppSelect v-model="theme" :options="themeOptions" />
      <p class="form-hint">保存后立即生效；"跟随系统"会随系统明暗切换自动变化</p>
    </div>

    <!-- 主题色：写入 settings.accent，切换强调色预设（详见 appearance.ts 的 ACCENT_PRESETS） -->
    <div class="form-group">
      <label class="form-label">主题色</label>
      <div class="accent-picker">
        <button
          v-for="preset in accentPresets"
          :key="preset.key"
          type="button"
          class="accent-swatch"
          :class="{ active: accent === preset.key }"
          :style="{ '--swatch': preset.color }"
          :title="preset.label"
          @click="setAccent(preset.key)"
        >
          <span class="accent-dot"></span>
          <span class="accent-name">{{ preset.label }}</span>
        </button>
      </div>
      <p class="form-hint">点击色块即时生效并写入设置；深色主题下会自动提亮一档保证对比度</p>
    </div>

    <!-- 界面字体大小：写入 settings.ui_font_size，以应用级 zoom 缩放整个 UI -->
    <div class="form-group">
      <label class="form-label">界面字体大小</label>
      <AppSelect v-model="uiFontSize" :options="uiFontSizeOptions" />
      <p class="form-hint">缩放整个应用界面（含侧边栏与列表），保存后立即生效</p>
    </div>

    <!-- 界面字体族：写入 settings.ui_font，作用于全局 UI（侧边栏/列表/按钮/弹窗） -->
    <div class="form-group">
      <label class="form-label">界面字体</label>
      <AppSelect v-model="uiFont" :options="uiFontOptions" />
      <p class="form-hint">
        作用于整个应用界面；列表同时列出本机已安装的字体，保存后立即生效
      </p>
    </div>

    <!-- 内容字体族：写入 settings.content_font，作用于正文/标题/摘要卡片 -->
    <div class="form-group">
      <label class="form-label">内容字体</label>
      <AppSelect v-model="contentFont" :options="contentFontOptions" />
      <p class="form-hint">
        仅作用于正文阅读区域；可选内置的衬线 / 无衬线，也可指定任意本机字体
      </p>
    </div>

    <!-- 阅读字号：写入 settings.font_size，并立即应用到阅读区（--reading-scale） -->
    <div class="form-group">
      <label class="form-label">阅读字体大小</label>
      <AppSelect v-model="fontSize" :options="fontSizeOptions" />
      <p class="form-hint">仅作用于正文阅读区域的字号，保存后立即生效</p>
    </div>

    <div class="divider"></div>

    <!-- RSSHub：实例地址 + 添加订阅时的路由前缀拼装与自动探测 -->
    <div class="form-group">
      <label class="form-label">RSSHub 实例地址</label>
      <input class="form-input" v-model="rsshubUrl" placeholder="https://rsshub.app" />
      <p class="form-hint">
        添加订阅时可直接输入 RSSHub 路由（如 /twitter/user/xxx），将自动拼接此实例地址。
        留空则默认使用 rsshub.app；若该实例不可用，会自动依次尝试其它公共实例，并把选中的实例写回这里
      </p>
    </div>

    <!-- 刷新频率：写入 settings.refresh_interval_minutes，调度器在下次启动时读取生效 -->
    <div class="form-group">
      <label class="form-label">刷新频率</label>
      <AppSelect v-model="refreshInterval" :options="refreshOptions" />
      <p class="form-hint">后台自动刷新订阅源的频率，保存后需重启应用生效</p>
    </div>

    <!-- 按来源自动分组：写入 settings.auto_group_by_source，缺省开启（"0" 为关）。
         开启时新增订阅（未指定分组）会自动归入同来源分组；
         下方按钮承接原侧栏「···」菜单里的手动整理入口，对存量源整理一次。 -->
    <div class="setting">
      <div class="setting-info">
        <h4>按来源自动分组</h4>
        <p>添加订阅时自动把同一来源（普通站点取域名、RSSHub 源取平台名）的源归入同名分组</p>
      </div>
      <AppSwitch v-model="autoGroup" label="按来源自动分组" />
    </div>
    <!-- 立即整理存量：一次性扫描全部订阅源，同来源的未分类源归组；可重复执行 -->
    <div class="opml-actions">
      <button class="btn btn-default" :disabled="grouping" @click="handleAutoGroup">
        {{ grouping ? '整理中…' : '立即整理存量订阅源' }}
      </button>
    </div>
    <p class="save-hint" v-if="autoGroupMsg">{{ autoGroupMsg }}</p>

    <!-- 系统通知：写入 settings.notify_enabled，缺省开启（详见 Rust 侧 notify.rs） -->
    <div class="setting">
      <div class="setting-info">
        <h4>系统通知</h4>
        <p>应用在后台时，刷新到新文章或自动化任务结束会发送系统通知；正在阅读时不会打扰</p>
      </div>
      <AppSwitch v-model="notifyEnabled" label="系统通知" />
    </div>

    <!-- 关闭到托盘：写入 settings.close_to_tray，缺省开启。
         开启时点窗口关闭按钮（含 Alt+F4 / 任务栏右键）只隐藏窗口，
         应用驻留托盘继续后台刷新；真退出走托盘菜单「退出」。 -->
    <div class="setting">
      <div class="setting-info">
        <h4>关闭到托盘</h4>
        <p>点击窗口关闭按钮时隐藏到系统托盘，应用继续在后台刷新；通过托盘菜单退出</p>
      </div>
      <AppSwitch v-model="closeToTray" label="关闭到托盘" />
    </div>

    <div class="divider"></div>

    <!-- 文章保留策略：写入 settings.article_retention_days 与 retention_keep_unread -->
    <div class="form-group">
      <label class="form-label">文章保留策略</label>
      <AppSelect v-model="retentionDays" :options="retentionOptions" />
      <p class="form-hint">超过保留天数的旧文章将自动清理（收藏文章始终保留）；后台每日凌晨 3 点执行</p>
    </div>
    <!-- 保留未读：开关型设置项，开启时未读文章即便超期也不清理 -->
    <div class="setting">
      <div class="setting-info">
        <h4>保留未读文章</h4>
        <p>开启后，超过保留天数但尚未阅读的旧文章不会被自动删除</p>
      </div>
      <AppSwitch v-model="keepUnread" label="保留未读文章" />
    </div>

    <!-- 立即清理：按当前策略手动触发一次，返回删除条数 -->
    <div class="opml-actions" style="margin-top: var(--sp-3)">
      <button class="btn btn-default" @click="handleCleanup">立即清理旧文章</button>
    </div>
    <p class="save-hint" v-if="cleanupMsg">{{ cleanupMsg }}</p>

    <p class="save-hint" v-if="savedGeneral">{{ savedGeneral }}</p>
  </div>
</template>

<script setup lang="ts">
/**
 * 设置 · 常规 tab
 *
 * # 职责
 * 主题 / 主题色 / 界面与内容字体 / 阅读字号 / RSSHub 实例 / 刷新频率 /
 * 系统通知 / 关闭到托盘 / 文章保留策略，以及"立即清理旧文章"的手动入口。
 *
 * # 与外部的边界
 * - 表单状态完全自持；「保存」按钮在模态底部（SettingsModal 的 `.m-foot`），
 *   本组件用 `defineExpose({ save })` 把 `saveGeneral` 交出去供其调用。
 * - 打开时回填自己的字段：监听 `props.open`，由隐藏变显示时从 settings store 读值。
 * - 「已保存 / 已清理」提示靠 `setTimeout` 到期复位，句柄集中登记、卸载时清空。
 */
import { ref, computed, watch, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '@/stores/settings'
import { useFeedsStore } from '@/stores/feeds'
import { useModalStore } from '@/stores/modal'
import { applyAppearance, ACCENT_PRESETS } from '@/utils/appearance'
import AppSelect, { type SelectOption } from './AppSelect.vue'
import AppSwitch from './AppSwitch.vue'
import { loadSystemFonts, type FontOption } from '@/utils/systemFonts'

/** 模态是否显示（由 SettingsModal 透传，用于"打开即回填"） */
const props = defineProps<{ open: boolean }>()

/** 设置 store 实例：读写后端 settings 表 */
const settingsStore = useSettingsStore()
/** 订阅源 store 实例：「立即整理存量订阅源」按钮需要调它的 autoGroupBySource */
const feedsStore = useFeedsStore()
/** 全局弹窗 store 实例 */
const modalStore = useModalStore()

/** 常规设置：刷新频率（分钟字符串）与阅读字体档位 */
const refreshInterval = ref('60')
const fontSize = ref('medium')
/** 主题：light / dark / system */
const theme = ref('light')
/** 界面字体大小档位（应用级 zoom 缩放） */
const uiFontSize = ref('medium')
/** 界面字体族：system（应用内置默认栈）/ 任意系统字体族名 */
const uiFont = ref('system')
/** 内容字体族：system（跟随界面字体）/ sans / serif / 任意系统字体族名 */
const contentFont = ref('system')
/**
 * 本机系统字体选项（懒加载）
 *
 * 由后端 `system_fonts_list` 枚举，供「界面字体」「内容字体」两个下拉共同使用。
 * 尚未加载完成时为空数组，两个下拉只显示内置预设，不阻塞设置页渲染。
 */
const systemFonts = ref<FontOption[]>([])

/**
 * 懒加载本机系统字体列表
 *
 * 打开设置页时触发一次；`utils/systemFonts.ts` 内有模块级缓存，
 * 重复调用不会重复扫描磁盘。失败时得到空数组，只影响下拉可选项数量，
 * 因此这里不做错误提示。
 *
 * @returns 无返回值；副作用为填充 systemFonts
 */
async function loadFonts() {
  if (systemFonts.value.length > 0) return
  systemFonts.value = await loadSystemFonts()
}
/**
 * 主题色（强调色）预设清单与当前选中值
 *
 * 清单直接取自 appearance.ts 导出的 ACCENT_PRESETS——同一份数据源保证
 * "可选项"与 "applyAppearance 认可的合法值"永不分叉。
 */
const accentPresets = ACCENT_PRESETS
const accent = ref(accentPresets[0].key)
/** RSSHub 实例地址（添加订阅时路由自动拼接此前缀） */
const rsshubUrl = ref('')
/** 文章保留策略：保留天数（字符串数字，0 = 永久保留）与是否保留未读 */
const retentionDays = ref('30')
const keepUnread = ref(true)
/** 系统通知开关：缺省开启，落库为 settings.notify_enabled（"1" / "0"） */
const notifyEnabled = ref(true)
/** 关闭到托盘开关：缺省开启，落库为 settings.close_to_tray（"1" / "0"） */
const closeToTray = ref(true)
/** 按来源自动分组开关：缺省开启，落库为 settings.auto_group_by_source（"0" 为关） */
const autoGroup = ref(true)
/** 存量整理是否正在执行（防重复点击 + 按钮忙碌文案） */
const grouping = ref(false)
/** 存量整理的结果 / 错误反馈（save-hint 展示，数秒后自动复位） */
const autoGroupMsg = ref('')
/** 立即清理操作的反馈信息 */
const cleanupMsg = ref('')
/** 常规设置保存反馈 */
const savedGeneral = ref('')

// ─── 下拉选项表（供自绘 AppSelect 使用） ─────────────────────────────────────
// 原先这些选项写在模板的 <option> 里；改用 AppSelect 后需要以数组形式传入。
// 取值必须与后端约定完全一致（如 refreshInterval 是"分钟数的字符串"），
// 因此这里的值全部保持原有字面量，不做类型"优化"，避免静默改变语义。

/** 主题选项 */
const themeOptions: SelectOption[] = [
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
  { value: 'system', label: '跟随系统' },
]
/** 界面字体大小档位选项（应用级 zoom） */
const uiFontSizeOptions: SelectOption[] = [
  { value: 'small', label: '小' },
  { value: 'medium', label: '标准' },
  { value: 'large', label: '大' },
  { value: 'xlarge', label: '特大' },
]
/** 正文阅读字体档位选项 */
const fontSizeOptions: SelectOption[] = [
  { value: 'small', label: '小' },
  { value: 'medium', label: '中' },
  { value: 'large', label: '大' },
  { value: 'xlarge', label: '特大' },
]

/**
 * 为选项列表补一条"当前值已不在列表中"的兜底项
 *
 * 用户可能选过某个系统字体后又把它卸载了 —— 此时选中值不在选项里，AppSelect 的
 * 触发器会显示为空、看起来像设置丢了。这里补一条标注「未安装」的选项，
 * 让当前值仍可见，也让用户一眼看出需要换一个。
 *
 * @param options - 基础选项列表
 * @param current - 当前设置值
 * @param presets - 内置预设值集合（属于预设即视为"存在"，无需兜底）
 * @returns 可能已前置兜底项的选项列表
 */
function withMissingOption(
  options: SelectOption[],
  current: string,
  presets: string[],
): SelectOption[] {
  if (!current || presets.includes(current)) return options
  if (options.some((option) => option.value === current)) return options
  return [{ value: current, label: `${current}（未安装）` }, ...options]
}

/**
 * 界面字体族选项：内置默认栈 + 本机系统字体
 *
 * 「系统默认」的判定与落地在 utils/appearance.ts —— 它对应"移除 --ui-font 内联变量"，
 * 即回落到 styles.css 中声明的 font-family 栈。
 */
const uiFontOptions = computed<SelectOption[]>(() =>
  withMissingOption(
    [{ value: 'system', label: '系统默认' }, ...systemFonts.value],
    uiFont.value,
    ['system'],
  ),
)

/** 内容字体族选项：三个预设 + 本机系统字体 */
const contentFontOptions = computed<SelectOption[]>(() =>
  withMissingOption(
    [
      { value: 'system', label: '跟随界面字体' },
      { value: 'sans', label: '无衬线（黑体）' },
      { value: 'serif', label: '衬线（宋体）' },
      ...systemFonts.value,
    ],
    contentFont.value,
    ['system', 'sans', 'serif'],
  ),
)
/** 自动刷新间隔选项（值为分钟数的字符串） */
const refreshOptions: SelectOption[] = [
  { value: '15', label: '每 15 分钟' },
  { value: '30', label: '每 30 分钟' },
  { value: '60', label: '每小时' },
  { value: '180', label: '每 3 小时' },
  { value: '360', label: '每 6 小时' },
  { value: '720', label: '每 12 小时' },
  { value: '1440', label: '每天' },
]
/** 文章保留天数选项（值为天数的字符串，'0' = 永久保留） */
const retentionOptions: SelectOption[] = [
  { value: '7', label: '保留 7 天' },
  { value: '30', label: '保留 30 天' },
  { value: '90', label: '保留 90 天' },
  { value: '180', label: '保留 180 天' },
  { value: '0', label: '永久保留' },
]

/**
 * 组件内在途定时器的句柄
 *
 * 「已保存 / 已清理」提示靠 `setTimeout` 到期复位；弹窗关闭即卸载，
 * 统一登记以便在卸载时一次性清空，避免定时器向已销毁的组件写状态。
 */
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

// 卸载（关闭弹窗）时清空全部在途定时器
onUnmounted(() => {
  timers.forEach(clearTimeout)
  timers.length = 0
})

/**
 * 打开模态时回填表单
 *
 * 监听 props.open：由隐藏变为显示时读取后端 settings 的当前值填入各字段，
 * 并重置保存提示，让用户看到的是"已保存值"而非空白。
 * `immediate: true` 是因为切 tab 会重新挂载本组件，此时 open 已是 true，
 * 不走 immediate 就会显示空白。
 */
watch(
  () => props.open,
  (open) => {
    if (!open) return
    // 常规设置：刷新频率（缺省 60 分钟）与字体档位（缺省 medium）
    refreshInterval.value = settingsStore.getSetting('refresh_interval_minutes') || '60'
    fontSize.value = settingsStore.getSetting('font_size') || 'medium'
    // 外观设置：主题 / 主题色 / 界面字体 / 内容字体
    theme.value = settingsStore.getSetting('theme') || 'light'
    accent.value = settingsStore.getSetting('accent') || accentPresets[0].key
    uiFontSize.value = settingsStore.getSetting('ui_font_size') || 'medium'
    uiFont.value = settingsStore.getSetting('ui_font') || 'system'
    contentFont.value = settingsStore.getSetting('content_font') || 'system'
    // 系统字体列表：懒加载一次（内部有缓存），不阻塞弹窗渲染与其它字段回填
    void loadFonts()
    // RSSHub 实例地址（缺省官方实例）
    rsshubUrl.value = settingsStore.getSetting('rsshub_base_url') || 'https://rsshub.app'
    // 文章保留策略：保留天数（缺省 30 天）与保留未读开关（缺省开启）
    retentionDays.value = settingsStore.getSetting('article_retention_days') || '30'
    keepUnread.value = settingsStore.getSetting('retention_keep_unread') !== '0'
    // 系统通知：缺省开启，只有显式存过 "0" 才算关闭（与 Rust 侧 notify.rs 判据一致）
    notifyEnabled.value = settingsStore.getSetting('notify_enabled') !== '0'
    // 关闭到托盘：缺省开启，只有显式存过 "0" 才算关闭（与 Rust 侧 win_close 判据一致）
    closeToTray.value = settingsStore.getSetting('close_to_tray') !== '0'
    // 按来源自动分组：缺省开启，只有显式存过 "0" 才算关闭（与 Rust 侧 feeds_add 判据一致）
    autoGroup.value = settingsStore.getSetting('auto_group_by_source') !== '0'
    savedGeneral.value = ''
  },
  { immediate: true }
)

/**
 * 选择主题色并即时预览
 *
 * 之所以直接落库而非等"保存"按钮：颜色是强视觉反馈的选择，
 * 点击后必须立刻看到全界面主色变化才有意义；
 * 这里写入 settings 表后调用 applyAppearance 刷新 html[data-accent]，
 * saveGeneral 也会再写一次同一 key（幂等，无副作用）。
 *
 * @param key - 强调色预设 key（必须存在于 ACCENT_PRESETS）
 * @returns 无返回值；失败仅打印日志（保留界面上的选中态，下次保存会重试）
 */
async function setAccent(key: string) {
  accent.value = key
  try {
    await settingsStore.setSetting('accent', key)
    applyAppearance(settingsStore)
  } catch (e) {
    console.error('保存主题色失败:', e)
  }
}

/**
 * 立即整理存量订阅源（按来源自动分组）
 *
 * 承接自 FeedPanel「···」菜单的同名动作：后端扫描全部订阅源，把指向同一来源
 * 且数量达到 2 个的**未分类**源归入同名分组；已手工归组的源不受影响，可重复执行。
 * 结果与错误都走 save-hint 内联提示（数秒后自动复位），不打断设置页的浏览。
 *
 * @returns 无返回值；副作用为后端可能新建分组并归入若干源，随后重载 folders 与 feeds
 */
async function handleAutoGroup() {
  if (grouping.value) return
  grouping.value = true
  autoGroupMsg.value = ''
  try {
    const result = await feedsStore.autoGroupBySource()
    if (result.created_folders === 0 && result.moved_feeds === 0) {
      autoGroupMsg.value = '没有可整理的分组：同一来源的未分类订阅源不足 2 个'
    } else {
      const names = result.group_names.length ? `（${result.group_names.join('、')}）` : ''
      autoGroupMsg.value = `新建 ${result.created_folders} 个分组，归入 ${result.moved_feeds} 个订阅源${names}`
    }
  } catch (e) {
    autoGroupMsg.value = `整理失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    grouping.value = false
    schedule(() => (autoGroupMsg.value = ''), 6000)
  }
}

/**
 * 保存常规设置
 *
 * 把主题 / 主题色 / 界面字体（大小与族）/ 内容字体 / RSSHub / 刷新频率 / 阅读字号
 * 全部写入后端 settings 表。
 * 外观类设置（主题、字体）保存后立即调用 applyAppearance 生效，无需重启；
 * 刷新频率在下次应用启动时由调度器读取生效。
 *
 * @returns 无返回值；成功后置 savedGeneral 提示
 */
async function saveGeneral() {
  try {
    await Promise.all([
      settingsStore.setSetting('refresh_interval_minutes', refreshInterval.value),
      settingsStore.setSetting('font_size', fontSize.value),
      settingsStore.setSetting('theme', theme.value),
      settingsStore.setSetting('accent', accent.value),
      settingsStore.setSetting('ui_font_size', uiFontSize.value),
      settingsStore.setSetting('ui_font', uiFont.value),
      settingsStore.setSetting('content_font', contentFont.value),
      // RSSHub 实例地址：去尾部斜杠归一化，避免拼路由时出现双斜杠
      settingsStore.setSetting('rsshub_base_url', rsshubUrl.value.trim().replace(/\/+$/, '')),
      settingsStore.setSetting('article_retention_days', retentionDays.value),
      settingsStore.setSetting('retention_keep_unread', keepUnread.value ? '1' : '0'),
      settingsStore.setSetting('notify_enabled', notifyEnabled.value ? '1' : '0'),
      settingsStore.setSetting('close_to_tray', closeToTray.value ? '1' : '0'),
      settingsStore.setSetting('auto_group_by_source', autoGroup.value ? '1' : '0'),
    ])
    // 主题与字体即时生效（映射集中在 utils/appearance.ts，与启动时同一入口）
    applyAppearance(settingsStore)
    savedGeneral.value = '已保存'
    schedule(() => (savedGeneral.value = ''), 2000)
  } catch (e) {
    console.error('保存常规设置失败:', e)
    await modalStore.showAlert({
      title: '保存失败',
      message: '常规设置保存失败，请重试',
    })
  }
}

/**
 * 立即按当前保留策略清理旧文章
 *
 * 把表单里的保留天数与保留未读开关交给后端 `db_cleanup` 执行一次清理，
 * 返回删除条数；删除后后端会重算各订阅源未读数，侧边栏计数器自动同步。
 *
 * @returns 无返回值；成功提示本次清理条数，失败提示原因
 */
async function handleCleanup() {
  cleanupMsg.value = ''
  try {
    const deleted = await invoke<number>('db_cleanup', {
      keepDays: Number(retentionDays.value) || 0,
      keepUnread: keepUnread.value,
    })
    cleanupMsg.value = deleted > 0 ? `已清理 ${deleted} 篇旧文章` : '没有符合清理条件的旧文章'
    schedule(() => (cleanupMsg.value = ''), 2500)
  } catch (err) {
    cleanupMsg.value = '清理失败：' + (err instanceof Error ? err.message : String(err))
  }
}

// 底部「保存」按钮由 SettingsModal 调用（表单状态在本组件，按钮在模态底部）
defineExpose({ save: saveGeneral })
</script>

<style scoped>
/* 表单原语（.form-group / .form-label / .form-input / .form-hint / .save-hint /
   .setting / .divider / .tab-pane / .opml-actions）已提升为全局类（styles.css）：
   它们被本弹窗的多个 tab 共用，而各 tab 拆分后是独立组件，scoped 样式跨不过组件边界。
   这里只保留「常规」tab 独有的主题色选择器与「未安装字体」兜底样式。 */

/* 主题色选择器：色块网格，选中项主色边框 + 浅色底 */
.accent-picker {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(96px, 1fr));
  gap: var(--sp-2);
  margin-top: var(--sp-025);
}
.accent-swatch {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-15);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: border-color var(--dur) var(--ease), background var(--dur) var(--ease),
    box-shadow var(--dur) var(--ease), transform var(--dur) var(--ease);
}
.accent-swatch:hover { border-color: var(--border); background: var(--fill); transform: translateY(-1px); box-shadow: var(--shadow-sm); }
.accent-swatch:active { transform: scale(0.98); }
.accent-swatch.active {
  border-color: var(--swatch);
  background: var(--surface);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--swatch) 22%, transparent);
  color: var(--text-primary);
}
.accent-dot {
  width: 16px; height: 16px; border-radius: 50%;
  background: var(--swatch); flex-shrink: 0;
  box-shadow: var(--ring-inset);
}
.accent-name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
