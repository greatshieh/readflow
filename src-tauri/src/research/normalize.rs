//! 实体归一化与事件落库
//!
//! 职责：把 [`ExtractedEvent`]（实体仅有名称）转换为 [`crate::db::ResearchEvent`]
//! （实体为归一化后的 `entity_id`）并幂等入库。
//!
//! 归一化规则（按序匹配）：
//! 1. canonical 名精确命中已有实体的主名（大小写不敏感）→ 复用该实体；
//! 2. canonical 名命中已有实体的别名 → 复用该实体；
//! 3. 未命中 → 新建实体，同时把模型给出的别称写入 `aliases` 字段。
//!
//! 复用已有实体时会做**别名合并**：模型本次给出的别称若不在实体别名表中，
//! 追加进去，让"字节跳动/字节/ByteDance"这类写法随着使用逐步收敛到同一实体。
//!
//! 全程只读一次实体表（[`EntityIndex`]）：一篇文章的多条事件共享同一份内存索引，
//! 批量内新建的实体也会立刻登记进去，避免"每条事件查一次全表"。

use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::db::{Entity, ResearchEvent};

use super::ExtractedEvent;

/// 「名称或别名 → 实体」的内存索引
///
/// # 为什么需要
/// 原实现对每条事件调一次 `Entity::find_by_canonical_or_alias`，而它内部本身就是
/// "全表载入 + 内存遍历"——一篇文章抽 10 条事件就要把实体表整读 10 遍。实体量级是
/// 个人订阅场景的百级，一次载入建索引后按 key 命中即可，同时批量内新建的实体也能
/// 立刻被后续事件复用（无需回查数据库）。
struct EntityIndex {
    /// 小写名称或别名 → 实体在 `entities` 中的下标
    by_key: HashMap<String, usize>,
    /// 全部实体（保持数据库中 id 升序，保证命中顺序与原实现一致）
    entities: Vec<Entity>,
}

impl EntityIndex {
    /// 载入全部实体并建立索引
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    ///
    /// # 返回值
    /// 可立即用于匹配的索引
    ///
    /// # 错误
    /// 实体读取失败时返回错误信息
    async fn load(pool: &SqlitePool) -> Result<Self, String> {
        let entities = Entity::list_all(pool).await?;
        let mut by_key = HashMap::new();
        for (idx, entity) in entities.iter().enumerate() {
            register(&mut by_key, &entity.name, idx);
            // 别名解析失败按"无别名"处理，不影响主名匹配
            if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&entity.aliases) {
                for alias in &aliases {
                    register(&mut by_key, alias, idx);
                }
            }
        }
        Ok(Self { by_key, entities })
    }

    /// 按名称或别名查找实体
    ///
    /// # 参数
    /// * `name` - 待匹配的名称（大小写不敏感）
    ///
    /// # 返回值
    /// 命中的实体；名称为空或未命中时返回 `None`
    fn get(&self, name: &str) -> Option<&Entity> {
        let key = name.trim().to_lowercase();
        if key.is_empty() {
            return None;
        }
        self.by_key.get(&key).map(|idx| &self.entities[*idx])
    }

    /// 登记一个新建的实体，使其在本次批量内即可被匹配到
    ///
    /// # 参数
    /// * `entity` - 刚写入数据库的实体
    fn push(&mut self, entity: Entity) {
        let idx = self.entities.len();
        register(&mut self.by_key, &entity.name, idx);
        if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&entity.aliases) {
            for alias in &aliases {
                register(&mut self.by_key, alias, idx);
            }
        }
        self.entities.push(entity);
    }
}

/// 把一个名称登记进索引
///
/// 已有的登记不被覆盖（`entry().or_insert()`）：这与原实现的命中顺序等价——
/// 它按 id 升序逐实体先比主名再比别名，先出现的实体先命中。空名一律跳过，
/// 否则会变成一个"命中一切"的通配 key。
///
/// # 参数
/// * `map` - 名称到实体下标的映射
/// * `name` - 主名或别名
/// * `idx` - 实体下标
fn register(map: &mut HashMap<String, usize>, name: &str, idx: usize) {
    let key = name.trim().to_lowercase();
    if !key.is_empty() {
        map.entry(key).or_insert(idx);
    }
}

/// 与 `rows`（已落库的事件）下标一一对应的附属数据（情感 + 财务）。
///
/// 用途：构建 `rows` 的同一循环里同步收集，使后续写入 `event_sentiments` /
/// `financial_data` 时能按同一下标索引对齐，**不再依赖 `events` 的原始下标**
/// （原实现用 `events` 下标索引 `rows`，但空 canonical 事件被跳过导致两个数组错位）。
struct EventExtras {
    sentiment: String,
    confidence: f64,
    financials: Option<Vec<(String, String)>>,
}

/// 把提取出的事件归一化实体后批量入库
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_id` - 来源文章 ID
/// * `events` - 模型提取出的事件（应已通过 [`super::extract`] 的枚举校验）
/// * `source_model` - 抽取所用模型标识，写入事件供溯源
///
/// # 返回值
/// 实际新插入的事件条数（去重后）
///
/// # 错误
/// 实体创建或事件入库失败时返回错误信息
pub async fn normalize_and_store_events(
    pool: &SqlitePool,
    article_id: i64,
    events: &[ExtractedEvent],
    source_model: &str,
) -> Result<u64, String> {
    let mut rows: Vec<ResearchEvent> = Vec::with_capacity(events.len());
    // 与 `rows` 下标一一对应的附属数据（情感 + 财务）。原实现对齐用的是 `events` 的下标，
    // 但 `rows` 已跳过空 canonical 的事件导致两个数组错位——财务/情感被挂到错误事件上。
    // 改为在构建 `rows` 的同一循环里同步收集，用同一下标索引天然对齐。
    let mut extras: Vec<EventExtras> = Vec::with_capacity(events.len());
    let mut index = EntityIndex::load(pool).await?;

    for event in events {
        let canonical = event.canonical_name.trim();
        // canonical 为空的事件无法归一化到任何实体，直接丢弃（前置校验已过滤空 fact，
        // 这里是最后一道防线：模型偶尔输出只有 evidence 的残缺条目）
        if canonical.is_empty() {
            continue;
        }

        // 归一化：canonical/别名命中即复用，未命中自动新建
        let entity = match index.get(canonical).cloned() {
            Some(existing) => {
                // 别名合并：本次模型给出的别称若不在已有别名表中则追加，
                // 使实体别名随使用逐渐完备
                merge_aliases(pool, &existing, &event.aliases).await?;
                existing
            }
            None => {
                let entity_type = if event.entity_type.trim().is_empty() {
                    "company"
                } else {
                    event.entity_type.trim()
                };
                let new_id = Entity::create(pool, canonical, entity_type).await?;
                // 新实体直接以模型给出的别称作初始别名表
                let aliases_json = if event.aliases.is_empty() {
                    "[]".to_string()
                } else {
                    serde_json::to_string(&event.aliases).unwrap_or_else(|_| "[]".to_string())
                };
                if !event.aliases.is_empty() {
                    sqlx::query("UPDATE entities SET aliases = ? WHERE id = ?")
                        .bind(&aliases_json)
                        .bind(new_id)
                        .execute(pool)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                // 就地构造实体对象带回：刚写入的这行就是权威值（enabled 取建表默认的启用态），
                // 省掉"再从库里读一遍"的往返
                let created = Entity {
                    id: new_id,
                    name: canonical.to_string(),
                    entity_type: entity_type.to_string(),
                    aliases: aliases_json,
                    enabled: true,
                    created_at: chrono::Utc::now(),
                };
                // 登记进索引：同一篇文章里后续提到同一实体时直接复用，不会重复建实体
                index.push(created.clone());
                created
            }
        };

        rows.push(ResearchEvent {
            id: 0, // 入库后由 insert_batch 回填真实自增 id
            article_id,
            entity_id: entity.id,
            event_type: event.event_type.trim().to_string(),
            event_date: event.event_date.trim().to_string(),
            fact: event.fact.trim().to_string(),
            evidence: event.evidence.trim().to_string(),
            source_model: source_model.to_string(),
            // 入库时由 SQLite 默认值生成
            created_at: chrono::Utc::now(),
        });
        // 同步收集该事件的情感与财务，下标与 rows 严格同步
        extras.push(EventExtras {
            sentiment: event.sentiment.as_deref().unwrap_or("neutral").to_string(),
            confidence: event.sentiment_confidence.unwrap_or(0.5),
            financials: event.financial_metrics.clone(),
        });
    }

    // 批量入库，并把真实事件 id 回填进 rows（insert_batch 内部按唯一键回查）
    let inserted = ResearchEvent::insert_batch(pool, &mut rows).await?;

    // 写入情感分析和财务数据（按 rows 下标一一对应，event_id 已正确回填）
    for (i, row) in rows.iter().enumerate() {
        let extra = &extras[i];
        let event_id = row.id;
        // 防御：理论上 insert_batch 已回填；若仍为 0 说明唯一键回查未命中，跳过避免 FK 失败
        if event_id == 0 {
            continue;
        }
        // 写入情感分析
        sqlx::query(
            "INSERT OR REPLACE INTO event_sentiments (event_id, sentiment, confidence) VALUES (?, ?, ?)"
        )
        .bind(event_id)
        .bind(&extra.sentiment)
        .bind(extra.confidence)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        // 写入财务数据（若有）
        if let Some(ref metrics) = extra.financials {
            for (key, value) in metrics {
                let numeric: f64 = value.trim().parse().unwrap_or(0.0);
                sqlx::query(
                    "INSERT OR REPLACE INTO financial_data (article_id, entity_id, metric_key, metric_value, metric_numeric, evidence, source_model) VALUES (?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(article_id)
                .bind(row.entity_id)
                .bind(key)
                .bind(value)
                .bind(numeric)
                .bind(&row.evidence)
                .bind(source_model)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            }
        }
    }

    Ok(inserted)
}

/// 把模型给出的别称合并进已有实体的别名表
///
/// 大小写不敏感去重后追加；无新增别名时跳过写入，避免无谓的 UPDATE。
/// 合并策略刻意保守：只增不删——自动删除别名可能误伤用户手工维护的别名。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `entity` - 已存在的实体
/// * `new_aliases` - 本次提取给出的别称列表
///
/// # 错误
/// 数据库写入失败时返回错误信息
async fn merge_aliases(
    pool: &SqlitePool,
    entity: &Entity,
    new_aliases: &[String],
) -> Result<(), String> {
    // 解析现有别名表；损坏的 JSON 按空表处理（下次合并会重建）
    let mut aliases: Vec<String> =
        serde_json::from_str(&entity.aliases).unwrap_or_default();
    let before = aliases.len();

    for alias in new_aliases {
        let alias = alias.trim();
        if alias.is_empty() {
            continue;
        }
        // 与主名、现有别名做大小写不敏感去重
        let dup = alias.eq_ignore_ascii_case(&entity.name)
            || aliases.iter().any(|a| a.eq_ignore_ascii_case(alias));
        if !dup {
            aliases.push(alias.to_string());
        }
    }

    if aliases.len() > before {
        let aliases_json = serde_json::to_string(&aliases).unwrap_or_else(|_| "[]".to_string());
        sqlx::query("UPDATE entities SET aliases = ? WHERE id = ?")
            .bind(aliases_json)
            .bind(entity.id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! 抽取落库回归测试
    //!
    //! 重点锁死「`insert_batch` 必须回填 `event_id`」——此前未回填导致
    //! `event_sentiments`/`financial_data` 外键插入 event_id=0 触发
    //! `FOREIGN KEY constraint failed (code: 787)`。同时验证空 canonical 事件被跳过、
    //! 财务数据挂到正确实体（解决 `events` 与 `rows` 下标错位问题）。

    use super::*;
    use sqlx::SqlitePool;

    /// 建内存库并建好抽取落库链路所需的全部表（含外键）。
    /// 外键由 sqlx 的 SQLite 连接默认开启（PRAGMA foreign_keys = ON）。
    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("建内存库失败");
        sqlx::query("CREATE TABLE articles (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT, link TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE entities (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                name TEXT NOT NULL, \
                entity_type TEXT NOT NULL DEFAULT 'company', \
                aliases TEXT NOT NULL DEFAULT '[]', \
                enabled INTEGER DEFAULT 1, \
                alert_rules TEXT, \
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP, \
                UNIQUE(name, entity_type))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE research_events (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                article_id INTEGER NOT NULL, \
                entity_id INTEGER NOT NULL, \
                event_type TEXT NOT NULL, \
                event_date TEXT DEFAULT '', \
                fact TEXT NOT NULL, \
                evidence TEXT NOT NULL DEFAULT '', \
                source_model TEXT DEFAULT '', \
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP, \
                FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE, \
                FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE UNIQUE INDEX idx_events_dedupe \
             ON research_events(article_id, entity_id, event_type, fact)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE event_sentiments (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                event_id INTEGER NOT NULL, \
                sentiment TEXT, \
                confidence REAL, \
                FOREIGN KEY (event_id) REFERENCES research_events(id) ON DELETE CASCADE, \
                UNIQUE(event_id))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE financial_data (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                article_id INTEGER NOT NULL, \
                entity_id INTEGER NOT NULL, \
                metric_key TEXT NOT NULL, \
                metric_value TEXT NOT NULL, \
                metric_numeric REAL DEFAULT 0.0, \
                currency TEXT DEFAULT '', \
                period TEXT DEFAULT '', \
                evidence TEXT NOT NULL DEFAULT '', \
                source_model TEXT DEFAULT '', \
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP, \
                FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE, \
                FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE, \
                UNIQUE(article_id, entity_id, metric_key))",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// 回归：event_id 必须回填，否则外键插入失败（787）；并验证空 canonical 跳过与财务对齐。
    #[tokio::test]
    async fn event_id_backfilled_and_fk_ok() {
        let pool = setup().await;
        let article_id = sqlx::query("INSERT INTO articles (title, link) VALUES (?, ?)")
            .bind("标题")
            .bind("https://example.com/a")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();

        let events = vec![
            ExtractedEvent {
                canonical_name: "OpenAI".to_string(),
                aliases: vec!["OpenAI Inc.".to_string()],
                entity_type: "company".to_string(),
                event_type: "product".to_string(),
                event_date: "2026-09-20".to_string(),
                fact: "OpenAI 发布 GPT-5".to_string(),
                evidence: "OpenAI 发布 GPT-5".to_string(),
                sentiment: Some("positive".to_string()),
                sentiment_confidence: Some(0.9),
                financial_metrics: Some(vec![("revenue".to_string(), "100亿".to_string())]),
                article_title: None,
                article_link: None,
            },
            // 空 canonical：应被跳过，不产生事件/情感/财务
            ExtractedEvent {
                canonical_name: String::new(),
                aliases: vec![],
                entity_type: "company".to_string(),
                event_type: "ma".to_string(),
                event_date: String::new(),
                fact: "残缺事件".to_string(),
                evidence: String::new(),
                sentiment: None,
                sentiment_confidence: None,
                financial_metrics: Some(vec![("rd_spend".to_string(), "10亿".to_string())]),
                article_title: None,
                article_link: None,
            },
            ExtractedEvent {
                canonical_name: "Anthropic".to_string(),
                aliases: vec![],
                entity_type: "company".to_string(),
                event_type: "ma".to_string(),
                event_date: String::new(),
                fact: "Anthropic 完成融资".to_string(),
                evidence: String::new(),
                sentiment: Some("neutral".to_string()),
                sentiment_confidence: Some(0.7),
                financial_metrics: None,
                article_title: None,
                article_link: None,
            },
        ];

        // 修复前此处会因 event_id=0 抛 FOREIGN KEY constraint failed (code: 787)
        let inserted = normalize_and_store_events(&pool, article_id, &events, "test-model")
            .await
            .expect("抽取落库不应失败");
        assert_eq!(inserted, 2, "空 canonical 事件应被跳过，仅 2 条入库");

        let ev_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM research_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(ev_count, 2);

        // 每条 event_sentiments 的 event_id 必须真实存在（回填成功，无孤儿外键）
        let orphan: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM event_sentiments es \
             LEFT JOIN research_events re ON es.event_id = re.id \
             WHERE re.id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphan, 0, "不应存在指向不存在事件的情感记录");

        let sent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event_sentiments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(sent_count, 2, "每条入库事件各应有一份情感");

        // 财务数据仅 1 条，且挂到 OpenAI 实体（空 canonical 事件不应产出财务）
        let fin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM financial_data")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fin_count, 1, "空 canonical 事件不应产出财务数据");

        let openai_id: i64 = sqlx::query_scalar("SELECT id FROM entities WHERE name = ?")
            .bind("OpenAI")
            .fetch_one(&pool)
            .await
            .unwrap();
        let fin_entity: i64 =
            sqlx::query_scalar("SELECT entity_id FROM financial_data WHERE metric_key = ?")
                .bind("revenue")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(fin_entity, openai_id, "财务数据应挂到 OpenAI 实体");
    }
}
