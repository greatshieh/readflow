<template>
  <!-- 来源分歧徽标：回答"这条判断是不是只有一家在说"。
       这是整条投研链路里唯一一处会主动质疑"证据数量"的地方——
       五条证据可能只是同一句话说了五遍。 -->
  <div v-if="d" class="div" :class="{ warn: onlyOneSource }">
    <span v-if="!hasAny" class="div-line muted">还没有证据，谈不上互证</span>
    <template v-else>
      <span class="div-line">
        支持 <b>{{ d.supportSources }}</b> 家来源
        <template v-if="d.opposeSources > 0"> · 反对 <b>{{ d.opposeSources }}</b> 家</template>
      </span>
      <!-- 只有一家在说的时候必须点出来：这时候"证据很多"是假象 -->
      <span v-if="onlyOneSource" class="div-hint">支持侧只有 1 家来源——条数再多也只是同一句话说了几遍</span>
      <span v-else-if="d.supportSources >= 3" class="div-hint ok">多家独立来源互证</span>
      <!-- 没有来源记录的老证据单独说明：把它们算成"1 家"会让上面那句警示在最该出现时消失 -->
      <span v-if="d.unrecorded > 0" class="div-hint muted" :title="unrecordedTip">
        另有 {{ d.unrecorded }} 条证据没有来源记录
      </span>
      <span v-if="d.sources.length > 0" class="div-src" :title="d.sources.join('、')">
        {{ d.sources.slice(0, 3).join(' · ') }}<template v-if="d.sources.length > 3"> 等 {{ d.sources.length }} 家</template>
      </span>
    </template>
  </div>
</template>

<script setup lang="ts">
/**
 * 来源分歧徽标
 *
 * # 职责
 * 把「支持侧 / 反对侧各来自几家不同来源」摆成一行，并在只有一家时给出警示。
 *
 * # 为什么按"来源名去重"而不是按证据条数
 * 分歧度的全部意义就在这个去重上：一条判断挂 5 条证据、全来自同一家媒体，
 * 那不是互证，是同一句话说了五遍。算成条数，这个功能就白做了
 * （口径在 Rust 侧 `research::search::evidence_divergence`，前端不做二次计算）。
 *
 * # 为什么不做成可点开的浮层
 * 这一行要回答的是一个是不是问题，不是一个需要展开探索的列表。
 * 来源名已经用 `title` 带了全量提示，鼠标停留即可看到；
 * 为一个"看一眼"的信息开一层浮层，成本远大于收益。
 */
import { computed, onMounted, watch } from 'vue'
import { useThesesStore } from '@/stores/theses'

const props = defineProps<{
  /** 论点 ID */
  thesisId: number
}>()

const store = useThesesStore()

/** 当前缓存里的分歧度（未加载到为 undefined） */
const d = computed(() => store.divergenceById[props.thesisId])

/** 是否已经有任何一条证据（含无来源的） */
const hasAny = computed(() => {
  const v = d.value
  if (!v) return false
  return v.supportSources + v.opposeSources + v.unrecorded > 0
})

/**
 * 是否"只有一家在说"
 *
 * 判定放在 items>0 的前提下：一条支持证据都没有时，"只有 1 家"这个警示不成立
 * （那是"还没有证据"，不是"只有一家"）。
 */
const onlyOneSource = computed(() => {
  const v = d.value
  return !!v && v.supportSources === 1 && v.opposeSources === 0
})

/** 无来源证据的悬停说明：解释为什么它们不算来源 */
const unrecordedTip = '这些证据挂得太早，当时还没有记录来源。文章被清理后，来源名已经找不回来了。'

/**
 * 拉一次分歧度
 *
 * 失败静默（store 里已记日志）：这是详情页顶部的一行辅助信息，
 * 取不到就不显示，不该打断阅读或弹错误。
 */
function load() {
  void store.loadDivergence(props.thesisId)
}

onMounted(load)
// 切到另一条判断时重取；同一条不重复请求（缓存直接命中模板）
watch(() => props.thesisId, load)
</script>

<style scoped>
.div {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3);
  margin-bottom: var(--sp-3);
  border-radius: var(--r-md);
  background: var(--fill);
  font-size: var(--fs-modal-meta);
  color: var(--text-secondary);
  line-height: var(--lh-modal-meta);
}
/* 只有一家在说 → 用警示色而不是红色：这不是错误，是"该找第二条来源了" */
.div.warn {
  background: color-mix(in srgb, var(--warning) 10%, transparent);
}
.div-line b {
  font-weight: 600;
  color: var(--text-primary);
}
.div.warn .div-line b {
  color: var(--warning-text);
}
.div-line.muted {
  color: var(--text-tertiary);
}
.div-hint {
  color: var(--warning-text);
}
.div-hint.ok {
  color: var(--success-text);
}
.div-hint.muted {
  color: var(--text-tertiary);
}
.div-src {
  /* 来源名可能很长：让它成为可收缩的那一段，前面对计数不会把它挤掉 */
  flex: 1 1 auto;
  min-width: 0;
  text-align: right;
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
