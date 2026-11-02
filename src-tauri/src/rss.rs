//! RSS/Atom/JSON Feed 抓取与解析模块
//!
//! # 职责
//! 本模块承担 ReadFlow 的**内容获取层**，把"远端 Feed 文本"转换为可入库的结构化数据：
//! - 通过 HTTP 拉取 Feed 原文，并按 `Content-Type` 分派到 XML 或 JSON 解析器
//!   （[`fetch_feed`]）
//! - 解析 RSS 2.0 的 `<item>`（[`parse_xml_feed`]）与 JSON Feed 的 `items[]`
//!   （[`parse_json_feed`]），统一收敛为 [`ParsedItem`]
//! - 提供宽容的日期解析（[`parse_date`]）与 HTML 安全清洗（[`sanitize_html`]）
//! - 将解析结果写入数据库（[`store_items`]）
//!
//! # 设计意图
//! - **格式归一**：RSS / JSON Feed 的字段命名与可选性差异很大，模块对外只暴露
//!   一份 [`ParsedItem`] 结构，让上层的刷新流程（scheduler）不必感知源格式。
//! - **绝不因脏数据失败**：Feed 是外部不可信输入，缺标题、缺日期、缺 guid 都是常态。
//!   因此所有取值都用 `unwrap_or_default()` / 降级填充，而不是把错误抛给调用方；
//!   只有"整个文档无法解析"才返回 `Err`。
//! - **保留安全 HTML**：Feed 正文（content / description）来自不可信的第三方站点，
//!   入库前统一经 [`sanitize_html`]（ammonia 白名单）清洗——保留 `p` / `strong` /
//!   `a` / `img` / `h1`-`h6` / `ul` / `li` / `blockquote` 等排版标签以还原富文本样式，
//!   同时剥离 `<script>`、内联事件处理器（`onclick` 等）与危险 URL 协议，
//!   从而能在前端 `v-html` 渲染时既保留排版又杜绝脚本注入。
//!   白名单另放行 `video` / `source` 两个标签，但媒体 `src` 仅当主机名恰为
//!   `video.twimg.com` 时保留（Twitter 源的原生视频直链，见 [`sanitize_html`]）。
//! - **失败可观测**：抓取链路中的错误一律转成 `String`，便于 Tauri command
//!   直接回传前端提示，而调度器侧的失败只打印日志、不中断整体刷新。

use sqlx::SqlitePool;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use std::io::Cursor;
use ammonia::Builder;

use crate::db::Article;

/// 解析后的 RSS/Atom feed
///
/// 一次抓取的产物：源站点的标题 + 本次能识别出的全部条目。
#[derive(Debug, Clone)]
pub struct ParsedFeed {
    /// Feed 标题（取自 channel.title / JSON Feed 的 title）
    pub title: String,
    /// 订阅源图标 URL（可选）
    ///
    /// 优先取 channel 自带图标（RSS 2.0 的 `<image><url>` / JSON Feed 的 `favicon`），
    /// 缺省时由 [`fallback_icon`] 回退为源站点根路径的 `/favicon.ico`，
    /// 保证每个订阅源都有一个可尝试的图标地址；加载失败时由前端回退为首字母头像。
    pub icon: Option<String>,
    /// 解析后的文章列表（保持源文档中的原始顺序）
    pub items: Vec<ParsedItem>,
}

/// 解析后的单篇文章
///
/// 与 [`crate::db::Article`] 的区别：这里没有 `id` / `feed_id` / 已读收藏等
/// 本地状态字段，纯粹是"从远端读到的一篇内容"，入库时才由 `feed_id` 关联。
#[derive(Debug, Clone)]
pub struct ParsedItem {
    /// 唯一标识符（guid）
    ///
    /// 与 `feed_id` 一起构成数据库层 `UNIQUE(feed_id, guid)` 去重依据。
    pub guid: String,
    /// 文章标题
    pub title: String,
    /// 原文链接（阅读器"在浏览器打开"跳转让用）
    pub link: String,
    /// 摘要
    pub summary: String,
    /// 正文内容（HTML）
    ///
    /// 来自 Feed 的 `content` / `content:encoded`（优先）或 `description`（回退），
    /// 已通过 [`sanitize_html`] 清洗，保留安全标签、剥离脚本与事件属性。
    pub content: String,
    /// 作者
    pub author: String,
    /// 发布时间
    ///
    /// 注意该字段非空：源未提供或解析失败时会被填充为当前时间，
    /// 缺时间的条目因此会排在最前而不是被丢弃。
    pub published_at: DateTime<Utc>,
    /// 媒体附件 URL（播客音频 / 视频）
    ///
    /// 来自 RSS 2.0 的 `<enclosure url>` 或 JSON Feed 的 `attachments[].url`；
    /// 纯文字源没有附件时为 `None`，前端据此决定是否渲染播放器。
    pub enclosure_url: Option<String>,
    /// 附件 MIME 类型（如 `audio/mpeg`、`video/mp4`）
    ///
    /// RSS 取 `<enclosure type>`；该属性缺失时由 [`infer_enclosure_mime`] 按 URL
    /// 扩展名兜底推断，保证前端总能拿到一个可用类型来选 `<audio>` / `<video>`。
    pub enclosure_type: Option<String>,
}

/// 抓取并解析 RSS/Atom/JSON Feed
///
/// 先发起带自定义 UA 的 GET 请求（部分站点会拒绝空 UA 或默认爬虫 UA），
/// 再根据响应头 `Content-Type` 决定解析分支；头部缺失时按 XML 处理。
///
/// # 错误可定位性
/// 非 2xx 响应直接报「HTTP {状态码} {原因}」并附响应体片段；解析失败时嗅探响应体，
/// 区分「返回的是 HTML 网页」（需登录 / 实例错误页 / 被限流）与「返回的是 JSON 但
/// Content-Type 未标记」两种情况，避免把"HTML 错误页"和"Atom 文档"报成同一个
/// 无法区分的解析错误。
///
/// # 参数
/// * `url` - Feed 的 URL 地址
///
/// # 返回值
/// 解析后的 Feed 数据，包含标题和文章列表
///
/// # 错误
/// 网络请求失败、非 2xx 响应、响应体读取失败或文档解析失败时返回错误信息
pub async fn fetch_feed(url: &str) -> Result<ParsedFeed, String> {
    // 复用进程内共享客户端：一轮刷新会依次访问几十个订阅源，公用连接池可省下
    // 每个源一次 TCP + TLS 握手（超时档见 crate::net）
    let client = crate::net::feed_client();

    let resp = client
        .get(url)
        .header("User-Agent", "ReadFlow/1.0 (RSS Reader)")
        .send()
        .await
        .map_err(|e| network_error_message(&e, url))?;

    // status / headers 必须在 text() 消费 resp 之前取出
    let status = resp.status();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Cloudflare 挑战页的判定依据也在响应头里（`cf-mitigated` / `server`），
    // 同样要在 `text()` 之前取；取不到就留空，交由正文特征兜底。
    let cf_mitigated = resp
        .headers()
        .get("cf-mitigated")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let server = resp
        .headers()
        .get("server")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // 读 body 失败与"发请求失败"是两种情形：连接已建立、状态码也拿到了，
    // 是传输过程中断（大文件、长响应、对端提前关闭）。分开说，用户才知道
    // 该换网络还是该认为源本身有问题。
    let body = resp
        .text()
        .await
        .map_err(|e| format!("已连上但响应内容读取中断：{}", e))?;

    // 非 2xx：先报状态码与原因，再附响应体片段——限流页 / 实例错误页 / 需登录
    // 都会走到这里，没有状态码的话错误信息完全无法定位
    if !status.is_success() {
        let reason = status.canonical_reason().unwrap_or("Unknown");
        return Err(http_error_message(
            status.as_u16(),
            reason,
            &body,
            url,
            &ChallengeHeaders {
                cf_mitigated: &cf_mitigated,
                server: &server,
            },
        ));
    }

    // 根据内容类型选择解析器；只要头部里有 json 就走 JSON Feed 分支。
    // 解析失败时结合响应体补齐可读提示（HTML 错误页 / JSON 未标记类型）
    let mut parsed = if content_type.contains("json") {
        parse_json_feed(&body).map_err(|e| enrich_parse_error(e, &body, true))?
    } else {
        parse_xml_feed(&body).map_err(|e| enrich_parse_error(e, &body, false))?
    };

    // 图标兜底：channel 未自带图标时，回退为源站点根路径的 /favicon.ico。
    // 这里不发起额外的网络探测（避免为取图标增加请求），
    // 加载失败由前端回退为首字母头像，因此宁可存一个可能 404 的地址。
    if parsed.icon.is_none() {
        parsed.icon = fallback_icon(url);
    }

    Ok(parsed)
}

/// 把 reqwest 的错误翻译成人话，并在失败点是 RSSHub 实例时点明
///
/// # 参数
/// * `err` - 请求阶段的错误
/// * `url` - 本次请求的完整地址（用于判断它是不是 RSSHub 实例）
///
/// # 返回值
/// 可直接展示的错误说明
///
/// # 为什么要单独点明实例
/// RSSHub 源的错误里有两跳：**应用 → 实例** 与 **实例 → 原站**。只报「连接被中断」
/// 的话，用户会以为是原站挂了，跑去检查原站——而真正断掉的是实例那一跳。
/// 把实例名写进错误里，用户才知道该换实例还是该配代理。
fn network_error_message(err: &reqwest::Error, url: &str) -> String {
    build_network_error(
        classify_network_error(err.is_timeout(), err.is_connect(), &err.to_string()),
        url,
        &crate::net::effective_proxy_label(),
    )
}

/// 拼装网络层错误文案（纯函数，便于单测）
///
/// # 参数
/// * `base` - [`classify_network_error`] 给出的基础说明
/// * `url` - 本次请求的地址
/// * `route` - 本次连接实际走的路径（代理地址或「直连」，见 [`crate::net::effective_proxy_label`]）
///
/// # 返回值
/// 可直接展示的错误字符串
///
/// # 为什么要把「本次连接」写进去
/// 「浏览器能开、应用不能」有一半成因是**应用实际走的路和用户以为的不一样**：
/// 隐式环境变量代理、TUN 只覆盖了部分流量、代理规则把某个域名放行成直连……
/// 这些都不体现在任何界面元素上。把实际路径写进错误，用户不必再猜，
/// 也不必再让别人远程排查一轮。
fn build_network_error(base: String, url: &str, route: &str) -> String {
    let host = crate::rsshub::host_of(url).unwrap_or_default();
    if crate::rsshub::looks_like_rsshub_host(&host) {
        return format!(
            "RSSHub 实例 {} 连不上（断点在这一跳，与它要抓的原站无关）：{}（本次连接：{}）",
            host, base, route
        );
    }
    format!("{}（本次连接：{}）", base, route)
}

/// 把 reqwest 的连接级错误翻译成用户能据此采取行动的话
///
/// # 参数
/// * `is_timeout` - `reqwest::Error::is_timeout()`，任何阶段的超时
/// * `is_connect` - `reqwest::Error::is_connect()`，DNS / TCP / TLS 阶段就失败了
/// * `raw` - 原始错误文本（`reqwest::Error` 的 Display，含底层 hyper / TLS 原因）
///
/// # 返回值
/// 可直接展示在"失败原因"里的说明，末尾保留原始错误，便于回报问题时对得上号。
///
/// # 为什么要按字符串再分一层
/// `is_connect()` 把「域名解析不了」「端口没人监听」「TLS 被掐断」全归成一类，
/// 但这三种的下一步动作完全不同：换 DNS / 查端口 / 配代理。只给布尔标记的话
/// 只能得到一句"连接失败"，用户没有任何可执行动作。
///
/// 尤其 `unexpected EOF`：那是 TCP 已通、TLS 握手被对端掐断，成因基本只有两种
/// ——网络路径不通（要代理），或 TLS ClientHello 指纹被 WAF 识别（也要代理）。
/// 「浏览器能打开、应用打不开」正是这一条的典型症状，故文案直接把用户引到代理设置。
///
/// # 为什么连接超时不算"请求超时"
/// 连接阶段的超时同时满足 `is_connect` 与 `is_timeout`。若按超时优先处理，
/// 用户会看到"请求超时"而拿不到"可能需要代理"的提示——恰恰是最需要的那半句。
fn classify_network_error(is_timeout: bool, is_connect: bool, raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();

    if is_connect {
        if lower.contains("timed out") || lower.contains("timeout") {
            return format!(
                "连接超时：无法在限定时间内建立到该站点的连接。\
                 若浏览器能打开而应用不能，通常是当前网络需要代理——可在「设置 → 通用 → 网络代理」填写后重试。（原始错误：{}）",
                raw
            );
        }
        if lower.contains("unexpected eof")
            || lower.contains("connection reset")
            || lower.contains("broken pipe")
            || lower.contains("tls")
            || lower.contains("handshake")
        {
            return format!(
                "连接被中断（TLS 握手阶段）：连接已建立，但在握手过程中被对端切断。\
                 若浏览器能打开而应用不能，通常是需要走代理——可在「设置 → 通用 → 网络代理」填写后重试。（原始错误：{}）",
                raw
            );
        }
        if lower.contains("dns")
            || lower.contains("name or service")
            || lower.contains("failed to lookup")
        {
            return format!(
                "域名解析失败：当前网络找不到该站点（DNS 不可用或被污染）。\
                 可更换 DNS，或在「设置 → 通用 → 网络代理」配置代理后重试。（原始错误：{}）",
                raw
            );
        }
        if lower.contains("connection refused") {
            return format!(
                "连接被拒绝：该地址上没有服务在监听，请核对地址与端口是否正确。（原始错误：{}）",
                raw
            );
        }
        return format!(
            "无法建立连接：目标站点在当前网络下不可达。\
             可在「设置 → 通用 → 网络代理」配置代理后重试。（原始错误：{}）",
            raw
        );
    }

    if is_timeout {
        return format!(
            "请求超时：对端在限定时间内没有响应（该源站较慢，或被中间设备拦截）。（原始错误：{}）",
            raw
        );
    }

    // 其余错误（重定向策略、编码、协议层）照原样透出：它们不是网络可达性问题，
    // 编一句人话反而会盖掉真正有用的原始信息。
    format!("请求失败：{}", raw)
}

/// 判定"响应是不是 Cloudflare 人机验证页"所需的最小响应头集合
///
/// 单独成结构体而不是两个相邻的 `&str` 参数：同类型的两个参数最容易传反，
/// 而传反之后的现象是"挑战页识别时灵时不灵"，这种 bug 极难在测试之外发现。
struct ChallengeHeaders<'a> {
    /// `cf-mitigated` 响应头（Cloudflare 挑战页会写 `challenge`）
    cf_mitigated: &'a str,
    /// `server` 响应头（挑战页通常是 `cloudflare`）
    server: &'a str,
}

/// 判断该响应是否为 Cloudflare 人机验证页
///
/// # 参数
/// * `status` - HTTP 状态码
/// * `body` - 响应体原文
/// * `headers` - 判定所需响应头
///
/// # 返回值
/// 是挑战页返回 `true`，其余一律 `false`
///
/// # 为什么要单独判
/// 挑战页是 `403` + 一段要求执行 JS 的 HTML。若归进「返回网页而非订阅内容」，
/// 用户会以为源坏了或地址填错了，而真正该做的往往是**换实例**——这个判断
/// 决定下一步动作，不能省。
///
/// `cf-mitigated` 是 Cloudflare 主动标注的确定信号，优先采信；正文特征只作兜底：
/// 万一响应头被中间设备剥掉，仍能凭 "Just a moment" 认出来。
fn is_cloudflare_challenge(status: u16, body: &str, headers: &ChallengeHeaders<'_>) -> bool {
    if status != 403 {
        return false;
    }
    if headers
        .cf_mitigated
        .to_ascii_lowercase()
        .contains("challenge")
    {
        return true;
    }
    headers.server.eq_ignore_ascii_case("cloudflare")
        && body.to_ascii_lowercase().contains("just a moment")
}

/// 构造非 2xx 响应的错误信息
///
/// # 参数
/// * `status` - HTTP 状态码（如 403）
/// * `reason` - 状态原因短语（如 "Forbidden"）
/// * `body` - 响应体原文（内部会压缩空白并截断）
/// * `url` - 本次请求的地址（判断是否 RSSHub 实例，决定能否解析上游失败）
/// * `headers` - 判定 Cloudflare 挑战页所需的响应头
///
/// # 返回值
/// 可直接展示给用户的错误字符串
///
/// # 判定顺序
/// Cloudflare 挑战 → RSSHub 上游失败 → 通用格式。挑战页必须排在最前：
/// 它同样是 403，但结论与"被站点拒绝"完全不同——站点是活的，只是挡住程序。
///
/// # 为什么 RSSHub 的错误页要单独处理
/// 实例返回 503 时，真正失败的很可能是**它去抓原站**那一步，而不是实例自身。
/// 这两种处境用户的下一步完全不同（换实例 vs 换不到也别折腾），而 RSSHub 的
/// 错误页里就写着失败的上游地址，解析出来比丢一段 HTML 片段有用得多。
fn http_error_message(
    status: u16,
    reason: &str,
    body: &str,
    url: &str,
    headers: &ChallengeHeaders<'_>,
) -> String {
    let host = crate::rsshub::host_of(url).unwrap_or_default();

    if is_cloudflare_challenge(status, body, headers) {
        // 说清两件事：断在哪一环（人机验证，不是网络、不是源没了）、下一步做什么。
        // RSSHub 实例与普通站点的话术要分开——对前者"换实例"是对的处方，
        // 对后者说"换实例"等于让人去改一个并不存在的设置。
        return if crate::rsshub::looks_like_rsshub_host(&host) {
            format!(
                "HTTP {} {}：RSSHub 实例 {} 被 Cloudflare 人机验证拦住了（响应是 \"Just a moment...\" 挑战页），\
                 程序无法通过这种验证，换一个实例通常就好（本次连接：{}）",
                status,
                reason,
                host,
                crate::net::effective_proxy_label()
            )
        } else {
            format!(
                "HTTP {} {}：该站点被 Cloudflare 人机验证拦住了（响应是 \"Just a moment...\" 挑战页），\
                 程序无法通过这种验证；源本身通常还活着，可在浏览器里打开确认（本次连接：{}）",
                status,
                reason,
                crate::net::effective_proxy_label()
            )
        };
    }
    if crate::rsshub::looks_like_rsshub_host(&host) {
        if let Some(failure) = crate::rsshub::parse_upstream_failure(body) {
            let mut msg = format!(
                "HTTP {} {}：RSSHub 实例自己抓不到上游 {}",
                status, reason, failure.upstream
            );
            if !failure.reason.is_empty() {
                msg.push_str(&format!("（实例给出的原因：{}）", failure.reason));
            }
            // 换实例有没有用，取决于上游是否对所有实例都不可达。
            // 公共实例大多同处一地，写清这一点能省掉用户逐个试的时间。
            msg.push_str("。这类失败与实例是否存活无关，换一个实例通常也一样");
            return msg;
        }
    }
    format!(
        "HTTP {} {}（响应内容：{}）",
        status,
        reason,
        body_snippet(body, 120)
    )
}

/// 解析失败时结合响应体给出更可定位的错误信息
///
/// 嗅探响应体开头（跳过 BOM / 空白）：
/// - 像 HTML（`<!doctype html` / 含 `<html`）→ 提示"返回的是网页而非订阅内容"；
/// - 像 JSON（`{` / `[` 开头）且本次走的是 XML 分支 → 提示"是 JSON 但 Content-Type 未标记"；
/// - 其它 → 保留原解析错误并附响应体片段。
///
/// # 参数
/// * `err` - 原始解析错误
/// * `body` - 响应体原文
/// * `json_branch` - 本次解析是否走的 JSON 分支
///
/// # 返回值
/// 补全提示后的错误字符串
fn enrich_parse_error(err: String, body: &str, json_branch: bool) -> String {
    let head = body_head(body, 200);
    let lower = head.to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.contains("<html") {
        return format!(
            "{}——该地址返回的是网页而非订阅内容（可能需要登录、实例错误页或被限流）",
            err
        );
    }
    if !json_branch && (head.starts_with('{') || head.starts_with('[')) {
        return format!("{}——返回的是 JSON，但 Content-Type 未标记为 json", err);
    }
    format!("{}（响应片段：{}）", err, body_snippet(body, 120))
}

/// 取响应体的可读片段：跳过 BOM，把换行 / 连续空白压成单空格，截断到 `max_chars` 字符
///
/// # 参数
/// * `body` - 响应体原文
/// * `max_chars` - 最多保留的字符数（按 Unicode 标量计）
///
/// # 返回值
/// 压平并截断后的单行片段
fn body_snippet(body: &str, max_chars: usize) -> String {
    body.trim_start_matches('\u{feff}')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

/// 取响应体开头（跳过 BOM 与前导空白），用于"是否像 HTML / JSON"的判断
///
/// # 参数
/// * `body` - 响应体原文
/// * `max_chars` - 最多保留的字符数
///
/// # 返回值
/// 跳过 BOM 与空白、截断后的开头字符串（保留原始空白，仅用于前缀判断）
fn body_head(body: &str, max_chars: usize) -> String {
    body.trim_start_matches('\u{feff}')
        .trim_start()
        .chars()
        .take(max_chars)
        .collect()
}

/// 从 Feed URL 推导兜底图标地址
///
/// 站点根路径的 `/favicon.ico` 是事实上的约定，绝大多数站点都支持；
/// 这里只做字符串/URL 推导、不发网络请求，加载失败由前端回退首字母头像。
/// 对 RSSHub 路由类地址，推导结果是 RSSHub 实例站点的图标，属可接受的降级表现。
///
/// # 参数
/// * `feed_url` - Feed 的 URL 地址
///
/// # 返回值
/// 形如 `https://example.com/favicon.ico` 的图标地址；URL 非法或非 http(s) 时返回 `None`
pub fn fallback_icon(feed_url: &str) -> Option<String> {
    // 只认 http(s)：非标准地址（如测试用的裸字符串）推不出有意义的图标
    url::Url::parse(feed_url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        // origin() 的序列化包含 scheme://host[:port]，比手拼更可靠
        .map(|u| format!("{}/favicon.ico", u.origin().ascii_serialization()))
}

/// 仅根据 URL 扩展名推断附件 MIME 类型
///
/// 部分 RSS 2.0 feed 的 `<enclosure>` 不带 `type` 属性，前端又必须拿到一个
/// 类型来决定渲染 `<audio>` 还是 `<video>`，于是用扩展名兜底。匹配不到时回退为
/// `application/octet-stream`（前端按"未知附件"处理，仍给出下载入口）。
///
/// # 参数
/// * `url` - 附件 URL
///
/// # 返回值
/// 形如 `audio/mpeg` 的 MIME 类型字符串
fn infer_enclosure_mime(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        "audio/mpeg".into()
    } else if lower.ends_with(".m4a") {
        "audio/mp4".into()
    } else if lower.ends_with(".oga") || lower.ends_with(".ogg") {
        "audio/ogg".into()
    } else if lower.ends_with(".wav") {
        "audio/wav".into()
    } else if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
        "video/mp4".into()
    } else if lower.ends_with(".webm") {
        "video/webm".into()
    } else if lower.ends_with(".mov") {
        "video/quicktime".into()
    } else {
        "application/octet-stream".into()
    }
}

/// 解析 XML 格式的 RSS / Atom feed（按根标签分派）
///
/// `rss` crate 只认 `<rss>` 根标签，Atom 的 `<feed>` 会直接报
/// "the input did not begin with an rss tag"（Feed 添加时表现为"该源无法解析"）。
/// 本函数先轻量检测根元素名：`rss` → [`parse_rss_feed`]、`feed` → [`parse_atom_feed`]；
/// 检测不出时两个解析器都试一遍取成功者，都失败则返回聚合两个分支原因的错误。
///
/// # 参数
/// * `content` - Feed 原始 XML 文本
///
/// # 返回值
/// 解析后的 Feed 数据；两个分支都失败时返回组合错误
fn parse_xml_feed(content: &str) -> Result<ParsedFeed, String> {
    match detect_xml_root(content).as_deref() {
        Some("rss") => parse_rss_feed(content),
        Some("feed") => parse_atom_feed(content),
        _ => {
            // 根标签既非 rss 也非 feed（或检测不出）：两个解析器都试，取成功者。
            // 检测器只做"跳过 BOM / 声明 / 注释后看首个标签"的轻量判断，边角形态可能漏判，
            // 故此处兜底再试，不因检测器的局限误拒合法文档。
            let rss_err = match parse_rss_feed(content) {
                Ok(parsed) => return Ok(parsed),
                Err(e) => e,
            };
            match parse_atom_feed(content) {
                Ok(parsed) => Ok(parsed),
                Err(atom_err) => Err(format!(
                    "无法解析为 RSS 或 Atom（RSS 分支：{}；Atom 分支：{}）",
                    rss_err, atom_err
                )),
            }
        }
    }
}

/// 轻量检测 XML 文档的根元素名（小写）
///
/// 依次跳过 UTF-8 BOM、空白、`<?xml ... ?>` 声明 / 处理指令、`<!-- ... -->` 注释与
/// `<!DOCTYPE ...>`，然后读取首个元素的标签名。仅用于"分派给哪个解析器"的粗判，
/// 不做完整 XML 校验——真正的合法性由两个解析库各自保证。
///
/// # 参数
/// * `content` - Feed 原始 XML 文本
///
/// # 返回值
/// 根元素名（小写）；空文档 / 非 `<` 开头 / 畸形而无法定位时返回 `None`
fn detect_xml_root(content: &str) -> Option<String> {
    let bytes = content.as_bytes();
    let mut i = if content.starts_with('\u{feff}') { 3 } else { 0 };
    while i < bytes.len() {
        // 跳过标签之间的空白
        while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'<' {
            return None;
        }
        i += 1;
        match bytes.get(i).copied()? {
            // 处理指令 / XML 声明：跳到 `?>`
            b'?' => {
                i += content[i..].find("?>")? + 2;
            }
            // 注释 `<!-- -->` 或 `<!DOCTYPE ...>`：分别跳到各自结尾
            b'!' => {
                let rest = &content[i..];
                if let Some(after_open) = rest.strip_prefix("!--") {
                    // after_open = 注释体 + "-->"（3 为 "!--" 的长度）
                    i += 3 + after_open.find("-->")? + 3;
                } else {
                    i += rest.find('>')? + 1;
                }
            }
            // 普通元素起始：读取标签名
            _ => {
                let start = i;
                while i < bytes.len() {
                    let c = bytes[i] as char;
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                if i == start {
                    return None;
                }
                return Some(content[start..i].to_ascii_lowercase());
            }
        }
    }
    None
}

/// 解析 RSS 2.0 / 0.9x 的 `<rss>` 文档
///
/// 使用 `rss` crate 读取 `<channel>`，逐条把 `<item>` 映射为 [`ParsedItem`]。
/// 各字段都做了缺省兜底，单个条目的字段缺失不会导致整篇 Feed 解析失败。
///
/// # 参数
/// * `content` - Feed 原始 XML 文本
///
/// # 返回值
/// 解析后的 Feed 数据；文档结构非法时返回错误
fn parse_rss_feed(content: &str) -> Result<ParsedFeed, String> {
    let channel = rss::Channel::read_from(Cursor::new(content))
        .map_err(|e| format!("RSS 解析失败: {}", e))?;

    let mut items = Vec::new();
    for item in channel.items() {
        // guid 缺失时留空字符串，由数据库层的唯一约束负责去重
        let guid = item
            .guid()
            .map(|g| g.value().to_string())
            .unwrap_or_default();
        let link = item.link().unwrap_or_default().to_string();

        // 优先使用 content（通常是 content:encoded / 完整正文），为空时回退到 description。
        // 注意这里调用的是 sanitize_html（ammonia 白名单）而非剥离成纯文本：
        // 保留 p / strong / a / img / h1-h6 / ul / li 等排版标签以还原富文本样式，
        // 同时由 ammonia 剥离 <script> / on* 事件处理器 / 危险 URL 协议，
        // 保证交给前端 v-html 渲染时既有排版又安全。
        let content = sanitize_html(item.content().unwrap_or_default());
        let content = if content.is_empty() {
            sanitize_html(item.description().unwrap_or_default())
        } else {
            content
        };

        // enclosure：播客音频 / 视频。rss crate 的 `Item::enclosure()` 返回
        // `Option<Enclosure>`；`Enclosure::url()` / `Enclosure::mime_type()` 直接返回 `&str`
        // （空串表示缺失），URL 缺失时视为无附件，mime_type 缺失则按 URL 扩展名兜底，
        // 保证前端总能拿到一个类型来选 <audio> / <video>。
        let (enclosure_url, enclosure_type) = match item.enclosure() {
            Some(e) => {
                let url = Some(e.url().to_string()).filter(|u| !u.trim().is_empty());
                let mime = if !e.mime_type().trim().is_empty() {
                    Some(e.mime_type().to_string())
                } else {
                    url.as_ref().map(|u| infer_enclosure_mime(u))
                };
                (url, mime)
            }
            None => (None, None),
        };

        items.push(ParsedItem {
            guid,
            title: item.title().unwrap_or_default().to_string(),
            link: link.clone(),
            // summary 与 content 均来自不可信的外部站点，入库前统一过一遍安全清洗，
            // 保留安全标签、剥离脚本与事件属性，避免 v-html 渲染时注入脚本
            summary: sanitize_html(item.description().unwrap_or_default()),
            content,
            author: item.author().unwrap_or_default().to_string(),
            // 没有 pub_date 或格式无法识别时用当前时间兜底，保证条目仍会入库
            published_at: item
                .pub_date()
                .map(|s| parse_date(s))
                .unwrap_or_else(Utc::now),
            enclosure_url,
            enclosure_type,
        });
    }

    // channel 自带图标：RSS 2.0 的 <image><url>；缺失时由 fetch_feed 做 favicon 兜底
    let icon = channel
        .image()
        .map(|img| img.url().to_string())
        .filter(|u| !u.trim().is_empty());

    Ok(ParsedFeed {
        title: channel.title().to_string(),
        icon,
        items,
    })
}

/// 解析 Atom 1.0 的 `<feed>` 文档
///
/// 各字段都做了缺省兜底，单个条目的字段缺失不会导致整篇 Feed 解析失败。
/// 所有 HTML 字段（content / summary）与 RSS 路径同样过 [`sanitize_html`]，
/// 保证交给前端 `v-html` 渲染时既有排版又安全。
///
/// # 字段映射
/// - 标题：`Feed::title().as_str()`；图标：`Feed::icon()`（可能相对 URL，保持原样）
/// - guid：`Entry::id()`；标题：`Entry::title().as_str()`
/// - link：优先 `rel` 为 `alternate` / 空的 link，兜底首个 link
/// - content：优先 `Entry::content().value()`，兜底 `Entry::summary()`
/// - author：`Entry::authors()` 首个元素的 name
/// - 时间：优先 `published()`，兜底 `updated()`
/// - enclosure：`<link rel="enclosure">` 的 href；mime 取 link 的 `type`，
///   缺省时按 URL 扩展名兜底（与 RSS 路径行为一致）
///
/// # 参数
/// * `content` - Feed 原始 XML 文本
///
/// # 返回值
/// 解析后的 Feed 数据；文档结构非法时返回错误
fn parse_atom_feed(content: &str) -> Result<ParsedFeed, String> {
    let feed = atom_syndication::Feed::read_from(Cursor::new(content))
        .map_err(|e| format!("Atom 解析失败: {}", e))?;

    let mut items = Vec::new();
    for entry in feed.entries() {
        // guid：Atom 的 id 是必填且要求稳定，直接作为去重依据
        let guid = entry.id().to_string();
        let title = entry.title().as_str().to_string();

        // link：优先 alternate（官方的"可读页面"链接），rel 缺省（空串）也视为 alternate；
        // 都没有时退到首个 link，保证不会因 rel 命名差异而丢链接
        let link = entry
            .links()
            .iter()
            .find(|l| matches!(l.rel(), "alternate" | ""))
            .or_else(|| entry.links().first())
            .map(|l| l.href().to_string())
            .unwrap_or_default();

        // 正文：优先 content（inline HTML 正文），为空时回退 summary，
        // 与 RSS 路径的"content 优先、description 回退"保持一致的语义
        let raw_content = entry
            .content()
            .and_then(|c| c.value())
            .unwrap_or_default();
        let raw_summary = entry.summary().map(|s| s.as_str()).unwrap_or_default();
        let content = sanitize_html(raw_content);
        let content = if content.is_empty() {
            sanitize_html(raw_summary)
        } else {
            content
        };

        let author = entry
            .authors()
            .first()
            .map(|p| p.name().to_string())
            .unwrap_or_default();

        // 时间：优先 published（首次发布），回退 updated（最后修改）；
        // FixedDateTime = DateTime<FixedOffset>，转 RFC3339 字符串后复用 parse_date
        let published_at = entry
            .published()
            .or(Some(entry.updated()))
            .map(|d| parse_date(&d.to_rfc3339()))
            .unwrap_or_else(Utc::now);

        // enclosure：Atom 用 <link rel="enclosure"> 表达附件；
        // mime 缺失时按 URL 扩展名兜底，保证前端总能选 <audio>/<video>
        let (enclosure_url, enclosure_type) = match entry
            .links()
            .iter()
            .find(|l| l.rel() == "enclosure")
        {
            Some(l) => {
                let url = Some(l.href().to_string()).filter(|u| !u.trim().is_empty());
                let mime = l
                    .mime_type()
                    .map(|m| m.to_string())
                    .filter(|m| !m.trim().is_empty())
                    .or_else(|| url.as_ref().map(|u| infer_enclosure_mime(u)));
                (url, mime)
            }
            None => (None, None),
        };

        items.push(ParsedItem {
            guid,
            title,
            link,
            // summary 与 content 均来自不可信外部站点，入库前统一清洗防 XSS
            summary: sanitize_html(raw_summary),
            content,
            author,
            published_at,
            enclosure_url,
            enclosure_type,
        });
    }

    // feed 自带图标：Atom 的 <icon>；缺失时由 fetch_feed 做 favicon 兜底
    let icon = feed
        .icon()
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());

    Ok(ParsedFeed {
        title: feed.title().as_str().to_string(),
        icon,
        items,
    })
}

///
/// 宽松读取 JSON Feed 规范的常用字段：条目在 `items` 数组下，
/// 标题取 `title`，链接优先 `url`、回退 `external_url`，正文优先
/// `content_html`、回退 `summary`，时间优先 `date_published`、回退 `date_modified`。
///
/// `items` 缺失或类型不符时不报错，返回空条目列表——视为"源当前无内容"。
///
/// # 参数
/// * `content` - Feed 原始 JSON 文本
///
/// # 返回值
/// 解析后的 Feed 数据；JSON 语法错误时返回错误信息
fn parse_json_feed(content: &str) -> Result<ParsedFeed, String> {
    let data: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("JSON 解析失败: {}", e))?;

    let mut items = Vec::new();
    if let Some(items_arr) = data.get("items").and_then(|v| v.as_array()) {
        for item in items_arr {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled")
                .to_string();
            let link = item
                .get("url")
                .or_else(|| item.get("external_url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // 规范里 id 是必填且要求稳定；缺失时用链接顶替，避免出现空 guid
            let id_val = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(link);
            let summary = item
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content_html = item
                .get("content_html")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // JSON Feed 的 author 是对象，取其中的 name 字段
            let author = item
                .get("author")
                .and_then(|a| a.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let date = item
                .get("date_published")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("date_modified").and_then(|v| v.as_str()))
                .unwrap_or("");

            // 提前各清洗一次，避免下方回退分支重复调用（ammonia 解析有成本）
            let summary_html = sanitize_html(summary);
            let content_html_sanitized = sanitize_html(content_html);
            // enclosure：JSON Feed 的 `attachments[]` 里取首个 audio/video 附件。
            // 只认 audio/* / video/*，忽略图片等其它类型；mime_type 缺失时按 URL 扩展名兜底。
            let (enclosure_url, enclosure_type) = item
                .get("attachments")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter().find(|a| {
                        let mime = a.get("mime_type").and_then(|m| m.as_str()).unwrap_or("");
                        mime.starts_with("audio/") || mime.starts_with("video/")
                    })
                })
                .map(|a| {
                    let url = a
                        .get("url")
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                        .filter(|s| !s.trim().is_empty());
                    let mime = a
                        .get("mime_type")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| url.as_ref().map(|u| infer_enclosure_mime(u)));
                    (url, mime)
                })
                .unwrap_or((None, None));
            items.push(ParsedItem {
                guid: id_val.to_string(),
                title,
                link: link.to_string(),
                // summary 与 content 均来自不可信外部站点，入库前统一清洗防 XSS；
                // 保留安全标签（p/strong/a/img/h1-h6/ul/li 等）以还原富文本排版
                summary: summary_html.clone(),
                // 如果 content_html 为空，使用 summary 作为正文
                content: if content_html_sanitized.is_empty() {
                    summary_html
                } else {
                    content_html_sanitized
                },
                author: author.to_string(),
                published_at: if !date.is_empty() {
                    parse_date(date)
                } else {
                    Utc::now()
                },
                enclosure_url,
                enclosure_type,
            });
        }
    }

    // JSON Feed 规范的 favicon / icon 字段（均可选，优先 favicon）；缺失时由 fetch_feed 兜底
    let icon = data
        .get("favicon")
        .or_else(|| data.get("icon"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(ParsedFeed {
        title: data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("JSON Feed")
            .to_string(),
        icon,
        items,
    })
}

/// 解析日期字符串为 UTC 时间
///
/// Feed 里的时间格式极不统一，故按命中率从高到低依次尝试 RFC 2822（RSS 规定格式）、
/// RFC 3339 / ISO 8601、两种常见的无时区写法，最后退到纯日期。
/// 全部失败时返回当前时间：宁可时间不准，也不要丢掉这条文章。
///
/// # 参数
/// * `s` - 待解析的日期字符串
///
/// # 返回值
/// UTC 时区的时间；无法识别时返回当前时间
fn parse_date(s: &str) -> DateTime<Utc> {
    // 尝试多种日期格式
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return dt.and_utc();
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return dt.and_utc();
    }
    // 纯日期补零时刻；and_hms_opt 理论上不会失败（0,0,0 恒合法），仍留兜底
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc())
            .unwrap_or_else(Utc::now);
    }
    Utc::now()
}

/// 对外部 HTML 做安全清洗（XSS 防护）
///
/// 文章正文 / 摘要来自不可信的第三方站点，最终会通过前端的 `v-html` 注入 DOM，
/// 因此入库前必须剥离 `<script>`、内联事件处理器（`onclick` 等）与危险 URL 协议。
/// 使用 ammonia 的默认白名单：保留 `p` / `a` / `b` / `i` / `img` / `blockquote` /
/// `ul` / `li` / `h1`-`h6` 等排版标签，移除所有脚本与事件属性，从而在还原富文本排版的同时
/// 杜绝脚本注入。
/// 链接统一追加 `rel="noopener noreferrer"`，防止 `target="_blank"` 的 tabnabbing 攻击。
///
/// # 视频媒体放行（Twitter 源）
/// 默认白名单不含 `video` / `source`——RSSHub 的 Twitter 路由把推文视频渲染为原生
/// `<video src="https://video.twimg.com/...mp4" controls>`（非 iframe），此前整个标签
/// 被剥掉，用户读推文源看不到任何视频。这里显式放行两个标签及其 `src` 等属性，
/// 但媒体 `src` 仅当主机名**恰为** `video.twimg.com` 时保留：既让 Twitter 视频可播，
/// 又避免任意域名媒体加载与跟踪画像。主机名经 `url` crate 解析后严格相等判断，
/// userinfo@ 欺骗、子域变体与尾点 FQDN 都会被解析出不同主机名而拒绝。
///
/// **只对重新抓取的文章生效**：存量文章的正文在入库时就已按旧白名单净化，
/// 且数据库无原始 HTML 备份列，被剥掉的内容不可逆——这是已知限制。
///
/// # 图片尺寸处理
/// 第三方 RSS 常在 `<img>` 上写死 `width` / `height`（如 Substack 的 1456px 宽），
/// 若不剥离，前端即便用 `max-width:100%` 也会被固有像素尺寸撑破布局、产生横向滚动条。
/// 因此在清洗时通过 `attribute_filter` 直接丢弃 `img` 的 `width` / `height`，
/// 让图片在前端只受 CSS 约束（自适应容器宽度、等比缩放），既美观又不会溢出。
///
/// # 参数
/// * `html` - 待清洗的 HTML 片段
///
/// # 返回值
/// 清洗后的安全 HTML 字符串
///
/// 可见性标注为 `pub(crate)`：除本模块入库清洗外，[`crate::ai`] 的翻译链路
/// 也需要把模型返回的 HTML 译文做同样的安全清洗（保留标签结构、剥离脚本），
/// 因此提升为模块间共享，避免逻辑重复。
pub(crate) fn sanitize_html(html: &str) -> String {
    Builder::default()
        // 放行 <video> / <source>（Twitter 源的原生视频直链，见函数文档）。
        // 仅放行标签不够——属性同样要走白名单，故 src / controls / type 逐个补上。
        .add_tags(["video", "source"])
        .add_tag_attributes("video", ["src", "controls"])
        .add_tag_attributes("source", ["src", "type"])
        .link_rel(Some("noopener noreferrer"))
        // 丢弃 <img> 的 width / height 固有尺寸，交由前端 CSS 控制自适应宽度
        // attribute_filter 的返回值类型是 Option<Cow<str>>，故保留的属性需包成 Cow::Borrowed
        .attribute_filter(|element, attribute, value| {
            if element == "img" && (attribute == "width" || attribute == "height") {
                None
            } else if (element == "video" || element == "source") && attribute == "src" {
                // 媒体 src 只放行受信的 Twitter 视频 CDN：严格主机名相等，
                // 不用宽后缀匹配（pbs.twimg.com 等非视频子域无必要放行）
                if is_trusted_twimg_video_src(value) {
                    Some(std::borrow::Cow::Borrowed(value))
                } else {
                    None
                }
            } else {
                Some(std::borrow::Cow::Borrowed(value))
            }
        })
        .clean(html)
        .to_string()
}

/// 判断视频媒体 URL 是否来自受信的 Twitter 视频 CDN
///
/// 严格三项：协议 `https`、主机名恰为 `video.twimg.com`。经 `url` crate 解析后
/// 比对，因此 `https://video.twimg.com@evil.com/`（userinfo 欺骗）、
/// `https://video.twimg.com.evil.com/`（子域绕过）、`https://VIDEO.TWIMG.COM./x`
/// （大小写 / 尾点 FQDN）等变体都会解析出不同主机名而被拒绝。
///
/// # 参数
/// * `value` - `<video>` / `<source>` 的 `src` 属性原值
///
/// # 返回值
/// 受信返回 `true`；不可解析或主机名不符返回 `false`
fn is_trusted_twimg_video_src(value: &str) -> bool {
    match url::Url::parse(value.trim()) {
        Ok(u) => u.scheme() == "https" && u.host_str() == Some("video.twimg.com"),
        Err(_) => false,
    }
}

/// 将解析后的文章存储到数据库
///
/// 本模块对数据库细节的收口点：刷新流程只需持有 `pool` 与 `feed_id`，
/// 无需了解 `articles` 表的列结构。去重与写入量封顶都在
/// [`Article::batch_insert`] 内部完成（`INSERT OR IGNORE` + 单次最多 30 条）。
/// 入库前会一次性加载启用中的过滤规则与源的文件夹，
/// 供 [`Article::batch_insert`] 对**新建**文章求值并施加动作（规则引擎）。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `feed_id` - 订阅源 ID
/// * `feed_name` - 订阅源名称（供 `field=feed` 的规则匹配）
/// * `items` - 解析后的文章列表
///
/// # 返回值
/// 新插入的文章数量（已存在于库中的条目不计入）
///
/// # 错误
/// 数据库写入失败时返回错误信息
pub async fn store_items(
    pool: &SqlitePool,
    feed_id: i64,
    feed_name: &str,
    items: &[ParsedItem],
) -> Result<i64, String> {
    // 入库前一次性加载规则与文件夹，避免在 batch_insert 循环里反复查库；
    // 加载失败（极少见）降级为空规则集/未分类，保证刷新流程不中断。
    let filters = crate::db::FilterRule::list_enabled(pool).await.unwrap_or_default();
    let folder_id = crate::db::Feed::folder_id_of(pool, feed_id).await.unwrap_or(0);
    Article::batch_insert(pool, feed_id, feed_name, folder_id, &filters, &items.to_vec()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `unexpected EOF` 是"TCP 通了、TLS 被掐断"，必须把用户引到代理设置
    ///
    /// 这是用户实际报过的那条错误（forbes.com）。文案里若不出现「代理」二字，
    /// 用户只会以为源坏了，而这一类的成因几乎都在网络路径上。
    #[test]
    fn tls_eof_points_at_proxy_setting() {
        let msg = classify_network_error(
            false,
            true,
            "error trying to connect: unexpected EOF",
        );
        assert!(msg.contains("握手"), "应点明发生在握手阶段: {msg}");
        assert!(msg.contains("代理"), "应给出可执行动作（代理）: {msg}");
        assert!(msg.contains("unexpected EOF"), "应保留原始错误便于排查: {msg}");
    }

    /// 连接阶段的超时也是"连不上"，不能只说"请求超时"而丢掉代理提示
    ///
    /// 连接超时同时满足 is_connect 与 is_timeout，按超时优先会把最需要的半句吃掉。
    #[test]
    fn connect_timeout_still_mentions_proxy() {
        let msg = classify_network_error(
            true,
            true,
            "error trying to connect: connection timed out",
        );
        assert!(msg.contains("连接超时"), "应区分于请求超时: {msg}");
        assert!(msg.contains("代理"), "连接超时同样要提示代理: {msg}");
    }

    /// 只在总超时里失败（连接已建立）时，不硬塞代理建议
    #[test]
    fn plain_request_timeout_has_no_proxy_advice() {
        let msg = classify_network_error(true, false, "operation timed out");
        assert!(msg.contains("请求超时"), "{msg}");
        assert!(!msg.contains("代理"), "连接已建立时误导用户去配代理是错的: {msg}");
    }

    /// DNS 失败与"端口没人监听"要给出不同动作，否则等于没说
    #[test]
    fn dns_and_refused_are_told_apart() {
        let dns = classify_network_error(false, true, "dns error: failed to lookup address");
        assert!(dns.contains("域名解析"), "{dns}");

        let refused = classify_network_error(false, true, "error trying to connect: connection refused");
        assert!(refused.contains("没有服务在监听"), "{refused}");
        assert!(!refused.contains("代理"), "端口填错时提代理是噪音: {refused}");
    }

    /// 认不出的错误原样透出，不编造解释
    #[test]
    fn unknown_error_is_passed_through() {
        let msg = classify_network_error(false, false, "builder error: bad header value");
        assert!(msg.starts_with("请求失败："), "{msg}");
        assert!(msg.contains("bad header value"), "{msg}");
    }

    /// 普通源的错误要带上「本次连接走哪条路」
    ///
    /// 「浏览器能开、应用不能」有一半是"实际走的路和以为的不一样"，
    /// 不写出来用户只能猜。
    #[test]
    fn network_error_reports_effective_route() {
        let msg = build_network_error(
            "连接被中断".into(),
            "https://example.com/feed",
            "http://127.0.0.1:7899",
        );
        assert!(msg.contains("连接被中断"), "{msg}");
        assert!(
            msg.contains("本次连接：http://127.0.0.1:7899"),
            "要写出实际路径: {msg}"
        );
        assert!(!msg.contains("RSSHub"), "普通源不该被判成实例: {msg}");
    }

    /// RSSHub 源要同时说清「断在这一跳」与「走的是哪条路」
    #[test]
    fn rsshub_network_error_names_instance_and_route() {
        let msg = build_network_error(
            "连接被中断".into(),
            "https://rsshub.app/latepost/4",
            "直连",
        );
        assert!(msg.contains("RSSHub 实例 rsshub.app"), "{msg}");
        assert!(msg.contains("与它要抓的原站无关"), "{msg}");
        assert!(msg.contains("本次连接：直连"), "{msg}");
    }

    /// Twitter 源的原生视频：video 标签、src 与 controls 都应保留
    /// （RSSHub twitter 路由的实际产物形态，见函数文档）
    #[test]
    fn sanitize_html_keeps_trusted_twimg_video() {
        let html = r#"<p>前</p><video src="https://video.twimg.com/ext_tw_video/123/pu/vid/640x360/x.mp4" controls></video><p>后</p>"#;
        let out = sanitize_html(html);
        assert!(out.contains("<video"), "video 标签应保留: {out}");
        assert!(
            out.contains("https://video.twimg.com/ext_tw_video/123/pu/vid/640x360/x.mp4"),
            "受信 src 应保留: {out}"
        );
        assert!(out.contains("controls"), "controls 属性应保留: {out}");
        assert!(out.contains("前") && out.contains("后"));
    }

    /// 不可信主机名的 src 必须被剥掉，但 video 标签本身保留
    /// （标签保留是为了不破坏正文结构；媒体加载不了，页面不会挂）
    #[test]
    fn sanitize_html_strips_untrusted_video_src() {
        let html = r#"<video src="https://evil.com/x.mp4" controls></video>"#;
        let out = sanitize_html(html);
        assert!(out.contains("<video"), "video 标签应保留: {out}");
        assert!(!out.contains("evil.com"), "不可信 src 应被剥掉: {out}");
        assert!(!out.contains("src="), "src 属性应整体消失: {out}");
    }

    /// userinfo 欺骗：主机名实为 evil.com，必须拒绝
    /// （若用字符串前缀判断就会误放行，这是走 URL 解析的原因）
    #[test]
    fn sanitize_html_strips_video_src_userinfo_spoof() {
        let html = r#"<video src="https://video.twimg.com@evil.com/x.mp4"></video>"#;
        let out = sanitize_html(html);
        assert!(!out.contains("evil.com"), "userinfo 欺骗应被拒绝: {out}");
        assert!(!out.contains("src="), "src 属性应整体消失: {out}");
    }

    /// source 标签（多码率兜底形态）：受信 src 与 type 保留，不可信 src 被剥
    #[test]
    fn sanitize_html_handles_source_tag() {
        let ok = sanitize_html(r#"<video controls><source src="https://video.twimg.com/a.mp4" type="video/mp4"></video>"#);
        assert!(ok.contains("<source"), "source 标签应保留: {ok}");
        assert!(ok.contains("https://video.twimg.com/a.mp4"), "受信 source src 应保留: {ok}");
        assert!(ok.contains("type"), "type 属性应保留: {ok}");

        let bad = sanitize_html(r#"<video controls><source src="https://cdn.example.com/a.mp4" type="video/mp4"></video>"#);
        assert!(!bad.contains("cdn.example.com"), "不可信 source src 应被剥掉: {bad}");
    }

    /// 脚本注入仍是硬边界：放行 video 不得削弱原有 XSS 防护
    #[test]
    fn sanitize_html_still_removes_script_and_events() {
        let out = sanitize_html(r#"<script>alert(1)</script><p onclick="steal()">正文</p>"#);
        assert!(!out.contains("script"), "script 应被剥掉: {out}");
        assert!(!out.contains("alert"), "脚本内容应被剥掉: {out}");
        assert!(!out.contains("onclick"), "事件处理器应被剥掉: {out}");
        assert!(out.contains("正文"), "安全内容应保留: {out}");
    }

    // ─── P1：Atom 解析 / 根标签分派 ────────────────────────────────────────────

    /// 最小 RSS 文档：分派到 RSS 解析器，字段正确提取
    #[test]
    fn parse_xml_feed_handles_rss_document() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel><title>测试源</title>
<item><title>文章一</title><link>https://example.com/1</link><guid>g1</guid>
<description>正文一</description><pubDate>Wed, 02 Oct 2024 10:00:00 GMT</pubDate></item>
</channel></rss>"#;
        let parsed = parse_xml_feed(xml).expect("RSS 应解析成功");
        assert_eq!(parsed.title, "测试源");
        assert_eq!(parsed.items.len(), 1);
        let item = &parsed.items[0];
        assert_eq!(item.guid, "g1");
        assert_eq!(item.title, "文章一");
        assert_eq!(item.link, "https://example.com/1");
        assert!(item.content.contains("正文一"), "content: {}", item.content);
    }

    /// 最小 Atom 文档：验证 entry / author / alternate link / published 的映射
    #[test]
    fn parse_xml_feed_handles_atom_document() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom 测试源</title>
  <icon>https://example.com/icon.png</icon>
  <updated>2024-10-02T12:00:00Z</updated>
  <entry>
    <title>Atom 文章一</title>
    <id>urn:uuid:1</id>
    <link rel="alternate" href="https://example.com/atom/1"/>
    <author><name>作者甲</name></author>
    <published>2024-10-02T10:00:00Z</published>
    <updated>2024-10-02T11:00:00Z</updated>
    <content type="html">&lt;p&gt;Atom 正文&lt;/p&gt;</content>
  </entry>
</feed>"#;
        let parsed = parse_xml_feed(xml).expect("Atom 应解析成功");
        assert_eq!(parsed.title, "Atom 测试源");
        assert_eq!(parsed.icon.as_deref(), Some("https://example.com/icon.png"));
        assert_eq!(parsed.items.len(), 1);
        let item = &parsed.items[0];
        assert_eq!(item.guid, "urn:uuid:1");
        assert_eq!(item.title, "Atom 文章一");
        assert_eq!(item.link, "https://example.com/atom/1");
        assert_eq!(item.author, "作者甲");
        assert!(
            item.content.contains("Atom 正文"),
            "content: {}",
            item.content
        );
        // published 优先于 updated
        assert_eq!(item.published_at.to_rfc3339(), "2024-10-02T10:00:00+00:00");
    }

    /// Atom 的 published 缺失时应回退到 updated
    #[test]
    fn parse_atom_falls_back_to_updated_when_published_missing() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
  <title>T</title><updated>2024-01-01T00:00:00Z</updated>
  <entry><title>E</title><id>i</id>
    <link href="https://example.com/e"/>
    <updated>2024-03-05T08:30:00Z</updated>
  </entry>
</feed>"#;
        let parsed = parse_xml_feed(xml).expect("Atom 应解析成功");
        assert_eq!(
            parsed.items[0].published_at.to_rfc3339(),
            "2024-03-05T08:30:00+00:00"
        );
    }

    /// Atom 的 <link rel="enclosure"> 应映射为附件；mime 缺失时按扩展名兜底
    #[test]
    fn parse_atom_maps_enclosure_link() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
  <title>T</title><updated>2024-01-01T00:00:00Z</updated>
  <entry><title>E</title><id>i</id>
    <updated>2024-01-01T00:00:00Z</updated>
    <link rel="enclosure" href="https://example.com/ep.mp3"/>
  </entry>
</feed>"#;
        let parsed = parse_xml_feed(xml).expect("Atom 应解析成功");
        let item = &parsed.items[0];
        assert_eq!(item.enclosure_url.as_deref(), Some("https://example.com/ep.mp3"));
        assert_eq!(item.enclosure_type.as_deref(), Some("audio/mpeg"));
    }

    /// 畸形输入（HTML 网页）应报错，而不是被当成 feed 解析出垃圾
    #[test]
    fn parse_xml_feed_rejects_html_page() {
        let html = "<!DOCTYPE html>\n<html><head><title>登录</title></head><body>请登录</body></html>";
        assert!(parse_xml_feed(html).is_err(), "HTML 网页不应被解析为 feed");
    }

    /// 空文档应报错
    #[test]
    fn parse_xml_feed_rejects_empty_document() {
        assert!(parse_xml_feed("").is_err());
        assert!(parse_xml_feed("   \n  ").is_err());
    }

    /// 根元素检测应跳过 XML 声明、注释与空白
    #[test]
    fn detect_xml_root_skips_declaration_and_comment() {
        assert_eq!(detect_xml_root("<?xml version=\"1.0\"?><rss></rss>").as_deref(), Some("rss"));
        assert_eq!(
            detect_xml_root("\u{feff}\n<!-- 注释 -->\n<feed>x</feed>").as_deref(),
            Some("feed")
        );
        assert_eq!(detect_xml_root("<unknown").as_deref(), Some("unknown"));
        assert_eq!(detect_xml_root("not xml"), None);
    }

    // ─── P2a：抓取 / 解析错误信息 ─────────────────────────────────────────────

    /// 测试用：不含任何 Cloudflare 特征的普通响应头
    fn plain_headers() -> ChallengeHeaders<'static> {
        ChallengeHeaders {
            cf_mitigated: "",
            server: "",
        }
    }

    /// 非 2xx 错误信息应含状态码、原因与响应片段
    #[test]
    fn http_error_message_includes_status_and_snippet() {
        let msg = http_error_message(
            403,
            "Forbidden",
            "<html>\n  <body>blocked</body>\n</html>",
            "https://example.com/feed",
            &plain_headers(),
        );
        assert!(msg.contains("HTTP 403 Forbidden"), "msg: {msg}");
        assert!(msg.contains("blocked"), "应含响应体片段: {msg}");
    }

    /// Cloudflare 挑战页（带 `cf-mitigated` 头）：必须点明"人机验证"并给出换实例的处方
    ///
    /// 这是 `rsshub.app` 从 2026-11-02 起的真实状态：站点活着、TLS 正常、也返回了
    /// 403，但正文是一段要求执行 JS 的挑战页。笼统报"被拒绝"会让人去查网络和地址。
    #[test]
    fn cloudflare_challenge_is_named_for_rsshub() {
        let msg = http_error_message(
            403,
            "Forbidden",
            "<html><head><title>Just a moment...</title></head><body>Enable JavaScript</body></html>",
            "https://rsshub.app/latepost/4",
            &ChallengeHeaders {
                cf_mitigated: "challenge",
                server: "cloudflare",
            },
        );
        assert!(msg.contains("人机验证"), "msg: {msg}");
        assert!(
            msg.contains("换一个实例"),
            "RSSHub 场景应给出换实例的处方: {msg}"
        );
        assert!(
            msg.contains("本次连接："),
            "要带上本次实际走的连接: {msg}"
        );
        assert!(
            !msg.contains("响应内容："),
            "不应退化成通用 403 文案: {msg}"
        );
    }

    /// 响应头被中间设备剥掉时，仍能靠正文 "Just a moment" 认出挑战页
    #[test]
    fn cloudflare_challenge_falls_back_to_body() {
        let msg = http_error_message(
            403,
            "Forbidden",
            "<html><body>Just a moment...</body></html>",
            "https://example.com/feed",
            &ChallengeHeaders {
                cf_mitigated: "",
                server: "cloudflare",
            },
        );
        assert!(msg.contains("人机验证"), "msg: {msg}");
        assert!(
            msg.contains("源本身通常还活着"),
            "普通站点应提示源没坏: {msg}"
        );
        assert!(
            !msg.contains("换一个实例"),
            "普通站点没有实例可换，说这句话会误导: {msg}"
        );
    }

    /// 普通 403（无 Cloudflare 特征）不受影响，仍走原格式
    #[test]
    fn plain_403_keeps_original_wording() {
        let msg = http_error_message(
            403,
            "Forbidden",
            "<html><body>blocked</body></html>",
            "https://example.com/feed",
            &ChallengeHeaders {
                cf_mitigated: "",
                server: "nginx",
            },
        );
        assert!(msg.contains("响应内容"), "msg: {msg}");
        assert!(!msg.contains("人机验证"), "不该误判普通 403: {msg}");
    }

    /// RSSHub 实例的 503 要点出「它抓不到哪个上游」，而不是丢一段 HTML
    ///
    /// 这是用户实际遇到过的那条：实例返回 503，真正失败的是它去抓
    /// `www.latepost.com/site/get-column`。不说清这一层，用户会以为原站挂了。
    #[test]
    fn rsshub_503_reports_upstream_target() {
        let body = r#"<html><body><h1>Welcome to RSSHub!</h1>
            <p>Error: [GET] &quot;https://www.latepost.com/site/get-column&quot;: &lt;no response&gt;</p>
            </body></html>"#;
        let msg = http_error_message(
            503,
            "Service Unavailable",
            body,
            "https://rsshub.rssforever.com/latepost/4",
            &plain_headers(),
        );
        assert!(msg.contains("抓不到上游"), "msg: {msg}");
        assert!(
            msg.contains("https://www.latepost.com/site/get-column"),
            "应点名上游地址: {msg}"
        );
        assert!(msg.contains("no response"), "应带上实例给的原因: {msg}");
        assert!(
            msg.contains("换一个实例通常也一样"),
            "要提前拦住「换个实例试试」这种无效尝试: {msg}"
        );
    }

    /// 非 RSSHub 站点的错误页不参与上面的解析
    ///
    /// 普通站点正文里也可能出现 `[GET] "http…"` 之类的字样，
    /// 若不加限定就会把一段无关文字当成"上游失败"报给用户。
    #[test]
    fn upstream_parsing_is_limited_to_rsshub_hosts() {
        let body = r#"<p>Error: [GET] "https://example.com/x": timed out</p>"#;
        let msg = http_error_message(
            503,
            "Service Unavailable",
            body,
            "https://example.com/feed",
            &plain_headers(),
        );
        assert!(!msg.contains("抓不到上游"), "msg: {msg}");
        assert!(msg.contains("响应内容"), "普通站点仍走原格式: {msg}");
    }

    /// HTML 响应 → 提示"返回的是网页而非订阅内容"
    #[test]
    fn enrich_parse_error_flags_html_page() {
        let msg = enrich_parse_error(
            "RSS 解析失败: 坏标签".into(),
            "<!DOCTYPE html>\n<html><body>登录</body></html>",
            false,
        );
        assert!(msg.contains("网页而非订阅内容"), "msg: {msg}");
    }

    /// XML 分支遇到 JSON 体 → 提示"是 JSON 但 Content-Type 未标记"
    #[test]
    fn enrich_parse_error_flags_unmarked_json() {
        let msg = enrich_parse_error("RSS 解析失败: x".into(), r#"{"items":[]}"#, false);
        assert!(msg.contains("Content-Type 未标记为 json"), "msg: {msg}");
    }

    /// JSON 分支下不应误报"未标记为 json"，只附响应片段
    #[test]
    fn enrich_parse_error_json_branch_keeps_snippet() {
        let msg = enrich_parse_error("JSON 解析失败: x".into(), r#"{"broken":}"#, true);
        assert!(!msg.contains("未标记为 json"), "msg: {msg}");
        assert!(msg.contains("响应片段"), "msg: {msg}");
    }

    /// 响应片段应压缩换行 / 连续空白并过滤 BOM
    #[test]
    fn body_snippet_collapses_whitespace() {
        assert_eq!(body_snippet("\u{feff}  a\n\n  b\tc  ", 100), "a b c");
        assert_eq!(body_snippet("abcdef", 3), "abc");
    }
}
