//! RSSHub 实例的可用性探测与选择
//!
//! # 职责
//! 解决"用户只输入一条 RSSHub 路由（如 `/anthropic/research`）时，该拼到哪个实例上"的问题：
//! - 维护一份候选实例清单（内置默认 + 用户自定义），见 [`candidate_instances`]；
//! - 按优先级**依次探测**，选出第一个真正能返回内容的实例，见 [`resolve`]；
//! - 对「完整 URL」形态的路由做同样的可用性判断，见 [`reject_unusable_route`]；
//! - 为"按来源自动分组"提供实例主机名判定，见 [`is_rsshub_host`]。
//!
//! # 为什么需要探测而不是写死一个实例
//! RSSHub 的公共实例没有可用性保证：官方 `rsshub.app` 会限流、会短时不可达，
//! 用户填一条路由时若恰好撞上它掉线，得到的是一个指向死实例的订阅源，
//! 而且要等后台抓取失败后才知道。提前探测能把"选哪个实例"这件事在落库前解决掉。
//!
//! # 设计意图
//! - **结果缓存**：命中过可用实例后，在 [`CACHE_TTL_OK`] 内直接复用，避免反复探测；
//!   缓存键是"用户配置值 + 路由"——配置一改即失效，换了路由也重新探测
//!   （探测结论与路由强绑定，只按配置缓存会让需要浏览器的路由被静默放行）。
//! - **失败兜底不阻塞**：全部实例都测不通时**不报错**，退回首选实例照常落库——
//!   探测失败往往只是本地断网，此时拒绝添加反而挡住用户；真实抓取失败由既有
//!   的后台回填链路（`feed-updated` 事件）负责提示。
//! - **但「路由本身跑不了」必须当场报错**：缺浏览器、路由不存在这类问题与实例是否
//!   存活无关，换多少实例结果都一样。旧实现把它和「实例不可达」一起当成失败静默兜底，
//!   于是订阅照常落库、之后永远抓不到内容——用户只会以为源坏了（见 [`RouteFailure`]）。
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

/// 「这条路由本身跑不了」的分类
///
/// 与 [`ProbeOutcome::Unreachable`] 的区别是本模块最重要的一条分界线：
/// 这里的两种都是**实例已经给了明确答复**，换实例重试没有意义；
/// 而 `Unreachable` 只是"此刻没答上来"，可能只是本地断网。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteFailure {
    /// 路由需要浏览器渲染（RSSHub 文档里标 🎭 的那些），而实例没配 Chromium
    BrowserRequired,
    /// 实例上不存在这条路由
    NotFound,
}

impl RouteFailure {
    /// 生成给用户看的原因说明
    ///
    /// 必须写到"下一步该做什么"，否则用户只会知道失败了、不知道为什么，
    /// 而这正是这个功能要解决的问题。
    fn message(self, route: &str) -> String {
        match self {
            RouteFailure::BrowserRequired => format!(
                "这条路由（{route}）需要浏览器渲染页面才能抓取，公共 RSSHub 实例不提供浏览器，\
                 因此抓不到内容。可以换一条不需要浏览器的路由、改用站点自带的 RSS 地址，\
                 或自建一个带 Chromium 的 RSSHub 实例，再把它的地址填进「设置 → 通用 → RSSHub 实例地址」。"
            ),
            RouteFailure::NotFound => format!(
                "RSSHub 上找不到这条路由（{route}），请检查拼写——\
                 这里要填路由本身（以 / 开头），而不是某个具体站点的完整地址。"
            ),
        }
    }
}

/// 单个实例对一次探测的答复
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// 返回了 2xx：这个实例此刻能服务这条路由
    Served,
    /// 实例有答复，但这条路由在它上面跑不了
    RouteFailed(RouteFailure),
    /// 没有任何有效答复：超时、连接失败、网关错误。责任在实例（或本地网络）而不在路由
    Unreachable,
}

/// 响应体里出现这些片段，就说明失败原因是"没有浏览器"
///
/// 全部小写：比较前会把响应体转小写。收录的是 puppeteer / playwright 两条链路的
/// 典型措辞，任意命中一条即可（启动失败时通常同时出现多条）。
const BROWSER_ERROR_NEEDLES: [&str; 5] = [
    "could not find chrome",
    "chromium",
    "puppeteer",
    "playwright",
    "launch the browser",
];

/// 判断一个非 2xx 响应是不是「路由级」故障
///
/// # 参数
/// * `status` - HTTP 状态码
/// * `body` - 响应体（仅在非 2xx 时读取）
///
/// # 返回值
/// 命中浏览器特征串 → [`RouteFailure::BrowserRequired`]；
/// 404 → [`RouteFailure::NotFound`]；其余一律 [`ProbeOutcome::Unreachable`]。
///
/// # 为什么靠 body 特征串判定
/// RSSHub 对「路由内部抛异常」（含 Chromium 启动失败）和「实例自身故障」都回 503，
/// 状态码本身分不开这两件事。唯一能区分的是响应体：puppeteer / playwright 的启动失败
/// 消息里必然出现 Chromium 相关的字样（`Could not find Chrome (ver. …)` 等）。
/// 判错的代价不对称——把实例故障误判成路由故障会拦住用户添加，
/// 所以这里只认**明确的**浏览器特征，认不出来就仍按实例故障处理。
fn classify_error_response(status: u16, body: &str) -> ProbeOutcome {
    let lower = body.to_ascii_lowercase();
    if BROWSER_ERROR_NEEDLES.iter().any(|n| lower.contains(n)) {
        return ProbeOutcome::RouteFailed(RouteFailure::BrowserRequired);
    }
    if status == 404 {
        return ProbeOutcome::RouteFailed(RouteFailure::NotFound);
    }
    ProbeOutcome::Unreachable
}

/// 汇总一轮探测：所有候选是否给出了**同一个**明确的路由级拒绝
///
/// # 参数
/// * `outcomes` - 每个候选的探测结果，按候选顺序
///
/// # 返回值
/// 全部为 [`ProbeOutcome::RouteFailed`] 且理由一致时返回该理由，否则 `None`。
///
/// # 为什么要"全部且一致"
/// 只要有一个实例没答上来（超时 / 断网），就不能断言"这条路由到哪儿都跑不了"；
/// 一半实例说缺浏览器、另一半说路由不存在，说明行为因实例而异，下任何结论都可能是错的。
/// 这两种情况下退回兜底更安全——宁可少报，不可误拦。
fn unanimous_failure(outcomes: &[ProbeOutcome]) -> Option<RouteFailure> {
    let mut kind: Option<RouteFailure> = None;
    for outcome in outcomes {
        match outcome {
            ProbeOutcome::RouteFailed(f) => match kind {
                None => kind = Some(*f),
                Some(k) if k == *f => {}
                Some(_) => return None,
            },
            _ => return None,
        }
    }
    kind
}

/// 在 [`unanimous_failure`] 之上，再判断这个一致结论是否足以拦住用户
///
/// # 参数
/// * `outcomes` - 已探测候选的结果
/// * `budget_exhausted` - 是否因 [`TOTAL_BUDGET`] 耗尽而提前结束探测
///
/// # 返回值
/// 可以据此拒绝时返回理由，否则 `None`。
///
/// # 为什么预算耗尽只放行"缺浏览器"
/// 预算耗尽意味着还有候选没探过。**缺浏览器是所有公共实例的共性**——它们都不装
/// Chromium，没探到的实例不会给出不同答案，所以结论仍然可靠；而**"路由不存在"
/// 可能因实例而异**（某个镜像也许正好有这条路由），没探过就说"哪儿都没有"是武断的。
fn conclusive_failure(outcomes: &[ProbeOutcome], budget_exhausted: bool) -> Option<RouteFailure> {
    let failure = unanimous_failure(outcomes)?;
    if budget_exhausted && failure != RouteFailure::BrowserRequired {
        return None;
    }
    Some(failure)
}

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
    /// 写入缓存时所探测的路由
    ///
    /// 必须进缓存键：探测结论是「这条路由在这个实例上能不能跑」，
    /// 与路由强绑定。只按 configured 缓存会让"添加第一条源时探到的成功实例"
    /// 在 5 分钟内顶替掉所有后续路由的探测——需要浏览器的路由因此被静默放行，
    /// 正是本模块要避免的那种失败。代价是每条新路由各探一次（通常几百毫秒）。
    route: String,
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
/// 2xx → [`ProbeOutcome::Served`]；
/// 非 2xx 且能认出是路由级故障 → [`ProbeOutcome::RouteFailed`]（理由见 [`classify_error_response`]）；
/// 超时 / 连接失败 / 认不出的错误 → [`ProbeOutcome::Unreachable`]。
///
/// # 为什么成功时不读 body
/// 探测只需要知道"能用"，成功分支把整份 feed 拉下来纯属浪费；只有失败时才读，
/// 因为响应体是区分"实例挂了"和"这条路由跑不了"的唯一依据。
///
/// # 超时
/// 用的是共享客户端（自身已设 6s 超时），外面**再套一层** `tokio::time::timeout`：
/// 万一客户端构建失败退化成不带超时的默认客户端，"单个实例最多等 6 秒"这个前提
/// 仍然成立——否则一次探测就能吃掉整个候选列表的时间预算。
/// 客户端超时覆盖到读 body 为止，所以读 body 不会额外延长这个上限。
pub async fn probe(instance: &str, route: &str) -> ProbeOutcome {
    let url = format!("{}{}", instance.trim_end_matches('/'), route);
    let request = crate::net::probe_client()
        .get(&url)
        .header("User-Agent", PROBE_USER_AGENT)
        .send();

    let resp = match tokio::time::timeout(crate::net::PROBE_TIMEOUT, request).await {
        Ok(Ok(resp)) => resp,
        // 网络错误与超时同义：都是"此刻没答上来"，可能只是本地断网
        _ => return ProbeOutcome::Unreachable,
    };

    let status = resp.status();
    if status.is_success() {
        return ProbeOutcome::Served;
    }

    let code = status.as_u16();
    let body = resp.text().await.unwrap_or_default();
    classify_error_response(code, &body)
}

/// 选取可用于指定路由的 RSSHub 实例
///
/// # 参数
/// * `configured` - 设置项 `rsshub_base_url` 的值（可为空）
/// * `route` - 以 `/` 开头的 RSSHub 路由
///
/// # 返回值
/// 顺序为：命中缓存 → 按 [`candidate_instances`] 顺序探测出的第一个可用实例 →
/// 全部失败时的首个候选实例。前两种为 `Ok`，第三种见下。
///
/// 当**每个候都明确回答"这条路由跑不了"且理由一致**时返回 `Err`，内容是可以直接
/// 显示给用户的原因说明。这是本模块唯一会给"添加订阅"制造失败的分支，
/// 因为那类问题换任何实例都一样，让订阅落库只会变成一个永远抓不到内容的空源。
///
/// # 副作用
/// - 会发起最多"候选数"次网络探测（受 [`TOTAL_BUDGET`] 约束）；
/// - 结果写入进程内缓存，供后续调用复用。**失败（`Err`）不写缓存**：
///   路由级故障与具体路由绑定，而缓存键里的 `configured` 是全局共享的，
///   写进去会连带污染其它路由的实例选择。
pub async fn resolve(configured: &str, route: &str) -> Result<String, String> {
    let candidates = candidate_instances(configured);
    // 候选列表保证非空（DEFAULT_INSTANCE 恒定纳入），无需再处理 None
    let fallback = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_INSTANCE.to_string());

    if let Some(hit) = cached(configured, route) {
        return Ok(hit);
    }

    let started = Instant::now();
    let mut outcomes: Vec<ProbeOutcome> = Vec::new();
    // 预算耗尽时还有候选没探过，此时不能对路由下任何结论
    let mut budget_exhausted = false;

    for instance in &candidates {
        // 预算耗尽即停止：继续探测只会让用户多等，而兜底结果是一样的
        if started.elapsed() >= TOTAL_BUDGET {
            budget_exhausted = true;
            break;
        }
        let outcome = probe(instance, route).await;
        if matches!(outcome, ProbeOutcome::Served) {
            store_cache(configured, route, instance, false);
            return Ok(instance.clone());
        }
        outcomes.push(outcome);
    }

    if let Some(failure) = conclusive_failure(&outcomes, budget_exhausted) {
        return Err(failure.message(route));
    }

    store_cache(configured, route, &fallback, true);
    Ok(fallback)
}

/// 对一条「完整 URL」形态的 RSSHub 路由做一次性判断
///
/// # 背景
/// 完整 URL（`https://rsshub.app/xxx`）不走实例选择——用户给的就是最终地址，
/// 补前缀或换实例都可能违背他的意图。但它同样可能属于"这条路由跑不了"那一类，
/// 而旧实现会把它原样放过：订阅静默落库、之后永远抓不到内容。
/// 偏偏"直接粘官网示例里的完整地址"是最常见的添加方式，这道检查必须覆盖到。
///
/// # 参数
/// * `url` - 用户输入的完整 http(s) 地址
/// * `configured` - 设置项 `rsshub_base_url` 的值（用于识别用户自己的实例）
///
/// # 返回值
/// 该实例明确拒绝这条路由时返回原因说明（可直接显示给用户）；
/// 不是 RSSHub 实例、探测不通、或探测成功时一律 `Ok(())`——保持原有的宽容语义。
///
/// # 为什么只探测 URL 里那一个实例
/// 用户指定了实例，就按这个实例的答复办事。遍历候选会引入"某个镜像能跑"的误导，
/// 而用户粘的地址根本不会指向那个镜像。代价是每次这类添加多一次探测（通常几百毫秒）。
pub async fn reject_unusable_route(url: &str, configured: &str) -> Result<(), String> {
    let Some((origin, route)) = split_origin_path(url) else {
        return Ok(());
    };
    let (host, _) = split_host_path(url);
    // 只对已知 RSSHub 实例做这项检查：普通站点的 404 是它自己的事，
    // 拦下来只会挡住"这个源过两天可能就好了"的正常情况
    if host.is_empty() || !is_rsshub_host(&host, configured) {
        return Ok(());
    }
    if route.is_empty() || route == "/" {
        return Ok(());
    }
    match probe(&origin, &route).await {
        ProbeOutcome::RouteFailed(failure) => Err(failure.message(&route)),
        _ => Ok(()),
    }
}

/// 把 URL 拆成 `(origin, path+query)` 两段
///
/// # 参数
/// * `url` - 形如 `https://host:port/a/b?c=1` 的完整地址
///
/// # 返回值
/// `origin` 为 `协议://主机[:端口]`，`route` 为其后的部分原样保留
/// （含查询串，以便探测的就是用户真正给定的那个地址）；缺协议头时返回 `None`。
fn split_origin_path(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some((format!("{}://{}", scheme, &rest[..end]), rest[end..].to_string()))
}

/// 读取缓存的解析结果
///
/// # 参数
/// * `configured` - 当前设置项 `rsshub_base_url` 的值（须与写入缓存时一致）
/// * `route` - 本次要探测的路由（须与写入缓存时一致）
///
/// # 返回值
/// 缓存命中且未过期时返回实例地址，否则 `None`
fn cached(configured: &str, route: &str) -> Option<String> {
    let guard = cache_slot().lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.configured != normalize_instance(configured) || entry.route != route {
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
/// * `configured` - 当前的设置项值（作为缓存键的一部分）
/// * `route` - 本次探测的路由（作为缓存键的一部分）
/// * `resolved` - 选中的实例地址
/// * `fallback` - 是否为"全部失败后的兜底值"（决定 TTL 长短）
fn store_cache(configured: &str, route: &str, resolved: &str, fallback: bool) {
    if let Ok(mut guard) = cache_slot().lock() {
        *guard = Some(CacheEntry {
            configured: normalize_instance(configured),
            route: route.to_string(),
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

    /// 缺浏览器：RSSHub 把「路由内部抛异常」统一回 503，状态码分不出原因，只能认响应体
    #[test]
    fn puppeteer_error_is_route_failure() {
        // 取自 puppeteer 实际抛出的原文（Chromium 未安装时的标准姿势）
        let body = r#"{"error":{"message":"Error: Could not find Chrome (ver. 123.0.6312.86). This can occur if either 1. you did not perform an installation before running the script"}}"#;
        assert_eq!(
            classify_error_response(503, body),
            ProbeOutcome::RouteFailed(RouteFailure::BrowserRequired)
        );
        // 特征串匹配大小写无关
        assert_eq!(
            classify_error_response(500, "Playwright: failed to launch the browser process"),
            ProbeOutcome::RouteFailed(RouteFailure::BrowserRequired)
        );
    }

    /// 认不出的 5xx / 429 一律按「实例此刻不可用」处理——
    /// 误判成路由故障会拦住用户添加，这个方向的代价更大
    #[test]
    fn opaque_errors_stay_instance_level() {
        for (status, body) in [
            (503u16, "<html>Service Unavailable</html>"),
            (502, ""),
            (500, "Internal Server Error"),
            (429, "Too Many Requests"),
        ] {
            assert_eq!(
                classify_error_response(status, body),
                ProbeOutcome::Unreachable,
                "{} + {:?} 不应被判成路由级故障",
                status,
                body
            );
        }
    }

    /// 404 是实例明确说"没这条路由"，属于路由级
    #[test]
    fn not_found_is_route_failure() {
        assert_eq!(
            classify_error_response(404, "Path Not Found"),
            ProbeOutcome::RouteFailed(RouteFailure::NotFound)
        );
    }

    /// 起一个只会回错误的假 RSSHub 实例
    ///
    /// 按请求路径给出不同的状态码与响应体，用来验证「同样是非 2xx，
    /// 响应体不同则判定结果必须不同」这条核心分界。
    ///
    /// # 返回值
    /// 实例 origin（`http://127.0.0.1:端口`），同时也可当作 `configured` 传入——
    /// 这样 [`is_rsshub_host`] 会把它认作"用户自己的实例"。
    async fn spawn_fake_instance() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let (status, body) = if req.contains("/needs/browser") {
                    (503, r#"{"error":{"message":"Error: Could not find Chrome"}}"#)
                } else if req.contains("/gone") {
                    (404, "Path Not Found")
                } else {
                    (502, "<html>Bad Gateway</html>")
                };
                let resp = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        format!("http://{}", addr)
    }

    /// 端到端验证 `probe` 真的读了响应体
    ///
    /// 纯函数测试只覆盖了"给了 body 怎么判"，覆盖不到"body 有没有读上来"。
    #[tokio::test]
    async fn probe_reads_body_to_tell_route_failure_from_instance_failure() {
        let instance = spawn_fake_instance().await;

        assert_eq!(
            probe(&instance, "/needs/browser").await,
            ProbeOutcome::RouteFailed(RouteFailure::BrowserRequired),
            "503 + 浏览器特征串 → 路由级故障"
        );
        assert_eq!(
            probe(&instance, "/gone").await,
            ProbeOutcome::RouteFailed(RouteFailure::NotFound),
            "404 → 路由不存在"
        );
        assert_eq!(
            probe(&instance, "/boom").await,
            ProbeOutcome::Unreachable,
            "认不出的 5xx → 仍按实例不可用处理，不能拦用户"
        );
    }

    /// 完整 URL 形态：落在已知 RSSHub 实例上时也要拦住"跑不了"的路由
    ///
    /// 这条分支最容易被漏掉——用户习惯直接粘官网示例里的完整地址，
    /// 而完整 URL 不走实例选择，等于绕过了全部探测。
    #[tokio::test]
    async fn full_url_on_rsshub_instance_is_checked() {
        let instance = spawn_fake_instance().await;
        let url = format!("{}/needs/browser", instance);

        let err = reject_unusable_route(&url, &instance)
            .await
            .expect_err("需要浏览器的路由应当被拒绝");
        assert!(err.contains("浏览器"), "原因里要说清是浏览器问题：{err}");

        // 认不出的 5xx：保持宽容，不能连加都加不上
        assert!(
            reject_unusable_route(&format!("{}/boom", instance), &instance)
                .await
                .is_ok()
        );
        // 不是已知 RSSHub 实例：一律放过，普通站点的 404 是它自己的事
        assert!(
            reject_unusable_route(&url, "").await.is_ok(),
            "没有配置成该实例时不应插手"
        );
    }

    /// URL 拆分：origin 保留端口，route 原样带上查询串（探测的就是用户给定的地址）
    #[test]
    fn split_origin_path_keeps_port_and_query() {
        assert_eq!(
            split_origin_path("http://127.0.0.1:1200/a/b?c=1"),
            Some(("http://127.0.0.1:1200".to_string(), "/a/b?c=1".to_string()))
        );
        assert_eq!(
            split_origin_path("https://rsshub.app"),
            Some(("https://rsshub.app".to_string(), String::new()))
        );
        // 裸路由没有 origin，不属于这条分支
        assert_eq!(split_origin_path("/bilibili/user"), None);
    }

    /// 只有「每个候选都明确拒绝、且理由一致」才允许对路由下定论
    #[test]
    fn unanimous_failure_requires_every_candidate() {
        use ProbeOutcome::{RouteFailed, Unreachable};
        let browser = RouteFailed(RouteFailure::BrowserRequired);

        assert_eq!(
            unanimous_failure(&[browser.clone(), browser.clone()]),
            Some(RouteFailure::BrowserRequired)
        );
        // 有一个实例没答上来（可能只是断网）→ 不下结论
        assert_eq!(unanimous_failure(&[browser.clone(), Unreachable]), None);
        // 两种不同故障 → 行为因实例而异，不下结论
        assert_eq!(
            unanimous_failure(&[browser, RouteFailed(RouteFailure::NotFound)]),
            None
        );
        // 空集（预算一开始就耗尽）不能被当成"全都拒绝"
        assert_eq!(unanimous_failure(&[]), None);
    }

    /// 预算耗尽时只有"缺浏览器"仍可下结论：它是公共实例的共性，
    /// 没探到的实例不会给出不同答案；"路由不存在"则可能因实例而异
    #[test]
    fn budget_exhaustion_only_concludes_browser_failures() {
        use ProbeOutcome::RouteFailed;
        let browser = [RouteFailed(RouteFailure::BrowserRequired)];
        let missing = [RouteFailed(RouteFailure::NotFound)];

        // 探测走完了：两种都能下结论
        assert_eq!(
            conclusive_failure(&browser, false),
            Some(RouteFailure::BrowserRequired)
        );
        assert_eq!(
            conclusive_failure(&missing, false),
            Some(RouteFailure::NotFound)
        );
        // 预算耗尽：缺浏览器仍然算数
        assert_eq!(
            conclusive_failure(&browser, true),
            Some(RouteFailure::BrowserRequired)
        );
        // 预算耗尽 + 路由不存在：可能有实例没探到，不能拦
        assert_eq!(conclusive_failure(&missing, true), None);
    }

    /// 失败说明必须写到"下一步做什么"：只报失败，用户的困惑一点没解决
    #[test]
    fn failure_messages_are_actionable() {
        let no_browser = RouteFailure::BrowserRequired.message("/twitter/user/foo");
        assert!(
            no_browser.contains("/twitter/user/foo"),
            "要带上路由，用户才知道说的是哪一条"
        );
        assert!(no_browser.contains("自建"), "要给出出路，而不只是否定");

        let missing = RouteFailure::NotFound.message("/nope/nothing");
        assert!(missing.contains("/nope/nothing"));
        assert!(
            missing.contains("以 / 开头"),
            "顺手纠正最常见的误用：填了完整 URL 会被当成站外地址放过探测"
        );
    }

    /// 缓存键必须包含路由
    ///
    /// 只按 configured 缓存时，"添加第一条源时探到的成功实例"会在 TTL 内顶替掉
    /// 后续所有路由的探测——需要浏览器的路由因此被静默放行，正是这次要堵的洞。
    #[test]
    fn cache_key_includes_route() {
        store_cache("https://cache-key-test.dev", "/a/b", "https://inst.example", false);
        assert_eq!(
            cached("https://cache-key-test.dev", "/a/b"),
            Some("https://inst.example".to_string())
        );
        assert_eq!(
            cached("https://cache-key-test.dev", "/a/c"),
            None,
            "不同路由不应命中同一条缓存"
        );
    }
}
