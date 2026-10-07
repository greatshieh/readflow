//! LLM 结构化信息提取
//!
//! 职责：读取文章 → 组装提取 prompt → 调用云端 chat API → 容错解析 JSON 输出
//! → 产出 [`ExtractedEvent`] 列表。实体的归一化与落库不在此模块（见 [`super::normalize`]）。
//!
//! prompt 设计要点（与方案定稿一致）：
//! 1. 任务定性为"信息抽取员"而非"分析师"——禁止概括、禁止情绪判断、禁止补全原文没有的事实；
//! 2. event_type 必须从固定枚举中选，缺省维度输出空数组，抑制模型硬编；
//! 3. evidence 强制回填原文片段——反幻觉锚点 + 人工抽检依据；
//! 4. JSON-only 要求 + 代码侧围栏剥离双保险（本地/小参数云端模型常加 ```json 围栏）；
//! 5. few-shot 示例提升小参数模型的 schema 遵从度；
//! 6. temperature 0.1：提取任务确定性优先。

use sqlx::SqlitePool;

use crate::ai;
use crate::db::Article;

use super::ExtractedEvent;

/// 事件类型的合法枚举（与 prompt 中的枚举说明保持一致）
///
/// 模型输出此集合之外的 event_type 时该条事件直接丢弃（记日志），
/// 宁缺毋滥：脏枚举值会污染时间线筛选。
pub const EVENT_TYPES: [&str; 7] = [
    "product",         // 产品动态：发布、更新、下线、定价
    "executive",       // 高管变动：任命、离职、发言表态
    "ma",              // 并购与投资：收购、入股、合资
    "regulatory",      // 监管与合规：处罚、审查、新规影响
    "strategy",        // 战略意图：扩张、收缩、转型、组织调整
    "competition",     // 竞争格局：市场份额、对标、互操作
    "industry_signal", // 行业信号：供需、技术趋势、上下游变化
];

/// 送入模型的最大正文长度（字符数）
///
/// 提取并不需要全文：事件多集中在导语与前几段。截断到 8000 字符
/// 可显著控制 token 成本与延迟，同时覆盖绝大多数文章的全部关键事实。
const MAX_CONTENT_CHARS: usize = 8000;

/// 组装结构化提取 prompt
///
/// 结构：任务定性 → 固定枚举说明 → 输出 schema → few-shot 示例 → 正文。
/// 正文放在最后，让模型先带着明确的输出契约再读内容。
///
/// # 参数
/// * `title` - 文章标题
/// * `content` - 纯文本正文（调用方已做 HTML 剥离与截断）
///
/// # 返回值
/// 完整提示词
fn build_extract_prompt(title: &str, content: &str) -> String {
    format!(
        "你是一名严格的信息抽取员，负责从行业与公司研究类文章中抽取结构化事件。\
你不是分析师：禁止概括、禁止情绪判断、禁止推测，只抽取原文明确陈述的事实。\
每条事件必须同时满足以下要求：

1. event_type 只能从以下枚举中选择：
   product（产品动态）、executive（高管变动）、ma（并购投资）、\
regulatory（监管合规）、strategy（战略意图）、competition（竞争格局）、\
industry_signal（行业信号）
2. fact 是一句客观事实（50字以内），不得改写为评价或概括
3. evidence 必须是原文中连续的片段（原样复制，不得超过60字），用于溯源
4. event_date 为事件发生日期（YYYY-MM-DD）；原文未明确提到日期时留空字符串
5. canonical_name 为实体规范名（用最通用的完整名称）；\
aliases 列出文中出现的其他写法；entity_type 从 company/person/product/industry/other 中选择
6. sentiment 判断事件对市场的影响倾向（positive/negative/neutral），置信度 0-1
7. 若文章涉及财务数据（营收、利润、增长率等），提取 financial_metrics 列表，每项为键值对
8. 文章中没有可抽取的事件时输出 {{\"events\": []}}；某个维度没有事实就不要编造

输出格式（只输出 JSON，不要任何其他文字或代码围栏）：
{{\"events\": [{{\"canonical_name\": \"...\", \"aliases\": [\"...\"], \
\"entity_type\": \"company\", \"event_type\": \"product\", \
\"event_date\": \"2026-09-20\", \"fact\": \"...\", \"evidence\": \"...\", \
\"sentiment\": \"positive\", \"sentiment_confidence\": 0.85, \
\"financial_metrics\": [[\"metric_key\", \"metric_value\"]}}]}}

示例：
文章：OpenAI 宣布以 30 亿美元收购 io Devices，CEO 奥特曼称将进军硬件领域。
输出：{{\"events\": [{{\"canonical_name\": \"OpenAI\", \"aliases\": [], \
\"entity_type\": \"company\", \"event_type\": \"ma\", \"event_date\": \"\", \
\"fact\": \"OpenAI 以30亿美元收购io Devices\", \
\"evidence\": \"OpenAI 宣布以 30 亿美元收购 io Devices\", \
\"sentiment\": \"positive\", \"sentiment_confidence\": 0.9, \
\"financial_metrics\": [[\"acquisition_price\", \"30亿美元\"]}}, \
{{\"canonical_name\": \"OpenAI\", \"aliases\": [], \"entity_type\": \"company\", \
\"event_type\": \"strategy\", \"event_date\": \"\", \
\"fact\": \"OpenAI 将进军硬件领域\", \"evidence\": \"奥特曼称将进军硬件领域\", \
\"sentiment\": \"neutral\", \"sentiment_confidence\": 0.7, \
\"financial_metrics\": []}}]}}

现在处理以下文章。

标题：{title}

正文：
{content}"
    )
}

/// 从模型输出中容错地解析出事件列表
///
/// 解析策略（按序降级）：
/// 1. 直接 `serde_json::from_str`（理想路径：模型严格输出 JSON）；
/// 2. 剥离 ```json ... ``` 围栏后重试；
/// 3. 截取首个 `{` 到末个 `}` 之间的子串重试（应对模型在 JSON 前后加说明文字）。
///
/// 全部失败时返回空列表而不是错误——批量提取场景下单篇解析失败
/// 不应中断整批任务，调用方以"0 条事件"记录该篇即可。
///
/// # 参数
/// * `raw` - 模型原始输出文本
///
/// # 返回值
/// 解析出的事件列表（可能为空）
fn parse_events_loose(raw: &str) -> Vec<ExtractedEvent> {
    // 尝试按三种策略提取 JSON 文本
    let candidates: Vec<String> = {
        let mut v = Vec::new();
        v.push(raw.trim().to_string());
        // 剥离 markdown 代码围栏：```json ... ``` 或 ``` ... ```
        if let Some(start) = raw.find('{') {
            if let Some(end) = raw.rfind('}') {
                if end > start {
                    v.push(raw[start..=end].to_string());
                }
            }
        }
        v
    };

    for candidate in candidates {
        // 先按整体对象解析（{"events": [...]}）；
        // 失败再尝试把整段文本当作裸数组解析（模型偶尔省略外层对象）
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&candidate) {
            let events_value = value
                .get("events")
                .cloned()
                .unwrap_or(value);
            if let Ok(events) = serde_json::from_value::<Vec<ExtractedEvent>>(events_value) {
                return events;
            }
        }
    }
    Vec::new()
}

/// 过滤非法事件：枚举校验 + 空事实剔除
///
/// 证据允许为空（部分模型偶发漏填，事实本身仍有价值），
/// 但 fact 与 event_type 是事件库的底线，不合规直接丢弃。
///
/// # 参数
/// * `events` - 模型输出的事件列表
///
/// # 返回值
/// 校验通过的事件列表
fn validate_events(events: Vec<ExtractedEvent>) -> Vec<ExtractedEvent> {
    events
        .into_iter()
        .filter(|e| {
            // fact 为空说明模型没抽出有效事实，丢弃
            if e.fact.trim().is_empty() {
                return false;
            }
            // event_type 不在枚举内说明模型幻觉或 schema 遵从失败，丢弃并记日志
            if !EVENT_TYPES.contains(&e.event_type.as_str()) {
                eprintln!(
                    "[research] 丢弃非法 event_type 的事件: {} (fact: {})",
                    e.event_type,
                    e.fact.chars().take(30).collect::<String>()
                );
                return false;
            }
            true
        })
        .collect()
}

/// 对单篇文章执行结构化提取（只提取，不落库）
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_id` - 文章 ID
/// * `provider` - AI 提供商配置
/// * `custom_extract_prompt` - 可选的自定义提取 prompt；若为空则使用默认 prompt
///
/// # 返回值
/// 校验通过的事件列表；文章无内容或模型无可抽取事件时为空列表
///
/// # 错误
/// 文章不存在、网络请求失败时返回错误信息（JSON 解析失败不视为错误）
pub async fn extract_article(
    pool: &SqlitePool,
    article_id: i64,
    provider: &ai::AIProvider,
    custom_extract_prompt: Option<&str>,
) -> Result<Vec<ExtractedEvent>, String> {
    let article = sqlx::query_as::<_, Article>("SELECT * FROM articles WHERE id = ? LIMIT 1")
        .bind(article_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("文章不存在: {}", article_id))?;

    // 与摘要链路一致：入库的 content 是 HTML，先剥成纯文本再送模型，
    // 避免 token 浪费与标签干扰；按字符截断控制成本
    let raw = format!("{}\n\n{}", article.summary, article.content);
    let text = ai::html_to_text(&raw);
    let truncated: String = if text.chars().count() > MAX_CONTENT_CHARS {
        text.chars().take(MAX_CONTENT_CHARS).collect()
    } else {
        text
    };

    let prompt = if let Some(custom) = custom_extract_prompt {
        format!("{}\n\n现在处理以下文章。\n\n标题：{}\n\n正文：\n{}", custom, article.title, truncated)
    } else {
        build_extract_prompt(&article.title, &truncated)
    };
    let raw_output = ai::chat_completion(provider, &prompt, Some(0.1)).await?;

    let events = validate_events(parse_events_loose(&raw_output));
    if events.is_empty() {
        eprintln!("[research] 文章 {} 未抽出事件（或解析失败）", article_id);
    }
    Ok(events)
}

/// 标记文章已完成提取尝试
///
/// 记录的是"尝试过"而不是"抽到了事件"：模型明确回答"没有可抽取的事实"（输出空数组）
/// 也是一次有效结论，不该每轮重新付费问一遍。因此**成功拿到回答就置位**，
/// 只有网络/HTTP 层失败（`extract_article` 返回 `Err`）才留空，让下一轮重试。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_id` - 文章 ID
///
/// # 错误
/// 数据库写入失败时返回错误信息
pub async fn mark_extracted(pool: &SqlitePool, article_id: i64) -> Result<(), String> {
    sqlx::query("UPDATE articles SET ai_extracted_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(article_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 批量提取：对命中粗筛阈值且尚未尝试提取的文章逐篇处理
///
/// 粗筛闸门有两条：`entity_score >= min_score`（命中关注实体）与
/// `ai_extracted_at IS NULL`（未尝试过）。后者必须写在 SQL 里而不是取完再在内存里过滤：
/// 若"先按时间取最新 N 篇、再把已提取的剔掉"，最新 N 篇提完之后本任务就会永远
/// 处理 0 篇——更早的未提取文章再也挤不进候选窗口。写进 WHERE 后 LIMIT 才真正
/// 落在"待处理的最新 N 篇"上。
///
/// 单篇失败（网络抖动 / 文章异常 / 归一化入库失败）只记日志继续下一篇，不中断整批。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `provider` - AI 提供商配置
/// * `min_score` - 粗筛阈值（entity_score 下限）
/// * `limit` - 本批次最多处理的文章数
///
/// # 返回值
/// `(处理的文章数, 新增事件数)`
///
/// # 错误
/// 候选筛选查询失败时返回错误信息；单篇提取/入库失败不计为整体错误
pub async fn extract_batch(
    pool: &SqlitePool,
    provider: &ai::AIProvider,
    min_score: i64,
    limit: i64,
) -> Result<(usize, u64), String> {
    // 只取 id：后续 extract_article 会按 id 自行取回整行，这里没必要把正文一并拉进内存
    let candidates = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM articles \
         WHERE entity_score >= ? AND ai_extracted_at IS NULL \
         ORDER BY published_at DESC LIMIT ?",
    )
    .bind(min_score)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut processed = 0usize;
    let mut total_events: u64 = 0;
    for article_id in candidates {
        match extract_article(pool, article_id, provider, None).await {
            Ok(events) => {
                // 入库失败只影响本篇：标记留在未置位状态，下一轮会重试
                match super::normalize::normalize_and_store_events(
                    pool,
                    article_id,
                    &events,
                    &provider.model,
                )
                .await
                {
                    Ok(inserted) => {
                        // 标记放在最后：先落事件再置位，中途失败时不会出现
                        // "标记为已提取但事件一条没进库"的静默丢失
                        if let Err(e) = mark_extracted(pool, article_id).await {
                            eprintln!("[research] 文章 {} 标记提取时间失败: {}", article_id, e);
                        }
                        processed += 1;
                        total_events += inserted;
                    }
                    Err(e) => {
                        eprintln!("[research] 文章 {} 事件入库失败，跳过: {}", article_id, e)
                    }
                }
            }
            // 单篇失败不中断整批：批量任务的价值在推进整体进度
            Err(e) => eprintln!("[research] 文章 {} 提取失败，跳过: {}", article_id, e),
        }
    }
    Ok((processed, total_events))
}
