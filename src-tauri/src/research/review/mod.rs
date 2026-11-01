//! 周复盘（阶段二）
//!
//! 本模块当前只承载 TASK-00 的一次性数据搬运：把「证据的来源身份」补进
//! `thesis_evidence_source`。周复盘本体的表与命令尚未开工，届时一并放进这里。
//!
//! # 为什么回填放在这里而不是 `thesis.rs`
//! `thesis.rs` 是论点的**日常**读写（新建、改状态、挂证据），一次性的数据搬运
//! 挂在那里会让"每次打开论点都在跑"的代码和"只在启动跑一次"的代码混在一起。
//!
//! # 为什么不在 `db::create_tables` 里跑
//! 建表函数只该建结构。一旦它带副作用，将来任何一次 schema 变更都可能顺带触发
//! 数据搬运，而建表是**每次启动**都跑的。

pub mod health;

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::research::thesis::{self, NewEvidence, ThesisPatch};

/// 回填证据来源身份（启动时跑一次，幂等）
///
/// # 为什么需要
/// 本表建出来之前挂的证据没有 source 行，而**源还在的每一小时都是回填窗口**——
/// 清理判据是 `published_at` 而非抓取时间，一篇 2019 年的旧文下次清理就没了，
/// 与"证据挂了多久"毫无关系。
///
/// # 行为
/// 只处理「还没有 source 行」的证据：
/// - 源仍在 → 写入 `article_id` + 来源名 + 发布日期；
/// - 源已失联 → **写一行空来源**（`article_id` 为 NULL、两个文本列为空串），而不是跳过。
///   跳过的代价是每次启动都重新扫一遍这些永远不会成功的行，且从此无法区分
///   「没跑过」（改代码能救）与「跑过但当时就丢了」（只能接受）。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回
/// 本次实际写入的行数。第二次启动起恒为 0。
///
/// # 错误
/// 查询或写入失败时返回错误信息
///
/// # 诊断
/// 三种状态的计数（三者之和恒等于 `thesis_evidence` 行数）：
/// ```sql
/// SELECT SUM(s.article_id IS NOT NULL)                          AS recorded,
///        SUM(s.evidence_id IS NOT NULL AND s.article_id IS NULL) AS lost,
///        SUM(s.evidence_id IS NULL)                             AS pending
/// FROM thesis_evidence v
/// LEFT JOIN thesis_evidence_source s ON s.evidence_id = v.id;
/// ```
pub async fn backfill_evidence_sources(pool: &SqlitePool) -> Result<u64, String> {
    // 一次查出全部待办。`pending` 为空时立即返回 —— 稳态下这是每次启动的主路径，
    // 不该为此再跑一轮全表 UPDATE。
    let pending = sqlx::query(
        "SELECT v.id AS evidence_id, v.kind AS kind, v.ref_id AS ref_id \
         FROM thesis_evidence v \
         LEFT JOIN thesis_evidence_source s ON s.evidence_id = v.id \
         WHERE s.evidence_id IS NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询待回填证据失败：{e}"))?;

    if pending.is_empty() {
        return Ok(0);
    }

    let mut written = 0u64;
    for row in pending {
        let evidence_id: i64 = row.get("evidence_id");
        let kind: String = row.get("kind");
        let ref_id: i64 = row.get("ref_id");

        // 按证据类型倒推文章 ID。`article` 类证据的 ref_id 就是文章自身；
        // 另两类要去来源表查 —— 源可能已随文章级联删除，查不到就是 None。
        let article_id = match kind.as_str() {
            "article" => Some(ref_id),
            "event" => sqlx::query_scalar::<_, i64>("SELECT article_id FROM research_events WHERE id = ?")
                .bind(ref_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("查询事件所属文章失败：{e}"))?,
            "highlight" => sqlx::query_scalar::<_, i64>("SELECT article_id FROM highlights WHERE id = ?")
                .bind(ref_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("查询高亮所属文章失败：{e}"))?,
            // 未来可能出现的外部摘录（kind='external'）等：没有对应的本地文章
            _ => None,
        };

        let (aid, name, published) = match article_id {
            Some(aid) => {
                let src = sqlx::query(
                    "SELECT COALESCE(NULLIF(f.name, ''), NULLIF(a.author, ''), '') AS source_name, \
                            COALESCE(a.published_at, '') AS published_at \
                     FROM articles a LEFT JOIN feeds f ON f.id = a.feed_id \
                     WHERE a.id = ?",
                )
                .bind(aid)
                .fetch_optional(pool)
                .await
                .map_err(|e| format!("查询证据来源失败：{e}"))?;
                match src {
                    Some(r) => (
                        Some(aid),
                        r.get::<String, _>("source_name"),
                        r.get::<String, _>("published_at"),
                    ),
                    // 来源实体还在、文章行却已没了。级联删不出这种组合，但脏数据会有
                    // ——记成"失联"（状态②），不留着下次再试。
                    None => (None, String::new(), String::new()),
                }
            }
            None => (None, String::new(), String::new()),
        };

        sqlx::query(
            "INSERT OR IGNORE INTO thesis_evidence_source \
             (evidence_id, article_id, source_name, published_at) VALUES (?, ?, ?, ?)",
        )
        .bind(evidence_id)
        .bind(aid)
        .bind(&name)
        .bind(&published)
        .execute(pool)
        .await
        .map_err(|e| format!("写入证据来源失败：{e}"))?;
        written += 1;
    }
    Ok(written)
}

// ─── 周复盘：入参（**不加 rename**，前端传 snake_case） ──────────────────────
//
// 本项目两套约定并存，不是笔误：出参一律 camelCase，入参结构体内层字段一律
// snake_case（前端手写 `thesis_id` / `window_kind`）。入参写错是硬失败
// （`invalid args for command`），出参 rename 漏了才是静默 undefined。

/// 保存判据的入参
#[derive(Debug, Clone, Deserialize)]
pub struct FalsifierInput {
    /// 论点 ID
    pub thesis_id: i64,
    /// ① 判据句（必填，≤400 字）
    pub statement: String,
    /// ② 检验窗口形态："date" | "cycle"
    pub window_kind: String,
    /// `window_kind = "date"` 时必填（YYYY-MM-DD）
    pub due_at: Option<String>,
    /// `window_kind = "cycle"` 时必填（> 0）
    pub every_n_days: Option<i64>,
    /// ③ 检验线索，0–3 个
    pub keywords: Option<Vec<String>>,
    /// 线索限定事件类型，空 = 不限
    pub event_type: Option<String>,
    /// 线索实体，空 = 继承论点实体
    pub entity_id: Option<i64>,
    /// 变更理由（写进 thesis_revisions.reason）
    pub reason: Option<String>,
}

/// 记一次复盘处置的入参
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewMarkInput {
    /// 会话 ID
    pub review_id: i64,
    /// 论点 ID
    pub thesis_id: i64,
    /// unchanged / confidence / status / falsifier / evidence
    pub action: String,
    /// `action = "confidence"` 时必填
    pub confidence: Option<i64>,
    /// `action = "status"` 时必填（归档也走这里，status = "archived"）
    pub status: Option<String>,
    /// 除 unchanged 外**必须非空**——理由是这个功能的全部产出
    pub reason: Option<String>,
    /// `action = "falsifier"` 时必填
    pub falsifier: Option<FalsifierInput>,
    /// `action = "evidence"` 时必填
    pub evidence: Option<NewEvidence>,
}

// ─── 周复盘：出参（全部 camelCase，每个都配了断言单测） ──────────────────────

/// 一条判据
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Falsifier {
    pub id: i64,
    pub thesis_id: i64,
    pub statement: String,
    pub window_kind: String,
    pub due_at: Option<String>,
    pub every_n_days: Option<i64>,
    pub keywords: Vec<String>,
    pub event_type: String,
    pub entity_id: Option<i64>,
    pub verdict: String,
    pub verdict_evidence_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// 复盘卡片（四区共用同一个结构，用所在数组区分归属）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCard {
    pub thesis_id: i64,
    pub statement: String,
    pub entity_id: i64,
    pub entity_name: String,
    pub status: String,
    pub confidence: i64,
    pub support_count: i64,
    pub oppose_count: i64,
    pub mixed_count: i64,
    /// 论点建立至今的天数
    pub age_days: i64,
    /// 最近一条证据距今天数；None = 从未挂证据
    pub last_evidence_days: Option<i64>,
    /// 上次复盘处置时间
    pub last_review_at: Option<String>,
    /// None → 进 C 区（待补判据）
    pub falsifier: Option<Falsifier>,
    /// Some 即到期，值是逾期天数（0 = 今天到期）
    pub overdue_days: Option<i64>,
    /// 命中的健康规则
    pub hits: Vec<health::HealthHit>,
    /// 仅 C 区非空：三条候选判据句式
    pub suggested_falsifiers: Vec<String>,
    /// P1-2 用；本轮恒为 None
    pub clue_hit_count: Option<i64>,
}

/// 一次进行中的复盘会话
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSession {
    pub id: i64,
    pub started_at: String,
    pub handled_count: i64,
}

/// 复盘结束摘要
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub id: i64,
    pub started_at: String,
    pub finished_at: String,
    pub summary: String,
    pub handled_count: i64,
    pub confidence_count: i64,
    pub status_count: i64,
    pub falsifier_count: i64,
    pub evidence_count: i64,
    pub unchanged_count: i64,
    pub archived_count: i64,
}

/// 四区总览
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOverview {
    pub session: Option<ReviewSession>,
    /// A 区：判据到期（按逾期天数倒序）
    pub due: Vec<ReviewCard>,
    /// B 区：结构有偏（按严重级 → 数值排序）
    pub health: Vec<ReviewCard>,
    /// C 区：待补判据（按 age_days 倒序）
    pub missing_falsifier: Vec<ReviewCard>,
    /// D 区：本期无变化
    pub idle_count: i64,
    /// 本次会话已处置数
    pub handled_count: i64,
    /// 三区全空
    pub quiet: bool,
    /// 安静态文案用（"下次检查 2026-11-08"）
    pub next_check_at: Option<String>,
    /// 实体抽取被关停、但仍有活跃论点的实体数（近似口径，顶部小字用）
    pub extraction_disabled_entities: i64,
    /// 全局上次复盘结束时间
    pub last_review_at: Option<String>,
}

// ─── 周复盘：判据读写 ─────────────────────────────────────────────────────────

/// 保存判据（一个论点至多一条，已有则覆盖）
///
/// # 校验
/// 判据句非空且 ≤400 字；`window_kind` ∈ date|cycle 且对应字段必填；
/// 检验线索 ≤3 个。**在写入前全部校验**，避免半截数据落库。
///
/// # 为什么不改 `verdict`
/// 用户编辑判据可能只是改错别字。把结论清回 pending 等于悄悄丢掉"已证实/已证伪"
/// 这个判断，而它比判据措辞重要得多。
///
/// # 参数
/// * `pool` - 连接池
/// * `input` - 判据内容
///
/// # 返回
/// 判据行 ID
///
/// # 错误
/// 校验失败、论点不存在、写入失败
pub async fn falsifier_save(pool: &SqlitePool, input: FalsifierInput) -> Result<i64, String> {
    let statement = input.statement.trim().to_string();
    if statement.is_empty() {
        return Err("判据不能为空：写清「什么情况下这条判断算错」".into());
    }
    if statement.chars().count() > 400 {
        return Err("判据不能超过 400 字".into());
    }
    match input.window_kind.as_str() {
        "date" => {
            if input.due_at.as_deref().unwrap_or("").trim().is_empty() {
                return Err("按日期检验需要给出到期日".into());
            }
        }
        "cycle" => match input.every_n_days {
            Some(n) if n > 0 => {}
            _ => return Err("按周期检验需要给出正整数天数".into()),
        },
        _ => return Err("检验窗口只能是 date 或 cycle".into()),
    }
    let keywords = input.keywords.unwrap_or_default();
    if keywords.len() > 3 {
        return Err("检验线索最多 3 个".into());
    }
    let kw_json = serde_json::to_string(&keywords).map_err(|e| format!("序列化检验线索失败：{e}"))?;

    // 论点必须存在；实体默认继承论点
    let thesis = thesis::get_thesis(pool, input.thesis_id).await?;
    let entity_id = input.entity_id.or(Some(thesis.entity_id));

    let old: Option<String> =
        sqlx::query_scalar("SELECT statement FROM thesis_falsifiers WHERE thesis_id = ?")
            .bind(input.thesis_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("读取原判据失败：{e}"))?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO thesis_falsifiers \
         (thesis_id, statement, window_kind, due_at, every_n_days, keywords, event_type, entity_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(thesis_id) DO UPDATE SET \
           statement = excluded.statement, window_kind = excluded.window_kind, \
           due_at = excluded.due_at, every_n_days = excluded.every_n_days, \
           keywords = excluded.keywords, event_type = excluded.event_type, \
           entity_id = excluded.entity_id, updated_at = CURRENT_TIMESTAMP \
         RETURNING id",
    )
    .bind(input.thesis_id)
    .bind(&statement)
    .bind(&input.window_kind)
    .bind(input.due_at.as_deref())
    .bind(input.every_n_days)
    .bind(&kw_json)
    .bind(input.event_type.as_deref().unwrap_or(""))
    .bind(entity_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("保存判据失败：{e}"))?;

    record_falsifier_revision(
        pool,
        input.thesis_id,
        &old.unwrap_or_default(),
        &statement,
        input.reason.as_deref().unwrap_or(""),
    )
    .await?;
    Ok(id)
}

/// 清空判据
///
/// 幂等：本来就没有判据时直接返回（不记一条空变更）。
///
/// # 参数
/// * `pool` - 连接池
/// * `thesis_id` - 论点 ID
/// * `reason` - 变更理由
///
/// # 错误
/// 查询或删除失败
pub async fn falsifier_clear(
    pool: &SqlitePool,
    thesis_id: i64,
    reason: Option<String>,
) -> Result<(), String> {
    let old: Option<String> =
        sqlx::query_scalar("SELECT statement FROM thesis_falsifiers WHERE thesis_id = ?")
            .bind(thesis_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("读取原判据失败：{e}"))?;
    let Some(old) = old else { return Ok(()) };

    sqlx::query("DELETE FROM thesis_falsifiers WHERE thesis_id = ?")
        .bind(thesis_id)
        .execute(pool)
        .await
        .map_err(|e| format!("清空判据失败：{e}"))?;
    record_falsifier_revision(pool, thesis_id, &old, "", reason.as_deref().unwrap_or(""))
        .await
}

/// 写一条判据变更记录
///
/// `thesis::record_revision` 是模块私有的，这里直接插一条同一形状的行。
/// 判据变了就是论点变了，必须留痕——否则复盘里"补判据"这个动作在变更史上不可见。
async fn record_falsifier_revision(
    pool: &SqlitePool,
    thesis_id: i64,
    from: &str,
    to: &str,
    reason: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO thesis_revisions (thesis_id, field, from_value, to_value, reason) \
         VALUES (?, 'falsifier', ?, ?, ?)",
    )
    .bind(thesis_id)
    .bind(from)
    .bind(to)
    .bind(reason)
    .execute(pool)
    .await
    .map_err(|e| format!("记录判据变更失败：{e}"))?;
    Ok(())
}

// ─── 周复盘：到期判定 ─────────────────────────────────────────────────────────

/// 判定一条判据是否到期，返回逾期天数
///
/// # 参数
/// * `window_kind` - "date" / "cycle"
/// * `date_overdue_days` - date 窗：now − due_at（负数 = 还没到）
/// * `every_n_days` - cycle 窗：周期天数
/// * `cycle_elapsed_days` - cycle 窗：now − (上次复盘 ?? 判据创建)
/// * `verdict` - 判据结论；非 pending 的不再 due（已经出结论了）
///
/// # 返回
/// `Some(n)` = 到期且逾期 n 天（`0` = 今天到期）；`None` = 不到期
pub fn due_overdue_days(
    window_kind: &str,
    date_overdue_days: Option<i64>,
    every_n_days: Option<i64>,
    cycle_elapsed_days: Option<i64>,
    verdict: &str,
) -> Option<i64> {
    if verdict != "pending" {
        return None;
    }
    match window_kind {
        "date" => date_overdue_days.filter(|d| *d >= 0),
        "cycle" => {
            let every = every_n_days?;
            let elapsed = cycle_elapsed_days?;
            // 已经过去了整个周期 ⇒ 到点（overdue = 0 表示今天该复查）
            let overdue = elapsed - every;
            if overdue >= 0 {
                Some(overdue)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ─── 周复盘：会话 ─────────────────────────────────────────────────────────────

/// 开始（或继续）一次复盘会话
///
/// 幂等：已有进行中的会话就返回它。**打开页面就该调它**，先于 `overview`——
/// 否则 `overview` 拿不到会话 ID，"本次已处置"恒为 0，卡片点了不会离开三区。
///
/// # 错误
/// 查询或写入失败
pub async fn start_session(pool: &SqlitePool) -> Result<ReviewSession, String> {
    if let Some(s) = open_session(pool).await? {
        return Ok(s);
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO thesis_reviews DEFAULT VALUES RETURNING id")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("开始复盘失败：{e}"))?;
    let started_at: String = sqlx::query_scalar("SELECT started_at FROM thesis_reviews WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("读取复盘时间失败：{e}"))?;
    Ok(ReviewSession {
        id,
        started_at,
        handled_count: 0,
    })
}

/// 取当前进行中的会话（至多一行）
async fn open_session(pool: &SqlitePool) -> Result<Option<ReviewSession>, String> {
    let row = sqlx::query(
        "SELECT r.id, r.started_at, \
                (SELECT COUNT(*) FROM thesis_review_items i WHERE i.review_id = r.id) AS handled_count \
         FROM thesis_reviews r WHERE r.finished_at IS NULL ORDER BY r.id DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询进行中的复盘失败：{e}"))?;
    Ok(row.map(|r| ReviewSession {
        id: r.get("id"),
        started_at: r.get("started_at"),
        handled_count: r.get("handled_count"),
    }))
}

// ─── 周复盘：四区装配与处置 ───────────────────────────────────────────────────

/// 四区取数：一次查询喂满 A/B/C/D
///
/// # 为什么天数都在 SQL 里算
/// 库里 `DEFAULT CURRENT_TIMESTAMP` 是 UTC，`julianday('now') - julianday(x)` 与之同源，
/// 不会在午夜前后差一天。Rust 侧因此不需要解析任何时间字符串，判定变成纯整数比较。
///
/// # `source_count` 那两个约束别"简化"
/// ① 命名空间 `'a:'` / `'e:'` 前缀：`COUNT(DISTINCT)` 会跳过 NULL 行，不加前缀
///    会让"证据都在、来源行还没回填"的论点算成 `src = 0`，进而误报 H3
///    （"全部来自同一篇文章"）。
/// ② 兜底 `'e:' || v.id`：服务"回填没跑到"的脏数据，让每条证据至少算一个独立来源。
///    **宁可漏报 H3，不可误报**——漏报只是少提醒一次，误报会让用户不信任这个提醒。
const REVIEW_SQL: &str = r#"
SELECT t.id AS thesis_id, t.statement, t.entity_id, t.status, t.confidence,
       COALESCE(e.name, '') AS entity_name,
       (SELECT COUNT(*) FROM thesis_evidence v WHERE v.thesis_id=t.id AND v.stance='support') AS support_count,
       (SELECT COUNT(*) FROM thesis_evidence v WHERE v.thesis_id=t.id AND v.stance='oppose')  AS oppose_count,
       (SELECT COUNT(*) FROM thesis_evidence v WHERE v.thesis_id=t.id AND v.stance='mixed')   AS mixed_count,
       (SELECT COUNT(DISTINCT COALESCE(
                  'a:' || CAST(src.article_id AS TEXT),
                  'e:' || CAST(v.id AS TEXT)))
          FROM thesis_evidence v
          LEFT JOIN thesis_evidence_source src ON src.evidence_id = v.id
         WHERE v.thesis_id = t.id) AS source_count,
       (SELECT CAST(julianday('now') - julianday(MAX(v.created_at)) AS INTEGER)
          FROM thesis_evidence v WHERE v.thesis_id=t.id) AS last_evidence_days,
       CAST(julianday('now') - julianday(t.created_at) AS INTEGER) AS age_days,
       (SELECT COUNT(*) FROM research_events re
         WHERE re.entity_id = t.entity_id
           AND julianday('now') - julianday(re.created_at) <= 30) AS event_count_30d,
       (SELECT re.event_type FROM research_events re
         WHERE re.entity_id = t.entity_id
           AND julianday('now') - julianday(re.created_at) <= 90
         GROUP BY re.event_type
         ORDER BY COUNT(*) DESC, re.event_type ASC
         LIMIT 1) AS top_event_type,
       (SELECT COUNT(*) FROM thesis_revisions r WHERE r.thesis_id=t.id) AS revision_count,
       (SELECT MAX(ri.created_at) FROM thesis_review_items ri WHERE ri.thesis_id=t.id) AS last_review_at,
       f.id AS f_id, f.statement AS f_statement, f.window_kind AS f_window_kind,
       f.due_at AS f_due_at, f.every_n_days AS f_every_n_days, f.keywords AS f_keywords,
       f.event_type AS f_event_type, f.entity_id AS f_entity_id,
       f.verdict AS f_verdict, f.verdict_evidence_id AS f_verdict_evidence_id,
       f.created_at AS f_created_at, f.updated_at AS f_updated_at,
       CASE WHEN f.window_kind='date'
            THEN CAST(julianday('now') - julianday(f.due_at) AS INTEGER) END AS date_overdue_days,
       CASE WHEN f.window_kind='cycle'
            THEN CAST(julianday('now') - julianday(COALESCE(
                   (SELECT MAX(ri.created_at) FROM thesis_review_items ri WHERE ri.thesis_id=t.id),
                   f.created_at)) AS INTEGER) END AS cycle_elapsed_days,
       (SELECT COUNT(*) FROM thesis_review_items ri
         WHERE ri.review_id = ?1 AND ri.thesis_id = t.id) AS handled_in_session
FROM theses t
LEFT JOIN entities e ON e.id = t.entity_id
LEFT JOIN thesis_falsifiers f ON f.thesis_id = t.id
WHERE t.status <> 'archived'
"#;

/// 四区装配
///
/// # 签名为什么没有 `review_id`
/// 会话 ID 是后端的内部状态，让前端传等于把"当前是哪次复盘"变成前端责任；
/// 一旦传错（比如传了上次已结束的会话），"本次已处置"集合会完全错位。
/// 本函数内部自查进行中的会话。
///
/// # 调用契约（**必须先 start，再 overview**）
/// 本函数只读、**不建会话**。查不到进行中会话时 `session = None`、`handled` 恒 0，
/// 表现是"卡片处置后不离开三区"——这是整个设计唯一的静默失效点。
/// 所以前端加载顺序必须是 `await startSession(); await overview()`。
///
/// # 错误
/// 查询失败
pub async fn overview(pool: &SqlitePool) -> Result<ReviewOverview, String> {
    let session = open_session(pool).await?;
    let session_id = session.as_ref().map(|s| s.id).unwrap_or(0);

    let rows = sqlx::query(REVIEW_SQL)
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("读取复盘数据失败：{e}"))?;

    let mut due: Vec<ReviewCard> = Vec::new();
    let mut health_cards: Vec<ReviewCard> = Vec::new();
    let mut missing: Vec<ReviewCard> = Vec::new();
    let mut idle_count = 0i64;
    let mut handled_count = 0i64;

    for row in rows {
        let handled_in_session: i64 = row.get("handled_in_session");
        if handled_in_session > 0 {
            // 本次已处置的不再进 A/B/C（否则用户处置完，卡片还在原地）
            handled_count += 1;
            continue;
        }

        let thesis_id: i64 = row.get("thesis_id");
        let statement: String = row.get("statement");
        let entity_id: i64 = row.get("entity_id");
        let status: String = row.get("status");
        let confidence: i64 = row.get("confidence");
        let entity_name: String = row.get("entity_name");
        let support_count: i64 = row.get("support_count");
        let oppose_count: i64 = row.get("oppose_count");
        let mixed_count: i64 = row.get("mixed_count");
        let source_count: i64 = row.get("source_count");
        let last_evidence_days: Option<i64> = row.get("last_evidence_days");
        let age_days: i64 = row.get::<Option<i64>, _>("age_days").unwrap_or(0);
        let event_count_30d: i64 = row.get("event_count_30d");
        let top_event_type: Option<String> = row.get("top_event_type");
        let revision_count: i64 = row.get("revision_count");
        let last_review_at: Option<String> = row.get("last_review_at");
        let date_overdue_days: Option<i64> = row.get("date_overdue_days");
        let cycle_elapsed_days: Option<i64> = row.get("cycle_elapsed_days");

        let falsifier = match row.get::<Option<i64>, _>("f_id") {
            Some(fid) => Some(Falsifier {
                id: fid,
                thesis_id,
                statement: row.get::<Option<String>, _>("f_statement").unwrap_or_default(),
                window_kind: row.get::<Option<String>, _>("f_window_kind").unwrap_or_default(),
                due_at: row.get("f_due_at"),
                every_n_days: row.get("f_every_n_days"),
                keywords: serde_json::from_str(&row.get::<Option<String>, _>("f_keywords").unwrap_or_default())
                    .unwrap_or_default(),
                event_type: row.get::<Option<String>, _>("f_event_type").unwrap_or_default(),
                entity_id: row.get("f_entity_id"),
                verdict: row.get::<Option<String>, _>("f_verdict").unwrap_or_else(|| "pending".into()),
                verdict_evidence_id: row.get("f_verdict_evidence_id"),
                created_at: row.get::<Option<String>, _>("f_created_at").unwrap_or_default(),
                updated_at: row.get::<Option<String>, _>("f_updated_at").unwrap_or_default(),
            }),
            None => None,
        };

        let overdue_days = falsifier.as_ref().and_then(|f| {
            due_overdue_days(
                &f.window_kind,
                date_overdue_days,
                f.every_n_days,
                cycle_elapsed_days,
                &f.verdict,
            )
        });

        let metrics = health::HealthMetrics {
            status: status.clone(),
            confidence,
            support_count,
            oppose_count,
            mixed_count,
            source_count,
            age_days,
            last_evidence_days,
            event_count_30d,
            revision_count,
        };
        let hits = health::evaluate(&metrics);

        // 三区互斥，优先级 A > B > C：一张卡只出现在一个区，
        // 否则用户会在"该复查"和"结构有偏"里看到同一条判断，以为有两件事要做
        let suggested = if overdue_days.is_none() && hits.is_empty() && falsifier.is_none() {
            suggest_falsifiers(&statement, &entity_name, top_event_type.as_deref())
        } else {
            Vec::new()
        };

        let card = ReviewCard {
            thesis_id,
            statement,
            entity_id,
            entity_name,
            status,
            confidence,
            support_count,
            oppose_count,
            mixed_count,
            age_days,
            last_evidence_days,
            last_review_at,
            falsifier,
            overdue_days,
            hits,
            suggested_falsifiers: suggested,
            clue_hit_count: None,
        };

        if card.overdue_days.is_some() {
            due.push(card);
        } else if !card.hits.is_empty() {
            health_cards.push(card);
        } else if card.falsifier.is_none() {
            missing.push(card);
        } else {
            idle_count += 1;
        }
    }

    // A 区：逾期越久越前面
    due.sort_by(|a, b| b.overdue_days.unwrap_or(0).cmp(&a.overdue_days.unwrap_or(0)));
    // B 区：严重级优先；同级时命中条数多的优先（问题更集中）
    health_cards.sort_by(|a, b| {
        let ra = a.hits.iter().map(|h| health::severity_rank(&h.severity)).min().unwrap_or(9);
        let rb = b.hits.iter().map(|h| health::severity_rank(&h.severity)).min().unwrap_or(9);
        ra.cmp(&rb)
            .then(b.hits.len().cmp(&a.hits.len()))
            .then(b.age_days.cmp(&a.age_days))
    });
    // C 区：越老越前面（老的判断更可能已经过时）
    missing.sort_by(|a, b| b.age_days.cmp(&a.age_days));

    let next_check_at: Option<String> = sqlx::query_scalar("SELECT date('now', '+7 days')")
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("计算下次检查日失败：{e}"))?;
    let last_review_at: Option<String> =
        sqlx::query_scalar("SELECT MAX(finished_at) FROM thesis_reviews WHERE finished_at IS NOT NULL")
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("读取上次复盘时间失败：{e}"))?;
    // 近似口径：实体被停用、但仍有活跃论点的数量。只用于顶部可折叠小字，
    // 精确口径要解析每个自动化的实体过滤条件，代价远大于这句话的价值。
    let extraction_disabled_entities: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT t.entity_id) FROM theses t \
         JOIN entities e ON e.id = t.entity_id \
         WHERE e.enabled = 0 AND t.status <> 'archived'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("统计停用实体失败：{e}"))?;

    let quiet = due.is_empty() && health_cards.is_empty() && missing.is_empty();
    Ok(ReviewOverview {
        session,
        due,
        health: health_cards,
        missing_falsifier: missing,
        idle_count,
        handled_count,
        quiet,
        next_check_at,
        extraction_disabled_entities,
        last_review_at,
    })
}

/// 生成三条候选判据句式（C 区用）
///
/// 只给**句式**，不给结论：把"什么情况下算错"替用户想好，等于替他做判断
/// （PRD §4.1 的边界）。这里做的只是把三个必填槽位摆出来，让他少写点字。
///
/// 取不到事件类型时用"相关事件"兜底 —— 不编一个具体类型，那才是替人猜。
fn suggest_falsifiers(statement: &str, entity_name: &str, top_event_type: Option<&str>) -> Vec<String> {
    let event = top_event_type.unwrap_or("相关事件");
    let who = if entity_name.is_empty() { "该标的" } else { entity_name };
    let short = if statement.chars().count() > 20 {
        format!("{}…", statement.chars().take(20).collect::<String>())
    } else {
        statement.to_string()
    };
    vec![
        format!("如果 {who} 接下来 90 天没有出现{event}，说明「{short}」的前提站不住"),
        format!("如果下一次{event}的数据低于这一次，「{short}」就要下调"),
        format!("如果同业同期也出现同样的{event}，那这条判断就不是 {who} 独有的"),
    ]
}

/// 记录一次复盘处置
///
/// 一张卡片的动作在这里落地，同时写一行 `thesis_review_items` —— 那是"看了没改"
/// 的唯一载体（`thesis::update` 在无字段变更时直接 return，什么都不写）。
///
/// # 动作
/// - `unchanged`：只看不改。理由可空。
/// - `confidence`：改置信，需 `confidence` + `reason`。
/// - `status`：改状态（归档也是它，`status = "archived"`），需 `status` + `reason`。
/// - `falsifier`：补判据，需 `falsifier`。
/// - `evidence`：挂证据，需 `evidence`。
///
/// # 错误
/// 动作非法、会话不存在或已结束、必填项缺失、下游写入失败
pub async fn mark(pool: &SqlitePool, input: ReviewMarkInput) -> Result<(), String> {
    const ACTIONS: [&str; 5] = ["unchanged", "confidence", "status", "falsifier", "evidence"];
    if !ACTIONS.contains(&input.action.as_str()) {
        return Err(format!("未知的复盘动作：{}", input.action));
    }

    // 会话必须存在且仍在进行中
    let finished: Option<Option<String>> =
        sqlx::query_scalar("SELECT finished_at FROM thesis_reviews WHERE id = ?")
            .bind(input.review_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("查询复盘会话失败：{e}"))?;
    match finished {
        None => return Err("这次复盘不存在".into()),
        Some(Some(_)) => return Err("这次复盘已经结束了".into()),
        Some(None) => {}
    }

    let reason = input.reason.clone().unwrap_or_default().trim().to_string();
    if input.action != "unchanged" && reason.is_empty() {
        return Err("写一句理由吧——半年后它比结论本身更有用".into());
    }

    let mut from_value = String::new();
    let mut to_value = String::new();

    match input.action.as_str() {
        "unchanged" => {
            to_value = "无变化".to_string();
        }
        "confidence" => {
            let c = input.confidence.ok_or("改置信需要给出新值")?;
            let old = thesis::get_thesis(pool, input.thesis_id).await?.confidence;
            from_value = old.to_string();
            to_value = c.to_string();
            thesis::update(
                pool,
                input.thesis_id,
                ThesisPatch {
                    statement: None,
                    status: None,
                    confidence: Some(c),
                    reason: Some(reason.clone()),
                },
            )
            .await?;
        }
        "status" => {
            let s = input.status.clone().ok_or("改状态需要给出新状态")?;
            let old = thesis::get_thesis(pool, input.thesis_id).await?.status;
            from_value = old;
            to_value = s.clone();
            thesis::update(
                pool,
                input.thesis_id,
                ThesisPatch {
                    statement: None,
                    status: Some(s),
                    confidence: None,
                    reason: Some(reason.clone()),
                },
            )
            .await?;
        }
        "falsifier" => {
            let mut f = input.falsifier.clone().ok_or("补判据需要给出判据内容")?;
            // 前端可能只传 thesis_id 而把判据挂在别的论点上，这里强制对齐
            f.thesis_id = input.thesis_id;
            falsifier_save(pool, f).await?;
            to_value = "已补判据".to_string();
        }
        "evidence" => {
            let mut ev = input.evidence.clone().ok_or("挂证据需要给出证据内容")?;
            ev.thesis_id = input.thesis_id;
            thesis::add_evidence(pool, ev).await?;
            to_value = "已挂证据".to_string();
        }
        _ => unreachable!("动作已在上面校验过"),
    }

    // 一次会话里一条论点只留**最终**处置：后一次动作覆盖前一次的 action，
    // 但证据本身仍在 thesis_evidence / thesis_revisions 里可查，不丢信息
    sqlx::query(
        "INSERT INTO thesis_review_items (review_id, thesis_id, action, reason, from_value, to_value) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(review_id, thesis_id) DO UPDATE SET \
           action = excluded.action, reason = excluded.reason, \
           from_value = excluded.from_value, to_value = excluded.to_value, \
           created_at = CURRENT_TIMESTAMP",
    )
    .bind(input.review_id)
    .bind(input.thesis_id)
    .bind(&input.action)
    .bind(&reason)
    .bind(&from_value)
    .bind(&to_value)
    .execute(pool)
    .await
    .map_err(|e| format!("记录复盘处置失败：{e}"))?;
    Ok(())
}

/// 结束一次复盘，生成摘要
///
/// 摘要文案在 Rust 拼：前端只渲染一句话，不参与措辞。
/// 已结束的会话重复调用直接返回既有摘要（幂等，防误点两次）。
///
/// # 错误
/// 会话不存在、写入失败
pub async fn finish(pool: &SqlitePool, review_id: i64) -> Result<ReviewSummary, String> {
    let row = sqlx::query("SELECT started_at, finished_at, summary FROM thesis_reviews WHERE id = ?")
        .bind(review_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("查询复盘会话失败：{e}"))?
        .ok_or_else(|| "这次复盘不存在".to_string())?;
    let started_at: String = row.get("started_at");
    let already_finished: Option<String> = row.get("finished_at");

    if already_finished.is_none() {
        sqlx::query(
            "UPDATE thesis_reviews SET finished_at = CURRENT_TIMESTAMP WHERE id = ? AND finished_at IS NULL",
        )
        .bind(review_id)
        .execute(pool)
        .await
        .map_err(|e| format!("结束复盘失败：{e}"))?;
    }

    // 计数全部由 items 聚合，不存冗余列（少存一份就少一处不一致）
    let counts = sqlx::query(
        "SELECT COUNT(*) AS handled, \
                SUM(action = 'confidence') AS confidence, \
                SUM(action = 'status') AS status_n, \
                SUM(action = 'falsifier') AS falsifier, \
                SUM(action = 'evidence') AS evidence, \
                SUM(action = 'unchanged') AS unchanged, \
                SUM(action = 'status' AND to_value = 'archived') AS archived \
         FROM thesis_review_items WHERE review_id = ?",
    )
    .bind(review_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("统计复盘处置失败：{e}"))?;

    let handled_count: i64 = counts.get("handled");
    let confidence_count: i64 = counts.get::<Option<i64>, _>("confidence").unwrap_or(0);
    let status_count: i64 = counts.get::<Option<i64>, _>("status_n").unwrap_or(0);
    let falsifier_count: i64 = counts.get::<Option<i64>, _>("falsifier").unwrap_or(0);
    let evidence_count: i64 = counts.get::<Option<i64>, _>("evidence").unwrap_or(0);
    let unchanged_count: i64 = counts.get::<Option<i64>, _>("unchanged").unwrap_or(0);
    let archived_count: i64 = counts.get::<Option<i64>, _>("archived").unwrap_or(0);

    let summary = build_summary(
        handled_count,
        confidence_count,
        status_count,
        falsifier_count,
        evidence_count,
        unchanged_count,
        archived_count,
    );

    // 摘要只在第一次结束时写。重复调用读回既有值，不覆盖用户看到过的那句话
    let summary: String = match already_finished {
        Some(_) => row.get::<Option<String>, _>("summary").unwrap_or_default(),
        None => {
            sqlx::query("UPDATE thesis_reviews SET summary = ? WHERE id = ?")
                .bind(&summary)
                .bind(review_id)
                .execute(pool)
                .await
                .map_err(|e| format!("写入复盘摘要失败：{e}"))?;
            summary
        }
    };

    let finished_at: String = sqlx::query_scalar("SELECT finished_at FROM thesis_reviews WHERE id = ?")
        .bind(review_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("读取复盘结束时间失败：{e}"))?;

    Ok(ReviewSummary {
        id: review_id,
        started_at,
        finished_at,
        summary,
        handled_count,
        confidence_count,
        status_count,
        falsifier_count,
        evidence_count,
        unchanged_count,
        archived_count,
    })
}

/// 拼一句摘要
///
/// 只写"做了什么"，不写"应该做什么"——后者是建议，不是复盘记录。
/// 零处置时给一句明确的无事发生，而不是空串（空串在界面上像加载失败）。
fn build_summary(
    handled: i64,
    confidence: i64,
    status: i64,
    falsifier: i64,
    evidence: i64,
    unchanged: i64,
    archived: i64,
) -> String {
    if handled == 0 {
        return "本期没有处理任何判断。".to_string();
    }
    let mut parts = vec![format!("本期处理 {handled} 条")];
    if confidence > 0 {
        parts.push(format!("改置信 {confidence} 条"));
    }
    if status > 0 {
        parts.push(format!("改状态 {status} 条"));
    }
    if archived > 0 {
        parts.push(format!("归档 {archived} 条"));
    }
    if falsifier > 0 {
        parts.push(format!("补判据 {falsifier} 条"));
    }
    if evidence > 0 {
        parts.push(format!("挂证据 {evidence} 条"));
    }
    if unchanged > 0 {
        parts.push(format!("{unchanged} 条看过没改"));
    }
    format!("{}。", parts.join("，"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 建一个**真实 schema** 的内存库
    ///
    /// 刻意不手抄表结构：手抄会在 schema 演进时静默漂移，而回填逻辑横跨
    /// `thesis_evidence` / `thesis_evidence_source` / `articles` / `feeds` 四张表，
    /// 漂移的代价是"测试全绿但线上查错列"。
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("建库失败");
        crate::db::create_tables(&pool).await.expect("建表失败");
        pool
    }

    /// 造 1 个订阅源 + 1 个实体 + 1 个论点 + n 篇文章
    ///
    /// @param feed_name - 订阅源名（同时用作 url 的一部分，避免唯一约束冲突）
    /// @param n - 文章数量（每篇文章 guid 不同）
    /// @returns `(thesis_id, article_ids)`
    async fn seed(pool: &SqlitePool, feed_name: &str, n: usize) -> (i64, Vec<i64>) {
        let feed_id: i64 = sqlx::query_scalar("INSERT INTO feeds (name, url) VALUES (?, ?) RETURNING id")
            .bind(feed_name)
            .bind(format!("https://example.com/{feed_name}"))
            .fetch_one(pool)
            .await
            .expect("插订阅源失败");
        let mut aids = Vec::new();
        for i in 0..n {
            let aid: i64 = sqlx::query_scalar(
                "INSERT INTO articles (feed_id, guid, link, title, author, published_at) \
                 VALUES (?, ?, ?, '储能出货超预期', '张三', '2026-10-28') RETURNING id",
            )
            .bind(feed_id)
            .bind(format!("guid-{feed_name}-{i}"))
            .bind(format!("https://example.com/{feed_name}/{i}"))
            .fetch_one(pool)
            .await
            .expect("插文章失败");
            aids.push(aid);
        }
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO entities (name, entity_type) VALUES ('宁德时代', 'company') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .expect("插实体失败");
        let tid: i64 = sqlx::query_scalar("INSERT INTO theses (statement, entity_id) VALUES ('判断', ?) RETURNING id")
            .bind(eid)
            .fetch_one(pool)
            .await
            .expect("插论点失败");
        (tid, aids)
    }

    /// 挂一条 article 类证据（绕开 thesis::add_evidence，模拟"本表建之前挂的老证据"）
    async fn attach_legacy(pool: &SqlitePool, thesis_id: i64, article_id: i64) {
        sqlx::query(
            "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance) \
             VALUES (?, 'article', ?, 'support')",
        )
        .bind(thesis_id)
        .bind(article_id)
        .execute(pool)
        .await
        .expect("插老证据失败");
    }

    #[tokio::test]
    async fn backfill_is_idempotent() {
        let pool = test_pool().await;
        let (tid, aids) = seed(&pool, "财新", 3).await;
        for aid in &aids {
            attach_legacy(&pool, tid, *aid).await;
        }

        let first = backfill_evidence_sources(&pool).await.expect("首轮回填失败");
        assert_eq!(first, 3, "三条老证据都该被回填");

        let second = backfill_evidence_sources(&pool).await.expect("二轮回填失败");
        assert_eq!(second, 0, "第二次启动不该再有可回填的行");

        let recorded: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thesis_evidence_source WHERE article_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("统计失败");
        assert_eq!(recorded, 3, "三行都该带着 article_id");
    }

    #[tokio::test]
    async fn backfill_distinguishes_unbackfilled_from_lost() {
        let pool = test_pool().await;
        let (tid, aids) = seed(&pool, "财新", 2).await;
        for aid in &aids {
            attach_legacy(&pool, tid, *aid).await;
        }
        // 第二篇文章被清理（`thesis_evidence.ref_id` 故意不建外键，证据行留着）
        sqlx::query("DELETE FROM articles WHERE id = ?")
            .bind(aids[1])
            .execute(&pool)
            .await
            .expect("删文章失败");

        let written = backfill_evidence_sources(&pool).await.expect("回填失败");
        assert_eq!(written, 2, "源失联的那条也要写行，否则会每次启动都重扫");

        let recorded: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thesis_evidence_source WHERE article_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("统计①失败");
        let lost: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM thesis_evidence_source WHERE article_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .expect("统计②失败");
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_evidence")
            .fetch_one(&pool)
            .await
            .expect("统计证据失败");

        assert_eq!(recorded, 1, "源还在的那条应是状态①");
        assert_eq!(lost, 1, "源已删的那条应是状态②");
        assert_eq!(recorded + lost, total, "三种状态之和必须等于证据总数");

        let again = backfill_evidence_sources(&pool).await.expect("二次回填失败");
        assert_eq!(again, 0, "状态②不该被反复重试");
    }
}
