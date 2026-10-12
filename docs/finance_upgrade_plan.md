# ReadFlow 金融投资能力升级实施方案（草案，待确认）

> 承接 `docs/finance_expert_review.md` 的评审结论（P0/P1/P2 缺口），给出可落地的分阶段升级方案。
> 本文为**实施方案草案**：依据项目铁律 #1，所有代码改动须用户确认后方可实施；本文仅做规划与范围界定。
> 约束：铁律 #2（HTTP/数据获取/解析/AI/SQLite 全在 Rust，前端只 `invoke`）与铁律 #3（Schema 一次成型，新表/列只在 `create_tables`，真机有数据则 `.bak` + 一次性手工 ALTER，代码真源仍唯一）全程适用。

---

## 0. 升级前的两项架构约束（务必先定）

1. **数据来源的边界（决定 Phase 1 可行性）**
   - 现状：纯 RSS，无行情/基本面。
   - 选项 A：**接入可联网金融数据源**（行情/财务/日历 API，需评估 key、频率限制、合规与费用）。
   - 选项 B：**仅扩充 RSS 源**（巨潮/交易所公告、卖方研报 RSS、RSSHub 聚合），不接实时行情。
   - 选项 C：**A + B 混合**。
   - ⚠️ 此决策影响 Phase 1 全部设计，需用户拍板。
2. **本地优先 vs 跨设备同步**：铁律层面本地优先是默认；若需同步/协作，须明确取舍（建议本期仍保持本地优先，同步列为 P2 可选）。

---

## 1. Phase 0 — 事件语义补全（纯代码，零/低风险）

**目标**：让 finance 事件覆盖面与情感表达达到"够用"，且不触碰 Schema 红线（或仅走铁律 #3 标准流程）。

| 项 | 内容 | 涉及文件 | Schema 影响 |
|---|---|---|---|
| P0-1 扩展 finance 事件类型 | `PRESET_DOMAINS` 的 finance 域由 4 类扩至 ~14 类：业绩预告/业绩修正/预期差、回购/增持/减持/股权激励、可转债/配股/定增/解禁、违规/立案/退市风险/停牌、融资融券/北向资金/大宗交易/龙虎榜 | `src-tauri/src/research/extract.rs` | 零（枚举走模型抽取 + `effective_event_types` 并集，复用既有机制） |
| P0-2 前端标签同步 | 新增类型中文标签 + 配色（`--entity-*` 映射，沿用 `--tone-*` 色板） | `src/stores/eventTypes.ts`、`src/utils/eventTypeLabels.ts`、`src/styles.css` | 零 |
| P0-3 情感强度维度 | `ExtractedEvent` 增 `sentiment_magnitude: Option<f32>`（如 -1..1 强度），预警/报告据此分级 | `src-tauri/src/research/mod.rs`、`extract.rs`、`notify.rs`、`report.rs` | **需加列**（events 表）→ 走铁律 #3：`create_tables` 加 `sentiment_magnitude`，真机 `.bak` + 一次性 ALTER |
| P0-4 多源去重 | events 表增 `dedup_key`（实体+类型+日期+归一化事实），抽取后聚合同源重复报道 | `db.rs`、`extract.rs` | **需加列** → 铁律 #3 |

**交付物**：finance 事件类型覆盖达标；情感可表达强度；同源事件去重。
**工作量**：小（~2–3 天）。**依赖**：无。

---

## 2. Phase 1 — 数据与事件层（P0 核心，最大投入）

**目标**：让情报能"对齐价格/估值/未来事件"，进入研究员工作流。

| 项 | 内容 | 涉及文件 | Schema 影响 |
|---|---|---|---|
| P1-1 行情/基本面接入（新模块 `market`） | 新建 `src-tauri/src/market.rs`：按铁律 #2 在 Rust 内拉取行情/财务；表 `quotes`(symbol,date,o/h/l/c,vol)、`fundamentals`(symbol,period,营收/利润/…)、`indices`(板块/指数) | `market.rs`（新）、`lib.rs`（注册命令）、`db.rs` | **新表** → 铁律 #3：`create_tables` 一次性定义 + 真机 `.bak` + ALTER |
| P1-2 财报日历 | 表 `earnings_calendar`(symbol,report_date,period,eps_est,eps_act)；RSS + 数据源混合刷新 | `market.rs`、`scheduler` | **新表** → 铁律 #3 |
| P1-3 实体↔代码映射 | 表 `entity_symbols`(entity_id,symbol,market)，关系图/报告可显示股票代码与涨跌 | `db.rs`、`research/mod.rs` | **新表** → 铁律 #3 |
| P1-4 前端行情视图 | 实体仪表盘/关系图叠加行情卡片与迷你走势（前端仅渲染，数据来自 `invoke`） | `EntityDashboardModal.vue`、`GraphView.vue`、`stores` | 零（前端只渲染） |

**交付物**：可查任意实体/代码的行情、财务、财报日历；报告可对齐"新闻 + 价格反应"。
**工作量**：大（~1.5–2.5 周，取决于数据源选项）。**依赖**：第 0 节决策 A/B/C。

---

## 3. Phase 2 — 组合与预警增强（P1）

**目标**：把"舆情"与"我的持仓/价格"打通。

| 项 | 内容 | 涉及文件 | Schema 影响 |
|---|---|---|---|
| P2-1 自选股/组合 | 表 `watchlist`(symbol,note)、`portfolio`(symbol,shares,cost)；实体页显示持仓相关度 | `db.rs`、前端页 | **新表** → 铁律 #3 |
| P2-2 价格触发预警 | 规则类型扩展 `trigger: price_above/below`，定时轮询 `quotes` 触发推送 | `notify.rs`、`scheduler` | 零（规则存 `alert_rules` JSON 已支持扩展） |
| P2-3 A 股专属源模板 | 内置 RSSHub/公告源预设（巨潮/北向/龙虎榜）+ OPML 导入导出 | `feeds` 相关、`Settings` | 零 |
| P2-4 报告导出 PDF/Excel | `report.rs` 增导出（HTML→PDF 或结构化→xlsx），供客户/团队分发 | `report.rs`、前端按钮 | 零 |

**工作量**：中（~1–1.5 周）。**依赖**：Phase 1（价格数据）。

---

## 4. Phase 3 — 研究深度（P2，可选）

| 项 | 内容 | 说明 |
|---|---|---|
| P3-1 信号回测 | 历史事件情感/预警 vs 后续涨跌幅，算命中率/IC，给出可信度 | 需 Phase 1 历史行情 |
| P3-2 多源可信度加权 | source 表加 `credibility`，事件聚合加权展示 | 提升噪声环境可用性 |
| P3-3 跨设备同步（可选） | 与本地优先冲突，需用户明确取舍 | 建议本期不做 |

---

## 5. 推荐落地顺序与里程碑

1. **M1（Phase 0）**：事件类型 + 情感强度 + 去重 —— 低成本立竿见影，建议优先。
2. **M2（Phase 1）**：数据接入 —— 门槛最高，依赖第 0 节决策；建议先做"财报日历 + 基础行情"最小集。
3. **M3（Phase 2）**：组合 + 价格预警 + 导出。
4. **M4（Phase 3）**：回测 + 可信度（按反馈决定）。

---

## 6. 待用户确认的关键决策（实施前必须拍板）

1. **P1 数据来源**：A（联网金融 API）/ B（纯 RSS 扩充）/ C（混合）？—— 决定 Phase 1 能否启动及成本。
2. **情感强度（P0-3）是否本期做**？需 events 表加列（铁律 #3 流程）。
3. **是否需要持仓/组合（P2-1）**？涉及个人财务数据，隐私权衡。
4. **A 股专属源**是否纳入本期（依赖 RSS 源可用性调研）。

---

## 7. 风险与缓解

- **Schema 变更风险**（P0-3/P1 全量）：统一走铁律 #3——`create_tables` 为唯一真源，真机先 `.bak` 再一次性 ALTER，沙箱库可直接重置。
- **数据源合规/稳定性**：优先选有明文授权的数据源；RSS 源做可用性探测与降级。
- **AI 情感误判**：强度维度 + 多源去重 + "需人工复核"提示，缓解二元误导。
- **工作量集中**：Phase 1 最大，建议先交付最小可用子集（日历 + 日频行情），再迭代。

> 本文为草案。确认后，将按铁律 #1 对每一 Phase 拆出细化实施方案（具体文件改动清单）再行实施。
