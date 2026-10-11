//! 系统通知（应用在后台时把耗时操作的结果推给系统通知中心）
//!
//! # 职责
//! 在两个"用户没盯着窗口、结果却已经出来"的场景发送桌面通知：
//! - 订阅刷新完成且发现新文章（[`crate::scheduler::refresh_all_feeds`]）
//! - 自动化任务（批量提取 / 日报 / 周报）执行结束（[`crate::research::run_task_once`]）
//!
//! # 设计意图
//! - **不打扰正在阅读的用户**：三重门（句柄已注入 + 设置开关开启 + 窗口不可见或失焦）
//!   全部满足才发送。用户盯着窗口时界面本身已有进度条与结果提示，再弹系统通知是噪音；
//!   只有"窗口被隐藏到托盘""切到别的应用去了"这两种情况才值得提醒。
//! - **沿用既有解法解决 cron 困境**：cron 回调拿不到 `AppHandle`，
//!   与 [`crate::research::report::set_reports_dir`] 面对的是同一个问题，
//!   因此照同样思路在启动阶段注入全局句柄，而不是给每个管理器穿参。
//! - **失败绝不外溢**：无桌面环境、无通知守护（D-Bus 不可用）、用户未授权都可能让
//!   发送失败。这属于锦上添花的能力，只记日志，不能影响刷新或任务本身。
//! - **缺省开启**：设置项 `notify_enabled` 只在显式写入 `"0"` 时关闭；
//!   键不存在或为空串都按开启处理。
//! - **仅 Rust 侧调用**：与 dialog 插件同样不在 capabilities 里授权，
//!   WebView 无法自行弹出系统通知。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

use crate::db::{Entity, Setting};
use crate::research::extract::is_significant;
use crate::research::ExtractedEvent;

/// 全局应用句柄（启动阶段注入一次，之后只读）
///
/// 用 `OnceLock` 而非 `Mutex<Option<_>>`：句柄只在启动时写入一次、运行期纯读，
/// 无锁读取比互斥量更合适。`AppHandle` 本身是 `Send + Sync + Clone` 的廉价句柄。
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// 注入全局应用句柄（`lib.rs` 的 `setup` 中调用一次）
///
/// 重复调用只保留第一次的值：应用只有一个主窗口，句柄天然唯一。
///
/// # 参数
/// * `handle` - 应用句柄（`AppHandle` 的克隆）
pub fn set_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

/// 判断用户是否"不在看"这个应用（窗口不存在、不可见、或已失焦）
///
/// 判定取向是**宁可多发一条也不漏掉**：只有能明确判定"可见且聚焦"时才抑制通知。
/// 查询失败（某些窗口管理器不回报焦点状态）一律按"不在看"处理——
/// 漏掉一条该发的通知会让用户以为功能坏了，多发一条只是多看一眼。
///
/// # 参数
/// * `app` - 应用句柄
///
/// # 返回值
/// 窗口不可见、已失焦或不存在时返回 `true`
fn window_away(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        // 单窗口应用理论上不会走到这里；窗口已销毁时发通知也无害
        return true;
    };
    if !window.is_visible().unwrap_or(false) {
        return true;
    }
    !window.is_focused().unwrap_or(false)
}

/// 读取「系统通知」开关（缺省开启）
///
/// 只有值为 `"0"` 才算关闭：`Setting::get` 对空串返回 `Some("")`，
/// 所以判据写成"值不等于 0"而非"值等于 1"，避免历史脏数据把功能意外关掉。
/// 读库失败时按开启处理——一次查询失败不该让通知永久静默。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 开关开启返回 `true`
async fn enabled(pool: &SqlitePool) -> bool {
    match Setting::get(pool, "notify_enabled").await {
        Ok(Some(value)) => value.trim() != "0",
        _ => true,
    }
}

/// 在满足全部发送条件时弹一条系统通知
///
/// 三重门按代价从低到高依次判定：句柄是否已注入（内存读）→ 窗口是否离开（窗口 API）
/// → 设置开关是否开启（查库）。任一不满足即静默返回，调用方无需关心是否真的发出去了。
///
/// # 参数
/// * `pool` - 数据库连接池（读设置开关）
/// * `title` - 通知标题
/// * `body` - 通知正文
pub async fn send(pool: &SqlitePool, title: &str, body: &str) {
    // 启动早期（数据库初始化完成前）句柄尚未注入，任务不可能已运行，直接返回
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    if !window_away(app) {
        return;
    }
    if !enabled(pool).await {
        return;
    }

    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("发送系统通知失败（不影响主流程）: {}", e);
    }
}

/// 发送一条自动化任务的执行结果通知
///
/// 把"成功 / 失败"映射成标题措辞，正文直接用执行器给出的人类可读描述
/// （成功时为结果摘要，失败时为原因），避免在调用处重复拼串。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `task_name` - 任务名称（用户自定义，直接展示）
/// * `succeeded` - 是否执行成功
/// * `message` - 结果描述或失败原因
pub async fn task_result(pool: &SqlitePool, task_name: &str, succeeded: bool, message: &str) {
    let title = if succeeded {
        format!("任务完成：{}", task_name)
    } else {
        format!("任务失败：{}", task_name)
    };
    send(pool, &title, message).await;
}

/// 读取「事件预警」开关（缺省开启）
///
/// 与 [`enabled`]（`notify_enabled`）同一判据：只有值为 `"0"` 才算关闭，
/// 键不存在 / 空串 / 其它值都按开启处理，避免历史脏数据把功能误关。
/// 事件预警依赖系统通知总开关，但两者独立可控：用户可关掉全部系统通知，
/// 也可只关事件预警而保留刷新 / 任务结束通知。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 开关开启返回 `true`
async fn event_alert_enabled(pool: &SqlitePool) -> bool {
    match Setting::get(pool, "event_alert_enabled").await {
        Ok(Some(value)) => value.trim() != "0",
        _ => true,
    }
}

/// 每个实体的最近一次预警时刻（进程内限频）
///
/// 同一实体在 [`ALERT_COOLDOWN`] 内重复命中的事件只推一次，
/// 避免一轮批量抽取里同一实体的多条强情感事件或连续抽取反复刷屏。
/// 仅在进程存活期内有效（应用重启后重新计时）——事件预警是"提醒"而非"审计"，
/// 重启后重新计时是可接受的取舍，也避免为此改动数据库 schema。
static LAST_ALERT_AT: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

/// 同一实体的限频窗口（30 分钟）
const ALERT_COOLDOWN: Duration = Duration::from_secs(30 * 60);

/// 抽取发现重大事件时主动推桌面通知（P0-D 事件预警）
///
/// 与 [`send`] 共用三重门（句柄已注入 + 窗口离开 + 系统通知总开关），
/// 再叠加独立的「事件预警」子开关：总开关关或子开关关都不发送。
/// `events` 是批量抽取回传的全部事件，本函数内部只筛出强情感（positive/negative
/// 且置信度 >= 0.7）事件推送，事件按实体聚合后同一实体限频窗口内只推一次。
///
/// # 参数
/// * `pool` - 数据库连接池（读子开关）
/// * `events` - 本批抽取出的全部事件（内部按强情感门槛筛选）
pub async fn send_event_alert(pool: &SqlitePool, events: &[ExtractedEvent]) {
    if events.is_empty() {
        return;
    }
    // 仅保留强情感事件（P0-D 门槛）：`extract_batch` 已回传全部事件，
    // 这里按情感维度二道筛，避免把中性或模型拿不准的事件当成"重大事件"推送
    let sig: Vec<&ExtractedEvent> = events.iter().filter(|e| is_significant(e)).collect();
    if sig.is_empty() {
        return;
    }
    // 启动早期句柄尚未注入，任务不可能已运行，直接返回
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    if !window_away(app) {
        return;
    }
    if !enabled(pool).await {
        return;
    }
    if !event_alert_enabled(pool).await {
        return;
    }

    // 按实体聚合：同一实体多条事件合并到一条通知，减少打扰；
    // 同时过滤处于限频窗口内的实体（避免重复刷屏）
    let rate_map = LAST_ALERT_AT.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = rate_map.lock().await;
    let now = Instant::now();

    let mut by_entity: HashMap<&str, Vec<&ExtractedEvent>> = HashMap::new();
    for e in &sig {
        let name = e.canonical_name.trim();
        if let Some(last) = guard.get(name) {
            if now.saturating_duration_since(*last) < ALERT_COOLDOWN {
                continue; // 限频窗口内，跳过本实体
            }
        }
        by_entity.entry(name).or_default().push(e);
    }
    if by_entity.is_empty() {
        return;
    }

    // 刷新限频时间戳（只更新本次实际推送的实体）
    for name in by_entity.keys() {
        guard.insert(name.to_string(), now);
    }
    drop(guard);

    // 组装通知正文：每条实体「【实体名】发生新事件：事实1；事实2」
    let mut bodies: Vec<String> = Vec::new();
    for (name, evs) in &by_entity {
        let facts: Vec<&str> = evs
            .iter()
            .map(|e| e.fact.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if facts.is_empty() {
            continue;
        }
        bodies.push(format!("【{}】发生新事件：{}", name, facts.join("；")));
    }
    if bodies.is_empty() {
        return;
    }

    let title = if bodies.len() == 1 {
        "ReadFlow · 事件预警".to_string()
    } else {
        format!("ReadFlow · 事件预警（{} 个实体）", bodies.len())
    };
    let body = bodies.join("\n");

    if let Err(e) = app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
    {
        eprintln!("发送事件预警通知失败（不影响主流程）: {}", e);
    }
}

/// 实体告警规则（解析自 `entities.alert_rules` 列的 JSON）
///
/// 各字段为空集合时表示"该维度不限制"：例如 `event_types` 为空表示任何类型都命中，
/// `sentiments` 为空表示任何情感都命中，`keywords` 为空表示不按关键词过滤。
/// `enabled` 缺省为 `true`（只要用户配置了规则即视为启用）。
#[derive(Debug, Clone, serde::Deserialize)]
struct AlertRule {
    /// 是否启用该规则（缺省启用）
    #[serde(default = "default_true")]
    enabled: bool,
    /// 命中的事件类型白名单（空 = 不限类型）
    #[serde(default)]
    event_types: Vec<String>,
    /// 命中的情感白名单（空 = 不限情感）
    #[serde(default)]
    sentiments: Vec<String>,
    /// 命中关键词（事实/证据里出现其一即命中；空 = 不按关键词过滤）
    #[serde(default)]
    keywords: Vec<String>,
}

/// `enabled` 字段缺省为真：用户一旦写下规则即希望生效，避免"配了却静默不提醒"
fn default_true() -> bool {
    true
}

/// 解析实体告警规则 JSON
///
/// 返回 `None` 表示无规则 / 非法 JSON（无规则自然不触发告警）。
///
/// # 参数
/// * `json` - `entities.alert_rules` 列的字符串
///
/// # 返回值
/// 解析成功返回规则，否则 `None`
fn parse_alert_rule(json: &str) -> Option<AlertRule> {
    serde_json::from_str::<AlertRule>(json).ok()
}

/// 判断单条事件是否命中某实体的告警规则
///
/// 命中条件：规则启用，且（若规则限定了类型则事件类型须在内）、（若限定了情感则
/// 事件情感须在内）、（若限定了关键词则事实或证据须含其一）。任一维度空集合则不限制。
///
/// # 参数
/// * `rule` - 实体告警规则
/// * `e` - 待判定的抽取事件
///
/// # 返回值
/// 命中返回 `true`
fn rule_matches(rule: &AlertRule, e: &ExtractedEvent) -> bool {
    if !rule.enabled {
        return false;
    }
    if !rule.event_types.is_empty() && !rule.event_types.iter().any(|t| t == &e.event_type) {
        return false;
    }
    if !rule.sentiments.is_empty() {
        match &e.sentiment {
            Some(s) if rule.sentiments.iter().any(|x| x == s) => {}
            _ => return false,
        }
    }
    if !rule.keywords.is_empty() {
        let hay = format!("{} {}", e.fact, e.evidence);
        if !rule.keywords.iter().any(|k| hay.contains(k.as_str())) {
            return false;
        }
    }
    true
}

/// 每个实体的最近一次「规则告警」时刻（进程内限频，独立于 P0-D 强情感预警）
///
/// 与 [`LAST_ALERT_AT`] 分开：一条事件可能同时触发强情感预警与规则告警，
/// 两条通知应各自独立限频，避免互相吞掉。
static LAST_RULE_ALERT_AT: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

/// 抽取命中实体告警规则时主动推桌面通知（P1-F 实体级告警）
///
/// 与 [`send_event_alert`]（P0-D）共用三重门 + 「事件预警」子开关，再按各实体的
/// `alert_rules` 规则匹配。规则读取按实体名查库，并在本次调用内做一层内存缓存，
/// 避免同批事件对同一实体反复查库；命中事件按实体聚合、独立限频后推送。
///
/// # 参数
/// * `pool` - 数据库连接池（读子开关 + 实体规则）
/// * `events` - 本批抽取出的全部事件
pub async fn send_rule_alerts(pool: &SqlitePool, events: &[ExtractedEvent]) {
    if events.is_empty() {
        return;
    }
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    if !window_away(app) {
        return;
    }
    if !enabled(pool).await {
        return;
    }
    if !event_alert_enabled(pool).await {
        return;
    }

    // 按实体名查规则，调用内缓存避免同批重复查库
    let mut rule_cache: HashMap<String, Option<AlertRule>> = HashMap::new();
    let mut matched: Vec<(String, &ExtractedEvent)> = Vec::new();
    for e in events {
        let name = e.canonical_name.trim().to_string();
        let rule = match rule_cache.get(&name) {
            Some(r) => r.clone(),
            None => {
                let r = Entity::get_alert_rules_by_name(pool, &name)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|json| parse_alert_rule(&json));
                rule_cache.insert(name.clone(), r.clone());
                r
            }
        };
        if let Some(rule) = rule {
            if rule_matches(&rule, e) {
                matched.push((name, e));
            }
        }
    }
    if matched.is_empty() {
        return;
    }

    // 按实体聚合 + 独立限频（key 为实体名，与 P0-D 的强情感预警限频互不干扰）
    let rate_map = LAST_RULE_ALERT_AT.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = rate_map.lock().await;
    let now = Instant::now();

    let mut by_entity: HashMap<String, Vec<&ExtractedEvent>> = HashMap::new();
    for (name, e) in &matched {
        if let Some(last) = guard.get(name) {
            if now.saturating_duration_since(*last) < ALERT_COOLDOWN {
                continue;
            }
        }
        by_entity.entry(name.clone()).or_default().push(*e);
    }
    if by_entity.is_empty() {
        return;
    }
    for name in by_entity.keys() {
        guard.insert(name.clone(), now);
    }
    drop(guard);

    let mut bodies: Vec<String> = Vec::new();
    for (name, evs) in &by_entity {
        let facts: Vec<&str> = evs
            .iter()
            .map(|e| e.fact.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if facts.is_empty() {
            continue;
        }
        bodies.push(format!("【{}】触发你的告警规则：{}", name, facts.join("；")));
    }
    if bodies.is_empty() {
        return;
    }

    let title = if bodies.len() == 1 {
        "ReadFlow · 实体告警".to_string()
    } else {
        format!("ReadFlow · 实体告警（{} 个实体）", bodies.len())
    };
    let body = bodies.join("\n");

    if let Err(e) = app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
    {
        eprintln!("发送实体告警通知失败（不影响主流程）: {}", e);
    }
}

#[cfg(test)]
mod tests {
    //! 「系统通知」开关读取的验证
    //!
    //! 只测纯逻辑（[`enabled`]）：真正弹通知依赖桌面环境与 D-Bus 通知守护，
    //! 在无头环境里必然失败，不属于可自动化的部分。

    use super::*;
    use crate::db::create_tables;

    /// 建内存库（复用正式建表函数，settings 表结构与正式库一致）
    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("建内存库失败");
        create_tables(&pool).await.expect("建表失败");
        pool
    }

    /// 未写入设置项时视为开启：缺省开启是产品决定，不是意外
    #[tokio::test]
    async fn enabled_defaults_to_on_when_unset() {
        let pool = setup().await;
        assert!(enabled(&pool).await);
    }

    /// 显式写 "0" 才关闭
    #[tokio::test]
    async fn enabled_respects_explicit_zero() {
        let pool = setup().await;
        Setting::set(&pool, "notify_enabled", "0")
            .await
            .expect("写入设置失败");
        assert!(!enabled(&pool).await);
    }

    /// 空串与其它值都算开启
    ///
    /// `Setting::get` 对空串返回 `Some("")` 而非 `None`——若判据写成"值等于 1"，
    /// 新建库里的空串就会被误判成关闭，功能看起来像"没做"。此处把这个边界钉住。
    #[tokio::test]
    async fn enabled_treats_empty_and_other_values_as_on() {
        let pool = setup().await;
        for value in ["", "1", "true"] {
            Setting::set(&pool, "notify_enabled", value)
                .await
                .expect("写入设置失败");
            assert!(enabled(&pool).await, "值 {:?} 应视为开启", value);
        }
    }

    /// 事件预警子开关：未写入设置项时视为开启（默认开）
    #[tokio::test]
    async fn event_alert_enabled_defaults_to_on_when_unset() {
        let pool = setup().await;
        assert!(event_alert_enabled(&pool).await);
    }

    /// 事件预警子开关：显式写 "0" 才关闭
    #[tokio::test]
    async fn event_alert_enabled_respects_explicit_zero() {
        let pool = setup().await;
        Setting::set(&pool, "event_alert_enabled", "0")
            .await
            .expect("写入设置失败");
        assert!(!event_alert_enabled(&pool).await);
    }

    // ─── 实体级告警规则匹配（P1-F）纯函数验证 ───────────────────────────────

    /// 测试用事件构造器
    fn make_rule_event(event_type: &str, sentiment: Option<&str>, fact: &str) -> ExtractedEvent {
        ExtractedEvent {
            canonical_name: "测试实体".to_string(),
            aliases: vec![],
            entity_type: "company".to_string(),
            event_type: event_type.to_string(),
            event_date: String::new(),
            fact: fact.to_string(),
            evidence: String::new(),
            sentiment: sentiment.map(|s| s.to_string()),
            sentiment_confidence: Some(0.9),
            financial_metrics: None,
        }
    }

    /// 规则未启用时不命中（哪怕其它维度全空）
    #[test]
    fn rule_matches_skips_disabled() {
        let rule = super::AlertRule {
            enabled: false,
            event_types: vec![],
            sentiments: vec![],
            keywords: vec![],
        };
        assert!(!super::rule_matches(&rule, &make_rule_event("ma", None, "")));
    }

    /// 全维度空集合 = 放行任何事件（规则缺省语义）
    #[test]
    fn rule_matches_empty_dimensions_pass_all() {
        let rule = super::AlertRule {
            enabled: true,
            event_types: vec![],
            sentiments: vec![],
            keywords: vec![],
        };
        assert!(super::rule_matches(&rule, &make_rule_event("ma", Some("neutral"), "")));
    }

    /// 事件类型维度过滤
    #[test]
    fn rule_matches_event_type_filter() {
        let rule = super::AlertRule {
            enabled: true,
            event_types: vec!["ma".to_string()],
            sentiments: vec![],
            keywords: vec![],
        };
        assert!(super::rule_matches(&rule, &make_rule_event("ma", None, "")));
        assert!(!super::rule_matches(&rule, &make_rule_event("product", None, "")));
    }

    /// 情感维度过滤；事件情感缺失且规则限定情感则不命中
    #[test]
    fn rule_matches_sentiment_filter() {
        let rule = super::AlertRule {
            enabled: true,
            event_types: vec![],
            sentiments: vec!["negative".to_string()],
            keywords: vec![],
        };
        assert!(super::rule_matches(&rule, &make_rule_event("ma", Some("negative"), "")));
        assert!(!super::rule_matches(&rule, &make_rule_event("ma", Some("positive"), "")));
        assert!(!super::rule_matches(&rule, &make_rule_event("ma", None, "")));
    }

    /// 关键词命中事实
    #[test]
    fn rule_matches_keyword_filter() {
        let rule = super::AlertRule {
            enabled: true,
            event_types: vec![],
            sentiments: vec![],
            keywords: vec!["收购".to_string()],
        };
        assert!(super::rule_matches(&rule, &make_rule_event("ma", None, "完成收购")));
        assert!(!super::rule_matches(&rule, &make_rule_event("ma", None, "签署合作备忘录")));
    }

    /// 解析：缺省 enabled 为 true
    #[test]
    fn parse_alert_rule_defaults_enabled_true() {
        let r = super::parse_alert_rule(r#"{"event_types":["ma"]}"#).expect("应解析成功");
        assert!(r.enabled);
        assert_eq!(r.event_types, vec!["ma".to_string()]);
    }

    /// 解析：非法 JSON 返回 None（无规则则不触发）
    #[test]
    fn parse_alert_rule_invalid_json_is_none() {
        assert!(super::parse_alert_rule("not json").is_none());
    }
}
