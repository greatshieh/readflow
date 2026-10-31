//! 论点台账（Thesis Ledger）
//!
//! # 职责
//! 承载"用户自己的判断"这一层——本项目此前的表全是系统产出（文章 / 事件 / 报告 / 高亮），
//! 唯独没有地方放人的观点。本模块提供论点的增删改查、证据挂接、变更留痕。
//!
//! # 为什么要有它
//! 分析师的真实循环是「假设 → 找证据 → 主动找反证 → 更新假设 → 输出」。
//! 前两步工具都能做（`research_events` 已在抽事实），但**第三步（找反证）和第四步（更新）**
//! 必须有人写下判断才能发生。没有这一层，工具只是在生产数据，用户只是被动消费。
//!
//! # 设计上的三个硬约束
//! 1. **证据带快照**：文章有保留策略、`research_events` 是级联删除，
//!    只存 `ref_id` 的话三个月后打开论点会看到一片"已删除"。见 [`EvidenceSnapshot`]。
//! 2. **变更留痕**：置信度/状态/判断文本的每次变化都记一行，
//!    半年后能回答"我当初为什么这么判断"。见 [`Revision`]。
//! 3. **反驳必须可被看见**：排序与分区在 SQL 与前端都刻意把 `oppose` 放前
//!    （对抗确认偏误，见 `README` §3.2）。
//!
//! # 边界
//! 本模块不调 AI、不做语义判断——"这条证据支持还是反驳"是**人**填的。
//! 让模型替人判断立场，等于把确认偏误换了个来源。

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// 论点状态取值
const STATUSES: [&str; 4] = ["active", "confirmed", "refuted", "archived"];
/// 证据来源类型取值
const KINDS: [&str; 3] = ["event", "highlight", "article"];
/// 证据立场取值
const STANCES: [&str; 3] = ["support", "oppose", "mixed"];

/// 论点（列表与详情共用）
///
/// 字段全部 camelCase 序列化：前端 `stores/theses.ts` 的 `Thesis` 接口如此声明。
/// 两边不一致时**不报错**，只表现为字段 `undefined`（周期报告踩过一次，见
/// `period::tests::summary_and_source_serialize_as_camel_case`）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thesis {
    /// 论点 ID
    pub id: i64,
    /// 一句话判断
    pub statement: String,
    /// 标的实体 ID
    pub entity_id: i64,
    /// 标的实体名（读数时 join 出来，避免前端再查一次）
    pub entity_name: String,
    /// 状态：active / confirmed / refuted / archived
    pub status: String,
    /// 主观置信 1–5
    pub confidence: i64,
    /// 支持证据条数
    pub support_count: i64,
    /// 反驳证据条数
    pub oppose_count: i64,
    /// 存疑证据条数
    pub mixed_count: i64,
    /// 创建时间
    pub created_at: String,
    /// 最后更新时间
    pub updated_at: String,
}

/// 一条证据（含快照）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// 关联行 ID
    pub id: i64,
    /// 来源类型：event / highlight / article
    pub kind: String,
    /// 来源行 ID
    pub ref_id: i64,
    /// 立场：support / oppose / mixed
    pub stance: String,
    /// 我为什么认/不认它
    pub note: String,
    /// 快照正文（源被删后这是唯一可读内容）
    pub snapshot_text: String,
    /// 快照时间
    pub snapshot_at: String,
    /// 创建时间
    pub created_at: String,
    /// 来源文章 ID（可跳转；event/highlight 能反查到，article 本身就是）
    pub article_id: Option<i64>,
    /// 来源文章标题（可跳转时给出）
    pub article_title: String,
}

/// 一次变更留痕
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    /// 行 ID
    pub id: i64,
    /// 变化的字段：confidence / status / statement
    pub field: String,
    /// 旧值
    pub from_value: String,
    /// 新值
    pub to_value: String,
    /// 变更理由
    pub reason: String,
    /// 时间
    pub created_at: String,
}

/// 论点详情载荷（一次取全，减少往返）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThesisDetail {
    /// 论点本体
    pub thesis: Thesis,
    /// 证据（按"反驳 → 存疑 → 支持"排序，见模块注释第 3 条）
    pub evidence: Vec<Evidence>,
    /// 变更记录（时间倒序）
    pub revisions: Vec<Revision>,
}

/// 新建论点的入参
#[derive(Debug, Clone, Deserialize)]
pub struct NewThesis {
    /// 一句话判断
    pub statement: String,
    /// 标的实体 ID
    pub entity_id: i64,
    /// 初始置信 1–5（缺省 3）
    pub confidence: Option<i64>,
}

/// 更新论点的入参（None = 不改）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ThesisPatch {
    /// 新的一句话判断
    pub statement: Option<String>,
    /// 新状态
    pub status: Option<String>,
    /// 新置信度
    pub confidence: Option<i64>,
    /// 本次变更的理由（写进 thesis_revisions.reason）
    pub reason: Option<String>,
}

/// 挂证据的入参
#[derive(Debug, Clone, Deserialize)]
pub struct NewEvidence {
    /// 论点 ID
    pub thesis_id: i64,
    /// 来源类型
    pub kind: String,
    /// 来源行 ID
    pub ref_id: i64,
    /// 立场
    pub stance: String,
    /// 备注
    pub note: Option<String>,
}

// ─── 内部工具 ──────────────────────────────────────────────────────────────────

/// 校验并规范化枚举取值
///
/// 非法值一律报错而不是静默降级：这些值会直接决定界面分区，
/// 猜一个默认值会让证据悄悄跑到错误的分区里。
fn check<'a>(value: &'a str, allowed: &[&str], what: &str) -> Result<&'a str, String> {
    let v = value.trim();
    if allowed.contains(&v) {
        Ok(v)
    } else {
        Err(format!("{what}取值非法：{v}"))
    }
}

/// 置信度夹到 1–5
fn clamp_confidence(v: i64) -> i64 {
    v.clamp(1, 5)
}

/// 判断文本的硬上限（前端也限，但后端必须自己守——防绕过）
const MAX_STATEMENT: usize = 400;

/// 截断到字符数（不是字节数，中文一字符多字节）
fn clip_statement(s: &str) -> String {
    s.trim().chars().take(MAX_STATEMENT).collect()
}

// ─── 读 ────────────────────────────────────────────────────────────────────────

/// 论点列表的统一 SELECT 前缀
///
/// 证据计数用三个相关子查询而不是 JOIN + GROUP BY：证据量很小（一个论点几条到几十条），
/// 子查询可走 `idx_evidence_thesis`，比聚合更快也更易读。
const LIST_SQL: &str = r#"
    SELECT t.id, t.statement, t.entity_id, t.status, t.confidence,
           t.created_at, t.updated_at, COALESCE(e.name, '') AS entity_name,
           (SELECT COUNT(*) FROM thesis_evidence v WHERE v.thesis_id = t.id AND v.stance = 'support') AS support_count,
           (SELECT COUNT(*) FROM thesis_evidence v WHERE v.thesis_id = t.id AND v.stance = 'oppose')  AS oppose_count,
           (SELECT COUNT(*) FROM thesis_evidence v WHERE v.thesis_id = t.id AND v.stance = 'mixed')   AS mixed_count
    FROM theses t
    LEFT JOIN entities e ON e.id = t.entity_id
"#;

/// 把一行聚合成 [`Thesis`]
fn row_to_thesis(r: &sqlx::sqlite::SqliteRow) -> Thesis {
    Thesis {
        id: r.get("id"),
        statement: r.get("statement"),
        entity_id: r.get("entity_id"),
        entity_name: r.get("entity_name"),
        status: r.get("status"),
        confidence: r.get("confidence"),
        support_count: r.get("support_count"),
        oppose_count: r.get("oppose_count"),
        mixed_count: r.get("mixed_count"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    }
}

/// 列出论点
///
/// # 参数
/// - `status`：按状态筛选，`None`/`"all"` 表示不过滤（归档态默认隐藏，除非显式要归档）
/// - `entity_id`：按标的筛选
///
/// # 排序
/// 观察中的在前（它们需要持续跟进），其次按更新时间倒序。
/// 已归档的永远排最后——归档的语义就是"暂时不用看"。
pub async fn list(
    pool: &SqlitePool,
    status: Option<&str>,
    entity_id: Option<i64>,
) -> Result<Vec<Thesis>, String> {
    let mut sql = String::from(LIST_SQL);
    let mut clauses: Vec<&str> = Vec::new();
    let want_archived = matches!(status, Some("archived")) || matches!(status, Some("all"));
    if !want_archived {
        clauses.push("t.status <> 'archived'");
    }
    if let Some(s) = status {
        if s != "all" && s != "archived" {
            clauses.push("t.status = ?");
        } else if s == "archived" {
            clauses.push("t.status = 'archived'");
        }
    }
    if entity_id.is_some() {
        clauses.push("t.entity_id = ?");
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(
        " ORDER BY CASE t.status WHEN 'active' THEN 0 WHEN 'confirmed' THEN 1 \
                  WHEN 'refuted' THEN 2 ELSE 3 END, t.updated_at DESC, t.id DESC",
    );

    let mut q = sqlx::query(&sql);
    if let Some(s) = status {
        if s != "all" && s != "archived" {
            q = q.bind(s.trim());
        }
    }
    if let Some(id) = entity_id {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(pool)
        .await
        .map_err(|e| format!("读取论点列表失败：{e}"))?;
    Ok(rows.iter().map(row_to_thesis).collect())
}

/// 按 ID 取论点本体
pub async fn get_thesis(pool: &SqlitePool, id: i64) -> Result<Thesis, String> {
    let sql = format!("{LIST_SQL} WHERE t.id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("读取论点失败：{e}"))?
        .ok_or_else(|| "该论点已不存在".to_string())?;
    Ok(row_to_thesis(&row))
}

/// 取论点完整详情：本体 + 证据 + 变更记录
///
/// 证据排序刻意把 `oppose` 放最前（0），`mixed` 居中，`support` 最后：
/// 这是对抗确认偏误的实现位置之一。前端即使改了这个顺序，
/// 详情页也会再按 stance 分组渲染，双保险。
pub async fn get_detail(pool: &SqlitePool, id: i64) -> Result<ThesisDetail, String> {
    let thesis = get_thesis(pool, id).await?;

    // 证据：JOIN 出源文章的 id/title，供"跳到原文"用。
    // 用 LEFT JOIN 而不是 INNER：源数据被删后证据仍要能看到（靠 snapshot_text）。
    let ev_rows = sqlx::query(
        r#"
        SELECT v.id, v.kind, v.ref_id, v.stance, v.note, v.snapshot_text,
               v.snapshot_at, v.created_at,
               CASE v.kind
                 WHEN 'event'     THEN (SELECT re.article_id FROM research_events re WHERE re.id = v.ref_id)
                 WHEN 'highlight' THEN (SELECT h.article_id  FROM highlights h    WHERE h.id  = v.ref_id)
                 WHEN 'article'   THEN v.ref_id
               END AS src_article_id,
               COALESCE(CASE v.kind
                 WHEN 'event'     THEN (SELECT a.title FROM research_events re JOIN articles a ON a.id = re.article_id WHERE re.id = v.ref_id)
                 WHEN 'highlight' THEN (SELECT a.title FROM highlights h JOIN articles a ON a.id = h.article_id WHERE h.id = v.ref_id)
                 WHEN 'article'   THEN (SELECT a.title FROM articles a WHERE a.id = v.ref_id)
               END, '') AS src_article_title
        FROM thesis_evidence v
        WHERE v.thesis_id = ?
        ORDER BY CASE v.stance WHEN 'oppose' THEN 0 WHEN 'mixed' THEN 1 ELSE 2 END,
                 v.created_at DESC, v.id DESC
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取证据失败：{e}"))?;

    let evidence = ev_rows
        .iter()
        .map(|r| Evidence {
            id: r.get("id"),
            kind: r.get("kind"),
            ref_id: r.get("ref_id"),
            stance: r.get("stance"),
            note: r.get("note"),
            snapshot_text: r.get("snapshot_text"),
            snapshot_at: r.get("snapshot_at"),
            created_at: r.get("created_at"),
            article_id: r.get::<Option<i64>, _>("src_article_id"),
            article_title: r.get("src_article_title"),
        })
        .collect();

    // 变更记录：时间倒序（最近的变化在最上面，这是"我最近怎么想的"）
    let rev_rows = sqlx::query(
        "SELECT id, field, from_value, to_value, reason, created_at \
         FROM thesis_revisions WHERE thesis_id = ? ORDER BY created_at DESC, id DESC",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取变更记录失败：{e}"))?;

    let revisions = rev_rows
        .iter()
        .map(|r| Revision {
            id: r.get("id"),
            field: r.get("field"),
            from_value: r.get("from_value"),
            to_value: r.get("to_value"),
            reason: r.get("reason"),
            created_at: r.get("created_at"),
        })
        .collect();

    Ok(ThesisDetail {
        thesis,
        evidence,
        revisions,
    })
}

/// 反查：某条来源被挂到过哪些论点
///
/// 用于阅读时显示"这条我评过没有"，避免重复挂载。
/// 返回 `(thesis_id, stance)` 列表。
pub async fn evidence_theses_of(
    pool: &SqlitePool,
    kind: &str,
    ref_id: i64,
) -> Result<Vec<(i64, String)>, String> {
    let rows = sqlx::query("SELECT thesis_id, stance FROM thesis_evidence WHERE kind = ? AND ref_id = ?")
        .bind(kind.trim())
        .bind(ref_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("反查证据失败：{e}"))?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<i64, _>("thesis_id"), r.get::<String, _>("stance")))
        .collect())
}

// ─── 写 ────────────────────────────────────────────────────────────────────────

/// 新建论点
///
/// 返回新行 ID。`statement` 为空直接拒绝——空判断的论点没有意义，
/// 允许它存在只会让台账变成待办清单。
pub async fn create(pool: &SqlitePool, input: NewThesis) -> Result<i64, String> {
    let statement = clip_statement(&input.statement);
    if statement.is_empty() {
        return Err("论点的判断不能为空".into());
    }

    // 实体必须存在：标的悬空的论点无法被"该实体的事件"供血，退化成普通笔记
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM entities WHERE id = ?")
        .bind(input.entity_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("校验实体失败：{e}"))?;
    if exists.is_none() {
        return Err("标的实体不存在，请先跟踪该实体".into());
    }

    let confidence = clamp_confidence(input.confidence.unwrap_or(3));
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO theses (statement, entity_id, status, confidence) \
         VALUES (?, ?, 'active', ?) RETURNING id",
    )
    .bind(&statement)
    .bind(input.entity_id)
    .bind(confidence)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("新建论点失败：{e}"))?;

    // 建立动作本身也留痕：半年后回看，"论点是什么时候建的"是最常问的问题
    record_revision(pool, id, "status", "", "active", "建立论点").await?;
    Ok(id)
}

/// 更新论点
///
/// 只对**真正变化的字段**写变更记录。全量写会让历史里塞满"3 → 3"的噪声，
/// 把真正的判断变化淹掉。
///
/// # 参数
/// - `patch`：各字段 `None` 表示不改
///
/// # 返回
/// 无（成功即返回）。论点不存在时报错。
pub async fn update(pool: &SqlitePool, id: i64, patch: ThesisPatch) -> Result<(), String> {
    let old = get_thesis(pool, id).await?;

    let mut sets: Vec<&str> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    let mut revs: Vec<(&str, String, String)> = Vec::new();
    let reason = patch.reason.unwrap_or_default();

    if let Some(s) = patch.statement {
        let s = clip_statement(&s);
        if s.is_empty() {
            return Err("论点的判断不能为空".into());
        }
        if s != old.statement {
            sets.push("statement = ?");
            binds.push(s.clone());
            revs.push(("statement", old.statement.clone(), s));
        }
    }
    if let Some(st) = patch.status {
        let st = check(&st, &STATUSES, "状态")?.to_string();
        if st != old.status {
            sets.push("status = ?");
            binds.push(st.clone());
            revs.push(("status", old.status.clone(), st));
        }
    }
    if let Some(c) = patch.confidence {
        let c = clamp_confidence(c);
        if c != old.confidence {
            sets.push("confidence = ?");
            binds.push(c.to_string());
            revs.push(("confidence", old.confidence.to_string(), c.to_string()));
        }
    }

    if sets.is_empty() {
        return Ok(());
    }
    sets.push("updated_at = CURRENT_TIMESTAMP");

    let sql = format!("UPDATE theses SET {} WHERE id = ?", sets.join(", "));
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    q.bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("更新论点失败：{e}"))?;

    for (field, from, to) in revs {
        record_revision(pool, id, field, &from, &to, &reason).await?;
    }
    Ok(())
}

/// 删除论点（证据与变更记录随 CASCADE 删；未开外键时显式删）
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
    // 显式删子表：PRAGMA foreign_keys 未开启时 ON DELETE CASCADE 不生效，
    // 会留下一堆孤儿证据（项目已在 report_sources 上踩过同一个坑）
    sqlx::query("DELETE FROM thesis_evidence WHERE thesis_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除证据失败：{e}"))?;
    sqlx::query("DELETE FROM thesis_revisions WHERE thesis_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除变更记录失败：{e}"))?;
    sqlx::query("DELETE FROM theses WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除论点失败：{e}"))?;
    Ok(())
}

/// 挂一条证据
///
/// # 快照策略
/// 三种来源各取一段可读文本存进 `snapshot_text`：
/// - `event` → `fact` + `evidence`（机器抽的事实及其原文锚点）
/// - `highlight` → 高亮文本 + 批注
/// - `article` → 标题 + 摘要（不存正文：正文可能很长，而论点里的证据不需要全文）
///
/// # 幂等
/// 同一论点 + 同一来源只存一行（表上有 UNIQUE）。重复挂载时**更新立场与备注**——
/// 用户改主意（支持→反驳）时应该就地改，而不是报"已存在"。
pub async fn add_evidence(pool: &SqlitePool, input: NewEvidence) -> Result<i64, String> {
    let kind = check(&input.kind, &KINDS, "证据类型")?.to_string();
    let stance = check(&input.stance, &STANCES, "证据立场")?.to_string();
    let note = input.note.unwrap_or_default();

    // 论点必须存在（悬空证据没有任何意义）
    get_thesis(pool, input.thesis_id).await?;

    let (snapshot, article_id) = load_snapshot(pool, &kind, input.ref_id).await?;
    let _ = article_id;

    // 同一论点下同一来源：改立场/备注（用户改主意是正常行为，不该报错）
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM thesis_evidence WHERE thesis_id = ? AND kind = ? AND ref_id = ?",
    )
    .bind(input.thesis_id)
    .bind(&kind)
    .bind(input.ref_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询已有证据失败：{e}"))?;

    if let Some(eid) = existing {
        sqlx::query("UPDATE thesis_evidence SET stance = ?, note = ?, snapshot_text = ?, snapshot_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(&stance)
            .bind(&note)
            .bind(&snapshot)
            .bind(eid)
            .execute(pool)
            .await
            .map_err(|e| format!("更新证据失败：{e}"))?;
        // 证据变了，论点"最近动过"的时间要跟着走（列表按 updated_at 排序）
        touch(pool, input.thesis_id).await?;
        return Ok(eid);
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance, note, snapshot_text) \
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(input.thesis_id)
    .bind(&kind)
    .bind(input.ref_id)
    .bind(&stance)
    .bind(&note)
    .bind(&snapshot)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("挂载证据失败：{e}"))?;
    touch(pool, input.thesis_id).await?;
    Ok(id)
}

/// 摘掉一条证据
pub async fn remove_evidence(pool: &SqlitePool, evidence_id: i64) -> Result<(), String> {
    let thesis_id: Option<i64> =
        sqlx::query_scalar("SELECT thesis_id FROM thesis_evidence WHERE id = ?")
            .bind(evidence_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("查询证据失败：{e}"))?;
    sqlx::query("DELETE FROM thesis_evidence WHERE id = ?")
        .bind(evidence_id)
        .execute(pool)
        .await
        .map_err(|e| format!("移除证据失败：{e}"))?;
    if let Some(tid) = thesis_id {
        touch(pool, tid).await?;
    }
    Ok(())
}

// ─── 内部：快照与留痕 ──────────────────────────────────────────────────────────

/// 更新论点的 `updated_at`
async fn touch(pool: &SqlitePool, thesis_id: i64) -> Result<(), String> {
    sqlx::query("UPDATE theses SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(thesis_id)
        .execute(pool)
        .await
        .map_err(|e| format!("刷新论点时间失败：{e}"))?;
    Ok(())
}

/// 写一行变更记录
async fn record_revision(
    pool: &SqlitePool,
    thesis_id: i64,
    field: &str,
    from: &str,
    to: &str,
    reason: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO thesis_revisions (thesis_id, field, from_value, to_value, reason) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(thesis_id)
    .bind(field)
    .bind(from)
    .bind(to)
    .bind(reason)
    .execute(pool)
    .await
    .map_err(|e| format!("记录变更失败：{e}"))?;
    Ok(())
}

/// 按来源类型取快照文本
///
/// 返回 `(快照文本, 来源文章 ID)`。来源不存在时报错——
/// 允许挂一个不存在的证据，等于让台账记录幻觉。
async fn load_snapshot(
    pool: &SqlitePool,
    kind: &str,
    ref_id: i64,
) -> Result<(String, Option<i64>), String> {
    match kind {
        "event" => {
            let row = sqlx::query(
                "SELECT fact, evidence, article_id FROM research_events WHERE id = ?",
            )
            .bind(ref_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("读取事件失败：{e}"))?
            .ok_or_else(|| "这条研究事件已不存在".to_string())?;
            let fact: String = row.get("fact");
            let evidence: String = row.get("evidence");
            let article_id: i64 = row.get("article_id");
            let snap = if evidence.trim().is_empty() {
                fact
            } else {
                format!("{fact}\n\n原文：{evidence}")
            };
            Ok((snap, Some(article_id)))
        }
        "highlight" => {
            let row = sqlx::query(
                "SELECT text, note, article_id FROM highlights WHERE id = ?",
            )
            .bind(ref_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("读取高亮失败：{e}"))?
            .ok_or_else(|| "这条高亮已不存在".to_string())?;
            let text: String = row.get("text");
            let note: String = row.get("note");
            let article_id: i64 = row.get("article_id");
            let snap = if note.trim().is_empty() {
                text
            } else {
                format!("{text}\n\n批注：{note}")
            };
            Ok((snap, Some(article_id)))
        }
        "article" => {
            let row = sqlx::query("SELECT title, summary FROM articles WHERE id = ?")
                .bind(ref_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("读取文章失败：{e}"))?
                .ok_or_else(|| "这篇文章已不存在".to_string())?;
            let title: String = row.get("title");
            let summary: String = row.get("summary");
            let snap = if summary.trim().is_empty() {
                title
            } else {
                format!("{title}\n\n{summary}")
            };
            Ok((snap, Some(ref_id)))
        }
        // 上面 check() 已挡住非法 kind，这里只是穷尽 match
        other => Err(format!("未知的证据类型：{other}")),
    }
}

// ─── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 建一个内存库并落表（只建本模块用得到的表）
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("建库失败");
        sqlx::query(
            "CREATE TABLE entities (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, \
             entity_type TEXT NOT NULL DEFAULT 'company', aliases TEXT NOT NULL DEFAULT '[]', \
             enabled INTEGER DEFAULT 1, alert_rules TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("建实体表失败");
        sqlx::query(
            "CREATE TABLE articles (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, \
             summary TEXT DEFAULT '', content TEXT DEFAULT '')",
        )
        .execute(&pool)
        .await
        .expect("建文章表失败");
        sqlx::query(
            "CREATE TABLE research_events (id INTEGER PRIMARY KEY AUTOINCREMENT, \
             article_id INTEGER NOT NULL, entity_id INTEGER NOT NULL, event_type TEXT NOT NULL, \
             event_date TEXT DEFAULT '', fact TEXT NOT NULL, evidence TEXT NOT NULL DEFAULT '', \
             source_model TEXT DEFAULT '', created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("建事件表失败");
        sqlx::query(
            "CREATE TABLE highlights (id INTEGER PRIMARY KEY AUTOINCREMENT, \
             article_id INTEGER NOT NULL, text TEXT NOT NULL, note TEXT DEFAULT '', \
             color TEXT DEFAULT 'yellow', start_offset INTEGER DEFAULT 0, \
             end_offset INTEGER DEFAULT 0, created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .expect("建高亮表失败");
        for ddl in [
            r#"CREATE TABLE theses (
                id INTEGER PRIMARY KEY AUTOINCREMENT, statement TEXT NOT NULL, entity_id INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'active', confidence INTEGER NOT NULL DEFAULT 3,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP)"#,
            r#"CREATE TABLE thesis_evidence (
                id INTEGER PRIMARY KEY AUTOINCREMENT, thesis_id INTEGER NOT NULL, kind TEXT NOT NULL,
                ref_id INTEGER NOT NULL, stance TEXT NOT NULL, note TEXT NOT NULL DEFAULT '',
                snapshot_text TEXT NOT NULL DEFAULT '',
                snapshot_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(thesis_id, kind, ref_id))"#,
            r#"CREATE TABLE thesis_revisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT, thesis_id INTEGER NOT NULL, field TEXT NOT NULL,
                from_value TEXT NOT NULL DEFAULT '', to_value TEXT NOT NULL DEFAULT '',
                reason TEXT NOT NULL DEFAULT '', created_at DATETIME DEFAULT CURRENT_TIMESTAMP)"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("建论点表失败");
        }
        pool
    }

    /// 插入一个实体与一篇文章，返回 (entity_id, article_id)
    async fn seed(pool: &SqlitePool) -> (i64, i64) {
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO entities (name, entity_type) VALUES ('宁德时代', 'company') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .expect("插实体失败");
        let aid: i64 = sqlx::query_scalar(
            "INSERT INTO articles (title, summary) VALUES ('储能出货超预期', 'Q3 出货 22GWh') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .expect("插文章失败");
        (eid, aid)
    }

    #[tokio::test]
    async fn create_rejects_empty_statement_and_missing_entity() {
        let pool = test_pool().await;
        let (eid, _) = seed(&pool).await;

        assert!(create(&pool, NewThesis { statement: "  ".into(), entity_id: eid, confidence: None })
            .await
            .is_err());
        assert!(create(&pool, NewThesis { statement: "有判断".into(), entity_id: 9999, confidence: None })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn create_clamps_confidence_and_records_birth() {
        let pool = test_pool().await;
        let (eid, _) = seed(&pool).await;
        let id = create(
            &pool,
            NewThesis { statement: "储能出货会超预期".into(), entity_id: eid, confidence: Some(9) },
        )
        .await
        .expect("新建失败");

        let t = get_thesis(&pool, id).await.expect("读取失败");
        assert_eq!(t.confidence, 5, "置信度应夹到 1–5");
        assert_eq!(t.entity_name, "宁德时代", "实体名应 join 出来");
        let d = get_detail(&pool, id).await.expect("详情失败");
        assert_eq!(d.revisions.len(), 1, "建立动作要留痕");
        assert_eq!(d.revisions[0].field, "status");
    }

    #[tokio::test]
    async fn update_only_records_real_changes() {
        let pool = test_pool().await;
        let (eid, _) = seed(&pool).await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: Some(3) })
            .await
            .expect("新建失败");

        // 全量传原值：不应产生新记录（否则历史里全是 "3 → 3" 噪声）
        update(&pool, id, ThesisPatch { confidence: Some(3), status: Some("active".into()), ..Default::default() })
            .await
            .expect("更新失败");
        let d = get_detail(&pool, id).await.expect("详情失败");
        assert_eq!(d.revisions.len(), 1, "无变化不该留痕");

        update(
            &pool,
            id,
            ThesisPatch {
                confidence: Some(2),
                status: Some("refuted".into()),
                reason: Some("欧洲库存去化慢".into()),
                ..Default::default()
            },
        )
        .await
        .expect("更新失败");
        let d = get_detail(&pool, id).await.expect("详情失败");
        assert_eq!(d.revisions.len(), 3, "建立 + 两处变更");
        assert_eq!(d.thesis.status, "refuted");
        assert_eq!(d.thesis.confidence, 2);
        assert!(
            d.revisions.iter().any(|r| r.reason == "欧洲库存去化慢"),
            "变更理由要留下来"
        );
    }

    #[tokio::test]
    async fn update_rejects_illegal_status() {
        let pool = test_pool().await;
        let (eid, _) = seed(&pool).await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        assert!(update(&pool, id, ThesisPatch { status: Some("whatever".into()), ..Default::default() })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn evidence_snapshot_survives_after_source_deleted() {
        let pool = test_pool().await;
        let (eid, aid) = seed(&pool).await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");

        let ev: i64 = sqlx::query_scalar(
            "INSERT INTO research_events (article_id, entity_id, event_type, fact, evidence) \
             VALUES (?, ?, 'capacity', '二期产能提前投产', '计划提前一个季度') RETURNING id",
        )
        .bind(aid)
        .bind(eid)
        .fetch_one(&pool)
        .await
        .expect("插事件失败");

        add_evidence(
            &pool,
            NewEvidence {
                thesis_id: id,
                kind: "event".into(),
                ref_id: ev,
                stance: "support".into(),
                note: Some("投产节奏快于预期".into()),
            },
        )
        .await
        .expect("挂证据失败");

        // 模拟文章被保留策略清理：事件随 CASCADE 一起没了
        sqlx::query("DELETE FROM research_events WHERE id = ?")
            .bind(ev)
            .execute(&pool)
            .await
            .expect("删事件失败");

        let d = get_detail(&pool, id).await.expect("详情失败");
        assert_eq!(d.evidence.len(), 1, "源被删后证据仍要在");
        assert!(
            d.evidence[0].snapshot_text.contains("二期产能提前投产"),
            "快照要能独立可读，实际：{}",
            d.evidence[0].snapshot_text
        );
    }

    #[tokio::test]
    async fn evidence_oppose_sorts_first() {
        let pool = test_pool().await;
        let (eid, aid) = seed(&pool).await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");

        for (stance, text) in [("support", "支持的理由"), ("oppose", "反驳的理由"), ("mixed", "存疑的理由")] {
            let ev: i64 = sqlx::query_scalar(
                "INSERT INTO research_events (article_id, entity_id, event_type, fact) \
                 VALUES (?, ?, 'x', ?) RETURNING id",
            )
            .bind(aid)
            .bind(eid)
            .bind(text)
            .fetch_one(&pool)
            .await
            .expect("插事件失败");
            add_evidence(
                &pool,
                NewEvidence {
                    thesis_id: id,
                    kind: "event".into(),
                    ref_id: ev,
                    stance: stance.into(),
                    note: None,
                },
            )
            .await
            .expect("挂证据失败");
        }

        let d = get_detail(&pool, id).await.expect("详情失败");
        assert_eq!(d.evidence[0].stance, "oppose", "反驳必须排最前（对抗确认偏误）");
        assert_eq!(d.evidence[2].stance, "support");
        assert_eq!(d.thesis.support_count, 1);
        assert_eq!(d.thesis.oppose_count, 1);
    }

    #[tokio::test]
    async fn re_adding_same_source_updates_stance_instead_of_duplicating() {
        let pool = test_pool().await;
        let (eid, aid) = seed(&pool).await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");

        let first = add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "support".into(), note: None },
        )
        .await
        .expect("首次挂载失败");
        // 改主意：从支持改为反驳
        let second = add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "oppose".into(), note: Some("看错了".into()) },
        )
        .await
        .expect("二次挂载失败");

        assert_eq!(first, second, "同源应就地更新，不新增行");
        let d = get_detail(&pool, id).await.expect("详情失败");
        assert_eq!(d.evidence.len(), 1);
        assert_eq!(d.evidence[0].stance, "oppose");
        assert_eq!(d.evidence[0].note, "看错了");
    }

    #[tokio::test]
    async fn delete_removes_children_explicitly() {
        let pool = test_pool().await;
        let (eid, aid) = seed(&pool).await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "mixed".into(), note: None },
        )
        .await
        .expect("挂证据失败");

        delete(&pool, id).await.expect("删除失败");
        let ev: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_evidence")
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        let rev: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_revisions")
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        assert_eq!((ev, rev), (0, 0), "子表要显式清掉（外键 CASCADE 不可依赖）");
    }

    #[tokio::test]
    async fn list_hides_archived_unless_asked() {
        let pool = test_pool().await;
        let (eid, _) = seed(&pool).await;
        let a = create(&pool, NewThesis { statement: "甲".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        let b = create(&pool, NewThesis { statement: "乙".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        update(&pool, b, ThesisPatch { status: Some("archived".into()), ..Default::default() })
            .await
            .expect("归档失败");

        let visible = list(&pool, None, None).await.expect("列表失败");
        assert_eq!(visible.len(), 1, "归档的默认不出现");
        assert_eq!(visible[0].id, a);

        let archived = list(&pool, Some("archived"), None).await.expect("列表失败");
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, b);

        let all = list(&pool, Some("all"), None).await.expect("列表失败");
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn evidence_theses_of_finds_backlink() {
        let pool = test_pool().await;
        let (eid, aid) = seed(&pool).await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "support".into(), note: None },
        )
        .await
        .expect("挂证据失败");

        let found = evidence_theses_of(&pool, "article", aid).await.expect("反查失败");
        assert_eq!(found, vec![(id, "support".to_string())]);
    }

    /// 序列化契约：跨边界的键名必须是 camelCase
    #[test]
    fn thesis_serializes_as_camel_case() {
        let t = Thesis {
            id: 1,
            statement: "判断".into(),
            entity_id: 2,
            entity_name: "宁德时代".into(),
            status: "active".into(),
            confidence: 3,
            support_count: 5,
            oppose_count: 2,
            mixed_count: 0,
            created_at: "2026-10-30 22:00".into(),
            updated_at: "2026-10-30 22:10".into(),
        };
        let v = serde_json::to_value(&t).expect("序列化失败");
        for key in ["entityId", "entityName", "supportCount", "opposeCount", "createdAt"] {
            assert!(v.get(key).is_some(), "缺字段 {key}");
        }
        assert!(v.get("entity_id").is_none(), "不该出现 snake_case 键");

        let ev = Evidence {
            id: 1,
            kind: "event".into(),
            ref_id: 9,
            stance: "oppose".into(),
            note: String::new(),
            snapshot_text: "事实".into(),
            snapshot_at: "2026-10-30 22:00".into(),
            created_at: "2026-10-30 22:00".into(),
            article_id: Some(3),
            article_title: "标题".into(),
        };
        let v = serde_json::to_value(&ev).expect("序列化失败");
        for key in ["refId", "snapshotText", "articleId", "articleTitle"] {
            assert!(v.get(key).is_some(), "缺字段 {key}");
        }
    }
}
