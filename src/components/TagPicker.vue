<template>
  <!-- 标签弹层（正文工具条下方浮出）
       两种模式：
       - 选择：勾选即把当前文章的标签整组覆盖为该集合（后端 article_tags_set 为覆盖语义）
       - 管理：重命名 / 改色 / 删除标签本体，属低频操作，收在第二个页签里
       @click.stop 阻止冒泡：父级的正文区点击会切到列表抽屉，浮层内点击不该触发它。 -->
  <div class="tag-picker" @click.stop>
    <div class="tp-head">
      <div class="tp-tabs">
        <button class="tp-tab" :class="{ active: mode === 'pick' }" @click="mode = 'pick'">选择</button>
        <button class="tp-tab" :class="{ active: mode === 'manage' }" @click="mode = 'manage'">管理</button>
      </div>
      <button class="tp-close" @click="emit('close')" aria-label="关闭">×</button>
    </div>

    <!-- 选择模式：点击整行切换该标签在当前文章上的有无 -->
    <div class="tp-body" v-if="mode === 'pick'">
      <p class="tp-empty" v-if="tagsStore.tags.length === 0">还没有标签，请在下方新建一个</p>
      <button
        v-for="t in tagsStore.tags"
        :key="t.id"
        class="tp-row"
        :class="{ on: selectedIds.has(t.id) }"
        @click="toggle(t.id)"
      >
        <span class="tp-check">
          <svg v-if="selectedIds.has(t.id)" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="20 6 9 17 4 12"></polyline>
          </svg>
        </span>
        <span class="tag-chip" :style="{ '--tag-c': tagColorVar(t.color) }">{{ t.name }}</span>
        <span class="tp-count">{{ t.article_count }}</span>
      </button>
    </div>

    <!-- 管理模式：色点循环换色 / 名称行内编辑（失焦或回车提交）/ 删除 -->
    <div class="tp-body" v-else>
      <p class="tp-empty" v-if="tagsStore.tags.length === 0">还没有标签</p>
      <div class="tp-row tp-manage" v-for="t in tagsStore.tags" :key="t.id">
        <button
          class="tp-swatch"
          :style="{ background: tagColorVar(t.color) }"
          @click="cycleColor(t)"
          title="切换颜色"
          aria-label="切换颜色"
        ></button>
        <input
          class="tp-name-input"
          :value="draftName(t)"
          @input="onDraftInput(t.id, $event)"
          @blur="commitRename(t)"
          @keydown.enter="commitRename(t)"
        />
        <button class="tp-del" @click="removeTag(t)" title="删除标签">×</button>
      </div>
    </div>

    <!-- 新建标签：名称 + 色板。两种模式下都保留，是最常用入口 -->
    <div class="tp-new">
      <input
        class="tp-new-input"
        v-model="newName"
        placeholder="新建标签…"
        maxlength="24"
        @keydown.enter="createTag"
      />
      <div class="tp-swatches">
        <button
          v-for="c in TAG_COLORS"
          :key="c"
          class="tp-swatch"
          :class="{ on: newColor === c }"
          :style="{ background: `var(--tone-${c})` }"
          @click="newColor = c"
          :aria-label="c"
        ></button>
      </div>
      <button class="btn btn-primary btn-sm" :disabled="!newName.trim()" @click="createTag">新建</button>
    </div>

    <!-- 操作反馈：新建/改名/删除的结果提示（成功为中性提示，失败为错误色） -->
    <p class="tp-msg" :class="{ error: msgError }" v-if="msg">{{ msg }}</p>
  </div>
</template>

<script setup lang="ts">
/**
 * 标签选择 / 管理弹层
 *
 * # 职责
 * 正文阅读区工具条「标签」按钮展开的面板，承载与单篇文章标签相关的全部交互：
 * 勾选当前文章的标签（选择页签）、维护标签目录（管理页签）、新建标签。
 *
 * # 数据来源与写路径
 * - 目录与文章标签均从 store 读（`useTagsStore` / `useArticlesStore`），本组件不缓存任何副本；
 * - 勾选提交走 `tagsStore.setArticleTags`（整组覆盖），命名/删除走 `tagsStore.updateTag` / `deleteTag`；
 * - 组件自身只保存"正在编辑的草稿名"这类纯 UI 状态。
 *
 * # 为什么不做成 BaseModal
 * 它是**锚定在工具条按钮下方的浮层**（popover），不是占据屏幕中央的模态框；
 * 但浮层内部不含需要焦点陷阱的复杂结构，Esc 关闭与点击外部关闭由父组件（ContentColumn）处理。
 */

import { ref, reactive, computed } from 'vue'
import { useTagsStore, TAG_COLORS, tagColorVar } from '@/stores/tags'
import { useArticlesStore } from '@/stores/articles'
import { useModalStore } from '@/stores/modal'
import type { Tag } from '@/types'

const emit = defineEmits<{ (e: 'close'): void }>()

const tagsStore = useTagsStore()
const articlesStore = useArticlesStore()
const modalStore = useModalStore()

/** 当前页签：选择（打标）/ 管理（目录维护） */
const mode = ref<'pick' | 'manage'>('pick')
/** 新建标签的输入名 */
const newName = ref('')
/** 新建标签选中的颜色 */
const newColor = ref<string>(TAG_COLORS[0])
/** 操作反馈文案与是否为错误色 */
const msg = ref('')
const msgError = ref(false)

/**
 * 当前文章已选中的标签 ID 集合
 *
 * 由 `selectedArticle.tags` 派生：勾选后由 store 回写文章对象，这里自然随之更新，
 * 组件无需再维护一份"本地选中集"，避免两份状态不一致。
 */
const selectedIds = computed<Set<number>>(
  () => new Set((articlesStore.selectedArticle?.tags ?? []).map((t) => t.id)),
)

/**
 * 切换某标签在当前文章上的有无
 *
 * 每次都提交"最终集合"而非单个增删：后端 `article_tags_set` 是整组覆盖，
 * 只改一个标签时也把完整集合发过去，语义单一、不会出现增量与全量两套路径读写出分歧。
 *
 * @param tagId - 被点击的标签 ID
 * @returns 无返回值；失败时写入 `msg` 提示
 */
async function toggle(tagId: number) {
  const article = articlesStore.selectedArticle
  if (!article) return
  const next = new Set(selectedIds.value)
  if (next.has(tagId)) next.delete(tagId)
  else next.add(tagId)
  try {
    await tagsStore.setArticleTags(article.id, [...next])
    msgError.value = false
    msg.value = ''
  } catch (e) {
    msgError.value = true
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

/** 新建标签并立即打到当前文章上（"建了就是要用"的最短路径） */
async function createTag() {
  const name = newName.value.trim()
  if (!name) return
  try {
    const id = await tagsStore.createTag(name, newColor.value)
    newName.value = ''
    // 直接勾上刚建的标签：用户在此处建标签的意图几乎总是给当前文章打标
    const article = articlesStore.selectedArticle
    if (article && !selectedIds.value.has(id)) {
      await tagsStore.setArticleTags(article.id, [...selectedIds.value, id])
    }
    msgError.value = false
    msg.value = ''
  } catch (e) {
    msgError.value = true
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

// ─── 管理页签 ────────────────────────────────────────────────────────────────

/**
 * 名称编辑草稿
 *
 * 用"输入即改草稿、失焦/回车才提交"的方式而非每次按键都写库：
 * 否则每敲一个字都会触发一次 UPDATE + 全量重载目录，输入框会被重载打断。
 * 按 id 存放，输入框只读 `draftName(t)` 的值。
 */
const drafts = reactive<Record<number, string>>({})

/**
 * 取某标签的编辑草稿（无草稿时回落到库里的名称）
 *
 * @param t - 标签
 * @returns 输入框应显示的名称
 */
function draftName(t: Tag): string {
  return drafts[t.id] ?? t.name
}

/**
 * 记录输入草稿
 *
 * @param id - 标签 ID
 * @param ev - 输入事件（从中取新的名称文本）
 */
function onDraftInput(id: number, ev: Event) {
  drafts[id] = (ev.target as HTMLInputElement).value
}

/**
 * 提交重命名（失焦 / 回车触发）
 *
 * 名称没变则不请求后端：失焦是高频动作，无谓的写库会让目录频繁重载。
 *
 * @param t - 被编辑的标签
 * @returns 无返回值；失败时把草稿丢弃并提示，避免输入框停留在一个未落库的名字上
 */
async function commitRename(t: Tag) {
  const next = (drafts[t.id] ?? t.name).trim()
  if (next === t.name || next === '') {
    delete drafts[t.id]
    return
  }
  try {
    await tagsStore.updateTag(t.id, next, t.color)
    delete drafts[t.id]
    msgError.value = false
    msg.value = ''
  } catch (e) {
    delete drafts[t.id]
    msgError.value = true
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

/**
 * 循环切换颜色
 *
 * 用"点一下换下一色"而不是展开色板：管理页签每行已有三个控件（色点/输入框/删除），
 * 再塞一排色板会让行高与横向空间都失控；7 色循环最坏点 7 次即回到原色，成本可接受。
 *
 * @param t - 标签
 */
async function cycleColor(t: Tag) {
  const idx = TAG_COLORS.indexOf(t.color as (typeof TAG_COLORS)[number])
  const next = TAG_COLORS[(idx + 1) % TAG_COLORS.length]
  try {
    await tagsStore.updateTag(t.id, t.name, next)
  } catch (e) {
    msgError.value = true
    msg.value = e instanceof Error ? e.message : String(e)
  }
}

/**
 * 删除标签（二次确认）
 *
 * 删除会一并移除所有文章上的该标签关联，属不可逆操作，必须确认。
 *
 * @param t - 待删除的标签
 */
async function removeTag(t: Tag) {
  const ok = await modalStore.showConfirm({
    title: '删除标签',
    message: `确定删除标签「${t.name}」吗？\n它会从所有文章上移除，且不可恢复。`,
  })
  if (!ok) return
  try {
    await tagsStore.deleteTag(t.id)
    delete drafts[t.id]
    msgError.value = false
    msg.value = ''
  } catch (e) {
    msgError.value = true
    msg.value = e instanceof Error ? e.message : String(e)
  }
}
</script>

<style scoped>
/* 浮层容器：定宽 + 最大高度内滚动，避免标签很多时把工具条以下全部遮住 */
.tag-picker {
  width: 260px;
  max-height: 380px;
  display: flex;
  flex-direction: column;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-pop);
  overflow: hidden;
}
.tp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--sp-1) var(--sp-15);
  border-bottom: 1px solid var(--border);
}
.tp-tabs { display: flex; gap: var(--sp-025); }
.tp-tab {
  border: none;
  background: none;
  padding: var(--sp-05) var(--sp-1);
  border-radius: var(--r-sm);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  cursor: pointer;
}
.tp-tab:hover { color: var(--text-primary); }
.tp-tab.active { background: var(--primary-fg); color: var(--primary); font-weight: 600; }
.tp-close {
  border: none;
  background: none;
  color: var(--text-tertiary);
  font-size: 15px;
  line-height: 1;
  cursor: pointer;
  padding: 0 var(--sp-05);
}
.tp-close:hover { color: var(--text-primary); }

/* 主体：flex:1 + min-height:0 才能让"最大高度"内的滚动条落在正确容器上 */
.tp-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-1);
}
.tp-empty {
  margin: 0;
  padding: var(--sp-3) var(--sp-1);
  text-align: center;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.tp-row {
  display: flex;
  align-items: center;
  gap: var(--sp-15);
  width: 100%;
  padding: var(--sp-1);
  border: none;
  background: none;
  border-radius: var(--r-sm);
  cursor: pointer;
  text-align: left;
}
.tp-row:hover { background: var(--fill); }
/* 勾选框：未选中也保留占位，避免勾选时整行文字左右抖动 */
.tp-check {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  border: 1px solid var(--border);
  border-radius: 3px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--primary);
  background: var(--surface);
}
.tp-check svg { width: 10px; height: 10px; }
.tp-row.on .tp-check { border-color: var(--primary); background: var(--primary-fg); }
.tp-count {
  margin-left: auto;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
}

/* 管理页签的行不是按钮（内含输入框与两个按钮），故覆盖 .tp-row 的交互态 */
.tp-manage { cursor: default; }
.tp-manage:hover { background: none; }
.tp-name-input {
  flex: 1;
  min-width: 0;
  border: 1px solid transparent;
  background: none;
  border-radius: var(--r-sm);
  padding: 2px var(--sp-05);
  font-size: var(--fs-sm);
  color: var(--text-primary);
}
.tp-name-input:hover { border-color: var(--border); }
.tp-name-input:focus { outline: none; border-color: var(--primary); background: var(--surface); }
.tp-del {
  border: none;
  background: none;
  color: var(--text-tertiary);
  font-size: 15px;
  line-height: 1;
  cursor: pointer;
  padding: 0 var(--sp-05);
  flex-shrink: 0;
}
.tp-del:hover { color: var(--danger); }

/* 新建区：始终钉在浮层底部，不随列表滚动 */
.tp-new {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  padding: var(--sp-1) var(--sp-15);
  border-top: 1px solid var(--border);
  background: var(--surface-muted);
}
.tp-new-input {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  padding: var(--sp-05) var(--sp-1);
  font-size: var(--fs-xs);
  background: var(--surface);
  color: var(--text-primary);
}
.tp-new-input:focus { outline: none; border-color: var(--primary); }
.tp-swatches { display: flex; gap: 3px; flex-shrink: 0; }
.tp-swatch {
  width: 12px;
  height: 12px;
  padding: 0;
  border-radius: var(--r-pill);
  border: 1px solid transparent;
  cursor: pointer;
}
/* 选中色点用内描边环而非外扩阴影：不改变布局尺寸，7 个点挤在一行时不会互相顶开 */
.tp-swatch.on { box-shadow: var(--ring-inset); border-color: var(--text-primary); }

.tp-msg {
  margin: 0;
  padding: var(--sp-05) var(--sp-15) var(--sp-1);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}
.tp-msg.error { color: var(--danger); }
</style>
