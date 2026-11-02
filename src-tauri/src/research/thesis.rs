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

/// 把一条论点组装成一页 markdown（「组装」出口的取数端）
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `id` - 论点 ID
///
/// # 返回值
/// 一段可直接粘进 Obsidian / Word 的 markdown 原文；论点不存在时返回错误
///
/// # 为什么拼接放在后端
/// 章节顺序、引用写法、日期截断都是**内容格式**，不是渲染。前端的
/// `utils/markdown.ts::renderMarkdown` 只负责把已有 markdown 变成 HTML；
/// 若拼接也搬到前端，同一套格式就会在 `period.rs` / `coverage.rs` / 这里
/// 各长一份，改一处漏一处，三个出口的产出物迟早互相不认识。
///
/// # 为什么反驳证据排在支持证据之前
/// 与 `ThesisDetail` 界面的排序一致：先看反证是这套工具的核心主张
/// （对抗确认偏误）。导出的那一页若按"支持在前"排，等于在产物里
/// 把这条主张又抹掉了——而产物是要拿给别人看的，抹得更彻底。
pub async fn compose_markdown(pool: &SqlitePool, id: i64) -> Result<String, String> {
    // 复用 get_detail：证据排序、来源 join、快照字段全在那里，
    // 这里再查一遍就成了第二份"证据怎么读"的真相
    let detail = get_detail(pool, id).await?;
    let t = &detail.thesis;

    // 判据是一对一，可能没有；没有时下面要明说，不能静默跳过
    let falsifier = sqlx::query(
        "SELECT statement, window_kind, due_at, every_n_days, verdict \
         FROM thesis_falsifiers WHERE thesis_id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("读取判据失败：{e}"))?;

    let mut md = String::new();
    md.push_str(&format!("# {}：{}\n\n", t.entity_name, t.statement));
    md.push_str(&format!(
        "> 置信 {}/5 · {} · 建立于 {}\n\n",
        t.confidence,
        status_cn(&t.status),
        date_part(&t.created_at)
    ));

    md.push_str("## 判据\n\n");
    match &falsifier {
        Some(row) => {
            md.push_str(&format!("{}\n\n", row.get::<String, _>("statement")));
            md.push_str(&format!("- 检验窗口：{}\n", window_cn(row)));
            md.push_str(&format!(
                "- 结论：{}\n\n",
                verdict_cn(&row.get::<String, _>("verdict"))
            ));
        }
        None => md.push_str(
            "_还没有写判据。判据是「出现什么我就承认自己错了」——没有它，这条判断无法被证伪。_\n\n",
        ),
    }

    // 三档固定顺序：反驳 → 存疑 → 支持。只有反驳档带空态提示：
    // 「暂无反驳证据」不等于论点成立，只说明还没遇到，这句话值得写进产物
    let groups: [(&str, &str, &str); 3] = [
        (
            "oppose",
            "反驳证据",
            "暂无反驳证据 —— 这不等于它成立，只说明还没遇到。",
        ),
        ("mixed", "存疑证据", ""),
        ("support", "支持证据", ""),
    ];
    for (stance, title, empty_hint) in groups {
        let items: Vec<&Evidence> = detail
            .evidence
            .iter()
            .filter(|e| e.stance == stance)
            .collect();
        if items.is_empty() && empty_hint.is_empty() {
            continue;
        }
        md.push_str(&format!("## {}\n\n", title));
        if items.is_empty() {
            md.push_str(&format!("_{empty_hint}_\n\n"));
            continue;
        }
        for (i, e) in items.iter().enumerate() {
            // 标题取源文章标题并标注来源类型；源文章被删时退回"某类快照"的说法，
            // **绝不写「已删除」**——快照正文还在，这一条证据依然可读
            let head = if e.article_title.is_empty() {
                format!("{}快照", kind_cn(&e.kind))
            } else {
                format!("{}（{}）", e.article_title, kind_cn(&e.kind))
            };
            md.push_str(&format!("**{}. {}**\n\n", i + 1, head));
            if !e.note.is_empty() {
                md.push_str(&format!("我的批注：{}\n\n", e.note));
            }
            let snap = clip(&e.snapshot_text, 300);
            if !snap.is_empty() {
                md.push_str(&format!("> {}\n\n", snap.replace('\n', "\n> ")));
            }
        }
    }

    if !detail.revisions.is_empty() {
        md.push_str("## 变更记录\n\n");
        for r in &detail.revisions {
            let line = format!(
                "- {} · {} {} → {}",
                date_part(&r.created_at),
                field_cn(&r.field),
                r.from_value,
                r.to_value
            );
            md.push_str(&line);
            if !r.reason.is_empty() {
                md.push_str(&format!("：{}", r.reason));
            }
            md.push('\n');
        }
        md.push('\n');
    }

    Ok(md)
}

/// 取时间串的日期部分（`2026-10-30 12:00:00` → `2026-10-30`）
///
/// 库里存的是 `CURRENT_TIMESTAMP` 的 `YYYY-MM-DD HH:MM:SS`。整串写进文档，
/// 秒级时间会出现在给人看的正文里，既吵又让人误以为精度重要。
fn date_part(ts: &str) -> &str {
    ts.split_whitespace().next().unwrap_or(ts)
}

/// 论点状态 → 中文（与前端 `STATUS_META` 逐字一致）
///
/// 刻意不复用 `coverage.rs` 的 `status_label`：那里是给覆盖清单用的口语
/// （"跟进中 / 已确认"），这里是给对外产物用的正式说法。两处文案会分叉，
/// 共享一个函数反而会把一边的措辞绑死。
fn status_cn(status: &str) -> &'static str {
    match status {
        "active" => "观察中",
        "confirmed" => "已验证",
        "refuted" => "已推翻",
        "archived" => "已归档",
        _ => "未知",
    }
}

/// 证据来源类型 → 中文（与前端 `KIND_LABEL` 一致）
fn kind_cn(kind: &str) -> &'static str {
    match kind {
        "event" => "研究事件",
        "highlight" => "高亮",
        "article" => "文章",
        _ => "来源",
    }
}

/// 判据结论 → 中文
fn verdict_cn(verdict: &str) -> &'static str {
    match verdict {
        "verified" => "已证实",
        "refuted" => "已证伪",
        "unclear" => "无法判断",
        _ => "待验证",
    }
}

/// 变更字段 → 中文
fn field_cn(field: &str) -> &'static str {
    match field {
        "confidence" => "置信度",
        "status" => "状态",
        "statement" => "判断",
        _ => "字段",
    }
}

/// 检验窗口 → 一行中文
///
/// `date` 与 `cycle` 互斥（`window_kind` 决定哪个字段有值），
/// 两个都空时给出"未设窗口"而不是空白——空白会让人以为数据丢了。
fn window_cn(row: &sqlx::sqlite::SqliteRow) -> String {
    let kind: String = row.get("window_kind");
    match kind.as_str() {
        "date" => match row.get::<Option<String>, _>("due_at") {
            Some(d) => format!("到期日 {}", date_part(&d)),
            None => "到期日未填".to_string(),
        },
        "cycle" => match row.get::<Option<i64>, _>("every_n_days") {
            Some(n) => format!("每 {} 天复核一次", n),
            None => "复核周期未填".to_string(),
        },
        _ => "未设窗口".to_string(),
    }
}

/// 按字符截断（不按字节，中文不能从半个字切开）
///
/// 超过 `max` 个字符时补省略号。快照正文最长可达整篇文章，
/// 原样导出会让"一页"变成"一篇"，失去快速转述的价值。
fn clip(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    let mut out = String::new();
    for (i, ch) in trimmed.chars().enumerate() {
        if i >= max {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
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
    //
    // 顺序是"子表的子表 → 子表 → 主表"，三级都不能反：`thesis_evidence_source`
    // 靠 `thesis_evidence.id` 定位，一旦证据行先被删掉，这条 IN 子查询就查不到
    // 任何行，孤儿行永远留下且**不报错**。
    sqlx::query(
        "DELETE FROM thesis_evidence_source \
         WHERE evidence_id IN (SELECT id FROM thesis_evidence WHERE thesis_id = ?)",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| format!("删除证据来源失败：{e}"))?;
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

    // 来源身份在**挂载那一刻**定格。文章会被清理（判据是 `published_at` 而非抓取时间），
    // 一被清理，事件/高亮随之级联消失，"谁说的、哪天说的"就永久丢失且不可回填。
    // 所以这里必须留一份独立拷贝，而不只是记一个 ref_id。
    let identity = match article_id {
        Some(aid) => load_source_identity(pool, aid).await?,
        None => None,
    };

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
        // 老证据可能还没有 source 行（本表建出来之前挂的），`INSERT OR IGNORE` 顺手补齐；
        // 已有行时**不覆盖**——来源身份记的是"我当时从哪看到的"，不该随改立场而重写。
        write_source_identity(pool, eid, article_id, &identity).await?;
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
    write_source_identity(pool, id, article_id, &identity).await?;
    Ok(id)
}

/// 读取一篇文章的来源身份（媒体名 + 发布日期）
///
/// `source_name` 取值优先级：`feeds.name`（订阅源即媒体）→ `articles.author` → 空串。
/// 用 `NULLIF(...,'')` 而不是裸 `COALESCE`：空串也该跳到下一档（`feeds.name` 为空的
/// 脏数据是存在的）。`LEFT JOIN feeds` 是为了"文章还在但 feed 行缺失"时不报错。
///
/// # 参数
/// * `article_id` - 文章 ID
///
/// # 返回
/// `Some((来源名, 发布日期))`；文章已不存在时 `None`。
/// `published_at` 为空串表示该文章没有发布时间（这类文章在清理时天然被保留）。
///
/// # 错误
/// 查询失败时返回错误信息
async fn load_source_identity(
    pool: &SqlitePool,
    article_id: i64,
) -> Result<Option<(String, String)>, String> {
    let row = sqlx::query(
        "SELECT COALESCE(NULLIF(f.name, ''), NULLIF(a.author, ''), '') AS source_name, \
                COALESCE(a.published_at, '') AS published_at \
         FROM articles a LEFT JOIN feeds f ON f.id = a.feed_id \
         WHERE a.id = ?",
    )
    .bind(article_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("读取文章来源失败：{e}"))?;
    Ok(row.map(|r| {
        let name: String = r.get("source_name");
        let published: String = r.get("published_at");
        (name, published)
    }))
}

/// 写一行证据来源身份（幂等，不覆盖已有行）
///
/// 用 `INSERT OR IGNORE` 而非 `REPLACE`，理由有二：
/// 1. **不覆盖**——来源身份是"我当时从哪看到的"，用户把立场从支持改成反驳，
///    不该连带改写来源；
/// 2. **自动补齐**——"证据行存在但没有 source 行"的情况不需要额外判断分支。
///
/// 文章查不到时（`identity` 为 `None`）仍要写行，只是 `article_id` 写 NULL、
/// 两个文本列写空串（状态②）。**不能跳过**：跳过会让它永远停在"待回填"，
/// 每次启动都白扫一遍，且无法区分"没跑过"与"跑过但当时就丢了"。
///
/// # 参数
/// * `evidence_id` - 证据行 ID
/// * `article_id` - `load_snapshot` 给出的文章 ID（可空）
/// * `identity` - `load_source_identity` 的结果
///
/// # 错误
/// 写入失败时返回错误信息——**不吞**。吞掉错误的后果是"表建了但一直是空的"，
/// 而表面上一切正常。
async fn write_source_identity(
    pool: &SqlitePool,
    evidence_id: i64,
    article_id: Option<i64>,
    identity: &Option<(String, String)>,
) -> Result<(), String> {
    // 查不到来源 ⇒ 连 article_id 也写 NULL：状态②的定义就是"行在、源没"
    let (aid, name, published) = match identity {
        Some((n, p)) => (article_id, n.as_str(), p.as_str()),
        None => (None, "", ""),
    };
    sqlx::query(
        "INSERT OR IGNORE INTO thesis_evidence_source \
         (evidence_id, article_id, source_name, published_at) VALUES (?, ?, ?, ?)",
    )
    .bind(evidence_id)
    .bind(aid)
    .bind(name)
    .bind(published)
    .execute(pool)
    .await
    .map_err(|e| format!("记录证据来源失败：{e}"))?;
    Ok(())
}

/// 摘掉一条证据
pub async fn remove_evidence(pool: &SqlitePool, evidence_id: i64) -> Result<(), String> {
    let thesis_id: Option<i64> =
        sqlx::query_scalar("SELECT thesis_id FROM thesis_evidence WHERE id = ?")
            .bind(evidence_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("查询证据失败：{e}"))?;
    // 先删子表的子表：PRAGMA foreign_keys 未开启，ON DELETE CASCADE 不生效。
    // 顺序不能反 —— 先把 thesis_evidence 行删了，就再也找不到对应的来源行了，
    // 孤儿行会静默累积（不报错、不占界面，只在库变大时才被发现）。
    sqlx::query("DELETE FROM thesis_evidence_source WHERE evidence_id = ?")
        .bind(evidence_id)
        .execute(pool)
        .await
        .map_err(|e| format!("移除证据来源失败：{e}"))?;
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
        // 订阅源与文章的列按**真实 schema** 给全：TASK-00 起 `load_source_identity`
        // 要 JOIN feeds 并读 author / published_at。缺列不会报错，只会静默取到空串
        // ——那正是"测试全绿但来源身份全是空"的来源。
        sqlx::query(
            "CREATE TABLE feeds (id INTEGER PRIMARY KEY AUTOINCREMENT, \
             name TEXT NOT NULL, url TEXT NOT NULL UNIQUE)",
        )
        .execute(&pool)
        .await
        .expect("建订阅源表失败");
        sqlx::query(
            "CREATE TABLE articles (id INTEGER PRIMARY KEY AUTOINCREMENT, \
             feed_id INTEGER NOT NULL DEFAULT 0, guid TEXT NOT NULL DEFAULT '', \
             link TEXT NOT NULL DEFAULT '', title TEXT NOT NULL, \
             summary TEXT DEFAULT '', content TEXT DEFAULT '', \
             author TEXT DEFAULT '', published_at DATETIME)",
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
            // 证据来源身份（TASK-00）。字段与 db.rs::create_tables 里的定义必须一致，
            // 这里能保留手抄是因为本模块的测试只需要论点族；一旦本表结构变更，两处同改。
            r#"CREATE TABLE thesis_evidence_source (
                evidence_id INTEGER PRIMARY KEY, article_id INTEGER,
                source_name TEXT NOT NULL DEFAULT '', published_at TEXT NOT NULL DEFAULT '',
                captured_at DATETIME DEFAULT CURRENT_TIMESTAMP)"#,
            // 判据表：写入端在 `research/review`，读取端在「组装」。
            // 缺这张表时 compose_markdown 会报 `no such table`，
            // 而报错发生在测试库、不是真实库——很容易被当成"生产代码有 bug"。
            r#"CREATE TABLE thesis_falsifiers (
                id INTEGER PRIMARY KEY AUTOINCREMENT, thesis_id INTEGER NOT NULL UNIQUE,
                statement TEXT NOT NULL, window_kind TEXT NOT NULL, due_at DATETIME,
                every_n_days INTEGER, keywords TEXT NOT NULL DEFAULT '[]',
                event_type TEXT NOT NULL DEFAULT '', entity_id INTEGER,
                verdict TEXT NOT NULL DEFAULT 'pending', verdict_evidence_id INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP)"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("建论点表失败");
        }
        pool
    }

    /// 插入一个实体与一篇文章，返回 (entity_id, article_id)
    async fn seed(pool: &SqlitePool) -> (i64, i64) {
        let (eid, aid, _) = seed_with_source(pool, "", "", "").await;
        (eid, aid)
    }

    /// 插入一个订阅源 + 一篇带来源身份的文章 + 一个实体
    ///
    /// @param feed_name - 订阅源名（媒体名来源的第一档）
    /// @param author - 作者（`feeds.name` 为空时才会被取到）
    /// @param published_at - 发布日期，空串表示没有发布时间
    /// @returns `(entity_id, article_id, feed_id)`
    async fn seed_with_source(
        pool: &SqlitePool,
        feed_name: &str,
        author: &str,
        published_at: &str,
    ) -> (i64, i64, i64) {
        // feeds.url 有 UNIQUE 约束，同一测试里可能建多个源，靠序号区分
        static FEED_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = FEED_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let feed_id: i64 = sqlx::query_scalar("INSERT INTO feeds (name, url) VALUES (?, ?) RETURNING id")
            .bind(feed_name)
            .bind(format!("https://example.com/feed-{seq}"))
            .fetch_one(pool)
            .await
            .expect("插订阅源失败");
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO entities (name, entity_type) VALUES ('宁德时代', 'company') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .expect("插实体失败");
        let aid: i64 = sqlx::query_scalar(
            "INSERT INTO articles (feed_id, guid, link, title, summary, author, published_at) \
             VALUES (?, 'g1', 'https://example.com/a1', '储能出货超预期', 'Q3 出货 22GWh', ?, ?) \
             RETURNING id",
        )
        .bind(feed_id)
        .bind(author)
        .bind(published_at)
        .fetch_one(pool)
        .await
        .expect("插文章失败");
        (eid, aid, feed_id)
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

    // ─── TASK-00：证据来源身份 ────────────────────────────────────────────────

    #[tokio::test]
    async fn add_evidence_writes_source_snapshot() {
        let pool = test_pool().await;
        let (eid, aid, _) = seed_with_source(&pool, "财新", "张三", "2026-10-28").await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "support".into(), note: None },
        )
        .await
        .expect("挂证据失败");

        let row = sqlx::query(
            "SELECT article_id, source_name, published_at, captured_at FROM thesis_evidence_source",
        )
        .fetch_one(&pool)
        .await
        .expect("来源行应存在");

        assert_eq!(row.get::<i64, _>("article_id"), aid, "应记下文章 ID");
        assert_eq!(
            row.get::<String, _>("source_name"),
            "财新",
            "订阅源名优先于作者（feed 名才是媒体）"
        );
        assert!(
            row.get::<String, _>("published_at").contains("2026-10-28"),
            "发布日期要快照下来"
        );
        assert!(
            row.get::<Option<String>, _>("captured_at").is_some(),
            "captured_at 不该为空"
        );
    }

    #[tokio::test]
    async fn source_snapshot_survives_source_deleted() {
        let pool = test_pool().await;
        let (eid, aid, _) = seed_with_source(&pool, "财新", "张三", "2026-10-28").await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "support".into(), note: None },
        )
        .await
        .expect("挂证据失败");

        // 文章被保留策略清理（`thesis_evidence.ref_id` 故意不建外键，证据行留着）
        sqlx::query("DELETE FROM articles WHERE id = ?")
            .bind(aid)
            .execute(&pool)
            .await
            .expect("删文章失败");

        let row = sqlx::query("SELECT article_id, source_name FROM thesis_evidence_source")
            .fetch_one(&pool)
            .await
            .expect("来源行必须还在——它的全部价值就是文章没了它还在");
        assert_eq!(
            row.get::<i64, _>("article_id"),
            aid,
            "article_id 是快照，不该跟着文章消失"
        );
        assert_eq!(
            row.get::<String, _>("source_name"),
            "财新",
            "来源名更要留住：snapshot_text 保住了说什么，这张表保住谁说的"
        );
    }

    #[tokio::test]
    async fn re_attaching_same_source_keeps_original_identity() {
        let pool = test_pool().await;
        let (eid, aid, _) = seed_with_source(&pool, "财新", "张三", "2026-10-28").await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "support".into(), note: None },
        )
        .await
        .expect("首次挂载失败");
        // 打一个可识别的旧时间戳：CURRENT_TIMESTAMP 的秒级精度会让"被覆盖成新值"
        // 与"压根没动"看起来一样，测不出 INSERT OR IGNORE 与 REPLACE 的区别
        sqlx::query("UPDATE thesis_evidence_source SET captured_at = '2000-01-01 00:00:00'")
            .execute(&pool)
            .await
            .expect("打旧时间戳失败");

        // 改立场：走到 add_evidence 的 UPDATE 分支
        add_evidence(
            &pool,
            NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "oppose".into(), note: None },
        )
        .await
        .expect("改立场失败");

        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_evidence_source")
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        assert_eq!(n, 1, "改立场不该多出一行");
        let captured: String = sqlx::query_scalar("SELECT captured_at FROM thesis_evidence_source")
            .fetch_one(&pool)
            .await
            .expect("读时间失败");
        assert_eq!(
            captured, "2000-01-01 00:00:00",
            "来源身份在首次挂载时定格：它记的是「我当时从哪看到的」，不随立场改写"
        );
    }

    #[tokio::test]
    async fn delete_removes_evidence_source_rows() {
        let pool = test_pool().await;
        let (eid, aid1, _) = seed_with_source(&pool, "财新", "张三", "2026-10-28").await;
        let (_, aid2, _) = seed_with_source(&pool, "路透", "李四", "2026-10-29").await;
        let id = create(&pool, NewThesis { statement: "判断".into(), entity_id: eid, confidence: None })
            .await
            .expect("新建失败");
        for aid in [aid1, aid2] {
            add_evidence(
                &pool,
                NewEvidence { thesis_id: id, kind: "article".into(), ref_id: aid, stance: "support".into(), note: None },
            )
            .await
            .expect("挂证据失败");
        }

        delete(&pool, id).await.expect("删除失败");

        let ev: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_evidence")
            .fetch_one(&pool)
            .await
            .expect("统计证据失败");
        let src: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_evidence_source")
            .fetch_one(&pool)
            .await
            .expect("统计来源失败");
        assert_eq!(ev, 0, "证据应被清空");
        assert_eq!(
            src, 0,
            "来源行也必须清掉：PRAGMA foreign_keys 未开启，CASCADE 不生效，只能显式删"
        );
    }

    /// 「组装」产出：反驳排在支持之前、没有证据的档位不留空小节、没判据要说人话
    ///
    /// 三条断言都是"有牙齿"的：
    /// - **先挂支持、后挂反驳**，导出后反驳仍在前——若有人把顺序改成
    ///   "按挂载时间"，这条立刻变红。顺序不是排版偏好，是对抗确认偏误的
    ///   核心主张，产物里抹掉它比界面里抹掉更彻底（产物是给别人看的）；
    /// - **没有对应证据的档位不留空小节**（用「存疑」档验证）：空标题会让
    ///   读者以为数据丢了；
    /// - **没写判据时要有明确一句话**，留白等于默许一条无法被证伪的判断。
    ///
    /// 另注：`create()` 自己会写一条「建立论点」的变更记录，所以这里断言的是
    /// "变更小节只出现一次"而不是"不出现"——想测"无变更"，得绕过 create 直接写库。
    #[tokio::test]
    async fn compose_markdown_orders_oppose_first_and_skips_empty_sections() {
        let pool = test_pool().await;
        let (eid, aid) = seed(&pool).await;

        let id = create(
            &pool,
            NewThesis {
                statement: "2027 年储能出货会超预期".into(),
                entity_id: eid,
                confidence: Some(4),
            },
        )
        .await
        .expect("建论点失败");

        // 两条事件：一条将要挂成支持，一条挂成反驳
        let mut ev_ids = Vec::new();
        for fact in ["中标 2GWh 大单", "储能毛利率下滑 6 个点"] {
            let ev: i64 = sqlx::query_scalar(
                "INSERT INTO research_events (article_id, entity_id, event_type, fact, evidence) \
                 VALUES (?, ?, 'order', ?, '') RETURNING id",
            )
            .bind(aid)
            .bind(eid)
            .bind(fact)
            .fetch_one(&pool)
            .await
            .expect("插事件失败");
            ev_ids.push(ev);
        }

        // 刻意先挂支持、后挂反驳：顺序断言不能靠插入顺序蒙对
        add_evidence(
            &pool,
            NewEvidence {
                thesis_id: id,
                kind: "event".into(),
                ref_id: ev_ids[0],
                stance: "support".into(),
                note: Some("订单可见度提升".into()),
            },
        )
        .await
        .expect("挂支持失败");
        add_evidence(
            &pool,
            NewEvidence {
                thesis_id: id,
                kind: "event".into(),
                ref_id: ev_ids[1],
                stance: "oppose".into(),
                note: Some("价格战开始".into()),
            },
        )
        .await
        .expect("挂反驳失败");

        let md = compose_markdown(&pool, id).await.expect("组装失败");

        assert!(
            md.contains("2027 年储能出货会超预期"),
            "标题必须带判断原文：\n{md}"
        );
        let opp = md.find("## 反驳证据").expect("缺反驳小节");
        let sup = md.find("## 支持证据").expect("缺支持小节");
        assert!(opp < sup, "反驳必须排在支持之前，实际：\n{md}");
        assert!(md.contains("价格战开始"), "反驳的批注要进产物：\n{md}");
        assert!(
            !md.contains("## 存疑证据"),
            "没有存疑证据时不该留空小节：\n{md}"
        );
        assert_eq!(
            md.matches("## 变更记录").count(),
            1,
            "变更小节只能出现一次：\n{md}"
        );
        assert!(
            md.contains("还没有写判据"),
            "没写判据时必须明说，不能留白：\n{md}"
        );

        // 补上判据后再组装一次：窗口与结论要出现在产物里
        sqlx::query(
            "INSERT INTO thesis_falsifiers (thesis_id, statement, window_kind, due_at) \
             VALUES (?, '2026 年底交付未达 20GWh 即算错', 'date', '2026-12-31')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .expect("插判据失败");

        let md2 = compose_markdown(&pool, id).await.expect("二次组装失败");
        assert!(
            md2.contains("到期日 2026-12-31"),
            "检验窗口要带日期：\n{md2}"
        );
        assert!(md2.contains("待验证"), "判据结论要出现：\n{md2}");
        assert!(
            !md2.contains("还没有写判据"),
            "有判据后不该再出现空态提示：\n{md2}"
        );
    }
}
