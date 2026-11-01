//! 复盘的提醒链路（周节律 / 一条汇总 / 无料不发）
//!
//! # 职责
//! 自动化任务 `thesis_review` 的执行器：筛出"该看一眼"的判断，落一条提醒记录，
//! 再发**一条**汇总系统通知。
//!
//! # 三条刻意的设计
//! 1. **一条汇总，不按论点发多条**。周节律下"每条判断一条通知"等于催办；
//!    复盘要的是"停下来想"，不是"清待办"（PRD §3.4）。
//! 2. **无候选则彻底安静**：既不落库也不发通知，任务结果只写一句"本期无需复盘"。
//!    安静态是这个功能的正常形态，不是失败。
//! 3. **先落库再说话**：与"先记账再发言"一致。若顺序反过来，
//!    通知发出去了但落库失败，下一次会立刻重复提醒同一批判断。
//!
//! # 去重为什么落库而不是进程内表
//! 进程内计时在应用重启后清零（[`crate::notify`] 的实体限频就是这么做的，
//! 对"提醒"是可接受取舍）。但复盘的节律是 7 天，用户一天里可能开关应用十次，
//! 用进程内状态会让"7 天一次"退化成"每次启动一次"。所以 `thesis_reminders`
//! 落表，键是 `(thesis_id, 'review')`。
//!
//! # 复合主键带来的两个必改点（改了会静默错）
//! - 候选查询必须带 `kind = 'review'`：将来写入 `falsifier_due` 之后，
//!   不加限定会读到另一类提醒的时间戳，把"昨天刚提醒过判据到期"误判成"刚提醒过复盘"；
//! - 去重判断必须 `WHERE kind = 'review'` 而不是裸 `WHERE thesis_id IN (...)`。
//!
//! # 与通用"任务完成"回执的关系
//! `run_task_once` 对成功任务会无条件发一条"任务完成：xxx"。本模块自己决定发不发，
//! 所以那条回执必须在 `run_task_once` 里对 `thesis_review` 关掉——
//! 否则每 7 天会先弹"任务完成"，安静态设计当场作废，且那条噪音无法单独关。

use sqlx::SqlitePool;

use crate::db::Setting;
use crate::research::review::{overview, ReviewCard};
use crate::research::TaskOutcome;

/// 去重窗口（天）
///
/// 与复盘节律一致：7 天内提醒过的判断不重复提醒。用 SQL 的 `julianday`
/// 而不是 Rust 侧算时间，避免跨时区/跨年时把窗口算歪。
const REMIND_WINDOW_DAYS: i64 = 7;

/// 子开关键名（缺省开，只有 `"0"` 才算关，判据与 `event_alert_enabled` 一致）
const REMIND_SWITCH: &str = "thesis_review_reminder_enabled";

/// 一次提醒的候选集合
///
/// 分开记 due 与 health 的条数只为了写通知文案（"2 条判据到期、1 条结构有偏"
/// 比"3 条需要复查"有用得多：前者告诉用户去干什么，后者只告诉他出事了）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemindBatch {
    /// 待提醒的论点 ID（去重后）
    pub ids: Vec<i64>,
    /// 其中来自 A 区（判据到期）
    pub due_count: usize,
    /// 其中来自 B 区（结构有偏，且含 high 级命中）
    pub health_count: usize,
}

impl RemindBatch {
    fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// 提醒执行结果（区分"关掉了" / "没料" / "发了"，供任务结果文案使用）
///
/// 不用 `Result<u64, String>`：0 有两种完全不同的含义（关掉了 / 没料），
/// 混成同一个 0 会让任务列表里显示"本期无需复盘"——而用户明明是自己关掉的开关。
/// 这不是致命错误，但会让人以为功能坏了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemindOutcome {
    /// 子开关关闭，什么都没做（时间戳不前进）
    Disabled,
    /// 无候选，不发通知
    Quiet,
    /// 已落库并发出汇总通知
    Sent(RemindBatch),
}

/// 读取复盘提醒子开关（缺省开启）
///
/// 与 [`crate::notify`] 的 `event_alert_enabled` 同一判据：只有 `"0"` 才算关闭，
/// 键不存在 / 空串 / 其它值都按开启处理，避免历史脏数据把功能误关。
async fn reminder_enabled(pool: &SqlitePool) -> bool {
    match Setting::get(pool, REMIND_SWITCH).await {
        Ok(Some(value)) => value.trim() != "0",
        _ => true,
    }
}

/// 从四区里挑出该提醒的判断
///
/// # 复用 `overview` 而不是另写一条 SQL
/// 提醒与页面必须用**同一套判据**决定"谁该看一眼"：另写一条 SQL 就会出现
/// "通知说有 3 条，点进去页面是空的"这种最伤信任的错位。这里多付一次
/// 四区装配的查询代价（每周一次），换掉"两处阈值各改各的"这个长期风险。
///
/// 顺带白拿一个正确行为：`overview` 会排除"本次会话已处置"的判断——
/// 用户已经处理完的，本周不该再被通知一次。
///
/// # 筛选口径
/// - A 区（判据到期）：全部收；
/// - B 区（结构有偏）：只收含 `high` 级命中的。`mid` / `low` 留在页面上看，
///   但不值得把人从别的应用里拽出来——周节律的通知预算只有一条，
///   用在中等级问题上，高危问题就会被稀释。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 去重后的候选集合（按论点 ID 升序，便于测试与日志比对）
///
/// # 错误
/// 读取复盘数据或提醒记录失败
pub async fn candidates(pool: &SqlitePool) -> Result<RemindBatch, String> {
    let ov = overview(pool).await?;

    let mut batch = RemindBatch::default();
    let mut seen = std::collections::HashSet::new();

    for card in &ov.due {
        if seen.insert(card.thesis_id) {
            batch.ids.push(card.thesis_id);
            batch.due_count += 1;
        }
    }
    for card in &ov.health {
        if !has_high_hit(card) {
            continue;
        }
        if seen.insert(card.thesis_id) {
            batch.ids.push(card.thesis_id);
            batch.health_count += 1;
        }
    }

    // 7 天窗口过滤：把刚提醒过的摘掉。单独一次查询而不是塞进 `overview`，
    // 是因为"谁该看一眼"与"谁最近被提醒过"是两件事，混进同一条 SQL 后
    // 页面上的四区也会被去重逻辑污染（页面不该因为提醒过就少显示一条）。
    let recent: Vec<i64> = sqlx::query_scalar(
        "SELECT thesis_id FROM thesis_reminders \
         WHERE kind = 'review' \
           AND julianday('now') - julianday(last_reminded_at) < ?1",
    )
    .bind(REMIND_WINDOW_DAYS)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取提醒记录失败：{e}"))?;

    if recent.is_empty() {
        batch.ids.sort_unstable();
        return Ok(batch);
    }

    let recent: std::collections::HashSet<i64> = recent.into_iter().collect();
    // 计数要跟着过滤一起减，否则通知文案会说"3 条"而实际只推了 1 条
    batch.due_count = 0;
    batch.health_count = 0;
    let ov_due: std::collections::HashSet<i64> = ov.due.iter().map(|c| c.thesis_id).collect();
    batch.ids.retain(|id| {
        if recent.contains(id) {
            return false;
        }
        if ov_due.contains(id) {
            batch.due_count += 1;
        } else {
            batch.health_count += 1;
        }
        true
    });
    batch.ids.sort_unstable();
    Ok(batch)
}

/// 卡片是否含 `high` 级命中
fn has_high_hit(card: &ReviewCard) -> bool {
    card.hits.iter().any(|h| h.severity == "high")
}

/// 落库 + 发一条汇总通知
///
/// # 顺序（不可颠倒）
/// 候选 → 落库 → 发通知。发通知放最后：`notify::send` 内部三重门可能整条跳过
/// （窗口聚焦 / 总开关关闭 / 无桌面环境），这属于"没发出去但已消费掉本周窗口"，
/// 是可接受的（用户正盯着应用，页面本身就是提醒）；反过来先发后落库则可能
/// 通知发了、时间戳没写，下一分钟又提醒一次。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// [`RemindOutcome`]：关闭 / 无料 / 已发（含条数明细）
///
/// # 错误
/// 读取候选或写入提醒记录失败
pub async fn notify_review(pool: &SqlitePool) -> Result<RemindOutcome, String> {
    // 开关判断放在最前：关掉时连候选都不查，也**不动时间戳**。
    // 若关了开关还写时间戳，等于白吃掉一周窗口——用户重新打开开关后
    // 会发现"怎么一周都没提醒"。
    if !reminder_enabled(pool).await {
        return Ok(RemindOutcome::Disabled);
    }

    let batch = candidates(pool).await?;
    if batch.is_empty() {
        return Ok(RemindOutcome::Quiet);
    }

    for id in &batch.ids {
        sqlx::query(
            "INSERT INTO thesis_reminders (thesis_id, kind, last_reminded_at, remind_count) \
             VALUES (?1, 'review', CURRENT_TIMESTAMP, 1) \
             ON CONFLICT(thesis_id, kind) DO UPDATE SET \
                 last_reminded_at = CURRENT_TIMESTAMP, \
                 remind_count = thesis_reminders.remind_count + 1",
        )
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("写入提醒记录失败：{e}"))?;
    }

    crate::notify::send(pool, "该复盘了", &remind_body(&batch)).await;
    Ok(RemindOutcome::Sent(batch))
}

/// 拼通知正文
///
/// 只说"有多少条、分几类"，不列论点标题：系统通知的展示宽度有限，
/// 列了会被截断成半句话；且标题本身可能敏感（判断是私人的）。
fn remind_body(batch: &RemindBatch) -> String {
    let mut parts = Vec::new();
    if batch.due_count > 0 {
        parts.push(format!("{} 条判据到期", batch.due_count));
    }
    if batch.health_count > 0 {
        parts.push(format!("{} 条结构有偏", batch.health_count));
    }
    format!("{}。按 W 进入复盘页", parts.join("、"))
}

/// 自动化任务执行器（被 [`crate::research::run_task_once`] 调用）
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// - 关闭：`Success("复盘提醒已关闭")`——这不是失败，别在任务健康度里标红；
/// - 无料：`Success("本期无需复盘")`；
/// - 已发：`Success("已提醒 N 条待复查判断")`；
/// - 出错：`Failed`。
pub async fn run_review_task(pool: &SqlitePool) -> TaskOutcome {
    match notify_review(pool).await {
        Ok(RemindOutcome::Disabled) => TaskOutcome::Success("复盘提醒已关闭".into()),
        Ok(RemindOutcome::Quiet) => TaskOutcome::Success("本期无需复盘".into()),
        Ok(RemindOutcome::Sent(batch)) => {
            TaskOutcome::Success(format!("已提醒 {} 条待复查判断", batch.ids.len()))
        }
        Err(e) => TaskOutcome::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 建真实 schema 的内存库（手抄表结构会随演进静默漂移）
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
    /// 同一测试里造两个论点会撞上。用序号而不是让调用方传名字，
    /// 是因为这些用例根本不关心实体是谁（健康度规则只看计数与天数）。
    ///
    /// @param age_days - 论点的 `created_at` 距今天数（健康度 H1/H4 依赖它）
    /// @returns `(entity_id, thesis_id)`
    async fn seed_thesis(pool: &SqlitePool, statement: &str, age_days: i64) -> (i64, i64) {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let eid: i64 = sqlx::query_scalar(
            "INSERT INTO entities (name, entity_type) VALUES (?, 'company') RETURNING id",
        )
        .bind(format!("标的{}", n))
        .fetch_one(pool)
        .await
        .expect("插实体失败");
        let tid: i64 = sqlx::query_scalar(
            "INSERT INTO theses (statement, entity_id, created_at) \
             VALUES (?, ?, datetime('now', ?)) RETURNING id",
        )
        .bind(statement)
        .bind(eid)
        .bind(format!("-{} days", age_days))
        .fetch_one(pool)
        .await
        .expect("插论点失败");
        (eid, tid)
    }

    /// 挂 n 条支持证据（不带来源行，H3 会命中；本组用例不关心 H3）
    async fn attach_support(pool: &SqlitePool, thesis_id: i64, n: i64) {
        for i in 0..n {
            sqlx::query(
                "INSERT INTO thesis_evidence (thesis_id, kind, ref_id, stance, snapshot_text) \
                 VALUES (?, 'highlight', ?, 'support', '原文')",
            )
            .bind(thesis_id)
            .bind(1000 + i)
            .execute(pool)
            .await
            .expect("插证据失败");
        }
    }

    /// 造一条已到期的判据（A 区）
    async fn seed_due_falsifier(pool: &SqlitePool, thesis_id: i64) {
        sqlx::query(
            "INSERT INTO thesis_falsifiers (thesis_id, statement, window_kind, due_at) \
             VALUES (?, '如果下季度出货低于 100GWh 则判断不成立', 'date', date('now', '-3 days'))",
        )
        .bind(thesis_id)
        .execute(pool)
        .await
        .expect("插判据失败");
    }

    #[tokio::test]
    async fn no_candidates_is_quiet() {
        let pool = test_pool().await;
        // 一条有判据、未到期、无健康度命中的判断 → 三区都不进
        let (_, tid) = seed_thesis(&pool, "储能出货将超预期", 1).await;
        sqlx::query(
            "INSERT INTO thesis_falsifiers (thesis_id, statement, window_kind, due_at) \
             VALUES (?, '如果出货低于 100GWh', 'date', date('now', '+30 days'))",
        )
        .bind(tid)
        .execute(&pool)
        .await
        .expect("插判据失败");

        let outcome = notify_review(&pool).await.expect("执行失败");
        assert_eq!(outcome, RemindOutcome::Quiet);

        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_reminders")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "无候选时不该落提醒记录");
    }

    #[tokio::test]
    async fn due_falsifier_is_reminded_once() {
        let pool = test_pool().await;
        let (_, tid) = seed_thesis(&pool, "储能出货将超预期", 3).await;
        seed_due_falsifier(&pool, tid).await;

        match notify_review(&pool).await.expect("执行失败") {
            RemindOutcome::Sent(batch) => {
                assert_eq!(batch.ids, vec![tid]);
                assert_eq!(batch.due_count, 1);
                assert_eq!(batch.health_count, 0);
            }
            other => panic!("应当发出提醒，实际 {:?}", other),
        }

        // 7 天内再跑：窗口内不重复
        assert_eq!(
            notify_review(&pool).await.expect("执行失败"),
            RemindOutcome::Quiet,
            "同一提示窗口内不应重复提醒"
        );

        let count: i64 = sqlx::query_scalar("SELECT remind_count FROM thesis_reminders WHERE thesis_id = ?")
            .bind(tid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "重复提醒不该叠加计数");
    }

    #[tokio::test]
    async fn seven_day_window_expires() {
        let pool = test_pool().await;
        let (_, tid) = seed_thesis(&pool, "储能出货将超预期", 3).await;
        seed_due_falsifier(&pool, tid).await;

        // 手工打一条 8 天前的提醒记录，模拟"上周提醒过"
        sqlx::query(
            "INSERT INTO thesis_reminders (thesis_id, kind, last_reminded_at, remind_count) \
             VALUES (?, 'review', datetime('now', '-8 days'), 1)",
        )
        .bind(tid)
        .execute(&pool)
        .await
        .unwrap();

        match notify_review(&pool).await.expect("执行失败") {
            RemindOutcome::Sent(batch) => assert_eq!(batch.ids, vec![tid]),
            other => panic!("窗口已过应当再次提醒，实际 {:?}", other),
        }

        let count: i64 = sqlx::query_scalar("SELECT remind_count FROM thesis_reminders WHERE thesis_id = ?")
            .bind(tid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2, "第二次提醒应累加计数");
    }

    #[tokio::test]
    async fn only_high_severity_health_cards_are_reminded() {
        let pool = test_pool().await;
        // H1：支持 ≥3、反驳 0、建立 ≥14 天 → high
        let (_, high_tid) = seed_thesis(&pool, "高优先级判断", 20).await;
        attach_support(&pool, high_tid, 3).await;
        // H6：活跃、无变更、≥90 天 → low（只提示不推送）
        let (_, low_tid) = seed_thesis(&pool, "低优先级判断", 120).await;
        sqlx::query(
            "INSERT INTO thesis_falsifiers (thesis_id, statement, window_kind, due_at) \
             VALUES (?, '长期观察条件', 'date', date('now', '+60 days'))",
        )
        .bind(low_tid)
        .execute(&pool)
        .await
        .unwrap();

        match notify_review(&pool).await.expect("执行失败") {
            RemindOutcome::Sent(batch) => {
                assert!(batch.ids.contains(&high_tid), "high 级命中应当被提醒");
                assert!(!batch.ids.contains(&low_tid), "low 级（H6）不该推送");
                assert_eq!(batch.health_count, 1);
                assert_eq!(batch.due_count, 0);
            }
            other => panic!("应当发出提醒，实际 {:?}", other),
        }
    }

    #[tokio::test]
    async fn disabled_switch_sends_nothing_and_keeps_timestamp() {
        let pool = test_pool().await;
        let (_, tid) = seed_thesis(&pool, "储能出货将超预期", 3).await;
        seed_due_falsifier(&pool, tid).await;
        Setting::set(&pool, REMIND_SWITCH, "0").await.expect("写设置失败");

        assert_eq!(
            notify_review(&pool).await.expect("执行失败"),
            RemindOutcome::Disabled
        );

        // 关掉开关时**绝不能**写时间戳：否则重新打开后会白等一周
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM thesis_reminders")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "关掉开关不该消耗提醒窗口");
    }

    #[tokio::test]
    async fn task_outcome_reports_reason() {
        let pool = test_pool().await;
        // 无料 → 明确说"无需复盘"，而不是"0 条"
        match run_review_task(&pool).await {
            TaskOutcome::Success(m) => assert_eq!(m, "本期无需复盘"),
            other => panic!("不该失败：{:?}", other),
        }

        let (_, tid) = seed_thesis(&pool, "储能出货将超预期", 3).await;
        seed_due_falsifier(&pool, tid).await;
        match run_review_task(&pool).await {
            TaskOutcome::Success(m) => assert_eq!(m, "已提醒 1 条待复查判断"),
            other => panic!("不该失败：{:?}", other),
        }
    }
}
