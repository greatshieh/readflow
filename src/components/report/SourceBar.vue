<template>
  <!-- 来源条：从某份报告的引用跳到文章后，正文栏顶部常驻一条 32px 的返回入口。
       它解决的问题是"跳出去就回不来"——滚动位置本身从未丢失（弹窗无 v-if、
       三个页签 v-show 常驻），缺的从头到尾只是一条入口。 -->
  <div class="source-bar" role="region" aria-label="报告来源">
    <button class="sb-main" type="button" @click="goBack">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <line x1="19" y1="12" x2="5" y2="12" />
        <polyline points="12 19 5 12 12 5" />
      </svg>
      <span class="sb-text">
        {{ ctx.reportTitle || '回到报告' }}
        <span v-if="ctx.refNo" class="sb-ref">· 引用 {{ ctx.refNo }}</span>
      </span>
    </button>
    <button class="sb-x" type="button" aria-label="关闭来源条" title="不回到报告" @click="store.clearSourceContext()">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round">
        <line x1="18" y1="6" x2="6" y2="18" />
        <line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  </div>
</template>

<script setup lang="ts">
/**
 * 报告来源条
 *
 * # 职责
 * 用户从报告里点「打开全文」跳到文章后，在正文栏顶部提供一个"一键回到原位置"的入口。
 * 点它会把弹窗重新打开并定位到那份报告（`B1` 的出口之一，见设计文档 §4.6.5）。
 *
 * # 为什么点完就清空 `B`
 * 不清的话，用户关掉弹窗后再点这条又会"返回"一次——但那时弹窗本就该是开的，
 * 行为就错乱了。清空后这条会自动消失，符合"用过即弃"的心智。
 *
 * # 为什么不恢复主屏视图
 * 跳转前可能在推荐流，回来仍停在文章阅读：用户手上正看着文章，
 * 把他拽回卡片流是反直觉的。原视图属于"跳转前的历史"，不追。
 */
import { computed } from 'vue'
import { useReportPeriodStore } from '@/stores/reportPeriod'

const store = useReportPeriodStore()

/** 当前来源上下文（为 null 时本组件不会渲染，见父级 v-if） */
const ctx = computed(() => store.sourceContext!)

/**
 * 返回来源报告
 *
 * 先清上下文再请求打开：报告若已被删除，弹窗那边会提示"该报告已不存在"，
 * 此时留着这条来源条也没有意义。
 */
function goBack() {
  const target = ctx.value
  store.clearSourceContext()
  store.requestOpen(target.reportId)
}
</script>

<style scoped>
.source-bar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  height: 32px;
  padding: 0 var(--sp-4);
  background: var(--primary-fg);
  border-bottom: 1px solid var(--border);
}
.sb-main {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: none;
  padding: 0;
  cursor: pointer;
  color: var(--primary);
  font-family: inherit;
  /* 用全局阶梯而不是 --fs-modal-label：这条来源条挂在正文栏（主屏），
     不是弹窗内，弹窗的局部令牌继承不到，写别名会让整条声明静默作废 */
  font-size: var(--fs-sm);
  font-weight: 500;
  text-align: left;
}
.sb-main svg {
  width: 13px;
  height: 13px;
  flex-shrink: 0;
}
.sb-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sb-ref {
  color: var(--text-secondary);
  font-weight: 400;
}
.sb-main:hover .sb-text {
  text-decoration: underline;
}
.sb-main:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  border-radius: 3px;
}
.sb-x {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  display: grid;
  place-items: center;
  border: none;
  background: none;
  border-radius: 5px;
  color: var(--text-tertiary);
  cursor: pointer;
}
.sb-x:hover {
  background: var(--fill);
  color: var(--text-primary);
}
.sb-x:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
</style>
