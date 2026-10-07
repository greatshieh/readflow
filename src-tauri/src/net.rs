//! 共享 HTTP 客户端
//!
//! # 职责
//! 为「抓取类」网络请求提供进程内复用的 [`reqwest::Client`]。
//!
//! # 设计意图
//! `reqwest::Client` 内部维护连接池与 TLS 会话缓存，每次请求都新建客户端等于
//! 每轮都重做 TCP + TLS 握手。一轮刷新会依次访问几十个订阅源，而复用同一个
//! 客户端还能让同域名的多个源共享 keep-alive 连接。
//!
//! 把超时集中在这里定义，也是为了避免各调用点各写一套、出现漏设超时的路径
//! （AI 链路曾因为直接 `Client::new()` 而永久挂起，同一类错误不应再犯）。
//!
//! # 分工
//! 按超时档位各建一个客户端，互不共享：
//! - [`feed_client`]：抓订阅源，宽容（30s），异常源不该长期占用刷新任务；
//! - [`probe_client`]：实例连通性探测，最严格（6s），只需要知道对方是否活着。

use reqwest::Client;
use std::sync::OnceLock;
use std::time::Duration;

/// 抓取订阅源的总超时
///
/// 30 秒足够覆盖绝大多数源；再慢的源宁可失败也不拖慢整轮刷新
/// （一轮刷新是串行遍历，单个源卡住会拖住后面所有源）。
const FEED_TIMEOUT: Duration = Duration::from_secs(30);

/// 实例探测的单次超时
///
/// 取 6 秒：RSSHub 首次生成某条路由时可能需要回源抓取（较慢），但超过 6 秒仍无响应
/// 基本可判定该实例当时不可用，继续等只会拖住用户的"添加订阅"操作。
///
/// 对 `pub` 是刻意的：`rsshub` 会用它再套一层 `tokio::time::timeout`，
/// 让这个上限在客户端自身超时失效时依然成立（见 `rsshub::probe`）。
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

/// 建立连接的超时（DNS + TCP + TLS）
///
/// 各类请求共用同一个值：连不上就应当尽快失败，把时间预算留给真正在传输的数据。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 抓订阅源用的共享客户端
static FEED_CLIENT: OnceLock<Client> = OnceLock::new();

/// 实例探测用的共享客户端
static PROBE_CLIENT: OnceLock<Client> = OnceLock::new();

/// 构建带超时的客户端
///
/// # 参数
/// * `timeout` - 请求总超时
///
/// # 返回值
/// 配置好的客户端。构建失败只可能源于不合法的 TLS 后端配置，此时退化为默认
/// 客户端并打印日志——那种环境下任何请求都会立刻失败并回传可读错误，
/// 不值得为此把所有调用方都改成 `Result` 返回。
fn build(timeout: Duration) -> Client {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .unwrap_or_else(|e| {
            eprintln!("构建 HTTP 客户端失败，退化为默认客户端: {}", e);
            Client::new()
        })
}

/// 取抓订阅源用的共享客户端（首次调用时构建）
///
/// # 返回值
/// 进程内唯一的客户端引用，可直接用于 `.get(url)`
pub fn feed_client() -> &'static Client {
    FEED_CLIENT.get_or_init(|| build(FEED_TIMEOUT))
}

/// 取实例探测用的共享客户端（首次调用时构建）
///
/// # 返回值
/// 进程内唯一的客户端引用；自身即带 [`PROBE_TIMEOUT`] 超时
pub fn probe_client() -> &'static Client {
    PROBE_CLIENT.get_or_init(|| build(PROBE_TIMEOUT))
}
