//! RSSHub 实例的可用性探测与选择
//!
//! # 职责
//! 解决"用户只输入一条 RSSHub 路由（如 `/anthropic/research`）时，该拼到哪个实例上"的问题：
//! - 维护一份候选实例清单（内置默认 + 用户自定义），见 [`candidate_instances`]；
//! - 按优先级**依次探测**，选出第一个真正能返回内容的实例，见 [`resolve`]；
//! - 为"按来源自动分组"提供实例主机名判定，见 [`is_rsshub_host`]。
//!
//! # 为什么需要探测而不是写死一个实例
//! RSSHub 的公共实例没有可用性保证：官方 `rsshub.app` 会限流、会短时不可达，
//! 用户填一条路由时若恰好撞上它掉线，得到的是一个指向死实例的订阅源，
//! 而且要等后台抓取失败后才知道。提前探测能把"选哪个实例"这件事在落库前解决掉。
//!
//! # 设计意图
//! - **结果缓存**：命中过可用实例后，在 [`CACHE_TTL_OK`] 内直接复用，避免连续添加
//!   多个订阅源时反复探测；缓存键包含用户配置值，用户一改设置即自动失效。
//! - **失败兜底不阻塞**：全部实例都测不通时**不报错**，退回首选实例照常落库——
//!   探测失败往往只是本地断网，此时拒绝添加反而挡住用户；真实抓取失败由既有
//!   的后台回填链路（`feed-updated` 事件）负责提示。
//! - **失败结果短缓存**：兜底值只缓存 [`CACHE_TTL_FALLBACK`]，让实例恢复后能较快被重新探测到，
//!   同时避免"首选一直不可达"时每次添加都要把候选挨个等一遍。
//! - **总预算**：一轮探测最多花 [`TOTAL_BUDGET`]，防止最坏情况下把添加操作拖到一分钟以上。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 默认（首选）RSSHub 实例
pub const DEFAULT_INSTANCE: &str = "https://rsshub.app";

/// 首选实例不可达时按顺序尝试的备选实例
pub const FALLBACK_INSTANCES: [&str; 4] = [
    "https://rsshub.rssforever.com",
    "https://rss.qiuyuair.com",
    "https://rsshub.uneasy.win",
    "https://rsshub.netlify.app",
];

// 单次探测的超时（6s）定义在 `crate::net::PROBE_TIMEOUT`：与客户端实例放在一起，
// 避免"超时值"和"设超时的客户端"分散在两个模块里各改一处。

/// 一轮探测（遍历全部候选实例）的总时间预算
///
/// 没有它时最坏耗时是"候选数 × 单次超时"（约 30 秒），用户会以为界面卡死。
/// 预算耗尽即停止探测并按兜底规则返回首个实例。
const TOTAL_BUDGET: Duration = Duration::from_secs(18);

/// 探测成功结果的缓存有效期
const CACHE_TTL_OK: Duration = Duration::from_secs(300);

/// 全部失败（兜底）结果的缓存有效期
///
/// 明显短于成功缓存：兜底值本身是"不确定可用"的猜测，让实例恢复后能较快被重新发现。
const CACHE_TTL_FALLBACK: Duration = Duration::from_secs(60);

/// 探测请求使用的 User-Agent
///
/// 与 `rss::fetch_feed` 保持一致：部分实例会拒绝空 UA 或默认爬虫 UA，
/// 用同一个标识能保证"探测能过、正式抓取也能过"。
const PROBE_USER_AGENT: &str = "ReadFlow/1.0 (RSS Reader)";

/// 单个实例的探测结果缓存条目
struct CacheEntry {
    /// 写入缓存时用户配置的实例地址
    ///
    /// 作为缓存键的一部分：用户在设置里改了实例地址，缓存立即视为失效，
    /// 否则新设置的地址要等 TTL 过期才生效，表现为"设置改了没反应"。
    ///
    /// 注意调用方（[`crate::normalize_feed_url`]）探测成功后会把这个实例写回设置，
    /// 因此"发生实例切换"那一轮之后缓存键会与上一轮不同、缓存自然失效。
    /// 代价是切换后多一次探测，而这次探测的候选首位正是刚验证过的实例，
    /// 单次请求即可命中，可以接受。
    configured: String,
    /// 本轮解析选中的实例地址
    resolved: String,
    /// 写入时间（用于 TTL 判断）
    at: Instant,
    /// 该结果是否为"全部探测失败后的兜底值"
    fallback: bool,
}

/// 进程内解析结果缓存
///
/// 不加 TTL 之外的失效机制：候选清单与探测逻辑都是纯函数式的，
/// 唯一的外部输入是用户配置值（已作为缓存键）。
static RESOLVED: OnceLock<Mutex<Option<CacheEntry>>> = OnceLock::new();

/// 取（必要时初始化）全局缓存的互斥槽位
fn cache_slot() -> &'static Mutex<Option<CacheEntry>> {
    RESOLVED.get_or_init(|| Mutex::new(None))
}

/// 规范化实例地址：去首尾空白与尾部斜杠
///
/// # 参数
/// * `instance` - 原始实例地址，可能带尾斜杠或空白（用户在设置里手输）
///
/// # 返回值
/// 规范化后的地址；输入无有效内容时返回空串
///
/// # 说明
/// 对 `crate::lib` 公开：`normalize_feed_url` 写回设置前需要用它判断
/// "探测结果是否真的与当前设置不同"，避免每次添加都白写一次库。
pub fn normalize_instance(instance: &str) -> String {
    instance.trim().trim_end_matches('/').trim().to_string()
}

/// 列出本次可用的 RSSHub 实例，按优先级排列
///
/// # 参数
/// * `configured` - 设置项 `rsshub_base_url` 的值（可为空）
///
/// # 返回值
/// 去重后的实例地址列表。用户配置的实例排在最前（体现"用户指定优先"），
/// 其后是 [`DEFAULT_INSTANCE`] 与 [`FALLBACK_INSTANCES`]。
/// 配置为空或与内置实例重复时，结果即内置清单本身。
///
/// # 为什么不允许返回空列表
/// 调用方需要"至少有一个实例可拼"。把 [`DEFAULT_INSTANCE`] 无条件纳入，
/// 保证"未配置实例"不再是错误分支，而是回退到官方实例。
pub fn candidate_instances(configured: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    let configured = normalize_instance(configured);
    if !configured.is_empty() {
        out.push(configured);
    }

    for instance in std::iter::once(DEFAULT_INSTANCE).chain(FALLBACK_INSTANCES) {
        let instance = normalize_instance(instance);
        if !instance.is_empty() && !out.contains(&instance) {
            out.push(instance);
        }
    }

    out
}

/// 提取 URL 的主机名（小写、去 `www.`、去 userinfo 与端口）
///
/// # 参数
/// * `url` - 任意 http(s) URL；不是完整 URL 时按"整串就是主机"处理
///
/// # 返回值
/// 主机名；无法解析出非空主机时返回 `None`
pub fn host_of(url: &str) -> Option<String> {
    let (host, _) = split_host_path(url);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// 把 URL 拆成"主机名"与"路径"两段
///
/// # 参数
/// * `url` - 任意 http(s) URL，或裸主机名
///
/// # 返回值
/// `(主机名, 路径)`。主机名已去 userinfo、端口与 `www.` 前缀并转小写；
/// 路径保留首个 `/`（无路径时为空串），且**不含** query 与 fragment——
/// 调用方只关心路径上的路由段，查询参数对"来源判定"没有意义。
///
/// # 为什么不用 `url` crate
/// 该函数也要处理"用户在设置里填的实例前缀"这种可能缺协议头的字符串，
/// 以及历史上产生过的畸形 URL（实例前缀后嵌了 `rsshub://`），
/// 宽松的字符串切分比严格解析更不容易在这些输入上直接失败。
pub fn split_host_path(url: &str) -> (String, String) {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);

    // 主机段在第一个 `/`、`?`、`#` 之前结束
    let end = without_scheme
        .find(['/', '?', '#'])
        .unwrap_or(without_scheme.len());
    let authority = &without_scheme[..end];
    let rest = &without_scheme[end..];

    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = authority.split(':').next().unwrap_or(authority);
    let host = host.strip_prefix("www.").unwrap_or(host);
    let path = rest.split(['?', '#']).next().unwrap_or("");

    (host.to_lowercase(), path.to_string())
}

/// 判断一个主机名是否属于 RSSHub 实例
///
/// # 参数
/// * `host` - 待判定的主机名（大小写不敏感）
/// * `configured` - 设置项 `rsshub_base_url` 的值，用于纳入手工配置的私有/自建实例
///
/// # 返回值
/// 属于 RSSHub 实例返回 `true`
///
/// # 判定规则
/// 1. 精确命中 [`candidate_instances`] 中的实例主机名（含用户自定义实例）；
/// 2. 或以 `rsshub.` 开头且**总段数不超过 3**。
///
/// 第 2 条的段数限制是防误判的启发式：`rsshub.app`（2 段）、`rsshub.netlify.app`（3 段）
/// 是实例形态，而 `rsshub.app.evil.com`（4 段）只是碰巧以 `rsshub.` 开头的普通站点，
/// 若被当成实例，它的路径首段会被错误地当作"来源平台名"。
pub fn is_rsshub_host(host: &str, configured: &str) -> bool {
    let host = host.trim().trim_start_matches("www.").to_lowercase();
    if host.is_empty() {
        return false;
    }

    if instance_hosts(configured).iter().any(|h| h == &host) {
        return true;
    }

    match host.strip_prefix("rsshub.") {
        Some(rest) => !rest.is_empty() && host.split('.').count() <= 3,
        None => false,
    }
}

/// 列出全部候选实例的主机名（小写）
///
/// # 参数
/// * `configured` - 设置项 `rsshub_base_url` 的值（可为空）
///
/// # 返回值
/// 候选实例的主机名列表，供 [`is_rsshub_host`] 做精确匹配
pub fn instance_hosts(configured: &str) -> Vec<String> {
    candidate_instances(configured)
        .iter()
        .filter_map(|instance| host_of(instance))
        .collect()
}

/// 探测某个实例是否能返回指定路由的内容
///
/// 发一次 GET 到 `{instance}{route}`，只读响应状态码、不读 body——
/// 本函数只关心"这个实例现在能不能服务这条路由"，正文由后续正式抓取负责。
///
/// # 参数
/// * `instance` - 实例地址（无尾斜杠），如 `https://rsshub.app`
/// * `route` - 以 `/` 开头的路由，如 `/anthropic/research`
///
/// # 返回值
/// 返回 2xx 视为可用；超时、连接失败、非 2xx 一律为不可用
///
/// # 超时
/// 用的是共享客户端（自身已设 6s 超时），外面**再套一层** `tokio::time::timeout`：
/// 万一客户端构建失败退化成不带超时的默认客户端，"单个实例最多等 6 秒"这个前提
/// 仍然成立——否则一次探测就能吃掉整个候选列表的时间预算。
pub async fn probe(instance: &str, route: &str) -> bool {
    let url = format!("{}{}", instance.trim_end_matches('/'), route);
    let request = crate::net::probe_client()
        .get(&url)
        .header("User-Agent", PROBE_USER_AGENT)
        .send();

    match tokio::time::timeout(crate::net::PROBE_TIMEOUT, request).await {
        Ok(Ok(resp)) => resp.status().is_success(),
        // 网络错误与超时同义：都是"此刻不可用"，调用方只需知道可用/不可用
        _ => false,
    }
}

/// 选取可用于指定路由的 RSSHub 实例
///
/// # 参数
/// * `configured` - 设置项 `rsshub_base_url` 的值（可为空）
/// * `route` - 以 `/` 开头的 RSSHub 路由
///
/// # 返回值
/// 选中的实例地址（无尾斜杠）。顺序为：命中缓存 → 按 [`candidate_instances`]
/// 顺序探测出的第一个可用实例 → 全部失败时的首个候选实例。
///
/// # 副作用
/// - 会发起最多"候选数"次网络探测（受 [`TOTAL_BUDGET`] 约束）；
/// - 结果写入进程内缓存，供后续调用复用。
pub async fn resolve(configured: &str, route: &str) -> String {
    let candidates = candidate_instances(configured);
    // 候选列表保证非空（DEFAULT_INSTANCE 恒定纳入），无需再处理 None
    let fallback = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_INSTANCE.to_string());

    if let Some(hit) = cached(configured) {
        return hit;
    }

    let started = Instant::now();
    for instance in &candidates {
        // 预算耗尽即停止：继续探测只会让用户多等，而兜底结果是一样的
        if started.elapsed() >= TOTAL_BUDGET {
            break;
        }
        if probe(instance, route).await {
            store_cache(configured, instance, false);
            return instance.clone();
        }
    }

    store_cache(configured, &fallback, true);
    fallback
}

/// 读取缓存的解析结果
///
/// # 参数
/// * `configured` - 当前设置项 `rsshub_base_url` 的值（须与写入缓存时一致）
///
/// # 返回值
/// 缓存命中且未过期时返回实例地址，否则 `None`
fn cached(configured: &str) -> Option<String> {
    let guard = cache_slot().lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.configured != normalize_instance(configured) {
        return None;
    }
    let ttl = if entry.fallback {
        CACHE_TTL_FALLBACK
    } else {
        CACHE_TTL_OK
    };
    if entry.at.elapsed() > ttl {
        return None;
    }
    Some(entry.resolved.clone())
}

/// 写入解析结果缓存
///
/// # 参数
/// * `configured` - 当前的设置项值（作为缓存键）
/// * `resolved` - 选中的实例地址
/// * `fallback` - 是否为"全部失败后的兜底值"（决定 TTL 长短）
fn store_cache(configured: &str, resolved: &str, fallback: bool) {
    if let Ok(mut guard) = cache_slot().lock() {
        *guard = Some(CacheEntry {
            configured: normalize_instance(configured),
            resolved: resolved.to_string(),
            at: Instant::now(),
            fallback,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 候选清单：配置为空时就是内置清单，顺序固定
    #[test]
    fn candidates_default_order() {
        let list = candidate_instances("");
        assert_eq!(
            list,
            vec![
                "https://rsshub.app".to_string(),
                "https://rsshub.rssforever.com".to_string(),
                "https://rss.qiuyuair.com".to_string(),
                "https://rsshub.uneasy.win".to_string(),
                "https://rsshub.netlify.app".to_string(),
            ]
        );
    }

    /// 候选清单：用户配置的实例提到最前，且与内置项去重（含尾斜杠差异）
    #[test]
    fn candidates_configured_first_and_dedup() {
        let list = candidate_instances("https://rsshub.app/");
        assert_eq!(list.len(), 5, "尾斜杠差异不应产生重复项");
        assert_eq!(list[0], "https://rsshub.app");

        let custom = candidate_instances("https://rss.example.dev/");
        assert_eq!(custom.len(), 6);
        assert_eq!(custom[0], "https://rss.example.dev");
    }

    /// URL 拆分：去 www / userinfo / 端口，路径丢掉 query 与 fragment
    #[test]
    fn split_host_path_basics() {
        assert_eq!(
            split_host_path("https://rsshub.app/anthropic/research"),
            ("rsshub.app".to_string(), "/anthropic/research".to_string())
        );
        assert_eq!(
            split_host_path("https://www.wired.com/feed/rss"),
            ("wired.com".to_string(), "/feed/rss".to_string())
        );
        assert_eq!(
            split_host_path("https://user:pw@Example.com:8443/a/b?c=1#d"),
            ("example.com".to_string(), "/a/b".to_string())
        );
        assert_eq!(
            split_host_path("https://example.com"),
            ("example.com".to_string(), String::new())
        );
    }

    /// 实例判定：候选清单命中，以及 rsshub.* 的段数启发式
    #[test]
    fn rsshub_host_detection() {
        // 候选清单精确命中（含非 rsshub.* 前缀的实例）
        assert!(is_rsshub_host("rsshub.app", ""));
        assert!(is_rsshub_host("rss.qiuyuair.com", ""));
        assert!(is_rsshub_host("rsshub.rssforever.com", ""));
        assert!(is_rsshub_host("rsshub.netlify.app", ""));
        // 通配：rsshub.* 且段数 ≤ 3
        assert!(is_rsshub_host("rsshub.uneasy.win", ""));
        assert!(is_rsshub_host("RSSHub.Some-Mirror.io", ""));
        // 用户自定义实例
        assert!(is_rsshub_host("my.proxy.dev", "https://my.proxy.dev"));
        // 普通站点
        assert!(!is_rsshub_host("wired.com", ""));
        assert!(!is_rsshub_host("", ""));
        // 段数启发式：4 段不视为实例
        assert!(!is_rsshub_host("rsshub.app.evil.com", ""));
    }

    /// 实例主机名列表来自候选清单
    #[test]
    fn instance_hosts_from_candidates() {
        let hosts = instance_hosts("");
        assert_eq!(hosts.len(), 5);
        assert!(hosts.contains(&"rss.qiuyuair.com".to_string()));
        assert!(instance_hosts("https://my.proxy.dev/x").contains(&"my.proxy.dev".to_string()));
    }
}
