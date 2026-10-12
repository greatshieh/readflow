//! Tushare 行情源（设置配置 token 后启用）
//!
//! # 职责
//! 提供历史日线、财务快照（估值）、财报日历等深度数据。需用户在「设置 → 数据源」配置
//! Tushare token；未配置时上层不应调用本模块（前端渲染占位符）。
//!
//! # 实现要点
//! - 端点 `https://api.tushare.pro`，POST `{api_name, token, params, fields}`，返回
//!   `{code, msg, data:{fields:[...], items:[[...],...]}}`。
//! - 用 [`serde_json::Value`] 灵活解析 `data.items`，避免字段顺序/缺字段导致结构不匹配。
//! - 任何失败返回 `None` / 空向量，交由上层渲染占位符。
//!
//! # 约束
//! 复用 [`crate::net::market_client`] 共享客户端；token 由调用方从设置读取后传入，不落日志。

use crate::market::{EarningsCalendarItem, Fundamental, Quote};
use crate::net::market_client;
use serde_json::json;

const TUSHARE_API: &str = "https://api.tushare.pro";

/// 通用 Tushare POST；成功返回 `data` 节点，否则 `None`
async fn call(api_name: &str, token: &str, params: serde_json::Value, fields: &str) -> Option<serde_json::Value> {
    let payload = json!({
        "api_name": api_name,
        "token": token,
        "params": params,
        "fields": fields,
    });
    let client = market_client();
    let resp = client.post(TUSHARE_API).json(&payload).send().await.ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    // code != 0 表示接口报错（如 token 无效 / 权限不足 / 频率限制）
    if v.get("code").and_then(|c| c.as_i64()).unwrap_or(0) != 0 {
        eprintln!("Tushare[{}] 返回错误: {:?}", api_name, v.get("msg"));
        return None;
    }
    v.get("data").cloned()
}

/// 拉取日线（最近 `limit` 条，按日期升序）
pub async fn fetch_daily(symbol: &str, token: &str, limit: usize) -> Vec<Quote> {
    let params = json!({ "ts_code": symbol });
    let fields = "trade_date,open,high,low,close,vol,amount";
    let data = match call("daily", token, params, fields).await {
        Some(d) => d,
        None => return vec![],
    };
    let items = match data.get("items").and_then(|x| x.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    let start = items.len().saturating_sub(limit);
    let mut out = Vec::with_capacity(items.len() - start);
    for it in &items[start..] {
        let row = match it.as_array() {
            Some(a) => a,
            None => continue,
        };
        let trade_date = row.get(0).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let open = row.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let high = row.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let low = row.get(3).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let close = row.get(4).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let vol = row.get(5).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let amount = row.get(6).and_then(|x| x.as_f64()).unwrap_or(0.0);
        out.push(Quote {
            symbol: symbol.to_string(),
            trade_date,
            open,
            high,
            low,
            close,
            vol,
            amount,
            source: "tushare".into(),
        });
    }
    out
}

/// 拉取最新财务快照（估值口径：daily_basic 的 pe_ttm/pb/ps_ttm/total_mv）
pub async fn fetch_fundamentals(symbol: &str, token: &str) -> Option<Fundamental> {
    let params = json!({ "ts_code": symbol });
    let fields = "trade_date,close,pe_ttm,pb,ps_ttm,total_mv";
    let data = call("daily_basic", token, params, fields).await?;
    let items = data.get("items").and_then(|x| x.as_array())?;
    let last = items.last()?;
    let row = last.as_array()?;
    let report_date = row.get(0).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let pe_ttm = row.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let pb = row.get(3).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let ps_ttm = row.get(4).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let total_mv = row.get(5).and_then(|x| x.as_f64()).unwrap_or(0.0);
    Some(Fundamental {
        symbol: symbol.to_string(),
        report_date,
        total_mv,
        pe_ttm,
        pb,
        ps_ttm,
        // 以下字段需 fina_indicator 等额外接口，首版留空（schema 已预留），后续可补
        roe: 0.0,
        revenue: 0.0,
        net_profit: 0.0,
        currency: "CNY".into(),
        source: "tushare".into(),
    })
}

/// 拉取财报日历（用 fina_indicator 的 end_date / ann_date 近似）
pub async fn fetch_earnings_calendar(symbol: &str, token: &str, entity_id: i64) -> Vec<EarningsCalendarItem> {
    let params = json!({ "ts_code": symbol });
    let fields = "ann_date,end_date,end_type";
    let data = match call("fina_indicator", token, params, fields).await {
        Some(d) => d,
        None => return vec![],
    };
    let items = match data.get("items").and_then(|x| x.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    // 取最近 8 期（items 按 end_date 升序，截末尾）
    let start = items.len().saturating_sub(8);
    let mut out = Vec::with_capacity(items.len() - start);
    for it in &items[start..] {
        let row = match it.as_array() {
            Some(a) => a,
            None => continue,
        };
        let ann = row.get(0).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let end = row.get(1).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let end_type = row.get(2).and_then(|x| x.as_str()).unwrap_or("").to_string();
        out.push(EarningsCalendarItem {
            symbol: symbol.to_string(),
            entity_id,
            // 优先用实际披露日，缺失则退回报告期
            report_date: if ann.is_empty() { end.clone() } else { ann },
            report_type: end_type,
            actual_or_forecast: "actual".into(),
            source: "tushare".into(),
        });
    }
    out
}
