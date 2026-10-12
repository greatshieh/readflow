# ReadFlow 金融能力升级 · 文件级实施方案（待确认）

> 承接 `finance_expert_review.md`（评审）与 `finance_data_source_survey.md`（数据源调研）。
> 方向已确认：**C 混合** —— RSS 文本情报（现有）+ 行情/财务 API（新增，Tushare 主 + 新浪/东财快照）+ 用户凭据本地化。
> 本文件为**铁律 #1 要求的实施方案**：列出具体改动文件、内容与影响，经用户确认后方可实施。
> 遵守：铁律 #2（数据获取全在 Rust 后端）、铁律 #3（Schema 一次成型，`create_tables` 唯一真源）。

---

## 0. 沙箱限制（先讲清，避免误判"未验证"）

- 沙箱**无法联网**（pip/npm 网络不可用、SSH 远端不可达已多次复现）。
- 因此本方案里**任何调用 Tushare / 新浪 / 东财真实 API 的代码，在沙箱只能做 `cargo check` 编译自检，无法 live 验证返回数据**。
- 真机联网 + 用户提供 Tushare token（free tier 即可拉日线）后方可端到端验证。
- 纯代码改动（Phase 0 事件类型扩充）不依赖联网，可在沙箱完整自测。

---

## Phase 0 — 事件语义补全（纯代码 / 零 Schema / 立即可做，必做）

**目标**：把 finance 事件类型从 4 类扩到 ~14 类，消除评审 P0"事件类型过窄"。零 Schema 变更。

### 改动文件
1. `src-tauri/src/research/extract.rs`（~line 63-71 `PRESET_DOMAINS` finance 段）
   - 由 4 类扩为 ~14 类，保持"领域/类型/中文注释"三位一体格式：
     ```
     "earnings",      // 财报：营收、利润、业绩
     "profit_warning",// 业绩预告 / 业绩修正（预增、预减、变脸）
     "rating",        // 评级：上调、下调、首次覆盖
     "ipo",           // 上市：申购、挂牌、过会
     "dividend",      // 分红：派息、送转
     "buyback",       // 回购 / 增持
     "reduce",        // 减持 / 股东抛售
     "restructuring", // 重组 / 资产注入 / 剥离
     "financing",     // 再融资 / 定增 / 发债 / 可转债
     "unlock",        // 解禁 / 限售股流通
     "violation",     // 违规 / 立案 / 处罚 / 退市风险
     "margin",        // 融资融券 / 质押
     "northbound",    // 北向资金流向
     "dragon_tiger",  // 龙虎榜
     ```
   - 同步在 `eventTypeLabels.ts`（前端）补充对应中文标签；`validate_events` 按枚举自动过滤，无需改逻辑。

2. `src/styles.css`（参考已完成 #6 模式）
   - 在 `--entity-other` 段后补 tone 映射 + `.type-*` 全局类（新增类型各一个色），保持 `--tone-fg` 恒白。

3. `src/stores/eventTypes.ts` / `utils/eventTypeLabels.ts`
   - 补 `profit_warning`/`buyback`/`reduce` 等新类型的中文标签与配色 key。

**验证**：`cargo check --lib` 0 warning + 现有 `extract` 测试 + `npm run typecheck` 0 error。

---

## Phase 1 — 行情/财务数据层（核心，需 token + 真机联网）

### 1.0 数据源分层策略（用户 15:44 确认：**新浪免 token 默认 + 其他 settings 配置 + 未配置留占位符**）

采用"开箱可用 + 渐进增强"模型，化解"必须配 token 才可用"的门槛：

- **默认源 = 新浪（免 token，开箱即用）**：后端 `market` 模块默认启用 Sina client，无需任何配置即可拉**实时价/快照**；价格触发预警（1.6）也默认基于新浪快照跑。应用首次打开即有最小可用行情能力。
- **增强源 = Tushare / 东财（settings 配置后启用）**：在「设置 → 数据源」填写 Tushare token / 东财 apikey 后，提供**历史行情、财务快照、财报日历**等深度数据。`settings` 键：`tushare_token`、`emapi_key`（均未配置时留空）。
- **未配置即占位符（关键 UX）**：行情面板的每一类数据按"当前已配置的源"渲染；若某类数据对应的源未配置，显示**占位卡片**而非空白/报错，文案示例：
  - 未关联证券代码：`「关联证券代码后展示行情」`（跳转实体-代码关联）
  - 源未配置：`「配置 Tushare 后查看历史走势 / 财务快照 / 财报日历 →」`（点击跳设置对应区）
  - 配置完成后**同一界面自动出数据**，无需改代码、无需刷新重装。
- **源优先级（同一字段多源候选时）**：实时价/快照 → 新浪（默认，无需配置）→ 无网络则占位；历史/财务/日历 → Tushare（若配置）→ 东财（若配置）→ 占位符。
- **实现要点**：`market` 模块内用 `MarketSource` 枚举 + 优先级链；前端 `MarketPanel.vue` 每个数据区块独立判断"是否已配置源"，未配置渲染占位组件（含跳设置 CTA）。

> 说明：即便新浪免 token，**证券代码 ↔ 实体映射**（`market_symbols`）仍需用户关联（或抽取时按名称猜测候选待确认）——这是"未关联代码"占位符，与上"未配置源"占位符是两个独立状态。

### 1.1 新模块（铁律 #2：HTTP 全在后端）
新增 `src-tauri/src/market/`：
- `mod.rs` —— 模块入口、`MarketClient` trait/枚举（Tushare / Sina）、统一错误、`sync_entity()` 编排。
- `tushare.rs` —— `reqwest` 调用 Tushare Pro HTTP API（`https://api.tushare.pro`，POST `{token, api_name, params}`），解析日线/财务/财报日历。
- `sina.rs` —— `hq.sinajs.cn` 实时快照（免 token，约 15s 延迟），用于价格触发预警。

### 1.2 建表（铁律 #3：全部进 `create_tables`，`CREATE TABLE IF NOT EXISTS`）
`src-tauri/src/db.rs` `create_tables()` 内追加 4 表（既有真实库下次启动自动建，无需 ALTER）：
- `market_symbols`(id, entity_id, symbol, market, exchange, created_at, UNIQUE(entity_id, symbol))
  - 实体 ↔ 证券代码映射（用户手动关联，或抽取时按名称猜测候选待用户确认）。
- `market_quotes`(id, symbol, trade_date, open, high, low, close, vol, amount, adj_factor, source, PRIMARY KEY(symbol, trade_date))
  - 日线/快照时序。
- `market_fundamentals`(id, symbol, report_date, total_mv, pe_ttm, pb, ps_ttm, roe, revenue, net_profit, currency, source, UNIQUE(symbol, report_date))
- `earnings_calendar`(id, symbol, entity_id, report_date, report_type, actual_or_forecast, source, UNIQUE(symbol, report_date, report_type))

> 注：`financial_data` 表（db.rs:2258）是**AI 从文章抽取的实体指标**，与此行情时序表语义不同，不复用、不修改。

### 1.3 凭据本地化（铁律：用户凭据不出本机）
- `settings` 表新增键 `tushare_token`（经 `Setting::set` 写入，明文存本地库，库已 `.gitignore` 且有本地优先定位）。
- `market_set_token(token)` 命令写设置；所有 Tushare 调用从设置读 token，不硬编码、不落日志。

### 1.4 新增命令（lib.rs：模块声明 + `generate_handler!` 清单 + 顶部 88 命令计数同步更新）
- `market_set_token(token: String)` → 存设置。
- `market_link_symbol(entity_id: i64, symbol: String, market: String)` → 写 `market_symbols`。
- `market_sync(entity_id: i64)` → 调 Tushare 拉日线/财务/日历落库（后台任务式，失败 `eprintln!` 不阻塞）。
- `market_get_quote(symbol: String)` → 新浪快照（实时价，供价格预警）。
- `market_list_calendar(entity_id: i64, from: String, to: String)` → 财报日历。
- `market_get_fundamentals(entity_id: i64)` → 估值/财务快照。
- `market_list_quotes(symbol: String, limit: i64)` → 近期日线（前端画走势）。

### 1.5 实体详情接入
- `EntityDashboardModal`（实体仪表盘，已有）新增「行情」分区：调用 `market_list_quotes` + `market_get_fundamentals` + `market_list_calendar` 渲染；未关联代码时显示「关联证券代码」入口。
- 新增 `src/components/MarketPanel.vue`（仅渲染，数据经 invoke 取得）。

### 1.6 价格触发预警（P1，可后置到 Phase 2）
- 新增 `market_alerts` 表（symbol, op<`>`/`>=`, price, enabled）+ 命令 `market_add_alert`；定时或行情拉取后比对，命中走现有 `notify` 三道门推送「【代码】价格突破 X」。
- 沙箱不可 live 验证，真机复验。

---

## Phase 2 — 组合与预警增强（P1，后置）
- 自选股/关注列表（复用 `entities` + `market_symbols`）。
- A 股专属源模板 + OPML 导入（巨潮/交易所公告/卖方研报 RSS）。
- 研究报告 PDF/Excel 导出（复用已有 `report` 模块 + 本地导出）。

## Phase 3 — 研究深度（P2，可选，与本地优先冲突建议不做）
- 信号回测（需历史行情批量）、多源可信度加权、跨设备同步（违背本地优先，**建议不做**）。

---

## 影响范围与风险
- **后端**：新增 `market` 模块（3 文件）、`db.rs` 建表 4 表、`lib.rs` 命令 +6~7 条 + 模块声明、`notify.rs`/`settings` 复用。
- **前端**：新增 `MarketPanel.vue`、仪表盘加分区、`eventTypeLabels.ts` 标签、`SettingsModal` 加「数据源」区、`styles.css` 配色。
- **Schema**：仅新增表（进 `create_tables`，自动建表）；若后续要给 `research_events` 加 `sentiment_magnitude` 列（情感强度）才走铁律 #3 的 .bak+ALTER 流程——本方案暂不包含该列，留待确认。
- **依赖**：`reqwest`（Rust，async + json）；需确认 `Cargo.toml` 是否已引入（未引入则加，走 venv/隔离规则）。
- **联网**：新浪快照默认路径仅需真机联网（无需 token）；Tushare/东财深度数据在用户于设置填入凭据后联网拉取。沙箱全程仅 `cargo check` 编译自检。

---

## 需你拍板（确认本方案时一并答复）
1. **是否同意先落地 Phase 0（零风险、可自测），再实施 Phase 1？** 还是一次性把 Phase 0+1 代码全写完（沙箱只编译自检、真机联网复验）？注：新浪默认源无需 token，Phase 1 默认路径可立即编码；Tushare/东财深度源按你设置填入凭据后真机复验。
2. **Tushare token**：**已决议（用户 15:44）** —— 新浪免 token 作为开箱默认源，Tushare/东财在「设置 → 数据源」中可选配置；未配置的源在界面留占位符提示，配置后同一界面自动出数据。详见 §1.0。
3. **情感强度列**（`sentiment_magnitude`）本期是否要做？（若做需 .bak+ALTER 既有表）
4. **价格触发预警**（1.6）是否纳入本期，还是放 Phase 2？

> 确认以上任一项后，我按铁律 #1 进入实施（改文件 → `cargo check`/`typecheck`/`build` 自检 → 暂存不提交 → 真机复验后由你提交）。
