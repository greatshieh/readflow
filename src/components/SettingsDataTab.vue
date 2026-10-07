<template>
  <div class="tab-pane">
    <!-- 数据统计：进入本标签页时由 loadStats 拉取一次，纯只读展示 -->
    <div class="stats-grid" v-if="stats">
      <div class="stat-cell">
        <div class="stat-num">{{ stats.feeds }}</div>
        <div class="stat-label">订阅源</div>
      </div>
      <div class="stat-cell">
        <div class="stat-num">{{ stats.articles }}</div>
        <div class="stat-label">文章总数</div>
      </div>
      <div class="stat-cell">
        <div class="stat-num">{{ stats.read }}</div>
        <div class="stat-label">已读</div>
      </div>
      <div class="stat-cell">
        <div class="stat-num">{{ stats.unread }}</div>
        <div class="stat-label">未读</div>
      </div>
      <div class="stat-cell">
        <div class="stat-num">{{ stats.bookmarks }}</div>
        <div class="stat-label">收藏</div>
      </div>
    </div>
    <p class="form-hint" v-else>正在加载统计…</p>

    <div class="divider"></div>

    <!-- 数据库备份 / 恢复：本地优先应用的关键能力，保证数据可随 .db 文件迁移 -->
    <div class="setting">
      <div class="setting-info">
        <h4>数据库备份</h4>
        <p>导出完整的 readflow.db（含订阅源、文章与设置），用于迁移或防丢</p>
      </div>
    </div>
    <div class="opml-actions">
      <button class="btn btn-default" @click="handleBackup">备份数据库</button>
      <button class="btn btn-default" @click="handleRestore">恢复备份</button>
    </div>
    <p class="save-hint" v-if="backupMsg">{{ backupMsg }}</p>

    <div class="divider"></div>

    <!-- 订阅源 OPML：批量导入 / 导出 -->
    <div class="setting">
      <div class="setting-info">
        <h4>订阅源备份</h4>
        <p>从 OPML 文件批量导入订阅源，或导出当前全部订阅源</p>
      </div>
    </div>
    <div class="opml-actions">
      <button class="btn btn-default" @click="handleImportOpml">导入 OPML</button>
      <button class="btn btn-default" @click="handleExportOpml">导出 OPML</button>
    </div>
    <p class="save-hint" v-if="opmlMsg">{{ opmlMsg }}</p>
  </div>
</template>

<script setup lang="ts">
/**
 * 设置 · 数据 tab
 *
 * # 职责
 * 只读的数据统计 + 数据库备份/恢复 + 订阅源 OPML 导入导出（集中到设置便于管理）。
 *
 * # 与外部的边界
 * 本 tab 没有「保存」按钮——所有操作都是即点即执行的命令，结果以行内提示反馈。
 * 挂载时自行拉取一次统计（原先由 SettingsModal 在切 tab 时触发，等价）。
 */
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useFeedsStore } from '@/stores/feeds'
import { useModalStore } from '@/stores/modal'

/** 订阅源 store 实例：OPML 导入 / 导出走其后端 command */
const feedsStore = useFeedsStore()
/** 全局弹窗 store 实例 */
const modalStore = useModalStore()

/** 数据统计快照 */
interface Stats {
  feeds: number
  articles: number
  read: number
  unread: number
  bookmarks: number
}
/** 统计数据（进入本标签页时拉取一次） */
const stats = ref<Stats | null>(null)
/** 备份/恢复操作的反馈信息 */
const backupMsg = ref('')
/** OPML 操作的反馈信息（导入成功数 / 失败原因） */
const opmlMsg = ref('')

/**
 * 拉取全库数据统计
 *
 * 调用后端 `db_stats` 一次性汇总订阅源数、文章/已读/未读/收藏分布，
 * 填充 `stats` 供本标签页展示。
 *
 * @returns 无返回值；失败仅打印日志（统计非关键功能，不应打断设置）
 */
async function loadStats() {
  try {
    stats.value = await invoke<Stats>('db_stats')
  } catch (e) {
    console.error('加载统计失败:', e)
  }
}

// 挂载即拉取一次统计（本组件只在切到「数据」tab 时挂载，加载时机与拆分前一致）
onMounted(loadStats)

/**
 * 数据库备份（导出 SQLite 文件）
 *
 * 保存路径由后端弹出系统「另存为」对话框让用户选择，选好后由 Rust 直接复制文件
 * （不再用 Base64 经 IPC 传输——库可能有数百 MB）。返回 `null` 表示用户取消。
 *
 * @returns 无返回值
 */
async function handleBackup() {
  try {
    const dest = await invoke<string | null>('db_backup')
    backupMsg.value = dest ? `已导出到 ${dest}` : '已取消备份'
  } catch (err) {
    backupMsg.value = '备份失败：' + (err instanceof Error ? err.message : String(err))
  }
}

/**
 * 恢复数据库备份（选文件、校验、覆盖全在后端）
 *
 * 覆盖整个数据库属破坏性操作，因此先弹一次确认；确认后由后端弹出系统「打开文件」
 * 对话框，用户取消时返回 `null`（不是错误）。后端在覆盖前会校验所选文件是否为
 * SQLite 库、拒绝"选中的就是当前库"，并把现有库留底为 `.before-restore`。
 *
 * 恢复成功后**应用会自动重启**：覆盖时连接池与后台调度器仍持有指向旧库的连接，
 * 不重启就可能把旧页缓存写回新文件。故先展示提示、稍候再触发重启。
 *
 * @returns 无返回值；结果通过 backupMsg 反馈
 */
async function handleRestore() {
  const ok = await modalStore.showConfirm({
    title: '恢复数据库',
    message:
      '将用所选备份文件覆盖当前数据库，订阅源、文章与设置都会被替换。' +
      '当前数据库会先留底为 readflow.db.before-restore，恢复完成后应用将自动重启。',
  })
  if (!ok) return

  try {
    const src = await invoke<string | null>('db_restore')
    if (!src) {
      backupMsg.value = '已取消恢复'
      return
    }
    backupMsg.value = `已从 ${src} 恢复，正在重启…`
    // 刻意用裸 setTimeout 而非组件级的 schedule()：该回调只触发重启、不写任何组件状态，
    // 且必须保证执行——若随组件卸载被清空，用户在这 1.2 秒内关掉设置面板，
    // 应用就不会重启，新库也就不会生效。
    setTimeout(() => {
      invoke('app_restart').catch((e) => console.error('重启应用失败:', e))
    }, 1200)
  } catch (err) {
    backupMsg.value = '恢复失败：' + (err instanceof Error ? err.message : String(err))
  }
}

/**
 * 导入 OPML 订阅源（选文件、读文件、解析入库全在后端）
 *
 * 后端弹出系统「打开文件」对话框，用户取消时返回 `null`（不是错误）；
 * 选中则由 Rust 读盘、解析并批量入库，前端不再碰文件。与「导出 OPML」完全对称。
 *
 * @returns 无返回值；结果通过 opmlMsg 反馈
 */
async function handleImportOpml() {
  try {
    const ids = await feedsStore.importOpml()
    opmlMsg.value = ids === null ? '已取消导入' : `成功导入 ${ids.length} 个订阅源`
  } catch (err) {
    opmlMsg.value = '导入失败：' + (err instanceof Error ? err.message : String(err))
  }
}

/**
 * 导出全部订阅源为 OPML 文件
 *
 * 保存路径由后端弹出系统「另存为」对话框让用户选择，选好后由 Rust 直接写盘，
 * 前端不再参与文件操作。返回 `null` 表示用户在对话框中取消，不算失败。
 *
 * @returns 无返回值
 */
async function handleExportOpml() {
  try {
    const dest = await feedsStore.exportOpml()
    opmlMsg.value = dest ? `已导出到 ${dest}` : '已取消导出'
  } catch (err) {
    opmlMsg.value = '导出失败：' + (err instanceof Error ? err.message : String(err))
  }
}
</script>

<style scoped>
/* 表单原语（.setting / .setting-info / .divider / .save-hint / .opml-actions /
   .tab-pane）已提升为全局类（styles.css，被多个 tab 共用）。这里只保留数据统计网格。 */

/* 数据统计网格 */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: var(--sp-2);
  margin-bottom: var(--sp-1);
}
.stat-cell {
  background: var(--fill);
  border-radius: var(--r-md);
  padding: var(--sp-3) var(--sp-2);
  text-align: center;
}
.stat-num { font-size: var(--fs-xl); font-weight: 600; color: var(--text-primary); }
.stat-label { font-size: var(--fs-xs); color: var(--text-tertiary); margin-top: var(--sp-025); }
</style>
