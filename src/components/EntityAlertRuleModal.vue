<template>
  <!-- ═══ 实体级告警规则编辑（P1-F）═══ -->
  <BaseModal
    :open="open"
    :title="`告警规则 · ${entity?.name ?? ''}`"
    size="lg"
    @close="emit('close')"
  >
    <div class="arm" v-if="entity">
      <!-- 启用总开关 -->
      <div class="arm-row arm-head">
        <div class="arm-info">
          <h4>启用告警</h4>
          <p>开启后，AI 抽取发现该实体命中下方条件的事件时，自动推送系统通知</p>
        </div>
        <AppSwitch v-model="enabled" label="启用告警" />
      </div>

      <div v-if="enabled" class="arm-body">
        <!-- 事件类型 -->
        <div class="arm-block">
          <label class="arm-label">事件类型（不选 = 不限类型）</label>
          <div class="arm-chips">
            <button
              v-for="opt in eventTypeOptions"
              :key="opt.value"
              class="arm-chip"
              :class="{ on: eventTypes.includes(opt.value) }"
              @click="toggleIn(eventTypes, opt.value)"
            >
              {{ opt.label }}
            </button>
          </div>
        </div>

        <!-- 情感 -->
        <div class="arm-block">
          <label class="arm-label">情感（不选 = 不限情感）</label>
          <div class="arm-chips">
            <button
              v-for="opt in sentimentOptions"
              :key="opt.value"
              class="arm-chip"
              :class="{ on: sentiments.includes(opt.value) }"
              @click="toggleIn(sentiments, opt.value)"
            >
              {{ opt.label }}
            </button>
          </div>
        </div>

        <!-- 关键词 -->
        <div class="arm-block">
          <label class="arm-label">关键词（逗号 / 空格分隔，命中事实即提醒；留空 = 不按关键词过滤）</label>
          <input
            v-model="keywordsText"
            type="text"
            class="arm-input"
            placeholder="如：收购、处罚、退市"
          />
        </div>
      </div>

      <!-- 反馈与保存 -->
      <p v-if="error" class="arm-error">{{ error }}</p>
      <div class="arm-actions">
        <span v-if="saving" class="arm-hint">保存中…</span>
        <button class="arm-cancel" @click="emit('close')">取消</button>
        <button class="arm-save" @click="handleSave" :disabled="saving">保存</button>
      </div>
    </div>
    <div v-else class="arm-empty">未选择实体</div>
  </BaseModal>
</template>

<script setup lang="ts">
/**
 * 实体级告警规则编辑弹窗（P1-F）
 *
 * 对应后端 `entities.alert_rules` 列的 JSON：
 * `{ enabled, event_types[], sentiments[], keywords[] }`。
 * 任一维度为空集合即表示"该维度不限制"。弹窗只负责读取 / 回写该 JSON，
 * 匹配与推送逻辑完全在 Rust 侧（notify::send_rule_alerts）完成，前端不持有匹配状态。
 *
 * # 与外壳/Store 的契约
 * - 父组件（ResearchEntitiesTab）通过 `:entity` 传入被编辑实体，`:open` 控制显隐；
 * - 打开时从 store 拉取当前规则并回填；保存时写回并经 store 落库；
 * - 不修改实体列表，规则文本由后端在抽取时读取，无需向上 emit 刷新。
 */
import { ref, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import AppSwitch from './AppSwitch.vue'
import { useEntitiesStore } from '@/stores/entities'
import { useEventTypesStore } from '@/stores/eventTypes'

const props = defineProps<{
  /** 弹窗显隐 */
  open: boolean
  /** 当前编辑的实体（仅需 id 与 name） */
  entity: { id: number; name: string } | null
}>()

const emit = defineEmits<{
  /** 关闭弹窗（取消 / 保存后均触发） */
  (e: 'close'): void
}>()

const entitiesStore = useEntitiesStore()
const eventTypesStore = useEventTypesStore()

// 打开弹窗时加载生效事件类型枚举（启用预设领域 + 自定义类型），供规则编辑 chips 使用
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void eventTypesStore.loadAll()
  },
  { immediate: true },
)

/** 事件类型可选项：来自生效枚举（启用预设领域 ∪ 自定义类型，与抽取侧同源） */
const eventTypeOptions = eventTypesStore.effectiveTypes

/** 情感可选项 */
const sentimentOptions = [
  { value: 'positive', label: '利好' },
  { value: 'negative', label: '利空' },
  { value: 'neutral', label: '中性' },
]

/** 是否启用该实体告警 */
const enabled = ref(false)
/** 选中的事件类型 */
const eventTypes = ref<string[]>([])
/** 选中的情感 */
const sentiments = ref<string[]>([])
/** 关键词输入（逗号 / 空格 / 顿号分隔） */
const keywordsText = ref('')
/** 保存中状态 */
const saving = ref(false)
/** 校验 / 失败反馈 */
const error = ref('')

/**
 * 在数组中切换某项的存在（用于事件类型 / 情感的多选）
 *
 * @param arr - 当前选中的字符串数组（直接 mutate）
 * @param value - 待切换的项
 */
function toggleIn(arr: string[], value: string) {
  const i = arr.indexOf(value)
  if (i >= 0) arr.splice(i, 1)
  else arr.push(value)
}

/**
 * 把关键词输入框文本拆成数组（逗号 / 空格 / 顿号 / 中文逗号）
 *
 * @returns 去空白后的非空关键词列表
 */
function parseKeywords(): string[] {
  return keywordsText.value
    .split(/[,，、\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

/**
 * 打开弹窗时回填当前规则：有规则则解析填入，无规则则回到默认空态
 */
watch(
  () => props.open,
  async (isOpen) => {
    error.value = ''
    if (!isOpen || !props.entity) return
    saving.value = false
    enabled.value = false
    eventTypes.value = []
    sentiments.value = []
    keywordsText.value = ''
    try {
      const json = await entitiesStore.getAlertRules(props.entity.id)
      if (!json) return
      const r = JSON.parse(json) as {
        enabled?: boolean
        event_types?: string[]
        sentiments?: string[]
        keywords?: string[]
      }
      enabled.value = r.enabled ?? false
      eventTypes.value = r.event_types ?? []
      sentiments.value = r.sentiments ?? []
      keywordsText.value = (r.keywords ?? []).join('、')
    } catch (e) {
      // 解析失败当作无规则，不阻断编辑
      console.error('解析实体告警规则失败:', e)
    }
  },
  { immediate: true }
)

/**
 * 保存规则：兜底校验后写回后端
 *
 * - 未启用：写空串，等价于清空规则；
 * - 已启用：至少选择一个条件（类型 / 情感 / 关键词），否则提示。
 *
 * @returns 无返回值；成功则关闭弹窗，失败写入 `error`
 */
async function handleSave() {
  if (!props.entity) return
  if (enabled.value) {
    if (eventTypes.value.length === 0 && sentiments.value.length === 0 && parseKeywords().length === 0) {
      error.value = '请至少选择一个条件（事件类型 / 情感 / 关键词）'
      return
    }
  }
  error.value = ''
  saving.value = true
  const payload = enabled.value
    ? {
        enabled: true,
        event_types: eventTypes.value,
        sentiments: sentiments.value,
        keywords: parseKeywords(),
      }
    : ''
  try {
    await entitiesStore.setAlertRules(
      props.entity.id,
      typeof payload === 'string' ? payload : JSON.stringify(payload)
    )
    emit('close')
  } catch (e) {
    error.value = `保存失败：${e instanceof Error ? e.message : String(e)}`
  } finally {
    saving.value = false
  }
}
</script>

<style scoped>
.arm {
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}
.arm-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-3);
}
.arm-head {
  padding-bottom: var(--sp-3);
  border-bottom: 1px solid var(--border);
}
.arm-info h4 {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 var(--sp-05);
}
.arm-info p {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  margin: 0;
  line-height: 1.5;
}
.arm-body {
  display: flex;
  flex-direction: column;
  gap: var(--sp-4);
}
.arm-block {
  display: flex;
  flex-direction: column;
  gap: var(--sp-15);
}
.arm-label {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}
.arm-chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-1);
}
.arm-chip {
  padding: 4px 12px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: all 0.15s;
}
.arm-chip:hover {
  border-color: var(--primary);
}
.arm-chip.on {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--primary-fg);
}
.arm-input {
  height: 34px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 0 var(--sp-15);
  font-size: var(--fs-md);
  background: var(--fill);
  color: var(--text-primary);
  outline: none;
  transition: border-color 0.15s;
}
.arm-input:focus {
  border-color: var(--primary);
}
.arm-input::placeholder {
  color: var(--text-tertiary);
}
.arm-error {
  font-size: var(--fs-sm);
  color: var(--danger);
  margin: 0;
}
.arm-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--sp-2);
}
.arm-hint {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  margin-right: auto;
}
.arm-cancel {
  height: 32px;
  padding: 0 var(--sp-35);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: all 0.15s;
}
.arm-cancel:hover {
  border-color: var(--text-tertiary);
}
.arm-save {
  height: 32px;
  padding: 0 var(--sp-35);
  border: none;
  border-radius: var(--r-md);
  background: var(--primary);
  color: var(--tone-fg);
  font-size: var(--fs-sm);
  cursor: pointer;
  transition: opacity 0.15s;
}
.arm-save:hover:not(:disabled) {
  opacity: 0.85;
}
.arm-save:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.arm-empty {
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  padding: var(--sp-3) 0;
}
</style>
