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
}

/// 抓取并解析 RSS/Atom/JSON Feed
///
/// 先发起带自定义 UA 的 GET 请求（部分站点会拒绝空 UA 或默认爬虫 UA），
/// 再根据响应头 `Content-Type` 决定解析分支；头部缺失时按 XML 处理。
///
/// # 参数
/// * `url` - Feed 的 URL 地址
///
/// # 返回值
/// 解析后的 Feed 数据，包含标题和文章列表
///
/// # 错误
/// 网络请求失败、响应体读取失败或文档解析失败时返回错误信息
pub async fn fetch_feed(url: &str) -> Result<ParsedFeed, String> {
    // 复用进程内共享客户端：一轮刷新会依次访问几十个订阅源，公用连接池可省下
    // 每个源一次 TCP + TLS 握手（超时档见 crate::net）
    let client = crate::net::feed_client();

    let resp = client
        .get(url)
        .header("User-Agent", "ReadFlow/1.0 (RSS Reader)")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = resp.text().await.map_err(|e| format!("读取响应失败: {}", e))?;

    // 根据内容类型选择解析器；只要头部里有 json 就走 JSON Feed 分支
    let mut parsed = if content_type.contains("json") {
        parse_json_feed(&body)?
    } else {
        parse_xml_feed(&body)?
    };

    // 图标兜底：channel 未自带图标时，回退为源站点根路径的 /favicon.ico。
    // 这里不发起额外的网络探测（避免为取图标增加请求），
    // 加载失败由前端回退为首字母头像，因此宁可存一个可能 404 的地址。
    if parsed.icon.is_none() {
        parsed.icon = fallback_icon(url);
    }

    Ok(parsed)
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

/// 解析 XML 格式的 RSS/Atom feed
///
/// 使用 `rss` crate 读取 `<channel>`，逐条把 `<item>` 映射为 [`ParsedItem`]。
/// 各字段都做了缺省兜底，单个条目的字段缺失不会导致整篇 Feed 解析失败。
///
/// # 参数
/// * `content` - Feed 原始 XML 文本
///
/// # 返回值
/// 解析后的 Feed 数据；文档结构非法时返回错误
fn parse_xml_feed(content: &str) -> Result<ParsedFeed, String> {
    let channel = rss::Channel::read_from(Cursor::new(content))
        .map_err(|e| format!("XML 解析失败: {}", e))?;

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
        .link_rel(Some("noopener noreferrer"))
        // 丢弃 <img> 的 width / height 固有尺寸，交由前端 CSS 控制自适应宽度
        // attribute_filter 的返回值类型是 Option<Cow<str>>，故保留的属性需包成 Cow::Borrowed
        .attribute_filter(|element, attribute, value| {
            if element == "img" && (attribute == "width" || attribute == "height") {
                None
            } else {
                Some(std::borrow::Cow::Borrowed(value))
            }
        })
        .clean(html)
        .to_string()
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
