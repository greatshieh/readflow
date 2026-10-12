//! 新浪行情源（默认、免 token）
//!
//! # 职责
//! 提供实时快照（`hq.sinajs.cn`），开箱即用、无需任何凭据。价格触发预警也默认走这里。
//!
//! # 实现要点
//! - 需带 `Referer: https://finance.sina.com.cn` 头，否则返回 403。
//! - 返回形如 `var hq_str_sh600519="名称,今开,昨收,现价,最高,最低,竞买,竞卖,成交量(手),成交额,...,日期,时间";`
//!   字段以逗号分隔，关键下标：1=今开 3=现价 4=最高 5=最低 8=成交量(手) 9=成交额 30=日期 31=时间。
//! - 解析失败 / 空响应一律返回 `None`，交由上层渲染占位符（不抛错、不阻塞）。
//!
//! # 约束
//! 复用 [`crate::net::market_client`] 共享客户端（含超时），禁止 `Client::new()`。

use crate::market::Quote;
use crate::net::market_client;

/// 把 Tushare 格式代码（`600519.SH` / `000001.SZ`）转换为新浪代码（`sh600519` / `sz000001`）
fn to_sina_code(symbol: &str) -> String {
    let parts: Vec<&str> = symbol.split('.').collect();
    if parts.len() == 2 {
        format!("{}{}", parts[1].to_ascii_lowercase(), parts[0])
    } else {
        symbol.to_ascii_lowercase()
    }
}

/// 拉取实时快照；任何失败返回 `None`
pub async fn fetch_snapshot(symbol: &str) -> Option<Quote> {
    let code = to_sina_code(symbol);
    let url = format!("https://hq.sinajs.cn/list={}", code);
    let client = market_client();
    let resp = client
        .get(&url)
        .header("Referer", "https://finance.sina.com.cn")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .ok()?;
    let text = resp.text().await.ok()?;
    // 提取首对引号内的主体
    let start = text.find('"')?;
    let end = text.rfind('"')?;
    if end <= start + 1 {
        return None;
    }
    let body = &text[start + 1..end];
    if body.is_empty() || body.contains("无效") || body.contains("已退市") {
        return None;
    }
    let f: Vec<&str> = body.split(',').collect();
    if f.len() < 32 {
        return None;
    }
    let open = f[1].trim().parse().unwrap_or(0.0);
    let close = f[3].trim().parse().unwrap_or(0.0);
    let high = f[4].trim().parse().unwrap_or(0.0);
    let low = f[5].trim().parse().unwrap_or(0.0);
    let vol: f64 = f[8].trim().parse().unwrap_or(0.0);
    let amount: f64 = f[9].trim().parse().unwrap_or(0.0);
    let date = f[30].trim().to_string();
    let time = f.get(31).map(|s| s.trim()).unwrap_or("").to_string();
    let trade_date = if date.is_empty() {
        String::new()
    } else {
        format!("{} {}", date, time)
    };
    Some(Quote {
        symbol: symbol.to_string(),
        trade_date,
        open,
        high,
        low,
        close,
        // 新浪成交量为「手」，近似转「股」便于统一口径（精确值以 Tushare 为准）
        vol: vol * 100.0,
        amount,
        source: "sina".into(),
    })
}
