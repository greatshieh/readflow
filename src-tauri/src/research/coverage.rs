//! 覆盖范围与组装（阶段三）
//!
//! # 职责
//! 两件事，一件是纪律、一件是产出：
//! - **覆盖清单**（[`list`] / [`upsert`] / [`remove`]）：我盯哪些标的、为什么盯、
//!   什么条件才不盯了，以及最近一次被复盘是什么时候；
//! - **生成一页**（[`dossier`]）：把一个标的的全部判断、判据、正反证据、
//!   变更记录拼成一页带引用的 markdown，供复制或导出。
//!
//! # 为什么这两件事放在一个模块
//! 它们是同一条链的上下游：覆盖清单回答"写谁"，组装回答"写成什么"。
//! 拆成两个模块会各自持有一半的取数口径（谁的证据算数、哪些论点还在跟踪），
//! 两处一旦漂移，用户就会看到"覆盖表说 5 条判断、导出的那页只有 3 条"。
//!
//! # 边界（写在最前，防止后来扩张）
//! - **不做富文本 / 模板库 / 封面 / 图表编号**：排版交给 Obsidian 或 Word，
//!   这里只出骨架（ROADMAP §6）；
//! - **不做自动选题清单 / 热点推荐**：给所有人同一份清单等于消灭差异化；
//! - **不做产业链位置自动推断**：`position_note` 是用户手写的一句话。
//!   推断错了会被当真，而"它在产业链哪一环"本身就是结论的一部分。
//!
//! # 退出条件为什么不与判据表合并
//! 判据（`thesis_falsifiers`）挂在**判断**上，回答"什么情况下这条判断不成立"；
//! 退出条件挂在**标的**上，回答"什么条件下我不再盯它"。两者层级不同，
//! 但形式一样（一句话 + 窗口），所以 ROADMAP 提醒过"将来必然合并"。
//! 本轮的处理是：覆盖表存自由文本，但**界面把该标的已有的判据列在同一屏**，
//! 让"退出条件往往就是判据里的一条"这件事可见。等真有第二个使用场景
//! 再合并表，而不是现在先猜一个结构出来。
//!
//! # 时间口径
//! 所有"多少天前"都在 SQL 里用 `julianday('now') - julianday(x)` 算成整数，
//! 与 [`crate::research::review`] 保持一致：`CURRENT_TIMESTAMP` 与
//! `julianday('now')` 同为 UTC，不会因为在午夜前后差一天。

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// 覆盖档位（三档，与 ROADMAP §1.3 一致）
///
/// 用模块级常量而不是枚举：这一列是 TEXT，将来加档位只需改这里，
/// 不需要重建表（`UNIQUE`/`CHECK` 才会卡住结构演进）。
pub const TIER_CORE: &str = "core";
pub const TIER_WATCH: &str = "watch";
pub const TIER_EXIT: &str = "exit";

/// 档位合法性检查
fn valid_tier(tier: &str) -> bool {
    matches!(tier, TIER_CORE | TIER_WATCH | TIER_EXIT)
}

/// 覆盖条目的写入入参
///
/// **内层不加 `rename_all`**：前端手写 `snake_case`（与 `FalsifierInput`、
/// `ReviewMarkInput` 同一约定）。写错会硬失败 `invalid args for command`，
/// 这是好事——比静默写成空串强。
#[derive(Debug, Clone, Deserialize)]
pub struct CoverageInput {
    /// 标的（实体）ID
    pub entity_id: i64,
    /// 档位：core / watch / exit
    pub tier: String,
    /// 为什么盯（一句话）
    #[serde(default)]
    pub reason: String,
    /// 什么条件下不再盯
    #[serde(default)]
    pub exit_condition: String,
    /// 它在产业链上的位置（手写一句话，不自动推断）
    #[serde(default)]
    pub position_note: String,
}

/// 覆盖清单的一行（出参，camelCase）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageRow {
    /// 标的 ID
    pub entity_id: i64,
    /// 标的名称
    pub entity_name: String,
    /// 标的类型（company / person / ...）
    pub entity_type: String,
    /// 档位
    pub tier: String,
    /// 为什么盯
    pub reason: String,
    /// 退出条件
    pub exit_condition: String,
    /// 产业链位置
    pub position_note: String,
    /// 最后一次改这个条目的时间
    pub updated_at: String,
    /// 在跟踪的论点条数（不含归档）——"我对它有多少判断"
    pub thesis_count: i64,
    /// 未结案的判据条数——"有多少事在等结果"
    pub falsifier_open: i64,
    /// 最近一次被复盘处置的时间（未复盘过为 None）
    pub last_review_at: Option<String>,
    /// 近 30 天的事件条数——"它最近有没有动静"
    pub event_count_30d: i64,
}

/// 生成一页的结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dossier {
    /// 标的 ID
    pub entity_id: i64,
    /// 一级标题用的名字
    pub title: String,
    /// 另存为对话框的预填名（不含扩展名）
    pub default_name: String,
    /// markdown 原文（复制与导出都用它，前端不做二次拼装）
    pub markdown: String,
    /// 收录的论点数
    pub thesis_count: usize,
    /// 收录的证据条数
    pub evidence_count: usize,
}

/// 读取覆盖清单
///
/// # 排序
/// 先按档位（核心 → 观察 → 本月退出），同档内按最后更新时间倒序。
/// **不按字母序**：清单的用途是"按重要性扫一眼"，字母序对扫读毫无帮助。
///
/// # 错误
/// 查询失败
pub async fn list(pool: &SqlitePool) -> Result<Vec<CoverageRow>, String> {
    let rows = sqlx::query(
        r#"
        SELECT c.entity_id, COALESCE(e.name, '') AS entity_name,
               COALESCE(e.entity_type, '') AS entity_type,
               c.tier, c.reason, c.exit_condition, c.position_note, c.updated_at,
               (SELECT COUNT(*) FROM theses t
                 WHERE t.entity_id = c.entity_id AND t.status <> 'archived') AS thesis_count,
               (SELECT COUNT(*) FROM thesis_falsifiers f
                  JOIN theses t ON t.id = f.thesis_id
                 WHERE t.entity_id = c.entity_id AND f.verdict = 'pending') AS falsifier_open,
               (SELECT MAX(ri.created_at) FROM thesis_review_items ri
                  JOIN theses t ON t.id = ri.thesis_id
                 WHERE t.entity_id = c.entity_id) AS last_review_at,
               (SELECT COUNT(*) FROM research_events re
                 WHERE re.entity_id = c.entity_id
                   AND julianday('now') - julianday(re.created_at) <= 30) AS event_count_30d
        FROM coverage c
        LEFT JOIN entities e ON e.id = c.entity_id
        ORDER BY CASE c.tier WHEN 'core' THEN 0 WHEN 'watch' THEN 1 ELSE 2 END,
                 c.updated_at DESC, c.entity_id ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取覆盖清单失败：{e}"))?;

    Ok(rows
        .iter()
        .map(|r| CoverageRow {
            entity_id: r.get("entity_id"),
            entity_name: r.get("entity_name"),
            entity_type: r.get("entity_type"),
            tier: r.get("tier"),
            reason: r.get("reason"),
            exit_condition: r.get("exit_condition"),
            position_note: r.get("position_note"),
            updated_at: r.get("updated_at"),
            thesis_count: r.get("thesis_count"),
            falsifier_open: r.get("falsifier_open"),
            last_review_at: r.get("last_review_at"),
            event_count_30d: r.get("event_count_30d"),
        })
        .collect())
}

/// 新增或更新一条覆盖
///
/// # 为什么只有这一个写入口
/// "新建覆盖"与"改覆盖"在用户眼里是同一个动作（都是填那几个框），
/// 分成两条命令只会让前端在两处判断"我是新建还是编辑"，而判断一错就会出现
/// 同标的重复行。这里用 `ON CONFLICT(entity_id) DO UPDATE` 一条兜住。
///
/// # 变更历史何时写
/// **只有档位变了才写**（含首次建档：`from=''`）。理由：档位是"我今年盯哪几个"
/// 的决策记录，改错别字不是决策；把每次文本编辑都记进去，历史会淹没真变化，
/// 用户也就不再看了。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `input` - 覆盖内容
///
/// # 返回值
/// 标的 ID
///
/// # 错误
/// 档位非法、实体不存在或超长、写入失败
pub async fn upsert(pool: &SqlitePool, input: CoverageInput) -> Result<i64, String> {
    let tier = input.tier.trim();
    if !valid_tier(tier) {
        return Err(format!("未知的覆盖档位: {}（支持 core / watch / exit）", tier));
    }
    let reason = input.reason.trim();
    let exit_condition = input.exit_condition.trim();
    let position_note = input.position_note.trim();
    // 上限不是洁癖：这几个字段会进导出的 markdown 单行，
    // 一行三千字在导出文件里既不可读，也会把 Obsidian 的编辑体验拖垮
    if reason.chars().count() > 200 {
        return Err("「为什么盯」最多 200 字".into());
    }
    if exit_condition.chars().count() > 300 {
        return Err("「退出条件」最多 300 字".into());
    }
    if position_note.chars().count() > 100 {
        return Err("「产业链位置」最多 100 字".into());
    }

    // 实体必须存在：给一个已删除的实体建覆盖会留下永远不显示的行
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM entities WHERE id = ?")
        .bind(input.entity_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("读取实体失败：{e}"))?;
    if exists.is_none() {
        return Err("标的已不存在，无法加入覆盖".into());
    }

    let old_tier: Option<String> = sqlx::query_scalar("SELECT tier FROM coverage WHERE entity_id = ?")
        .bind(input.entity_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("读取原覆盖失败：{e}"))?;

    sqlx::query(
        r#"
        INSERT INTO coverage (entity_id, tier, reason, exit_condition, position_note)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(entity_id) DO UPDATE SET
            tier = excluded.tier,
            reason = excluded.reason,
            exit_condition = excluded.exit_condition,
            position_note = excluded.position_note,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(input.entity_id)
    .bind(tier)
    .bind(reason)
    .bind(exit_condition)
    .bind(position_note)
    .execute(pool)
    .await
    .map_err(|e| format!("保存覆盖失败：{e}"))?;

    if old_tier.as_deref() != Some(tier) {
        let from = old_tier.unwrap_or_default();
        sqlx::query(
            "INSERT INTO coverage_history (entity_id, tier_from, tier_to, note) VALUES (?, ?, ?, ?)",
        )
        .bind(input.entity_id)
        .bind(&from)
        .bind(tier)
        .bind(reason)
        .execute(pool)
        .await
        .map_err(|e| format!("写入覆盖变更史失败：{e}"))?;
    }

    Ok(input.entity_id)
}

/// 把标的移出覆盖
///
/// **只删覆盖行，不动实体**：实体还可能在供血论点与事件，
/// 删实体会连带删掉证据（`thesis_evidence` 的 ref 无外键，但论点会被级联）。
/// "我不盯了"与"这个标的从我的世界消失"是两件事。
///
/// 变更史里留一条 `to=''` 的记录：复盘时要能回答"上次把一只移出覆盖，
/// 是在什么条件下决定的"（ROADMAP §1.3 的验收问题）。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `entity_id` - 标的 ID
///
/// # 错误
/// 删除或写历史失败
pub async fn remove(pool: &SqlitePool, entity_id: i64) -> Result<(), String> {
    let old: Option<(String, String)> =
        sqlx::query_as("SELECT tier, reason FROM coverage WHERE entity_id = ?")
            .bind(entity_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("读取原覆盖失败：{e}"))?;
    let Some((from, reason)) = old else {
        return Ok(());
    };
    sqlx::query("DELETE FROM coverage WHERE entity_id = ?")
        .bind(entity_id)
        .execute(pool)
        .await
        .map_err(|e| format!("移出覆盖失败：{e}"))?;
    // 理由必须跟着搬进历史：coverage 行删掉之后，"为什么盯过"这唯一的记录就没了，
    // 而它正是半年后要回答的那个问题
    sqlx::query("INSERT INTO coverage_history (entity_id, tier_from, tier_to, note) VALUES (?, ?, '', ?)")
        .bind(entity_id)
        .bind(&from)
        .bind(&reason)
        .execute(pool)
        .await
        .map_err(|e| format!("写入覆盖变更史失败：{e}"))?;
    Ok(())
}

/// 生成「一页」（五节 markdown 骨架）
///
/// # 输出形态是刻意朴素的
/// 只有标题、列表、引用块，没有表格、没有自定义语法、没有排版指令。
/// 理由：这页东西的终点是 Obsidian / Word，在那里排版是用户的活。
/// 我们如果输出带样式的 HTML 或自定义标记，用户第一次想改一个词就得先
/// 理解我们的格式——那等于把"能出东西"换成"能出我们的东西"。
///
/// # 五节的顺序及其不可换
/// 我的判断 → 判据与结论 → 反对证据 → 支持证据 → 变更记录。
/// **反对证据在支持证据之前**：这是全产品一致对抗确认偏误的位置
/// （`thesis::get_detail` 的排序、详情页的反驳置顶都是同一件事）。
/// 导出物如果反过来，等于在最后一道出口上把偏误还回去。
///
/// # 混合立场为什么不并进支持/反对
/// `stance='mixed'` 记的是"这条证据两面都有"。并进任何一边都是篡改，
/// 而丢弃它会让"我明明挂过这条"的记忆断掉。所以它单独成一节，
/// 且只在非空时出现。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `entity_id` - 标的 ID
///
/// # 返回值
/// [`Dossier`]：markdown 原文 + 标题 + 另存为预填名 + 计数
///
/// # 错误
/// 实体不存在或查询失败
pub async fn dossier(pool: &SqlitePool, entity_id: i64) -> Result<Dossier, String> {
    let entity: Option<(String, String)> =
        sqlx::query_as("SELECT name, COALESCE(entity_type, '') FROM entities WHERE id = ?")
            .bind(entity_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("读取标的失败：{e}"))?;
    let Some((name, _entity_type)) = entity else {
        return Err("标的已不存在".into());
    };

    let (tier, reason, exit_condition, position_note) =
        load_coverage_meta(pool, entity_id).await?;

    let theses = load_theses(pool, entity_id).await?;
    let falsifiers = load_falsifiers(pool, entity_id).await?;
    let evidence = load_evidence(pool, entity_id).await?;
    let revisions = load_revisions(pool, entity_id).await?;

    // 生成日期只用于展示与文件名，取本地日期而不是 UTC：
    // 北京时间 08:00 导出的一页若写成昨天的日期，用户会以为导出失败过。
    let today: String = sqlx::query_scalar("SELECT date('now', 'localtime')")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("读取当前日期失败：{e}"))?;

    let mut md = String::new();
    md.push_str(&format!("# {}\n\n", name));
    md.push_str(&format!(
        "> 覆盖档位：{} · 生成于 {}\n",
        tier_label(&tier),
        today
    ));
    if !reason.is_empty() {
        md.push_str(&format!("> 为什么盯：{}\n", reason));
    }
    if !exit_condition.is_empty() {
        md.push_str(&format!("> 退出条件：{}\n", exit_condition));
    }
    if !position_note.is_empty() {
        md.push_str(&format!("> 产业链位置：{}\n", position_note));
    }
    md.push('\n');

    md.push_str("## 我的判断\n\n");
    if theses.is_empty() {
        md.push_str("_还没有对这个标的的判断。判断是一句话，写在「研究工作台 → 论点」里。_\n\n");
    }
    for t in &theses {
        md.push_str(&format!(
            "- **{}**（置信 {}/5 · {} · {} 建立）\n",
            t.statement,
            t.confidence,
            status_label(&t.status),
            date_part(&t.created_at)
        ));
    }
    if !theses.is_empty() {
        md.push('\n');
    }

    md.push_str("## 判据与结论\n\n");
    if falsifiers.is_empty() {
        md.push_str("_还没有写判据。没有判据的判断无法被证伪，也就不会在复盘时被顶到面前。_\n\n");
    }
    for f in &falsifiers {
        md.push_str(&format!("- {}\n", f.statement));
        md.push_str(&format!("  - 窗口：{}\n", window_label(f)));
        md.push_str(&format!("  - 结论：{}\n", verdict_label(&f.verdict)));
    }
    if !falsifiers.is_empty() {
        md.push('\n');
    }

    let oppose: Vec<&EvidenceLine> = evidence.iter().filter(|e| e.stance == "oppose").collect();
    let support: Vec<&EvidenceLine> = evidence.iter().filter(|e| e.stance == "support").collect();
    let mixed: Vec<&EvidenceLine> = evidence.iter().filter(|e| e.stance == "mixed").collect();

    md.push_str("## 反对证据\n\n");
    if oppose.is_empty() {
        md.push_str("_一条反对证据都没有——这本身就是个信号。_\n\n");
    } else {
        push_evidence(&mut md, &oppose);
    }

    md.push_str("## 支持证据\n\n");
    if support.is_empty() {
        md.push_str("_暂无支持证据。_\n\n");
    } else {
        push_evidence(&mut md, &support);
    }

    if !mixed.is_empty() {
        md.push_str("## 存疑 / 混合证据\n\n");
        push_evidence(&mut md, &mixed);
    }

    md.push_str("## 变更记录\n\n");
    if revisions.is_empty() {
        md.push_str("_还没有改过判断。_\n\n");
    }
    for r in &revisions {
        let reason = if r.reason.trim().is_empty() {
            String::new()
        } else {
            format!("：{}", r.reason.trim())
        };
        md.push_str(&format!(
            "- {} · {}：{} → {}{}\n",
            date_part(&r.created_at),
            field_label(&r.field),
            blank_dash(&r.from_value),
            blank_dash(&r.to_value),
            reason
        ));
    }

    let default_name = format!("{}-研究一页-{}", name, today);
    Ok(Dossier {
        entity_id,
        title: format!("{} · 研究一页", name),
        default_name,
        markdown: md,
        thesis_count: theses.len(),
        evidence_count: evidence.len(),
    })
}

/// 覆盖元信息（无覆盖行时全部为空串，档位为空表示"未在覆盖内"）
async fn load_coverage_meta(
    pool: &SqlitePool,
    entity_id: i64,
) -> Result<(String, String, String, String), String> {
    let row = sqlx::query(
        "SELECT tier, reason, exit_condition, position_note FROM coverage WHERE entity_id = ?",
    )
    .bind(entity_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("读取覆盖信息失败：{e}"))?;
    Ok(match row {
        Some(r) => (
            r.get("tier"),
            r.get("reason"),
            r.get("exit_condition"),
            r.get("position_note"),
        ),
        None => (String::new(), String::new(), String::new(), String::new()),
    })
}

/// 一页里的判断行
struct ThesisLine {
    statement: String,
    confidence: i64,
    status: String,
    created_at: String,
}

/// 一页里的判据行
struct FalsifierLine {
    statement: String,
    window_kind: String,
    due_at: Option<String>,
    every_n_days: Option<i64>,
    verdict: String,
}

/// 一页里的证据行
///
/// `source_name` / `published_at` 来自 [`crate::research::review`] 维护的
/// `thesis_evidence_source`：源文章被保留策略清掉之后，`snapshot_text` 保住了
/// "说了什么"，这张表保住"谁说的、哪天说的"。这正是 TASK-00 存在的理由。
struct EvidenceLine {
    stance: String,
    snapshot_text: String,
    note: String,
    source_name: String,
    published_at: String,
}

/// 一页里的变更行
struct RevisionLine {
    field: String,
    from_value: String,
    to_value: String,
    reason: String,
    created_at: String,
}

/// 读该标的的在跟踪论点（不含归档）
///
/// 归档的排除在外：归档的含义就是"这条判断我不再跟了"，
/// 把它写进对外的一页，会让人以为它还在观察名单上。
async fn load_theses(pool: &SqlitePool, entity_id: i64) -> Result<Vec<ThesisLine>, String> {
    let rows = sqlx::query(
        "SELECT statement, confidence, status, created_at FROM theses \
         WHERE entity_id = ? AND status <> 'archived' \
         ORDER BY CASE status WHEN 'active' THEN 0 WHEN 'confirmed' THEN 1 ELSE 2 END, \
                  created_at DESC, id DESC",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取论点失败：{e}"))?;
    Ok(rows
        .iter()
        .map(|r| ThesisLine {
            statement: r.get("statement"),
            confidence: r.get("confidence"),
            status: r.get("status"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// 读该标的全部判据（跨它的所有论点，按论点建立顺序）
async fn load_falsifiers(pool: &SqlitePool, entity_id: i64) -> Result<Vec<FalsifierLine>, String> {
    let rows = sqlx::query(
        "SELECT f.statement, f.window_kind, f.due_at, f.every_n_days, f.verdict \
         FROM thesis_falsifiers f JOIN theses t ON t.id = f.thesis_id \
         WHERE t.entity_id = ? AND t.status <> 'archived' \
         ORDER BY CASE f.verdict WHEN 'pending' THEN 0 ELSE 1 END, f.due_at ASC, f.id ASC",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取判据失败：{e}"))?;
    Ok(rows
        .iter()
        .map(|r| FalsifierLine {
            statement: r.get("statement"),
            window_kind: r.get("window_kind"),
            due_at: r.get("due_at"),
            every_n_days: r.get("every_n_days"),
            verdict: r.get("verdict"),
        })
        .collect())
}

/// 读该标的全部证据（带来源身份）
///
/// 排序：先说反对（0）再说混合（1）最后支持（2），同档内新证据在前。
/// `dossier` 里虽然有分区兜底，这里仍按立场排一遍——将来若有人改成
/// "证据按时间排列"，分区还在，不会因为排序改动而让反证沉到危险的位置。
async fn load_evidence(pool: &SqlitePool, entity_id: i64) -> Result<Vec<EvidenceLine>, String> {
    let rows = sqlx::query(
        r#"
        SELECT v.stance, v.snapshot_text, v.note,
               COALESCE(src.source_name, '') AS source_name,
               COALESCE(NULLIF(src.published_at, ''), substr(v.created_at, 1, 10)) AS published_at
          FROM thesis_evidence v
          JOIN theses t ON t.id = v.thesis_id
          LEFT JOIN thesis_evidence_source src ON src.evidence_id = v.id
         WHERE t.entity_id = ? AND t.status <> 'archived'
         ORDER BY CASE v.stance WHEN 'oppose' THEN 0 WHEN 'mixed' THEN 1 ELSE 2 END,
                  v.created_at DESC, v.id DESC
        "#,
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取证据失败：{e}"))?;
    Ok(rows
        .iter()
        .map(|r| EvidenceLine {
            stance: r.get("stance"),
            snapshot_text: r.get("snapshot_text"),
            note: r.get("note"),
            source_name: r.get("source_name"),
            published_at: r.get("published_at"),
        })
        .collect())
}

/// 读该标的的变更记录（跨它的所有论点）
async fn load_revisions(pool: &SqlitePool, entity_id: i64) -> Result<Vec<RevisionLine>, String> {
    let rows = sqlx::query(
        "SELECT r.field, r.from_value, r.to_value, r.reason, r.created_at \
         FROM thesis_revisions r JOIN theses t ON t.id = r.thesis_id \
         WHERE t.entity_id = ? AND t.status <> 'archived' \
         ORDER BY r.created_at DESC, r.id DESC",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取变更记录失败：{e}"))?;
    Ok(rows
        .iter()
        .map(|r| RevisionLine {
            field: r.get("field"),
            from_value: r.get("from_value"),
            to_value: r.get("to_value"),
            reason: r.get("reason"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// 把一组证据写进 markdown
///
/// 每条一行 `- 日期 · 来源 · 「快照」`，备注折进下一层缩进列表。
/// 快照压成单行并截断到 120 字：导出物是给人扫读的，
/// 一条三百字的报价原文会把五节的层次压平，用户就只看第一条了。
fn push_evidence(md: &mut String, items: &[&EvidenceLine]) {
    for e in items {
        let source = if e.source_name.trim().is_empty() {
            "来源未记录".to_string()
        } else {
            e.source_name.trim().to_string()
        };
        let snap = one_line(&e.snapshot_text, 120);
        md.push_str(&format!(
            "- {} · {} · 「{}」\n",
            date_part(&e.published_at),
            source,
            snap
        ));
        let note = e.note.trim();
        if !note.is_empty() {
            md.push_str(&format!("  - 备注：{}\n", one_line(note, 200)));
        }
    }
    md.push('\n');
}

/// 压缩空白并截断到 `max` 个字符（按字符而非字节，中文不会被切坏）
fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    format!("{}…", flat.chars().take(max).collect::<String>())
}

/// 取时间的日期部分（`2026-10-28 12:00:00` → `2026-10-28`）
///
/// 空串原样返回：导出物里宁可留空，也不要出现 `1970-01-01` 这种假日期。
fn date_part(ts: &str) -> String {
    ts.split([' ', 'T']).next().unwrap_or(ts).to_string()
}

/// 空值占位
fn blank_dash(v: &str) -> String {
    if v.trim().is_empty() {
        "（空）".into()
    } else {
        v.trim().to_string()
    }
}

/// 档位中文名
fn tier_label(tier: &str) -> &'static str {
    match tier {
        TIER_CORE => "核心",
        TIER_WATCH => "观察",
        TIER_EXIT => "本月退出",
        _ => "未加入覆盖",
    }
}

/// 论点状态中文名
fn status_label(status: &str) -> &'static str {
    match status {
        "active" => "跟进中",
        "confirmed" => "已确认",
        "refuted" => "已否定",
        "archived" => "已归档",
        _ => "未知",
    }
}

/// 判据结论中文名
///
/// 取值与 `review::falsifier_save` 的写入口径一致（`pending` / `verified` /
/// `refuted` / `unclear`）。措辞刻意带上"这对判断意味着什么"：
/// 「判据已成立」单独看是中性的，但它真正的含义是"我原来押的那个前提错了"，
/// 只写前半句，读的人会把它当成好消息。
fn verdict_label(verdict: &str) -> &'static str {
    match verdict {
        "verified" => "判据已成立（该下调这条判断）",
        "refuted" => "判据未成立（判断仍站得住）",
        "unclear" => "存疑（材料不足以下结论）",
        "pending" => "待验证",
        _ => "待验证",
    }
}

/// 变更字段中文名
fn field_label(field: &str) -> &'static str {
    match field {
        "confidence" => "置信度",
        "status" => "状态",
        "statement" => "判断表述",
        _ => "其它",
    }
}

/// 判据窗口的可读描述
fn window_label(f: &FalsifierLine) -> String {
    if f.window_kind == "cycle" {
        match f.every_n_days {
            Some(n) => format!("每 {} 天检查一次", n),
            None => "周期检查".to_string(),
        }
    } else {
        match f.due_at.as_deref() {
            Some(d) if !d.is_empty() => format!("{} 到期", date_part(d)),
            _ => "未设到期日".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 真实 schema 内存库（手抄表结构会在演进时静默漂移）
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("建库失败");
        crate::db::create_tables(&pool).await.expect("建表失败");
        pool
    }

    /// 造一个实体
    async fn seed_entity(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO entities (name, entity_type) VALUES (?, 'company') RETURNING id")
            .bind(name)
            .fetch_one(pool)
            .await
            .expect("插实体失败")
    }

    /// 造一个论点（`created_at` 用当前时间，本组用例不关心天数）
    async fn seed_thesis(pool: &SqlitePool, entity_id: i64, statement: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO theses (statement, entity_id) VALUES (?, ?) RETURNING id")
            .bind(statement)
            .bind(entity_id)
            .fetch_one(pool)
            .await
            .expect("插论点失败")
    }

    /// 挂一条带来源的证据
    ///
    /// `ref_id` 用快照文本的哈希做区分：`thesis_evidence` 上有
    /// `UNIQUE(thesis_id, kind, ref_id)`，写死同一个 ref_id 第二条就撞了。
    async fn attach_evidence(pool: &SqlitePool, thesis_id: i64, stance: &str, text: &str, source: &str) {
        let ref_id: i64 = text.bytes().map(|b| b as i64).sum();
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance, snapshot_text) \
             VALUES (?, 'highlight', ?, ?, ?) RETURNING id",
        )
        .bind(thesis_id)
        .bind(ref_id)
        .bind(stance)
        .bind(text)
        .fetch_one(pool)
        .await
        .expect("插证据失败");
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

    fn input(entity_id: i64, tier: &str, reason: &str) -> CoverageInput {
        CoverageInput {
            entity_id,
            tier: tier.into(),
            reason: reason.into(),
            exit_condition: String::new(),
            position_note: String::new(),
        }
    }

    #[tokio::test]
    async fn upsert_is_idempotent_per_entity() {
        let pool = test_pool().await;
        let eid = seed_entity(&pool, "宁德时代").await;

        upsert(&pool, input(eid, TIER_CORE, "储能出货拐点")).await.unwrap();
        upsert(&pool, input(eid, TIER_CORE, "储能出货拐点已确认")).await.unwrap();

        let rows = list(&pool).await.unwrap();
        assert_eq!(rows.len(), 1, "同一标的只能有一行覆盖");
        assert_eq!(rows[0].reason, "储能出货拐点已确认");
    }

    #[tokio::test]
    async fn history_records_tier_changes_only() {
        let pool = test_pool().await;
        let eid = seed_entity(&pool, "宁德时代").await;

        // 首次加入：'' → core
        upsert(&pool, input(eid, TIER_CORE, "拐点")).await.unwrap();
        // 只改文字，不改档位：不该多一条历史
        upsert(&pool, input(eid, TIER_CORE, "拐点（补一句）")).await.unwrap();
        // 改档位：该多一条
        upsert(&pool, input(eid, TIER_EXIT, "逻辑已破")).await.unwrap();

        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT tier_from, tier_to, note FROM coverage_history ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2, "只有档位变化才进历史，改错别字不算");
        assert_eq!(rows[0], (String::new(), TIER_CORE.to_string(), "拐点".to_string()));
        assert_eq!(rows[1].1, TIER_EXIT);
    }

    #[tokio::test]
    async fn remove_keeps_entity_and_leaves_trace() {
        let pool = test_pool().await;
        let eid = seed_entity(&pool, "宁德时代").await;
        upsert(&pool, input(eid, TIER_WATCH, "先看着")).await.unwrap();

        remove(&pool, eid).await.unwrap();

        // 覆盖没了
        assert!(list(&pool).await.unwrap().is_empty());
        // 实体还在：移出覆盖不等于删标的
        let still: Option<i64> = sqlx::query_scalar("SELECT id FROM entities WHERE id = ?")
            .bind(eid)
            .fetch_optional(&pool)
            .await
            .unwrap();
        assert!(still.is_some(), "移出覆盖不该删实体");
        // 历史留痕：to 为空串表示移出，note 保留当时的理由
        let row: (String, String, String) = sqlx::query_as(
            "SELECT tier_from, tier_to, note FROM coverage_history ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, TIER_WATCH);
        assert_eq!(row.1, "", "移出覆盖用空 tier_to 表达");
        assert_eq!(row.2, "先看着", "理由要跟着留，否则半年后说不清当初为什么盯");
    }

    #[tokio::test]
    async fn rejects_unknown_tier_and_missing_entity() {
        let pool = test_pool().await;
        let eid = seed_entity(&pool, "宁德时代").await;
        assert!(upsert(&pool, input(eid, "vip", "")).await.is_err());
        assert!(upsert(&pool, input(9999, TIER_CORE, "")).await.is_err());
    }

    #[tokio::test]
    async fn dossier_puts_oppose_before_support() {
        let pool = test_pool().await;
        let eid = seed_entity(&pool, "宁德时代").await;
        let tid = seed_thesis(&pool, eid, "储能出货将超预期").await;
        attach_evidence(&pool, tid, "support", "出货增长 40%", "财新").await;
        attach_evidence(&pool, tid, "oppose", "友商降价抢单", "晚点").await;

        let d = dossier(&pool, eid).await.unwrap();
        let oppose_at = d.markdown.find("## 反对证据").expect("应有一段反对证据");
        let support_at = d.markdown.find("## 支持证据").expect("应有一段支持证据");
        assert!(oppose_at < support_at, "反对证据必须排在支持证据之前");
        // 反证与支持各自带着来源
        assert!(d.markdown.contains("晚点"));
        assert!(d.markdown.contains("财新"));
        assert_eq!(d.evidence_count, 2);
        assert_eq!(d.thesis_count, 1);
    }

    #[tokio::test]
    async fn dossier_keeps_mixed_evidence_in_own_section() {
        let pool = test_pool().await;
        let eid = seed_entity(&pool, "宁德时代").await;
        let tid = seed_thesis(&pool, eid, "判断").await;
        attach_evidence(&pool, tid, "mixed", "两面说法都有", "第一财经").await;

        let d = dossier(&pool, eid).await.unwrap();
        assert!(
            d.markdown.contains("## 存疑 / 混合证据"),
            "混合立场不能被并进支持或反对，也不能丢"
        );
        assert!(d.markdown.contains("两面说法都有"));
    }

    #[tokio::test]
    async fn dossier_marks_evidence_without_source() {
        let pool = test_pool().await;
        let eid = seed_entity(&pool, "宁德时代").await;
        let tid = seed_thesis(&pool, eid, "判断").await;
        // 不带来源行：模拟 TASK-00 之前的证据
        sqlx::query(
            "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance, snapshot_text) \
             VALUES (?, 'article', 1, 'support', '原文片段')",
        )
        .bind(tid)
        .execute(&pool)
        .await
        .unwrap();

        let d = dossier(&pool, eid).await.unwrap();
        assert!(d.markdown.contains("来源未记录"), "来源缺失要明说，不能留空让人以为是排版问题");
    }

    #[tokio::test]
    async fn dossier_of_unknown_entity_errors() {
        let pool = test_pool().await;
        assert!(dossier(&pool, 9999).await.is_err());
    }

    #[test]
    fn coverage_row_serializes_as_camel_case() {
        // 跨边界字段名是契约：Rust 侧 snake_case、前端 camelCase，
        // 漏 rename 不会报错，只表现为前端字段 undefined（数字空、日期 NaN）
        let row = CoverageRow {
            entity_id: 1,
            entity_name: "宁德时代".into(),
            entity_type: "company".into(),
            tier: TIER_CORE.into(),
            reason: String::new(),
            exit_condition: String::new(),
            position_note: String::new(),
            updated_at: "2026-11-01 00:00:00".into(),
            thesis_count: 2,
            falsifier_open: 1,
            last_review_at: None,
            event_count_30d: 3,
        };
        let v = serde_json::to_value(&row).unwrap();
        assert!(v.get("entityId").is_some());
        assert!(v.get("thesisCount").is_some());
        assert!(v.get("falsifierOpen").is_some());
        assert!(v.get("eventCount30d").is_some());
        assert!(v.get("entity_id").is_none(), "不该同时输出 snake_case");
    }
}
