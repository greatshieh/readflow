//! 数据库模块 - 管理 SQLite 连接与 Feed / Article / Setting 的持久化
//!
//! # 职责
//! 本模块是 ReadFlow 的**唯一数据访问层**，向上（Tauri command 层）屏蔽所有 SQL 细节：
//! - 数据库文件定位与连接池创建（[`init_db`]）
//! - 表结构与索引的声明式建立（[`create_tables`]，幂等，可重复执行）
//! - Feed / Article / Setting 三个实体的模型定义与 CRUD（分别见各 `impl` 块）
//!
//! # 设计意图
//! - **本地优先**：数据存放在操作系统标准数据目录下的 `readflow/readflow.db`，
//!   不依赖任何远端服务；整个应用数据可随该文件迁移。
//! - **幂等初始化**：连接的建立与建表合并为一个入口，任何一处需要数据库时
//!   直接调用 [`init_db`] 即可，无需关心"表是否还没建好"。
//! - **错误扁平化**：所有对外函数统一返回 `Result<_, String>`，把 sqlx 的具体错误
//!   转成可读字符串，方便 Tauri command 直接序列化给前端展示。
//! - **存储约定**：SQLite 无原生布尔类型，布尔字段一律以 `INTEGER 0/1` 存储，
//!   由 `FromRow` 在读取时映射回 Rust 的 `bool`；时间统一以 UTC 存储。

use sqlx::{SqlitePool, FromRow, Row};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use tauri::Manager;

/// 订阅源的 AI 配置（feed-specific 提示词模板）
///
/// 对应 `feed_ai_configs` 表。每个订阅源可独立覆盖全局 AI 摘要/提取的 prompt，
/// 实现"配方系统"：不同行业/风格的源可配专属指令（如"重点抽取财务数据"、
/// "用 bullet points 输出"等）。空串表示不覆盖全局默认，保持行为向后兼容。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FeedAiConfig {
    /// 唯一标识符（自增主键）
    pub id: i64,
    /// 所属订阅源 ID（外键，级联删除）
    pub feed_id: i64,
    /// 摘要自定义指令（空=全局默认）
    pub summary_prompt: String,
    /// 提取自定义指令
    pub extract_prompt: String,
    /// 语言偏好：auto / zh / en
    pub language: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

impl Default for FeedAiConfig {
    fn default() -> Self {
        Self {
            id: 0,
            feed_id: 0,
            summary_prompt: String::new(),
            extract_prompt: String::new(),
            language: "auto".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

/// 订阅源数据模型
///
/// 对应 `feeds` 表。注意本结构体只映射业务常用字段，
/// 表上的 `last_fetch_at` 由刷新流程直接回写。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Feed {
    /// 唯一标识符（自增主键）
    pub id: i64,
    /// 订阅源名称
    pub name: String,
    /// RSS/Atom URL（表级 UNIQUE，是"同一源不重复订阅"的判定依据）
    pub url: String,
    /// 图标 URL（可选）
    pub icon: Option<String>,
    /// 所属文件夹 ID（0 表示未归入任何文件夹，落到侧边栏"未分类"分组）
    pub folder_id: i64,
    /// 未读文章数（冗余字段，由刷新流程统一重算，避免列表页频繁 COUNT）
    pub unread_count: i64,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

/// 文章内容模型
///
/// 对应 `articles` 表。AI 摘要与翻译结果与文章同表存放，
/// 这样前端拉取文章列表时无需二次关联查询即可判断是否已有 AI 产物。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Article {
    /// 唯一标识符（自增主键）
    pub id: i64,
    /// 所属订阅源 ID（外键，级联删除）
    pub feed_id: i64,
    /// GUID（用于去重）
    ///
    /// 与 `feed_id` 组成联合唯一约束，是"刷新时不重复入库"的关键。
    pub guid: String,
    /// 原文链接
    pub link: String,
    /// 文章标题
    pub title: String,
    /// 摘要（由 feed 自带 description 清洗而来）
    pub summary: String,
    /// 正文内容（HTML）
    pub content: String,
    /// 作者（可选）
    pub author: Option<String>,
    /// 发布时间（可选：部分 feed 不提供，此时排序时排在末尾）
    pub published_at: Option<DateTime<Utc>>,
    /// 是否已读
    pub is_read: bool,
    /// 阅读进度：正文滚动位置占「可滚高度」的比例（0.0–1.0）
    ///
    /// 存比例而非像素偏移：窗口尺寸、界面字号、阅读字号改变后像素值会指向错误位置，
    /// 比例仍能落回大致同一段落。0.0 表示未开始或已回到顶部。
    /// 由前端节流上报（见 `set_progress`），空值语义不存在——未读文章就是 0.0。
    pub read_progress: f64,
    /// 是否已收藏
    pub is_bookmarked: bool,
    /// 用户偏好标记：'like' = 喜欢（提升权重），'skip' = 跳过（降低权重），NULL = 未标记
    pub user_preference: Option<String>,
    /// 是否有 AI 摘要（与 `ai_summary` 冗余，供列表页快速判断，避免读大文本）
    pub has_ai_summary: bool,
    /// 是否有翻译
    pub has_ai_translation: bool,
    /// AI 摘要内容（可选）
    pub ai_summary: Option<String>,
    /// 翻译内容（可选）
    pub ai_translation: Option<String>,
    /// 使用的 AI 模型（可选，记录生成时用的模型，便于结果溯源）
    pub ai_model: Option<String>,
    /// AI 智能评分（0-100），由 entities 匹配度与内容质量综合计算
    ///
    /// 高评分（≥70）文章在列表中以红色徽章显示，帮助用户快速定位高价值内容。
    /// 评分为空表示尚未计算（异步后台计算）或实体列表为空。
    pub entity_score: Option<i64>,
    /// 研究提取的**尝试**时间（NULL = 从未提取过）
    ///
    /// 记录的是"尝试过"而不是"抽到了事件"：模型明确回答"没有可抽取的事实"
    /// （输出空数组）也是一次有效结论，不该每轮重新付费问一遍。阅读区的悬浮面板
    /// 依据本字段区分「尚未提取」与「已提取过，但本文没有可抽取的事件」——
    /// 少了它，提取空结果与功能失灵在界面上完全一样。
    pub ai_extracted_at: Option<DateTime<Utc>>,
    /// 文章标签（多对多，见 [`Tag`]）
    ///
    /// **不是 `articles` 表上的列**：由 [`Article::attach_tags`] 在查询之后批量挂载，
    /// 因此 `FromRow` 派生用 `#[sqlx(skip)]` 跳过（按列名取值会得到 `ColumnNotFound`）。
    /// 只做聚合/计数、不渲染列表的查询可以不管它，此时保持空数组，前端按"无标签"处理。
    #[sqlx(skip)]
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

/// 评分用的文章精简行（只含实体匹配所需的列）
///
/// # 为什么单独建一个结构体
/// [`batch_score_entities`] 只需要标题与摘要做实体子串匹配，不需要正文。
/// 此前它复用 [`Article`] 承接 `SELECT id, title, summary`，而 `Article` 有 18 个字段，
/// sqlx 的 `FromRow` 按**列名**逐字段取值，查出来的结果里没有 `feed_id` 等列时
/// 直接返回 `ColumnNotFound`——命令于是长期报错，全库 `entity_score` 始终为 NULL，
/// 批量提取的 `entity_score >= 40` 粗筛永远命中 0 篇（且任务仍记为 success，无人察觉）。
///
/// 用独立结构体承接部分列：既消除该缺陷，也不必为了凑齐字段而把正文整表读进内存。
#[derive(Debug, Clone, FromRow)]
struct ArticleDigest {
    /// 文章 ID（回写评分时使用）
    id: i64,
    /// 标题（参与实体命中统计）
    title: String,
    /// 摘要（参与实体命中统计）
    summary: String,
}

/// 应用设置
///
/// 对应 `settings` 表，采用简单的 key-value 结构（值统一为字符串，
/// 复杂结构由调用方自行序列化为 JSON 后存入）。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Setting {
    /// 设置键（主键，例如 `ai_openai_url`）
    pub key: String,
    /// 设置值（原始字符串或 JSON 文本）
    pub value: String,
}

/// 关注实体（用户关心的公司、人物、产品等）
///
/// 对应 `entities` 表。用于实体感知的高分筛选：文章标题/摘要中出现的
/// 实体越多、匹配越精准，智能评分越高；同时作为研究事件提取的目标实体集，
/// `aliases` 支撑"字节跳动/字节"这类不同写法归并到同一实体。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Entity {
    /// 唯一标识符（自增主键）
    pub id: i64,
    /// 实体名称（如 "腾讯"、"GOOG"、"OpenAI"）
    pub name: String,
    /// 实体类型：company / person / product / industry / other
    pub entity_type: String,
    /// 别称列表（JSON 数组字符串，如 `["字节","ByteDance"]`），用于实体归一化
    pub aliases: String,
    /// 是否启用（禁用后不再参与评分计算）
    pub enabled: bool,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl Entity {
    /// 获取全部已启用的实体列表
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    ///
    /// # 返回值
    /// 已启用的实体列表，按创建时间升序排列
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn list_enabled(pool: &SqlitePool) -> Result<Vec<Entity>, String> {
        sqlx::query_as::<_, Entity>(
            "SELECT id, name, entity_type, aliases, enabled, created_at FROM entities WHERE enabled = 1 ORDER BY id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 获取全部实体列表（含禁用项），供实体归一化做全量内存匹配
    ///
    /// 归一化需要同时考虑禁用实体（历史事件仍应挂到正确实体上），
    /// 因此与 [`Self::list_enabled`]（仅评分用）分开提供。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    ///
    /// # 返回值
    /// 全部实体，按创建时间升序
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Entity>, String> {
        sqlx::query_as::<_, Entity>(
            "SELECT id, name, entity_type, aliases, enabled, created_at FROM entities ORDER BY id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 新增一个实体，同名同类型则忽略（UNIQUE 约束）
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `name` - 实体名称
    /// * `entity_type` - 实体类型
    ///
    /// # 返回值
    /// 新实体的 ID
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn create(pool: &SqlitePool, name: &str, entity_type: &str) -> Result<i64, String> {
        // INSERT OR IGNORE 确保同名实体不会重复插入，返回的行数会被忽略
        sqlx::query(
            "INSERT OR IGNORE INTO entities (name, entity_type) VALUES (?, ?)",
        )
        .bind(name)
        .bind(entity_type)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        // INSERT OR IGNORE 成功后返回的行数可能为 0（重复名），但 id 可通过 SELECT 取回
        let id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM entities WHERE name = ? AND entity_type = ? LIMIT 1",
        )
        .bind(name)
        .bind(entity_type)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// 删除实体
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 实体 ID
    ///
    /// # 错误
    /// 数据库删除失败时返回错误信息
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM entities WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 切换实体启用状态
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 实体 ID
    ///
    /// # 返回值
    /// 切换后的启用状态（true = 已启用）
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn toggle_enabled(pool: &SqlitePool, id: i64) -> Result<bool, String> {
        let current = sqlx::query_scalar::<_, i64>(
            "SELECT enabled FROM entities WHERE id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
        let next = if current == 1 { 0 } else { 1 };
        sqlx::query("UPDATE entities SET enabled = ? WHERE id = ?")
            .bind(next)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(next == 1)
    }

    /// 按名称模糊搜索实体（支持部分匹配）
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `query` - 搜索关键词
    ///
    /// # 返回值
    /// 匹配的实体列表
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn search_by_name(pool: &SqlitePool, query: &str) -> Result<Vec<Entity>, String> {
        let like_pattern = format!("%{}%", query);
        sqlx::query_as::<_, Entity>(
            "SELECT id, name, entity_type, aliases, enabled, created_at FROM entities WHERE name LIKE ? ORDER BY id ASC",
        )
        .bind(&like_pattern)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 构建实体共现图（关系图的唯一数据来源）
    ///
    /// **边的定义**：两个实体只要「同时出现在同一篇文章的标题或摘要里」就连一条边，
    /// 权重 = 共现的文章数。判定口径与 [`batch_score_entities`]、
    /// [`get_high_score_articles_grouped`] **完全一致**（实体 `name` 在
    /// `title + summary` 中的大小写不敏感子串命中），否则会出现「图里的邻居」
    /// 与「高分分组里的邻居」对不上的错位。
    ///
    /// **为什么不读 `research_events`**：事件表是 AI 抽取的产物，只覆盖被显式
    /// 提取过的少量文章（当前全库仅百余条），据此成图会稀疏到没有信息量；
    /// 而共现口径对全库可用、零 AI 依赖、无需任何新表。
    ///
    /// **只在内存里做一次匹配**：每篇文章对「全部实体名」做一次子串扫描，
    /// 复杂度 O(文章数 × 实体数)。当前量级（千余篇 × 数十实体）是毫秒级，
    /// 换来的是零 SQL 往返与零 schema 变更。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `days` - 时间窗（只看最近多少天的文章）；`<= 0` 表示不限时间
    /// * `min_weight` - 最小共现数，低于此值的边被丢弃（`< 1` 时按 1 处理）
    /// * `max_edges` - 边数上限，按权重降序截断；`<= 0` 表示不限制
    ///
    /// # 返回值
    /// [`EntityGraph`]。`nodes` 只含**在图上出现过**的实体（孤立实体不返回，
    /// 否则力导向布局会把可视区域摊薄在空白节点上）；`edges` 按权重降序排列，
    /// 权重相同时按 `source`/`target` 升序，保证同一份数据每次返回顺序稳定。
    ///
    /// # 错误
    /// 实体查询或文章查询失败时返回错误信息
    pub async fn graph(
        pool: &SqlitePool,
        days: i64,
        min_weight: i64,
        max_edges: i64,
    ) -> Result<EntityGraph, String> {
        let entities = Self::list_enabled(pool).await?;
        // 没有关注实体就没有图的顶点，直接返回空图（前端显示引导语）
        if entities.is_empty() {
            return Ok(EntityGraph {
                nodes: Vec::new(),
                edges: Vec::new(),
                scanned_articles: 0,
                truncated: false,
            });
        }

        // 预先降级为小写：若在文章循环里对每个实体各调一次 to_lowercase，
        // 千余篇文章会重复分配数万次字符串，纯属浪费
        let needles: Vec<String> = entities.iter().map(|e| e.name.to_lowercase()).collect();

        // 承接结构体用 ArticleDigest 而非 Article：本查询只取三列，
        // 用 Article 会因缺少 feed_id 等列报 ColumnNotFound（见 ArticleDigest 文档）。
        // COALESCE 兜底：列声明虽是 NOT NULL/有默认值，但历史行仍可能存进 NULL。
        const DIGEST_SQL: &str =
            "SELECT id, COALESCE(title, '') AS title, COALESCE(summary, '') AS summary FROM articles";
        let rows = if days > 0 {
            let cutoff = Utc::now() - chrono::Duration::days(days);
            let sql = format!("{} WHERE published_at >= ?", DIGEST_SQL);
            sqlx::query_as::<_, ArticleDigest>(&sql)
                .bind(cutoff)
                .fetch_all(pool)
                .await
        } else {
            sqlx::query_as::<_, ArticleDigest>(DIGEST_SQL)
                .fetch_all(pool)
                .await
        }
        .map_err(|e| e.to_string())?;

        // 顶点权重（命中文章数）与边权重（共现文章数）一次扫描同时累计
        let mut counts = vec![0i64; entities.len()];
        let mut pairs: HashMap<(usize, usize), i64> = HashMap::new();

        for row in &rows {
            let haystack = format!("{} {}", row.title, row.summary).to_lowercase();
            // 命中下标天然升序且互不重复（由 0..len 顺序筛出），
            // 升序保证配对键 (a, b) 中 a < b，同一对实体不会分裂成两个键
            let hits: Vec<usize> = (0..needles.len())
                .filter(|&i| !needles[i].is_empty() && haystack.contains(&needles[i]))
                .collect();
            for &i in &hits {
                counts[i] += 1;
            }
            for a in 0..hits.len() {
                for b in (a + 1)..hits.len() {
                    *pairs.entry((hits[a], hits[b])).or_insert(0) += 1;
                }
            }
        }

        let floor = min_weight.max(1);
        let mut edges: Vec<GraphEdge> = pairs
            .into_iter()
            .filter(|(_, weight)| *weight >= floor)
            .map(|((a, b), weight)| GraphEdge {
                source: entities[a].id,
                target: entities[b].id,
                weight,
            })
            .collect();
        // 权重降序；同权重按端点升序 —— 稳定顺序让测试可断言，也让前端布局动画不抖
        edges.sort_by(|x, y| {
            y.weight
                .cmp(&x.weight)
                .then(x.source.cmp(&y.source))
                .then(x.target.cmp(&y.target))
        });
        let truncated = max_edges > 0 && edges.len() as i64 > max_edges;
        if truncated {
            edges.truncate(max_edges as usize);
        }

        // 节点集合 = 出现在（截断后）边上的实体。用 HashSet 而不是遍历边去重，
        // 是因为下面还要按 entities 的原始顺序输出，保证节点顺序稳定。
        let mut used: HashSet<i64> = HashSet::new();
        for e in &edges {
            used.insert(e.source);
            used.insert(e.target);
        }
        let nodes: Vec<GraphNode> = entities
            .iter()
            .enumerate()
            .filter(|(_, e)| used.contains(&e.id))
            .map(|(i, e)| GraphNode {
                id: e.id,
                name: e.name.clone(),
                entity_type: e.entity_type.clone(),
                article_count: counts[i],
            })
            .collect();

        Ok(EntityGraph {
            nodes,
            edges,
            scanned_articles: rows.len() as i64,
            truncated,
        })
    }
}

/// 实体关系图的节点
///
/// `article_count` 是**时间窗内**标题或摘要命中该实体名的文章数，
/// 前端据此决定节点半径；与 [`batch_score_entities`] 的命中口径一致。
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    /// 实体 ID
    pub id: i64,
    /// 实体名称（图上的节点标签）
    pub name: String,
    /// 实体类型：company / person / product / industry / other（前端据此着色）
    pub entity_type: String,
    /// 命中文章数（时间窗内）
    pub article_count: i64,
}

/// 实体关系图中的一条边
///
/// 无向边，`source < target` 恒成立（由 [`Entity::graph`] 的升序命中下标保证），
/// 因此前端无需再做一次端点排序即可去重。
#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    /// 端点实体 ID（较小者）
    pub source: i64,
    /// 端点实体 ID（较大者）
    pub target: i64,
    /// 共现文章数（边的粗细）
    pub weight: i64,
}

/// 实体共现图（关系图的完整载荷，一次请求返回，前端不再二次查询）
#[derive(Debug, Clone, Serialize)]
pub struct EntityGraph {
    /// 节点（仅含出现在边上的实体）
    pub nodes: Vec<GraphNode>,
    /// 边（按权重降序）
    pub edges: Vec<GraphEdge>,
    /// 参与扫描的文章数，用于前端提示「已分析 N 篇」
    pub scanned_articles: i64,
    /// 边是否因超过上限而被截断（前端据此提示"仅显示最重要的 N 条关系"）
    pub truncated: bool,
}

/// 研究事件（LLM 从文章中抽取的结构化要素）
///
/// 对应 `research_events` 表。一条事件 = 某篇文章中关于某个实体的一个客观事实，
/// 携带原文片段（evidence）作为溯源与反幻觉锚点——这与"摘要"的本质区别在于：
/// 事件必须可回溯到原文，缺证据的事件宁可不入库。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ResearchEvent {
    /// 唯一标识符（自增主键）
    pub id: i64,
    /// 来源文章 ID（外键，级联删除）
    pub article_id: i64,
    /// 归一化后的实体 ID（外键，级联删除）
    pub entity_id: i64,
    /// 事件类型：product / executive / ma / regulatory / strategy / competition / industry_signal
    pub event_type: String,
    /// 事件发生日期（ISO 8601 字符串；空串表示未知，展示时回退文章发布日）
    pub event_date: String,
    /// 客观事实一句话（非概括、非评价）
    pub fact: String,
    /// 原文片段（证据）
    pub evidence: String,
    /// 抽取所用模型（便于溯源与质量对比）
    pub source_model: String,
    /// 入库时间
    pub created_at: DateTime<Utc>,
}

impl ResearchEvent {
    /// 批量插入事件（幂等去重）
    ///
    /// 依赖 `(article_id, entity_id, event_type, fact)` 唯一索引配合
    /// `INSERT OR IGNORE`：同一篇文章对同一实体抽出的相同事实重复提交时静默跳过，
    /// 使"重复提取"成为安全操作（换模型重跑、手动补跑都不会产生重复数据）。
    /// evidence 不参与唯一键：同一事实的证据补强时应可覆盖更新。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `events` - 待入库的事件列表（entity_id 需已归一化）
    ///
    /// # 返回值
    /// 实际新插入的事件条数
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn insert_batch(
        pool: &SqlitePool,
        events: &[ResearchEvent],
    ) -> Result<u64, String> {
        let mut inserted = 0u64;
        for event in events {
            // INSERT OR IGNORE + 影响行数计数：0 表示命中唯一索引被去重跳过
            let result = sqlx::query(
                "INSERT OR IGNORE INTO research_events \
                 (article_id, entity_id, event_type, event_date, fact, evidence, source_model) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(event.article_id)
            .bind(event.entity_id)
            .bind(&event.event_type)
            .bind(&event.event_date)
            .bind(&event.fact)
            .bind(&event.evidence)
            .bind(&event.source_model)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            inserted += result.rows_affected();
        }
        Ok(inserted)
    }

    /// 删除单条事件（物理删除）
    ///
    /// 供研究工作台的"删除该条事实"入口使用：抽错 / 已过期 / 不关心的事件
    /// 可以由用户手工清掉，避免噪声事件污染时间线与生成的报告。
    ///
    /// 注意这是**物理删除**：唯一索引是 `(article_id, entity_id, event_type, fact)`，
    /// 与"是否被删过"无关，因此若日后对同一篇文章再次运行提取，模型仍可能
    /// 抽出相同事实并重新入库。当前阶段接受该行为（正常流程下已提取过的
    /// 文章不会被反复重跑），暂不引入"忽略名单"这类额外状态。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 事件 ID
    ///
    /// # 返回值
    /// true = 确有该记录且已被删除；false = 事件不存在（已被删除或从未存在）
    ///
    /// # 错误
    /// 数据库删除失败时返回错误信息
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool, String> {
        let result = sqlx::query("DELETE FROM research_events WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(result.rows_affected() > 0)
    }

    /// 查询某篇文章已抽取的全部事件（用于跳过已处理文章与结果展示）
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `article_id` - 文章 ID
    ///
    /// # 返回值
    /// 该文章的事件列表
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn list_by_article(
        pool: &SqlitePool,
        article_id: i64,
    ) -> Result<Vec<ResearchEvent>, String> {
        sqlx::query_as::<_, ResearchEvent>(
            "SELECT * FROM research_events WHERE article_id = ? ORDER BY id ASC",
        )
        .bind(article_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 查询时间窗口内的全部事件并附带实体名与来源文章信息（报告渲染用）
    ///
    /// 一次 JOIN 查询同时取回事件本体 + 实体规范名 + 来源文章标题/链接，
    /// 避免渲染时按实体/按文章逐条回查的 N+1 问题；分组在调用方内存中完成。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `days` - 时间窗口天数（按入库时间过滤，0 表示全部）
    ///
    /// # 返回值
    /// 扁平的事件行列表（含 entity_name / article_title / article_link），
    /// 按实体名、事件日期排序
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn list_with_context(
        pool: &SqlitePool,
        days: i64,
    ) -> Result<Vec<EventWithContext>, String> {
        let sql = if days > 0 {
            "SELECT e.name AS entity_name, \
                    r.id, r.article_id, r.entity_id, r.event_type, r.event_date, \
                    r.fact, r.evidence, r.source_model, r.created_at, \
                    a.title AS article_title, a.link AS article_link, \
                    COALESCE(es.sentiment, 'neutral') AS sentiment, \
                    COALESCE(es.confidence, 0.0) AS sentiment_confidence \
             FROM research_events r \
             JOIN entities e ON e.id = r.entity_id \
             LEFT JOIN articles a ON a.id = r.article_id \
             LEFT JOIN event_sentiments es ON es.event_id = r.id \
             WHERE r.created_at >= datetime('now', '-' || ? || ' days') \
             ORDER BY e.name ASC, r.event_date DESC, r.id DESC"
        } else {
            "SELECT e.name AS entity_name, \
                    r.id, r.article_id, r.entity_id, r.event_type, r.event_date, \
                    r.fact, r.evidence, r.source_model, r.created_at, \
                    a.title AS article_title, a.link AS article_link, \
                    COALESCE(es.sentiment, 'neutral') AS sentiment, \
                    COALESCE(es.confidence, 0.0) AS sentiment_confidence \
             FROM research_events r \
             JOIN entities e ON e.id = r.entity_id \
             LEFT JOIN articles a ON a.id = r.article_id \
             LEFT JOIN event_sentiments es ON es.event_id = r.id \
             ORDER BY e.name ASC, r.event_date DESC, r.id DESC"
        };
        let mut query = sqlx::query_as::<_, EventWithContext>(sql);
        if days > 0 {
            query = query.bind(days);
        }
        query.fetch_all(pool).await.map_err(|e| e.to_string())
    }
}

/// 带上下文的研究事件行（报告渲染用）
///
/// [`ResearchEvent`] 的超集：额外携带实体规范名与来源文章标题/链接，
/// 由 `list_with_context` 的 JOIN 查询产出；不映射 research_events 表本身，
/// 因此只实现 FromRow 而不参与写入。
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EventWithContext {
    /// 实体规范名（JOIN entities 而来）
    pub entity_name: String,
    /// 事件 ID
    pub id: i64,
    /// 来源文章 ID
    pub article_id: i64,
    /// 实体 ID
    pub entity_id: i64,
    /// 事件类型（枚举同 research_events 表）
    pub event_type: String,
    /// 事件发生日期（空串表示未知）
    pub event_date: String,
    /// 客观事实一句话
    pub fact: String,
    /// 原文片段（证据）
    pub evidence: String,
    /// 抽取所用模型
    pub source_model: String,
    /// 入库时间
    pub created_at: DateTime<Utc>,
    /// 来源文章标题（LEFT JOIN，文章可能已被保留策略清理）
    pub article_title: Option<String>,
    /// 来源文章链接（LEFT JOIN）
    pub article_link: Option<String>,
    /// 事件情感倾向（LEFT JOIN event_sentiments，无记录时 COALESCE 为 neutral）
    pub sentiment: String,
    /// 情感置信度（LEFT JOIN event_sentiments，无记录时 COALESCE 为 0.0）
    pub sentiment_confidence: f64,
}

/// 事件情感分析结果
///
/// 对应 `event_sentiments` 表。记录每条研究事件的情感倾向与置信度。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EventSentiment {
    /// 唯一标识符
    pub id: i64,
    /// 关联的事件 ID（外键）
    pub event_id: i64,
    /// 情感倾向：positive / negative / neutral
    pub sentiment: String,
    /// 置信度（0.0-1.0）
    pub confidence: f64,
    /// 入库时间
    pub created_at: DateTime<Utc>,
}

impl EventSentiment {
    /// 按事件 ID 查询情感数据
    pub async fn get_by_event_id(pool: &SqlitePool, event_id: i64) -> Result<Option<Self>, String> {
        sqlx::query_as::<_, Self>(
            "SELECT id, event_id, sentiment, confidence, created_at FROM event_sentiments WHERE event_id = ? LIMIT 1"
        )
        .bind(event_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 批量查询多条事件的情感数据
    pub async fn get_by_event_ids(pool: &SqlitePool, event_ids: &[i64]) -> Result<Vec<Self>, String> {
        if event_ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: Vec<&str> = std::iter::repeat("?").take(event_ids.len()).collect();
        let sql = format!(
            "SELECT id, event_id, sentiment, confidence, created_at FROM event_sentiments WHERE event_id IN ({})",
            placeholders.join(",")
        );
        let mut query = sqlx::query_as::<_, Self>(&sql);
        for id in event_ids {
            query = query.bind(id);
        }
        query.fetch_all(pool).await.map_err(|e| e.to_string())
    }

    /// 写入或更新情感数据（UPSERT）
    pub async fn upsert(pool: &SqlitePool, event_id: i64, sentiment: &str, confidence: f64) -> Result<i64, String> {
        let result = sqlx::query(
            "INSERT INTO event_sentiments (event_id, sentiment, confidence) VALUES (?, ?, ?)
             ON CONFLICT(event_id) DO UPDATE SET sentiment = excluded.sentiment, confidence = excluded.confidence"
        )
        .bind(event_id)
        .bind(sentiment)
        .bind(confidence)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.last_insert_rowid())
    }

    /// 按实体 ID 聚合查询该实体的最新情感状态
    /// 通过 JOIN research_events 表，统计实体关联事件中的最新情感
    pub async fn get_by_entity(pool: &SqlitePool, entity_id: i64) -> Result<Option<Self>, String> {
        sqlx::query_as::<_, Self>(
            r#"SELECT es.id, es.event_id, es.sentiment, es.confidence, es.created_at
               FROM event_sentiments es
               INNER JOIN research_events re ON es.event_id = re.id
               WHERE re.entity_id = ?
               ORDER BY es.created_at DESC
               LIMIT 1"#
        )
        .bind(entity_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
    }
}

/// 财务数据抽取结果
///
/// 对应 `financial_data` 表。记录从文章中抽取的结构化财务指标。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FinancialData {
    /// 唯一标识符
    pub id: i64,
    /// 来源文章 ID
    pub article_id: i64,
    /// 关联实体 ID
    pub entity_id: i64,
    /// 指标键名（如 revenue, profit, growth_rate）
    pub metric_key: String,
    /// 指标值（原始文本）
    pub metric_value: String,
    /// 数值形式（便于排序比较）
    pub metric_numeric: f64,
    /// 币种（如 CNY, USD）
    pub currency: String,
    /// 时期（如 2026Q3）
    pub period: String,
    /// 原文证据片段
    pub evidence: String,
    /// 抽取所用模型
    pub source_model: String,
    /// 入库时间
    pub created_at: DateTime<Utc>,
}

impl FinancialData {
    /// 按文章 ID 查询该文章的所有财务数据
    pub async fn list_by_article(pool: &SqlitePool, article_id: i64) -> Result<Vec<Self>, String> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM financial_data WHERE article_id = ? ORDER BY id ASC"
        )
        .bind(article_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 按实体 ID 查询该实体的所有财务数据
    pub async fn list_by_entity(pool: &SqlitePool, entity_id: i64) -> Result<Vec<Self>, String> {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM financial_data WHERE entity_id = ? ORDER BY created_at DESC"
        )
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 批量写入财务数据（UPSERT）
    ///
    /// 根据 (article_id, entity_id, metric_key) 唯一键去重
    pub async fn upsert_batch(pool: &SqlitePool, data: &[(i64, i64, &str, &str, f64, &str, &str, &str)]) -> Result<usize, String> {
        let mut count = 0usize;
        for &(article_id, entity_id, key, value, numeric, currency, period, evidence) in data {
            let result = sqlx::query(
                "INSERT INTO financial_data (article_id, entity_id, metric_key, metric_value, metric_numeric, currency, period, evidence)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(article_id, entity_id, metric_key) DO UPDATE SET
                 metric_value = excluded.metric_value,
                 metric_numeric = excluded.metric_numeric,
                 currency = excluded.currency,
                 period = excluded.period,
                 evidence = excluded.evidence"
            )
            .bind(article_id)
            .bind(entity_id)
            .bind(key)
            .bind(value)
            .bind(numeric)
            .bind(currency)
            .bind(period)
            .bind(evidence)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            count += result.rows_affected() as usize;
        }
        Ok(count)
    }
}

/// 文章预览结构（供前端展示用，不含正文）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ArticlePreview {
    /// 文章 ID
    pub id: i64,
    /// 文章标题
    pub title: String,
    /// 原文链接
    pub link: String,
}

impl ResearchEvent {
    /// 按实体 ID 查询该实体关联的所有文章预览
    pub async fn list_articles_by_entity(pool: &SqlitePool, entity_id: i64, limit: i64) -> Result<Vec<ArticlePreview>, String> {
        sqlx::query_as::<_, ArticlePreview>(
            r#"SELECT DISTINCT a.id, a.title, a.link
               FROM research_events re
               JOIN articles a ON a.id = re.article_id
               WHERE re.entity_id = ?
               ORDER BY re.created_at DESC
               LIMIT ?"#
        )
        .bind(entity_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 按实体 ID 查询多源交叉验证条目（同一实体在不同订阅源中的报道）
    ///
    /// 一次 JOIN 同时取回事件本体 + 来源文章标题/链接 + 订阅源名称 + 情感，
    /// 供「来源验证」视图并列展示同一事件在不同来源中的报道差异。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `entity_id` - 实体 ID
    /// * `days` - 时间窗口天数（按事件入库时间过滤，0 表示全部）
    /// * `limit` - 返回条数上限
    ///
    /// # 返回值
    /// 按事件入库时间倒序的交叉验证条目列表
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn list_cross_reference(
        pool: &SqlitePool,
        entity_id: i64,
        days: i64,
        limit: i64,
    ) -> Result<Vec<CrossReferenceEntry>, String> {
        let sql = if days > 0 {
            r#"SELECT r.id AS event_id, r.article_id, r.event_type, r.fact, r.event_date,
                      r.created_at,
                      a.title AS article_title, a.link AS article_link,
                      f.name AS source_name,
                      COALESCE(es.sentiment, 'neutral') AS sentiment
               FROM research_events r
               LEFT JOIN articles a ON a.id = r.article_id
               LEFT JOIN feeds f ON f.id = a.feed_id
               LEFT JOIN event_sentiments es ON es.event_id = r.id
               WHERE r.entity_id = ?
                 AND r.created_at >= datetime('now', '-' || ? || ' days')
               ORDER BY r.created_at DESC
               LIMIT ?"#
        } else {
            r#"SELECT r.id AS event_id, r.article_id, r.event_type, r.fact, r.event_date,
                      r.created_at,
                      a.title AS article_title, a.link AS article_link,
                      f.name AS source_name,
                      COALESCE(es.sentiment, 'neutral') AS sentiment
               FROM research_events r
               LEFT JOIN articles a ON a.id = r.article_id
               LEFT JOIN feeds f ON f.id = a.feed_id
               LEFT JOIN event_sentiments es ON es.event_id = r.id
               WHERE r.entity_id = ?
               ORDER BY r.created_at DESC
               LIMIT ?"#
        };
        let mut query = sqlx::query_as::<_, CrossReferenceEntry>(sql);
        query = query.bind(entity_id);
        if days > 0 {
            query = query.bind(days);
        }
        query.bind(limit).fetch_all(pool).await.map_err(|e| e.to_string())
    }
}

/// 多源交叉验证条目（「来源验证」视图用）
///
/// [`ResearchEvent`] 的投影子集：携带来源文章标题/链接、订阅源名称与情感倾向，
/// 由 `list_cross_reference` 的 JOIN 查询产出；不映射任何单表，只实现 FromRow。
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CrossReferenceEntry {
    /// 事件 ID
    pub event_id: i64,
    /// 来源文章 ID
    pub article_id: i64,
    /// 事件类型（枚举同 research_events 表）
    pub event_type: String,
    /// 客观事实一句话
    pub fact: String,
    /// 事件发生日期（空串表示未知）
    pub event_date: String,
    /// 事件入库时间
    pub created_at: DateTime<Utc>,
    /// 来源文章标题（LEFT JOIN，文章可能已被清理）
    pub article_title: Option<String>,
    /// 来源文章链接（LEFT JOIN）
    pub article_link: Option<String>,
    /// 订阅源名称（LEFT JOIN feeds，文章无源时为 NULL）
    pub source_name: Option<String>,
    /// 事件情感倾向（LEFT JOIN event_sentiments，无记录时 COALESCE 为 neutral）
    pub sentiment: String,
}

/// 自动化任务（定时提取 / 定时报告等后台自动化的一份配置记录）
///
/// 对应 `automation_tasks` 表。"任务即数据"：调度器启动时读表注册 cron，
/// 任务的启停与修改即时生效（重新注册），手动触发与 cron 走同一执行器。
/// last_run_at / last_status / last_message 回写最近一次运行结果，
/// 驱动设置页的"任务健康度"展示。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AutomationTask {
    /// 唯一标识符（自增主键）
    pub id: i64,
    /// 任务名称（如"每日研究报告"）
    pub name: String,
    /// 任务类型：batch_extract（批量提取）/ daily_report（日报）/ weekly_report（周报）
    pub task_type: String,
    /// cron 表达式（tokio-cron-scheduler 格式，如 "0 30 2 * * *"）
    pub cron_expr: String,
    /// 任务配置（JSON 字符串，结构随 task_type 不同）
    pub config: String,
    /// 是否启用（停用后不再参与调度，但仍可手动运行）
    pub enabled: bool,
    /// 最近一次运行时间（ISO 8601 字符串；空串表示从未运行）
    pub last_run_at: String,
    /// 最近一次运行状态：success / failed / unconfigured；空串表示从未运行
    pub last_status: String,
    /// 最近一次运行的结果消息或"缺什么配置"的提示
    pub last_message: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl AutomationTask {
    /// 获取全部任务列表
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    ///
    /// # 返回值
    /// 全部任务（按创建时间升序）
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<AutomationTask>, String> {
        sqlx::query_as::<_, AutomationTask>(
            "SELECT * FROM automation_tasks ORDER BY id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 按 ID 查找任务
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 任务 ID
    ///
    /// # 返回值
    /// 命中的任务；不存在返回 `None`
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn find_by_id(
        pool: &SqlitePool,
        id: i64,
    ) -> Result<Option<AutomationTask>, String> {
        sqlx::query_as::<_, AutomationTask>(
            "SELECT * FROM automation_tasks WHERE id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 新建任务
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `name` - 任务名称
    /// * `task_type` - 任务类型
    /// * `cron_expr` - cron 表达式
    /// * `config` - JSON 配置
    /// * `enabled` - 是否启用
    ///
    /// # 返回值
    /// 新任务的 ID
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn create(
        pool: &SqlitePool,
        name: &str,
        task_type: &str,
        cron_expr: &str,
        config: &str,
        enabled: bool,
    ) -> Result<i64, String> {
        let result = sqlx::query(
            "INSERT INTO automation_tasks (name, task_type, cron_expr, config, enabled) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(task_type)
        .bind(cron_expr)
        .bind(config)
        .bind(enabled as i64)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.last_insert_rowid())
    }

    /// 更新任务的可编辑字段
    ///
    /// 只覆盖传入 `Some` 的字段（`None` 表示保持原值），与前端"部分保存"交互对齐；
    /// 修改后由调用方负责触发调度器 reload。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 任务 ID
    /// * `name` / `cron_expr` / `config` - 可选的新值
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        name: Option<&str>,
        cron_expr: Option<&str>,
        config: Option<&str>,
    ) -> Result<(), String> {
        // 动态拼 SET 子句：只更新调用方明确传入的字段
        let mut sets: Vec<&str> = Vec::new();
        if name.is_some() {
            sets.push("name = ?");
        }
        if cron_expr.is_some() {
            sets.push("cron_expr = ?");
        }
        if config.is_some() {
            sets.push("config = ?");
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = format!("UPDATE automation_tasks SET {} WHERE id = ?", sets.join(", "));
        let mut query = sqlx::query(&sql);
        if let Some(v) = name {
            query = query.bind(v);
        }
        if let Some(v) = cron_expr {
            query = query.bind(v);
        }
        if let Some(v) = config {
            query = query.bind(v);
        }
        query
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 删除任务
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 任务 ID
    ///
    /// # 错误
    /// 数据库删除失败时返回错误信息
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM automation_tasks WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 启用 / 停用任务
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 任务 ID
    /// * `enabled` - 目标状态
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn set_enabled(
        pool: &SqlitePool,
        id: i64,
        enabled: bool,
    ) -> Result<(), String> {
        sqlx::query("UPDATE automation_tasks SET enabled = ? WHERE id = ?")
            .bind(enabled as i64)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 回写任务最近一次运行结果
    ///
    /// cron 与手动触发共用：无论哪条路径执行完，都把时间 / 状态 / 消息
    /// 写回任务行，保证设置页看到的是"最近一次运行"的统一视图。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 任务 ID
    /// * `status` - success / failed / unconfigured
    /// * `message` - 人类可读的结果描述
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn mark_run_result(
        pool: &SqlitePool,
        id: i64,
        status: &str,
        message: &str,
    ) -> Result<(), String> {
        // 时间戳由 SQLite 侧生成，与应用内其他 CURRENT_TIMESTAMP 字段保持同一时钟
        sqlx::query(
            "UPDATE automation_tasks \
             SET last_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                 last_status = ?, last_message = ? WHERE id = ?",
        )
        .bind(status)
        .bind(message)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// 全库数据快照（数据管理页统计展示用）
///
/// 一次性汇总订阅源数、文章总数以及已读 / 未读 / 收藏分布，
/// 避免前端多次请求拼接；所有字段均为计数，缺失即按 0 处理。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    /// 订阅源总数
    pub feeds: i64,
    /// 文章总数
    pub articles: i64,
    /// 已读文章数
    pub read: i64,
    /// 未读文章数
    pub unread: i64,
    /// 收藏文章数
    pub bookmarks: i64,
    /// 今日已读文章数（按 created_at 日期过滤）
    pub read_today: i64,
}

// ─── 数据库连接 ────────────────────────────────────────────────────────────────

/// 获取数据库文件路径
///
/// 使用 Tauri 的路径解析器获取用户数据目录，确保在 minijail 沙箱环境下能正确访问。
/// 目录不存在时自动创建，然后返回 `readflow/readflow.db` 的绝对路径。
///
/// # 参数
/// * `app` - Tauri 应用句柄，用于解析数据目录
///
/// # 返回值
/// 数据库文件的绝对路径字符串
///
/// # Panics
/// 系统数据目录不可解析、或目录创建失败时 panic —— 这两种情况属于环境异常，
/// 应用无法在无持久化能力的状态下继续运行，故直接失败而非向上传递错误。
/// 获取数据库文件路径
///
/// 通过 Tauri 的路径解析器获取用户数据目录，确保在 minijail 沙箱环境下能正确访问。
/// 目录不存在时自动创建，然后返回 `cn.readflow.app/readflow.db` 的绝对路径。
///
/// # 参数
/// * `app` - Tauri 应用句柄，用于解析数据目录
///
/// # 返回值
/// 数据库文件的绝对路径字符串
///
/// # Panics
/// 系统数据目录不可解析、或目录创建失败时 panic —— 这两种情况属于环境异常，
/// 应用无法在无持久化能力的状态下继续运行，故直接失败而非向上传递错误。
pub fn get_data_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::path::BaseDirectory;
    // 通过 Tauri 路径解析器获取数据目录，而非 dirs crate，以兼容沙箱环境
    // 使用 BaseDirectory::Data 让 Tauri 自动使用应用标识符（cn.readflow.app）作为目录名
    let data_dir = app
        .path()
        .resolve("cn.readflow.app", BaseDirectory::Data)
        .expect("无法解析数据目录");

    // 首次运行时目录尚不存在，需要主动创建，否则 SQLite 打开文件会失败
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir).expect("无法创建数据目录");
    }

    data_dir
}

pub fn get_db_path(app: &tauri::AppHandle) -> String {
    get_data_dir(app).join("readflow.db").to_string_lossy().to_string()
}

/// 暴露数据库文件的绝对路径（供备份 / 恢复命令读取与写回同一文件）
///
/// 直接复用 [`get_db_path`] 的定位逻辑，避免在命令层重复拼接目录。
///
/// # 参数
/// * `app` - Tauri 应用句柄
///
/// # 返回值
/// 数据库文件（`readflow.db`）的绝对路径字符串
pub fn db_file_path(app: &tauri::AppHandle) -> String {
    get_db_path(app)
}

/// 初始化数据库连接池
///
/// 建立连接池并确保表结构就绪，再种子化默认设置（仅首次运行写入）。
/// 本函数是幂等的，可安全地在任意时刻重复调用。
///
/// # 参数
/// * `app` - Tauri 应用句柄，用于解析用户数据目录
///
/// # 返回值
/// 可用的 [`SqlitePool`] 连接池
///
/// # 错误
/// 连接串非法、文件无法打开或建表失败时返回 sqlx 错误
///
/// # 设计意图
/// 采用"先建文件再连接"的策略：
/// 1. 先检查数据库文件是否存在
/// 2. 若不存在，先创建空文件（确保目录权限正确）
/// 3. 再建立连接池并执行建表逻辑
pub async fn init_db(app: &tauri::AppHandle) -> Result<SqlitePool, sqlx::Error> {
    let db_path = get_db_path(app);
    
    // 数据库文件不存在时，先创建空文件
    // 这样做可以提前暴露目录权限问题，避免后续 connect() 报 "unable to open database"
    if !std::path::Path::new(&db_path).exists() {
        std::fs::File::create(&db_path)
            .map_err(|e| sqlx::Error::Io(e.into()))?;
    }
    
    let database_url = format!("sqlite://{}", db_path);

    // 建立连接池
    let pool = SqlitePool::connect(&database_url).await?;
    
    // 创建表结构（IF NOT EXISTS 保证幂等）
    create_tables(&pool).await?;
    // 追加新表（不修改已有表结构）
    if let Err(e) = ensure_feed_ai_configs_table(&pool).await {
        eprintln!("创建 feed_ai_configs 表失败（不影响启动）: {}", e);
    }
    if let Err(e) = ensure_event_sentiments_table(&pool).await {
        eprintln!("创建 event_sentiments 表失败（不影响启动）: {}", e);
    }
    if let Err(e) = ensure_financial_data_table(&pool).await {
        eprintln!("创建 financial_data 表失败（不影响启动）: {}", e);
    }
    
    // 种子化默认设置：失败仅告警不阻断启动（与"局部失败容忍"一致）
    if let Err(e) = seed_default_settings(&pool).await {
        eprintln!("种子化默认设置失败（不影响启动）: {}", e);
    }

    // 种子化默认自动化任务：失败仅告警不阻断启动
    if let Err(e) = seed_default_tasks(&pool).await {
        eprintln!("种子化默认自动化任务失败（不影响启动）: {}", e);
    }

    eprintln!("数据库初始化成功: {}", db_path);

    Ok(pool)
}

/// 种子化默认设置（仅首次运行写入，绝不覆盖用户已有配置）
///
/// 在 `create_tables` 之后调用一次：写入关键设置项的默认值（AI 端点与模型、
/// 功能开关、刷新频率、字体、主题、保留策略等），使应用开箱即可用。
/// 已存在同名 key 时不写入，避免覆盖用户后续修改。
/// **AI 密钥不在种子之列**：它属于用户私有凭据，留空由用户自行填写。
///
/// 采用 `INSERT ... SELECT ... WHERE NOT EXISTS` 的写法而非先查后插，
/// 把"判断是否存在"与"插入"合并为单条原子语句，避免并发下的竞态。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 错误
/// 任何一条默认配置的写入失败时返回 sqlx 错误（由调用方决定告警或忽略）
async fn seed_default_settings(pool: &SqlitePool) -> Result<(), String> {
    // key -> 默认值：
    // - ai_agnes_url 存的是 Base URL（不含 /chat/completions，由 ai.rs 自动补全）；
    // - ai_agnes_key **刻意留空**：密钥属于用户私有凭据，不能随代码分发，
    //   由用户在「设置 → AI」里自行填写。
    // - ai_agnes_model 为默认模型名；
    // - 两个开关默认开启 AI 摘要与翻译。
    let defaults: &[(&str, &str)] = &[
        ("ai_agnes_url", "https://apihub.agnes-ai.com/v1"),
        ("ai_agnes_key", ""),
        ("ai_agnes_model", "agnes-2.5-flash"),
        ("ai_summary_enabled", "1"),
        ("ai_translation_enabled", "1"),
        // 后台刷新频率（分钟）：默认每小时；用户在"设置 → 常规"可调整，下次启动生效
        ("refresh_interval_minutes", "60"),
        // 阅读字体大小档位：small / medium / large / xlarge，默认 medium
        ("font_size", "medium"),
        // 文章保留策略：保留天数（0 表示永久保留，不自动清理）；默认 30 天
        ("article_retention_days", "30"),
        // 保留未读：为 1 时即使超过保留天数也不清理"未读"文章（避免误删还没看的内容）
        ("retention_keep_unread", "1"),
        // 界面主题：light / dark / system（跟随系统），默认浅色
        ("theme", "light"),
        // 界面字体大小档位：small / medium / large / xlarge，默认 medium（缩放整个 UI）
        ("ui_font_size", "medium"),
        // 界面字体族：system（跟随系统默认栈）/ 任意系统字体族名，默认 system
        ("ui_font", "system"),
        // 正文阅读字体族：system（跟随界面字体）/ sans（无衬线）/ serif（衬线），
        // 也可存任意系统字体族名（由「设置 → 常规 → 内容字体」的系统字体列表写入）
        ("content_font", "system"),
        // RSSHub 实例地址：添加订阅时输入 RSSHub 路由（如 /twitter/user/xxx）会自动拼接此前缀
        ("rsshub_base_url", "https://rsshub.app"),
    ];

    for (key, value) in defaults {
        sqlx::query(
            "INSERT INTO settings (key, value) \
             SELECT ?1, ?2 WHERE NOT EXISTS (SELECT 1 FROM settings WHERE key = ?3)",
        )
        .bind(*key)
        .bind(*value)
        .bind(*key)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// 种子化默认自动化任务（仅当任务表为空时写入，绝不覆盖用户已有任务）
///
/// 种子化三个默认任务：
/// - 智能提取（启用）：夜间批量提取，执行器 Phase A 已就绪；
/// - 每日报告（启用，channel=app）：开箱即用，无需任何额外配置；
/// - 周报（默认停用，channel=obsidian）：需用户先配置 Vault 路径再启用。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 错误
/// 数据库写入失败时返回错误信息
async fn seed_default_tasks(pool: &SqlitePool) -> Result<(), String> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM automation_tasks")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(());
    }

    // 批量提取任务：每天 02:30 夜间跑一次，
    // config 含 provider（AI 提供商标识）、min_score（粗筛阈值）、batch_limit（单次最多处理篇数）
    let extract_config = serde_json::json!({
        "provider": "agnes",
        "min_score": 40,
        "batch_limit": 50
    });
    sqlx::query(
        "INSERT INTO automation_tasks (name, task_type, cron_expr, config, enabled) \
         VALUES (?, ?, ?, ?, 1)",
    )
    .bind("智能提取")
    .bind("batch_extract")
    .bind("0 30 2 * * *")
    .bind(extract_config.to_string())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 每日报告：每天 08:00 生成，channel=app 落到应用数据目录，
    // 不依赖任何外部配置，开箱即用
    let daily_config = serde_json::json!({
        "channel": "app",
        "days": 1
    });
    sqlx::query(
        "INSERT INTO automation_tasks (name, task_type, cron_expr, config, enabled) \
         VALUES (?, ?, ?, ?, 1)",
    )
    .bind("每日研究报告")
    .bind("daily_report")
    .bind("0 0 8 * * *")
    .bind(daily_config.to_string())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 周报：每周一 08:00 生成，channel=obsidian 需要先配置 Vault 路径，
    // 因此默认停用，待用户在设置页完善配置后手动启用。
    // 不写 vault_subpath：落点跟随「设置 → Obsidian」的导出目录，
    // 使报告与文章笔记收在同一个 Vault 文件夹里
    let weekly_config = serde_json::json!({
        "channel": "obsidian",
        "days": 7
    });
    sqlx::query(
        "INSERT INTO automation_tasks (name, task_type, cron_expr, config, enabled) \
         VALUES (?, ?, ?, ?, 0)",
    )
    .bind("每周研究报告")
    .bind("weekly_report")
    .bind("0 0 8 * * 1")
    .bind(weekly_config.to_string())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// 创建数据库表结构与索引
///
/// 全部表按**最终形态**用 `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`
/// 一次性定义，可在每次启动时安全重复执行。项目未发布，schema 演进采取
/// "改定义 + 重置开发库"的方式，不做运行时补列/补丁式迁移。
///
/// 可见性放宽到 `pub(crate)`：其他模块的测试需要建出**真实 schema** 的内存库
/// （手抄表结构会在 schema 演进时静默漂移）。
pub(crate) async fn create_tables(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // 订阅源表：url 唯一约束保证同一个源不会被重复添加
    // last_fetch_at 记录最近一次抓取时刻，供刷新流程回写
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS feeds (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            url TEXT NOT NULL UNIQUE,
            icon TEXT DEFAULT '',
            folder_id INTEGER DEFAULT 0,
            last_fetch_at DATETIME,
            unread_count INTEGER DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 文章表：UNIQUE(feed_id, guid) 是去重的唯一依据，
    // 配合批量插入时的 INSERT OR IGNORE 实现"重复刷新不产生重复文章"
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS articles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            feed_id INTEGER NOT NULL,
            guid TEXT NOT NULL,
            link TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT DEFAULT '',
            content TEXT DEFAULT '',
            author TEXT DEFAULT '',
            published_at DATETIME,
            is_read INTEGER DEFAULT 0,
            read_progress REAL NOT NULL DEFAULT 0,
            is_bookmarked INTEGER DEFAULT 0,
            entity_score INTEGER DEFAULT NULL,
            /** 用户偏好标记：'like' = 喜欢（提升权重），'skip' = 跳过（降低权重），NULL = 未标记 */
            user_preference TEXT DEFAULT NULL,
            has_ai_summary INTEGER DEFAULT 0,
            has_ai_translation INTEGER DEFAULT 0,
            ai_summary TEXT,
            ai_translation TEXT,
            ai_model TEXT DEFAULT '',
            ai_extracted_at DATETIME DEFAULT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE,
            UNIQUE(feed_id, guid)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 索引服务于列表栏的高频查询：
    // - 按源分页 + 时间倒序：需要 (feed_id, published_at DESC) 复合索引，单列索引在
    //   `WHERE feed_id = ? ORDER BY published_at DESC` 下只能命中一半条件；
    // - 全库时间线：published_at 显式建为 DESC 索引，排序可直接走索引；
    // - 视图筛选与排序：is_read（未读筛选）、is_bookmarked（书签筛选）、entity_score（重要排序）；
    // - 研究提取的候选筛选：ai_extracted_at IS NULL（见 research::extract）。
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_articles_feed_id ON articles(feed_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_articles_is_read ON articles(is_read)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_articles_published_at ON articles(published_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_articles_feed_published \
         ON articles(feed_id, published_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_articles_is_bookmarked ON articles(is_bookmarked)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_articles_entity_score ON articles(entity_score)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_articles_ai_extracted_at ON articles(ai_extracted_at)",
    )
    .execute(pool)
    .await?;

    // 设置表：key 即主键，写入时用 UPSERT 保证"存在即更新"
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 文件夹表：用于把订阅源分组（对齐 Folo 的 SubscriptionColumn 分组）。
    // position 决定侧边栏内的显示顺序；订阅源与文件夹是**唯一**的分组关系
    // （feeds.folder_id = 0 表示"未分类"组）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS folders (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            position INTEGER DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 过滤规则表：实现"规则引擎"（对抗信息过载的核心，对齐 Inoreader/FreshRSS）。
    // scope: 作用范围 all(全库) / feed(指定源) / folder(指定文件夹)
    // field: 匹配字段 title / content / author / feed(源名)
    // op:    匹配方式 contains / not_contains / equals / not_equals
    // action:命中后的动作 mark_read(自动已读) / star(收藏) / hide(隐藏=标记已读)
    //                                   / tag(自动打标签)
    // action_arg: 动作参数——仅 action='tag' 时使用，存标签名（不存在则自动创建）。
    //             其余动作为空串。存**名字**而非 tag_id，是为了让规则可引用尚未创建的标签，
    //             且规则行自身不因标签被删而失效。代价是标签改名时必须同步本列，
    //             由 `Tag::update` / `Tag::delete` 负责（见其实现），否则规则会
    //             悄悄重新创建一个同名标签。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS filters (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            enabled INTEGER DEFAULT 1,
            scope TEXT DEFAULT 'all',
            scope_id INTEGER DEFAULT 0,
            field TEXT NOT NULL DEFAULT 'title',
            op TEXT NOT NULL DEFAULT 'contains',
            value TEXT NOT NULL DEFAULT '',
            action TEXT NOT NULL DEFAULT 'mark_read',
            action_arg TEXT NOT NULL DEFAULT '',
            priority INTEGER DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 高亮/批注表：正文内高亮片段与笔记，用于知识沉淀（对齐 Readwise 高亮→笔记流）。
    // article_id 外键级联删除：文章被清理时其高亮一并消失。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS highlights (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            article_id INTEGER NOT NULL,
            text TEXT NOT NULL,
            note TEXT DEFAULT '',
            color TEXT DEFAULT 'yellow',
            start_offset INTEGER DEFAULT 0,
            end_offset INTEGER DEFAULT 0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 标签表：用户自定义的自由标签（对齐 Inoreader/Folo 的 tag 维度）。
    // 与「关注实体」（entities）刻意分开：实体是给机器用的（参与 entity_score 评分、
    // 作为研究事件提取的目标），标签是给人用的整理维度（手工打标或规则自动打标），
    // 二者语义不同，不可互相替代——所以宁可多一张表也不复用 entities。
    //
    // name 声明 COLLATE NOCASE：SQLite 的 NOCASE 只折叠 ASCII 大小写，足以让
    // "AI" / "ai" 归到同一标签；中文标签本无大小写，不受影响。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE,
            color TEXT NOT NULL DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 文章-标签关联表：多对多。复合主键 (article_id, tag_id) 天然完成去重，
    // 且主键隐含索引以 article_id 为前导，正好覆盖「查某篇文章的标签」这一高频方向。
    //
    // 两个外键都是级联删除：文章被保留策略清理、或标签被删除时关联行自动消失，
    // 不留悬挂引用（依赖 sqlx 的 SQLite 连接默认开启 PRAGMA foreign_keys = ON，
    // 见 sqlx-sqlite options 的默认 pragma 表）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS article_tags (
            article_id INTEGER NOT NULL,
            tag_id INTEGER NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (article_id, tag_id),
            FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE,
            FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 反向索引：服务「查某标签下的全部文章」（列表栏按标签浏览）。
    // 复合主键只覆盖以 article_id 为前导的方向，反向查询（tag_id 在前）必须另建索引。
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_article_tags_tag ON article_tags(tag_id, article_id)",
    )
    .execute(pool)
    .await?;

    // 关注实体表：记录用户关心的公司、人物、产品、行业等实体。
    // 用于文章智能评分（entity_score）和研究事件提取（research_events）。
    // 实体名称大小写不敏感匹配（通过 LOWER() 索引优化查询）。
    // aliases 存 JSON 数组形式的别称（如 ["字节", "ByteDance"]），
    // 供实体归一化使用：模型提取出的不同写法可归并到同一 canonical 实体。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS entities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            entity_type TEXT NOT NULL DEFAULT 'company',
            aliases TEXT NOT NULL DEFAULT '[]',
            enabled INTEGER DEFAULT 1,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(name, entity_type)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 实体名称索引：加速匹配查询（LOWER(name) = ?）
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(LOWER(name))",
    )
    .execute(pool)
    .await?;

    // 研究事件表：LLM 从文章中抽取的结构化要素（公司动态/战略意图/竞争格局/行业信号），
    // 是"事件库"的核心。与 articles / entities 均为级联删除外键：
    // 文章被保留策略清理或实体被删除时，其事件一并消失，避免悬挂引用。
    //
    // fact 存客观事实一句话（非概括），evidence 存原文片段作为溯源与反幻觉锚点；
    // 二者配合唯一索引 (article_id, entity_id, event_type, fact) 实现
    // INSERT OR IGNORE 语义的幂等入库——同一篇文章对同一实体抽出的相同事实不会重复存储。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS research_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            article_id INTEGER NOT NULL,
            entity_id INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            event_date TEXT DEFAULT '',
            fact TEXT NOT NULL,
            evidence TEXT NOT NULL DEFAULT '',
            source_model TEXT DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE,
            FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 事件查询四条主路径的索引：
    // 1. 按实体查时间线（时间倒序） 2. 按事件类型筛选 3. 反查某篇文章已抽取的事件（配合去重）
    // 4. 按时间倒序取最近事件（工作台时间线与日报都按 created_at 排序，event_type 基数极低，单列索引无收益）
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_events_entity ON research_events(entity_id, event_date)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_type ON research_events(event_type)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_article ON research_events(article_id)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_events_created_at ON research_events(created_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_events_dedupe \
         ON research_events(article_id, entity_id, event_type, fact)",
    )
    .execute(pool)
    .await?;

    // 事件情感分析表：存储每条事件的正面/负面/中性情感及其置信度
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS event_sentiments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id INTEGER NOT NULL,
            sentiment TEXT NOT NULL DEFAULT 'neutral',
            confidence REAL DEFAULT 0.0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (event_id) REFERENCES research_events(id) ON DELETE CASCADE,
            UNIQUE(event_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 自动化任务表：任务即数据——定时提取、定时报告等后台自动化都以行记录存在这里，
    // 由 scheduler 侧的执行器按 task_type 分发，新增自动化能力只需加 task_type 而非改框架。
    // config 存 JSON（按 task_type 结构不同）；last_* 三列回写最近一次运行的结果，
    // 驱动设置页的"任务健康度"展示（跑没跑、成没成、缺什么配置）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS automation_tasks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            task_type TEXT NOT NULL,
            cron_expr TEXT NOT NULL,
            config TEXT NOT NULL DEFAULT '{}',
            enabled INTEGER DEFAULT 1,
            last_run_at TEXT DEFAULT '',
            last_status TEXT DEFAULT '',
            last_message TEXT DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 全文检索虚拟表（FTS5）：镜像 articles 的 title/content/summary，支撑"搜正文"需求。
    // 由下方三个触发器在 articles 增/删/改时自动维护，无需业务代码手动同步。
    //
    // ⚠️ 必须声明 `content='articles', content_rowid='id'`（content-linked 外部内容表）：
    //    不声明时 FTS5 会把列值自行存一份副本，而 'delete' 指令需要按"原始 token 序列"
    //    精确回滚，纯 FTS5 表在 trigram 分词器下会因拿不到原始值而抛 "SQL logic error"，
    //    导致**任何** UPDATE articles（含 is_read / is_bookmarked）全部失败。
    //    声明后 'delete' 会直接从 articles 表按 rowid 读取旧值，问题消失。
    sqlx::query(
        "CREATE VIRTUAL TABLE IF NOT EXISTS articles_fts USING fts5(\
         title, content, summary, content='articles', content_rowid='id', tokenize='trigram')",
    )
    .execute(pool)
    .await?;

    // FTS5 同步触发器：让 articles 的写入镜像到 articles_fts，保持索引新鲜。
    // 删除分支传 'delete' 哨兵行以从索引中移除；更新分支先删后插。
    //
    // ⚠️ 更新分支必须限定 `UPDATE OF title, content, summary`，不能写成裸的 `UPDATE ON articles`：
    //    articles_fts 是 `content='articles'` 的外部内容表且用 trigram 分词，重建一行的成本随
    //    正文长度增长；而 articles 上大量写入与正文无关（is_read / is_bookmarked / 阅读进度
    //    read_progress 都是高频小字段），裸触发器会让每次滚动、每次标记已读都白跑一遍全文
    //    重分词。限定列后只有真正改动被索引列的语句才重建索引。
    //    注意 `CREATE TRIGGER IF NOT EXISTS` 对老库不生效——已存在旧触发器的库需先
    //    `DROP TRIGGER articles_fts_au` 再重跑建表语句（见本次的一次性迁移脚本）。
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS articles_fts_ai AFTER INSERT ON articles BEGIN \
         INSERT INTO articles_fts(rowid, title, content, summary) \
         VALUES (new.id, new.title, new.content, new.summary); END;",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS articles_fts_ad AFTER DELETE ON articles BEGIN \
         INSERT INTO articles_fts(articles_fts, rowid, title, content, summary) \
         VALUES ('delete', old.id, old.title, old.content, old.summary); END;",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TRIGGER IF NOT EXISTS articles_fts_au AFTER UPDATE OF title, content, summary ON articles BEGIN \
         INSERT INTO articles_fts(articles_fts, rowid, title, content, summary) \
         VALUES ('delete', old.id, old.title, old.content, old.summary); \
         INSERT INTO articles_fts(rowid, title, content, summary) \
         VALUES (new.id, new.title, new.content, new.summary); END;",
    )
    .execute(pool)
    .await?;

    // 订阅源 AI 配置表：允许对单个订阅源覆盖全局 AI 摘要/提取 prompt，
    // 实现"配方系统"（feed-specific AI template）。每源一行，UNIQUE(feed_id)
    // 保证一对一；删除源时 CASCADE 清除配置。
    //
    // 前端在 FeedManageModal 的「AI 配置」tab 中维护本表；
    // 后端 ai_generate_summary / research_extract_article 在调用 AI 前优先
    // 读取本表的 prompt 字段，若为空则回退到全局默认 behavior。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS feed_ai_configs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            feed_id INTEGER NOT NULL UNIQUE,
            summary_prompt TEXT DEFAULT '',      -- 摘要自定义指令（空=全局默认）
            extract_prompt TEXT DEFAULT '',      -- 提取自定义指令
            language TEXT DEFAULT 'auto',        -- auto/zh/en
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 全部表结构与索引已在上方按最终形态一次性定义完毕（CREATE ... IF NOT EXISTS）。
    // 设计约定：项目未发布，schema 演进采取"改定义 + 重置开发库"的方式，
    // 不引入运行时补列（ALTER TABLE）等补丁式迁移，保证建表语句即完整真相。

    Ok(())
}

// ─── Feed AI 配置相关表（在 main 初始化时调用）────────────────────────────────

/// 动态创建 feed_ai_configs 表（若不存在）
///
/// 用于支持订阅源级别的 AI 提示词配置（配方系统）。
/// 注意：此函数与 create_tables 是分开的，因为它是追加功能。
pub async fn ensure_feed_ai_configs_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS feed_ai_configs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            feed_id INTEGER NOT NULL UNIQUE,
            summary_prompt TEXT DEFAULT '',
            extract_prompt TEXT DEFAULT '',
            language TEXT DEFAULT 'auto',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())
    .map(|_| ())
}

/// 动态创建 event_sentiments 表（若不存在）
///
/// 用于存储事件情感分析结果（正/负/中性）。
pub async fn ensure_event_sentiments_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS event_sentiments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id INTEGER NOT NULL,
            sentiment TEXT NOT NULL DEFAULT 'neutral',
            confidence REAL DEFAULT 0.0,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (event_id) REFERENCES research_events(id) ON DELETE CASCADE,
            UNIQUE(event_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())
    .map(|_| ())
}

/// 动态创建 financial_data 表（若不存在）
///
/// 用于存储从文章中抽取的结构化财务数据。
pub async fn ensure_financial_data_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS financial_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            article_id INTEGER NOT NULL,
            entity_id INTEGER NOT NULL,
            metric_key TEXT NOT NULL,
            metric_value TEXT NOT NULL,
            metric_numeric REAL DEFAULT 0.0,
            currency TEXT DEFAULT '',
            period TEXT DEFAULT '',
            evidence TEXT NOT NULL DEFAULT '',
            source_model TEXT DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE,
            FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE,
            UNIQUE(article_id, entity_id, metric_key)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())
    .map(|_| ())
}

// ─── Feed CRUD ────────────────────────────────────────────────────────────────

impl Feed {
    /// 添加新的订阅源
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `name` - 订阅源名称
    /// * `url` - RSS/Atom URL
    /// * `icon` - 图标 URL（可选）
    /// * `folder_id` - 所属文件夹 ID（0 表示未归入任何文件夹）
    ///
    /// # 返回值
    /// 新订阅源的 ID
    ///
    /// # 错误
    /// 数据库写入失败，或 `url` 与已有订阅源重复（违反 UNIQUE 约束）时返回错误信息
    pub async fn add(
        pool: &SqlitePool,
        name: &str,
        url: &str,
        icon: Option<&str>,
        folder_id: i64,
    ) -> Result<i64, String> {
        // 未显式写入的列依赖表定义的 DEFAULT 值
        let result = sqlx::query(
            "INSERT INTO feeds (name, url, icon, folder_id) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(url)
        .bind(icon)
        .bind(folder_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        // SQLite 的 last_insert_rowid 即本次自增主键，用于回传前端定位新源
        Ok(result.last_insert_rowid())
    }

    /// 获取所有订阅源列表
    ///
    /// 按名称排序返回，保证前端侧边栏顺序稳定。
    ///
    /// # 返回值
    /// 全部订阅源（不含分页）
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Feed>, String> {
        // 只 SELECT 结构体映射需要的列，避免把刷新流程字段一并取出
        sqlx::query_as::<_, Feed>(
            "SELECT id, name, url, icon, folder_id, unread_count, created_at, updated_at \
             FROM feeds ORDER BY name",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 删除订阅源
    ///
    /// 同时会删除该订阅源下的所有文章（ON DELETE CASCADE）
    ///
    /// # 注意
    /// 级联删除依赖 SQLite 的外键约束开启（sqlx 的 SQLite 连接默认执行
    /// `PRAGMA foreign_keys = ON`），若该 PRAGMA 被关闭，文章会变成孤儿数据。
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM feeds WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 更新订阅源的链接 / 名称 / 归属文件夹
    ///
    /// `url` 传 `None` 表示保持原链接不变（`COALESCE(?, url)` 自赋值）。改链接是"这个订阅
    /// 指向别处"的操作：已有文章按 `UNIQUE(feed_id, guid)` 保留，新地址的文章按 guid 去重追加，
    /// 因此不会产生重复条目，也不会丢书签（重抓策略见 [`crate::spawn_feed_backfill`]）。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 订阅源 ID
    /// * `url` - 新的订阅源链接（`None` 表示不改）
    /// * `name` - 新的订阅源名称（重命名）
    /// * `folder_id` - 新的所属文件夹 ID（0 表示移出文件夹、归入"未分类"）
    ///
    /// # 错误
    /// - 链接与其它订阅源重复（表级 UNIQUE 约束）时返回可读中文提示，供前端内联展示
    /// - 其余数据库写入失败时返回错误信息
    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        url: Option<&str>,
        name: &str,
        folder_id: i64,
    ) -> Result<(), String> {
        let result = sqlx::query(
            "UPDATE feeds SET url = COALESCE(?, url), name = ?, \
             folder_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(url)
        .bind(name)
        .bind(folder_id)
        .bind(id)
        .execute(pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            // 唯一约束冲突：SQLite 的英文报错对用户毫无意义，换成能指导下一步动作的提示。
            // 注意 SQLite 把 UNIQUE 违规报成 "UNIQUE constraint failed: feeds.url"。
            Err(e) if e.to_string().contains("UNIQUE constraint failed: feeds.url") => {
                Err("该链接已被其它订阅源占用，请检查是否重复订阅".to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// 更新订阅源图标 URL
    ///
    /// 供刷新流程回写：channel 自带图标优先，缺省时为站点 favicon
    /// （见 [`crate::rss::fallback_icon`]）。调用方应先比对现有值、
    /// 仅在拿到更优图标时调用，避免每轮刷新都产生无谓的 UPDATE。
    ///
    /// # 参数
    /// * `id` - 订阅源 ID
    /// * `icon` - 图标 URL
    ///
    /// # 错误
    /// 数据库更新失败时返回错误信息
    pub async fn update_icon(pool: &SqlitePool, id: i64, icon: &str) -> Result<(), String> {
        sqlx::query("UPDATE feeds SET icon = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(icon)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 批量导入订阅源（OPML 导入用）
    ///
    /// 采用 `INSERT OR IGNORE`：当 `url` 命中表上的 UNIQUE 约束（已订阅过）时 silently 跳过，
    /// 因此重复导入同一份 OPML 不会产生重复源、也不会报错。
    /// 名称缺省时由调用方已回退为 URL，这里直接写入。导入的源一律落在
    /// "未分类"组（`folder_id = 0`），后续可在 UI 中移动或由自动分组整理
    /// （OPML 的文件夹层级不强制映射）。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `items` - 待导入的 `(名称, URL)` 列表
    ///
    /// # 返回值
    /// 新插入订阅源的 ID 列表（被去重忽略的已存在项不计入）
    ///
    /// # 错误
    /// 任一条目写入失败即整体返回错误（已写入的条目不会回滚）
    pub async fn import_batch(
        pool: &SqlitePool,
        items: &[(String, String)],
    ) -> Result<Vec<i64>, String> {
        let mut ids = Vec::with_capacity(items.len());

        for (name, url) in items {
            let result = sqlx::query(
                "INSERT OR IGNORE INTO feeds (name, url) VALUES (?, ?)",
            )
            .bind(name)
            .bind(url)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;

            // rows_affected == 0 表示该 URL 已存在、被唯一约束忽略；
            // 只有真正插入的行才把自增主键回传前端，便于提示"新增 N 个"
            if result.rows_affected() > 0 {
                ids.push(result.last_insert_rowid());
            }
        }

        Ok(ids)
    }

    /// 获取订阅源的 AI 配置（若不存在则返回空默认值）
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `feed_id` - 订阅源 ID
    ///
    /// # 返回值
    /// 配置对象（字段为空串表示使用全局默认）；数据库查询失败时返回错误
    pub async fn get_ai_config(pool: &SqlitePool, feed_id: i64) -> Result<FeedAiConfig, String> {
        match sqlx::query_as::<_, FeedAiConfig>(
            "SELECT id, feed_id, COALESCE(summary_prompt, '') as summary_prompt, \
             COALESCE(extract_prompt, '') as extract_prompt, \
             COALESCE(language, 'auto') as language, \
             created_at, updated_at \
             FROM feed_ai_configs WHERE feed_id = ?",
        )
        .bind(feed_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        {
            Some(cfg) => Ok(cfg),
            None => Ok(FeedAiConfig {
                id: 0,
                feed_id,
                summary_prompt: String::new(),
                extract_prompt: String::new(),
                language: "auto".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }),
        }
    }

    /// 保存或更新订阅源的 AI 配置（UPSERT）
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `feed_id` - 订阅源 ID
    /// * `config` - 要保存的配置对象（summary_prompt / extract_prompt / language 均支持空串）
    ///
    /// # 返回值
    /// 配置行 ID（新建或已有则返回原 ID）
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn save_ai_config(
        pool: &SqlitePool,
        feed_id: i64,
        config: &FeedAiConfig,
    ) -> Result<i64, String> {
        // UPSERT：有则更新，无则插入
        // 注意：summary_prompt / extract_prompt 允许存空串（表示"不覆盖全局默认"），
        // 因此不能用 COALESCE 回退——必须显式写库
        let result = sqlx::query(
            r#"INSERT INTO feed_ai_configs (feed_id, summary_prompt, extract_prompt, language, updated_at)
               VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)
               ON CONFLICT(feed_id) DO UPDATE SET
                   summary_prompt = excluded.summary_prompt,
                   extract_prompt = excluded.extract_prompt,
                   language = excluded.language,
                   updated_at = CURRENT_TIMESTAMP"#,
        )
        .bind(feed_id)
        .bind(&config.summary_prompt)
        .bind(&config.extract_prompt)
        .bind(&config.language)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(result.last_insert_rowid())
    }

    /// 删除订阅源的 AI 配置（恢复到全局默认）
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `feed_id` - 订阅源 ID
    ///
    /// # 错误
    /// 数据库删除失败时返回错误信息（行不存在视为静默成功）
    pub async fn delete_ai_config(pool: &SqlitePool, feed_id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM feed_ai_configs WHERE feed_id = ?")
            .bind(feed_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 获取全部订阅源的 AI 配置列表
    ///
    /// # 返回值
    /// 配置列表，按 feed_id 升序排列
    pub async fn list_ai_configs(pool: &SqlitePool) -> Result<Vec<FeedAiConfig>, String> {
        sqlx::query_as::<_, FeedAiConfig>(
            "SELECT id, feed_id, COALESCE(summary_prompt, '') as summary_prompt, \
             COALESCE(extract_prompt, '') as extract_prompt, \
             COALESCE(language, 'auto') as language, \
             created_at, updated_at \
             FROM feed_ai_configs ORDER BY feed_id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 更新未读文章计数
    ///
    /// 直接写入给定数值（覆盖式），调用方需保证数值来源正确；
    /// 刷新流程一般用"重算子查询"的方式整体回写。
    pub async fn update_unread_count(pool: &SqlitePool, id: i64, count: i64) -> Result<(), String> {
        // 顺带刷新 updated_at，使"最近有活动的源"可被识别
        sqlx::query(
            "UPDATE feeds SET unread_count = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(count)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 按 ID 查询订阅源名称
    ///
    /// 供导出（Obsidian / 分享）等需要「文章 → 来源名」映射的场景使用，
    /// 查不到（源已被删）时返回 `None`，由调用方回退为「未知来源」。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 订阅源 ID
    ///
    /// # 返回值
    /// 订阅源名称（存在时）；不存在时 `None`（不视为错误）
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn name_by_id(pool: &SqlitePool, id: i64) -> Result<Option<String>, String> {
        let row = sqlx::query("SELECT name FROM feeds WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map(|r| r.get::<String, _>("name")))
    }

    /// 按 ID 查询单个订阅源（含 url / icon / unread_count 等全部字段）
    ///
    /// 供"刷新单个订阅源"等需要先用 ID 取回 url 的场景使用；
    /// 相比 `list_all` 全量拉取，这里只取一行，开销更小。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 订阅源 ID
    ///
    /// # 返回值
    /// 订阅源对象；不存在时返回 `None`（不视为错误，调用方据此回退为"源不存在"提示）
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Feed>, String> {
        // SELECT 明确列而非 *：feeds 表还有 last_fetch_at 等未映射进 Feed 的列，
        // 使用 * 会导致 sqlx 按名称匹配失败。
        sqlx::query_as::<_, Feed>(
            "SELECT id, name, url, icon, folder_id, unread_count, created_at, updated_at \
             FROM feeds WHERE id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 获取订阅源所属的文件夹 ID（供规则引擎按 folder 作用范围裁剪）
    ///
    /// # 返回值
    /// 文件夹 ID；查询失败时回退为 0（视为未分类），不中断入库流程
    pub async fn folder_id_of(pool: &SqlitePool, id: i64) -> Result<i64, String> {
        let row = sqlx::query("SELECT folder_id FROM feeds WHERE id = ? LIMIT 1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
        match row {
            Some(r) => Ok(r.get::<i64, _>("folder_id")),
            None => Ok(0),
        }
    }
}

// ─── Article CRUD ─────────────────────────────────────────────────────────────

/// 文章列表的排序方式（对应列表栏顶部的"排序"切换）
///
/// 之所以用枚举而非直接接收 ORDER BY 字符串：排序字段最终要拼进 SQL，
/// 枚举把可选的 SQL 片段限制在编译期常量集合内，前端传来的任意字符串
/// 无法影响语句结构（注入防护不依赖转义，而是根本拼不进去）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArticleSort {
    /// 最新优先（默认，按发布时间倒序）
    Latest,
    /// 重要优先（按实体评分倒序，同分再按时间）
    Score,
    /// 名称优先（按标题字典序）
    Title,
}

impl ArticleSort {
    /// 把前端传来的排序标识解析成枚举
    ///
    /// # 参数
    /// * `key` - 前端传来的排序标识（`latest` / `score` / `title`）
    ///
    /// # 返回值
    /// 对应的排序方式；无法识别时回退 [`ArticleSort::Latest`]
    /// （列表宁可保持默认顺序，也不因一个脏参数而报错）
    pub fn from_key(key: &str) -> Self {
        match key.trim().to_ascii_lowercase().as_str() {
            "score" => ArticleSort::Score,
            "title" => ArticleSort::Title,
            _ => ArticleSort::Latest,
        }
    }

    /// 该排序方式对应的 ORDER BY 子句
    ///
    /// # 返回值
    /// 编译期常量形式的 SQL 排序片段
    ///
    /// # 注意
    /// SQLite 在 `DESC` 排序中把 NULL 排在最末，故缺少 `published_at` 或
    /// `entity_score` 的条目会自然沉到列表底部（评分未计算的旧文章不会抢占前排）。
    fn order_by(self) -> &'static str {
        match self {
            ArticleSort::Latest => "published_at DESC",
            ArticleSort::Score => "entity_score DESC, published_at DESC",
            ArticleSort::Title => "title ASC",
        }
    }
}

impl Article {
    /// 列出文章（支持分页、排序，以及按订阅源 / 标签 / 书签 / 未读筛选）
    ///
    /// `feed_id` 为 `Some(id)` 时只返回该订阅源的文章；为 `None` 时返回**全部订阅源**的文章
    ///（统一时间线，仿 Folo 的 "Articles" 视图）。`tag_id` 为 `Some(id)` 时只返回带该标签的
    /// 文章。`bookmarked` 为 `Some(true)` 时只返回已收藏的文章，`unread` 为 `Some(true)` 时
    /// 只返回未读文章——四个条件可任意组合，例如"某源下的书签"或"某标签下的未读"。
    ///
    /// 书签与未读两个范围之所以必须由后端过滤而非前端筛已加载页：前端每页只拿 50 条，
    /// 收藏 / 未读若散落在上千条历史里，用户要反复「加载更多」才能逐步露出，
    /// 且 `hasMore` 无法正确反映剩余量。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `feed_id` - 订阅源 ID；`None` 表示不过滤（全部源）
    /// * `tag_id` - 标签 ID；`None` 表示不过滤（全部标签）
    /// * `bookmarked` - `Some(true)` 只返回已收藏文章；`Some(false)` / `None` 均不过滤
    ///   （`false` 刻意**不**翻译成 `is_bookmarked = 0`：调用方只需表达"要么收藏要么不限"，
    ///   把"只要没收藏的"也塞进来会让 `None`/`false` 的语义分叉得毫无必要）
    /// * `unread` - `Some(true)` 只返回未读文章；`Some(false)` / `None` 均不过滤
    ///   （同 `bookmarked`，`false` 不翻译成 `is_read = 1`）
    /// * `sort` - 排序方式（见 [`ArticleSort`]）
    /// * `limit` - 每页条数
    /// * `offset` - 偏移量（分页用）
    ///
    /// # 返回值
    /// 文章列表，按 `sort` 指定的顺序排列；每篇的 `tags` 已批量挂载
    ///
    /// # 注意
    /// SQLite 在 `DESC` 排序中把 NULL 排在最末，因此缺少 `published_at` 的条目会沉到列表底部。
    pub async fn list_by_feed(
        pool: &SqlitePool,
        feed_id: Option<i64>,
        tag_id: Option<i64>,
        bookmarked: Option<bool>,
        unread: Option<bool>,
        sort: ArticleSort,
        limit: i64,
        offset: i64,
        user_preference: Option<&str>,
    ) -> Result<Vec<Article>, String> {
        // ORDER BY 片段来自 ArticleSort 的常量表，拼进 SQL 不构成注入口；
        // WHERE 片段同理——只拼列名与占位符，任何用户输入都走 bind
        let order = sort.order_by();
        let mut wheres: Vec<&str> = Vec::new();
        if feed_id.is_some() {
            wheres.push("feed_id = ?");
        }
        if tag_id.is_some() {
            // 子查询而非 JOIN：JOIN 会因 article_tags 里同一文章的多行标签而放大结果集，
            // 需要额外 DISTINCT，反而更贵。此处只需存在性判断，子查询最直白。
            wheres.push("id IN (SELECT article_id FROM article_tags WHERE tag_id = ?)");
        }
        if bookmarked == Some(true) {
            // 常量条件，不含占位符，因此不参与下面的 bind 序列
            wheres.push("is_bookmarked = 1");
        }
        if unread == Some(true) {
            // 同书签：常量条件，不含占位符
            wheres.push("is_read = 0");
        }
        if let Some(pref) = user_preference {
            // 按用户偏好过滤：'like' = 只显示喜欢的，'skip' = 只显示跳过的（与 like 对称，
            // 便于检查与纠正错标的文章；「日常阅读中隐藏跳过」是默认行为之外的需求，
            // 由前端在渲染层处理，不进 SQL）
            match pref {
                "like" => wheres.push("user_preference = 'like'"),
                "skip" => wheres.push("user_preference = 'skip'"),
                _ => {}
            }
        }
        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", wheres.join(" AND "))
        };
        let sql = format!(
            "SELECT * FROM articles{} ORDER BY {} LIMIT ? OFFSET ?",
            where_sql, order
        );

        let mut q = sqlx::query_as::<_, Article>(&sql);
        // 参数绑定顺序必须与 SQL 中占位符出现顺序一致：
        // 先按 feed_id / tag_id 的拼接顺序绑，再统一绑 limit / offset。
        // 上面的书签与未读都是常量条件、无占位符，故不在此列——漏掉它会整体错位一位。
        if let Some(id) = feed_id {
            q = q.bind(id);
        }
        if let Some(id) = tag_id {
            q = q.bind(id);
        }
        let mut list = q
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;

        Self::attach_tags(pool, &mut list).await?;
        Ok(list)
    }

    /// 给一批文章批量挂载标签
    ///
    /// 凡是返回 `Vec<Article>` 的查询都应在返回前调用它，把"标签挂载"这件事
    /// 收口到一处，避免各查询点各写一遍 N+1 查询。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `articles` - 待挂载的文章切片（原地修改 `tags` 字段）
    ///
    /// # 错误
    /// 标签查询失败时返回错误信息
    async fn attach_tags(pool: &SqlitePool, articles: &mut [Article]) -> Result<(), String> {
        if articles.is_empty() {
            return Ok(());
        }
        let ids: Vec<i64> = articles.iter().map(|a| a.id).collect();
        let mut map = Tag::for_articles(pool, &ids).await?;
        for a in articles.iter_mut() {
            if let Some(tags) = map.remove(&a.id) {
                a.tags = tags;
            }
        }
        Ok(())
    }

    /// 统计未读文章数（可按订阅源 / 标签限定范围）
    ///
    /// 供列表栏顶部的「全部已读」按钮使用：按钮的可用性与提示语都需要一个**准确**的
    /// 未读数。单源视图能直接读 `feeds.unread_count`、全部视图把它求和即可，但**标签视图
    /// 没有预存的数字**（标签横跨所有源，未读数不落在任何一张表上），只能回后端数一次——
    /// 否则计数退化成"已加载那几页里的未读"，与按钮实际会影响的文章数不符。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `feed_id` - 订阅源 ID；`None` 表示不限源
    /// * `tag_id` - 标签 ID；`None` 表示不限标签
    ///
    /// # 返回值
    /// 满足范围的未读文章条数
    pub async fn count_unread(
        pool: &SqlitePool,
        feed_id: Option<i64>,
        tag_id: Option<i64>,
    ) -> Result<i64, String> {
        // 动态拼 WHERE：恒带 is_read = 0，其余条件与 [`Article::list_by_feed`] 同款写法
        let mut wheres: Vec<&str> = vec!["is_read = 0"];
        if feed_id.is_some() {
            wheres.push("feed_id = ?");
        }
        if tag_id.is_some() {
            wheres.push("id IN (SELECT article_id FROM article_tags WHERE tag_id = ?)");
        }
        let sql = format!(
            "SELECT COUNT(*) as cnt FROM articles WHERE {}",
            wheres.join(" AND ")
        );
        let mut q = sqlx::query(&sql);
        // 绑定顺序必须与拼接顺序一致
        if let Some(id) = feed_id {
            q = q.bind(id);
        }
        if let Some(id) = tag_id {
            q = q.bind(id);
        }
        // COUNT 结果一定存在一行，故用 fetch_one 而非 fetch_optional
        let row = q.fetch_one(pool).await.map_err(|e| e.to_string())?;
        Ok(row.get("cnt"))
    }

    /// 根据 ID 获取单篇文章
    ///
    /// # 返回值
    /// 存在时返回 `Some(article)`，不存在时返回 `None`（**不视为错误**，
    /// 便于 command 层直接把"没找到"映射为前端的空态）
    pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Article>, String> {
        let mut article = sqlx::query_as::<_, Article>(
            "SELECT * FROM articles WHERE id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
        // 详情页需要展示并编辑标签，因此这里必须挂载（列表查询同理）
        if let Some(a) = article.as_mut() {
            Self::attach_tags(pool, std::slice::from_mut(a)).await?;
        }
        Ok(article)
    }

    /// 标记文章为已读
    ///
    /// # 注意
    /// 这里只改文章自身状态，不会同步 `feeds.unread_count`；
    /// 未读计数依赖刷新流程末尾的批量重算来保持一致。
    pub async fn mark_as_read(pool: &SqlitePool, id: i64) -> Result<(), String> {
        // 复用 set_read(true)，顺带同步所属订阅源的未读数，
        // 使"打开文章即已读"后侧边栏未读角标立即递减，无需等下次刷新
        Article::set_read(pool, id, true).await
    }

    /// 精确设置文章的已读 / 未读状态
    ///
    /// 与 `mark_as_read`（只能置已读）不同，本方法可双向切换：
    /// 正文工具条的"标记为已读/未读"按钮就依赖它把已读文章重新置回未读。
    /// 关键点：写入 `is_read` 后**立即重算所属订阅源的 `unread_count`**，
    /// 因为未读数来自 `articles` 表的实时聚合，仅靠单篇 UPDATE 不会自动反映到 `feeds` 表，
    /// 不重算则侧边栏数字会与正文状态长期不一致。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 文章 ID
    /// * `is_read` - true 标记为已读，false 标记为未读
    ///
    /// # 错误
    /// 数据库写入失败（含所属源未读数重算失败）时返回错误信息
    pub async fn set_read(pool: &SqlitePool, id: i64, is_read: bool) -> Result<(), String> {
        // 先写文章自身状态：bool 绑定在 SQLite 下写成 0/1
        sqlx::query("UPDATE articles SET is_read = ? WHERE id = ?")
            .bind(is_read as i32)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;

        // 再同步所属订阅源的未读数：用相关子查询重算该源下"未读文章数"，
        // 只更新当前文章所属的那一个源（WHERE id = 当前文章所在源），避免无谓的全表更新
        sqlx::query(
            "UPDATE feeds SET unread_count = (
                SELECT COUNT(*) FROM articles
                WHERE articles.feed_id = feeds.id AND articles.is_read = 0
            ) WHERE id = (SELECT feed_id FROM articles WHERE id = ?)",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// 记录文章的阅读进度（正文滚动比例）
    ///
    /// 由前端在正文滚动时节流调用。**进度不属于关键路径**：写失败不影响阅读，
    /// 因此调用方应容忍错误、不打断用户（前端为此静默失败）。
    ///
    /// 两处写入抑制：
    /// 1. 入参 clamp 到 `0.0..=1.0`，且非有限值（NaN / ±inf）一律归 0——它们参与
    ///    `ABS(...) > 0.01` 比较时恒为 false，会让抑制逻辑失效并把脏值写进库，之后前端
    ///    按"比例 × 可滚高度"恢复就会落到不可预期的位置；
    /// 2. SQL 侧 `ABS(read_progress - ?) > 0.01`——滚动是连续事件，只有变化超过 1%
    ///    才真的落盘，顺带让 0/1 的边界抖动不再反复改库。
    ///
    /// **刻意不更新 `updated_at`**：该字段语义是"内容变更时间"，与 `set_read` 等
    /// 阅读状态写入保持一致；把滚动也算作内容更新会让它失去意义。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 文章 ID
    /// * `progress` - 滚动比例，0.0 = 顶部，1.0 = 底部
    ///
    /// # 返回值
    /// 无返回值；被抑制的写入同样返回 `Ok(())`（"无需更新"不是错误）
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn set_progress(pool: &SqlitePool, id: i64, progress: f64) -> Result<(), String> {
        let p = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };

        sqlx::query(
            "UPDATE articles SET read_progress = ? \
             WHERE id = ? AND ABS(read_progress - ?) > 0.01",
        )
        .bind(p)
        .bind(id)
        .bind(p)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// 批量标记文章为已读
    ///
    /// 用于文章列表顶部的"全部已读"操作：把当前范围内的未读文章一次性置为已读。
    /// `feed_id` 与 `tag_id` 都是可选范围限定，可单独使用也可叠加
    ///（"某标签下的全部"、"某源下的某标签"）；两者都为空表示全库。
    ///
    /// # 为什么必须支持标签范围
    /// 标签视图下若仍按全库处理，用户点一次"全部已读"会连带清空所有其它源，
    /// 属不可逆的越界操作——范围必须与列表当前展示的范围一致。
    ///
    /// 写完后统一重算所有订阅源的未读数（全表 UPDATE 开销可忽略，
    /// 且能保证批量置已读后侧边栏数字立即归零，而非滞后到下次刷新）。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `feed_id` - 订阅源 ID；`None` 表示不按源限定
    /// * `tag_id` - 标签 ID；`None` 表示不按标签限定
    ///
    /// # 返回值
    /// 实际被更新的文章行数（已读的文章不重复计入）
    ///
    /// # 错误
    /// 数据库写入或重算未读数失败时返回错误信息
    pub async fn mark_all_read(
        pool: &SqlitePool,
        feed_id: Option<i64>,
        tag_id: Option<i64>,
    ) -> Result<u64, String> {
        // 动态拼 WHERE：只拼列名与占位符，值一律 bind；
        // 恒带 `is_read = 0`，既避免对已读行重复写，也保证返回行数即"本次新标记数"
        let mut wheres: Vec<&str> = vec!["is_read = 0"];
        if feed_id.is_some() {
            wheres.push("feed_id = ?");
        }
        if tag_id.is_some() {
            wheres.push("id IN (SELECT article_id FROM article_tags WHERE tag_id = ?)");
        }
        let sql = format!("UPDATE articles SET is_read = 1 WHERE {}", wheres.join(" AND "));
        let mut q = sqlx::query(&sql);
        // 绑定顺序必须与拼接顺序一致
        if let Some(id) = feed_id {
            q = q.bind(id);
        }
        if let Some(id) = tag_id {
            q = q.bind(id);
        }
        let affected = q.execute(pool).await.map_err(|e| e.to_string())?.rows_affected();

        // 重算全部订阅源未读数（全表 UPDATE，一次性覆盖所有受影响源）
        sqlx::query(
            "UPDATE feeds SET unread_count = (
                SELECT COUNT(*) FROM articles
                WHERE articles.feed_id = feeds.id AND articles.is_read = 0
            )",
        )
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(affected)
    }

    /// 切换文章收藏状态
    ///
    /// 采用"先读后写"两步式：SQLite 没有直接可用的布尔取反语法，
    /// 且需要先确认文章存在，因此先查一次当前值。
    ///
    /// # 返回值
    /// 收藏后的状态（true=已收藏，false=已取消）
    ///
    /// # 错误
    /// 文章不存在时返回错误；数据库写入失败时返回错误
    pub async fn toggle_bookmark(pool: &SqlitePool, id: i64) -> Result<bool, String> {
        let row = sqlx::query("SELECT is_bookmarked FROM articles WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;

        match row {
            Some(r) => {
                // 列里存的是 INTEGER 0/1，FromRow/get 在此按 bool 读出
                let current: bool = r.get("is_bookmarked");
                let new_val = !current;
                // 写回时转 i32：sqlx 的 bool 绑定在 SQLite 下最终写成 0/1
                sqlx::query("UPDATE articles SET is_bookmarked = ? WHERE id = ?")
                    .bind(new_val as i32)
                    .bind(id)
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(new_val)
            }
            // 文章已被删除（或 ID 非法）时明确报错，避免前端误以为切换成功
            None => Err("文章不存在".to_string()),
        }
    }

    /// 设置文章的用户偏好标记（喜欢/跳过/取消）
    ///
    /// `preference` 只接受 `"like"` / `"skip"` / `""`（清空）三个合法值，
    /// 其余视为无效输入并报错——这样前端不会误传异常状态。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 文章 ID
    /// * `preference` - `"like"`（喜欢）/ `"skip"`（跳过）/ `""`（取消标记）
    ///
    /// # 错误
    /// 文章不存在、数据库写入失败、或 preference 非法时返回错误信息
    pub async fn set_user_preference(
        pool: &SqlitePool,
        id: i64,
        preference: &str,
    ) -> Result<(), String> {
        // 校验输入：只允许三种合法值
        match preference {
            "like" | "skip" | "" => {}
            _ => return Err(format!("非法偏好值: {}", preference)),
        }

        // 先确认文章存在
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM articles WHERE id = ?)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
        if !exists {
            return Err("文章不存在".to_string());
        }

        // 写回用户偏好（空字符串表示取消标记，存为 NULL）
        let pref_value: Option<&str> = if preference.is_empty() { None } else { Some(preference) };
        sqlx::query("UPDATE articles SET user_preference = ? WHERE id = ?")
            .bind(pref_value)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// 批量插入文章（忽略重复）
    ///
    /// # 返回值
    /// 新插入的文章数量（被唯一约束忽略的重复条目不计入）
    ///
    /// # 错误
    /// 任一条目写入失败即整体返回错误（未使用事务，已写入的条目不会回滚）
    ///
    /// # 去重与更新策略
    /// 以 `UNIQUE(feed_id, guid)` 为唯一键：
    /// - 首次出现 → 直接插入；
    /// - 已存在（刷新命中）→ **只更新内容类字段**（`link` / `title` / `summary` /
    ///   `content` / `author` / `published_at`），**不动**用户的本地状态
    ///   （`is_read` / `is_bookmarked`）与 AI 产物（`ai_summary` / `ai_translation`）。
    /// 这样刷新既能把最新的正文/摘要同步进来（例如后端改为保留富文本 HTML），
    /// 又不会让"已读"变回"未读"或丢失收藏与 AI 摘要。
    pub async fn batch_insert(
        pool: &SqlitePool,
        feed_id: i64,
        feed_name: &str,
        folder_id: i64,
        filters: &[FilterRule],
        items: &[crate::rss::ParsedItem],
    ) -> Result<i64, String> {
        let mut count = 0i64;
        // 本刷新批次内的「标签名 -> 标签 ID」缓存：一次刷新常有多篇文章命中同一标签，
        // 缓存后每名只走一次「查/建」，而不是每篇各走一次
        let mut tag_cache: HashMap<String, i64> = HashMap::new();

        // 只取前 30 条：单次刷新入库量封顶，避免历史超长 feed 首次订阅时写入过多
        for item in items.iter().take(30) {
            // 先尝试插入；命中 UNIQUE(feed_id, guid) 约束时 INSERT OR IGNORE 不会报错，
            // 只是 rows_affected() == 0（即"未新增"），此时转入下方更新分支
            let result = sqlx::query(
                "INSERT OR IGNORE INTO articles \
                 (feed_id, guid, link, title, summary, content, author, published_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(feed_id)
            .bind(&item.guid)
            .bind(&item.link)
            .bind(&item.title)
            .bind(&item.summary)
            .bind(&item.content)
            .bind(&item.author)
            .bind(item.published_at)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;

            if result.rows_affected() == 0 {
                // 已存在：仅更新会变的内容字段，保留已读 / 收藏 / AI 摘要等本地状态
                sqlx::query(
                    "UPDATE articles SET \
                     link = ?, title = ?, summary = ?, content = ?, author = ?, published_at = ? \
                     WHERE feed_id = ? AND guid = ?",
                )
                .bind(&item.link)
                .bind(&item.title)
                .bind(&item.summary)
                .bind(&item.content)
                .bind(&item.author)
                .bind(item.published_at)
                .bind(feed_id)
                .bind(&item.guid)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            } else {
                // 仅真正新增的条目计入"本次新增"，保证刷新计数语义稳定
                count += 1;

                // 规则引擎：对新建文章求值启用中的规则，命中即施加动作。
                // 用刚插入行的自增主键定位，避免再次查询。
                let new_id = result.last_insert_rowid();
                let mut mark_read = false;
                let mut star = false;
                // 本文命中的标签名（去重后的顺序即规则求值顺序）
                let mut tag_names: Vec<&str> = Vec::new();
                for f in filters {
                    if f.matches(feed_id, folder_id, feed_name, item) {
                        match f.action.as_str() {
                            "star" => star = true,
                            "mark_read" | "hide" => mark_read = true,
                            "tag" => {
                                // `action_arg` 为空说明规则没配标签名（例如标签被删除后
                                // 由 Tag::delete 清空），此时跳过——绝不能创建空名标签
                                let n = f.action_arg.trim();
                                if !n.is_empty() && !tag_names.contains(&n) {
                                    tag_names.push(n);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if mark_read {
                    sqlx::query("UPDATE articles SET is_read = 1 WHERE id = ?")
                        .bind(new_id)
                        .execute(pool)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                if star {
                    sqlx::query("UPDATE articles SET is_bookmarked = 1 WHERE id = ?")
                        .bind(new_id)
                        .execute(pool)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                // 自动打标签：**追加**语义（Tag::attach 是 INSERT OR IGNORE），
                // 不清理用户已有标签——规则只负责"补",不负责"删"
                for name in tag_names {
                    // 先 copied() 结束对 map 的不可变借用，否则下面的插入会与之冲突
                    let cached = tag_cache.get(name).copied();
                    let tag_id = match cached {
                        Some(id) => id,
                        None => {
                            // 标签不存在则自动创建（规则可以引用尚未建立的标签）
                            let id = Tag::create(pool, name, "").await?;
                            tag_cache.insert(name.to_string(), id);
                            id
                        }
                    };
                    Tag::attach(pool, new_id, tag_id).await?;
                }
            }
        }

        Ok(count)
    }
}

// ─── Setting CRUD ─────────────────────────────────────────────────────────────

impl Setting {
    /// 获取所有设置
    pub async fn get_all(pool: &SqlitePool) -> Result<Vec<Setting>, String> {
        sqlx::query_as::<_, Setting>("SELECT key, value FROM settings")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())
    }

    /// 设置或更新一个配置项
    pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 获取单个配置项的值
    pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map(|r| r.get::<String, _>("value")))
    }
}

// ─── 统计与备份 ─────────────────────────────────────────────────────────────

/// 汇总全库统计信息
///
/// 用四条独立的 `COUNT(*)` 一次性取出订阅源数、文章总数、已读/未读数、收藏数，
/// 全部为聚合查询、开销可忽略，结果组装为 [`Stats`] 供前端"数据"标签页展示。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 包含各项计数的 [`Stats`] 快照
///
/// # 错误
/// 任意一条 COUNT 查询失败时返回错误信息
// ─── 过滤规则（规则引擎） ──────────────────────────────────────────────────────

/// 过滤规则模型
///
/// 对应 `filters` 表。规则在文章入库时求值：命中即对该文章施加 `action`，
/// 是"多源订阅不淹死"的核心手段（对齐 Inoreader/FreshRSS 的过滤规则）。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FilterRule {
    /// 规则 ID（自增主键）
    pub id: i64,
    /// 规则名称（便于用户在 UI 中辨识）
    pub name: String,
    /// 是否启用
    pub enabled: bool,
    /// 作用范围：`all`(全库) / `feed`(指定源) / `folder`(指定文件夹)
    pub scope: String,
    /// 作用范围的限定 ID（`scope=feed` 时为 feed_id，`scope=folder` 时为 folder_id）
    pub scope_id: i64,
    /// 匹配字段：`title` / `content` / `author` / `feed`(源名)
    pub field: String,
    /// 匹配方式：`contains` / `not_contains` / `equals` / `not_equals`
    pub op: String,
    /// 匹配值（用户输入的关键字）
    pub value: String,
    /// 命中动作：`mark_read`(自动已读) / `star`(收藏) / `hide`(隐藏=标记已读)
    /// / `tag`(自动打标签)
    pub action: String,
    /// 动作参数：仅 `action = "tag"` 时使用，存标签名（同名标签不存在时自动创建）。
    /// 其余动作恒为空串。
    pub action_arg: String,
    /// 优先级（数值越大越先求值；同优先级按 id 升序）
    pub priority: i64,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl FilterRule {
    /// 列出全部规则（含停用），供设置页管理
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<FilterRule>, String> {
        sqlx::query_as::<_, FilterRule>(
            "SELECT * FROM filters ORDER BY priority DESC, id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 仅列出启用中的规则（入库时求值用，减少无效判断）
    pub async fn list_enabled(pool: &SqlitePool) -> Result<Vec<FilterRule>, String> {
        sqlx::query_as::<_, FilterRule>(
            "SELECT * FROM filters WHERE enabled = 1 ORDER BY priority DESC, id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 新建规则
    ///
    /// # 参数
    /// * `name` / `scope` / `scope_id` / `field` / `op` / `value` / `action` / `action_arg`
    ///   / `enabled` / `priority`
    /// * `action_arg` - 仅 `action = "tag"` 时有意义（标签名），其余动作传空串
    ///
    /// # 返回值
    /// 新规则 ID
    pub async fn create(
        pool: &SqlitePool,
        name: &str,
        scope: &str,
        scope_id: i64,
        field: &str,
        op: &str,
        value: &str,
        action: &str,
        action_arg: &str,
        enabled: bool,
        priority: i64,
    ) -> Result<i64, String> {
        let result = sqlx::query(
            "INSERT INTO filters (name, scope, scope_id, field, op, value, action, action_arg, enabled, priority) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(scope)
        .bind(scope_id)
        .bind(field)
        .bind(op)
        .bind(value)
        .bind(action)
        // 非 tag 动作统一落库为空串：避免残留上一次编辑时填的标签名
        .bind(if action == "tag" { action_arg } else { "" })
        .bind(enabled as i32)
        .bind(priority)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.last_insert_rowid())
    }

    /// 更新规则（按 id 全量覆盖可改字段）
    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        name: &str,
        scope: &str,
        scope_id: i64,
        field: &str,
        op: &str,
        value: &str,
        action: &str,
        action_arg: &str,
        enabled: bool,
        priority: i64,
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE filters SET name = ?, scope = ?, scope_id = ?, field = ?, op = ?, \
             value = ?, action = ?, action_arg = ?, enabled = ?, priority = ? WHERE id = ?",
        )
        .bind(name)
        .bind(scope)
        .bind(scope_id)
        .bind(field)
        .bind(op)
        .bind(value)
        .bind(action)
        .bind(if action == "tag" { action_arg } else { "" })
        .bind(enabled as i32)
        .bind(priority)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 删除规则
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM filters WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 判断单条新文章是否命中本规则
    ///
    /// 先做作用范围裁剪（feed/folder），再按字段取文本、按匹配方式比较。
    /// 比较统一转小写，做到大小写不敏感；`feed` 字段用源名匹配。
    ///
    /// # 参数
    /// * `feed_id` - 当前文章所属源 ID
    /// * `folder_id` - 当前文章所属文件夹 ID（0 表示未分类）
    /// * `feed_name` - 当前源名称（用于 `field=feed`）
    /// * `item` - 解析后的文章（提供 title/content/author）
    ///
    /// # 返回值
    /// 命中返回 true
    fn matches(
        &self,
        feed_id: i64,
        folder_id: i64,
        feed_name: &str,
        item: &crate::rss::ParsedItem,
    ) -> bool {
        // 作用范围裁剪：feed/folder 不匹配则直接跳过，避免误伤其他源
        match self.scope.as_str() {
            "feed" => {
                if self.scope_id != feed_id {
                    return false;
                }
            }
            "folder" => {
                if self.scope_id != folder_id {
                    return false;
                }
            }
            _ => {}
        }

        // 取匹配字段对应的文本
        let haystack = match self.field.as_str() {
            "title" => item.title.as_str(),
            "content" => item.content.as_str(),
            "author" => item.author.as_str(),
            "feed" => feed_name,
            _ => item.title.as_str(),
        };

        let needle = self.value.to_lowercase();
        let haystack = haystack.to_lowercase();

        // 按匹配方式比较
        match self.op.as_str() {
            "contains" => haystack.contains(&needle),
            "not_contains" => !haystack.contains(&needle),
            "equals" => haystack == needle,
            "not_equals" => haystack != needle,
            _ => false,
        }
    }
}

// ─── 文件夹（订阅源分组） ──────────────────────────────────────────────────────

/// 文件夹模型
///
/// 对应 `folders` 表，用于把订阅源分组（对齐 Folo 的 SubscriptionColumn 分组）。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Folder {
    /// 文件夹 ID
    pub id: i64,
    /// 文件夹名称
    pub name: String,
    /// 侧边栏显示顺序（小的在前）
    pub position: i64,
}

impl Folder {
    /// 列出全部文件夹（按 position、id 升序）
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Folder>, String> {
        sqlx::query_as::<_, Folder>("SELECT * FROM folders ORDER BY position ASC, id ASC")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())
    }

    /// 新建文件夹
    ///
    /// # 返回值
    /// 新文件夹 ID
    pub async fn create(pool: &SqlitePool, name: &str, position: i64) -> Result<i64, String> {
        let result = sqlx::query("INSERT INTO folders (name, position) VALUES (?, ?)")
            .bind(name)
            .bind(position)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(result.last_insert_rowid())
    }

    /// 重命名或调整文件夹顺序
    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        name: &str,
        position: i64,
    ) -> Result<(), String> {
        sqlx::query("UPDATE folders SET name = ?, position = ? WHERE id = ?")
            .bind(name)
            .bind(position)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 删除文件夹，并将其下订阅源移回"未分类"（folder_id 归零）
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
        // 先把归属该文件夹的订阅源置为未分类，避免悬空外键语义
        sqlx::query("UPDATE feeds SET folder_id = 0 WHERE folder_id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM folders WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 把某订阅源移动到指定文件夹（folder_id=0 表示移出）
    pub async fn set_feed_folder(
        pool: &SqlitePool,
        feed_id: i64,
        folder_id: i64,
    ) -> Result<(), String> {
        sqlx::query("UPDATE feeds SET folder_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(folder_id)
            .bind(feed_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ─── 高亮 / 批注 ──────────────────────────────────────────────────────────────

/// 高亮片段模型
///
/// 对应 `highlights` 表，记录正文内的高亮文本与可选批注，用于知识沉淀。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Highlight {
    /// 高亮 ID
    pub id: i64,
    /// 所属文章 ID
    pub article_id: i64,
    /// 高亮的原文文本
    pub text: String,
    /// 用户批注（可选）
    pub note: String,
    /// 高亮颜色（yellow / green / blue 等）
    pub color: String,
    /// 文本起点偏移（用于重新定位高亮，可选）
    pub start_offset: i64,
    /// 文本终点偏移（可选）
    pub end_offset: i64,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

impl Highlight {
    /// 列出某文章的全部高亮（按起点偏移升序，保证正文顺序）
    pub async fn list_by_article(
        pool: &SqlitePool,
        article_id: i64,
    ) -> Result<Vec<Highlight>, String> {
        sqlx::query_as::<_, Highlight>(
            "SELECT * FROM highlights WHERE article_id = ? ORDER BY start_offset ASC, id ASC",
        )
        .bind(article_id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 新建高亮
    ///
    /// # 返回值
    /// 新高亮 ID
    pub async fn create(
        pool: &SqlitePool,
        article_id: i64,
        text: &str,
        note: &str,
        color: &str,
        start_offset: i64,
        end_offset: i64,
    ) -> Result<i64, String> {
        let result = sqlx::query(
            "INSERT INTO highlights (article_id, text, note, color, start_offset, end_offset) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(article_id)
        .bind(text)
        .bind(note)
        .bind(color)
        .bind(start_offset)
        .bind(end_offset)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(result.last_insert_rowid())
    }

    /// 删除高亮
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM highlights WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ─── 标签 ──────────────────────────────────────────────────────────────────────

/// 标签模型
///
/// 对应 `tags` 表。标签是**用户侧**的整理维度：既可在正文页手工打标，
/// 也可由过滤规则（`action = "tag"`）在文章入库时自动打标。
///
/// # 与 [`Entity`] 的分工（不要合并二者）
/// 实体是给机器用的：参与 `entity_score` 评分、作为研究事件提取的目标；
/// 标签是给人用的：浏览、归档、批量筛选。二者数据来源与生命周期都不同，
/// 合并会同时污染自动评分与本就不该被自动修改的用户分类。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Tag {
    /// 唯一标识符（自增主键）
    pub id: i64,
    /// 标签名（表级 `UNIQUE COLLATE NOCASE`，ASCII 大小写不敏感去重）
    pub name: String,
    /// 标签颜色（空串表示使用界面默认色；具体取值由前端色板限定）
    pub color: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 该标签下的文章数
    ///
    /// **不是 `tags` 表上的列**，由 [`Tag::list_all`] 的 LEFT JOIN 聚合而来。
    /// 用 `#[sqlx(default)]` 承接：按 id 反查单个标签等查询不提供该列时，
    /// 派生逻辑会捕获 `ColumnNotFound` 并落回 `Default::default()`（0），
    /// 而不是直接报错——否则任何新增的「只取标签本体」查询都会踩雷。
    #[sqlx(default)]
    #[serde(default)]
    pub article_count: i64,
}

impl Tag {
    /// 列出全部标签（含各自文章数），按名称升序
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    ///
    /// # 返回值
    /// 标签列表；`article_count` 为 LEFT JOIN 聚合结果（无文章时为 0）
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Tag>, String> {
        // LEFT JOIN 保证没有人使用的标签也会出现在列表里（计数为 0）；
        // 分组时必须把 t.* 的全部非聚合列都列出，SQLite 才接受这种 SELECT 形式。
        sqlx::query_as::<_, Tag>(
            "SELECT t.id, t.name, t.color, t.created_at, COUNT(atg.article_id) AS article_count \
             FROM tags t LEFT JOIN article_tags atg ON atg.tag_id = t.id \
             GROUP BY t.id, t.name, t.color, t.created_at \
             ORDER BY t.name COLLATE NOCASE ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// 新建标签；同名（ASCII 大小写不敏感）已存在时复用既有标签
    ///
    /// 供两条路径共用：界面上的「新建标签」与规则引擎的「自动打标签」。
    /// 复用而非报错，是因为两处场景都不希望因为标签已存在而中断流程
    /// （规则引擎尤其如此——它无从询问用户）。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `name` - 标签名（会 trim；空名视为非法）
    /// * `color` - 颜色（仅在**新建**时生效；复用既有标签时不覆盖其颜色）
    ///
    /// # 返回值
    /// 标签 ID（新建的或既有的）
    ///
    /// # 错误
    /// 标签名为空、或数据库写入/查询失败时返回错误信息
    pub async fn create(pool: &SqlitePool, name: &str, color: &str) -> Result<i64, String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("标签名不能为空".to_string());
        }
        // INSERT OR IGNORE 让"同名复用"与"新建"合并为一条语句：
        // 命中 UNIQUE 时静默跳过（不覆盖既有颜色），之后再按名查回 id。
        sqlx::query("INSERT OR IGNORE INTO tags (name, color) VALUES (?, ?)")
            .bind(trimmed)
            .bind(color)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        let row = sqlx::query("SELECT id FROM tags WHERE name = ? LIMIT 1")
            .bind(trimmed)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
        row.map(|r| r.get::<i64, _>("id"))
            .ok_or_else(|| "标签创建失败".to_string())
    }

    /// 重命名 / 改色
    ///
    /// # 为什么要在事务里同步 `filters.action_arg`
    /// 规则引擎的「打标签」动作以**标签名**为参数（见 `filters` 表注释）。
    /// 若重命名不同步过去，规则会继续按旧名打标，而旧名已不存在于 `tags` 表，
    /// 引擎就会在下一次刷新时创建一个同名新标签——用户的标签被静默裂成两半。
    /// 因此改名与同步更新必须同事务提交，不能一成功一失败。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `id` - 标签 ID
    /// * `name` - 新名称（会 trim）
    /// * `color` - 新颜色
    ///
    /// # 错误
    /// 新名称为空、与其它标签重名（返回可读中文提示）、或数据库操作失败时返回错误信息
    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        name: &str,
        color: &str,
    ) -> Result<(), String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err("标签名不能为空".to_string());
        }
        // 先取旧名：改名后新旧名的对应关系就查不到了，必须在 UPDATE 之前读出来
        let old_name: Option<String> = sqlx::query("SELECT name FROM tags WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
            .map(|r| r.get::<String, _>("name"));

        let result = sqlx::query("UPDATE tags SET name = ?, color = ? WHERE id = ?")
            .bind(trimmed)
            .bind(color)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| {
                // 把 UNIQUE 冲突翻译成人话：原始错误是
                // "UNIQUE constraint failed: tags.name"，直接抛给用户毫无意义
                if e.to_string().contains("UNIQUE") {
                    "已存在同名标签".to_string()
                } else {
                    e.to_string()
                }
            })?;
        if result.rows_affected() == 0 {
            return Err("标签不存在".to_string());
        }

        if let Some(old) = old_name {
            if old != trimmed {
                sqlx::query(
                    "UPDATE filters SET action_arg = ? WHERE action = 'tag' AND action_arg = ?",
                )
                .bind(trimmed)
                .bind(&old)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// 删除标签
    ///
    /// 关联行（`article_tags`）由外键级联删除，无需在此手工清理。
    ///
    /// # 引用该标签的规则怎么处理
    /// 把规则的 `action_arg` 置空但**保留 `action = 'tag'`**：规则本身不删、
    /// 也不偷偷改成别的动作（改成 `mark_read` 会让它忽然开始标记已读，属意外副作用）。
    /// 置空后该规则在引擎里被跳过，界面会把它显示成「打标签（未指定）」，
    /// 用户可自行改指其它标签或删除规则——把决定权交回用户，而不是替他做选择。
    ///
    /// # 错误
    /// 数据库操作失败时返回错误信息
    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
        let name: Option<String> = sqlx::query("SELECT name FROM tags WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
            .map(|r| r.get::<String, _>("name"));

        // 删除与规则清理同事务：留一半会让规则引用一个已不存在的标签名，
        // 下次刷新又会把该标签名重新创建出来
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM tags WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(n) = name {
            sqlx::query("UPDATE filters SET action_arg = '' WHERE action = 'tag' AND action_arg = ?")
                .bind(&n)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 整组覆盖某篇文章的标签（先清空再写入）
    ///
    /// 采用"整组覆盖"而非增量增删：界面上的打标是一个多选面板，提交的是
    /// 最终勾选集合，覆盖语义既简单又天然幂等，不必让前端计算增删差集。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `article_id` - 文章 ID
    /// * `tag_ids` - 该文章最终应有的标签 ID 集合（空数组表示清空全部标签）
    ///
    /// # 错误
    /// 数据库操作失败时返回错误信息
    pub async fn set_for_article(
        pool: &SqlitePool,
        article_id: i64,
        tag_ids: &[i64],
    ) -> Result<(), String> {
        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM article_tags WHERE article_id = ?")
            .bind(article_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        for tag_id in tag_ids {
            sqlx::query("INSERT OR IGNORE INTO article_tags (article_id, tag_id) VALUES (?, ?)")
                .bind(article_id)
                .bind(*tag_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 给一篇文章追加一个标签（幂等，供规则引擎自动打标使用）
    ///
    /// 与 [`Self::set_for_article`] 的区别：这里**不做清空**，只做追加，
    /// 因为自动打标是在用户已有标签之上叠加，绝不该抹掉用户手工打的标签。
    ///
    /// # 错误
    /// 数据库写入失败时返回错误信息
    pub async fn attach(pool: &SqlitePool, article_id: i64, tag_id: i64) -> Result<(), String> {
        sqlx::query("INSERT OR IGNORE INTO article_tags (article_id, tag_id) VALUES (?, ?)")
            .bind(article_id)
            .bind(tag_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 批量取回若干文章的标签，按文章 ID 归组
    ///
    /// 列表页一次取 50 篇，若每篇各查一次标签就是 50 次往返（典型 N+1）；
    /// 这里用单条 `IN` 查询一次取回整页标签，再在内存里归组。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `ids` - 文章 ID 列表（空列表直接返回空 map，不发起查询）
    ///
    /// # 返回值
    /// `article_id -> 该文章的标签（按名称升序）`；没有标签的文章不会出现在 map 中
    ///
    /// # 错误
    /// 数据库查询失败时返回错误信息
    pub async fn for_articles(
        pool: &SqlitePool,
        ids: &[i64],
    ) -> Result<HashMap<i64, Vec<Tag>>, String> {
        let mut map: HashMap<i64, Vec<Tag>> = HashMap::new();
        if ids.is_empty() {
            return Ok(map);
        }
        // 只拼占位符个数，值仍走 bind，不构成 SQL 注入口
        let placeholders = std::iter::repeat("?")
            .take(ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT atg.article_id, t.id, t.name, t.color, t.created_at \
             FROM article_tags atg JOIN tags t ON t.id = atg.tag_id \
             WHERE atg.article_id IN ({}) ORDER BY t.name COLLATE NOCASE ASC",
            placeholders
        );
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(*id);
        }
        let rows = q.fetch_all(pool).await.map_err(|e| e.to_string())?;
        for row in rows {
            let article_id: i64 = row.get("article_id");
            map.entry(article_id).or_default().push(Tag {
                id: row.get("id"),
                name: row.get("name"),
                color: row.get("color"),
                created_at: row.get("created_at"),
                // 批量挂载只关心"有哪些标签"，不带计数（本查询也未聚合它）
                article_count: 0,
            });
        }
        Ok(map)
    }
}

// ─── 全文检索 ─────────────────────────────────────────────────────────────────

/// 全文搜索文章（支持标题/正文/摘要的子串匹配，含中文）
///
/// 底层用 FTS5 的 trigram 分词器：把查询串作为短语传入，trigram 索引即可高效完成
/// 子串匹配（CJK 也能命中，因为中文按字切成三连词）。触发器已保证 `articles_fts`
/// 与 `articles` 实时同步，故这里直接 JOIN 取回完整文章行。
///
/// # 参数
/// * `query` - 用户输入的搜索词（无需引号/通配符，内部会安全转义）
/// * `limit` - 返回上限
///
/// # 返回值
/// 命中的文章列表，按 FTS5 相关度（`rank`）排序
///
/// # 错误
/// 数据库查询失败时返回错误信息
pub async fn search_articles(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<Article>, String> {
    // 空查询直接返回空结果，避免对全表做无意义扫描
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    // trigram 分词器只索引「3 个字符」的滑窗，因此查询串短于 3 个字符时
    // 没有任何 trigram 可以命中，MATCH 恒为空。此类短查询（如中文"世界"、"AI"）
    // 退化用 LIKE 子串匹配处理，保证 1–2 字的检索依然可用。
    if trimmed.chars().count() < 3 {
        let pattern = format!("%{}%", escape_like(trimmed));
        let mut rows = sqlx::query_as::<_, Article>(
            "SELECT * FROM articles \
             WHERE title LIKE ? ESCAPE '\\' OR content LIKE ? ESCAPE '\\' OR summary LIKE ? ESCAPE '\\' \
             ORDER BY published_at DESC LIMIT ?",
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        // 搜索结果同样要展示标签（列表项统一渲染 chip），故与列表查询一致地挂载
        Article::attach_tags(pool, &mut rows).await?;
        return Ok(rows);
    }

    // 3 字及以上：走 FTS5 全文检索（覆盖标题 / 正文 / 摘要，中文按三元组匹配）。
    // 查询串包成 FTS5 短语：双引号内视为连续子串；
    // 内部的双引号按 FTS5 转义规则翻倍，防止用户引号破坏语法。
    let match_expr = format!("\"{}\"", trimmed.replace('"', "\"\""));

    let fts_result = sqlx::query_as::<_, Article>(
        "SELECT a.* FROM articles_fts f \
         JOIN articles a ON a.id = f.rowid \
         WHERE articles_fts MATCH ? ORDER BY rank LIMIT ?",
    )
    .bind(&match_expr)
    .bind(limit)
    .fetch_all(pool)
    .await;

    let mut rows = match fts_result {
        Ok(rows) => rows,
        // 索引损坏（SQLITE_CORRUPT_VTAB=267 / SQL logic error）时不能让搜索整个失败：
        // 退化为 LIKE 子串匹配保证功能可用，同时把异常打到 stderr 供排查；
        // 如损坏持续，删除数据库文件重启即可按建表语句重建全新索引。
        Err(e) => {
            eprintln!("全文索引查询失败，退化为 LIKE 匹配: {}", e);
            let pattern = format!("%{}%", escape_like(trimmed));
            sqlx::query_as::<_, Article>(
                "SELECT * FROM articles \
                 WHERE title LIKE ? ESCAPE '\\' OR content LIKE ? ESCAPE '\\' OR summary LIKE ? ESCAPE '\\' \
                 ORDER BY published_at DESC LIMIT ?",
            )
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?
        }
    };
    Article::attach_tags(pool, &mut rows).await?;
    Ok(rows)
}

/// 转义 LIKE 模式中的特殊字符（`%` / `_` / `\`）
///
/// # 参数
/// * `s` - 待转义的原始查询串
///
/// # 返回值
/// 可直接拼进 `LIKE ... ESCAPE '\'` 的模式片段（不含两端通配符）
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub async fn get_stats(pool: &SqlitePool) -> Result<Stats, String> {
    // 订阅源总数
    let feeds: i64 = sqlx::query("SELECT COUNT(*) FROM feeds")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
        .get(0);
    // 文章总数
    let articles: i64 = sqlx::query("SELECT COUNT(*) FROM articles")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
        .get(0);
    // 已读文章数
    let read: i64 = sqlx::query("SELECT COUNT(*) FROM articles WHERE is_read = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
        .get(0);
    // 收藏文章数
    let bookmarks: i64 = sqlx::query("SELECT COUNT(*) FROM articles WHERE is_bookmarked = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
        .get(0);
    // 今日已读：只统计今天创建的、已标记为已读的文章
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let read_today: i64 = sqlx::query(
        "SELECT COUNT(*) FROM articles WHERE is_read = 1 AND DATE(created_at) = ?",
    )
    .bind(&today)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?
    .get(0);

    Ok(Stats {
        feeds,
        articles,
        read,
        unread: articles - read,
        bookmarks,
        read_today,
    })
}

/// 备份文件的默认名：`readflow-backup-YYYY-MM-DD.db`
///
/// 只作为系统「另存为」对话框里的初始值，用户可改；用本地日期而非 UTC，
/// 否则在 UTC+8 的晚上导出会得到前一天的日期。
///
/// # 返回值
/// 形如 `readflow-backup-2026-10-06.db` 的文件名（不含目录）
pub fn default_backup_file_name() -> String {
    format!(
        "readflow-backup-{}.db",
        chrono::Local::now().format("%Y-%m-%d")
    )
}

/// 数据库备份：把 `readflow.db` 复制到指定路径
///
/// 用 [`tokio::fs::copy`] 而非 `std::fs::copy`：命令跑在异步运行时里，
/// 数据库可能有数百 MB，同步复制会长时间阻塞工作线程。
///
/// 复制主库文件成立的前提是**未启用 WAL**（本项目使用 SQLite 默认的 journal 模式）；
/// 若将来改用 WAL，须先 checkpoint，否则会漏掉尚在 `-wal` 文件里的已提交数据。
///
/// # 参数
/// * `app` - Tauri 应用句柄，用于定位数据库文件
/// * `dest` - 用户选定的目标路径
///
/// # 错误
/// 源文件读取或目标写入失败时返回错误信息
///
/// # 注意
/// 备份期间刷新调度器可能仍在写入，但复制得到的是某一时刻的快照，
/// 对本地备份场景足够可靠。
pub async fn db_backup_to(app: &tauri::AppHandle, dest: &str) -> Result<(), String> {
    let src = db_file_path(app);
    tokio::fs::copy(&src, dest)
        .await
        .map_err(|e| format!("备份数据库失败: {}", e))?;
    Ok(())
}

/// SQLite 数据库文件的魔数（每个合法库文件的前 16 字节）
///
/// 用文件头校验替代"打开连接跑 `PRAGMA integrity_check`"：前者只需读 16 字节、
/// 不建连接，却足以挡住"用户选错文件（图片 / 文本 / 其它二进制）导致主库被写坏"。
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// 恢复前给现有库留底时追加的后缀
///
/// 只保留最近一次（同名覆盖），路径固定为 `<数据目录>/readflow.db.before-restore`，
/// 便于误恢复后在文件管理器里直接找到并改回 `readflow.db`。
const PRE_RESTORE_SUFFIX: &str = ".before-restore";

/// 数据库恢复：用指定的备份文件覆盖 `readflow.db`
///
/// 与 [`db_backup_to`] 对称，由 Rust 直接操作文件（不再经 Base64 + IPC 传整个库）。
///
/// 覆盖是**破坏性操作**，因此按顺序做了三道保护：
/// 1. 拒绝"源文件就是当前库"——自己复制到自己是损坏行为（先做，不需要读盘）；
/// 2. 校验源文件前 16 字节是否为 SQLite 魔数，不是则明确报错且**不碰**目标库；
/// 3. 覆盖前把现有库复制为 `<库路径>.before-restore`，误恢复时可自行回滚。
///
/// # 参数
/// * `app` - Tauri 应用句柄，用于定位数据库文件
/// * `src` - 用户在系统对话框中选择的备份文件路径
///
/// # 错误
/// 源文件不存在 / 不可读 / 不是 SQLite 数据库、所选文件即当前库、留底或覆盖失败时返回错误信息
///
/// # 注意
/// 覆盖完成后已建立的连接池仍指向旧库，**必须重启应用**才能真正生效
/// （由命令层的 [`crate::app_restart`] 完成）；否则旧连接可能把旧页缓存写回新文件。
pub async fn db_restore_from(app: &tauri::AppHandle, src: &str) -> Result<(), String> {
    use tokio::io::AsyncReadExt;

    let dest = db_file_path(app);

    // 1) 拒绝自覆盖：先规范化两侧路径，避免 "../" 或符号链接绕过字符串比较。
    //    目标库一定存在（能进到恢复流程说明已初始化），源文件此前由对话框确认过存在
    let src_canon = std::fs::canonicalize(src).map_err(|e| format!("无法定位所选文件: {}", e))?;
    if let Ok(dest_canon) = std::fs::canonicalize(&dest) {
        if src_canon == dest_canon {
            return Err("所选文件就是当前数据库，无需恢复".to_string());
        }
    }

    // 2) 文件头校验：只读前 16 字节，不是 SQLite 就立即失败，绝不触碰目标库
    let mut header = [0u8; 16];
    tokio::fs::File::open(src)
        .await
        .map_err(|e| format!("打开所选文件失败: {}", e))?
        .read_exact(&mut header)
        .await
        .map_err(|e| format!("所选文件过小或读取失败: {}", e))?;
    if &header != SQLITE_HEADER {
        return Err("所选文件不是有效的 SQLite 数据库（文件头校验未通过）".to_string());
    }

    // 3) 覆盖前留底：只在目标库确实存在时复制（首次运行不会有恢复操作，属防御性判断）
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
        let keep = format!("{}{}", dest, PRE_RESTORE_SUFFIX);
        tokio::fs::copy(&dest, &keep)
            .await
            .map_err(|e| format!("恢复前备份当前数据库失败: {}", e))?;
    }

    // 4) 覆盖：用 tokio 的文件 API，库可能有数百 MB，同步复制会长时间阻塞工作线程
    tokio::fs::copy(src, &dest)
        .await
        .map_err(|e| format!("写入数据库失败: {}", e))?;
    Ok(())
}

/// 清理超过保留天数的旧文章
///
/// 按"发布时间早于 `keep_days` 天前"删除文章，但始终保留收藏（`is_bookmarked = 1`）文章；
/// 当 `keep_unread` 为 true 时，未读（`is_read = 0`）文章也一并豁免，避免误删尚未阅读的内容。
///
/// # 参数
/// * `keep_days` - 保留天数；小于等于 0 视为"永久保留"，直接返回 0（不做任何删除）
/// * `keep_unread` - 为 true 时，未读文章即便超期也不清理
///
/// # 返回值
/// 被删除的文章条数（用于前端提示"本次清理 N 篇"）
///
/// # 注意
/// - 删除后重新汇总各订阅源的未读数（`unread_count`），保证侧边栏计数器与真实数据一致；
///   否则超期未读文章被删后，侧边栏的未读角标会显示错误数字。
/// - `published_at` 为 NULL 的文章不会被 `datetime()` 比较匹配到，天然被保留（不删无日期文章）。
pub async fn cleanup_old_articles(
    pool: &SqlitePool,
    keep_days: i64,
    keep_unread: bool,
) -> Result<u64, String> {
    // keep_days <= 0：永久保留策略，不做任何删除
    if keep_days <= 0 {
        return Ok(0);
    }

    // 拼接 DELETE：收藏文章永远不清理；keep_unread 时再追加"仅删已读"约束
    let mut sql = String::from(
        "DELETE FROM articles \
         WHERE is_bookmarked = 0 \
         AND published_at < datetime('now', '-' || ?1 || ' days')",
    );
    if keep_unread {
        sql.push_str(" AND is_read = 1");
    }

    let deleted = sqlx::query(&sql)
        .bind(keep_days)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();

    // 重新汇总每个订阅源的未读数，修正被删除文章造成的计数偏差
    sqlx::query(
        "UPDATE feeds SET unread_count = (
            SELECT COUNT(*) FROM articles
            WHERE articles.feed_id = feeds.id AND is_read = 0
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(deleted)
}

/// 按当前设置执行文章保留策略
///
/// 读取 `article_retention_days` 与 `retention_keep_unread` 两个键值，
/// 转发给 [`cleanup_old_articles`]。供后台调度器每日定时调用，
/// 也供前端"立即清理"按钮复用同一套配置。
///
/// # 返回值
/// 被删除的文章条数
///
/// # 错误
/// 设置键读取失败、或清理执行失败时返回错误信息
pub async fn run_retention_cleanup(pool: &SqlitePool) -> Result<u64, String> {
    // 读取保留天数：键缺失或解析失败一律按 0（永久保留）处理，最保守也最安全
    let days_raw = Setting::get(pool, "article_retention_days")
        .await?
        .unwrap_or_else(|| "0".to_string());
    let keep_days: i64 = days_raw.trim().parse().unwrap_or(0);
    // 读取"保留未读"开关：除显式 '0' 外都视为开启（默认保护未读）
    let keep_unread = Setting::get(pool, "retention_keep_unread")
        .await?
        .map(|v| v != "0")
        .unwrap_or(true);

    cleanup_old_articles(pool, keep_days, keep_unread).await
}

// ─── 文章智能评分 ─────────────────────────────────────────────────────────────

/// 批量计算文章的实体匹配评分（异步后台任务）
///
/// 遍历所有启用的实体，统计每篇文章标题和摘要中出现的实体命中次数，
/// 将命中数映射到 0-100 分（命中 N 个实体 → score = min(N*20, 100)）。
/// 结果写入 `articles.entity_score`，不覆盖已有评分。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_ids` - 需要评分的文章 ID 列表（为空时全库评分）
///
/// # 返回值
/// 实际更新的文章条数
///
/// # 注意
/// 此为后台任务，不应阻塞主线程。调用方应在 `spawn` 或调度器中异步执行。
pub async fn batch_score_entities(
    pool: &SqlitePool,
    article_ids: Vec<i64>,
) -> Result<u64, String> {
    // 取全部已启用实体
    let entities = Entity::list_enabled(pool).await?;
    if entities.is_empty() {
        return Ok(0);
    }

    // 取需要评分的文章（entity_score IS NULL 或 0）
    //
    // 查询只取标题与摘要，故用 ArticleDigest 而不是 Article 承接——
    // 用 Article 会因缺少 feed_id 等列而让 sqlx 报 ColumnNotFound（见 ArticleDigest 文档）。
    let articles = if article_ids.is_empty() {
        // 全库评分：无 ID 过滤
        sqlx::query_as::<_, ArticleDigest>(
            "SELECT id, title, summary FROM articles WHERE entity_score IS NULL OR entity_score = 0",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
    } else {
        // 指定 ID 列表：构建动态 SQL
        let id_list: Vec<String> = article_ids.iter().map(|i| i.to_string()).collect();
        let cond = format!("id IN ({})", id_list.join(","));
        let sql = format!(
            "SELECT id, title, summary FROM articles WHERE {} AND (entity_score IS NULL OR entity_score = 0)",
            cond
        );
        sqlx::query_as::<_, ArticleDigest>(&sql)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?
    };

    let mut updated = 0u64;
    for article in &articles {
        let text = format!("{} {}", article.title, article.summary).to_lowercase();
        let mut hit_count = 0i64;
        for entity in &entities {
            if text.contains(&entity.name.to_lowercase()) {
                hit_count += 1;
            }
        }
        // 映射：每命中 1 个实体加 20 分，最高 100 分
        let score = (hit_count * 20).min(100);
        sqlx::query(
            "UPDATE articles SET entity_score = ? WHERE id = ?",
        )
        .bind(score)
        .bind(article.id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        updated += 1;
    }
    Ok(updated)
}

/// 获取指定时间范围内按实体维度的高分文章（用于每日摘要生成）
///
/// 返回最近 `days` 天内、entity_score ≥ threshold 的文章，按分数降序排列。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `days` - 回溯天数
/// * `threshold` - 最低评分阈值（0-100）
///
/// # 返回值
/// 高分文章列表
/// 智能摘要里的一个实体分组
///
/// 一个实体对应一组「标题或摘要里出现过它名字」的高分文章。同一篇文章可以
/// 出现在多个分组里（同时提到多个关注对象），这与评分口径一致——分数本就是
/// 命中实体数的累加。
#[derive(Debug, Clone, Serialize)]
pub struct EntityArticleGroup {
    /// 实体名（分组标题）
    pub entity: String,
    /// 实体类型：company / person / product / industry / other
    pub entity_type: String,
    /// 该实体命中的文章（评分倒序）
    pub articles: Vec<Article>,
}

pub async fn get_high_score_articles_grouped(
    pool: &SqlitePool,
    days: i64,
    threshold: i64,
) -> Result<Vec<EntityArticleGroup>, String> {
    let entities = Entity::list_enabled(pool).await?;
    // 没有关注实体就没有分组维度，直接返回空集合（前端显示"暂无高分文章"引导语）
    if entities.is_empty() {
        return Ok(Vec::new());
    }

    let cutoff = Utc::now() - chrono::Duration::days(days);
    let mut articles = sqlx::query_as::<_, Article>(
        "SELECT * FROM articles WHERE entity_score >= ? AND published_at >= ? ORDER BY entity_score DESC, published_at DESC",
    )
    .bind(threshold)
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    // 分组前一次性挂载标签：后面的 clone 会把 tags 一并带进各组，
    // 共享分组预览与列表项就能用同一套 chip 渲染
    Article::attach_tags(pool, &mut articles).await?;

    // 命中判定与 batch_score_entities 保持同一套规则（实体名在「标题 + 摘要」中的
    // 子串命中，大小写不敏感），否则会出现"分数 60 分却进不了任何分组"的错位。
    // 这里只在内存里做一次匹配：文章数受 days/threshold 限制，比逐实体查库省得多。
    let haystacks: Vec<String> = articles
        .iter()
        .map(|a| format!("{} {}", a.title, a.summary).to_lowercase())
        .collect();

    let mut groups: Vec<EntityArticleGroup> = Vec::new();
    for entity in &entities {
        let needle = entity.name.trim().to_lowercase();
        // 空名实体会命中一切文本（`contains("")` 恒为真），跳过以免污染分组
        if needle.is_empty() {
            continue;
        }
        let hit: Vec<Article> = articles
            .iter()
            .zip(haystacks.iter())
            .filter(|(_, text)| text.contains(&needle))
            .map(|(a, _)| a.clone())
            .collect();
        if !hit.is_empty() {
            groups.push(EntityArticleGroup {
                entity: entity.name.clone(),
                entity_type: entity.entity_type.clone(),
                articles: hit,
            });
        }
    }

    // 文章多的实体排前面（用户更关心"最近被反复提到"的对象），并列时按名字稳定排序，
    // 避免同一批数据两次打开顺序不同
    groups.sort_by(|a, b| {
        b.articles
            .len()
            .cmp(&a.articles.len())
            .then_with(|| a.entity.cmp(&b.entity))
    });

    Ok(groups)
}

#[cfg(test)]
mod tests {
    //! 订阅源 SQL 行为与智能摘要分组的验证
    //!
    //! 涉及整行映射的用例（如 [`Article`]，按列名取值）一律调用 [`create_tables`]
    //! 建表，夹具与 schema 不会再漂移；只验证约束翻译的订阅源用例仍保留最小表结构。
    //! ——此前本注释称 `create_tables` 需要 AppHandle，实为把它与 [`init_db`] 混淆，
    //! 于是各用例手抄建表语句，给 `Article` 加列时便集中炸在夹具上。

    use super::*;

    /// 建内存库与最小 feeds 表
    ///
    /// `Feed::update` 会写 name / url / folder_id / updated_at 四列，
    /// 夹具必须把它们全部备齐，否则会以 `no such column` 失败。
    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("建内存库失败");
        sqlx::query(
            "CREATE TABLE feeds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                folder_id INTEGER DEFAULT 0,
                updated_at DATETIME
            )",
        )
        .execute(&pool)
        .await
        .expect("建 feeds 表失败");
        pool
    }

    /// 插入一条订阅源并返回其 id
    async fn insert(pool: &SqlitePool, name: &str, url: &str) -> i64 {
        sqlx::query("INSERT INTO feeds (name, url) VALUES (?, ?)")
            .bind(name)
            .bind(url)
            .execute(pool)
            .await
            .expect("插入订阅源失败")
            .last_insert_rowid()
    }

    /// 读回一条订阅源的 (url, name, folder_id)
    async fn read(pool: &SqlitePool, id: i64) -> (String, String, i64) {
        sqlx::query_as::<_, (String, String, i64)>(
            "SELECT url, name, folder_id FROM feeds WHERE id = ?",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("查询订阅源失败")
    }

    /// 改链接生效，名称与文件夹一并落库
    #[tokio::test]
    async fn update_writes_new_url_and_fields() {
        let pool = setup().await;
        let id = insert(&pool, "旧名", "https://a.com/feed").await;

        Feed::update(&pool, id, Some("https://b.com/feed"), "新名", 3)
            .await
            .expect("更新失败");

        let (url, name, folder_id) = read(&pool, id).await;
        assert_eq!(url, "https://b.com/feed");
        assert_eq!(name, "新名");
        assert_eq!(folder_id, 3);
    }

    /// `url` 传 `None` 时链接原样保留
    ///
    /// 改名 / 移动文件夹不该动地址，否则一次"重命名"就会把订阅悄悄指向别处。
    #[tokio::test]
    async fn update_keeps_url_when_none() {
        let pool = setup().await;
        let id = insert(&pool, "旧名", "https://a.com/feed").await;

        Feed::update(&pool, id, None, "新名", 0)
            .await
            .expect("更新失败");

        let (url, ..) = read(&pool, id).await;
        assert_eq!(url, "https://a.com/feed");
    }

    /// 改成已存在的链接时报可读中文，而不是漏出 SQLite 的英文约束错误
    #[tokio::test]
    async fn update_rejects_duplicate_url() {
        let pool = setup().await;
        insert(&pool, "已存在", "https://a.com/feed").await;
        let other = insert(&pool, "另一个", "https://b.com/feed").await;

        let err = Feed::update(&pool, other, Some("https://a.com/feed"), "另一个", 0)
            .await
            .expect_err("改成已存在的链接应当报错");

        assert!(err.contains("已被其它订阅源占用"), "实际错误: {}", err);
    }

    // ─── 智能摘要分组 ─────────────────────────────────────────────────────────

    /// 建内存库（直接复用正式建表函数，夹具与 schema 不会再漂移）
    ///
    /// 此前这里是手抄的一份 `CREATE TABLE articles` + `entities`，而 `Article`
    /// 按**列名**映射（`SELECT *` 也要求每一列都在），夹单一少列，测试就会以
    /// `no column found for name: ...` 失败——给 `Article` 加 `ai_extracted_at`
    /// 时正是这样炸的。改为调用 [`create_tables`] 后，schema 增删列时夹具自动跟随，
    /// 不必再手工同步两份 DDL。
    async fn setup_digest() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("建内存库失败");
        create_tables(&pool).await.expect("建表失败");
        // `articles.feed_id` 带指向 `feeds(id)` 的外键，而 sqlite 默认启用外键约束
        // （libsqlite3-sys 以 -DSQLITE_DEFAULT_FOREIGN_KEYS=1 编译），
        // 所以必须先有一条 id = 1 的订阅源，评分用例插入的文章才挂得上。
        sqlx::query(
            "INSERT INTO feeds (id, name, url) VALUES (1, '测试源', 'https://example.com/feed.xml')",
        )
        .execute(&pool)
        .await
        .expect("插入测试订阅源失败");
        pool
    }

    /// 插入一个实体（`enabled` 直接给定，用于验证禁用实体不参与分组）
    async fn insert_entity(pool: &SqlitePool, name: &str, enabled: i64) -> i64 {
        sqlx::query("INSERT INTO entities (name, entity_type, enabled) VALUES (?, 'company', ?)")
            .bind(name)
            .bind(enabled)
            .execute(pool)
            .await
            .expect("插入实体失败")
            .last_insert_rowid()
    }

    /// 插入一篇文章：`published_at` 取当前时间，确保落在 days 回溯窗口内
    async fn insert_scored_article(
        pool: &SqlitePool,
        title: &str,
        summary: &str,
        score: i64,
    ) -> i64 {
        sqlx::query(
            "INSERT INTO articles (feed_id, guid, link, title, summary, entity_score, published_at) \
             VALUES (1, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
        )
        .bind(title)
        .bind(format!("https://example.com/{}", title))
        .bind(title)
        .bind(summary)
        .bind(score)
        .execute(pool)
        .await
        .expect("插入文章失败")
        .last_insert_rowid()
    }

    /// 回归：`batch_score_entities` 必须真的把分数写进 `entity_score`
    ///
    /// 该函数此前用 `SELECT id, title, summary` 装进要求 18 列的 [`Article`]，
    /// 而 sqlx 的 `FromRow` 按列名取值，缺 `feed_id` 直接 `ColumnNotFound`：
    /// 命令长期报错、全库 `entity_score` 恒为 NULL，批量提取的
    /// `entity_score >= 阈值` 于是永远命中 0 篇——偏偏任务卡片只显示
    /// "处理 0 篇"，健康度还是 success，所以这条链路坏了很久都没被发现。
    /// 本用例把"分数确实落库"钉死，避免同类静默失效再次发生。
    ///
    /// 同时验证口径：停用的实体不参与命中，未命中的文章写 0 分
    /// （写 0 而非留 NULL，是为了"给新实体补评分"时能被重新扫到）。
    #[tokio::test]
    async fn batch_score_writes_scores_to_articles() {
        let pool = setup_digest().await;
        insert_entity(&pool, "OpenAI", 1).await;
        insert_entity(&pool, "已停用实体", 0).await;

        // 分数传 0：走"待评分"分支（NULL 与 0 都算未评分）
        let hit = insert_scored_article(&pool, "OpenAI 发布新模型", "能力提升", 0).await;
        let miss = insert_scored_article(&pool, "无关标题", "无关摘要", 0).await;

        let updated = batch_score_entities(&pool, Vec::new())
            .await
            .expect("评分不应失败");
        assert_eq!(updated, 2, "两篇待评分文章都应被回写");

        let hit_score: i64 = sqlx::query_scalar("SELECT entity_score FROM articles WHERE id = ?")
            .bind(hit)
            .fetch_one(&pool)
            .await
            .expect("读取命中文章分数失败");
        let miss_score: i64 = sqlx::query_scalar("SELECT entity_score FROM articles WHERE id = ?")
            .bind(miss)
            .fetch_one(&pool)
            .await
            .expect("读取未命中文章分数失败");

        assert_eq!(hit_score, 20, "命中 1 个启用实体 = 20 分");
        assert_eq!(miss_score, 0, "未命中写 0，而不是留空");
    }

    /// 分组按实体名命中，文章多的实体排在前面，组内保持评分倒序
    #[tokio::test]
    async fn digest_groups_by_entity_name_hit() {
        let pool = setup_digest().await;
        insert_entity(&pool, "OpenAI", 1).await;
        insert_entity(&pool, "腾讯", 1).await;

        // OpenAI 命中 2 篇（分数一高一低），腾讯命中 1 篇。
        // 刻意让两篇标题/摘要互不交叉：交叉命中会把"分组"变成两篇各归两组，
        // 那时断言的就变成"同一篇文章能出现在多个组里"，与本用例要测的顺序无关。
        insert_scored_article(&pool, "OpenAI 发布新模型", "模型能力提升", 80).await;
        insert_scored_article(&pool, "OpenAI 扩招", "招聘计划公布", 70).await;
        insert_scored_article(&pool, "腾讯财报", "营收增长", 60).await;
        // 低于阈值：不该出现在任何分组里
        insert_scored_article(&pool, "无关标题", "无关摘要", 20).await;

        let groups = get_high_score_articles_grouped(&pool, 7, 60)
            .await
            .expect("分组查询失败");

        let names: Vec<&str> = groups.iter().map(|g| g.entity.as_str()).collect();
        assert_eq!(names, vec!["OpenAI", "腾讯"], "应按命中文章数倒序排列");
        assert_eq!(groups[0].articles.len(), 2);
        // 组内按评分倒序：80 分在前、70 分在后
        assert_eq!(groups[0].articles[0].title, "OpenAI 发布新模型");
        assert_eq!(groups[0].articles[1].title, "OpenAI 扩招");
        assert_eq!(groups[1].articles[0].title, "腾讯财报");
    }

    /// 同一篇文章可以同时归入多个实体分组（同时提到两个关注对象）
    ///
    /// 这与评分口径一致：分数本就是"命中实体数 × 20"的累加，
    /// 若分组只允许归一个组，分数与分组就会互相矛盾。
    #[tokio::test]
    async fn digest_article_can_join_multiple_groups() {
        let pool = setup_digest().await;
        insert_entity(&pool, "OpenAI", 1).await;
        insert_entity(&pool, "腾讯", 1).await;
        insert_scored_article(&pool, "OpenAI 与腾讯合作", "双方共建数据中心", 60).await;

        let groups = get_high_score_articles_grouped(&pool, 7, 60)
            .await
            .expect("分组查询失败");

        assert_eq!(groups.len(), 2, "两个实体都应各自成组");
        assert!(groups.iter().all(|g| g.articles.len() == 1));
    }

    /// 命中的判定与评分口径一致：摘要里的命中同样算命中
    #[tokio::test]
    async fn digest_hit_covers_summary_field() {
        let pool = setup_digest().await;
        insert_entity(&pool, "英伟达", 1).await;
        // 只有摘要提到英伟达，标题里没有
        insert_scored_article(&pool, "算力周报", "英伟达出货量创新高", 60).await;

        let groups = get_high_score_articles_grouped(&pool, 7, 60)
            .await
            .expect("分组查询失败");

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].articles.len(), 1);
    }

    /// 禁用实体不参与分组；空名实体不能"命中一切"
    #[tokio::test]
    async fn digest_skips_disabled_and_blank_entities() {
        let pool = setup_digest().await;
        insert_entity(&pool, "被禁用的实体", 0).await;
        insert_entity(&pool, "  ", 1).await;
        insert_scored_article(&pool, "被禁用的实体 相关新闻", "内容", 60).await;

        let groups = get_high_score_articles_grouped(&pool, 7, 60)
            .await
            .expect("分组查询失败");

        assert!(groups.is_empty(), "不该产生任何分组: {:?}", groups);
    }

    /// 没有已启用实体时直接返回空集合（前端据此显示引导语）
    #[tokio::test]
    async fn digest_returns_empty_without_entities() {
        let pool = setup_digest().await;
        insert_scored_article(&pool, "任意标题", "任意摘要", 90).await;

        let groups = get_high_score_articles_grouped(&pool, 7, 60)
            .await
            .expect("分组查询失败");

        assert!(groups.is_empty());
    }

    /// 排序标识解析：已知值各归其位，未知值回退「最新」
    #[test]
    fn article_sort_from_key_falls_back_to_latest() {
        assert_eq!(ArticleSort::from_key("latest"), ArticleSort::Latest);
        assert_eq!(ArticleSort::from_key("score"), ArticleSort::Score);
        assert_eq!(ArticleSort::from_key("title"), ArticleSort::Title);
        // 大小写与空白容错
        assert_eq!(ArticleSort::from_key(" SCORE "), ArticleSort::Score);
        // 前端传脏值时保持默认顺序，而不是报错或落到未定义的 SQL 片段上
        assert_eq!(ArticleSort::from_key("drop table"), ArticleSort::Latest);
        assert_eq!(ArticleSort::from_key(""), ArticleSort::Latest);
    }

    // ─── 阅读进度 ─────────────────────────────────────────────────────────────

    /// 读回一篇文章的阅读进度
    async fn read_progress(pool: &SqlitePool, id: i64) -> f64 {
        sqlx::query_scalar("SELECT read_progress FROM articles WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("读取阅读进度失败")
    }

    /// 越界比例被夹进 `[0, 1]`
    ///
    /// 存的是"可滚距离的百分比"，越界值只会来自窗口尺寸突变的竞态或前端算错；
    /// 落库前必须收紧，否则恢复时 `scrollTop` 会算成负数或超出底部。
    #[tokio::test]
    async fn set_progress_clamps_out_of_range() {
        let pool = setup_digest().await;
        let id = insert_scored_article(&pool, "长文", "摘要", 0).await;

        Article::set_progress(&pool, id, 1.5)
            .await
            .expect("写入不应失败");
        assert_eq!(read_progress(&pool, id).await, 1.0);

        Article::set_progress(&pool, id, -0.5)
            .await
            .expect("写入不应失败");
        assert_eq!(read_progress(&pool, id).await, 0.0);
    }

    /// NaN 与 ±∞ 一律归零
    ///
    /// 这类值一旦落库，后续 `ABS(read_progress - ?) > 0.01` 对 NULL/NaN 的判定
    /// 会让该文章的进度再也写不进去（静默失效），所以必须在入口拦掉。
    #[tokio::test]
    async fn set_progress_resets_non_finite_to_zero() {
        let pool = setup_digest().await;
        let id = insert_scored_article(&pool, "长文", "摘要", 0).await;

        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            Article::set_progress(&pool, id, 0.5)
                .await
                .expect("写入不应失败");
            Article::set_progress(&pool, id, bad)
                .await
                .expect("写入不应失败");
            assert_eq!(read_progress(&pool, id).await, 0.0, "{} 应归零", bad);
        }
    }

    /// 变化小于阈值时跳过写库
    ///
    /// 滚动是每 800ms 上报一次的高频操作，微小变化重复写只会白白拖动
    /// `updated_at`；阈值 0.01 约等于一屏的百分之一，用户感知不到差别。
    #[tokio::test]
    async fn set_progress_skips_negligible_change() {
        let pool = setup_digest().await;
        let id = insert_scored_article(&pool, "长文", "摘要", 0).await;

        Article::set_progress(&pool, id, 0.50)
            .await
            .expect("写入不应失败");
        Article::set_progress(&pool, id, 0.505)
            .await
            .expect("写入不应失败");
        assert_eq!(read_progress(&pool, id).await, 0.50, "差值 0.005 应被跳过");

        Article::set_progress(&pool, id, 0.52)
            .await
            .expect("写入不应失败");
        assert_eq!(read_progress(&pool, id).await, 0.52, "差值 0.02 应写入");
    }

    // ─── 标签 ────────────────────────────────────────────────────────────────

    /// 同名（ASCII 大小写不敏感、首尾空格）复用同一标签，且复用时不覆盖既有颜色
    #[tokio::test]
    async fn tag_create_reuses_same_name_case_insensitively() {
        let pool = setup_digest().await;

        let a = Tag::create(&pool, "  AI  ", "green").await.expect("创建失败");
        let b = Tag::create(&pool, "ai", "red").await.expect("创建失败");
        assert_eq!(a, b, "大小写与首尾空格应视为同一标签");

        let list = Tag::list_all(&pool).await.expect("列举失败");
        assert_eq!(list.len(), 1, "只应存在一个标签");
        assert_eq!(list[0].color, "green", "颜色只在新建时生效，复用时不得覆盖");

        assert!(Tag::create(&pool, "   ", "").await.is_err(), "空名必须被拒绝");
    }

    /// 打标是"整组覆盖"：只传部分标签时其余必须消失，空数组即清空
    #[tokio::test]
    async fn tag_set_for_article_replaces_whole_set() {
        let pool = setup_digest().await;
        let art = insert_scored_article(&pool, "标题", "摘要", 0).await;
        let a = Tag::create(&pool, "A", "").await.expect("创建失败");
        let b = Tag::create(&pool, "B", "").await.expect("创建失败");

        Tag::set_for_article(&pool, art, &[a, b]).await.expect("打标失败");
        let map = Tag::for_articles(&pool, &[art]).await.expect("批量取标签失败");
        assert_eq!(map[&art].len(), 2, "两个标签都应挂上");

        Tag::set_for_article(&pool, art, &[b]).await.expect("打标失败");
        let map = Tag::for_articles(&pool, &[art]).await.expect("批量取标签失败");
        let names: Vec<&str> = map[&art].iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["B"], "覆盖语义下 A 必须被摘掉");

        Tag::set_for_article(&pool, art, &[]).await.expect("清空失败");
        let map = Tag::for_articles(&pool, &[art]).await.expect("批量取标签失败");
        assert!(map.get(&art).is_none(), "清空后该文章不应再出现在分组结果中");

        // 计数随关联变化；无人使用的标签也要出现在列表里（计数 0），否则界面无法管理空标签
        Tag::set_for_article(&pool, art, &[a]).await.expect("打标失败");
        let list = Tag::list_all(&pool).await.expect("列举失败");
        let tag_a = list.iter().find(|t| t.name == "A").expect("应含 A");
        let tag_b = list.iter().find(|t| t.name == "B").expect("应含 B");
        assert_eq!(tag_a.article_count, 1);
        assert_eq!(tag_b.article_count, 0);
    }

    /// 删除标签：关联行级联清除，引用它的规则被清空参数但保留动作
    #[tokio::test]
    async fn tag_delete_cascades_and_clears_rule_arg() {
        let pool = setup_digest().await;
        let art = insert_scored_article(&pool, "标题", "摘要", 0).await;
        let id = Tag::create(&pool, "AI", "").await.expect("创建失败");
        Tag::set_for_article(&pool, art, &[id]).await.expect("打标失败");
        let rule = FilterRule::create(
            &pool, "规则", "all", 0, "title", "contains", "x", "tag", "AI", true, 0,
        )
        .await
        .expect("建规则失败");

        Tag::delete(&pool, id).await.expect("删除失败");

        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM article_tags")
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        assert_eq!(left, 0, "关联行应被外键级联删除");

        let (action, arg): (String, String) =
            sqlx::query_as("SELECT action, action_arg FROM filters WHERE id = ?")
                .bind(rule)
                .fetch_one(&pool)
                .await
                .expect("读规则失败");
        assert_eq!(action, "tag", "规则本身保留，不得被偷偷改成别的动作");
        assert_eq!(arg, "", "参数清空，引擎据此跳过该规则");
    }

    /// 重命名标签必须同步引用它的规则，否则下次刷新会重建一个旧名标签
    #[tokio::test]
    async fn tag_rename_updates_referencing_rules() {
        let pool = setup_digest().await;
        let id = Tag::create(&pool, "旧名", "").await.expect("创建失败");
        let rule = FilterRule::create(
            &pool, "规则", "all", 0, "title", "contains", "x", "tag", "旧名", true, 0,
        )
        .await
        .expect("建规则失败");

        Tag::update(&pool, id, "新名", "sky").await.expect("改名失败");
        let arg: String = sqlx::query_scalar("SELECT action_arg FROM filters WHERE id = ?")
            .bind(rule)
            .fetch_one(&pool)
            .await
            .expect("读规则失败");
        assert_eq!(arg, "新名", "规则参数必须跟着改名");

        // 重名要给可读中文错误，而不是把 sqlx 的 UNIQUE 原文抛给用户
        Tag::create(&pool, "另一个", "").await.expect("创建失败");
        let err = Tag::update(&pool, id, "另一个", "").await.expect_err("重名应报错");
        assert!(err.contains("同名"), "错误信息应可读，实际: {}", err);
    }

    /// 规则引擎的「打标签」动作：命中即自动建标签并挂上，未命中的不挂
    #[tokio::test]
    async fn batch_insert_applies_tag_rule() {
        let pool = setup_digest().await;
        let filters = vec![FilterRule {
            id: 0,
            name: "打标".to_string(),
            enabled: true,
            scope: "all".to_string(),
            scope_id: 0,
            field: "title".to_string(),
            op: "contains".to_string(),
            value: "AI".to_string(),
            action: "tag".to_string(),
            action_arg: "人工智能".to_string(),
            priority: 0,
            created_at: Utc::now(),
        }];
        let item = |guid: &str, title: &str, n: usize| crate::rss::ParsedItem {
            guid: guid.to_string(),
            title: title.to_string(),
            link: format!("https://example.com/{}", n),
            summary: String::new(),
            content: String::new(),
            author: String::new(),
            published_at: Utc::now(),
        };
        let items = vec![item("g1", "AI 新进展", 1), item("g2", "无关标题", 2)];

        let inserted = Article::batch_insert(&pool, 1, "测试源", 0, &filters, &items)
            .await
            .expect("入库失败");
        assert_eq!(inserted, 2);

        let list = Tag::list_all(&pool).await.expect("列举失败");
        assert_eq!(list.len(), 1, "标签不存在时应自动创建");
        assert_eq!(list[0].name, "人工智能");
        assert_eq!(list[0].article_count, 1, "只有标题命中的那篇该被打标");

        // 空参数的标签规则必须被跳过，绝不能创建一个空名标签
        let empty_arg = vec![FilterRule {
            action_arg: String::new(),
            ..filters[0].clone()
        }];
        let more = vec![item("g3", "AI 又来了", 3)];
        Article::batch_insert(&pool, 1, "测试源", 0, &empty_arg, &more)
            .await
            .expect("入库失败");
        assert_eq!(
            Tag::list_all(&pool).await.expect("列举失败").len(),
            1,
            "参数为空的规则应被跳过"
        );
    }

    /// 列表按标签过滤、详情与列表都挂载标签、批量已读只作用于标签范围
    #[tokio::test]
    async fn tag_scope_applies_to_list_and_mark_all_read() {
        let pool = setup_digest().await;
        let tag = Tag::create(&pool, "T1", "").await.expect("创建失败");
        let tagged = insert_scored_article(&pool, "有标签", "摘要", 0).await;
        let other = insert_scored_article(&pool, "无标签", "摘要", 0).await;
        Tag::set_for_article(&pool, tagged, &[tag]).await.expect("打标失败");

        let list = Article::list_by_feed(&pool, None, Some(tag), None, None, ArticleSort::Latest, 50, 0, None)
            .await
            .expect("列表查询失败");
        assert_eq!(list.len(), 1, "只应返回带该标签的文章");
        assert_eq!(list[0].id, tagged);
        assert_eq!(list[0].tags.len(), 1, "列表查询必须挂载标签供 chip 渲染");

        let detail = Article::get_by_id(&pool, tagged)
            .await
            .expect("详情查询失败")
            .expect("文章应存在");
        assert_eq!(detail.tags.len(), 1, "详情同样要挂载标签");

        let affected = Article::mark_all_read(&pool, None, Some(tag))
            .await
            .expect("批量已读失败");
        assert_eq!(affected, 1, "只应标记标签范围内的 1 篇");
        let other_read: i64 = sqlx::query_scalar("SELECT is_read FROM articles WHERE id = ?")
            .bind(other)
            .fetch_one(&pool)
            .await
            .expect("读取已读状态失败");
        assert_eq!(other_read, 0, "标签范围外的文章不得被误标已读");
    }

    /// 书签范围：`Some(true)` 只返回已收藏项，`None` / `Some(false)` 均不过滤
    #[tokio::test]
    async fn bookmark_scope_filters_list() {
        let pool = setup_digest().await;
        let marked = insert_scored_article(&pool, "被收藏", "摘要", 0).await;
        insert_scored_article(&pool, "没收藏 A", "摘要", 0).await;
        insert_scored_article(&pool, "没收藏 B", "摘要", 0).await;

        // 收藏状态走真实命令路径，顺带验证 toggle 的返回值语义
        let now = Article::toggle_bookmark(&pool, marked).await.expect("收藏失败");
        assert!(now, "切换后应处于已收藏状态");

        let only = Article::list_by_feed(&pool, None, None, Some(true), None, ArticleSort::Latest, 50, 0, None)
            .await
            .expect("书签查询失败");
        assert_eq!(only.len(), 1, "书签范围只应返回 1 篇");
        assert_eq!(only[0].id, marked);
        assert!(only[0].is_bookmarked, "返回项的收藏标记必须为真");

        // `None` 与 `Some(false)` 语义相同：都表示"不限书签"。
        // 这里刻意断言 `Some(false)` 也返回全部——若有人日后把 false 译成
        // `is_bookmarked = 0`，本断言会立刻失败，从而挡住这次语义分叉。
        for flag in [None, Some(false)] {
            let all = Article::list_by_feed(&pool, None, None, flag, None, ArticleSort::Latest, 50, 0, None)
                .await
                .expect("全量查询失败");
            assert_eq!(all.len(), 3, "flag={:?} 时不应过滤书签", flag);
        }
    }

    /// 用户偏好范围：'like' / 'skip' 均为**只显示该标记**的对称语义，
    /// 未标记（NULL）与 `None`（不过滤）互不干扰
    ///
    /// 此前 skip 分支曾写成 `!= 'skip' AND IS NOT NULL`（实际只显示 liked，
    /// 与工具栏「仅显示跳过的」按钮语义矛盾），本用例把对称口径钉死。
    #[tokio::test]
    async fn user_preference_scope_filters_list() {
        let pool = setup_digest().await;
        let liked = insert_scored_article(&pool, "被喜欢", "摘要", 0).await;
        let skipped = insert_scored_article(&pool, "被跳过", "摘要", 0).await;
        let untouched = insert_scored_article(&pool, "未标记", "摘要", 0).await;

        Article::set_user_preference(&pool, liked, "like")
            .await
            .expect("设置喜欢失败");
        Article::set_user_preference(&pool, skipped, "skip")
            .await
            .expect("设置跳过失败");

        // 'like'：只返回被喜欢的 1 篇
        let only_liked = Article::list_by_feed(&pool, None, None, None, None, ArticleSort::Latest, 50, 0, Some("like"))
            .await
            .expect("like 查询失败");
        assert_eq!(only_liked.len(), 1, "like 范围只应返回 1 篇");
        assert_eq!(only_liked[0].id, liked);

        // 'skip'：只返回被跳过的 1 篇（对称语义；未标记篇不得混入）
        let only_skipped = Article::list_by_feed(&pool, None, None, None, None, ArticleSort::Latest, 50, 0, Some("skip"))
            .await
            .expect("skip 查询失败");
        assert_eq!(only_skipped.len(), 1, "skip 范围只应返回 1 篇");
        assert_eq!(only_skipped[0].id, skipped, "未标记与被喜欢的文章不得出现在 skip 范围");

        // None（不过滤）：3 篇全部返回
        let all = Article::list_by_feed(&pool, None, None, None, None, ArticleSort::Latest, 50, 0, None)
            .await
            .expect("全量查询失败");
        assert_eq!(all.len(), 3, "未指定偏好不应过滤任何文章");

        // 取消标记（空串 → NULL）后，skip 范围不再返回它
        Article::set_user_preference(&pool, skipped, "")
            .await
            .expect("取消标记失败");
        let after_clear = Article::list_by_feed(&pool, None, None, None, None, ArticleSort::Latest, 50, 0, Some("skip"))
            .await
            .expect("取消后查询失败");
        assert!(after_clear.is_empty(), "取消标记后 skip 范围应为空");
        // 全量仍在（未标记不等于消失）
        let still = Article::list_by_feed(&pool, None, None, None, None, ArticleSort::Latest, 50, 0, None)
            .await
            .expect("全量查询失败");
        assert_eq!(still.len(), 3);
        assert_eq!(still.iter().filter(|a| a.id == untouched).count(), 1, "未标记文章不受影响");
    }

    /// 书签范围可与标签范围叠加（AND），且分页在过滤之后生效
    #[tokio::test]
    async fn bookmark_scope_combines_with_tag_and_paginates() {
        let pool = setup_digest().await;
        let tag = Tag::create(&pool, "T1", "").await.expect("创建失败");

        let hit = insert_scored_article(&pool, "书签+标签", "摘要", 0).await;
        let bm_only = insert_scored_article(&pool, "只有书签", "摘要", 0).await;
        let tag_only = insert_scored_article(&pool, "只有标签", "摘要", 0).await;
        Tag::set_for_article(&pool, hit, &[tag]).await.expect("打标失败");
        Tag::set_for_article(&pool, tag_only, &[tag]).await.expect("打标失败");
        Article::toggle_bookmark(&pool, hit).await.expect("收藏失败");
        Article::toggle_bookmark(&pool, bm_only).await.expect("收藏失败");

        let both = Article::list_by_feed(&pool, None, Some(tag), Some(true), None, ArticleSort::Latest, 50, 0, None)
            .await
            .expect("组合查询失败");
        assert_eq!(both.len(), 1, "两个范围应取交集（AND）");
        assert_eq!(both[0].id, hit);

        // limit 作用在过滤之后：书签共 2 篇，取第 1 页 1 条后应仍有下一页
        let page1 = Article::list_by_feed(&pool, None, None, Some(true), None, ArticleSort::Latest, 1, 0, None)
            .await
            .expect("分页查询失败");
        assert_eq!(page1.len(), 1, "每页 1 条");
        let page2 = Article::list_by_feed(&pool, None, None, Some(true), None, ArticleSort::Latest, 1, 1, None)
            .await
            .expect("分页查询失败");
        assert_eq!(page2.len(), 1, "第 2 页应还有 1 条（证明 limit 在过滤之后）");
        assert_ne!(page1[0].id, page2[0].id, "两页不得返回同一条");
    }

    /// 未读范围只返回未读文章，且 `None` / `Some(false)` 语义相同（都不过滤）
    #[tokio::test]
    async fn unread_scope_filters_list() {
        let pool = setup_digest().await;
        let unread_a = insert_scored_article(&pool, "未读 A", "摘要", 0).await;
        let unread_b = insert_scored_article(&pool, "未读 B", "摘要", 0).await;
        let read = insert_scored_article(&pool, "已读 C", "摘要", 0).await;
        Article::set_read(&pool, read, true).await.expect("标记已读失败");

        let only =
            Article::list_by_feed(&pool, None, None, None, Some(true), ArticleSort::Latest, 50, 0, None)
                .await
                .expect("未读查询失败");
        assert_eq!(only.len(), 2, "未读范围只应返回 2 篇");
        let ids: Vec<i64> = only.iter().map(|a| a.id).collect();
        assert!(
            ids.contains(&unread_a) && ids.contains(&unread_b),
            "两篇未读都应出现在结果里"
        );
        assert!(!ids.contains(&read), "已读文章不得出现在未读结果里");

        // 同书签：`None` 与 `Some(false)` 都表示"不限未读"。
        // 若有人日后把 false 译成 `is_read = 1`，本断言会立刻失败，挡住这次语义分叉。
        for flag in [None, Some(false)] {
            let all =
                Article::list_by_feed(&pool, None, None, None, flag, ArticleSort::Latest, 50, 0, None)
                    .await
                    .expect("全量查询失败");
            assert_eq!(all.len(), 3, "flag={:?} 时不应过滤未读", flag);
        }
    }

    /// 未读范围可与标签范围叠加（AND）
    #[tokio::test]
    async fn unread_scope_combines_with_tag() {
        let pool = setup_digest().await;
        let tag = Tag::create(&pool, "T2", "").await.expect("创建失败");

        let hit = insert_scored_article(&pool, "未读+标签", "摘要", 0).await;
        // 这篇未读但不带标签，用来验证范围叠加是"交集"而非"并集"
        insert_scored_article(&pool, "只有未读", "摘要", 0).await;
        let read_tagged = insert_scored_article(&pool, "已读+标签", "摘要", 0).await;
        Tag::set_for_article(&pool, hit, &[tag]).await.expect("打标失败");
        Tag::set_for_article(&pool, read_tagged, &[tag])
            .await
            .expect("打标失败");
        Article::set_read(&pool, read_tagged, true)
            .await
            .expect("标记已读失败");

        let both =
            Article::list_by_feed(&pool, None, Some(tag), None, Some(true), ArticleSort::Latest, 50, 0, None)
                .await
                .expect("组合查询失败");
        assert_eq!(both.len(), 1, "未读 ∩ 标签 应只有 1 篇");
        assert_eq!(both[0].id, hit);
    }

    /// 未读计数命令：口径与列表一致，且支持按源 / 按标签限定
    #[tokio::test]
    async fn count_unread_respects_scope() {
        let pool = setup_digest().await;
        let tag = Tag::create(&pool, "T3", "").await.expect("创建失败");

        let tagged_unread = insert_scored_article(&pool, "标签未读", "摘要", 0).await;
        insert_scored_article(&pool, "普通未读", "摘要", 0).await;
        let tagged_read = insert_scored_article(&pool, "标签已读", "摘要", 0).await;
        Tag::set_for_article(&pool, tagged_unread, &[tag])
            .await
            .expect("打标失败");
        Tag::set_for_article(&pool, tagged_read, &[tag])
            .await
            .expect("打标失败");
        Article::set_read(&pool, tagged_read, true)
            .await
            .expect("标记已读失败");

        // 3 篇里 2 篇未读
        let all = Article::count_unread(&pool, None, None)
            .await
            .expect("计数失败");
        assert_eq!(all, 2, "全库未读应为 2");

        // 夹具里所有文章都挂在 feed 1 上，故按源计数应与全库一致
        let by_feed = Article::count_unread(&pool, Some(1), None)
            .await
            .expect("计数失败");
        assert_eq!(by_feed, 2, "源 1 的未读数应与全库一致");

        // 标签下共 2 篇，其中只有 1 篇未读
        let by_tag = Article::count_unread(&pool, None, Some(tag))
            .await
            .expect("计数失败");
        assert_eq!(by_tag, 1, "标签范围内的未读应为 1");
    }

    // ─── 实体关系图 ───────────────────────────────────────────────────────────

    /// 插入一篇指定发布时间的文章（用于验证关系图的时间窗过滤）
    async fn insert_article_dated(pool: &SqlitePool, title: &str, summary: &str, days_ago: i64) -> i64 {
        sqlx::query(
            "INSERT INTO articles (feed_id, guid, link, title, summary, entity_score, published_at) \
             VALUES (1, ?, ?, ?, ?, 0, datetime('now', ?))",
        )
        .bind(title)
        .bind(format!("https://example.com/{}", title))
        .bind(title)
        .bind(summary)
        .bind(format!("-{} days", days_ago))
        .execute(pool)
        .await
        .expect("插入文章失败")
        .last_insert_rowid()
    }

    /// 共现统计：同篇命中的两个实体连边，只命中过一篇的实体不连线；
    /// 未出现在任何边上的孤立实体不进节点集
    #[tokio::test]
    async fn entity_graph_counts_cooccurrence() {
        let pool = setup_digest().await;
        insert_entity(&pool, "OpenAI", 1).await;
        insert_entity(&pool, "Meta", 1).await;
        insert_entity(&pool, "腾讯", 1).await;

        // 中文与英文混合命中：验证子串匹配对非 ASCII 同样成立
        insert_scored_article(&pool, "OpenAI 与 Meta 联合发布", "两家公司合作", 0).await;
        insert_scored_article(&pool, "OpenAI 再发新模型", "能力提升", 0).await;
        insert_scored_article(&pool, "腾讯发布财报", "营收增长", 0).await;

        let graph = Entity::graph(&pool, 0, 1, 0).await.expect("成图失败");

        assert_eq!(graph.scanned_articles, 3);
        assert!(!graph.truncated, "未设上限不应标记截断");
        assert_eq!(graph.edges.len(), 1, "只有 OpenAI 与 Meta 共现过");
        assert_eq!(graph.edges[0].weight, 1);

        // 节点按实体 id 升序；腾讯无共现被剔除
        let names: Vec<&str> = graph.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["OpenAI", "Meta"], "孤立实体不进节点集");
        assert_eq!(graph.nodes[0].article_count, 2, "OpenAI 命中两篇");
        assert_eq!(graph.nodes[1].article_count, 1, "Meta 只命中一篇");
    }

    /// 最小共现数过滤：阈值抬高到 2 后，权重为 1 的边被丢弃，
    /// 两个端点随之失去"出现在边上"的资格，图退化为空
    #[tokio::test]
    async fn entity_graph_min_weight_filters_edges_and_nodes() {
        let pool = setup_digest().await;
        insert_entity(&pool, "A公司", 1).await;
        insert_entity(&pool, "B公司", 1).await;
        insert_scored_article(&pool, "A公司 与 B公司 签约", "合作", 0).await;

        let loose = Entity::graph(&pool, 0, 1, 0).await.expect("成图失败");
        assert_eq!(loose.edges.len(), 1);
        assert_eq!(loose.nodes.len(), 2);

        let strict = Entity::graph(&pool, 0, 2, 0).await.expect("成图失败");
        assert!(strict.edges.is_empty(), "权重 1 低于阈值 2，应被丢弃");
        assert!(strict.nodes.is_empty(), "没有边就不该留下悬空节点");
    }

    /// 边数上限：按权重降序截断，并置 `truncated` 供前端提示；
    /// 权重相同时按端点 id 升序，保证顺序稳定可断言
    #[tokio::test]
    async fn entity_graph_truncates_edges_by_weight() {
        let pool = setup_digest().await;
        insert_entity(&pool, "A", 1).await;
        insert_entity(&pool, "B", 1).await;
        insert_entity(&pool, "C", 1).await;

        // A–B 共现两篇（权重 2），A–C 共现一篇（权重 1）
        insert_scored_article(&pool, "A 与 B 之一", "x", 0).await;
        insert_scored_article(&pool, "A 与 B 之二", "x", 0).await;
        insert_scored_article(&pool, "A 与 C 之一", "x", 0).await;

        let graph = Entity::graph(&pool, 0, 1, 1).await.expect("成图失败");
        assert!(graph.truncated, "边数超上限应标记截断");
        assert_eq!(graph.edges.len(), 1, "上限 1 只保留权重最高的那条");
        assert_eq!(graph.edges[0].weight, 2, "保留的应是权威重最高的边");
        // 截断后只剩 A–B，C 不再出现在任何边上
        let names: Vec<&str> = graph.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["A", "B"]);
    }

    /// 时间窗：窗口外的老文章不参与统计（`days <= 0` 表示不限时间）
    #[tokio::test]
    async fn entity_graph_respects_time_window() {
        let pool = setup_digest().await;
        insert_entity(&pool, "旧实体", 1).await;
        insert_entity(&pool, "新实体", 1).await;

        insert_article_dated(&pool, "旧实体 与 新实体 的旧闻", "一年前", 300).await;
        insert_article_dated(&pool, "旧实体 与 新实体 的近闻", "昨天", 1).await;

        // 近 30 天：只剩那篇近闻，边权为 1
        let recent = Entity::graph(&pool, 30, 1, 0).await.expect("成图失败");
        assert_eq!(recent.scanned_articles, 1, "只应扫描窗口内的文章");
        assert_eq!(recent.edges[0].weight, 1);

        // 不限时间：两篇都计入，边权为 2
        let all = Entity::graph(&pool, 0, 1, 0).await.expect("成图失败");
        assert_eq!(all.scanned_articles, 2);
        assert_eq!(all.edges[0].weight, 2);
    }

    /// 没有启用实体时返回空图而不是 panic（也不应触发除零等算术问题）
    #[tokio::test]
    async fn entity_graph_without_entities_is_empty() {
        let pool = setup_digest().await;
        insert_scored_article(&pool, "任意文章", "任意摘要", 0).await;
        // 停用的实体不参与成图（list_enabled 过滤）
        insert_entity(&pool, "已停用", 0).await;

        let graph = Entity::graph(&pool, 0, 1, 0).await.expect("成图不应失败");
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
        assert_eq!(graph.scanned_articles, 0, "没有实体就不必扫描文章");
    }

    /// 插入一条研究事件并返回事件 ID（交叉验证用例的专用夹具）
    ///
    /// 直接裸插 `research_events`：被测函数 `list_cross_reference` 走的是
    /// JOIN 投影（articles + feeds + event_sentiments），夹具越是绕开业务层，
    /// 断言越贴近 SQL 本身的口径。
    async fn insert_research_event(
        pool: &SqlitePool,
        article_id: i64,
        entity_id: i64,
        fact: &str,
        days_ago: i64,
    ) -> i64 {
        sqlx::query(
            "INSERT INTO research_events (article_id, entity_id, event_type, fact, evidence, created_at) \
             VALUES (?, ?, 'strategy', ?, '', datetime('now', '-' || ? || ' days'))",
        )
        .bind(article_id)
        .bind(entity_id)
        .bind(fact)
        .bind(days_ago)
        .execute(pool)
        .await
        .expect("插入事件失败")
        .last_insert_rowid()
    }

    /// 多源交叉验证：按实体取回不同来源的报道行，JOIN 带出订阅源名与情感；
    /// 时间窗外的老事件被过滤；无情感记录时回退 neutral
    #[tokio::test]
    async fn cross_reference_joins_source_and_filters_by_days() {
        let pool = setup_digest().await;
        let entity = insert_entity(&pool, "OpenAI", 1).await;
        let other = insert_entity(&pool, "别的实体", 1).await;

        // 两个订阅源各一篇报道：source_name 必须各自带出
        sqlx::query("INSERT INTO feeds (id, name, url) VALUES (2, '科技日报', 'https://tech.example.com/feed')")
            .execute(&pool)
            .await
            .expect("插入第二订阅源失败");
        let a1 = insert_scored_article(&pool, "OpenAI 发布新模型", "摘要", 0).await; // feed 1 = 测试源
        let a2: i64 = sqlx::query(
            "INSERT INTO articles (feed_id, guid, link, title, summary, published_at) \
             VALUES (2, 'g2', 'https://tech.example.com/2', 'OpenAI 融资十亿', '摘要', CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("插入第二源文章失败")
        .last_insert_rowid();

        // 三条事件：近事件 ×2（不同源，错开 1 天保证排序确定）+ 一条 300 天前的老事件 + 一条别人实体的事件
        let e1 = insert_research_event(&pool, a1, entity, "发布新模型", 1).await;
        let e2 = insert_research_event(&pool, a2, entity, "完成新一轮融资", 0).await;
        insert_research_event(&pool, a1, entity, "三百日前的旧闻", 300).await;
        insert_research_event(&pool, a1, other, "别的实体的事件", 0).await;

        // 只给 e1 写 negative 情感：e2 应回退 neutral
        EventSentiment::upsert(&pool, e1, "negative", 0.9)
            .await
            .expect("写情感失败");

        // 不限时间：该实体 3 条（含老事件），别人的事件不得混入
        let all = ResearchEvent::list_cross_reference(&pool, entity, 0, 50)
            .await
            .expect("交叉验证查询失败");
        assert_eq!(all.len(), 3, "别的实体的事件不得出现在结果里");
        // 按入库时间倒序：两条今日事件在前，老事件沉底
        assert_eq!(all[0].event_id, e2, "最新事件应排最前");
        assert_eq!(all[2].fact, "三百日前的旧闻");

        // 来源名与情感：e2 来自第二源且无情感记录 → source=科技日报 / neutral
        assert_eq!(all[0].source_name.as_deref(), Some("科技日报"));
        assert_eq!(all[0].sentiment, "neutral", "无情感记录应回退 neutral");
        assert_eq!(all[0].article_title.as_deref(), Some("OpenAI 融资十亿"));
        // e1 来自测试源且有 negative 情感
        assert_eq!(all[1].event_id, e1);
        assert_eq!(all[1].source_name.as_deref(), Some("测试源"));
        assert_eq!(all[1].sentiment, "negative");

        // 近 30 天窗口：老事件被过滤，只剩 2 条
        let recent = ResearchEvent::list_cross_reference(&pool, entity, 30, 50)
            .await
            .expect("交叉验证查询失败");
        assert_eq!(recent.len(), 2, "300 天前的事件应在时间窗外");

        // limit 上限：截断后只留最新的 1 条
        let capped = ResearchEvent::list_cross_reference(&pool, entity, 0, 1)
            .await
            .expect("交叉验证查询失败");
        assert_eq!(capped.len(), 1);
        assert_eq!(capped[0].event_id, e2, "截断应保留最新的事件");

        // 没有任何事件的实体返回空列表而不是错误
        let empty_entity = insert_entity(&pool, "无事件实体", 1).await;
        let empty = ResearchEvent::list_cross_reference(&pool, empty_entity, 0, 50)
            .await
            .expect("空实体查询不应失败");
        assert!(empty.is_empty());
    }

    /// 多源交叉验证：文章被清理（外键级联删除）后行内文章字段回退 NULL
    ///
    /// LEFT JOIN 的存在意义就在这里：物理删除文章时事件随级联删掉，
    /// 但若未来改成软删（文章行还在、feed 不在），查询也不应报错。
    #[tokio::test]
    async fn cross_reference_survives_missing_article() {
        let pool = setup_digest().await;
        let entity = insert_entity(&pool, "孤儿实体", 1).await;
        let article = insert_scored_article(&pool, "将被删除的文章", "摘要", 0).await;
        insert_research_event(&pool, article, entity, "孤儿事件", 0).await;

        // 级联删除文章 → 事件随之消失，结果为空而非报错
        sqlx::query("DELETE FROM articles WHERE id = ?")
            .bind(article)
            .execute(&pool)
            .await
            .expect("删除文章失败");
        let rows = ResearchEvent::list_cross_reference(&pool, entity, 0, 50)
            .await
            .expect("级联删除后的查询不应失败");
        assert!(rows.is_empty(), "事件随文章级联删除，应无返回行");
    }
}
