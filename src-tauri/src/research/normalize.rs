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
            id: 0, // 自增主键，入库时由 SQLite 分配
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
    }

    ResearchEvent::insert_batch(pool, &rows).await
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
