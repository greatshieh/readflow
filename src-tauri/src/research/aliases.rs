//! 实体命名解析：别名自动维护、同一实体判定、全库查重
//!
//! # 为什么需要这个模块
//! 归一化（[`super::normalize`]）只能做**字符串**匹配：库里已有「谷歌」，
//! 文章里的 `Google` 匹配不上，于是新建一行——两家"公司"从此并行，
//! 时间线、覆盖、论点各记一半，而且**不报任何错**。
//! 「谷歌 / Google / Alphabet」「三星 / 三星电子 / Samsung」这类判断需要世界知识，
//! 字符串层面永远做不到，只能问模型。
//!
//! # 三条路径，职责严格分开
//! 1. **主路径（零额外调用）**：把已知实体清单塞进抽取提示词，要求
//!    `canonical_name` 原样照抄清单里的名字。绝大多数情况在这一步就收敛了。
//! 2. **兜底路径（每篇最多一次调用）**：抽取完，对**未命中索引**的名字攒成一批
//!    问一次「这些名字是不是已有实体的另一种写法」。命中就复用并把名字记成别名；
//!    确实是新的，则让模型顺手给出它的中英文写法，新建时就带上别名。
//! 3. **人工路径**：全库查重（[`suggest_merge_groups`]）只产出候选，
//!    真正合并必须由用户在界面上确认——合并要迁移七张表的引用，且可能丢数据
//!    （见 [`crate::db::Entity::merge`]）。
//!
//! 边界：**自动化只负责"不再分裂"，不负责"修复已分裂"**。历史数据里已经拆成两行的，
//! 只能靠人工合并，因为任何自动合并都可能把两家不同公司的历史搅在一起。

use sqlx::SqlitePool;

use crate::ai;
use crate::db::{normalize_entity_key, Entity};

use super::parse_json_loose;

/// 注入提示词的已知实体上限（条）
///
/// 个人订阅场景的实体量级是百级，取 200 已能覆盖；再多会让提示词变长、
/// 挤压正文可用 token，反而降低抽取质量。
const MAX_KNOWN_ENTITIES: usize = 200;

/// 清单里每个实体最多展示几个别名
///
/// 别名表会随使用不断追加（同一家公司的各种写法），全列出来没有边际收益——
/// 判定"是不是同一个"通常看头几个就够。
const MAX_ALIASES_PER_ENTITY: usize = 6;

/// 清单总字符上限
///
/// 与 [`MAX_KNOWN_ENTITIES`] 是两道保险：实体名很长的场景（机构全称）靠字符上限截断，
/// 保证提示词不会被清单淹没。
const MAX_KNOWN_CHARS: usize = 6000;

/// 单次判定最多提交几个新名字
///
/// 一篇文章真正的新名字通常 0–2 个；给 20 是留给"评论区式"长文的上限，
/// 超出部分本轮新建、下轮再判，不影响正确性。
const MAX_RESOLVE_NAMES: usize = 20;

/// 一个实体的轻量快照：提示词渲染与判定共用同一个形状
#[derive(Debug, Clone)]
pub struct EntityBrief {
    /// 实体 ID（合并候选需要它回指数据库行）
    pub id: i64,
    /// 规范名（清单里展示、模型必须照抄的那个）
    pub name: String,
    /// 实体类型：company / person / product / industry / other
    pub entity_type: String,
    /// 已收录的别名
    pub aliases: Vec<String>,
}

impl EntityBrief {
    /// 从实体行构造快照
    ///
    /// # 参数
    /// * `entity` - 数据库实体行
    ///
    /// # 返回值
    /// 别名为损坏 JSON 时按空表处理（不影响主名匹配）
    pub fn from_entity(entity: &Entity) -> Self {
        Self {
            id: entity.id,
            name: entity.name.clone(),
            entity_type: entity.entity_type.clone(),
            aliases: serde_json::from_str::<Vec<String>>(&entity.aliases).unwrap_or_default(),
        }
    }
}

/// 把实体行批量转成快照
///
/// # 参数
/// * `entities` - 数据库实体行（通常来自 `Entity::list_all`）
///
/// # 返回值
/// 与入参同序的快照列表
pub fn briefs_from_entities(entities: &[Entity]) -> Vec<EntityBrief> {
    entities.iter().map(EntityBrief::from_entity).collect()
}

/// 读全库实体并转成快照
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 错误
/// 实体表读取失败时返回错误信息
pub async fn load_briefs(pool: &SqlitePool) -> Result<Vec<EntityBrief>, String> {
    Ok(briefs_from_entities(&Entity::list_all(pool).await?))
}

/// 渲染「已知实体清单」段落
///
/// 每行形如 `- 谷歌 | company | 别名：Alphabet、Google`。模型要做的事只是
/// "看到同一个就照抄"，所以格式越死板越好——加序号、加表格反而会让它开始解释。
///
/// # 参数
/// * `briefs` - 实体快照（调用方已按需要的顺序排好）
///
/// # 返回值
/// 可直接拼进提示词的段落；无实体或已超字符上限时返回空串
pub fn render_known(briefs: &[EntityBrief]) -> String {
    let mut out = String::new();
    for brief in briefs.iter().take(MAX_KNOWN_ENTITIES) {
        let name = brief.name.trim();
        if name.is_empty() {
            continue;
        }
        let mut line = format!("- {}", name);
        if !brief.entity_type.trim().is_empty() {
            line.push_str(&format!(" | {}", brief.entity_type.trim()));
        }
        // 别名里可能混着主名本身（历史数据），过滤掉免得模型以为有两个名字
        let name_key = normalize_entity_key(name);
        let aliases: Vec<&str> = brief
            .aliases
            .iter()
            .map(|a| a.trim())
            .filter(|a| !a.is_empty() && normalize_entity_key(a) != name_key)
            .take(MAX_ALIASES_PER_ENTITY)
            .collect();
        if !aliases.is_empty() {
            line.push_str(&format!(" | 别名：{}", aliases.join("、")));
        }
        // 预留一个换行的余量，保证加完这行仍在上限内
        if out.chars().count() + line.chars().count() + 1 > MAX_KNOWN_CHARS {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// 读库并渲染已知实体清单（抽取前调用一次，整批复用）
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 错误
/// 实体表读取失败时返回错误信息
pub async fn known_block(pool: &SqlitePool) -> Result<String, String> {
    Ok(render_known(&load_briefs(pool).await?))
}

/// 一个未命中索引的名字的判定结果
#[derive(Debug, Clone, Default)]
pub struct NameResolution {
    /// 被判定的名字（原样回传，用于与抽取结果对齐）
    pub name: String,
    /// 指代的已有实体名；`None` 表示确实是个新实体
    pub same_as: Option<String>,
    /// 其他写法：`same_as` 有值时并入那个实体的别名表，否则作为新实体的初始别名
    pub aliases: Vec<String>,
    /// 所属公司（规范名，已在清单中校验）；`None` = 模型没给或拿不准
    ///
    /// 只在新建实体时使用（并入已有实体时归属由那个实体自己决定）。
    /// 判定为产品、子公司、人物时才有意义——挂到谁名下不改变任何匹配口径，
    /// 只是让"这东西是谁的"在列表与时间线上看得见。
    pub parent: Option<String>,
}

/// 批量判定一批新名字是否等于已有实体
///
/// 这是**兜底路径**：主路径（提示词注入）已经让模型尽量照抄清单里的名字，
/// 这里只处理它没照抄的情况，因此一篇文章最多触发一次。
///
/// # 参数
/// * `provider` - AI 提供商配置
/// * `briefs` - 已知实体快照
/// * `names` - 待判定的名字（调用方已去重、且都不在索引里）
///
/// # 返回值
/// 判定结果；模型没给或给了清单外的名字时，对应项不出现（调用方按"新实体"处理）
///
/// # 错误
/// 网络请求失败时返回错误信息（调用方应降级为"全部按新实体处理"，不得阻断入库）
pub async fn resolve_names(
    provider: &ai::AIProvider,
    briefs: &[EntityBrief],
    names: &[String],
) -> Result<Vec<NameResolution>, String> {
    let pending: Vec<&String> = names
        .iter()
        .filter(|n| !n.trim().is_empty())
        .take(MAX_RESOLVE_NAMES)
        .collect();
    // 库里一个实体都没有时，"是不是已有实体的另一种写法"这个问题无意义
    if pending.is_empty() || briefs.is_empty() {
        return Ok(Vec::new());
    }

    let name_list = pending
        .iter()
        .map(|n| format!("- {}", n.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "你在做实体名归一化。下面是系统已收录的实体清单，以及一批刚出现的名字。\n\
对每个新名字判断：\n\
1. 它是否指代清单里的某个实体（同一家公司的中英文写法、简称、全称、旧名、股票代码）？\n\
   是则把 same_as 填成清单里的名字——必须**原样照抄**，不能翻译、不能改写、不能另起新名。\n\
2. 不是同一实体（新技术、新产品、不同公司）时 same_as 留空字符串，\n\
   并在 aliases 里给出它的其他常见写法（中英文名、简称、全称），没有就留空数组。\n\
3. 拿不准时宁可留空：错误合并会把两家公司的历史搅在一起，比重名糟得多。\n\
4. aliases 只收同一实体的其他写法，不要把人物、产品、母公司或相关方列进来。\n\
5. 若它确实是新的，并且有明确的所属公司（某家公司的产品、子公司，\n\
   或某位高管/创始人的主要任职公司），把该公司名填进 parent——必须**原样照抄**\n\
   清单里类型为 company 的名字。母公司、子公司、合作伙伴不算「所属」，\n\
   一个人或一件东西同时关联多家公司、拿不准最直接的那家时，parent 留空字符串。\n\
\n\
已收录实体：\n{}\n\
\n\
新出现的名字：\n{}\n\
\n\
只输出 JSON，不要任何说明文字或代码围栏：\n\
{{\"names\":[{{\"name\":\"三星电子\",\"same_as\":\"三星\",\"aliases\":[\"Samsung\"],\"parent\":\"\"}},\
{{\"name\":\"Claude\",\"same_as\":\"\",\"aliases\":[\"Claude Code\"],\"parent\":\"Anthropic\"}}]}}",
        render_known(briefs),
        name_list
    );

    let raw = ai::chat_completion(provider, &prompt, Some(0.1)).await?;
    Ok(parse_resolutions(&raw, briefs, &pending))
}

/// 解析判定结果，并剔除越界项
///
/// 两道过滤，缺一不可：
/// 1. `name` 必须是本次问过的名字——模型偶尔会顺手把清单里的名字也写回来；
/// 2. `same_as` 必须是清单里真实存在的名字，且归一化后不等于自身——
///    否则会出现"实体 A 的别名是实体 A"，或者"并入一个不存在的名字"，
///    后者会让调用方新建一个谁也匹配不到的实体。
///
/// # 参数
/// * `raw` - 模型原始输出
/// * `briefs` - 已知实体快照（用于校验 `same_as` 是否存在）
/// * `asked` - 本次问过的名字
///
/// # 返回值
/// 通过校验的判定；解析失败时返回空列表（调用方按"全部新实体"处理）
fn parse_resolutions(raw: &str, briefs: &[EntityBrief], asked: &[&String]) -> Vec<NameResolution> {
    let Some(value) = parse_json_loose(raw) else {
        return Vec::new();
    };
    let Some(items) = value.get("names").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let asked_keys: Vec<String> = asked
        .iter()
        .map(|n| normalize_entity_key(n))
        .filter(|k| !k.is_empty())
        .collect();

    let mut out: Vec<NameResolution> = Vec::new();
    for item in items {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
        let key = normalize_entity_key(name);
        // 只接受本次问过的名字，且不重复
        if key.is_empty()
            || !asked_keys.contains(&key)
            || out.iter().any(|r| normalize_entity_key(&r.name) == key)
        {
            continue;
        }
        let same_as = item
            .get("same_as")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            // 只接受清单里真实存在的规范名
            .and_then(|s| {
                let want = normalize_entity_key(&s);
                briefs
                    .iter()
                    .find(|b| normalize_entity_key(&b.name) == want)
                    .map(|b| b.name.clone())
            })
            // 并入自己等于什么都没做
            .filter(|s| normalize_entity_key(s) != key);
        let aliases: Vec<String> = item
            .get("aliases")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.as_str())
                    .map(|a| a.trim().to_string())
                    .filter(|a| !a.is_empty() && normalize_entity_key(a) != key)
                    .collect()
            })
            .unwrap_or_default();
        // 归属：只接受清单里**确实存在且类型是 company** 的名字。
        // 模型偶尔会把产品、行业或不在清单里的名字填进来——那会让新建的实体
        // 挂在一个不该有下级的东西上，且不报错。并入已有实体时归属无意义
        // （目标实体自己已经有归属，不能被这一次判定改写）。
        let parent = item
            .get("parent")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .and_then(|s| {
                let want = normalize_entity_key(&s);
                briefs
                    .iter()
                    .find(|b| {
                        normalize_entity_key(&b.name) == want
                            && b.entity_type.eq_ignore_ascii_case("company")
                    })
                    .map(|b| b.name.clone())
            })
            .filter(|s| normalize_entity_key(s) != key)
            .filter(|_| same_as.is_none());
        out.push(NameResolution { name: name.to_string(), same_as, aliases, parent });
    }
    out
}

/// 为已有实体补全别名（人工点「AI 补全」时调用）
///
/// # 参数
/// * `provider` - AI 提供商配置
/// * `brief` - 目标实体快照（含当前别名，避免让模型重复已收录的）
/// * `entity_type` - 实体类型（用于提示词约束）
///
/// # 返回值
/// 模型建议的**新增**别名（已剔除与主名、现有别名重复的项）
///
/// # 错误
/// 网络请求失败时返回错误信息
pub async fn suggest_aliases(
    provider: &ai::AIProvider,
    brief: &EntityBrief,
) -> Result<Vec<String>, String> {
    let existing = brief.aliases.join("、");
    let prompt = format!(
        "实体「{}」的类型是 {}。它目前收录的别名是：{}。\n\
请列出它在中文与英文媒体里常见的**其他写法**：中英文名、简称、全称、旧名、股票代码。\n\
要求：\n\
1. 只收同一实体的写法，不要把人物、产品、母公司、子公司或相关方列进来；\n\
2. 不要重复已有的别名，不要重复主名本身；\n\
3. 只输出确实常用的，拿不准就不要写。\n\
\n\
只输出 JSON，不要任何说明文字或代码围栏：\n\
{{\"aliases\":[\"Google\",\"Alphabet Inc.\"]}}",
        brief.name,
        if brief.entity_type.trim().is_empty() { "company" } else { brief.entity_type.trim() },
        if existing.is_empty() { "（无）" } else { &existing }
    );
    let raw = ai::chat_completion(provider, &prompt, Some(0.1)).await?;
    let Some(value) = parse_json_loose(&raw) else {
        return Ok(Vec::new());
    };
    let name_key = normalize_entity_key(&brief.name);
    let existing_keys: Vec<String> =
        brief.aliases.iter().map(|a| normalize_entity_key(a)).collect();
    let out = value
        .get("aliases")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.as_str())
                .map(|a| a.trim().to_string())
                .filter(|a| {
                    let key = normalize_entity_key(a);
                    !key.is_empty()
                        && key != name_key
                        && !existing_keys.contains(&key)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(out)
}

/// 一组疑似指代同一实体的名字（模型产出，未经人工确认）
#[derive(Debug, Clone, serde::Serialize)]
pub struct MergeGroup {
    /// 组内的名字（清单里的原始写法）
    pub names: Vec<String>,
    /// 判定理由（给用户看的一句中文，模型给出）
    pub reason: String,
}

/// 全库查重：让模型找出可能指向同一个真实实体的条目
///
/// 只产出**候选**。合并会迁移七张表的引用、可能丢数据，必须由用户在看到
/// 影响面后确认，所以本函数绝不落库。
///
/// # 参数
/// * `provider` - AI 提供商配置
/// * `briefs` - 全库实体快照
///
/// # 返回值
/// 只含真实存在名字、且每组至少两个不同实体的候选组
///
/// # 错误
/// 网络请求失败时返回错误信息
pub async fn suggest_merge_groups(
    provider: &ai::AIProvider,
    briefs: &[EntityBrief],
) -> Result<Vec<MergeGroup>, String> {
    if briefs.len() < 2 {
        return Ok(Vec::new());
    }
    let list = briefs
        .iter()
        .take(MAX_KNOWN_ENTITIES)
        .map(|b| format!("- {}（{}）", b.name, b.entity_type))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "下面是一份关注实体清单。找出其中**确实指代同一个真实实体**的条目：\n\
同一家公司的中英文写法、简称与全称、旧名与新名、因排版差异产生的重复（如带不带 Inc.）。\n\
要求：\n\
1. 只输出确实相同的；拿不准不要输出；\n\
2. 不要把母公司与子公司、公司与其产品/模型、公司与同名人物混为一谈；\n\
3. 每组至少两个名字，最多五个；\n\
4. reason 用一句中文说明为什么判断是同一实体。\n\
\n\
清单：\n{}\n\
\n\
只输出 JSON，不要任何说明文字或代码围栏：\n\
{{\"groups\":[{{\"names\":[\"谷歌\",\"Google\"],\"reason\":\"同一公司的中英文写法\"}}]}}",
        list
    );
    let raw = ai::chat_completion(provider, &prompt, Some(0.1)).await?;
    let Some(value) = parse_json_loose(&raw) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.get("groups").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };

    // 名字 → 规范名：模型可能把大小写或空白写歪，靠归一化键拉回清单里的写法
    let canon: std::collections::HashMap<String, &str> = briefs
        .iter()
        .map(|b| (normalize_entity_key(&b.name), b.name.as_str()))
        .collect();
    let mut out: Vec<MergeGroup> = Vec::new();
    for item in items {
        let names: Vec<String> = item
            .get("names")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|n| n.as_str())
                    .filter_map(|n| canon.get(&normalize_entity_key(n)).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        // 去重后仍要有两组以上，否则"这一组"合并后什么都没变
        let mut unique: Vec<String> = Vec::new();
        for n in names {
            if !unique.iter().any(|u| normalize_entity_key(u) == normalize_entity_key(&n)) {
                unique.push(n);
            }
        }
        if unique.len() < 2 {
            continue;
        }
        unique.sort_by_key(|n| normalize_entity_key(n));
        // 模型可能用不同顺序重复给出同一组，按排好序的名字串去重
        let key = unique
            .iter()
            .map(|n| normalize_entity_key(n))
            .collect::<Vec<_>>()
            .join("|");
        if out.iter().any(|g| {
            let mut k: Vec<String> = g.names.iter().map(|n| normalize_entity_key(n)).collect();
            k.sort();
            k.join("|") == key
        }) {
            continue;
        }
        let reason = item
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        out.push(MergeGroup { names: unique, reason });
    }
    Ok(out)
}

/// 一条给界面渲染的合并候选（名字已映射回实体 ID）
#[derive(Debug, Clone, serde::Serialize)]
pub struct MergeCandidate {
    /// 组内实体的 ID（按名字排序，与 `names` 一一对应）
    pub entity_ids: Vec<i64>,
    /// 组内实体的规范名
    pub names: Vec<String>,
    /// 判定理由
    pub reason: String,
}

/// 把模型产出的候选组映射成实体 ID
///
/// # 参数
/// * `briefs` - 全库实体快照（名字 → ID）
/// * `groups` - [`suggest_merge_groups`] 的结果
///
/// # 返回值
/// 与 `groups` 同序的候选；名字对不上实体的组被丢弃
pub fn groups_to_candidates(briefs: &[EntityBrief], groups: &[MergeGroup]) -> Vec<MergeCandidate> {
    let by_name: std::collections::HashMap<String, &EntityBrief> = briefs
        .iter()
        .map(|b| (normalize_entity_key(&b.name), b))
        .collect();
    groups
        .iter()
        .filter_map(|g| {
            let mut ids = Vec::new();
            let mut names = Vec::new();
            for name in &g.names {
                if let Some(brief) = by_name.get(&normalize_entity_key(name)) {
                    ids.push(brief.id);
                    names.push(brief.name.clone());
                }
            }
            if ids.len() < 2 {
                return None;
            }
            Some(MergeCandidate {
                entity_ids: ids,
                names,
                reason: g.reason.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! 纯函数验证：提示词清单渲染、判定结果过滤、候选映射
    //!
    //! 这三个函数的共同点是"错了不报错"——渲染多一行、放过一个不存在的
    //! `same_as`、候选里混进对不上的 ID，界面表现只是"看起来不太对"，
    //! 所以用断言把边界钉死。

    use super::*;

    /// 造一个实体快照
    fn brief(id: i64, name: &str, entity_type: &str, aliases: &[&str]) -> EntityBrief {
        EntityBrief {
            id,
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn render_known_emits_name_type_and_aliases() {
        let text = render_known(&[brief(8, "Alphabet", "company", &["谷歌", "Google"])]);
        assert!(text.contains("Alphabet"));
        assert!(text.contains("company"));
        assert!(text.contains("谷歌、Google"));
    }

    #[test]
    fn render_known_skips_self_alias_and_blank() {
        // 别名为空 → 不出现"别名："空标签；别名等于主名 → 过滤掉
        let text = render_known(&[
            brief(1, "腾讯", "company", &[]),
            brief(2, "Meta", "company", &["meta", " ", "Facebook"]),
        ]);
        assert!(!text.contains("腾讯 | company | 别名"));
        assert!(text.contains("Facebook"));
        // 归一化后等于主名的别名不得出现两次
        assert_eq!(text.matches("Meta").count(), 1);
        assert!(!render_known(&[]).contains('-'));
    }

    #[test]
    fn parse_resolutions_rejects_unknown_and_self() {
        let briefs = vec![brief(8, "Alphabet", "company", &["谷歌"])];
        let asked_names = vec!["Google".to_string(), "Google".to_string()];
        let asked: Vec<&String> = asked_names.iter().collect();
        let raw = r#"{"names":[
            {"name":"Google","same_as":"Alphabet","aliases":["Alphabet Inc."]},
            {"name":"Google","same_as":"Alphabet","aliases":[]},
            {"name":"Alphabet","same_as":"Alphabet","aliases":[]},
            {"name":"Foo","same_as":"不存在的实体","aliases":["Foo Inc"]}
        ]}"#;
        let out = parse_resolutions(raw, &briefs, &asked);
        // 重复的 Google 只留一条；未问过的名字与不存在的 same_as 都要丢掉
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].same_as.as_deref(), Some("Alphabet"));
        assert_eq!(out[0].aliases, vec!["Alphabet Inc.".to_string()]);
    }

    #[test]
    fn parse_resolutions_handles_fenced_output() {
        let briefs = vec![brief(6, "三星", "company", &[])];
        let asked_names = vec!["三星电子".to_string()];
        let asked: Vec<&String> = asked_names.iter().collect();
        let raw = "好的，结果如下：\n```json\n{\"names\":[{\"name\":\"三星电子\",\
                   \"same_as\":\"三星\",\"aliases\":[]}]}\n```";
        let out = parse_resolutions(raw, &briefs, &asked);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].same_as.as_deref(), Some("三星"));
    }

    #[test]
    fn parse_resolutions_accepts_only_company_parent() {
        // 归属只在"确实是新实体"时生效，且上级必须真实存在、类型是公司。
        // 少了任一道校验，新建的产品会挂到行业 / 另一个产品 / 空行上，
        // 而界面上只表现为怪异的"属于 XXX"。
        let briefs = vec![
            brief(11, "Anthropic", "company", &[]),
            brief(24, "Claude", "product", &[]),
            brief(39, "AI行业", "industry", &[]),
        ];
        let asked_names = vec![
            "Claude Code".to_string(),
            "Claude Code 2".to_string(),
            "Foo".to_string(),
            "Bar".to_string(),
            "Baz".to_string(),
        ];
        let asked: Vec<&String> = asked_names.iter().collect();
        let raw = r#"{"names":[
            {"name":"Claude Code","same_as":"","aliases":[],"parent":"Anthropic"},
            {"name":"Claude Code 2","same_as":"","aliases":[],"parent":"Claude"},
            {"name":"Foo","same_as":"","aliases":[],"parent":"AI行业"},
            {"name":"Bar","same_as":"","aliases":[],"parent":"不存在的公司"},
            {"name":"Baz","same_as":"Anthropic","aliases":[],"parent":"Anthropic"}
        ]}"#;
        let out = parse_resolutions(raw, &briefs, &asked);
        let by_name = |n: &str| out.iter().find(|r| r.name == n).unwrap();
        assert_eq!(by_name("Claude Code").parent.as_deref(), Some("Anthropic"));
        // 产品不能当上级
        assert!(by_name("Claude Code 2").parent.is_none());
        // 行业不能当上级
        assert!(by_name("Foo").parent.is_none());
        // 清单外的名字不能当上级
        assert!(by_name("Bar").parent.is_none());
        // 已经并入已有实体时，归属不由这一次决定
        assert!(by_name("Baz").parent.is_none());
        assert_eq!(by_name("Baz").same_as.as_deref(), Some("Anthropic"));
    }

    #[test]
    fn groups_to_candidates_drops_unknown_names() {
        let briefs = vec![
            brief(8, "Alphabet", "company", &[]),
            brief(23, "Google", "company", &[]),
            brief(6, "三星", "company", &[]),
        ];
        let groups = vec![
            MergeGroup {
                names: vec!["Alphabet".to_string(), "Google".to_string()],
                reason: "同一公司".to_string(),
            },
            // 只有一个名字对得上实体 → 合并后什么都没变，必须丢弃
            MergeGroup {
                names: vec!["三星".to_string(), "不存在的名字".to_string()],
                reason: "半组".to_string(),
            },
        ];
        let out = groups_to_candidates(&briefs, &groups);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entity_ids, vec![8, 23]);
        assert_eq!(out[0].names, vec!["Alphabet".to_string(), "Google".to_string()]);
    }
}
