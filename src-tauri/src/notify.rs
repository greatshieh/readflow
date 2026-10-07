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

use std::sync::OnceLock;

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::db::Setting;

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
}
