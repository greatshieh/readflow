//! 今日待办聚合（跨域只读）
//!
//! # 职责
//! 把散在四五个模块里的「今天该看一眼」的信息汇成一份清单：判据到期的论点、
//! 结构有偏的论点、一周内的财报、刚触发过的价格预警、该重新定夺的覆盖标的。
//! 主屏顶部那条可折叠的「今天要处理」读的就是它。
//!
//! # 为什么单独成模块，而不是塞进 research/
//! 它是**跨域只读聚合**：论点、复盘、行情、覆盖各属不同域。放进任何一个域模块，
//! 都会让那个模块反向依赖其余三个，形成调用环（项目铁律 #9 要求调用链是 DAG）。
//! 独立模块 + 单向依赖（它依赖所有人，没人依赖它）才能保持这张图无环。
//!
//! # 为什么只编排、不写自己的 SQL（最重要的一条）
//! 一旦这里自己写 `WHERE confidence < 40` 之类的条件，就出现了**第二份**
//! 「什么算结构有偏」的定义。两处定义不会同时改：页面改了、这里没改，
//! 用户就会看到「今天要处理说有 3 条，点进去只有 2 条」——这类不一致一旦出现，
//! 用户对整块功能就再也不信了。所以本模块只调用各域已有的 `pub` 函数并排序。
//!
//! # 边界（写在最前，防止后来扩张）
//! - **不做写入**：除「标记已复盘」走复盘自己的命令外，这里不提供任何改数据的动作；
//! - **不做提醒推送**：系统通知各有各的节律（复盘 7 天、预警 120 分钟），
//!   把待办也做成通知会与它们打架，且"打开就看见"已经够用；
//! - **不做历史**：待办只有"现在"这一档。要看过去发生了什么，去看事件时间线。
//!
//! # 时间口径
//! 所有跨天比较都在 SQL 里用 `date('now','localtime')` 完成。
//! 用 `localtime` 而不是 `now`：用户说的"今天"是本地日历上的今天，
//! 而库里的 `CURRENT_TIMESTAMP` 是 UTC。混用会让东八区用户在早上 8 点前
//! 看到「今天的财报」被算成「明天」。

use serde::Serialize;
use sqlx::SqlitePool;

use crate::research::{coverage, review};

/// 一条待办
///
/// `title` 由后端拼好：文案里含动态数字（逾期 3 天、跌破 12.30），
/// 前端拼会与后端各自算一遍口径，迟早对不上。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    /// 类别：thesis_due / review / earnings / alert / coverage_exit
    pub kind: String,
    /// 一句话正文
    pub title: String,
    /// 补充说明（实体名 / 日期 / 阈值），可为空串
    pub detail: String,
    /// 优先级：high / mid / low，决定同类别内的先后
    pub priority: String,
    /// 跳转目标：review / entity / thesis / coverage
    pub target: String,
    /// 跳转目标 ID（target 为 review 时无意义，填 0）
    pub target_id: i64,
    /// 是否逾期（前端标红）
    pub overdue: bool,
    /// 一键处理动作：mark_reviewed / open_thesis / open_entity / open_coverage
    ///
    /// 注意 `open_*` **不等于「已处理」**——它只是把人送到该看的地方，
    /// 条目会继续留着。只有 `mark_reviewed` 会真正把条目移出列表。
    pub action: String,
}

/// 今日汇总
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaySummary {
    /// 已排好序的待办（前端最多展示前 5 条）
    pub items: Vec<TodoItem>,
    /// 各类别计数（收起态摘要用），序列化为 `[{kind, count}]`
    pub counts: Vec<KindCount>,
    /// 总条数
    pub total: i64,
}

/// 某个类别的计数
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindCount {
    pub kind: String,
    pub count: i64,
}

/// 类别固定顺序（同时决定排序与摘要条的展示顺序）
///
/// 顺序按"时效性"排：价格预警最急（盘中就过时），财报次之（有明确日期），
/// 论点判据再次（有窗口但可按天计），结构有偏与覆盖退出最缓（可以拖）。
const KIND_ORDER: [&str; 5] = ["alert", "earnings", "thesis_due", "review", "coverage_exit"];

/// 类别顺序的数值表示（越小越靠前）
fn kind_rank(kind: &str) -> usize {
    KIND_ORDER.iter().position(|k| *k == kind).unwrap_or(usize::MAX)
}

/// 覆盖标的多久没动过就算「该重新定夺」
///
/// 30 天取自"月度复盘"的常识节律：一个标的盯了一个月还没有任何处置动作，
/// 那多半不是没想好，而是已经忘了自己为什么盯它。
const COVERAGE_STALE_DAYS: i64 = 30;

/// 财报只看未来这么多天
const EARNINGS_HORIZON_DAYS: i64 = 7;

/// 价格预警只看最近这么久的触发
///
/// 24 小时：早上打开应用能看到昨晚的触发，再往前就不该再占用"今天"的位置了。
const ALERT_RECENT_HOURS: i64 = 24;

/// 聚合当日待办
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 排好序的汇总；没有任何待办时 `items` 为空、`total` 为 0（前端据此整条不渲染）
///
/// # 错误
/// 任意一个数据源查询失败时返回错误信息。**刻意不做"部分成功"**：
/// 少了任何一类的"今天要处理"都是会误导人的——用户不知道有东西没显示出来，
/// 比整条不显示更危险。
pub async fn today(pool: &SqlitePool) -> Result<TodaySummary, String> {
    let mut items: Vec<TodoItem> = Vec::new();

    // ① 判据到期 + 结构有偏（复用复盘总览，不自己写判定条件）
    push_review_items(pool, &mut items).await?;

    // ② 财报日历（未来 N 天）
    push_earnings_items(pool, &mut items).await?;

    // ③ 刚触发的价格预警
    push_alert_items(pool, &mut items).await?;

    // ④ 长期未处置的覆盖标的
    push_coverage_items(pool, &mut items).await?;

    sort_items(&mut items);

    let counts = count_by_kind(&items);
    let total = items.len() as i64;
    Ok(TodaySummary {
        items,
        counts,
        total,
    })
}

/// 复盘来源的两类待办：判据到期、结构有偏
///
/// # 为什么 high 级健康规则只取一条
/// 同一条论点可能同时命中 H1（久未更新）与 H3（只有支持证据），
/// 全部列出会让一个论点占掉待办列表的三行——用户看到的是「这个论点问题很多」，
/// 而不是「有三件事要做」。
async fn push_review_items(pool: &SqlitePool, out: &mut Vec<TodoItem>) -> Result<(), String> {
    let ov = review::overview(pool).await?;

    // A 区：判据到期（overdue_days 为 Some 即到期，0 = 今天到期）
    for card in &ov.due {
        let overdue_days = card.overdue_days.unwrap_or(0);
        out.push(TodoItem {
            kind: "thesis_due".into(),
            title: format!("判据到期：{}", card.statement),
            detail: if overdue_days > 0 {
                format!("{} · 已逾期 {} 天", card.entity_name, overdue_days)
            } else {
                format!("{} · 今天到期", card.entity_name)
            },
            priority: if overdue_days > 0 { "high" } else { "mid" }.into(),
            target: "thesis".into(),
            target_id: card.thesis_id,
            overdue: overdue_days > 0,
            action: "open_thesis".into(),
        });
    }

    // B 区：结构有偏（只看 high 严重级，且每个论点最多一条）
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for card in &ov.health {
        if !seen.insert(card.thesis_id) {
            continue;
        }
        let Some(hit) = card.hits.iter().find(|h| h.severity == "high") else {
            continue;
        };
        out.push(TodoItem {
            kind: "review".into(),
            title: hit.text.clone(),
            detail: format!("{} · {}", card.entity_name, card.statement),
            priority: "mid".into(),
            target: "thesis".into(),
            target_id: card.thesis_id,
            overdue: false,
            action: "open_thesis".into(),
        });
    }

    Ok(())
}

/// 财报日历：未来 N 天内的披露安排
async fn push_earnings_items(pool: &SqlitePool, out: &mut Vec<TodoItem>) -> Result<(), String> {
    // 只取「还没披露」的（actual_or_forecast = forecast）与今天的；
    // 已披露的不用提醒——那已经是历史，去事件时间线看。
    let rows = sqlx::query_as::<_, (String, i64, String, String, String)>(
        r#"
        SELECT c.symbol, COALESCE(c.entity_id, 0), c.report_date,
               COALESCE(c.report_type, ''), COALESCE(e.name, c.symbol)
        FROM earnings_calendar c
        LEFT JOIN entities e ON e.id = c.entity_id
        WHERE date(c.report_date) >= date('now','localtime')
          AND date(c.report_date) <= date('now','localtime','+' || ? || ' days')
        ORDER BY c.report_date ASC
        LIMIT 20
        "#,
    )
    .bind(EARNINGS_HORIZON_DAYS.to_string())
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    for (symbol, entity_id, report_date, report_type, entity_name) in rows {
        // 天数差：0 = 今天，1 = 明天
        let days_left: i64 = sqlx::query_scalar(
            "SELECT CAST(julianday(?) - julianday(date('now','localtime')) AS INTEGER)",
        )
        .bind(&report_date)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

        let when = match days_left {
            0 => "今天".to_string(),
            1 => "明天".to_string(),
            n => format!("{} 天后", n),
        };
        let kind_label = if report_type.is_empty() {
            "财报".to_string()
        } else {
            report_type.clone()
        };
        out.push(TodoItem {
            kind: "earnings".into(),
            title: format!("{}：{} 披露", entity_name, kind_label),
            detail: format!("{} · {}（{}）", when, report_date, symbol),
            // 今天披露的最高，其余按天数自然靠后（排序里不区分，靠统一排序稳定）
            priority: if days_left == 0 { "high" } else { "mid" }.into(),
            target: "entity".into(),
            target_id: entity_id,
            overdue: false,
            action: "open_entity".into(),
        });
    }

    Ok(())
}

/// 价格预警：最近触发过的规则
///
/// 用 `last_triggered` 而不是实时算价：待办是"打开应用时看到有什么发生过"，
/// 实时检查由定时任务负责，这里再打一次外部接口既慢又会被限流。
async fn push_alert_items(pool: &SqlitePool, out: &mut Vec<TodoItem>) -> Result<(), String> {
    // `ms.entity_id` 在 LEFT JOIN 下可能是 NULL（预警的 symbol 尚未关联到实体），
    // 因此显式声明为 Option<i64>，避免 sqlx 把整行推断成另一种类型组合。
    let rows = sqlx::query_as::<_, (String, String, f64, String, Option<i64>)>(
        r#"
        SELECT a.symbol, a.op, a.price, COALESCE(a.note, ''), ms.entity_id
        FROM market_alerts a
        LEFT JOIN market_symbols ms ON ms.symbol = a.symbol
        WHERE a.enabled = 1
          AND a.last_triggered IS NOT NULL
          AND a.last_triggered >= datetime('now', '-' || ? || ' hours')
        ORDER BY a.last_triggered DESC
        LIMIT 20
        "#,
    )
    .bind(ALERT_RECENT_HOURS.to_string())
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    for (symbol, op, price, note, entity_id) in rows {
        let label = if note.trim().is_empty() {
            format!("{} 价格 {} {}", symbol, op, price)
        } else {
            format!("{}：{} {} {}", note, symbol, op, price)
        };
        out.push(TodoItem {
            kind: "alert".into(),
            title: format!("价格预警触发：{}", label),
            detail: "最近 24 小时内触发".into(),
            priority: "high".into(),
            target: "entity".into(),
            target_id: entity_id.unwrap_or(0),
            overdue: false,
            action: "open_entity".into(),
        });
    }

    Ok(())
}

/// 覆盖清单：标了「本月退出」或长期未处置的标的
///
/// 用 `updated_at` 判"多久没动过"：覆盖条目每次编辑（改档位、改理由）都会刷新它，
/// 所以它是"我最近还想过这个标的吗"的忠实代理。`last_review_at` 更准但只覆盖
/// 走过复盘的标的，多数标的没有值，用它会导致大量标的一律不算陈旧。
async fn push_coverage_items(pool: &SqlitePool, out: &mut Vec<TodoItem>) -> Result<(), String> {
    let rows = coverage::list(pool).await?;
    for row in rows {
        let stale_days: i64 = sqlx::query_scalar(
            "SELECT CAST(julianday('now') - julianday(?) AS INTEGER)",
        )
        .bind(&row.updated_at)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // 「本月退出」档位：无论多久没动都提醒——这是用户自己标的"要退出了"
        // 「核心/观察」档位：超过 STALE 天才提醒
        let is_exit = row.tier == coverage::TIER_EXIT;
        if !is_exit && stale_days < COVERAGE_STALE_DAYS {
            continue;
        }
        let detail = if is_exit {
            format!("标记为「本月退出」· 已有 {} 条判断在跟踪", row.thesis_count)
        } else {
            format!("已 {} 天未更新 · 有 {} 条判断在跟踪", stale_days, row.thesis_count)
        };
        out.push(TodoItem {
            kind: "coverage_exit".into(),
            title: format!("重新定夺：{}", row.entity_name),
            detail,
            priority: "low".into(),
            target: "coverage".into(),
            target_id: row.entity_id,
            overdue: false,
            action: "open_coverage".into(),
        });
    }
    Ok(())
}

/// 排序
///
/// # 比较顺序
/// 1. **逾期优先**（逾期是"已经欠着"，比任何"快要到期"都急）；
/// 2. 优先级 high → mid → low；
/// 3. 类别固定序（见 [`KIND_ORDER`]）；
/// 4. `target_id` 升序。
///
/// 第 4 条不只是为了好看：数字稳定，两次刷新之间不会因为"排序不稳定"
/// 让条目跳来跳去——列表跳动比不排序更让人烦躁，也更容易点错。
fn sort_items(items: &mut [TodoItem]) {
    items.sort_by(|a, b| {
        b.overdue
            .cmp(&a.overdue)
            .then_with(|| rank_priority(&a.priority).cmp(&rank_priority(&b.priority)))
            .then_with(|| kind_rank(&a.kind).cmp(&kind_rank(&b.kind)))
            .then_with(|| a.target_id.cmp(&b.target_id))
    });
}

/// 优先级数值（越小越靠前）；未知优先级排在最后
fn rank_priority(p: &str) -> u8 {
    match p {
        "high" => 0,
        "mid" => 1,
        "low" => 2,
        _ => 3,
    }
}

/// 按类别计数，顺序与 [`KIND_ORDER`] 一致（摘要条按这个顺序显示）
fn count_by_kind(items: &[TodoItem]) -> Vec<KindCount> {
    KIND_ORDER
        .iter()
        .map(|k| KindCount {
            kind: (*k).to_string(),
            count: items.iter().filter(|i| i.kind == *k).count() as i64,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一条待办（测试专用，字段全给默认值）
    fn item(kind: &str, priority: &str, overdue: bool, target_id: i64) -> TodoItem {
        TodoItem {
            kind: kind.into(),
            title: String::new(),
            detail: String::new(),
            priority: priority.into(),
            target: "thesis".into(),
            target_id,
            overdue,
            action: "open_thesis".into(),
        }
    }

    /// 逾期项必须排在任何未逾期项之前，哪怕后者优先级更高
    ///
    /// 逾期是"已经欠着"，high 只是"很急但还没欠"。人的直觉也是先补欠的。
    #[test]
    fn overdue_sorts_before_high_priority() {
        let mut items = vec![
            item("review", "high", false, 1),
            item("thesis_due", "mid", true, 2),
        ];
        sort_items(&mut items);
        assert!(items[0].overdue, "逾期项必须在最前");
        assert_eq!(items[0].target_id, 2);
    }

    /// 同优先级下按类别固定序：alert → earnings → thesis_due → review → coverage_exit
    #[test]
    fn same_priority_follows_kind_order() {
        let mut items = vec![
            item("coverage_exit", "mid", false, 1),
            item("thesis_due", "mid", false, 2),
            item("alert", "mid", false, 3),
            item("earnings", "mid", false, 4),
            item("review", "mid", false, 5),
        ];
        sort_items(&mut items);
        let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec!["alert", "earnings", "thesis_due", "review", "coverage_exit"]
        );
    }

    /// 同分同类别时顺序稳定（按 target_id），两次排序结果一致
    ///
    /// 列表在两次刷新之间跳动会让人点错——这条测试锁住确定性。
    #[test]
    fn same_priority_keeps_stable_order() {
        let build = || {
            vec![
                item("review", "mid", false, 30),
                item("review", "mid", false, 10),
                item("review", "mid", false, 20),
            ]
        };
        let mut a = build();
        sort_items(&mut a);
        let mut b = build();
        sort_items(&mut b);
        assert_eq!(
            a.iter().map(|i| i.target_id).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        assert_eq!(
            a.iter().map(|i| i.target_id).collect::<Vec<_>>(),
            b.iter().map(|i| i.target_id).collect::<Vec<_>>()
        );
    }

    /// 未知优先级排在最后而不是最前
    ///
    /// 若实现写成 `unwrap_or(0)`，一条拼错的优先级会插到最前面，
    /// 把真正紧急的挤下去。
    #[test]
    fn unknown_priority_sinks_to_bottom() {
        assert!(rank_priority("urgent") > rank_priority("low"));
        let mut items = vec![item("review", "urgent", false, 1), item("review", "low", false, 2)];
        sort_items(&mut items);
        assert_eq!(items[0].target_id, 2, "low 应排在未知优先级之前");
    }

    /// 计数顺序固定、无该类别时计 0（前端摘要条按固定顺序渲染）
    #[test]
    fn counts_keep_fixed_order_and_zero_fill() {
        let items = vec![item("review", "mid", false, 1), item("review", "mid", false, 2)];
        let counts = count_by_kind(&items);
        assert_eq!(counts.len(), 5, "五个类别全要出现，没数据补 0");
        assert_eq!(counts[0].kind, "alert");
        assert_eq!(counts[0].count, 0);
        let review = counts.iter().find(|c| c.kind == "review").unwrap();
        assert_eq!(review.count, 2);
    }

    /// 空待办返回 total = 0 而不是 None / 错误
    #[test]
    fn empty_items_yield_total_zero() {
        let counts = count_by_kind(&[]);
        assert_eq!(counts.iter().map(|c| c.count).sum::<i64>(), 0);
    }

    /// 序列化契约（camelCase）
    ///
    /// 前端 `TodaySummary` 手写 `targetId` / `overdue` 等键，
    /// 写错不报错、只表现为 `undefined` 进而让跳转失效。
    #[test]
    fn today_summary_serializes_as_camel_case() {
        let s = TodaySummary {
            items: vec![item("thesis_due", "high", true, 9)],
            counts: vec![KindCount {
                kind: "thesis_due".into(),
                count: 1,
            }],
            total: 1,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["total"], 1);
        assert_eq!(v["items"][0]["targetId"], 9);
        assert_eq!(v["items"][0]["overdue"], true);
        assert_eq!(v["items"][0]["action"], "open_thesis");
        assert_eq!(v["counts"][0]["kind"], "thesis_due");
        assert!(v["items"][0].get("target_id").is_none(), "不应出现 snake_case 键");
    }
}
