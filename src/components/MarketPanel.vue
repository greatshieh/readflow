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
          <div class="mp-h">
            价格预警
            <!-- 手动检查入口：定时任务每 5 分钟跑一次且失败静默，
                 用户加完规则无法判断"没触发"还是"没生效"，需要一次即时反馈 -->
            <button
              v-if="alerts.length"
              class="btn btn-sm mp-check-btn"
              :disabled="checking"
              :title="`立即检查一次全部规则（共 ${alerts.length} 条）`"
              @click="checkAlerts"
            >
              {{ checking ? '检查中…' : '立即检查' }}
            </button>
          </div>
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
              <span class="mp-alert-rule">{{ a.symbol }} {{ a.op }} {{ fmt(a.price) }}</span>
              <!-- 上次触发时间：一条永远不触发的规则与一个坏掉的功能，
                   用户从界面上是分不出来的，这里补上这个信号 -->
              <span class="mp-alert-time" :title="a.lastTriggered ? '上次触发时间' : '规则尚未触发过'">
                {{ a.lastTriggered ? `触发于 ${fmtTime(a.lastTriggered)}` : '未触发' }}
              </span>
            </li>
          </ul>
          <div v-else class="mp-sub">暂无预警规则</div>
          <div v-if="alerts.length" class="mp-sub">
            价格突破时发系统通知（同一标的 2 小时内只提醒一次）
          </div>
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

/**
 * 向上抛一条轻提示
 *
 * 保持"面板不做全局 UI"的分工：MarketPanel 只负责取数与渲染，
 * 提示的展示位置由父级（实体仪表盘）决定。这样同一面板将来复用到
 * 别处时不需要自带一套 Toast。
 */
const emit = defineEmits<{
  (e: 'toast', message: string): void
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
  /** 上次触发时间（UTC 字符串）；null = 从未触发 */
  lastTriggered: string | null
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
/** 「立即检查」进行中：防连点（一次检查会发网络请求） */
const checking = ref(false)

/** 数值格式化：0/NaN 显示 — */
function fmt(n: number): string {
  return n && !Number.isNaN(n) ? n.toFixed(2) : '—'
}

/**
 * 把后端返回的 UTC 时间串格式化为「MM-DD HH:mm」本地时间
 *
 * 库里存的是 `CURRENT_TIMESTAMP`（UTC，形如 `2026-11-03 06:20:11`），
 * 直接展示会与用户的本地时钟差 8 小时，让"刚触发过"看起来像"昨天触发过"。
 * 无时区后缀时按 UTC 解析（`Z`），否则 `new Date()` 会当成本地时间。
 */
function fmtTime(raw: string): string {
  const iso = raw.includes('T') ? raw : raw.replace(' ', 'T') + 'Z'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return raw
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/**
 * 立即执行一次价格预警检查
 *
 * 返回空不等于出错——可能是价格没到阈值，也可能是当前不在交易时段
 * （后端在非交易时段直接返回空，不发请求）。文案要中立，不能说"检查失败"。
 */
async function checkAlerts() {
  if (checking.value) return
  checking.value = true
  try {
    const n = await invoke<number>('market_check_alerts')
    alerts.value = await invoke<MarketAlert[]>('market_list_alerts')
    emit('toast', n > 0 ? `命中 ${n} 条规则，通知已发送` : '本次检查无触发（价格未到阈值或非交易时段）')
  } catch (e) {
    // Tauri 的 reject 是字符串而非 Error 对象
    emit('toast', `检查失败：${String(e)}`)
  } finally {
    checking.value = false
  }
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
/* 行情面板整体在 EntityDashboardModal 内，--fs-modal-* / --m-* 由 .base-modal 继承下来。
   说明与加载文案是整句 → 正文档 14 / 1.65。 */
.mp-loading,
.mp-sub {
  color: var(--text-tertiary);
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
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
  font-size: var(--fs-modal-body);
  font-family: inherit;
}
.mp-input:focus,
.mp-select:focus {
  outline: none;
  border-color: var(--primary);
}
/* 关联状态是一行字段 → 标签档 13 */
.mp-linked {
  color: var(--text-secondary);
  font-size: var(--fs-modal-label);
  white-space: nowrap;
}
.mp-placeholder {
  border: 1px dashed var(--border);
  border-radius: var(--r-lg);
  padding: var(--sp-35);
  color: var(--text-tertiary);
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
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
/* 主价格：大数字角色（22/700/1.1 + 等宽数字） */
.mp-price {
  font-size: var(--fs-modal-num);
  font-weight: 500;
  line-height: var(--lh-modal-num);
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
/* 分区标题 → 弹窗区块标题 16/600 */
.mp-h {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
  font-size: var(--fs-modal-section);
  font-weight: 600;
  line-height: var(--lh-modal-section);
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
/* 数据行属密集列表 → 标签档 13（与 .mp-linked 同档） */
.mp-fund {
  display: flex;
  gap: var(--sp-3);
  flex-wrap: wrap;
  font-size: var(--fs-modal-label);
  font-variant-numeric: tabular-nums;
}
.mp-quotes,
.mp-cal,
.mp-alerts {
  list-style: none;
  margin: 0;
  padding: 0;
  font-size: var(--fs-modal-label);
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
/* 预警规则行：左规则右时间。
   规则文本可能较长（含备注），给 flex:1 + min-width:0 让它自己省略，
   时间列 flex-shrink:0 保证不被挤掉——「上次触发」是判断规则是否活着的关键信息。 */
.mp-alert-rule {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mp-alert-time {
  flex-shrink: 0;
  white-space: nowrap;
  font-size: var(--fs-modal-meta);
  color: var(--text-tertiary);
}
/* 「立即检查」按钮靠右，与分段标题同一行；用 margin-left:auto 而不是
   在标题上加 justify-content: space-between，后者会让 ::before 色块的间距也变化 */
.mp-check-btn {
  margin-left: auto;
  flex-shrink: 0;
}
.mp-alert-add {
  display: flex;
  gap: var(--sp-2);
  align-items: center;
  flex-wrap: wrap;
}
/* .btn 走styles.css 全局族（次级描边），此处只补主操作变体的间距需求，
   不再在组件内重复定义按钮外观，避免与全局令牌漂移。 */
/* 文字链与所在段落同档（正文档 14），否则与 .mp-placeholder 的行内基线不一致 */
.btn-link {
  background: none;
  border: none;
  color: var(--primary);
  cursor: pointer;
  font-size: var(--fs-modal-body);
  padding: 0;
}
.btn-link:hover {
  text-decoration: underline;
}
</style>
