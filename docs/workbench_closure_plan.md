# 投研工作台三项闭环 · 实施方案（待确认）

> 承接上一轮评估结论：ReadFlow 信息链已成型，但有三处断点——**行情只显示不参与**、**无「今天要处理」入口**、**事件与价格不关联**。
> 本文件按实施顺序给出三项的文件级改动清单。依项目铁律 #1，**确认后方可实施**。
> 全程遵守：铁律 #2（网络/解析/SQLite 全在 Rust，前端只 `invoke` + 渲染）、铁律 #3（唯一建表入口 `db.rs::create_tables`，不新增 `ensure_*`）、铁律 #5（跨边界字段两边对齐 + 序列化断言单测）。

---

## 0. 共同约束（三项都适用）

| 约束 | 具体要求 |
|---|---|
| **不新增既有表列** | 三项共需 1 张**新表**（`portfolio`，仅第 3 项可选）。给 `research_events` / `market_alerts` 加列属危险操作，本方案**刻意规避**——价格反应走运行时计算，不落库。 |
| **唯一真源** | 建表只改 `db.rs::create_tables`；不在别处写 `CREATE TABLE`。 |
| **命令计数** | `lib.rs` 顶部有「N 命令」注释，新增命令须同步（+1、+2）。 |
| **契约对齐** | 新结构体一律 `#[serde(rename_all = "camelCase")]`，并在同文件 `#[cfg(test)]` 内加序列化断言（范本：`period::tests::summary_and_source_serialize_as_camel_case`）。 |
| **不碰 UI 类名** | 新增样式走 `styles.css`（≥2 组件用则提升为全局类），token 只记名不记数值。 |
| **可验证性** | 每项独立可交付、可单独回滚，三者之间无强依赖（顺序只是价值排序）。 |

### 沙箱限制（先讲清，避免误判"未验证"）

- 沙箱**无法联网** ⇒ 新浪快照、Tushare 的真实返回**无法 live 验证**，只能 `cargo test`/`cargo check` 编译与纯函数自测。
- 沙箱**Tauri GUI 起不来** ⇒ 观感（红涨绿跌、卡片密度、hover 态）必须**真机复验**，方案里标注为「真机待验」。
- 三项的**纯计算部分**（价格反应计算、待办聚合、阈值判定）都可以在沙箱用单测覆盖，这部分不留验证真空。

---

## 第 1 项 · 价格预警接入调度（最小闭环，先做）

### 现状实证

`market_alerts` 表已建（`db.rs:2416`，含 `last_triggered` 列）、`market_add_alert` / `market_list_alerts` 命令已注册（`lib.rs`）、`sina::fetch_snapshot` 可用——但**全仓没有任何调用方**。用户加规则后永远不会收到提醒。这是"半成品"，不是"未做功能"。

### 1.1 后端改动

**文件 A：`src-tauri/src/market/mod.rs`（新增 2 个函数 + 1 个结构体，约 120 行）**

```
pub struct AlertHit {
    alert_id, symbol, op, price, current, note, held(check 用)
}

/// 读取全部 enabled 规则、逐条取快照、判定是否命中。
/// 关键设计：
/// - 先读规则再决定要不要发网络请求：没有规则就零请求（多数用户没有规则，
///   每 5 分钟空发一次快照请求是纯浪费，也会触发上游限流）。
/// - 同一 symbol 的多条规则只取一次快照（HashMap 去重），N 条规则 1 次请求。
/// - 快照取不到（网络故障/退市/代码错）→ 该 symbol 静默跳过，**不写 last_triggered**，
///   也不清规则；故障不该惩罚规则。
pub async fn check_alerts(pool: &SqlitePool) -> Result<Vec<AlertHit>, String>

/// 把命中的规则写回 last_triggered（仅命中时写）
pub async fn mark_triggered(pool: &SqlitePool, alert_ids: &[i64]) -> Result<(), String>
```

**判定语义（必须写进注释，避免后人改错）：**

| 项 | 规则 | 理由 |
|---|---|---|
| 比较符 | `>` / `>=` / `<` / `<=`，未知运算符 → **不命中** | 宁可漏发，不可误发；误报会摧毁用户对预警的信任 |
| 冷却期 | `last_triggered` 在 **120 分钟内**则跳过 | 预警是"提醒",不是"每 5 分钟敲一次";120 分钟≈半天交易时段 |
| 网络失败 | 跳过、不写 `last_triggered` | 见上 |
| 现价缺失 | `close <= 0` 视为取不到 → 跳过 | 新浪对无效代码返回全 0，不能当"跌到 0"处理 |

**文件 B：`src-tauri/src/scheduler.rs`（`RefreshManager::start` 内新增第三个 Job，约 40 行）**

在现有「定时刷新」与「每天 3 点清理」之后、`scheduler.start()` 之前追加：

```rust
// 价格预警：每 5 分钟检查一次。与订阅刷新（默认 60 分钟）独立注册，
// 因为"价格跌破 X"和"新闻有几条"是两种时延需求，合成一个 Job 会让用户
// 改刷新频率时意外改掉预警灵敏度。
// 交易时段判断放在 check_alerts 内部（周末/夜间不请求，避免无效抓取）。
let pool3 = self.pool.clone();
let alert_job = Job::new_async("0 0/5 * * * ?", move |_uuid, _l| {
    let pool3 = pool3.clone();
    Box::pin(async move {
        match crate::market::check_alerts(&pool3).await {
            Ok(hits) if !hits.is_empty() => {
                crate::notify::price_alert(&pool3, &hits).await;
                let ids: Vec<i64> = hits.iter().map(|h| h.alert_id).collect();
                let _ = crate::market::mark_triggered(&pool3, &ids).await;
            }
            Ok(_) => {}  // 无命中是正常态，不打印
            Err(e) => eprintln!("价格预警检查失败: {}", e),
        }
    })
})?;
scheduler.add(alert_job).await?;
```

> **顺序约束（易错点，写进注释）**：`notify::price_alert` 必须在 `mark_triggered` **之前**调用。反过来一旦通知发送成功而写库失败，下一轮会重复提醒同一批规则；先发后写导致的漏写最多是"多提醒一次",而先写后发失败是"提醒永久丢失"。

**文件 C：`src-tauri/src/notify.rs`（新增 `price_alert`，约 50 行）**

```rust
/// 价格预警通知：**按 symbol 聚合成一条**，不按规则发多条
///
/// 同一个标的挂了 3 条价格规则是很常见的事（止损/止盈/突破各一条），
/// 逐条发等于连弹三条，用户第一反应是关掉通知开关。
/// 三重门与 send_event_alert 完全一致（句柄 → 窗口 → 总开关），
/// 额外加一道子开关 price_alert_enabled（缺省开，判据同 enabled）。
pub async fn price_alert(pool: &SqlitePool, hits: &[AlertHit])
```

> ⚠ **不做进程内限频**。`LAST_ALERT_AT` 是"事件预警"专用的限频表，价格预警自带 `last_triggered` 落库冷却（120 分钟），比进程内计时更可靠——应用重启后进程内状态清零会让"重启就重新提醒"成为可能。

**文件 D：`src-tauri/src/lib.rs`（2 行）**

- `market_list_alerts` 之后追加 `market_check_alerts`（手动触发一次检查，供前端「立即测试」按钮），命令计数 +1。
- 命令体约 20 行：`market::check_alerts` → 有命中则 `notify::price_alert` + `mark_triggered` → 返回命中数。

### 1.2 前端改动

**文件 E：`src/components/MarketPanel.vue`（改，约 60 行）**

- 预警区块现有的「添加规则」表单保持不动；
- 规则行右侧补 **「测试」按钮**：调 `market_check_alerts`，返回命中数后 toast「本次检查命中 N 条」；
- 规则行补 `lastTriggered` 展示（「上次触发 11-03 14:20」/「未触发过」）；
- 空态：由「暂无预警规则」改为带引导的「添加规则后，价格触发时会收到系统通知」；
- **必须真机验证**：通知开关关闭时点「测试」仍应返回命中数（只是不发通知），不能让用户以为没生效。

### 1.3 校验

```bash
cargo test --lib market::          # 新增 4 个单测
cargo check --lib                  # 0 warning
npm run typecheck
```

**新增单测（必须有牙齿）：**

1. `compare_op_covers_all_four_operators` —— 四运算符边界值（价格恰好等于阈值时 `>` 不命中、`>=` 命中）；
2. `missing_quote_is_not_zero_price` —— `close = 0` 的快照判为"取不到"，`<` 规则**不命中**；
3. `cooldown_blocks_repeat_within_120min` —— 刚触发过的规则在窗口内跳过；
4. `alert_hit_serializes_as_camel_case` —— 契约断言。

> 每条测试都应能"拆掉实现就变红"：例如把冷却期去掉，测试 3 必须失败。

### 1.4 风险

| 风险 | 缓解 |
|---|---|
| 5 分钟轮询是否过度 | 无规则时零请求；有规则时按本次涉及的 symbol 数（去重后）请求，通常 ≤ 10 次/5min |
| 新浪接口限流/改版 | `fetch_snapshot` 失败返回 `None` 已是静默降级；规则保留不删，修复代码后自动恢复 |
| 非交易日仍轮询 | `check_alerts` 内判「周末 / 21:00–09:00」直接返回空，不发请求（仅避免浪费，不追求完美交易日历） |

---

## 第 2 项 · 事件时间线叠加价格反应

### 现状实证

- `market_list_quotes(symbol, limit)` 已有，`market_symbols` 有 `entity_id → symbol` 映射（`market::get_symbol_for_entity`）；
- `ResearchEvent`（`db.rs:703`）**没有价格字段**，`list_with_context` 的 `COLS`（`db.rs:868`）也不带价格；
- `ResearchTimelineTab.vue` 的 `event-row` 已很拥挤（情感 + 日期 + 事实 + 类型 + 展开箭头 + 删除 = 6 个元素），`styles.css` 里有明确注释说明"左边缘只放宽度恒定元素"。

### 2.1 关键设计决策：**不落库，运行时计算**

**为什么不给 `research_events` 加 `price_change` 列**：
1. 加列是铁律 #3 里**最危险**的操作（真机需 `.bak` + ALTER）；
2. 行情会**持续修订**（Tushare 次日复权修正），落库等于把"当时的快照"固化，一旦行情源修正就永久错误；
3. 事件与价格的匹配是**查询期逻辑**（按 `event_date` 找当日 K 线），不是事件的固有属性。

**为什么不改 `list_with_context` / `EventWithContext`**：
`EventWithContext` 是报告模块共用结构（`report.rs:304` 也在读），动它的 `COLS` 会连带影响报告生成。价格反应**只在时间线视图需要**，放在独立命令里，改动面最小。

### 2.2 后端改动

**文件 A：`src-tauri/src/market/mod.rs`（新增 1 个函数 + 1 个结构体，约 120 行）**

```rust
/// 一批事件的价格反应（按 event_id 索引）
pub struct EventReaction {
    pub event_id: i64,
    /// 事件日当天涨跌幅 %；无当日 K 线为 None
    pub day_change: Option<f64>,
    /// 事件后 5 个交易日累计涨跌幅 %；数据不足为 None
    pub window_5d: Option<f64>,
    /// 事件日之前的收盘价（基准）
    pub base_close: Option<f64>,
}

/// 给一批事件批量计算价格反应
///
/// # 为什么必须批量
/// 时间线一次渲染 50–200 条事件，逐条查 K 线 = 200 次 SQL + 200 次网络。
/// 做法：① 先用 `market_symbols` 一次查出「entity_id → symbol」全表；
/// ② 按涉及的 symbol 集合一次取近 N 日 K 线到内存；③ 在内存里按日期二分定位。
/// **全程不发网络请求**——只用已落库的 `market_quotes`。
///
/// # 日期匹配规则（写死，避免后人漂移）
/// - 事件日 `event_date` 为空 → 回退文章 `published_at` 的日期部分；
/// - 仍为空 → `None`（不猜）；
/// - 行情缺失该日（停牌/未同步）→ 取**该日之前最近一个**有行情的交易日作基准，
///   并在结构体注释中说明这是"前收盘"语义。
pub async fn event_reactions(
    pool: &SqlitePool,
    events: &[(i64, i64, String)],   // (event_id, entity_id, event_date)
) -> Result<HashMap<i64, EventReaction>, String>
```

**文件 B：`src-tauri/src/lib.rs`（新增 1 命令，约 25 行，计数 +1）**

```rust
/// 取某时间窗内事件的价格反应
///
/// 入参与 `research_events_timeline` 保持一致（days / entity_id），
/// 保证前端两次调用拿到的是同一批事件，不会出现"列表 50 条、反应 48 条"。
async fn research_events_with_price(days, entity_id) -> Result<Vec<EventWithPrice>, String>
```

> **契约陷阱（重点）**：新结构体不能是 `EventWithContext` 的 `#[serde(flatten)]` 组合——前端 `ResearchEvent` 接口是手写的 TS 类型，flatten 在 Rust 侧能编过、前端会拿到 `undefined`。方案采用**显式平铺字段**（`eventId` / `dayChange` / `window5d` / `baseClose` 与原有字段并列），并在 `stores/research.ts` 里新建独立接口 `ResearchEventWithPrice extends ResearchEvent`。

### 2.3 前端改动

**文件 C：`src/stores/research.ts`（改，约 30 行）**

- 新增 `export interface ResearchEventWithPrice extends ResearchEvent { dayChange: number | null; window5d: number | null; baseClose: number | null }`
- 新增独立 `priceReactions` ref（`Map<number, ...>`），**不塞进 `events` 数组**：价格是"附加层",拉取失败时时间线必须照常显示；
- `loadEvents` 成功后**并行走一次** `research_events_with_price`（失败静默，只记 console）。

**文件 D：`src/components/ResearchTimelineTab.vue`（改行内 + 展开区，约 70 行）**

**行内（默认收起态）——只加一个「宽度恒定」的元素：**

```
[利好] 2026-11-01  ┃ +3.2%  ┃ 公司发布三季报…          [财报] ▸ ×
                    ↑ 新增，固定 56px 宽，tabular-nums，无数据显示 "—"
```

- 位置：**日期之后、事实之前**（价格是"事件的市场反应",紧贴日期语义最顺）；
- 宽度固定 56px（`+99.9%` 与 `—` 同宽），满足「左边缘只放宽度恒定元素」的约束；
- 颜色：**涨红跌绿**（项目 `--tone-*` 已有语义色，直接复用，不新增色板）；`—` 用 `--text-tertiary`；
- 窄屏（< 768px）隐藏该列：`@media` 里 `.event-market { display: none }`，避免挤压事实文本。

**展开区——补一段「市场反应」小卡片：**

```
市场反应
  当日      +3.21%   （基准收盘 148.50）
  5日累计   -1.05%
  ⓘ 来自已同步的日线数据，非实时
```

- 显式标注数据来源与时效（"非实时"），避免用户误以为这是盘中价格——金融场景里"数字的时点"比数字本身重要。

### 2.4 校验

```bash
cargo test --lib market::event_reactions   # 5 个纯函数单测
cargo check --lib && npm run typecheck
```

**单测（纯函数，不联网、不造库）：**

1. `day_change_uses_previous_close` —— 用"事件日前一交易日收盘"作基准，不是当日开盘；
2. `suspended_day_falls_back_to_last_trade` —— 事件日无 K 线时回退前一交易日，且返回 `Some` 而非 `None`（与"无价格"区分开）；
3. `five_day_window_short_of_data_returns_none` —— 只有 3 天数据时 `window5d` 为 `None`，不按 3 天算；
4. `empty_event_date_is_none` —— 空日期不猜；
5. `event_reaction_serializes_as_camel_case` —— 契约断言。

### 2.5 风险

| 风险 | 缓解 |
|---|---|
| 大部分用户没配 Tushare → 绝大多数事件无价格 | 无数据显示 `—` 而非隐藏整列（保持行高一致）；`MarketPanel` 的"未配置"占位已有，可复用其文案逻辑 |
| 一次渲染 200 条时计算变慢 | 全内存计算 + 按 symbol 批量取；实测上限 200 条 K 线查询为 2 次 SQL |
| 复权口径不一致导致涨跌幅失真 | 结构体注释明确"基于已落库原始收盘价，不做复权换算"；后续若接复权因子再升级 |

---

## 第 3 项 · 主屏「今天要处理」

### 现状实证

- `MainView = 'articles' | 'graph' | 'recommend' | 'review'`（`ui.ts:33`），无"今日"视图；
- 数据源全部**已存在**：`thesis_review_overview`（到期论点）、`earnings_calendar`（财报日历）、`market_alerts.last_triggered`（已触发预警）、`coverage_list`（到期退出条件）；
- `ArticleColumn.vue` 顶部已有 `ArticleListToolbar` + `ArticleSortBar`，两段固定高度——**追加第三段会挤压列表**，这是必须处理的布局约束。

### 3.1 布局决策：**顶部折叠条，不占常驻高度**

**为什么不做成独立列 / 独立视图**：用户打开应用的第一动作是读文章，"今天要处理"是**前置提醒**，不是目的地。做成整页视图等于把阅读器改成了待办应用，违背产品定位；做成第三栏则挤压正文。

**方案**：`ArticleColumn` 顶部、`ArticleListToolbar` 之上，加一条**可折叠的摘要条**：

```
┌─────────────────────────────────────────────────┐
│ 今天要处理 · 3 项   ⚠1 复盘2 财报0   [展开 ▾] │   ← 收起态：单行 40px
└─────────────────────────────────────────────────┘
（展开后：最多 5 条，每条一行 + 一键处理按钮 + 「查看全部 →」）
```

- **无待办时整条隐藏**（不显示"今天要处理 · 0 项"）——空提醒是噪音；
- 折叠状态存 `settings`（`today_bar_collapsed`），跨会话保持；
- 展开态最多 5 条（超出显示"还有 N 项 →"进复盘页/覆盖页），保证不撑爆列表区；
- 位置在工具栏之上而非之下：打开应用第一眼看到的是"该处理什么",第二眼才是列表。

### 3.2 待办聚合逻辑（后端，单一真源）

**文件 A：`src-tauri/src/market/…` 不动**，新增独立模块 **`src-tauri/src/workbench.rs`**（约 260 行）

> 为什么不塞进 `research/`：它是**跨域只读聚合**（论点 + 复盘 + 日历 + 预警 + 覆盖），放任何一个域模块里都会让那个模块反向依赖其余三个域。独立模块 + 单向依赖（它依赖所有人，没人依赖它）才符合铁律 #9 的 DAG 约束。

```rust
pub struct TodoItem {
    /// 类别：thesis_due / review / earnings / alert / coverage_exit
    pub kind: String,
    /// 一句话（后端拼好，前端只渲染——文案含动态数字，前端拼会出现两处口径）
    pub title: String,
    /// 补充说明（实体名 / 日期 / 阈值）
    pub detail: String,
    /// 优先级：high / mid / low，决定排序
    pub priority: String,
    /// 跳转目标：review / entity / thesis / coverage
    pub target: String,
    /// 跳转目标 ID
    pub target_id: i64,
    /// 是否逾期（前端标红）
    pub overdue: bool,
    /// 一键处理动作标识：mark_reviewed / open_thesis / open_entity
    pub action: String,
}

pub struct TodaySummary {
    pub items: Vec<TodoItem>,
    /// 各类别计数（收起态摘要用）
    pub counts: Vec<(String, i64)>,   // 序列化为 [{kind,count}]
    pub total: i64,
}

/// 聚合当日待办
///
/// # 口径（写死，避免与各模块页面显示不一致）
/// - **thesis_due**：`thesis_review_overview` 里 `due` 区，且 `overdue_days > 0`；
/// - **review**：同一 overview 的 `health` 区 high 严重级 + `quiet == false`（每实体只取一条）；
/// - **earnings**：`earnings_calendar` 中 `report_date` ∈ [今天, 今天+7天]；
/// - **alert**：`market_alerts` 中 `last_triggered` 在 24 小时内；
/// - **coverage_exit**：`coverage` 中 `tier = 'exit'` 且 `updated_at` 距今 > 30 天（该重新定夺）。
///
/// # 为什么复用各模块已有查询而不是新写 SQL
/// 一旦这里自己写 SQL，就出现了第二份"什么叫逾期论点"的定义，
/// 两天后页面改口径、这里没改，"今天要处理说有 3 条、点进去只有 2 条"必然发生。
/// 因此只做**编排**（调用各模块 `pub` 函数）+ 排序，不写自己的 WHERE。
pub async fn today(pool: &SqlitePool) -> Result<TodaySummary, String>
```

**排序规则（写进注释）**：`overdue(逾期) > priority > 类别固定序（alert > earnings > thesis_due > review > coverage_exit）> target_id`。同分时按 ID 稳定排序，避免每次刷新换位（列表跳来跳去比不排序更烦）。

**文件 B：`src-tauri/src/lib.rs`**（模块声明 1 行 + 命令 1 个约 15 行 + 计数 +1）

```rust
/// 今日待办聚合（跨域只读）
async fn workbench_today() -> Result<workbench::TodaySummary, String>
```

### 3.3 前端改动

**文件 C：`src/stores/workbench.ts`（新建，约 150 行）**

- 照 `coverage.ts` 范本：`defineStore` + `ref` + 全部通过本 store 取数，**没有组件自己 invoke**；
- **必须带 HMR**（`acceptHMRUpdate`），否则新 action 会 `undefined`；
- action：`load()` / `toggleCollapsed()` / `handleAction(item)` / `dismiss(item)`；
- `dismiss` 只在本次会话内隐藏（`sessionStorage`），**不落库**——"今天不想看"不该变成永久屏蔽。

**文件 D：`src/components/TodayBar.vue`（新建，约 220 行）**

- 收起态：单行，左侧「今天要处理 · N 项」+ 分类计数小标 + 右侧「展开/收起」；
- 展开态：≤5 行，每行 `[类别徽标] 标题 · 补充说明 [一键处理]`；
- **一键处理的行为必须明确**（三种，不能混用一个"处理"）：
  - `mark_reviewed` → 调 `thesis_review_mark`，从列表移除；
  - `open_thesis` / `open_entity` → 关闭展开条 + 跳转（**跳转不等于处理完**，条目保留）；
- 键盘：整条 `tabindex="0"`，Enter 展开、Esc 收起（遵循既有 Teleport/键盘等价约定）；
- 无待办时 `v-if` 整条不渲染。

**文件 E：`src/components/ArticleColumn.vue`（改，约 10 行）**

在 `<ArticleListToolbar>` **之前**插入 `<TodayBar />`。工具栏与排序栏不动——`TodayBar` 自带折叠，收起态只占 40px。

**文件 F：`src/styles.css`（新增约 90 行）**

- `.today-bar` 基础态 + `.today-bar.expanded`；
- **flex 三件套**：展开列表 `flex: 1 1 auto` + `min-height: 0` + `overflow-y: auto`，max-height 240px；
- 逾期红：`color: var(--danger)`；分类徽标复用 `--tone-*`，**不新增色板**；
- 行内按钮 `min-width: 24px; min-height: 24px` + `:focus-visible`（hover 才出现的按钮必须补键盘态）；
- 窄屏（< 768px）：收起态只显示「今天要处理 · N 项」，分类计数隐藏。

**文件 G：`src/App.vue`（改，约 20 行）**

- `onMounted` 里 `workbenchStore.load()`；
- 订阅既有 `articles-updated` / `thesis` 变更事件 → 节流 30s 后重新 `load()`（**不要每次渲染都拉**）；
- 切回窗口（`visibilitychange`）时若距上次加载 > 5 分钟则刷新——这是"打开就看见"的关键。

### 3.4 校验

```bash
cargo test --lib workbench::   # 6 个单测
cargo check --lib && npm run typecheck && npm run build:frontend
```

**单测：**

1. `overdue_sorts_before_high_priority` —— 逾期项必须在 high 之前；
2. `no_items_yields_empty_not_error` —— 空库（新装用户）返回 `total: 0` 且不报错；
3. `earnings_window_is_seven_days` —— 边界：今天 +7 天在内、+8 天在外；
4. `alert_within_24h_only` —— 24h 边界；
5. `today_summary_serializes_as_camel_case` —— 契约断言；
6. `same_priority_keeps_stable_order` —— 同分两次调用顺序一致（防列表跳动）。

### 3.5 风险

| 风险 | 缓解 |
|---|---|
| 聚合查询每次命中 5 张表，频繁刷新变慢 | 全量刷新节流 30s；单次实测目标 < 50ms（各表均有索引，数据量千级） |
| 口径漂移（这里说 3 条、点进去 2 条） | 只编排不写 SQL（前述），复用各模块 `pub` 函数 |
| 4 类待办都没有时整条消失，用户以为功能坏了 | `OnboardingModal` 文案补一句说明；空态不渲染但设置里有开关说明 |
| 用户误把"展开"当"已处理" | 每条都有独立「处理」按钮，展开本身不改变任何数据 |

---

## 4. 实施顺序与分批提交建议

| 批次 | 内容 | 预估改动量 | 可独立回滚 |
|---|---|---|---|
| **P1** | 第 1 项：价格预警接线 | 4 文件 / ~230 行后端 + 60 行前端 | ✅ |
| **P2** | 第 2 项：事件价格反应 | 3 文件 / ~170 行后端 + 100 行前端 | ✅（前端降级为无价格列） |
| **P3** | 第 3 项：今天要处理 | 1 新模块 / ~270 行后端 + 380 行前端 | ✅ |

**每批的自检顺序（不可省）**：

```bash
cargo test --lib && cargo check --lib        # 后端
npm run typecheck                            # 前端（.vue 导入路径/模板组件查不出，需额外 grep）
grep -r "import TodayBar" src/               # 新增组件必须被真实引用
npm run build:frontend                       # 生产产物能打包
```

**真机待验清单（沙箱做不到）**：
- 通知实际弹出（第 1 项，需关闭窗口后在后台触发）；
- 涨红跌绿在两套主题下都正确（第 2 项）；
- `TodayBar` 展开后列表区不被挤压、滚动正常（第 3 项）；
- 窄屏（≤768px）三项的降级表现。

---

## 5. 需要你拍板的三点

1. **价格预警轮询间隔**：默认 5 分钟。是否接受？若你更在意电量/请求量，可改 15 分钟（`0 0/15 * * * ?`）。
2. **「今天要处理」是否含 `coverage_exit` 类别**：我倾向**包含**（"盯了 30 天该重新定夺"是投研纪律的核心），但它会让待办条目变多。若你觉得吵，可以首版只放前四类。
3. **第 3 项是否顺带加一个 `portfolio` 表**（持仓）：本方案**未包含**。持仓会让"预警/待办"的优先级有真实权重来源，但涉及个人财务数据、且是新增功能而非修复断点。建议**单独一期**再做，不混进本轮。

> 确认后我按 P1 → P2 → P3 逐批实施，每批完成即跑上述校验、暂存不提交，由你真机复验后再提交。
