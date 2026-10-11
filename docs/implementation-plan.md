# ReadFlow 功能完善与优化实施方案

> 本文档整合两轮分析结论：
> 1. 项目整体功能完整度评估（88 个 IPC 命令、44 个前端组件，处于"可发布级"）。
> 2. 研究工作台（ResearchModal）专项评估——能力底子强，但价值被埋得太深、旗舰产出观感偏糙。
>
> 方案遵循项目既有铁律（详见 `.workbuddy/memory/MEMORY.md`）：前端只 `invoke('cmd', {camelCase})`、HTTP/RSS/AI/SQLite 全在 Rust；Schema 一次成型禁 `ALTER TABLE`；注释中文、标识符英文；改动前先确认方案（本文档即确认件，落地前需逐条获准）。

---

## 0. 背景与范围

ReadFlow 不是普通 RSS 阅读器，而是"本地优先 + AI + 研究流水线"三位一体的信息工具。当前最大问题不是"缺功能"，而是**两件事**：

- **闭环与可达性不足**：研究流水线的深度洞察（情感/财务/交叉验证）只在关系图节点点击后的 210px 窄卡里出现；时间线筛选太弱；报告以 `<pre>` 纯文本渲染；新文到达/重要事件没有主动提醒；新用户不知道主路径。
- **验收缺口**：真机 GUI 从未在真实桌面验证（沙箱仅过编译/构建），液态玻璃、命令面板、快捷键三态、托盘、流式、关系图等均属"编译通过但未验收"。

本方案据此分为 **P0（闭环与可达性）**、**P1（体验增强）**、**P2（探索性）** 三档，共 13 项。

---

## 1. 总体原则与架构边界

- **不破坏定位**：保持本地优先、无账户、无云端同步、不做网页全文抓取（仅用 RSS 自带 content）。
- **优先复用**：评分（`entity_score`）、偏好（`user_preference` like/skip）、自动化（`scheduler`）、实体（`Entity`）、通知（`notify::send` 三道门）、导出（Obsidian/报告）全部已具备，新功能尽量只做"聚合壳"而非"新引擎"。
- **前后端边界**：所有数据访问在 Rust；前端只做渲染/格式化/已加载列表排序过滤/Pinia 状态。新增任何数据查询必须走新 `#[tauri::command]`。
- **Schema 纪律**：不新增业务表时不动 Schema；若确需新表/列，写入 `create_tables` 并在真机备份 `.bak` 后一次性 `ALTER`（沙箱可重置）。

---

## 2. 优先级路线图总表

| 档位 | 编号 | 项目 | 价值 | 投入 | 主要依赖 |
|---|---|---|---|---|---|
| P0 | A | 真机 GUI 验收清单 | 高 | 低（非开发） | 无 |
| P0 | B | 实体仪表盘（聚合页） | 高 | 低 | 现有命令/store 方法 |
| P0 | C | 时间线筛选增强（类型/情感/搜索/排序） | 高 | 低 | `research_events_timeline` |
| P0 | D | 事件预警推送（P0-4） | 高 | 中 | `notify::send`、事件抽取 |
| P0 | E | 新文到达通知 + 首启引导 | 中 | 中 | `notify::send`、Onboarding |
| P1 | A | 智能推荐流（mainView='recommend'） | 高 | 中 | `high_score_articles`、uiStore |
| P1 | B | 报告 Markdown 渲染 | 中 | 中 | 引入轻量 md 库 |
| P1 | C | 阅读洞察看板 | 中 | 低 | `stats_daily_read` |
| P1 | D | 关系图增强（加宽面板/高亮/导出 PNG） | 中 | 中 | `EntityGraphDetail` |
| P1 | E | 播客/视频源（enclosure） | 中 | 中 | `rss.rs` |
| P1 | F | 实体级告警规则 | 中 | 中 | 实体表/调度 |
| P2 | A | 浏览器剪藏扩展 | 中 | 高 | 外部扩展 |
| P2 | B | AI 语音朗读 | 中 | 中 | TTS |
| P2 | C | 静态站分享 / Webhook 外部触发 | 低 | 高 | 导出/网络 |

---

## 3. 详细方案

### P0-A 真机 GUI 验收清单（非开发任务）

**目标**：把"编译通过但未验收"的隐性风险清零。

**做法**：在真实桌面逐项点检并截图留痕——
- 液态玻璃弹窗（`--glass-strong` + `blur`，Teleport to body 防裁剪）
- 命令面板滚动/hover、快捷键三态（Esc 专注 / meta·ctrl·alt 放行 / 输入类元素拦截 / `anyDialogOpen`）
- 托盘、系统通知、流式摘要、关系图渲染、未读后端化

**涉及**：无代码改动，产出一份 `docs/gui-acceptance-checklist.md` 验收表。
**验收**：清单每条标注 通过/问题+截图。

---

### P0-B 实体仪表盘（聚合页）★ 价值最高、优先做

**目标**：把"以实体为中心"的聚合视图抬到前台，让"腾讯最近的舆情+财务+事件"一步可达。

**现状依据**：
- 后端命令已齐备：`entity_sentiment(entity_id)`、`financial_data_list_by_entity(entity_id)`、`research_cross_reference(entity_id, days)`、`entity_articles(entity_id)`、`research_events_timeline(days, entity_id)`。
- `src/stores/research.ts` 已导出 `getEntitySentiment`(402)、`getFinancialDataByEntity`(392)、`getCrossReference`(414)、`getEntityArticles` 对应封装——目前**无任何组件调用**（死方法）。仪表盘可直接复用，无需新增后端。

**前端改动**：
1. 新增 `src/components/EntityDashboardModal.vue`（或并入 ResearchModal 新 Tab「仪表盘」）。
2. 入口打通：时间线分组头、实体列表项、关系图节点均改为可点击 → 传入 `entityId` 打开仪表盘。
3. 并行 `invoke` 上述 5 个命令，`onMounted` 聚合；布局：综合舆情卡（情感+趋势 sparkline）+ 财务快照表 + 近期事件迷你时间线 + 多源交叉验证并列。

**涉及文件**：
- 新增：`src/components/EntityDashboardModal.vue`
- 改：`src/components/ResearchTimelineTab.vue`（分组头加 `@click`）、`ResearchEntitiesTab.vue`、`EntityGraph.vue`/`EntityGraphDetail.vue`（节点点击改跳转仪表盘或并排）
- 复用：`stores/research.ts` 现有死方法（**先验证其返回结构后直接调用**，必要时补全 `getEntityArticles`）

**影响范围**：纯前端聚合，不改后端、不改 Schema。
**验收**：点击任一实体，仪表盘在 1 屏内展示舆情/财务/事件/交叉验证四块，且数据与时间线一致。

---

### P0-C 时间线筛选增强

**目标**：支持"上季度所有监管/利空事件"这类研究高频诉求。

**现状依据**：`research_events_timeline(days, entity_id)` 仅 `days`+`entity_id` 两参数（`lib.rs:2712`）；`ResearchEvent` 已含 `event_type`(String)、`event_date`(String)，`EventWithContext` 已 LEFT JOIN `event_sentiments` 带 `sentiment` 字段（`db.rs:685`）。数据层已具备筛选条件，只需加参数。

**后端改动**（`lib.rs` + `db.rs`）：
- `research_events_timeline` 新增 `event_type: Option<String>`、`sentiment: Option<String>`、`keyword: Option<String>`、`sort: Option<String>`（时间/实体）参数。
- `ResearchEvent::list_with_context` 改为带 WHERE 条件（event_type 等值、sentiment 等值、keyword `LIKE` fact/evidence）；当前在应用层 `retain` 过滤（`lib.rs:2720`），升级为 SQL 层条件拼接更稳。

**前端改动**：`ResearchTimelineTab.vue` 顶部加筛选条（类型下拉/情感下拉/搜索框/排序），参数透传到 `invoke`。

**涉及文件**：`lib.rs`(2712)、`db.rs`(list_with_context ~682)、`ResearchTimelineTab.vue`
**影响范围**：加参数向后兼容（均为 Option），前端筛选条新增。
**验收**：选「情感=利空 + 类型=监管 + 关键词"处罚"」能正确收敛结果。

---

### P0-D 事件预警推送（即暂缓的 P0-4）

**目标**：你关注的实体发生重大事件时主动通知，补回研究闭环。

**现状依据**：事件已抽取入库；`notify::send` 三道门（句柄注入→窗口离开→开关开启，`notify.rs:95`）已就绪，但**只在自动化任务完成时触发**，缺"事件→通知"触发逻辑。

**后端改动**：
- 在 `research_extract_batch`（`lib.rs:2677`）与单篇抽取完成后，对本次新增事件中 `entity_score` 高 或 `sentiment` 为强利空/强利好 的，调用 `notify::send` 推送"【实体名】发生新事件：事实摘要"。
- 新增设置项 `event_alert_enabled`（默认开）走 `notify::enabled` 同款读取；新增可选阈值设置。
- 触发限频：同一实体 N 分钟内不重复推（内存/设置记 last_alert_at）。

**涉及文件**：`lib.rs`(extract 命令)、`notify.rs`、`db.rs`（可选 last_alert 字段，若加需走 Schema 纪律）
**影响范围**：新增通知触发路径；若引入 last_alert 字段需按 Schema 纪律处理。
**验收**：跑一次批量抽取，关注实体出现重大事件后桌面收到系统通知，且重复事件被限频。

---

### P0-E 新文到达通知 + 首启引导

**目标**：补齐"基础 RSS 体验"与降低认知门槛。

**新文到达通知**：
- 在 RSS 全量/单源刷新入库后（`rss.rs` store 流程），对新增文章按"是否命中关注实体"决定是否 `notify::send`（命中实体 → "N 篇新文涉及你关注的 X"；否则可选静默或汇总）。
- 复用 `notify::send` 三道门 + `notify_enabled` 开关。

**首启引导（Onboarding）**：
- 新增 `src/components/OnboardingModal.vue`，首次启动（`Setting` 标记 `onboarded=1`）展示 3 步：加订阅源 → 加关注实体 → 开自动化 → 看研究工作台。
- 每步带"跳转"按钮，串起主路径；完成后写 `onboarded` 设置。

**涉及文件**：`rss.rs`(入库后钩子)、`notify.rs`、`OnboardingModal.vue`（新）、`App.vue`（首启判断）
**影响范围**：新增通知触发 + 一个引导弹窗；onboarded 标记走既有 Setting 表（无需新表）。
**验收**：首次启动弹出引导并可逐步跳转；后台刷新命中实体时桌面收到新文通知。

---

### P1-A 智能推荐流（mainView='recommend'）

**目标**：聚合 `entity_score` + `user_preference(like/skip)` + 阅读历史，给出个性化优先级队列。

**现状依据**：`uiStore.mainView` 当前仅 `'articles' | 'graph'`（`stores/ui.ts:34`，`setMainView` 类型同）；`high_score_articles(days, threshold)` 命令已存在（`lib.rs:2508`）返回按实体分组的高分文章。

**后端改动（可选增强）**：
- 新增 `recommend_articles(days, threshold)`：在 `high_score_articles` 基础上，结合 `user_preference`（like 加权、skip 降权）与已读状态做优先级打分，返回扁平有序列表。若评估前端排序足够，也可直接复用 `high_score_articles` + 前端加权。

**前端改动**：
- `uiStore.mainView` 类型扩展为 `'articles' | 'graph' | 'recommend'`（`stores/ui.ts:34,45`）。
- 新增 `src/components/RecommendView.vue`：消费推荐数据，卡片流展示（实体标签 + 评分 + like/skip 快捷操作回写 `user_preference`）。
- `App.vue` 渲染分支 + `displayArticles` 兜底层覆盖（遵循"非 all 视图须全覆盖"纪律）。
- 主屏/侧栏加切换入口（与研究台 FAB 并列或置于视图切换区）。

**涉及文件**：`lib.rs`(可选新命令)、`db.rs`(可选排序)、`stores/ui.ts`、`App.vue`、`displayArticles` 所在 store、`RecommendView.vue`（新）
**影响范围**：新增一种主视图；影响视图切换与已加载列表兜底逻辑。
**验收**：开启推荐视图，命中关注实体且未 skip 的文章排在前面，like/skip 操作实时影响重排。

---

### P1-B 报告 Markdown 渲染

**目标**：旗舰产出从"草稿"升级为"可读文档"。

**现状依据**：`ResearchReportsTab.vue`（约 323 行）以 `<pre>` 原始文本渲染报告；项目内无任何 Markdown 库（已 grep 确认无 `marked`/`markdown-it`/`katex`）。

**做法**：
- 引入轻量零依赖 md 渲染（如 `markdown-it` 或自渲染子集：标题/列表/粗体/表格/代码块），装于托管 Node 环境（`/home/ivan/.workbuddy/binaries/node/workspace`）。
- 报告读取（`research_report_read`，`lib.rs:3125`）返回 markdown 字符串，前端渲染为 HTML（注意 XSS：仅渲染自生成内容，必要时 DOMPurify）。
- 同样惠及 AI 摘要/日报等已有 markdown 文本。

**涉及文件**：`ResearchReportsTab.vue`、`package.json`（加依赖）、构建配置
**影响范围**：新增前端依赖与渲染层；不改变后端报告生成（已是 markdown 文本）。
**验收**：打开任一报告，标题/表格/粗体/列表正常排版，无原始 `#`/`**` 符号裸露。

---

### P1-C 阅读洞察看板

**目标**：把已就绪的 `stats_daily_read(days)`（`lib.rs:2368`，返回 `Vec<(String,i64)>`）变成可视化。

**做法**：新增 `src/components/InsightsModal.vue` 或并入设置/研究台：按周/月展示阅读量曲线、高分文章占比、实体覆盖数。数据全来自现有命令。

**涉及文件**：`InsightsModal.vue`（新）、`lib.rs`(已有命令)、`stores/`（按需）
**影响范围**：纯前端可视化，无后端改动。
**验收**：打开看板显示阅读趋势图与关键指标卡。

---

### P1-D 关系图增强

**目标**：让关系图从"可看"到"好用"。

**做法**：
- `EntityGraphDetail.vue` 详情卡宽 210px → 加宽至 ~360px（或改为点击节点跳转 P0-B 实体仪表盘，详情卡仅留摘要）；财务列表限高 120px、交叉验证限高 180px 改为可滚动/展开。
- 支持按 `event_type` / `sentiment` 高亮节点与边。
- 新增「导出 PNG」（canvas `toDataURL` + 下载，走 Tauri 文件系统或 `<a download>`）。

**涉及文件**：`EntityGraphDetail.vue`、`EntityGraph.vue`、`useGraphView.ts`
**影响范围**：关系图交互与渲染增强，不改数据层。
**验收**：节点详情可读；按情感高亮生效；可导出当前图为 PNG。

---

### P1-E 播客/视频源（enclosure）

**目标**：从"纯文字 RSS"扩展为"通用 RSS"（播客/视频）。

**现状依据**：`rss.rs` 完全未处理 `enclosure` 字段（已 grep 确认）。

**做法**：
- `db.rs` 文章表加 `enclosure_url` / `enclosure_type` 列（走 Schema 纪律：写入 `create_tables` + 真机 `.bak` 后一次性 ALTER）。
- `rss.rs` 解析时提取 enclosure；列表/详情对 audio/video 显示播放器（HTML5 `<audio>`/`<video>` 或外链）。
- 可选：后台下载管理（P2 再议，避免不可控抓取）。

**涉及文件**：`db.rs`(Schema)、`rss.rs`(解析)、文章列表/详情组件
**影响范围**：新增列 + 解析 + 播放 UI；属中等改动，需备份后 ALTER。
**验收**：添加一播客源，文章列表出现播放控件并可播放。

---

### P1-F 实体级告警规则

**目标**：让用户对特定实体定义"出现 XX 类型/情感事件就提醒"。

**做法**：
- `Entity` 表加 `alert_rules`（JSON 文本列，存事件类型/情感/关键词），走 Schema 纪律。
- 与 P0-D 触发逻辑联动：抽取完成后按实体规则匹配，命中则 `notify::send`。
- 实体管理 Tab（`ResearchEntitiesTab.vue`）提供规则编辑 UI。

**涉及文件**：`db.rs`(Schema)、`lib.rs`(抽取后匹配)、`ResearchEntitiesTab.vue`
**影响范围**：新增实体维度配置 + 通知匹配；中等改动。
**验收**：为"腾讯"设"并购+利空"规则，命中后收到定向通知。

---

### P2 探索性（按需再立项）

- **P2-A 浏览器剪藏扩展**：Chrome/Firefox 扩展 → 经 Tauri 命令 `add_article` 入库（后端需暴露手动添加文章命令）。投入高，依赖外部扩展开发。
- **P2-B AI 语音朗读**：文章正文 TTS（本地或 API），前端播放控件。中等投入。
- **P2-C 静态站分享 / Webhook 外部触发**：把报告/关系图导出为静态页或经 Webhook 推送到外部（如飞书/Slack）。高投入，视反馈再定。

---

## 4. 实施顺序与里程碑

1. **M1（验收 + 两个快赢）**：P0-A 验收清单 → P0-B 实体仪表盘 → P0-C 时间线筛选。三者几乎纯前端/小后端，能最快把"被埋的价值"抬到前台。
2. **M2（通知闭环）**：P0-D 事件预警 + P0-E 新文通知 + 首启引导。补齐 RSS 基础体验与研究闭环。
3. **M3（主视图与观感）**：P1-A 推荐流 → P1-B 报告 Markdown → P1-C 洞察看板。
4. **M4（深度增强）**：P1-D 关系图 → P1-E enclosure → P1-F 实体告警。
5. **M5（探索）**：P2 各项按反馈立项。

> 推荐顺序依据：先补"闭环与可达性"（功能量已足），再堆新功能。

---

## 5. 风险与未决问题

- **真机验收风险**（最高）：当前所有 GUI 仅过编译/构建，P0-B/D/E 的观感与交互需在真实桌面复验，不能在沙箱断言。
- **死方法复用风险**：P0-B 计划复用 `stores/research.ts` 的 5 个死方法，落地前需先验证其返回结构与后端命令一致（`getEntityArticles` 若不存在需补封装）。
- **Schema 改动风险**：P1-E/P1-F 涉及加列，必须备份 `.bak` + 一次性 ALTER，且真机有真实数据。
- **报告渲染 XSS**：P1-B 渲染自生成 markdown 相对安全，但若未来渲染外部内容须加 DOMPurify。
- **范围控制**：本文档为确认件，每一项落地前仍需单独提交"涉及文件+改动点+影响范围"的细化方案并获确认（遵守项目铁律）。

---

_文档由两轮分析（整体完整度 + 研究工作台专项）综合产出，未改动任何代码。_
