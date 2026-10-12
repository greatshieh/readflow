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
pub async fn list_alerts(
    pool: &SqlitePool,
    only_enabled: bool,
) -> Result<Vec<MarketAlert>, String> {
    let sql = if only_enabled {
        "SELECT id, symbol, op, price, enabled, note FROM market_alerts WHERE enabled = 1 ORDER BY id DESC"
    } else {
        "SELECT id, symbol, op, price, enabled, note FROM market_alerts ORDER BY id DESC"
    };
    let rows = sqlx::query(sql)
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
        })
        .collect())
}

/// 命中后更新最近触发时间
async fn mark_alert_triggered(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("UPDATE market_alerts SET last_triggered = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 遍历启用中的预警规则：拉新浪实时快照比对，命中即推送系统通知
///
/// 返回本次触发的规则数。单条规则失败（网络/解析）仅 `eprintln!` 跳过，不中断整体。
pub async fn check_alerts(pool: &SqlitePool) -> Result<usize, String> {
    let alerts = list_alerts(pool, true).await?;
    let mut fired = 0;
    for a in alerts {
        match sina::fetch_snapshot(&a.symbol).await {
            Some(q) => {
                let hit = match a.op.as_str() {
                    ">" => q.close > a.price,
                    "<" => q.close < a.price,
                    ">=" => q.close >= a.price,
                    "<=" => q.close <= a.price,
                    _ => false,
                };
                if hit {
                    crate::notify::notify_price_alert(pool, &a.symbol, q.close, &a.op, a.price).await;
                    let _ = mark_alert_triggered(pool, a.id).await;
                    fired += 1;
                }
            }
            None => eprintln!("价格预警：{} 快照拉取失败，跳过", a.symbol),
        }
    }
    Ok(fired)
}
