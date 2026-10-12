<template>
  <div class="market-panel">
    <div v-if="!state" class="mp-loading">加载中…</div>
    <template v-else>
      <!-- 关联证券代码 -->
      <div class="mp-link">
        <input
          v-model="symbolInput"
          class="mp-input"
          placeholder="证券代码，如 600519.SH / 000001.SZ"
          @keyup.enter="linkSymbol"
        />
        <button class="btn btn-primary" :disabled="!symbolInput.trim()" @click="linkSymbol">关联</button>
        <span v-if="state.symbol" class="mp-linked">已关联：{{ state.symbol }}</span>
      </div>

      <!-- 未关联代码：占位 -->
      <div v-if="!state.symbol" class="mp-placeholder">
        关联证券代码后展示行情数据
      </div>

      <template v-else>
        <!-- 实时快照（新浪免 token 默认源） -->
        <div class="mp-snapshot">
          <div class="mp-price" :class="priceClass">{{ snapshot ? fmt(snapshot.close) : '—' }}</div>
          <div class="mp-sub" v-if="snapshot">更新于 {{ snapshot.tradeDate }}</div>
          <div class="mp-sub mp-warn" v-else>实时快照暂不可达（需联网且新浪可访问）</div>
        </div>

        <!-- 深度数据：依赖 Tushare 配置 -->
        <div v-if="!state.tushareConfigured" class="mp-placeholder mp-source">
          <div>配置 Tushare 后查看历史走势 / 财务快照 / 财报日历</div>
          <button class="btn-link" @click="openSettings">去设置配置 →</button>
        </div>

        <template v-else>
          <div class="mp-section">
            <div class="mp-h">财务快照</div>
            <div v-if="fundamental" class="mp-fund">
              <span>PE(TTM) {{ fmt(fundamental.peTtm) }}</span>
              <span>PB {{ fmt(fundamental.pb) }}</span>
              <span>总市值 {{ fmt(fundamental.totalMv) }} 万</span>
            </div>
            <div v-else class="mp-sub">暂无（请先「同步行情」）</div>
            <button class="btn" @click="sync">同步行情</button>
          </div>

          <div class="mp-section">
            <div class="mp-h">近期日线</div>
            <ul v-if="quotes.length" class="mp-quotes">
              <li v-for="q in quotes" :key="q.tradeDate">
                <span>{{ q.tradeDate }}</span><span class="mp-num">{{ fmt(q.close) }}</span>
              </li>
            </ul>
            <div v-else class="mp-sub">暂无（请先「同步行情」）</div>
          </div>

          <div class="mp-section">
            <div class="mp-h">财报日历</div>
            <ul v-if="calendar.length" class="mp-cal">
              <li v-for="c in calendar" :key="c.reportDate + c.reportType">
                <span>{{ c.reportDate }}</span><span>{{ c.reportType || '—' }}</span>
              </li>
            </ul>
            <div v-else class="mp-sub">暂无</div>
          </div>
        </template>

        <!-- 价格预警（基于新浪实时快照） -->
        <div class="mp-section">
          <div class="mp-h">价格预警</div>
          <div class="mp-alert-add" v-if="state.symbol">
            <select v-model="alertOp" class="mp-select">
              <option value=">">&gt;</option>
              <option value="<">&lt;</option>
              <option value=">=">&ge;</option>
              <option value="<=">&le;</option>
            </select>
            <input v-model.number="alertPrice" class="mp-input" type="number" placeholder="触发价" />
            <button class="btn" @click="addAlert">添加</button>
          </div>
          <ul v-if="alerts.length" class="mp-alerts">
            <li v-for="a in alerts" :key="a.id">
              {{ a.symbol }} {{ a.op }} {{ fmt(a.price) }}
            </li>
          </ul>
          <div v-else class="mp-sub">暂无预警规则</div>
        </div>
      </template>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, watch, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const props = defineProps<{
  /** 实体 ID（来自实体仪表盘） */
  entityId: number | null
}>()

interface MarketState {
  symbol: string | null
  tushareConfigured: boolean
  emConfigured: boolean
}
interface Quote {
  symbol: string
  tradeDate: string
  open: number
  high: number
  low: number
  close: number
  vol: number
  amount: number
  source: string
}
interface Fundamental {
  symbol: string
  reportDate: string
  totalMv: number
  peTtm: number
  pb: number
  psTtm: number
  roe: number
  revenue: number
  netProfit: number
  currency: string
  source: string
}
interface CalItem {
  symbol: string
  entityId: number
  reportDate: string
  reportType: string
  actualOrForecast: string
  source: string
}
interface MarketAlert {
  id: number
  symbol: string
  op: string
  price: number
  enabled: boolean
  note: string
}

const state = ref<MarketState | null>(null)
const symbolInput = ref('')
const snapshot = ref<Quote | null>(null)
const quotes = ref<Quote[]>([])
const fundamental = ref<Fundamental | null>(null)
const calendar = ref<CalItem[]>([])
const alerts = ref<MarketAlert[]>([])
const alertOp = ref('>')
const alertPrice = ref<number>(0)

/** 数值格式化：0/NaN 显示 — */
function fmt(n: number): string {
  return n && !Number.isNaN(n) ? n.toFixed(2) : '—'
}

/** 价格涨跌色：仅以是否有快照决定中性，具体涨跌需对比昨收（此处简版取中性） */
const priceClass = ref('mp-neutral')

async function load() {
  if (!props.entityId) return
  state.value = await invoke<MarketState>('market_get_state', { entityId: props.entityId })
  if (state.value?.symbol) {
    snapshot.value = await invoke<Quote | null>('market_get_quote', { symbol: state.value.symbol })
    if (state.value.tushareConfigured) {
      quotes.value = await invoke<Quote[]>('market_list_quotes', {
        symbol: state.value.symbol,
        limit: 20,
      })
      fundamental.value = await invoke<Fundamental | null>('market_get_fundamentals', {
        symbol: state.value.symbol,
      })
      calendar.value = await invoke<CalItem[]>('market_list_calendar', { entityId: props.entityId })
      alerts.value = await invoke<MarketAlert[]>('market_list_alerts')
    } else {
      quotes.value = []
      fundamental.value = null
      calendar.value = []
      alerts.value = []
    }
  } else {
    snapshot.value = null
  }
}

async function linkSymbol() {
  if (!props.entityId || !symbolInput.value.trim()) return
  await invoke('market_link_symbol', {
    entityId: props.entityId,
    symbol: symbolInput.value.trim(),
    market: '',
  })
  await load()
}

async function sync() {
  if (!props.entityId) return
  await invoke('market_sync', { entityId: props.entityId })
  await load()
}

async function addAlert() {
  if (!state.value?.symbol || !alertPrice.value) return
  await invoke('market_add_alert', {
    symbol: state.value.symbol,
    op: alertOp.value,
    price: alertPrice.value,
    note: '',
  })
  alerts.value = await invoke<MarketAlert[]>('market_list_alerts')
}

/** 打开设置弹窗并定位数据源区（本项目弹窗走 window CustomEvent） */
function openSettings() {
  window.dispatchEvent(new CustomEvent('open-settings'))
}

onMounted(load)
watch(() => props.entityId, load)
</script>

<style scoped>
.market-panel {
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.mp-loading,
.mp-sub {
  color: var(--text-tertiary);
  font-size: var(--fs-sm);
}
.mp-link {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex-wrap: wrap;
}
.mp-input,
.mp-select {
  background: var(--surface);
  color: var(--text-primary);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  padding: 6px var(--sp-2);
  font-size: var(--fs-sm);
  font-family: inherit;
}
.mp-input:focus,
.mp-select:focus {
  outline: none;
  border-color: var(--primary);
}
.mp-linked {
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  white-space: nowrap;
}
.mp-placeholder {
  border: 1px dashed var(--border);
  border-radius: var(--r-lg);
  padding: var(--sp-35);
  color: var(--text-tertiary);
  font-size: var(--fs-sm);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  align-items: flex-start;
}
.mp-source {
  background: var(--surface);
}
/* 实时价格卡：与仪表盘科技风一致——左侧主色轨+ 微光底 */
.mp-snapshot {
  display: flex;
  align-items: baseline;
  gap: var(--sp-15);
  padding: var(--sp-3) var(--sp-35);
  border: 1px solid var(--tech-line);
  border-left: 3px solid var(--tech-rail);
  border-radius: var(--r-lg);
  background: linear-gradient(180deg, var(--tech-glow), transparent);
}
.mp-price {
  font-size: var(--fs-2xl);
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--text-primary);
}
.mp-price.mp-neutral {
  color: var(--text-primary);
}
.mp-warn {
  color: var(--tone-amber);
}
/* 分区标题沿用科技风分组头语汇（序号由模板省略，用主色小方块代替） */
.mp-section {
  border-top: 1px solid var(--tech-line);
  padding-top: var(--sp-15);
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}
.mp-h {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-secondary);
}
.mp-h::before {
  content: '';
  width: 3px;
  height: 12px;
  border-radius: 2px;
  background: var(--tech-rail);
  flex-shrink: 0;
}
.mp-fund {
  display: flex;
  gap: var(--sp-3);
  flex-wrap: wrap;
  font-size: var(--fs-sm);
  font-variant-numeric: tabular-nums;
}
.mp-quotes,
.mp-cal,
.mp-alerts {
  list-style: none;
  margin: 0;
  padding: 0;
  font-size: var(--fs-sm);
  max-height: 160px;
  overflow-y: auto;
}
.mp-quotes li,
.mp-cal li,
.mp-alerts li {
  display: flex;
  justify-content: space-between;
  gap: var(--sp-2);
  padding: var(--sp-025) 0;
  border-bottom: 1px solid var(--border);
  font-variant-numeric: tabular-nums;
}
.mp-num {
  color: var(--text-primary);
}
.mp-alert-add {
  display: flex;
  gap: var(--sp-2);
  align-items: center;
  flex-wrap: wrap;
}
/* .btn 走styles.css 全局族（次级描边），此处只补主操作变体的间距需求，
   不再在组件内重复定义按钮外观，避免与全局令牌漂移。 */
.btn-link {
  background: none;
  border: none;
  color: var(--primary);
  cursor: pointer;
  font-size: var(--fs-sm);
  padding: 0;
}
.btn-link:hover {
  text-decoration: underline;
}
</style>
