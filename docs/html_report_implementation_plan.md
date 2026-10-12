# 实施方案：Obsidian 研究报告增加自包含 HTML 报告

> 状态：**待确认**（按项目铁律 #1，代码改动需用户确认后方可实施）
> 关联评审：全员评审（PM/架构/QA/UI/数据/投资）已交叉验证，本方案吸收其 P0/P1 结论。

## 0. 需求澄清（用户已确认）

| # | 问题 | 确认结论 |
|---|------|----------|
| 1 | 综述/评论/建议由谁生成 | **AI 生成**，使用应用中已配置的模型（复用抽取链路的 provider 解析，缺省 `agnes`） |
| 2 | HTML 形态 | **独立 `.html` 文件**，纯静态、单文件自包含（内联 CSS/JS，无外部资源） |
| 3 | 与现有 Markdown 报告关系 | **共存**：同一目录下 `.md` 与 `.html` 并列（sidecar） |
| 4 | 时间线/类型视图标什么 | **都要**：事件摘要 + 原文链接 |
| 5 | LLM 不可用时的行为 | **模板占位**：三段渲染占位卡，不阻断导出 |

## 1. 目标架构 / 数据流

```
当前：  抽取 → 事件入 SQLite(+sentiment/financial) → render_report(模板 MD) → 多通道落盘
目标：  抽取 → 事件入 SQLite → 聚合(情感加权/类型计数/财务快照)
                       │
                       ├─[+1 次 LLM 调用]→ 叙述 Narration{综述,评论,建议}（失败则 None）
                       │
                       ├─ render_report(MD)            [保持并增强：补情感/财务摘要]
                       └─ render_report_html(自包含)   [新增]
                                │
                      多通道落盘（每通道双写 .md + .html sidecar）
```

**铁律兼容性**：AI 调用与 SQLite 全在 Rust 侧，前端零改动（仅可选增强导出提示）；叙述生成于导出时、**不落库** → 零 Schema 变更（符合铁律 #3）。

## 2. 改造范围与文件清单

### Rust（核心改动集中在 `src-tauri/src/research/report.rs`）

1. **`parse_report_config` 增加 `provider_id`**：取自 config JSON 的 `provider`，缺省 `"agnes"`（与 `research/mod.rs:157-160` 同规则）。
2. **新增 `generate_narration`**（异步）：
   - 调 `ai::chat_completion(provider, prompt, Some(0.3))`（`ai.rs:951`）。
   - prompt 注入**压缩后的事件清单**（id / 类型 / 日期 / 事实 / 情感，不含长证据以省 token）+ 指令「仅基于下列数据，引用事件 id 防幻觉」。
   - 返回 `Result<Option<Narration>, String>`；结构化 JSON 解析失败 / LLM 报错 / 超时 → 返回 `None`（降级，不传播错误）。
   - `Narration { summary, commentary, recommendations }`（`serde::Deserialize`）。
3. **新增 `render_report_html`**（纯函数，同步）：
   - 入参：`kind, days, &rows, &agg, narration: Option<Narration>`。
   - 自包含 HTML：`<head><style>` 内联；`:root` 变量**严格对齐应用真实 token**——`--primary`（非 `--accent`）、`--text-primary/secondary/tertiary`、`--surface`、`--border`、`--success`(利好绿)/`--danger`(利空红)/`--warning`(中性/热度)。
   - **严禁**裸 hex、`--accent`、`--text`、`--text-dim`、`--text-1/2`（已证实不存在，参见前期 UI 评审 P0）。
   - `@media (prefers-color-scheme: dark)` 深色配色 + `@media print` 打印/PDF 优化。
   - 结构：① 顶部信息卡（实体数/区间/生成时间 + 净情感/风险等级徽标）；② 综述；③ 评论；④ 建议；⑤ 时间线视图（按 `event_date` 倒序事件流，每事件=中文类型徽标+日期+事实+证据引用+来源链接）；⑥ 类型视图（按生效事件类型分组，每组列「摘要+原文链接」）。
   - 叙述为 `None` 时三段渲染占位卡：「本报告的综述/评论/建议由 AI（模型 X）生成，当前模型不可用，已留空」。
   - finance 事件（earnings/rating/ipo/dividend）特殊语义徽标（与前期投资评审 P0 一致）。
4. **新增安全函数 `html_escape` / `sanitize_url`**（建议放 `src-tauri/src/util/html.rs` 供全局复用）：
   - `html_escape`：转义 `& < > " '` 及控制字符；`sanitize_url`：仅放行 `http:// https://`（拦截 `javascript:` 等）。
   - 所有动态字段（fact/evidence/entity_name/source URL/叙述文本）必经转义。
5. **情感聚合增强**：新增 `aggregate_sentiment(rows)` 按 `sentiment_confidence` **加权**（`SUM` 而非 `COUNT(*)`，修正 `db.rs:578` 未加权问题），输出 利好/利空/中性 净分与主导倾向。
6. **事件类型中文标签（Rust 侧）**：运行时由 `effective_event_types`（`extract.rs:102`）+ `custom_event_types` 聚合得到 `Map<value, label>`，覆盖自定义类型；finance 域事件映射中文（财报/评级/IPO/分红）。
7. **`generate_report` 改造**（`report.rs:187`）：在 `render_report` 后解析 `provider_id` → `generate_narration` → `render_report_html`；把 `markdown` 与 `html` 一并交落盘。
8. **落盘双写**：扩展 `write_report_to`（`report.rs`，被 `deliver_to_obsidian`/`both` 调用）在写 `.md` 的同时以同基名写 `.html`（如 `research_daily_2026-10-11.md` + `.html`）。`app`/`obsidian`/`both`/`file` 四通道均双写。

### 前端（最小，可选）
- 现有导出入口若已 `invoke('generate_report')` 无需改动；结果提示补一句「同时生成 HTML」。
- 报告查看器可选加「打开 HTML」按钮（调 `open_path`）——**不阻塞本期**。

## 3. 分步实施（每步自带校验）

1. **安全基座**：`html_escape` / `sanitize_url` + 单测（零依赖，先锁 XSS）。
2. **聚合层**：`aggregate_sentiment` 加权 + Rust 侧事件类型标签映射。
3. **叙述层**：`generate_narration` + prompt + 降级；单测（无效 provider 触发 `None`）。
4. **渲染层**：`render_report_html` 自包含模板（三章节/时间线/类型/占位/样式）。
5. **落盘层**：`write_report_to` 双写 + 通道兼容；扩展单测。
6. **接线层**：`generate_report` 调叙述+HTML；`parse_report_config` 加 `provider_id`；顺带让 `render_report`（MD）也补一行情感/财务摘要（修前期 P0「报告丢弃 sentiment」）。
7. **全量校验**：`cargo test --lib` / `npm run typecheck` / `npm run build:frontend`（无前端改动则仅 cargo）。

## 4. 新增 / 扩展单测（report.rs tests mod）

- `html_escape_neutralizes_script_and_quotes`
- `sanitize_url_rejects_javascript_scheme`
- `render_report_html_has_three_sections_when_narration_present`
- `render_report_html_placeholder_when_narration_none`
- `render_report_html_escapes_evidence_and_source`
- `render_report_html_groups_by_timeline_and_type`
- `render_report_html_finance_event_has_semantic_label`
- `generate_narration_returns_none_on_llm_error`
- `both_channel_writes_html_sidecar`（扩展现有 `both_channel_writes_app_dir_and_vault`）

## 5. 风险与降级

| 风险 | 应对 |
|------|------|
| LLM 不可用/超时/限额 | `narration=None`，HTML 与 MD 仍生成（占位），**不阻断导出**（满足澄清#5） |
| 超长历史（days 大）超上下文 | 事件清单截断到最近 N 条（如 200）并在综述注明「仅含最近 N 条」 |
| XSS 注入 | 全动态字段 `html_escape`；URL `sanitize_url` |
| 深色不可读（前期 P0） | 自包含样式严格用真实 token + `prefers-color-scheme: dark`；禁止幽灵变量/裸 hex |
| 与铁律冲突 | 前端零改动、零 Schema 变更；导出物为独立文件，不走 `appearance.ts` 三步 |

## 6. 验收标准（可测）

1. 生成日报后，Vault/应用目录同时存在 `*.md` 与 `*.html`。
2. 浅色/深色下打开 `.html` 文字均清晰（无幽灵 token、无黑底黑字）。
3. 模型可用时，综述/评论/建议三段均有内容且引用了真实事件（非空白、含实体/事件关键词）。
4. 时间线视图按日期倒序、类型视图按事件类型分组；两者均含「事件摘要 + 原文链接」。
5. 断模型（无效 key）后重新生成，`.html` 仍生成且三段为模板占位，不报错、不阻断。
6. HTML 源码中 evidence/source 含 `<script>`/`javascript:` 时被转义/过滤。
7. finance 事件在类型视图显示中文标签与语义色。
8. `cargo test --lib` 全绿（含上述新增用例）。

## 7. 范围外（本期不做，记入后续）

- 叙述持久化 / 用户编辑层（`ai_summary`/`user_notes` 表）→ 未来版。
- 预警改造（利好/利空标注、来源、`finance` 告警模板）→ 前期评审 P0，属另一条线，另行方案。
- `notify.rs` 双函数合并（前期 P1）→ 不在本方案。
- 关系图/仪表盘补 finance 配色（前期 P0）→ 另行方案。
