//! 共享 HTTP 客户端
//!
//! # 职责
//! 为「抓取类」网络请求提供进程内复用的 [`reqwest::Client`]，并统一管理代理配置。
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
//! - [`probe_client`]：实例连通性探测，最严格（6s），只需要知道对方是否活着；
//! - [`market_client`]：行情/财务数据源（Tushare / 新浪等），中等（15s），单笔请求失败不应卡住整轮同步。
//!
//! # 代理（[`set_proxy`]）
//! 桌面应用**拿不到** macOS / Windows 的「系统代理」设置：`reqwest` 只读
//! `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 这几个**环境变量**，而从图标启动的
//! App 连环境变量都继承不到。这就是「浏览器能打开、应用报 `unexpected EOF`」的
//! 经典成因——应用根本没走代理，直连被中间设备按 TLS 指纹掐断。
//!
//! 因此代理必须在应用内显式配置，并且要解决两件事：
//! 1. 客户端是缓存的（[`ClientSlot`]），改配置后必须能**换掉**已建好的客户端，
//!    否则用户在设置页改了代理、不重启就毫无作用；
//! 2. 配置写错不能**静默**生效。一个拼错的代理地址会让全部请求都失败，
//!    表现成「所有订阅源同时挂了」，比不配代理难排查得多，故 [`set_proxy`] 会校验。

use reqwest::Client;
use std::sync::{Mutex, RwLock};
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

/// 行情数据源的单次超时
///
/// 取 15 秒：Tushare / 新浪单次 HTTP 调用通常在 1~3 秒内返回，超过 15 秒基本可判定
/// 该数据源当时不可达，继续等只会拖住「同步某实体行情」这类交互式操作。
const MARKET_TIMEOUT: Duration = Duration::from_secs(15);

/// 当前生效的代理地址（空串表示直连）
///
/// # 为什么是进程级全局
/// 客户端构建是**同步**的，而读设置是异步的（要访问 SQLite）。代理只在两个时刻
/// 变化——应用启动、设置页保存——这两处都会显式调用 [`set_proxy`]，因此这里不需要
/// 轮询数据库，也不该让每次取客户端都去查一次库。
static PROXY: RwLock<String> = RwLock::new(String::new());

/// 支持出现在代理地址里的协议
///
/// `socks5h` 与 `socks5` 的区别是「域名由谁解析」：`socks5h` 交给代理端解析
/// （DNS 污染环境下必须用它），`socks5` 在本地解析。两者都接受，由用户按需选。
const PROXY_SCHEMES: [&str; 4] = ["http", "https", "socks5", "socks5h"];

/// 缓存中的客户端及其构建条件
struct Cached {
    /// 构建这个客户端时生效的代理地址（空串 = 直连），用于判断缓存是否还有效
    proxy: String,
    /// 已构建好的客户端；`reqwest::Client` 内部是 `Arc`，克隆代价极低
    client: Client,
}

/// 一个「可失效」的客户端缓存槽
///
/// 之所以不用 `OnceLock`：`OnceLock` 一旦写入就再也换不掉，而代理配置是运行期
/// 可变的。这里用互斥锁包一个 `Option`，比对构建条件（当前代理地址）不一致时就
/// 重建——换配置立即生效，不需要重启应用。
pub(crate) struct ClientSlot {
    cached: Mutex<Option<Cached>>,
}

impl ClientSlot {
    /// 构造一个空的缓存槽（`const`，可直接用于 `static` 初始化）
    pub(crate) const fn new() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }
}

/// 抓订阅源用的客户端缓存槽
static FEED_CLIENT: ClientSlot = ClientSlot::new();

/// 实例探测用的客户端缓存槽
static PROBE_CLIENT: ClientSlot = ClientSlot::new();

/// 行情/财务数据源用的客户端缓存槽
static MARKET_CLIENT: ClientSlot = ClientSlot::new();

/// 设置代理地址
///
/// # 参数
/// * `raw` - 用户输入的代理地址，例如 `http://127.0.0.1:7890`、`socks5://127.0.0.1:1080`；
///   留空表示直连（关闭代理）
///
/// # 返回值
/// 归一化后的地址（补全了协议前缀），调用方应把**这个值**而非原始输入存进设置，
/// 否则下次启动读回来的还是用户手打的那份不一致的字符串。
///
/// # 错误
/// 协议不受支持、主机名缺失、地址无法解析时返回可读错误。
/// **校验失败不会改动当前生效的代理**——写错一项配置不该让整个应用断网。
///
/// # 为什么写错要报错而不是静默降级
/// 静默降级后用户看到的是「所有订阅源同时抓不到」，而根因是一个拼错的代理地址；
/// 这种"一次配置错误 + 全局面状故障"的组合最难定位，宁可在保存时就拒绝。
pub fn set_proxy(raw: &str) -> Result<String, String> {
    let normalized = normalize_proxy(raw)?;
    {
        // 锁中毒不该让代理设置永久写不进去（写失败只会让新配置不生效），
        // 故取回内部值继续用。
        let mut guard = PROXY.write().unwrap_or_else(|e| e.into_inner());
        *guard = normalized.clone();
    }
    Ok(normalized)
}

/// 读取当前生效的代理地址
///
/// # 返回值
/// 空串表示直连；否则是带协议前缀的完整代理地址
pub fn proxy_url() -> String {
    PROXY.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// 供错误文案展示的「本次连接走哪条路」
///
/// # 返回值
/// 配了代理时是代理地址，未配时是「直连」
///
/// # 为什么要出现在错误里
/// 「同一个源，浏览器能开、应用不能」有太多种成因，其中一半是**应用实际走的路**
/// 与用户以为的不一样（隐式环境变量代理、TUN 只覆盖了一部分流量……）。
/// 把这条路写进错误，用户不必再猜，也不必再让别人远程排查一轮。
pub fn effective_proxy_label() -> String {
    let proxy = proxy_url();
    if proxy.is_empty() {
        "直连".to_string()
    } else {
        proxy
    }
}

/// 归一化并校验代理地址（纯函数，便于单测）
///
/// 对 `pub` 是为了让 `lib.rs::settings_set` 能「先校验、再落库、最后切换」：
/// 顺序反了会出现"内存已切到新代理、库里还是旧值"的窗口，重启后用户会发现
/// 自己的改动莫名其妙地回退了。
///
/// # 参数
/// * `raw` - 用户输入的原始字符串，允许首尾空白、允许省略协议
///
/// # 返回值
/// - 空输入 → `Ok("")`，表示直连；
/// - 其余 → `Ok` 带协议前缀的规范地址。
///
/// # 错误
/// 协议不在 [`PROXY_SCHEMES`] 内、缺少主机名、URL 无法解析时返回错误说明。
pub fn normalize_proxy(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    // 用户习惯只填 `127.0.0.1:7890`（浏览器扩展里就这么写），补一个默认协议。
    // `Url::parse("127.0.0.1:7890")` 会把它当成 scheme 去解析并失败，所以必须先补。
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    };

    let parsed = url::Url::parse(&with_scheme)
        .map_err(|_| format!("代理地址无法解析：{}（示例：http://127.0.0.1:7890）", trimmed))?;

    let scheme = parsed.scheme();
    if !PROXY_SCHEMES.contains(&scheme) {
        return Err(format!(
            "不支持的代理协议 {}：只支持 http://、https://、socks5://",
            scheme
        ));
    }
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err(format!("代理地址缺少主机名：{}", trimmed));
    }

    Ok(with_scheme)
}

/// 构建带超时与代理的客户端
///
/// # 参数
/// * `timeout` - 请求总超时
/// * `proxy` - 代理地址，空串表示直连
///
/// # 返回值
/// 配置好的客户端。构建失败只可能源于不合法的 TLS 后端配置，此时退化为默认
/// 客户端并打印日志——那种环境下任何请求都会立刻失败并回传可读错误，
/// 不值得为此把所有调用方都改成 `Result` 返回。
fn build(timeout: Duration, proxy: &str) -> Client {
    let mut builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(CONNECT_TIMEOUT);

    if !proxy.is_empty() {
        match reqwest::Proxy::all(proxy) {
            Ok(p) => {
                // 本机地址一律不走代理。
                // 自建 RSSHub 常跑在 127.0.0.1 或 `localhost` 上，代理软件的回环规则
                // 各家不一，一旦把这些请求也塞进代理隧道，用户会得到"配好代理之后
                // 连自己的实例都连不上了"——那是本功能最容易被判定为 bug 的形态。
                //
                // `no_proxy` 收的是整个豁免表（不是追加），故三个地址写在同一个
                // 逗号串里，分多次调用只会剩下最后一次。
                builder = builder.proxy(
                    p.no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,[::1]")),
                );
            }
            // 走不到这里（`set_proxy` 已校验过一次），但保留兜底：任何路径下
            // "代理不可用"都不该升级成"应用不能联网"。
            Err(e) => eprintln!("代理地址无法解析，本次按直连处理: {}", e),
        }
    } else {
        // 留空 = 真直连，必须显式关掉环境变量代理探测。
        //
        // reqwest 默认会读 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY`，于是同一个
        // "留空"设置会出现两种行为：从桌面启动器起来（继承不到变量）是直连，
        // 从某个带变量的终端里起来却在走一个用户从没配过的代理。这类"配置看得见、
        // 行为对不上"的幽灵问题最难查——排查时甚至会把结论带偏（本项目就被带偏过
        // 一次）。界面上写的是"留空为直连"，这里就得让它名副其实。
        builder = builder.no_proxy();
    }

    builder.build().unwrap_or_else(|e| {
        eprintln!("构建 HTTP 客户端失败，退化为默认客户端: {}", e);
        Client::new()
    })
}

/// 从缓存槽取客户端；构建条件（代理地址）变化时自动重建
///
/// # 参数
/// * `slot` - 该超时档位专属的缓存槽
/// * `timeout` - 请求总超时
///
/// # 返回值
/// 可用的客户端。按值返回是刻意的：`reqwest::Client` 内部是 `Arc` 克隆，
/// 成本可以忽略，而按引用返回会逼着调用方持有 `'static` 生命周期，
/// 也就没法在代理变更后换掉底层实例。
pub(crate) fn shared(slot: &'static ClientSlot, timeout: Duration) -> Client {
    let proxy = proxy_url();
    // 锁中毒时取回内部值继续用，避免一次 panic 让后续所有请求永久失败
    let mut guard = slot.cached.lock().unwrap_or_else(|e| e.into_inner());

    if let Some(cached) = guard.as_ref() {
        if cached.proxy == proxy {
            return cached.client.clone();
        }
        // 代理变了：落到下面重建，旧客户端随赋值被丢弃（其连接池随之释放）
    }

    let client = build(timeout, &proxy);
    *guard = Some(Cached {
        proxy,
        client: client.clone(),
    });
    client
}

/// 取抓订阅源用的客户端（首次调用时构建，代理变更后自动重建）
///
/// # 返回值
/// 可直接用于 `.get(url)` 的客户端
pub fn feed_client() -> Client {
    shared(&FEED_CLIENT, FEED_TIMEOUT)
}

/// 取实例探测用的客户端（首次调用时构建，代理变更后自动重建）
///
/// # 返回值
/// 自身即带 [`PROBE_TIMEOUT`] 超时的客户端
pub fn probe_client() -> Client {
    shared(&PROBE_CLIENT, PROBE_TIMEOUT)
}

/// 取行情/财务数据源用的客户端（首次调用时构建，代理变更后自动重建）
///
/// # 返回值
/// 自身即带 [`MARKET_TIMEOUT`] 超时的客户端
pub fn market_client() -> Client {
    shared(&MARKET_CLIENT, MARKET_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 改动代理的测试必须串行
    ///
    /// `PROXY` 是进程级全局，而 cargo 默认多线程跑测试：两个测试同时改它，
    /// 断言会互相踩。这里用一把锁把「改代理」的用例排成队列。
    static PROXY_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 空输入表示直连，不是错误
    #[test]
    fn empty_input_means_direct() {
        assert_eq!(normalize_proxy("").unwrap(), "");
        assert_eq!(normalize_proxy("   ").unwrap(), "");
    }

    /// 省略协议时补 http://（用户最常见的填法）
    #[test]
    fn missing_scheme_defaults_to_http() {
        assert_eq!(
            normalize_proxy("127.0.0.1:7890").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy(" 127.0.0.1:7890 ").unwrap(),
            "http://127.0.0.1:7890"
        );
    }

    /// 四种协议都放行，且原样保留（socks5h 决定 DNS 在谁那边解析，不能被改写）
    #[test]
    fn supported_schemes_are_kept_verbatim() {
        for raw in [
            "http://127.0.0.1:7890",
            "https://proxy.example.com:8443",
            "socks5://127.0.0.1:1080",
            "socks5h://127.0.0.1:1080",
        ] {
            assert_eq!(normalize_proxy(raw).unwrap(), raw);
        }
    }

    /// 不支持的协议要明确报错，而不是悄悄当成直连或当成 http
    #[test]
    fn unsupported_scheme_is_rejected() {
        let err = normalize_proxy("ftp://127.0.0.1:21").unwrap_err();
        assert!(err.contains("ftp"), "错误信息应点出具体协议: {}", err);
    }

    /// 只有协议没有主机名要报错（`http://` 能让 Url::parse 通过，但代理建不出来）
    #[test]
    fn missing_host_is_rejected() {
        assert!(normalize_proxy("http://").is_err());
    }

    /// 校验失败不得改动当前生效的代理
    ///
    /// 这是本模块最容易被写错的不变量：先写全局再校验，一次手滑就会让整个应用
    /// 断网，且重启前无法自愈。
    #[test]
    fn invalid_proxy_does_not_change_effective_value() {
        let _guard = PROXY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_proxy("http://127.0.0.1:7890").unwrap();
        assert!(set_proxy("ftp://127.0.0.1:21").is_err());
        assert_eq!(
            proxy_url(),
            "http://127.0.0.1:7890",
            "校验失败时生效值必须保持原样"
        );
    }

    /// 代理变更后缓存槽必须重建，且记录新的构建条件
    ///
    /// 「改了设置要重启才生效」正是本功能最典型的事故形态，故直接断言缓存签名。
    #[test]
    fn cache_slot_follows_proxy_change() {
        let _guard = PROXY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 每个测试用独立槽位，避免与生产静态量互相影响
        let slot: &'static ClientSlot = Box::leak(Box::new(ClientSlot::new()));

        set_proxy("http://127.0.0.1:7890").unwrap();
        let _first = shared(slot, Duration::from_secs(1));
        assert_eq!(
            slot.cached.lock().unwrap().as_ref().map(|c| c.proxy.as_str()),
            Some("http://127.0.0.1:7890")
        );

        // 改代理：下一次取客户端必须带着新地址重建
        set_proxy("socks5://127.0.0.1:1080").unwrap();
        let _second = shared(slot, Duration::from_secs(1));
        assert_eq!(
            slot.cached.lock().unwrap().as_ref().map(|c| c.proxy.as_str()),
            Some("socks5://127.0.0.1:1080"),
            "缓存槽没有跟着代理走，意味着改设置要重启才生效"
        );

        // 复位，避免影响同进程内的其它用例
        set_proxy("").unwrap();
    }

    /// socks5 协议必须真的能被 reqwest 接受
    ///
    /// reqwest 的 SOCKS 支持要编译期打开 `socks` 特性，缺了它 `Proxy::all` 会返回
    /// Err，而 `build` 对 Err 只是打日志后**按直连继续**——用户填了 socks5 地址却
    /// 完全没生效，且没有任何提示。这条测试把"特性忘了开"挡在编译产物之外。
    #[test]
    fn socks5_proxy_is_accepted_by_reqwest() {
        assert!(
            reqwest::Proxy::all("socks5://127.0.0.1:1080").is_ok(),
            "reqwest 不接受 socks5，说明 Cargo.toml 少开了 socks 特性"
        );
        assert!(reqwest::Proxy::all("http://127.0.0.1:7890").is_ok());
    }

    /// 配了代理之后，请求必须**真的**从代理出去
    ///
    /// 纯函数测试只能证明"地址填对了会被接受"，证明不了流量真的改道——而"设置
    /// 存进去了但请求还是直连"正是这类功能最典型的假成功。这里起一个假 HTTP
    /// 代理：目标用不存在的 `proxied.invalid`（直连必 DNS 失败），只有走代理才可能
    /// 得到 200；同时断言代理收到的请求行是**绝对 URI** 形式，那是 HTTP 代理转发
    /// 的标志（直连只会有 `/probe` 这样的相对路径）。
    #[tokio::test]
    async fn request_actually_goes_through_configured_proxy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _guard = PROXY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 1024];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                    .await;
                let _ = sock.flush().await;
            }
        });

        set_proxy(&format!("http://{}", addr)).unwrap();
        let slot: &'static ClientSlot = Box::leak(Box::new(ClientSlot::new()));
        let outcome = shared(slot, Duration::from_secs(5))
            .get("http://proxied.invalid/probe")
            .send()
            .await;
        // 无论成败都复位，避免污染同进程的其它用例
        set_proxy("").unwrap();

        let resp = outcome.expect("配置了代理时请求不应在 DNS 阶段失败");
        assert_eq!(resp.status().as_u16(), 200, "应拿到假代理返回的响应");

        let request_line = rx.await.unwrap_or_default();
        assert!(
            request_line.starts_with("GET http://proxied.invalid/probe"),
            "代理收到的应是绝对 URI 请求行（说明确实走了代理），实际: {:?}",
            request_line.lines().next()
        );
    }

    /// 「本次连接」标签要如实反映当前设置
    #[test]
    fn effective_proxy_label_reflects_setting() {
        let _guard = PROXY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_proxy("").unwrap();
        assert_eq!(effective_proxy_label(), "直连");
        set_proxy("socks5://127.0.0.1:1080").unwrap();
        assert_eq!(effective_proxy_label(), "socks5://127.0.0.1:1080");
        set_proxy("").unwrap();
    }

    /// 「留空」必须是真的直连：不得偷偷沿用环境变量里的代理
    ///
    /// reqwest 默认会读 `HTTPS_PROXY`。不清掉这个隐式入口的话，同一个「留空」
    /// 会因为启动方式不同而行为不同——从带变量的终端启动时，应用会走一个用户
    /// 从没配过的代理，而界面上写着"直连"。这条测试用一个假代理守着：
    /// 它一旦收到连接，就说明留空时仍然用了环境变量代理。
    ///
    /// 把 `build` 里的 `.no_proxy()` 注释掉，本用例会立刻失败——已实测过。
    #[tokio::test]
    async fn empty_setting_ignores_environment_proxy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _guard = PROXY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 256];
                let _ = socket.read(&mut buf).await;
                let _ = tx.send(());
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });

        // 仅本进程内生效；并发用例走的都是 127.0.0.1（本就豁免代理）或显式设了代理
        std::env::set_var("HTTPS_PROXY", format!("http://{}", addr));
        set_proxy("").unwrap();
        let slot: &'static ClientSlot = Box::leak(Box::new(ClientSlot::new()));
        // 目标域名不可解析：走代理时会落到假代理上，真直连则会 DNS 失败
        let _ = shared(slot, Duration::from_secs(2))
            .get("https://env-proxy-must-not-be-used.invalid/x")
            .send()
            .await;
        std::env::remove_var("HTTPS_PROXY");

        assert!(
            tokio::time::timeout(Duration::from_millis(400), rx)
                .await
                .is_err(),
            "代理留空时仍然使用了环境变量里的代理"
        );
    }
}
