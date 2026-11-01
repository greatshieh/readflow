//! 统一检索与来源分歧（阶段四）
//!
//! # 职责
//! 两件事：
//! - **统一检索**（[`search_notes`]）：一次查询命中用户自己写的三类东西——
//!   高亮、论点、证据快照，并标出每条结果"被挂在哪个论点下"；
//! - **来源分歧**（[`evidence_divergence`]）：一条判断的证据来自几家不同来源。
//!   这是阶段四真正的价值所在——它回答"这条事实是不是只有一家在说"。
//!
//! # 边界（写在最前，防止后来扩张）
//! - **不承诺长期事件检索**：`research_events` 对 `articles` 是级联删除，
//!   而默认保留策略是 30 天。事件会随文章一起消失，这不是检索能修的，
//!   是保留策略决定的（ROADMAP §1.4 已明确划界）；
//! - **不做语义 / 向量检索**：本地单人数据量下 FTS + 实体过滤够用，
//!   向量索引带来模型依赖与重建成本，收益不明；
//! - **不做 AI 摘要式结果**：摘要会让人跳过原文，而这一阶段的全部价值是
//!   "回到原文"。
//!
//! # 为什么给三张表各建一个 FTS，而不是合成一张
//! 外部内容表（`content='x'`）只能挂一张源表，三源合一就得自己维护同步——
//! 而"自己维护"意味着**每一条写入路径都要记得更新索引**。漏一条不会报错，
//! 只会表现为"这段文字搜不到"，是最难发现的一类失效。
//! 用触发器镜像（与 `articles_fts` 同一套做法）把正确性交给数据库，
//! 代价只是多两张虚拟表。
//!
//! # 分词的坑
//! 用 `tokenize='trigram'`（与 `articles_fts` 一致）：默认的 unicode61 分词器
//! 会把整句中文当一个词，搜"储能"什么都搜不到。trigram 按三字滑窗建索引，
//! 中文子串检索才成立；代价是**查询词至少 2 个字符**，1 个字搜不出东西。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// 单次检索每类结果的条数上限
///
/// 三张表各取这么多条再合并：面板一次能看到 6~7 行，给多了也翻不到，
/// 反而拖慢返回。
const DEFAULT_LIMIT: i64 = 12;

/// 一条检索命中
///
/// 三类结果共用一个结构而不是三个类型：前端渲染的是同一种"一句话 + 出处"，
/// 分成三个类型会让前端写三套几乎一样的行模板。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteHit {
    /// 命中类型：`highlight` / `thesis` / `evidence`
    pub kind: String,
    /// 该类型的行 ID
    pub ref_id: i64,
    /// 关联论点 ID（高亮可能还没挂到任何论点，此时为 null）
    pub thesis_id: Option<i64>,
    /// 关联论点的一句话（用于显示"被挂在哪个论点下"）
    pub thesis_statement: String,
    /// 第一行文字：论点是判断句本身，高亮/证据是命中片段
    pub text: String,
    /// 第二行文字：出处（文章标题 / 来源名 / 立场）
    pub meta: String,
}

/// 一条判断的来水构成
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Divergence {
    /// 论点 ID
    pub thesis_id: i64,
    /// 支持证据来自几家不同来源
    pub support_sources: i64,
    /// 反对证据来自几家不同来源
    pub oppose_sources: i64,
    /// 去重后的来源名（含"未记录"以外的真实来源）
    pub sources: Vec<String>,
    /// 没有来源记录的证据条数（TASK-00 之前挂的老证据）
    pub unrecorded: i64,
}

/// 把用户输入转成 FTS5 查询串
///
/// # 为什么要包一层引号
/// FTS5 的查询语法里有 `AND` / `OR` / `NOT` / `*` / `:` / `^` 等操作符。
/// 用户输入"宁德 AND"这种半句话会让 MATCH 直接报语法错，而报错的表相是
/// "搜索框里一打字就空白"，极难归因。
///
/// 包成单个短语（`"..."`，内部引号双写转义）之后，用户输入永远被当成
/// **一段要原样出现的文字**：既能搜到，也不会因为一个符号就整条失效。
/// 代价是丢掉了布尔检索——这个功能不需要，本地数据量用不上。
///
/// # 返回值
/// 空输入（或只剩空白/控制字符）返回 `None`，调用方据此直接返回空结果，
/// 不去打数据库。
fn fts_query(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        return None;
    }
    Some(format!("\"{}\"", cleaned.replace('"', "\"\"")))
}

/// 一次性回填三张笔记索引
///
/// # 为什么每次都重建（而不是只在索引为空时重建）
/// 判"索引为空"在外部内容表上**做不到**：`SELECT COUNT(*) FROM x_fts` 读的是
/// **内容表**的行数，不是索引里的行数——数据早于索引存在时它照样报非零，
/// 于是"该回填"的判断永远为假，升级后存量笔记永久搜不到。
/// 用影子表（`x_fts_data`）判空属于依赖未文档化的内部结构，不能这么做。
///
/// 所以这里无条件重建。代价是每次启动重新分词一遍用户笔记——这是本应用里
/// 唯一由用户手写的表，量级是"千条"而不是"百万条"，实测在毫秒级；
/// 换来的是**索引永远与内容表一致**，不需要版本游标（本项目的铁律是不做
/// 迁移代码），也不存在"某条写入路径忘了更新索引"这类静默失效。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 实际重建的索引数量（源表为空的跳过）
///
/// # 错误
/// 计数或重建失败
pub async fn backfill_note_index(pool: &SqlitePool) -> Result<u64, String> {
    // (FTS 表名, 源表名)：两处都写全，避免用字符串拼表名去查（拼错就是运行期错）
    const PAIRS: &[(&str, &str)] = &[
        ("highlights_fts", "highlights"),
        ("theses_fts", "theses"),
        ("thesis_evidence_fts", "thesis_evidence"),
    ];
    let mut rebuilt = 0u64;
    for (fts, src) in PAIRS {
        let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {src}"))
            .fetch_one(pool)
            .await
            .map_err(|e| format!("统计 {src} 失败：{e}"))?;
        if total == 0 {
            continue;
        }
        // 外部内容表的官方重建入口：按源表重读并重新分词
        sqlx::query(&format!("INSERT INTO {fts}({fts}) VALUES('rebuild')"))
            .execute(pool)
            .await
            .map_err(|e| format!("重建 {fts} 失败：{e}"))?;
        rebuilt += 1;
    }
    Ok(rebuilt)
}

/// 统一检索笔记（高亮 / 论点 / 证据快照）
///
/// 三类结果各自查询再拼接，而不是用 UNION：
/// 三类要的附加信息完全不同（高亮要文章标题、论点要实体名、证据要立场），
/// UNION 会逼着把三张表的列补齐成同一形状，可读性换来的是零收益。
///
/// # 排序
/// 每类内部按 FTS5 的 `rank`（越小越相关）。**不跨类混排**：
/// 三类结果的可比性只有"命中同一个词"，硬混在一起会让用户看到
/// 一条高亮排在一条论点前面却不知道为什么。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `query` - 用户输入（原样传入，内部转义）
/// * `limit` - 每类上限；None 用 [`DEFAULT_LIMIT`]
///
/// # 返回值
/// 命中列表（高亮 → 论点 → 证据 的顺序）
///
/// # 错误
/// 查询失败
pub async fn search_notes(
    pool: &SqlitePool,
    query: &str,
    limit: Option<i64>,
) -> Result<Vec<NoteHit>, String> {
    let Some(q) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 50);

    // 短查询走 LIKE，不走 MATCH。trigram 只索引"3 个字符"的滑窗，
    // 查询串短于 3 个字符时没有任何 trigram 可命中，MATCH 恒为空——
    // 而中文里两字词（储能、出货、毛利）恰恰是最常见的查询。
    // 与 `db::articles_search` 同一判据、同一回退方式，两处不要各定一个阈值。
    if query.trim().chars().count() < 3 {
        return search_notes_like(pool, query.trim(), limit).await;
    }

    let mut hits = Vec::new();
    hits.extend(search_highlights(pool, &q, limit).await?);
    hits.extend(search_theses(pool, &q, limit).await?);
    hits.extend(search_evidence(pool, &q, limit).await?);
    Ok(hits)
}

/// 把用户输入转成 LIKE 模式串（转义 `%` `_` `\`）
///
/// 不转义的话，用户输入一个 `%` 就会匹配到全部笔记——看起来像"搜索坏了"。
fn like_pattern(raw: &str) -> String {
    let escaped = raw
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{}%", escaped)
}

/// 短查询（1–2 字符）的 LIKE 回退
///
/// 与 FTS 路径产出同样的 [`NoteHit`]，只是没有相关度排序——LIKE 没有 rank，
/// 统一按时间倒序（最近写的东西更可能是用户在找的）。
///
/// 三张表的列各不相同，所以这里没有把 SQL 抽成模板：抽出来只会得到一个
/// 带三个占位符的字符串，读起来比三条独立语句更费劲。
async fn search_notes_like(
    pool: &SqlitePool,
    raw: &str,
    limit: i64,
) -> Result<Vec<NoteHit>, String> {
    let pattern = like_pattern(raw);
    let mut hits = Vec::new();

    let highlights = sqlx::query(
        r#"
        SELECT h.id AS ref_id, h.text,
               COALESCE((SELECT a.title FROM articles a WHERE a.id = h.article_id), '') AS article_title,
               (SELECT MIN(v.thesis_id) FROM thesis_evidence v
                 WHERE v.kind = 'highlight' AND v.ref_id = h.id) AS thesis_id
          FROM highlights h
         WHERE h.text LIKE ?1 ESCAPE '\'
         ORDER BY h.created_at DESC, h.id DESC
         LIMIT ?2
        "#,
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("检索高亮失败：{e}"))?;
    for r in &highlights {
        hits.push(NoteHit {
            kind: "highlight".into(),
            ref_id: r.get("ref_id"),
            thesis_id: r.get("thesis_id"),
            thesis_statement: String::new(),
            text: r.get("text"),
            meta: r.get("article_title"),
        });
    }

    let theses = sqlx::query(
        r#"
        SELECT t.id AS ref_id, t.statement,
               COALESCE(e.name, '') AS entity_name
          FROM theses t
          LEFT JOIN entities e ON e.id = t.entity_id
         WHERE t.statement LIKE ?1 ESCAPE '\' AND t.status <> 'archived'
         ORDER BY t.updated_at DESC, t.id DESC
         LIMIT ?2
        "#,
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("检索论点失败：{e}"))?;
    for r in &theses {
        let statement: String = r.get("statement");
        hits.push(NoteHit {
            kind: "thesis".into(),
            ref_id: r.get("ref_id"),
            thesis_id: Some(r.get("ref_id")),
            thesis_statement: statement.clone(),
            text: statement,
            meta: r.get("entity_name"),
        });
    }

    let evidence = sqlx::query(
        r#"
        SELECT v.id AS ref_id, v.thesis_id, v.stance, v.snapshot_text, t.statement,
               COALESCE(src.source_name, '') AS source_name
          FROM thesis_evidence v
          JOIN theses t ON t.id = v.thesis_id
          LEFT JOIN thesis_evidence_source src ON src.evidence_id = v.id
         WHERE (v.snapshot_text LIKE ?1 ESCAPE '\' OR v.note LIKE ?1 ESCAPE '\')
           AND t.status <> 'archived'
         ORDER BY v.created_at DESC, v.id DESC
         LIMIT ?2
        "#,
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("检索证据失败：{e}"))?;
    for r in &evidence {
        let stance: String = r.get("stance");
        let source_name: String = r.get("source_name");
        hits.push(NoteHit {
            kind: "evidence".into(),
            ref_id: r.get("ref_id"),
            thesis_id: Some(r.get("thesis_id")),
            thesis_statement: r.get("statement"),
            text: r.get("snapshot_text"),
            meta: evidence_meta(&source_name, &stance),
        });
    }

    Ok(hits)
}

/// 证据命中行的第二行文字：来源优先，没有就标立场
///
/// 来源缺失时不写"来源未记录"而只写立场：检索结果是扫读列表，
/// 每个字都要有信息量，"来源未记录"在每条老证据上都会出现，很快变成噪音。
fn evidence_meta(source_name: &str, stance: &str) -> String {
    if source_name.trim().is_empty() {
        stance_label(stance).to_string()
    } else {
        format!("{} · {}", source_name.trim(), stance_label(stance))
    }
}

/// 搜高亮（附带"被挂在哪个论点下"）
///
/// 一条高亮可能被挂到多个论点，也可能还没挂。取 `MIN(thesis_id)` 作代表
/// 并显示它的一句话：列表里一行只能放一个去处，多挂的情况点进去就能看到。
async fn search_highlights(pool: &SqlitePool, q: &str, limit: i64) -> Result<Vec<NoteHit>, String> {
    let rows = sqlx::query(
        r#"
        SELECT h.id AS ref_id,
               snippet(highlights_fts, 0, '', '', '…', 16) AS text,
               COALESCE((SELECT a.title FROM articles a WHERE a.id = h.article_id), '') AS article_title,
               (SELECT MIN(v.thesis_id) FROM thesis_evidence v
                 WHERE v.kind = 'highlight' AND v.ref_id = h.id) AS thesis_id
          FROM highlights_fts
          JOIN highlights h ON h.id = highlights_fts.rowid
         WHERE highlights_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2
        "#,
    )
    .bind(q)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("检索高亮失败：{e}"))?;

    Ok(rows
        .iter()
        .map(|r| {
            let thesis_id: Option<i64> = r.get("thesis_id");
            NoteHit {
                kind: "highlight".into(),
                ref_id: r.get("ref_id"),
                thesis_id,
                // 论点一句话由调用方在合并后统一补：这里再查一次会变成 N+1
                thesis_statement: String::new(),
                text: r.get("text"),
                meta: r.get("article_title"),
            }
        })
        .collect())
}

/// 搜论点（判断句本身）
async fn search_theses(pool: &SqlitePool, q: &str, limit: i64) -> Result<Vec<NoteHit>, String> {
    let rows = sqlx::query(
        r#"
        SELECT t.id AS ref_id,
               snippet(theses_fts, 0, '', '', '…', 24) AS text,
               COALESCE(e.name, '') AS entity_name,
               t.status
          FROM theses_fts
          JOIN theses t ON t.id = theses_fts.rowid
          LEFT JOIN entities e ON e.id = t.entity_id
         WHERE theses_fts MATCH ?1 AND t.status <> 'archived'
         ORDER BY rank
         LIMIT ?2
        "#,
    )
    .bind(q)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("检索论点失败：{e}"))?;

    Ok(rows
        .iter()
        .map(|r| {
            let statement: String = r.get("text");
            NoteHit {
                kind: "thesis".into(),
                ref_id: r.get("ref_id"),
                thesis_id: Some(r.get("ref_id")),
                // 论点命中的"挂在哪个论点下"就是它自己，直接用判断句填，
                // 省掉一次反查
                thesis_statement: statement.clone(),
                text: statement,
                meta: r.get("entity_name"),
            }
        })
        .collect())
}

/// 搜证据快照（正文片段）
async fn search_evidence(pool: &SqlitePool, q: &str, limit: i64) -> Result<Vec<NoteHit>, String> {
    let rows = sqlx::query(
        r#"
        SELECT v.id AS ref_id,
               snippet(thesis_evidence_fts, 0, '', '', '…', 16) AS text,
               v.thesis_id,
               v.stance,
               t.statement,
               COALESCE(src.source_name, '') AS source_name
          FROM thesis_evidence_fts
          JOIN thesis_evidence v ON v.id = thesis_evidence_fts.rowid
          JOIN theses t ON t.id = v.thesis_id
          LEFT JOIN thesis_evidence_source src ON src.evidence_id = v.id
         WHERE thesis_evidence_fts MATCH ?1 AND t.status <> 'archived'
         ORDER BY rank
         LIMIT ?2
        "#,
    )
    .bind(q)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("检索证据失败：{e}"))?;

    Ok(rows
        .iter()
        .map(|r| {
            let stance: String = r.get("stance");
            let source_name: String = r.get("source_name");
            NoteHit {
                kind: "evidence".into(),
                ref_id: r.get("ref_id"),
                thesis_id: Some(r.get("thesis_id")),
                thesis_statement: r.get("statement"),
                text: r.get("text"),
                // 出处口径与短查询回退共用同一个函数：两条路径给出不同的第二行文字，
                // 会让用户以为"换个词搜，同一条证据变成了另一条"
                meta: evidence_meta(&source_name, &stance),
            }
        })
        .collect())
}

/// 立场中文名
fn stance_label(stance: &str) -> &'static str {
    match stance {
        "support" => "支持",
        "oppose" => "反对",
        "mixed" => "存疑",
        _ => "证据",
    }
}

/// 统计一条判断的来水构成（分歧度）
///
/// # 为什么按"来源名去重"而不是按证据条数
/// 一条判断挂 5 条证据、全来自同一家媒体，那不是互证，是同一句话说了五遍。
/// 分歧度的全部意义就在这个去重上——**算成证据条数，这个功能就白做了**。
///
/// # 空来源怎么处理
/// `source_name` 为空串的老证据不计入任何一侧的来源数，单独计进 `unrecorded`。
/// 把它们算成"1 家"会让"仅 1 家来源"这个警示消失在最需要它的地方；
/// 直接忽略又会让用户以为证据不存在。分开计数才能说明"你有证据，但不知道谁说的"。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `thesis_id` - 论点 ID
///
/// # 返回值
/// 支持侧来源数、反对侧来源数、来源名列表、无来源证据条数
///
/// # 错误
/// 查询失败
pub async fn evidence_divergence(
    pool: &SqlitePool,
    thesis_id: i64,
) -> Result<Divergence, String> {
    let rows = sqlx::query(
        r#"
        SELECT v.stance,
               COALESCE(NULLIF(TRIM(src.source_name), ''), '') AS source_name
          FROM thesis_evidence v
          LEFT JOIN thesis_evidence_source src ON src.evidence_id = v.id
         WHERE v.thesis_id = ?
        "#,
    )
    .bind(thesis_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("统计来源失败：{e}"))?;

    // 两侧各自去重：支持与反对必须分开数，否则"3 家支持 + 1 家反对"会被说成"4 家互证"
    let mut support: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut oppose: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut all: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut unrecorded = 0i64;

    for r in &rows {
        let stance: String = r.get("stance");
        let name: String = r.get("source_name");
        if name.is_empty() {
            unrecorded += 1;
            continue;
        }
        all.insert(name.clone());
        match stance.as_str() {
            "oppose" => {
                oppose.insert(name);
            }
            "support" => {
                support.insert(name);
            }
            // mixed 两面都有：计入 all，但不计入任何一侧
            _ => {}
        }
    }

    Ok(Divergence {
        thesis_id,
        support_sources: support.len() as i64,
        oppose_sources: oppose.len() as i64,
        sources: all.into_iter().collect(),
        unrecorded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 真实 schema 内存库（手抄表结构会让 FTS 触发器静默缺失）
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("建库失败");
        crate::db::create_tables(&pool).await.expect("建表失败");
        pool
    }

    /// 造一个实体 + 一个论点
    ///
    /// 实体名带自增序号：`entities` 有 `(name, entity_type)` 唯一约束，
    /// 一个测试里造两条论点会撞上（这些用例不关心实体是谁）。
    async fn seed_thesis(pool: &SqlitePool, statement: &str) -> i64 {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO entities (name, entity_type) VALUES (?, 'company') RETURNING id",
        )
        .bind(format!("标的{}", n))
        .fetch_one(pool)
        .await
        .expect("插实体失败");
        sqlx::query_scalar("INSERT INTO theses (statement, entity_id) VALUES (?, ?) RETURNING id")
            .bind(statement)
            .bind(eid)
            .fetch_one(pool)
            .await
            .expect("插论点失败")
    }

    /// 挂一条带来源的证据
    ///
    /// `ref_id` 用自增序号：`thesis_evidence` 上有
    /// `UNIQUE(thesis_id, kind, ref_id)`，用文本长度这类会重复的值第二条就撞了。
    async fn seed_evidence(pool: &SqlitePool, thesis_id: i64, stance: &str, text: &str, source: &str) -> i64 {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as i64;
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance, snapshot_text) \
             VALUES (?, 'article', ?, ?, ?) RETURNING id",
        )
        .bind(thesis_id)
        .bind(n)
        .bind(stance)
        .bind(text)
        .fetch_one(pool)
        .await
        .expect("插证据失败");
        if !source.is_empty() {
            sqlx::query(
                "INSERT INTO thesis_evidence_source (evidence_id, article_id, source_name, published_at) \
                 VALUES (?, NULL, ?, '2026-10-28')",
            )
            .bind(eid)
            .bind(source)
            .execute(pool)
            .await
            .expect("插来源失败");
        }
        eid
    }

    #[tokio::test]
    async fn triggers_keep_thesis_index_fresh() {
        let pool = test_pool().await;
        seed_thesis(&pool, "储能出货将超预期").await;

        // 3 个字以上才走 FTS（trigram 下限）
        let hits = search_notes(&pool, "出货将", None).await.unwrap();
        assert!(
            hits.iter().any(|h| h.kind == "thesis" && h.text.contains("储能出货")),
            "触发器应当把新论点写进索引，实际 {:?}",
            hits
        );

        // 改判断句：索引跟着变（旧的搜不到、新的搜得到）
        let tid: i64 = sqlx::query_scalar("SELECT id FROM theses LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE theses SET statement = '储能出货将低预期' WHERE id = ?")
            .bind(tid)
            .execute(&pool)
            .await
            .unwrap();
        assert!(search_notes(&pool, "出货将低", None).await.unwrap().iter().any(|h| h.kind == "thesis"));
        assert!(
            !search_notes(&pool, "出货将超", None).await.unwrap().iter().any(|h| h.kind == "thesis"),
            "更新触发器应当先删旧索引再插新索引"
        );
    }

    #[tokio::test]
    async fn short_query_falls_back_to_like() {
        let pool = test_pool().await;
        seed_thesis(&pool, "储能出货将超预期").await;

        // 2 个字：trigram 命中不了，必须靠 LIKE 回退
        let hits = search_notes(&pool, "储能", None).await.unwrap();
        assert!(
            hits.iter().any(|h| h.kind == "thesis"),
            "两字查询必须可用，否则中文最常见的查法全废"
        );
    }

    #[tokio::test]
    async fn like_pattern_escapes_wildcards() {
        let pool = test_pool().await;
        seed_thesis(&pool, "毛利率 40% 的判断").await;
        seed_thesis(&pool, "另一条判断").await;
        // 只看命中数：不转义的话 `%` 会匹配全部
        let hits = search_notes(&pool, "0%", None).await.unwrap();
        assert!(!hits.iter().any(|h| h.text.contains("另一条判断")), "通配符必须被转义");
    }

    #[tokio::test]
    async fn highlight_hits_carry_owning_thesis() {
        let pool = test_pool().await;
        let tid = seed_thesis(&pool, "储能出货将超预期").await;
        // 高亮有指向 articles 的外键，必须造一篇真实文章（sqlx 默认开启外键约束）
        let fid: i64 = sqlx::query_scalar(
            "INSERT INTO feeds (name, url) VALUES ('财新', 'https://example.com/cx') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .expect("插订阅源失败");
        let aid: i64 = sqlx::query_scalar(
            "INSERT INTO articles (feed_id, guid, link, title) \
             VALUES (?, 'g1', 'https://example.com/a1', '储能观察') RETURNING id",
        )
        .bind(fid)
        .fetch_one(&pool)
        .await
        .expect("插文章失败");
        let hid: i64 = sqlx::query_scalar(
            "INSERT INTO highlights (article_id, text) VALUES (?, '二期产能提前投产') RETURNING id",
        )
        .bind(aid)
        .fetch_one(&pool)
        .await
        .expect("插高亮失败");
        sqlx::query(
            "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance, snapshot_text) \
             VALUES (?, 'highlight', ?, 'support', '二期产能提前投产')",
        )
        .bind(tid)
        .bind(hid)
        .execute(&pool)
        .await
        .expect("插证据失败");

        let hits = search_notes(&pool, "产能提前", None).await.unwrap();
        let hit = hits.iter().find(|h| h.kind == "highlight").expect("应命中高亮");
        assert_eq!(hit.ref_id, hid);
        assert_eq!(hit.thesis_id, Some(tid), "高亮要标出它被挂在哪条判断下");
    }

    #[tokio::test]
    async fn archived_theses_are_excluded() {
        let pool = test_pool().await;
        let tid = seed_thesis(&pool, "储能出货将超预期").await;
        sqlx::query("UPDATE theses SET status = 'archived' WHERE id = ?")
            .bind(tid)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            !search_notes(&pool, "出货将超", None).await.unwrap().iter().any(|h| h.kind == "thesis"),
            "归档的判断不该出现在检索结果里"
        );
    }

    #[tokio::test]
    async fn backfill_rebuilds_index_from_content_table() {
        let pool = test_pool().await;
        // 先断掉触发器再插数据，模拟"数据早于索引存在"的升级现场
        sqlx::query("DROP TRIGGER theses_fts_ai").execute(&pool).await.unwrap();
        seed_thesis(&pool, "储能出货将超预期").await;
        assert!(
            !search_notes(&pool, "出货将超", None).await.unwrap().iter().any(|h| h.kind == "thesis"),
            "触发器缺失时索引是空的（正是升级现场）"
        );

        let rebuilt = backfill_note_index(&pool).await.unwrap();
        assert!(rebuilt >= 1, "有数据的源表应当触发重建");
        assert!(
            search_notes(&pool, "出货将超", None).await.unwrap().iter().any(|h| h.kind == "thesis"),
            "重建之后存量数据要能被搜到"
        );

        // 再重建一次：结果不重复（rebuild 是幂等的，不是把数据再插一遍）
        backfill_note_index(&pool).await.unwrap();
        let hits = search_notes(&pool, "出货将超", None).await.unwrap();
        assert_eq!(
            hits.iter().filter(|h| h.kind == "thesis").count(),
            1,
            "重复重建不能让同一条判断出现两次"
        );
    }

    #[tokio::test]
    async fn divergence_counts_distinct_sources_not_rows() {
        let pool = test_pool().await;
        let tid = seed_thesis(&pool, "判断").await;
        // 三条支持证据，但只有两家来源
        seed_evidence(&pool, tid, "support", "出货增长", "财新").await;
        seed_evidence(&pool, tid, "support", "毛利回升", "财新").await;
        seed_evidence(&pool, tid, "support", "订单饱满", "晚点").await;
        seed_evidence(&pool, tid, "oppose", "友商降价", "第一财经").await;
        // 没有来源的老证据
        seed_evidence(&pool, tid, "support", "老证据", "").await;

        let d = evidence_divergence(&pool, tid).await.unwrap();
        assert_eq!(d.support_sources, 2, "同一家媒体说三遍不是互证");
        assert_eq!(d.oppose_sources, 1);
        assert_eq!(d.sources.len(), 3);
        assert_eq!(d.unrecorded, 1, "没有来源的证据要单独计数，不能算成一家");
    }

    #[tokio::test]
    async fn divergence_of_thesis_without_evidence_is_all_zero() {
        let pool = test_pool().await;
        let tid = seed_thesis(&pool, "判断").await;
        let d = evidence_divergence(&pool, tid).await.unwrap();
        assert_eq!((d.support_sources, d.oppose_sources, d.unrecorded), (0, 0, 0));
        assert!(d.sources.is_empty());
    }

    #[test]
    fn note_hit_serializes_as_camel_case() {
        let hit = NoteHit {
            kind: "thesis".into(),
            ref_id: 1,
            thesis_id: Some(1),
            thesis_statement: "判断".into(),
            text: "判断".into(),
            meta: "宁德时代".into(),
        };
        let v = serde_json::to_value(&hit).unwrap();
        assert!(v.get("refId").is_some());
        assert!(v.get("thesisId").is_some());
        assert!(v.get("thesisStatement").is_some());
    }
}
