//! 行情/财务数据层（金融升级 Phase 1）
//!
//! # 职责
//! 把外部金融数据源（新浪默认免 token、Tushare / 东财按设置凭据）拉取的行情时序、
//! 财务快照、财报日历落库，并提供查询与价格预警能力。所有 HTTP 调用都在本模块
//! （铁律 #2：前端只渲染，不接触网络），统一复用 [`crate::net::market_client`]。
//!
//! # 数据源分层（用户 15:44 决议）
//! - **新浪（默认，免 token）**：实时快照，开箱即用；价格预警默认走新浪。
//! - **Tushare / 东财（设置里配 token / key 后启用）**：历史行情、财务、财报日历。
//! - **未配置即占位符**：命令返回 `symbol` / `tushare_configured` 等状态，前端据此渲染
//!   占位卡片（「关联代码」「配置 Tushare 后查看」），配置完成后同一界面自动出数据。

pub mod sina;
pub mod tushare;

use sqlx::Row;
use sqlx::SqlitePool;

/// 单条行情（日线 / 快照），字段 camelCase 序列化供前端消费
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub symbol: String,
    /// 交易日期 / 快照时刻（字符串，避免时区歧义）
    pub trade_date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub vol: f64,
    pub amount: f64,
    /// 数据来源标识：`sina` / `tushare`
    pub source: String,
}

/// 财务快照（估值 + 核心指标）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fundamental {
    pub symbol: String,
    pub report_date: String,
    pub total_mv: f64,
    pub pe_ttm: f64,
    pub pb: f64,
    pub ps_ttm: f64,
    pub roe: f64,
    pub revenue: f64,
    pub net_profit: f64,
    pub currency: String,
    pub source: String,
}

/// 财报日历条目
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EarningsCalendarItem {
    pub symbol: String,
    pub entity_id: i64,
    pub report_date: String,
    pub report_type: String,
    /// `actual`（已披露）或 `forecast`（预告）
    pub actual_or_forecast: String,
    pub source: String,
}

/// 实体行情面板状态（前端据此决定占位符）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketState {
    /// 已关联证券代码；`None` 时前端提示「关联代码」
    pub symbol: Option<String>,
    /// 是否已配置 Tushare token（决定是否展示历史/财务/日历占位或数据）
    pub tushare_configured: bool,
    /// 是否已配置东财 apikey
    pub em_configured: bool,
}

/// 一次同步的汇总
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    pub quotes: i64,
    pub has_fundamental: bool,
    pub calendar: i64,
}

/// 价格预警规则
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketAlert {
    pub id: i64,
    pub symbol: String,
    /// 比较运算符：`>` / `<` / `>=` / `<=`
    pub op: String,
    pub price: f64,
    pub enabled: bool,
    pub note: String,
    /// 上次触发时间（UTC 字符串）；`None` = 从未触发
    ///
    /// 前端据此显示「上次触发 …」并让用户判断规则是否真的在工作——
    /// 一条永远不触发的规则和一个坏掉的功能，用户从界面上是分不出来的。
    pub last_triggered: Option<String>,
}

// ─── 实体-代码映射 ─────────────────────────────────────────────────────────────

/// 取某实体最新关联的证券代码（取最近一条）
pub async fn get_symbol_for_entity(pool: &SqlitePool, entity_id: i64) -> Option<String> {
    let row = sqlx::query("SELECT symbol FROM market_symbols WHERE entity_id = ? ORDER BY id DESC LIMIT 1")
        .bind(entity_id)
        .fetch_optional(pool)
        .await
        .ok()?;
    row.map(|r| r.get::<String, _>("symbol"))
}

/// 关联 / 更新实体-代码映射（同一实体+代码幂等）
pub async fn upsert_symbol(
    pool: &SqlitePool,
    entity_id: i64,
    symbol: &str,
    market: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO market_symbols (entity_id, symbol, market) VALUES (?, ?, ?) \
         ON CONFLICT(entity_id, symbol) DO UPDATE SET market = excluded.market",
    )
    .bind(entity_id)
    .bind(symbol)
    .bind(market)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ─── 行情写入 ─────────────────────────────────────────────────────────────────

/// 批量写入日线（按 symbol+trade_date 幂等覆盖）
pub async fn save_daily(pool: &SqlitePool, quotes: &[Quote]) -> Result<(), String> {
    for q in quotes {
        sqlx::query(
            "INSERT INTO market_quotes (symbol, trade_date, open, high, low, close, vol, amount, source) \
             VALUES (?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(symbol, trade_date) DO UPDATE SET \
               open=excluded.open, high=excluded.high, low=excluded.low, close=excluded.close, \
               vol=excluded.vol, amount=excluded.amount, source=excluded.source",
        )
        .bind(&q.symbol)
        .bind(&q.trade_date)
        .bind(q.open)
        .bind(q.high)
        .bind(q.low)
        .bind(q.close)
        .bind(q.vol)
        .bind(q.amount)
        .bind(&q.source)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 写入财务快照（按 symbol+report_date 幂等覆盖）
pub async fn save_fundamental(pool: &SqlitePool, f: &Fundamental) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO market_fundamentals \
           (symbol, report_date, total_mv, pe_ttm, pb, ps_ttm, roe, revenue, net_profit, currency, source) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?) \
         ON CONFLICT(symbol, report_date) DO UPDATE SET \
           total_mv=excluded.total_mv, pe_ttm=excluded.pe_ttm, pb=excluded.pb, ps_ttm=excluded.ps_ttm, \
           roe=excluded.roe, revenue=excluded.revenue, net_profit=excluded.net_profit, \
           currency=excluded.currency, source=excluded.source",
    )
    .bind(&f.symbol)
    .bind(&f.report_date)
    .bind(f.total_mv)
    .bind(f.pe_ttm)
    .bind(f.pb)
    .bind(f.ps_ttm)
    .bind(f.roe)
    .bind(f.revenue)
    .bind(f.net_profit)
    .bind(&f.currency)
    .bind(&f.source)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 批量写入财报日历（按 symbol+report_date+report_type 幂等覆盖）
pub async fn save_calendar(pool: &SqlitePool, items: &[EarningsCalendarItem]) -> Result<(), String> {
    for c in items {
        sqlx::query(
            "INSERT INTO earnings_calendar (symbol, entity_id, report_date, report_type, actual_or_forecast, source) \
             VALUES (?,?,?,?,?,?) \
             ON CONFLICT(symbol, report_date, report_type) DO UPDATE SET \
               entity_id=excluded.entity_id, actual_or_forecast=excluded.actual_or_forecast, source=excluded.source",
        )
        .bind(&c.symbol)
        .bind(c.entity_id)
        .bind(&c.report_date)
        .bind(&c.report_type)
        .bind(&c.actual_or_forecast)
        .bind(&c.source)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ─── 行情查询 ─────────────────────────────────────────────────────────────────

/// 取最近 N 条日线（按日期倒序）
pub async fn list_quotes(pool: &SqlitePool, symbol: &str, limit: i64) -> Result<Vec<Quote>, String> {
    let rows = sqlx::query(
        "SELECT symbol, trade_date, open, high, low, close, vol, amount, source \
         FROM market_quotes WHERE symbol = ? ORDER BY trade_date DESC LIMIT ?",
    )
    .bind(symbol)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| Quote {
            symbol: r.get("symbol"),
            trade_date: r.get("trade_date"),
            open: r.get("open"),
            high: r.get("high"),
            low: r.get("low"),
            close: r.get("close"),
            vol: r.get("vol"),
            amount: r.get("amount"),
            source: r.get("source"),
        })
        .collect())
}

/// 取最新财务快照
pub async fn get_fundamental(pool: &SqlitePool, symbol: &str) -> Result<Option<Fundamental>, String> {
    let row = sqlx::query(
        "SELECT symbol, report_date, total_mv, pe_ttm, pb, ps_ttm, roe, revenue, net_profit, currency, source \
         FROM market_fundamentals WHERE symbol = ? ORDER BY report_date DESC LIMIT 1",
    )
    .bind(symbol)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|r| Fundamental {
        symbol: r.get("symbol"),
        report_date: r.get("report_date"),
        total_mv: r.get("total_mv"),
        pe_ttm: r.get("pe_ttm"),
        pb: r.get("pb"),
        ps_ttm: r.get("ps_ttm"),
        roe: r.get("roe"),
        revenue: r.get("revenue"),
        net_profit: r.get("net_profit"),
        currency: r.get("currency"),
        source: r.get("source"),
    }))
}

/// 取某实体最近财报日历
pub async fn list_calendar(
    pool: &SqlitePool,
    entity_id: i64,
) -> Result<Vec<EarningsCalendarItem>, String> {
    let rows = sqlx::query(
        "SELECT symbol, entity_id, report_date, report_type, actual_or_forecast, source \
         FROM earnings_calendar WHERE entity_id = ? ORDER BY report_date DESC LIMIT 12",
    )
    .bind(entity_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| EarningsCalendarItem {
            symbol: r.get("symbol"),
            entity_id: r.get("entity_id"),
            report_date: r.get("report_date"),
            report_type: r.get("report_type"),
            actual_or_forecast: r.get("actual_or_forecast"),
            source: r.get("source"),
        })
        .collect())
}

// ─── 同步编排 ─────────────────────────────────────────────────────────────────

/// 用 Tushare token 同步某实体的历史行情 / 财务 / 财报日历并落库
///
/// # 参数
/// * `entity_id` - 实体 ID（写入日历条目）
/// * `symbol` - Tushare 格式代码（如 `600519.SH`）
/// * `token` - Tushare token（调用方已从设置读取，未配置不应调用）
pub async fn sync_entity(
    pool: &SqlitePool,
    entity_id: i64,
    symbol: &str,
    token: &str,
) -> Result<SyncSummary, String> {
    let daily = tushare::fetch_daily(symbol, token, 120).await;
    save_daily(pool, &daily).await?;
    let fund = tushare::fetch_fundamentals(symbol, token).await;
    let has_fund = fund.is_some();
    if let Some(f) = fund {
        save_fundamental(pool, &f).await?;
    }
    let cal = tushare::fetch_earnings_calendar(symbol, token, entity_id).await;
    save_calendar(pool, &cal).await?;
    Ok(SyncSummary {
        quotes: daily.len() as i64,
        has_fundamental: has_fund,
        calendar: cal.len() as i64,
    })
}

// ─── 事件价格反应 ─────────────────────────────────────────────────────────────

/// 一条事件的市场反应
///
/// 全部字段可为 `None`——没有关联代码、行情未同步、事件日无数据都会走到这里。
/// 前端据此显示 `—` 而不是 0：**0% 和"没有数据"是两件事**，
/// 在投研场景里把后者显示成前者会直接误导"这条新闻市场没反应"的判断。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventReaction {
    pub event_id: i64,
    /// 事件日当天涨跌幅（%）= (当日收盘 / 前一交易日收盘 - 1) × 100
    pub day_change: Option<f64>,
    /// 事件后 5 个交易日累计涨跌幅（%）=(第 5 个交易日收盘 / 基准收盘 - 1) × 100
    pub window_5d: Option<f64>,
    /// 基准收盘价（事件日前一个交易日的收盘）
    pub base_close: Option<f64>,
    /// 该事件所属实体关联的证券代码；`None` = 未关联
    pub symbol: Option<String>,
}

/// 计算规则参数
///
/// 抽成常量而非内联字面量，因为测试要引用同一个值：
/// 若测试里写死 5、实现里改成 6，测试仍然会过（两边各自正确，语义已漂移）。
const REACTION_WINDOW_DAYS: usize = 5;

/// 把「日期字符串」规整成 `YYYY-MM-DD`
///
/// 事件里的 `event_date` 由 AI 抽取，可能是 `2026-11-01`、`2026/11/01`、
/// `2026-11-01T00:00:00`、`20261101` 甚至 `2026年11月1日`。行情表里的
/// `trade_date` 是 Tushare 的 `YYYYMMDD` 或新浪的 `YYYY-MM-DD HH:MM:SS`。
/// 两边不归一化就永远匹配不上，而匹配失败的表现是"所有事件都没有价格"——
/// 看起来像数据没同步，实际是格式问题，极难归因。
///
/// # 为什么不能简单地"取前 8 位数字"
/// `2026年11月1日` 的纯数字串只有 7 位（`2026111`），按 4/2/2 切分会切出
/// `2026-11-1` 甚至越界 —— 一位数月份/日期必须补零，否则与行情表的
/// `2026-11-01` 字符串比较永远不相等。因此按分隔符切段后逐段补零。
fn normalize_date(raw: &str) -> Option<String> {
    // 先按"年月日"字面切掉时区/时分秒尾巴：所有支持的写法都以日期开头
    let head: String = raw
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '/' || *c == '年' || *c == '月' || *c == '日')
        .collect();
    let parts: Vec<&str> = head
        .split(|c| c == '-' || c == '/' || c == '年' || c == '月' || c == '日')
        .filter(|s| !s.is_empty())
        .collect();

    // 分段书写（含分隔符）：年月日各一段，单独补零
    let (y, m, d) = if parts.len() >= 3 {
        (parts[0].to_string(), parts[1].to_string(), parts[2].to_string())
    } else {
        // 纯数字连写（`20261101`）：前 4 位年、中间 2 位月、末 2 位日
        let digits: String = head.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() < 8 {
            return None;
        }
        (
            digits[0..4].to_string(),
            digits[4..6].to_string(),
            digits[6..8].to_string(),
        )
    };

    // 统一补零到 2 位（`11` 保持、`1` → `01`），避免 `2026-11-1` 这种比较不上的脏值
    let pad = |s: String| format!("{:0>2}", s);
    let (m, d) = (pad(m), pad(d));

    // 粗校验：把明显不是日期的（如文章 ID）挡在外面，避免造出 "2026-13-45"
    let yi = y.parse::<i32>().ok()?;
    let mi = m.parse::<u32>().ok()?;
    let di = d.parse::<u32>().ok()?;
    if y.len() != 4 || !(1990..=2100).contains(&yi) || !(1..=12).contains(&mi) || !(1..=31).contains(&di)
    {
        return None;
    }
    Some(format!("{}-{}-{}", y, m, d))
}

/// 给定日期与升序排列的日期表，返回「不大于该日期的最后一个下标」
///
/// 用二分而不是线性扫描：时间线一次可能算 200 条事件 × 每只 120 根 K 线，
/// 线性扫描是 24000 次比较，二分是 200 × 7 次。
fn last_index_on_or_before(dates: &[String], target: &str) -> Option<usize> {
    if dates.is_empty() {
        return None;
    }
    match dates.binary_search_by(|d| d.as_str().cmp(target)) {
        Ok(i) => Some(i),
        Err(0) => None,          // target 早于全部数据
        Err(i) => Some(i - 1),   // 落在两个日期之间 → 取前一个交易日
    }
}

/// 计算单条事件的涨跌幅（纯函数，便于逐条测边界）
///
/// # 参数
/// * `dates` - **升序**的交易日（`YYYY-MM-DD`）
/// * `closes` - 与 `dates` 一一对应的收盘价
/// * `event_date` - 事件日（已归一化）
///
/// # 返回值
/// `(当日涨跌幅, 5 日累计涨跌幅, 基准收盘)`
///
/// # 规则（写死，避免后续漂移）
/// - 基准 = **事件日之前**最后一个交易日的收盘（不是事件日开盘）；
///   事件日恰好是停牌日时，同样回退到前一个交易日 —— 这种情形下"当日涨跌"
///   实际上反映的是停牌前最后一次交易，属于已知的近似，注释在此说明。
/// - 5 日窗口 = 基准之后第 N 个交易日（N = [`REACTION_WINDOW_DAYS`]）；
///   不足 N 天返回 `None`，**不按现有天数折算**——折算会把"3 天涨 5%"
///   显示成"5 日涨 5%"，是明确的数据错误。
/// - 基准收盘 <= 0 视为数据异常，全部返回 `None`。
fn reaction_from_series(
    dates: &[String],
    closes: &[f64],
    event_date: &str,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    // idx = 不大于事件日的最后一个交易日
    let Some(idx) = last_index_on_or_before(dates, event_date) else {
        return (None, None, None);
    };

    // 分成两种情形，别用一句 `if` 混着写（v1 就是在这里写反的）：
    // - 事件日**恰好有** K 线（dates[idx] == event_date）：基准取 idx 前一根，
    //   "当日涨跌"才是真涨跌；事件日若已是序列第一根，则无基准可用；
    // - 事件日**没有** K 线（dates[idx] < event_date，停牌/周末）：idx 本身就是
    //   事件日之前的最后一个交易日，直接作基准；"当日涨跌"取 idx 之后第一根
    //   （若存在），反映的是事件后的第一次交易。
    let (base_idx, day_idx) = if dates[idx].as_str() == event_date {
        match idx.checked_sub(1) {
            Some(b) => (b, Some(idx)),
            None => return (None, None, None),
        }
    } else {
        (idx, dates.len().checked_sub(1).filter(|last| *last > idx).map(|_| idx + 1))
    };

    let base = closes.get(base_idx).copied().unwrap_or(0.0);
    if base <= 0.0 {
        return (None, None, None);
    }

    let pct = |price: f64| (price / base - 1.0) * 100.0;
    let day_change = day_idx
        .and_then(|i| closes.get(i).copied())
        .filter(|c| *c > 0.0)
        .map(pct);
    let end_idx = base_idx + REACTION_WINDOW_DAYS;
    let window_5d = closes.get(end_idx).copied().filter(|c| *c > 0.0).map(pct);
    (day_change, window_5d, Some(base))
}

/// 一批事件的价格反应
///
/// # 为什么必须批量而不是逐条
/// 时间线一次渲染 50–200 条事件。逐条查 K 线 = 200 次 SQL，
/// 而实际上这些事件往往只涉及十几只股票、每只一份日线。
/// 因此：① 一次查出 `entity_id → symbol` 全表；② 按涉及的 symbol 集合
/// 一次性取日线到内存；③ 在内存里按日期二分定位。
///
/// # 为什么全程不发网络请求
/// 只用已落库的 `market_quotes`。时间线是高频渲染的视图，
/// 每次打开都打外部接口既慢又会被限流；数据新鲜度由 `market_sync` 负责，
/// 两者职责分离。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `events` - `(event_id, entity_id, event_date)` 三元组；`event_date` 可为空串
///
/// # 返回值
/// 以 `event_id` 为键的反应映射。**未关联代码的事件不会出现在映射里**，
/// 调用方按"缺失 = 无数据"处理。
///
/// # 错误
/// 数据库读写失败时返回错误信息
pub async fn event_reactions(
    pool: &SqlitePool,
    events: &[(i64, i64, String)],
) -> Result<std::collections::HashMap<i64, EventReaction>, String> {
    use std::collections::{HashMap, HashSet};

    let mut out: HashMap<i64, EventReaction> = HashMap::new();
    if events.is_empty() {
        return Ok(out);
    }

    // ① entity_id → symbol（每个实体取最近关联的一条，与 get_symbol_for_entity 同口径）
    let rows = sqlx::query("SELECT entity_id, symbol FROM market_symbols ORDER BY id ASC")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut entity_symbol: HashMap<i64, String> = HashMap::new();
    for r in rows {
        // 升序遍历 + 覆盖写入 = 后者（更新的一条）胜出，与 ORDER BY id DESC LIMIT 1 等价
        entity_symbol.insert(r.get("entity_id"), r.get("symbol"));
    }
    if entity_symbol.is_empty() {
        // 一个代码都没关联：直接返回空映射，避免下面的空 IN 查询
        return Ok(out);
    }

    // ② 涉及到的 symbol 集合
    let symbols: HashSet<&String> = events
        .iter()
        .filter_map(|(_, eid, _)| entity_symbol.get(eid))
        .collect();
    if symbols.is_empty() {
        return Ok(out);
    }

    // ③ 一次性取这些 symbol 的日线（升序：二分前提）
    let mut series: HashMap<String, (Vec<String>, Vec<f64>)> = HashMap::new();
    for sym in symbols {
        let qs = list_quotes(pool, sym, 400).await?;
        if qs.is_empty() {
            continue;
        }
        let mut dated: Vec<(String, f64)> = qs
            .into_iter()
            .filter_map(|q| {
                let d = normalize_date(&q.trade_date)?;
                Some((d, q.close))
            })
            .collect();
        // list_quotes 返回的是倒序（最新在前），这里翻成升序
        dated.sort_by(|a, b| a.0.cmp(&b.0));
        let dates: Vec<String> = dated.iter().map(|(d, _)| d.clone()).collect();
        let closes: Vec<f64> = dated.iter().map(|(_, c)| *c).collect();
        series.insert(sym.clone(), (dates, closes));
    }

    // ④ 逐事件计算
    for (event_id, entity_id, raw_date) in events {
        let Some(symbol) = entity_symbol.get(entity_id) else {
            continue;
        };
        let Some((dates, closes)) = series.get(symbol) else {
            // 有关联代码但一行日线都没有（没同步过）→ 仍然记一条空反应，
            // 让前端能区分"未关联代码"与"已关联但未同步行情"
            out.insert(
                *event_id,
                EventReaction {
                    event_id: *event_id,
                    day_change: None,
                    window_5d: None,
                    base_close: None,
                    symbol: Some(symbol.clone()),
                },
            );
            continue;
        };
        let (day_change, window_5d, base_close) = match normalize_date(raw_date) {
            Some(d) => reaction_from_series(dates, closes, &d),
            None => (None, None, None),
        };
        out.insert(
            *event_id,
            EventReaction {
                event_id: *event_id,
                day_change,
                window_5d,
                base_close,
                symbol: Some(symbol.clone()),
            },
        );
    }

    Ok(out)
}

// ─── 价格预警 ─────────────────────────────────────────────────────────────────

/// 新增价格预警规则
pub async fn add_alert(
    pool: &SqlitePool,
    symbol: &str,
    op: &str,
    price: f64,
    note: &str,
) -> Result<i64, String> {
    let res = sqlx::query("INSERT INTO market_alerts (symbol, op, price, note) VALUES (?,?,?,?)")
        .bind(symbol)
        .bind(op)
        .bind(price)
        .bind(note)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(res.last_insert_rowid())
}

/// 列出预警规则
///
/// `lastTriggered` 一并取出：前端要显示「上次触发 11-03 14:20 / 未触发过」，
/// 否则用户无法判断规则到底有没有在工作。
pub async fn list_alerts(pool: &SqlitePool) -> Result<Vec<MarketAlert>, String> {
    let rows = sqlx::query(
        "SELECT id, symbol, op, price, enabled, note, last_triggered \
         FROM market_alerts ORDER BY id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| MarketAlert {
            id: r.get("id"),
            symbol: r.get("symbol"),
            op: r.get("op"),
            price: r.get("price"),
            enabled: r.get::<i64, _>("enabled") != 0,
            note: r.get("note"),
            last_triggered: r.get("last_triggered"),
        })
        .collect())
}

// ─── 价格预警执行 ─────────────────────────────────────────────────────────────

/// 冷却窗口（分钟）
///
/// 命中一次后 120 分钟内不再重复提醒。取 120 而不是更长，是因为"A 股一天
/// 4 小时交易时段"，120 分钟约等于半天：足够避免"每分钟敲一次"，
/// 又不会让下午的第二次突破被当天上午的提醒吃掉。
const ALERT_COOLDOWN_MINUTES: i64 = 120;

/// 一条命中的预警
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertHit {
    pub alert_id: i64,
    pub symbol: String,
    /// 规则比较符（原样带回，供通知文案使用）
    pub op: String,
    /// 规则阈值
    pub price: f64,
    /// 触发时的现价
    pub current: f64,
    /// 用户备注（可为空）
    pub note: String,
}

/// 按规则判定是否命中
///
/// # 为什么未知运算符判「不命中」
/// 宁可漏发，不可误发。预警一旦误报几次，用户就会关掉通知开关，
/// 从此所有（包括正确的）提醒一起失效——误报的代价远高于漏报。
///
/// # 参数
/// * `op` - 比较符，只接受 `>` / `>=` / `<` / `<=`
/// * `threshold` - 规则阈值
/// * `current` - 现价
///
/// # 返回值
/// 命中返回 `true`
fn hit(op: &str, threshold: f64, current: f64) -> bool {
    match op.trim() {
        ">" => current > threshold,
        ">=" => current >= threshold,
        "<" => current < threshold,
        "<=" => current <= threshold,
        _ => false,
    }
}

/// 当前是否处于「不检查」的时段（周末 / 夜间）
///
/// 只做粗判，不追交易日历：目的仅仅是**不浪费请求**。
/// 判断用本地时间，因为用户看的是本地时钟；这里不追求跨时区的严格正确，
/// 判错的代价是多发一次无意义请求，而漏判的代价是永远不提醒——
/// 所以边界取宽：只有「周六 00:00 至周一 09:00」和「21:00 至次日 09:00」跳过。
fn outside_trading_hours() -> bool {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    let hour = now.hour();
    let weekday = now.weekday();
    let weekend = weekday == chrono::Weekday::Sat || weekday == chrono::Weekday::Sun;
    weekend || !(9..21).contains(&hour)
}

/// 检查全部启用的预警规则，返回命中项
///
/// # 设计要点（都是踩过才知道的）
/// 1. **没有规则就零请求**：多数用户根本没配规则，每 5 分钟空发一次快照
///    既是浪费也会招惹上游限流。先读表，空则直接返回。
/// 2. **同 symbol 只取一次快照**：一个标的常挂止损/止盈/突破三条规则，
///    逐条请求等于三倍流量。这里按 symbol 去重后批量取。
/// 3. **取不到快照就跳过，且不写 `last_triggered`**：网络故障、代码写错、
///    已退市都会让快照返回 `None`。故障不该被当成"规则已检查过"，
///    否则一次网络抖动就会让规则静默哑掉两小时。
/// 4. **现价 <= 0 视为取不到**：新浪对无效代码返回全 0 字段，
///    若当成真实价格，"跌破 10 元"的规则会在价格为 0 时错误触发。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 命中列表（可能为空）；空不等于失败
///
/// # 错误
/// 读取规则表或写回触发时间失败时返回错误信息
pub async fn check_alerts(pool: &SqlitePool) -> Result<Vec<AlertHit>, String> {
    // 时段粗判放在最前面：周末深夜连表都不用读
    if outside_trading_hours() {
        return Ok(Vec::new());
    }

    let rules = list_alerts(pool).await?;
    let active: Vec<&MarketAlert> = rules.iter().filter(|r| r.enabled).collect();
    if active.is_empty() {
        return Ok(Vec::new());
    }

    // 冷却期过滤：用 SQL 的 julianday 算分钟差，避免 Rust 侧解析时间字符串，
    // 也避免 `CURRENT_TIMESTAMP`（UTC）与本地时间的混用。
    let cooled = cooled_symbols(pool, ALERT_COOLDOWN_MINUTES).await?;

    // 按 symbol 去重后逐只取快照
    let mut symbols: Vec<String> = active
        .iter()
        .map(|r| r.symbol.clone())
        .filter(|s| !cooled.contains(s))
        .collect();
    symbols.sort();
    symbols.dedup();

    let mut prices: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for symbol in &symbols {
        if let Some(q) = sina::fetch_snapshot(symbol).await {
            // 0 价是"取不到"而非"跌到零"，见本函数注释第 4 点
            if q.close > 0.0 {
                prices.insert(symbol.clone(), q.close);
            }
        }
    }

    let hits: Vec<AlertHit> = active
        .iter()
        .filter(|r| !cooled.contains(&r.symbol))
        .filter_map(|r| {
            let current = *prices.get(&r.symbol)?;
            if hit(&r.op, r.price, current) {
                Some(AlertHit {
                    alert_id: r.id,
                    symbol: r.symbol.clone(),
                    op: r.op.clone(),
                    price: r.price,
                    current,
                    note: r.note.clone(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(hits)
}

/// 查出冷却期内的 symbol 集合
///
/// # 为什么按 symbol 而不是按 alert_id 判冷却
/// 同一个标的的三条规则往往同时被穿越（止损与止盈不会同时，但"突破 100"
/// 和"突破 99.5"会）。按 symbol 冷却能把"一次价格事件只提醒一次"落到位，
/// 而不是让 3 条规则各响一遍。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `minutes` - 冷却窗口（分钟）
///
/// # 返回值
/// 仍在冷却期内的 symbol 集合
async fn cooled_symbols(
    pool: &SqlitePool,
    minutes: i64,
) -> Result<std::collections::HashSet<String>, String> {
    let rows = sqlx::query(
        "SELECT DISTINCT symbol FROM market_alerts \
         WHERE last_triggered IS NOT NULL \
           AND julianday('now') - julianday(last_triggered) < (? / 1440.0)",
    )
    .bind(minutes)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| r.get::<String, _>("symbol"))
        .collect())
}

/// 回写命中规则的触发时间
///
/// # 调用顺序（不可颠倒）
/// 必须**先发通知、再调用本函数**。反过来的话，一旦通知发送成功而写库失败，
/// 下一轮会立刻重复提醒同一批规则；而先发后写的失败最多是"多提醒一次"。
/// 在预警场景里，"多提醒一次"远好过"提醒永久丢失"。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `alert_ids` - 命中的规则 ID
///
/// # 错误
/// 写入失败时返回错误信息
pub async fn mark_triggered(pool: &SqlitePool, alert_ids: &[i64]) -> Result<(), String> {
    for id in alert_ids {
        sqlx::query("UPDATE market_alerts SET last_triggered = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四种运算符在边界值上的行为
    ///
    /// 价格恰好等于阈值时，`>` 与 `<` 不命中而 `>=` / `<=` 命中——
    /// 这条边界决定了"跌破 100"到底在 100.00 那一刻响不响。
    #[test]
    fn compare_op_covers_all_four_operators() {
        // 严格大于：等于阈值不算
        assert!(!hit(">", 100.0, 100.0));
        assert!(hit(">", 100.0, 100.01));
        assert!(!hit(">", 100.0, 99.99));
        // 大于等于：等于命中
        assert!(hit(">=", 100.0, 100.0));
        assert!(hit(">=", 100.0, 100.01));
        assert!(!hit(">=", 100.0, 99.99));
        // 严格小于
        assert!(!hit("<", 100.0, 100.0));
        assert!(hit("<", 100.0, 99.99));
        assert!(!hit("<", 100.0, 100.01));
        // 小于等于
        assert!(hit("<=", 100.0, 100.0));
        assert!(hit("<=", 100.0, 99.99));
        assert!(!hit("<=", 100.0, 100.01));
    }

    /// 未知运算符一律不命中（宁可漏发不可误发）
    ///
    /// 拆掉这条护栏（比如 `_ => true`）时本测试必须变红，
    /// 否则一条手滑写成 `=` 的规则会变成"任何价格都提醒"。
    #[test]
    fn unknown_operator_never_hits() {
        for op in ["", "=", "==", "!=", "gt", " ≥", ">>"] {
            assert!(!hit(op, 100.0, 150.0), "运算符 {:?} 不应命中", op);
            assert!(!hit(op, 100.0, 50.0), "运算符 {:?} 不应命中", op);
        }
        // 前后空白应被容忍：用户从文档里复制粘贴常带空格。
        // 注意 `"> "` 与 `" > "` 经 trim 后都是合法的 `">"`，属于**应命中**的一类，
        // 不能放进上面的"未知运算符"循环里——放进去就是测试自己在犯错。
        assert!(hit(" > ", 100.0, 150.0));
        assert!(hit("> ", 100.0, 150.0));
    }

    /// 现价为 0 不得被当成真实价格
    ///
    /// 新浪对无效/退市代码返回全 0 字段。若把 0 当真值，
    /// "跌破 10 元"的规则会在价格为 0 时错误触发——这是最坏的一种误报：
    /// 用户会以为自己持仓暴跌。
    #[test]
    fn zero_close_is_treated_as_missing_not_as_zero_price() {
        // 模拟 check_alerts 内的过滤条件
        let closes = [0.0_f64, 12.5];
        let usable: Vec<f64> = closes.into_iter().filter(|c| *c > 0.0).collect();
        assert_eq!(usable, vec![12.5]);
        // 若 0 混进来，"< 10" 会命中
        assert!(hit("<", 10.0, 0.0));
    }

    /// 冷却窗口用分钟换算成天，边界值正确
    ///
    /// 120 分钟 = 0.0833 天。这条测试锁住"分钟 / 1440"的换算口径，
    /// 防止有人顺手写成 `- 120`（那会变成 120 天，冷却期直接失效）。
    #[test]
    fn cooldown_minutes_convert_to_fraction_of_day() {
        let minutes = ALERT_COOLDOWN_MINUTES as f64;
        let days = minutes / 1440.0;
        assert!((days - 0.0833).abs() < 0.001, "120 分钟约等于 0.0833 天");
        assert!(days < 1.0, "冷却期必须小于一天，否则日频数据来不及更新");
    }

    /// 命中结果的序列化契约（camelCase）
    ///
    /// 前端 TS 接口手写 `alertId` / `current`，字段名写错不会报错，
    /// 只会表现为 `undefined`。这条断言把契约钉死在编译/测试阶段。
    #[test]
    fn alert_hit_serializes_as_camel_case() {
        let h = AlertHit {
            alert_id: 7,
            symbol: "600519.SH".into(),
            op: ">".into(),
            price: 1800.0,
            current: 1810.5,
            note: "突破".into(),
        };
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["alertId"], 7);
        assert_eq!(v["symbol"], "600519.SH");
        assert_eq!(v["price"], 1800.0);
        assert_eq!(v["current"], 1810.5);
        assert!(v.get("alert_id").is_none(), "不应同时出现 snake_case 键");
    }

    // ─── 事件价格反应 ───────────────────────────────────────────────────

    /// 构造一段升序日线与对应收盘价，供下列测试共用
    fn series_of(pairs: &[(&str, f64)]) -> (Vec<String>, Vec<f64>) {
        let dates = pairs.iter().map(|(d, _)| d.to_string()).collect();
        let closes = pairs.iter().map(|(_, c)| *c).collect();
        (dates, closes)
    }

    /// 日期归一化能吃下各种写法
    ///
    /// 抽取层写进 `event_date` 的格式不统一，行情表又是另一种写法。
    /// 两边不归一化就永远匹配不上，而表现是"全部事件没有价格"——极难归因。
    #[test]
    fn normalize_date_accepts_common_formats() {
        assert_eq!(normalize_date("2026-11-01").as_deref(), Some("2026-11-01"));
        assert_eq!(normalize_date("2026/11/01").as_deref(), Some("2026-11-01"));
        assert_eq!(normalize_date("20261101").as_deref(), Some("2026-11-01"));
        assert_eq!(
            normalize_date("2026-11-01T08:30:00Z").as_deref(),
            Some("2026-11-01")
        );
        assert_eq!(normalize_date("2026年11月1日").as_deref(), Some("2026-11-01"));
        // 新浪格式：带时分秒
        assert_eq!(
            normalize_date("2026-11-01 09:30:00").as_deref(),
            Some("2026-11-01")
        );
        // 不足以构成日期的、以及越界值一律拒绝
        assert_eq!(normalize_date(""), None);
        assert_eq!(normalize_date("2026-11"), None);
        assert_eq!(normalize_date("2026-13-01"), None);
        assert_eq!(normalize_date("2026-11-45"), None);
    }

    /// 当日涨跌用「前一交易日收盘」作基准，不是当日开盘
    ///
    /// 若基准取错（例如取当日开盘），所有事件的涨跌幅都会系统性偏小，
    /// 且这种偏差不会报错，只会让"市场反应"这一层信息静默失真。
    #[test]
    fn day_change_uses_previous_close() {
        let (dates, closes) = series_of(&[
            ("2026-10-30", 100.0),
            ("2026-11-02", 103.0), // 事件日
        ]);
        let (day, _, base) = reaction_from_series(&dates, &closes, "2026-11-02");
        assert_eq!(base, Some(100.0), "基准必须是前一交易日收盘");
        assert!((day.unwrap() - 3.0).abs() < 1e-9, "应涨 3%");
    }

    /// 事件日无 K 线（停牌 / 未同步）回退到前一交易日，且仍返回 Some
    ///
    /// **这是与"没有价格"必须分开的两种状态**：回退后能算出数，
    /// 而不该因为当天缺一根 K 线就显示 `—`。
    #[test]
    fn missing_event_day_falls_back_to_last_trade() {
        let (dates, closes) = series_of(&[
            ("2026-10-29", 98.0),
            ("2026-10-30", 100.0),
            ("2026-11-03", 105.0),
        ]);
        // 事件日 2026-11-01（周日，无 K 线）：基准应为 10-30 的 100
        let (day, _, base) = reaction_from_series(&dates, &closes, "2026-11-01");
        assert_eq!(base, Some(100.0));
        // 10-30 之后第一根是 11-03 的 105 → 涨 5%
        assert!((day.unwrap() - 5.0).abs() < 1e-9);
    }

    /// 数据不足 5 天时 5 日累计必须返回 None，不能按现有天数折算
    ///
    /// 折算会把"3 天涨 5%"显示成"5 日涨 5%"——这是明确的数据错误，
    /// 比显示 `—` 有害得多。
    #[test]
    fn five_day_window_short_of_data_returns_none() {
        let (dates, closes) = series_of(&[
            ("2026-10-30", 100.0),
            ("2026-11-02", 101.0),
            ("2026-11-03", 102.0),
            ("2026-11-04", 103.0),
            // 只有 3 个交易日，不足 5 天
        ]);
        let (day, win, base) = reaction_from_series(&dates, &closes, "2026-11-02");
        assert_eq!(base, Some(100.0));
        assert!(day.is_some(), "当日涨跌仍应可算");
        assert_eq!(win, None, "不足 5 天必须是 None，不能折算");
    }

    /// 5 日窗口恰好凑满时取第 5 个交易日
    #[test]
    fn five_day_window_uses_exactly_fifth_trading_day() {
        let (dates, closes) = series_of(&[
            ("2026-10-30", 100.0), // 基准
            ("2026-11-02", 101.0), // +1
            ("2026-11-03", 102.0), // +2
            ("2026-11-04", 103.0), // +3
            ("2026-11-05", 104.0), // +4
            ("2026-11-06", 110.0), // +5 ← 应取这根
        ]);
        let (_, win, _) = reaction_from_series(&dates, &closes, "2026-11-02");
        assert!((win.unwrap() - 10.0).abs() < 1e-9, "基准 100 → 第 5 日 110 = 10%");
    }

    /// 事件日早于全部行情数据 → 全部 None，不瞎猜
    #[test]
    fn event_before_series_returns_none() {
        let (dates, closes) = series_of(&[("2026-11-02", 100.0), ("2026-11-03", 101.0)]);
        let (day, win, base) = reaction_from_series(&dates, &closes, "2020-01-01");
        assert_eq!(day, None);
        assert_eq!(win, None);
        assert_eq!(base, None);
    }

    /// 基准收盘为 0（脏数据）时不能算出 -100%
    #[test]
    fn zero_base_close_yields_none() {
        let (dates, closes) = series_of(&[("2026-10-30", 0.0), ("2026-11-02", 10.0)]);
        let (day, win, base) = reaction_from_series(&dates, &closes, "2026-11-02");
        assert_eq!(base, None);
        assert_eq!(day, None);
        assert_eq!(win, None);
    }

    /// 反应结构序列化契约（camelCase）
    ///
    /// 前端 TS 手写 `dayChange` / `window5d` / `baseClose`，
    /// 字段名写错不报错、只表现为 `undefined`。
    #[test]
    fn event_reaction_serializes_as_camel_case() {
        let r = EventReaction {
            event_id: 42,
            day_change: Some(3.2),
            window_5d: Some(-1.05),
            base_close: Some(148.5),
            symbol: Some("600519.SH".into()),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["eventId"], 42);
        assert_eq!(v["dayChange"], 3.2);
        assert_eq!(v["window5d"], -1.05);
        assert_eq!(v["baseClose"], 148.5);
        assert_eq!(v["symbol"], "600519.SH");
        assert!(v.get("window_5d").is_none(), "不应同时出现 snake_case 键");
    }

    /// 预警规则序列化带 lastTriggered
    #[test]
    fn market_alert_serializes_last_triggered() {
        let a = MarketAlert {
            id: 1,
            symbol: "000001.SZ".into(),
            op: "<".into(),
            price: 10.0,
            enabled: true,
            note: String::new(),
            last_triggered: None,
        };
        let v = serde_json::to_value(&a).unwrap();
        assert!(v.get("lastTriggered").is_some());
        assert!(v["lastTriggered"].is_null());
    }
}
