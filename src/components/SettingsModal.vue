<template>
  <!-- 设置模态层：open 为 true 时显示；遮罩点击、Esc 与焦点陷阱由 BaseModal 提供。
       本组件只是外壳：左列（标题 + 纵排 tab）+ 右列（内容滚动区 + 底部操作区），
       五个 tab 各自独立成组件。 -->
  <!-- layout="side"：固定高度弹窗（BaseModal 提供），内部排成「左 tab 列 + 右内容列」，
       切 tab 时弹窗高度不再跳变。 -->
  <BaseModal :open="open" title="设置" layout="side" @close="close">
    <!-- 左列：标题 + 纵排 tab。关闭钮由 BaseModal 的 side 布局统一放在面板右上角 -->
    <aside class="side-nav">
      <div class="side-head">
        <h3>设置</h3>
      </div>
      <!-- 纵排 tab 切换：常规 / AI 设置 / Obsidian / 自动化 / 数据 -->
      <nav class="side-tabs">
        <button class="side-tab" :class="{ active: tab === 'general' }" @click="tab = 'general'">常规</button>
        <button class="side-tab" :class="{ active: tab === 'ai' }" @click="tab = 'ai'">AI 设置</button>
        <button class="side-tab" :class="{ active: tab === 'obsidian' }" @click="tab = 'obsidian'">Obsidian</button>
        <button class="side-tab" :class="{ active: tab === 'automation' }" @click="tab = 'automation'">自动化</button>
        <button class="side-tab" :class="{ active: tab === 'data' }" @click="tab = 'data'">数据</button>
      </nav>
    </aside>

    <!-- 右列：tab 面板（滚动区）+ 底部操作区 -->
    <div class="side-main">
      <!-- 各 tab 面板：v-if 链保证同一时刻只挂载一个（与拆分前一致）。
           「打开即回填」由各面板自己监听 open 完成 —— 切 tab 即重新挂载，
           故面板内的回填监听带 immediate，不依赖父组件统一触发。
           `bindActiveTab` 登记当前挂载的实例，供底部保存按钮调用其 save()。 -->
      <div class="m-body">
        <SettingsGeneralTab v-if="tab === 'general'" :ref="bindActiveTab" :open="open" />
        <SettingsAiTab v-else-if="tab === 'ai'" :ref="bindActiveTab" :open="open" />
        <SettingsObsidianTab v-else-if="tab === 'obsidian'" :ref="bindActiveTab" :open="open" />
        <SettingsAutomationTab
          v-else-if="tab === 'automation'"
          :ref="bindActiveTab"
          @switch-tab="tab = $event"
        />
        <SettingsDataTab v-else :ref="bindActiveTab" />
      </div>

      <!-- 底部操作区：保存按钮只对「常规 / AI 设置 / Obsidian」三个 tab 出现
           （自动化与数据 tab 的改动都是即点即提交，无需保存）。
           表单状态收在各自的 tab 组件里，这里通过 activeTab 调其公开的 save()。 -->
      <div class="m-foot">
        <button class="btn btn-default" @click="close">取消</button>
        <button
          v-if="tab === 'general' || tab === 'obsidian'"
          class="btn btn-primary"
          @click="activeTab?.save?.()"
        >
          保存
        </button>
        <button
          v-else-if="tab === 'ai'"
          class="btn btn-primary"
          :disabled="activeTab?.saving"
          @click="activeTab?.save?.()"
        >
          {{ activeTab?.saving ? '保存中…' : '保存' }}
        </button>
      </div>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 设置模态框（外壳）
 *
 * # 职责
 * 只负责模态壳、tab 切换条与底部操作区；五个 tab 的实现分别在
 * `SettingsGeneralTab` / `SettingsAiTab` / `SettingsObsidianTab` /
 * `SettingsAutomationTab` / `SettingsDataTab`。所有写操作都通过各自的 store 落到
 * Rust 后端的 settings 表，符合"前端不直连、数据集中在后端"的架构规则。
 *
 * # 底部保存按钮为什么要绕一圈
 * 「保存」按钮必须在 `.m-foot`（模态底部的固定操作区），而 `.m-foot` 是滚动区
 * `.m-body` 的**兄弟**节点——按钮不能随 tab 内容一起被搬进面板，否则会跟着内容滚走。
 * 因此表单状态留在各 tab 组件内，由它们 `defineExpose` 出 `save()`（AI tab 另暴露
 * `saving` 驱动「保存中…」文案与禁用态），这里用模板 ref 拿到当前挂载的那个。
 * `v-if / v-else-if` 链保证同一时刻只挂一个，故同名 ref 始终指向当前 tab。
 *
 * # Props / Emits
 * - props.open：控制模态显示（v-model:open）
 * - emits.close：请求关闭模态（由父组件置 open=false）
 */

import { ref } from 'vue'
import BaseModal from './BaseModal.vue'
import SettingsGeneralTab from './SettingsGeneralTab.vue'
import SettingsAiTab from './SettingsAiTab.vue'
import SettingsObsidianTab from './SettingsObsidianTab.vue'
import SettingsAutomationTab from './SettingsAutomationTab.vue'
import SettingsDataTab from './SettingsDataTab.vue'

defineProps<{ open: boolean }>()
/** 关闭事件：父组件据此把 open 置为 false */
const emit = defineEmits<{ close: [] }>()

/** 当前激活的 tab：默认落在「常规」（主题 / 字体 / 刷新频率等外观与基础设置，最常访问） */
const tab = ref<'general' | 'ai' | 'obsidian' | 'automation' | 'data'>('general')

/**
 * 当前挂载 tab 暴露给外壳的接口
 *
 * `save` / `saving` 都是可选的：自动化与数据两个 tab 没有保存动作，不暴露任何东西。
 */
interface TabApi {
  /** 保存本 tab 的表单（由 tab 组件的 defineExpose 提供） */
  save?: () => void | Promise<void>
  /** 是否正在保存（驱动底部按钮的文案与禁用态） */
  saving?: boolean
}
/** 当前挂载的 tab 实例（卸载时会被清空） */
const activeTab = ref<TabApi | null>(null)

/**
 * 模板 ref 回调：登记当前挂载的 tab 实例
 *
 * 用函数 ref 而非字符串 ref，是因为五个 tab 暴露的接口并不相同，
 * 声明式 ref 会要求父组件为每个分支标注精确的实例类型。
 * 卸载时 Vue 会以 `null` 回调本函数，正好清空引用。
 *
 * @param el - 挂载的组件实例；组件卸载时为 null
 */
function bindActiveTab(el: unknown) {
  activeTab.value = el as TabApi | null
}

/**
 * 关闭模态
 *
 * @returns 无返回值；副作用为向父组件 emit close 事件
 */
function close() {
  emit('close')
}
</script>

<style scoped>
/* ===== 弹窗外壳（遮罩与面板由 BaseModal 的 layout="side" 提供，这里只负责双列排版） =====
   各 tab 面板的表单排版样式已随组件拆分出去；被多个 tab 共用的表单原语
   （.tab-pane / .form-* / .setting / .divider / .save-hint / .opml-actions）
   提升为全局类，见 styles.css 的「设置面板表单原语」一节。 */

/* 左列：标题 + 纵排 tab。宽度固定，内容列占满剩余空间 */
.side-nav {
  width: 148px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
}
.side-head {
  padding: var(--sp-4);
  display: flex; align-items: center;
}
.side-head h3 { font-size: var(--fs-base); font-weight: 500; margin: 0; }

/* 纵排 tab：左对齐文字 + 圆角悬浮态；激活态用主题色描一层浅底 */
.side-tabs {
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
  padding: 0 var(--sp-2) var(--sp-3);
  overflow-y: auto;
}
.side-tab {
  text-align: left;
  padding: var(--sp-15) var(--sp-3);
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  border: none;
  background: none;
  border-radius: var(--r-md);
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.side-tab:hover { background: var(--fill); color: var(--text-primary); }
.side-tab.active { background: var(--primary-fg); color: var(--primary); font-weight: 500; }

/* 右列：tab 面板（滚动区）+ 底部操作区（不随内容滚动） */
.side-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
/* 主体：占满右列剩余高度并内部滚动。
   `min-height: 0` 不可省：flex 子项默认 min-height:auto 会被内容撑破父容器。
   顶部预留出 BaseModal side 布局右上角关闭钮的高度（--side-close-reserve），
   否则首行内容（数据 tab 的统计卡等）会顶进按钮区被遮挡。 */
.m-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-top: var(--side-close-reserve, 48px);
}

/* 底部操作区：固定在右列底部（是滚动区 .m-body 的兄弟节点，不随内容滚动） */
.m-foot {
  padding: var(--sp-3) var(--sp-5);
  border-top: 1px solid var(--border);
  display: flex; justify-content: flex-end; gap: var(--sp-2);
}
/* 按钮统一由全局按钮体系提供（styles.css 的 .btn / .btn-default / .btn-primary），
   此处不再复制副本。 */
</style>
