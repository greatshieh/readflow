<template>
  <!-- 快捷键一览：内容全部来自 utils/shortcuts.ts，本组件只负责排版。
       Esc / 遮罩点击 / 焦点陷阱 / 滚动锁由 BaseModal 统一提供。 -->
  <BaseModal :open="open" title="快捷键一览" size="md" @close="emit('close')">
    <div class="sc-head">
      <span class="m-title">快捷键一览</span>
      <button class="sc-close" @click="emit('close')" aria-label="关闭">×</button>
    </div>

    <div class="sc-body">
      <section v-for="g in groups" :key="g" class="sc-group">
        <h4 class="sc-gtitle">{{ g }}</h4>
        <div v-for="s in shortcutsOfGroup(g)" :key="s.display + s.label" class="sc-row">
          <kbd class="sc-kbd">{{ s.display }}</kbd>
          <span class="sc-label">{{ s.label }}</span>
        </div>
      </section>
    </div>

    <div class="sc-foot">
      <!-- Esc 与 Alt+← 都不在 SHORTCUTS 表内：前者必须在「弹窗已打开 / 焦点在输入框」
           这些守卫之前判定，后者是组合键（表按 KeyboardEvent.key 逐字比较，
           一旦进表，输入框里向左移光标会被 preventDefault 吞掉）。
           两者都走不了统一查表，故只在此处以文字补明 -->
      <span><kbd class="sc-kbd">esc</kbd> 退出专注阅读模式</span>
      <span><kbd class="sc-kbd">Alt + ←</kbd> 回到刚才读的报告（macOS：Cmd + [）</span>
      <span class="sc-note">列表未获焦点、输入框内输入时，快捷键一律不响应</span>
    </div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 快捷键一览弹窗
 *
 * # 职责
 * 把 `utils/shortcuts.ts` 里的键位元数据渲染成给用户看的一览表。
 *
 * # 为什么不自己维护一份键位表
 * 键位同时被 `App.vue`（决定拦哪个键、执行什么动作）和本组件（展示）消费。
 * 各抄一份必然漂移，故展示数据直接从同一份 `SHORTCUTS` 派生——改键位只改一处。
 *
 * # 为何还需要一个可见入口
 * `?` 键本身没人能猜到，必须有命令面板底栏之类的可达入口把它带出来。
 */

import BaseModal from './BaseModal.vue'
import { SHORTCUT_GROUPS, shortcutsOfGroup } from '@/utils/shortcuts'

defineProps<{
  /** 弹窗是否打开 */
  open: boolean
}>()

const emit = defineEmits<{
  /** 用户按 Esc / 点遮罩 / 点 × 请求关闭 */
  (e: 'close'): void
}>()

/** 分区标题顺序（订阅 → 文章 → 视图 → 应用） */
const groups = SHORTCUT_GROUPS
</script>

<style scoped>
.sc-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-3) var(--sp-4);
  border-bottom: 1px solid var(--glass-divider);
  /* 字号不再挂在容器上：原先标题 span 靠继承取容器的 14/500 —— 500 在中文里
     常常回落 400，于是它看起来和正文一模一样。 */
  flex: 0 0 auto;
}
.m-title {
  font-size: var(--fs-modal-title);
  font-weight: 600;
  line-height: var(--lh-modal-title);
  color: var(--text-primary);
}
.sc-close {
  border: none;
  background: none;
  /* 原为 var(--fs-lg)=16px：9 个关闭钮里唯一用 token 的一个，但仍是三档尺寸之一。
     统一成 18px 字形 + 32×32 命中区，负 margin 抵掉多占的空间。 */
  font-size: var(--fs-xl);
  line-height: 1;
  min-width: 32px;
  min-height: 32px;
  display: grid;
  place-items: center;
  margin: -8px -8px -8px 0;
  color: var(--text-tertiary);
  cursor: pointer;
}
.sc-close:hover { color: var(--text-primary); }

/* 内容区限高 + 独立滚动：19 条键位全展开会把弹窗顶出视口 */
.sc-body {
  max-height: 56vh;
  overflow-y: auto;
  padding: var(--sp-2) var(--sp-4) var(--sp-3);
}

.sc-group + .sc-group { margin-top: var(--sp-4); }

.sc-gtitle {
  margin: 0 0 var(--sp-1);
  /* 组名是区块标题，不是元信息：原先 11px/500 与键位说明同级，读不出分组 */
  font-size: var(--fs-modal-section);
  font-weight: 600;
  line-height: var(--lh-modal-section);
  color: var(--text-primary);
}

.sc-row {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: var(--sp-1) var(--sp-2);
  border-radius: var(--r-sm);
}
.sc-row {
  /* 这一行是 <div>，全局 button 基线管不到它 —— 之前 hover 是瞬时跳变。
     补上 T1 档过渡，与按钮的手感对齐。 */
  transition: background var(--m-tap) var(--m-ease-out);
}
.sc-row:hover { background: var(--hover-bg); }

.sc-kbd {
  flex-shrink: 0;
  min-width: 46px;
  text-align: center;
  padding: 2px var(--sp-2);
  border-radius: var(--r-sm);
  background: var(--fill);
  /* 与命令面板底栏的 kbd 保持同一套观感（内描边而非 border） */
  box-shadow: var(--ring-inset);
  /* 项目内没有 --font-mono 令牌，写 var() 会让整条声明静默作废，故继承 */
  font-family: inherit;
  /* 键帽里的键名属于元信息 */
  font-size: var(--fs-modal-meta);
  line-height: 1.6;
}

.sc-label {
  font-size: var(--fs-modal-label);
  line-height: var(--lh-modal-label);
  color: var(--text-secondary);
}

.sc-foot {
  display: flex;
  flex-direction: column;
  gap: var(--sp-05);
  padding: var(--sp-2) var(--sp-4) var(--sp-3);
  border-top: 1px solid var(--glass-divider);
  font-size: var(--fs-modal-meta);
  line-height: var(--lh-modal-meta);
  color: var(--text-secondary);
}
.sc-foot > span { display: flex; align-items: center; gap: var(--sp-2); }
.sc-note { color: var(--text-tertiary); }
</style>
