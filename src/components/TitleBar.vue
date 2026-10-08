<template>
  <!-- 标题栏根容器：fixed 定位于窗口顶部，高度由全局 CSS 变量 --tb-h 统一控制。
       该变量同时被 FeedPanel（padding-top）与 ArticleColumn 移动端的 article-list（inset）复用，
       因此改动标题栏高度必须同步调整 --tb-h，否则会出现顶部遮挡。 -->
  <div class="titlebar">
    <!-- 左侧：应用标识区（Logo + 名称），仅做品牌展示 -->
    <div class="titlebar-app">
      <AppLogo :size="24" />
      <span class="titlebar-name">ReadFlow</span>
    </div>
    <!-- 中部：当前视图名称展示区。
         设置 pointer-events:none 是为了让该区域不拦截鼠标事件，
         从而保留"拖动标题栏移动窗口"的原生窗口行为。 -->
    <div class="titlebar-center">
      <span class="titlebar-view">{{ currentView }}</span>
    </div>
    <!-- 右侧业务操作区：主题切换 / 刷新订阅源 / 打开设置 -->
    <div class="titlebar-actions">
      <button class="tb-btn" @click="toggleTheme" :title="isDark ? '切换浅色主题' : '切换深色主题'">
        <!-- 太阳（当前深色，点击切浅色）/ 月亮（当前浅色，点击切深色）图标 -->
        <svg v-if="isDark" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="4"></circle>
          <line x1="12" y1="2" x2="12" y2="4"></line>
          <line x1="12" y1="20" x2="12" y2="22"></line>
          <line x1="4.93" y1="4.93" x2="6.34" y2="6.34"></line>
          <line x1="17.66" y1="17.66" x2="19.07" y2="19.07"></line>
          <line x1="2" y1="12" x2="4" y2="12"></line>
          <line x1="20" y1="12" x2="22" y2="12"></line>
          <line x1="4.93" y1="19.07" x2="6.34" y2="17.66"></line>
          <line x1="17.66" y1="6.34" x2="19.07" y2="4.93"></line>
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"></path>
        </svg>
      </button>
      <button class="tb-btn" @click="refreshFeeds" title="刷新">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="23 4 23 10 17 10"></polyline>
          <polyline points="1 20 1 14 7 14"></polyline>
          <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
        </svg>
      </button>
      <button class="tb-btn" @click="openSettings" title="设置">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="3"></circle>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
        </svg>
      </button>
    </div>
    <!-- 系统窗口控制区：最小化 / 最大化 / 关闭（自绘按钮，非系统原生标题栏） -->
    <div class="win-controls">
      <button class="win-btn" @click="minimize" title="最小化">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="5" y1="12" x2="19" y2="12"></line>
        </svg>
      </button>
      <button class="win-btn" @click="maximize" title="最大化">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="5" y="5" width="14" height="14" rx="2" ry="2"></rect>
        </svg>
      </button>
      <button class="win-btn close" @click="closeApp" title="关闭">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 窗口标题栏组件
 *
 * # 职责
 * 自绘的窗口标题栏（应用已隐藏系统原生标题栏），承载两部分内容：
 * 1. 品牌标识与当前视图名称的展示；
 * 2. 刷新 / 设置等业务入口，以及最小化 / 最大化 / 关闭三个窗口控制按钮。
 *
 * # 在整体布局中的位置
 * 由 App.vue 渲染在 `.main` 容器内的最顶部，采用 `position: fixed` 覆盖窗口顶栏区域。
 * 高度依赖全局 CSS 变量 `--tb-h`，与 FeedPanel、ArticleColumn 移动端的 article-list 共享同一份避让值。
 *
 * # 窗口控制实现
 * 最小化 / 最大化 / 关闭通过 invoke 调用 Rust 后端命令（`win_minimize` /
 * `win_toggle_maximize` / `win_close`）实现，而非直接使用 Tauri 前端 API。
 * 好处：错误处理统一在后端，前端只负责调用和展示反馈。
 */

import { ref, computed, onMounted, onUnmounted } from 'vue'
import AppLogo from '@/components/AppLogo.vue'
import { useSettingsStore } from '@/stores/settings'
import { useFeedsStore } from '@/stores/feeds'
import { applyAppearance } from '@/utils/appearance'

/**
 * 订阅源 store 实例（标题栏中部的数据来源）
 *
 * `selectedFeed` 是订阅源选中态的全局唯一事实来源（FeedPanel 写入、
 * ArticleColumn 消费），标题栏只做只读订阅：点击订阅源 / 重命名 / 删除后
 * Pinia 响应式更新会自动反映到这里，无需任何事件通信。
 */
const feedsStore = useFeedsStore()

/**
 * 标题栏中部展示的当前视图名称
 *
 * 选中订阅源时显示该源名称；未选中（统一时间线视图）时兜底为「全部文章」，
 * 与文章列表栏此前的展示文案保持一致。
 */
const currentView = computed(() => feedsStore.selectedFeed?.name || '全部文章')

/**
 * 触发订阅源刷新
 *
 * 作为标题栏的全局刷新入口，委托 feedsStore.refreshFeeds()（内部
 * `invoke('feeds_refresh')`）统一拉取，与 FeedPanel 的刷新按钮共用同一条链路。
 *
 * @returns 无返回值（Promise 完成时 resolve）
 * @副作用 触发 store 刷新；失败仅打印日志
 */
async function refreshFeeds() {
  const { useFeedsStore } = await import('@/stores/feeds')
  try {
    await useFeedsStore().refreshFeeds()
  } catch (e) {
    console.error('刷新失败:', e)
  }
}

/**
 * 打开设置面板
 *
 * 通过 window 派发 `open-settings` 自定义事件，由 App.vue 监听并控制 SettingsModal 显隐。
 * 采用事件而非直接引用组件，是为了与 FeedPanel 的 `toggle-feed-panel` 事件保持同一套
 * 解耦通信风格（标题栏不持有模态组件实例）。
 *
 * @returns 无返回值
 * @副作用 触发 window 上的 `open-settings` 事件
 */
function openSettings() {
  window.dispatchEvent(new CustomEvent('open-settings'))
}

/** 设置 store 实例：主题值读取与写回（主题是持久化设置，不是临时 UI 态） */
const settingsStore = useSettingsStore()

/** 当前是否处于深色主题（由 html 的 data-theme 属性驱动按钮图标切换） */
const isDark = ref(document.documentElement.dataset.theme === 'dark')

/** 主题切换监听：data-theme 由 applyAppearance 写入，这里用 MutationObserver 跟踪其变化 */
let themeObserver: MutationObserver | null = null

onMounted(() => {
  themeObserver = new MutationObserver(() => {
    isDark.value = document.documentElement.dataset.theme === 'dark'
  })
  // 只观察 html 根节点自身的属性变化（data-theme 切换正是这种变化）
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
})

onUnmounted(() => {
  themeObserver?.disconnect()
  themeObserver = null
})

/**
 * 快捷切换浅色 / 深色主题
 *
 * 读取当前主题（system 时按 html 实际渲染态取反）写回 settings 表并立即应用；
 * 与「设置 → 常规 → 主题」共享同一持久化键（theme），两处状态始终一致。
 *
 * @returns 无返回值（Promise 在写库完成后 resolve）
 * @副作用 写 settings.theme 并触发 applyAppearance；失败仅打印日志
 */
async function toggleTheme() {
  try {
    const next = isDark.value ? 'light' : 'dark'
    await settingsStore.setSetting('theme', next)
    applyAppearance(settingsStore)
  } catch (e) {
    console.error('切换主题失败:', e)
  }
}

/**
 * 最小化窗口
 *
 * 调用后端命令 `win_minimize`。失败（如非 Tauri 环境）仅打印日志，不阻断 UI。
 *
 * @returns 无返回值
 */
async function minimize() {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('win_minimize')
  } catch (e) {
    console.error('最小化窗口失败:', e)
  }
}

/**
 * 最大化 / 还原窗口切换
 *
 * 调用后端命令 `win_toggle_maximize`，后端自动判断当前状态并切换。
 *
 * @returns 无返回值
 */
async function maximize() {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('win_toggle_maximize')
  } catch (e) {
    console.error('最大化窗口失败:', e)
  }
}

/**
 * 关闭应用窗口
 *
 * 调用后端命令 `win_close`。行为由后端「关闭到托盘」设置决定：开启时只隐藏
 * 窗口（应用驻留托盘继续后台刷新），关闭时真正退出。各 store 的写操作
 * （已读标记、收藏等）都是即时落库，因此关闭前无需额外等待持久化；
 * 若后续引入"未落盘的草稿"，应在此前先 flush。
 *
 * @returns 无返回值
 */
async function closeApp() {
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('win_close')
  } catch (e) {
    console.error('关闭窗口失败:', e)
  }
}
</script>

<style scoped>
.titlebar {
  position: fixed;
  top: 0; left: 0; right: 0;
  height: var(--tb-h);
  /* 主题化底色：--titlebar-bg 在浅色/深色两套变量集中分别取值（见 styles.css） */
  background: var(--titlebar-bg);
  /* 玻璃化：与两侧面板同一套玻璃令牌，底色改用 --glass-2（比面板略实，层级更高）。
     上下各一道内边缘——上方一道是玻璃对天光的高光，下方一道与内容区分界；
     两道都靠 --glass-* 令牌，暗色主题下自动换成低透明度白线。 */
  box-shadow:
    inset 0 1px 0 var(--glass-hi),
    inset 0 -1px 0 var(--glass-lo);
  backdrop-filter: blur(16px) saturate(1.5);
  -webkit-backdrop-filter: blur(16px) saturate(1.5);
  display: flex;
  align-items: center;
  padding: 0 var(--sp-3);
  gap: var(--sp-15);
  z-index: 500;
  user-select: none;
}

.titlebar-app {
  display: flex; align-items: center; gap: var(--sp-2);
  flex-shrink: 0;
}

.titlebar-name { font-size: var(--fs-md); font-weight: 500; color: var(--text-primary); }

.titlebar-center {
  flex: 1;
  display: flex; align-items: center; justify-content: center;
  pointer-events: none;
}

.titlebar-view { font-size: var(--fs-sm); color: var(--text-tertiary); }

.titlebar-actions { display: flex; gap: var(--sp-1); align-items: center; }

.tb-btn {
  width: 32px; height: 32px;
  border-radius: var(--r-md);
  display: flex; align-items: center; justify-content: center;
  cursor: pointer; color: var(--text-tertiary);
  transition: all 0.15s; border: none; background: none;
}

.tb-btn:hover { background: var(--fill); color: var(--text-primary); }
.tb-btn svg { width: 16px; height: 16px; }

.win-controls {
  display: flex; gap: var(--sp-2); align-items: center; margin-left: auto;
}

.win-btn {
  width: 24px; height: 24px;
  border-radius: 50%;
  display: flex; align-items: center; justify-content: center;
  cursor: pointer; color: var(--text-tertiary);
  transition: all 0.15s; border: none; background: none;
}

.win-btn:hover { background: var(--fill); }
.win-btn.close:hover { background: var(--win-close); color: var(--tone-fg); }
.win-btn svg { width: 12px; height: 12px; }
</style>
