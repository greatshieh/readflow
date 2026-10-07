//! 系统托盘图标
//!
//! # 职责
//! 在系统托盘（Windows 任务栏通知区 / macOS 菜单栏 / Linux 托盘）常驻一个图标，
//! 让用户在窗口最小化或关闭到后台时仍能回到应用。
//!
//! # 交互设计
//! - **左键单击**：显示并聚焦主窗口；若窗口已可见则隐藏（toggle）。
//!   这是托盘图标最常用的动作，做到"点一下就能回到阅读"。
//! - **右键 / 左键菜单**：显式菜单，提供「显示窗口 / 隐藏窗口 / 刷新订阅源 / 退出」
//!   四项。菜单里保留「隐藏窗口」是因为左键 toggle 在"窗口意外被切走"时不够直观，
//!   用户需要一条确定的"藏起来"指令。
//! - **左键不弹菜单**（`show_menu_on_left_click(false)`）：菜单归右键，
//!   左键专用于 toggle。两者共用左键会让用户每次点击都先看到菜单，违背托盘预期。
//!
//! # 图标约定
//! 托盘图标直接复用应用图标（[`tauri::App::default_window_icon`]，取自
//! tauri.conf.json 的 icon 清单）：图标资产由 `tauri icon` 工作流统一管理，
//! 本模块不持有图标文件、不做明暗/颜色切换；也刻意不启用 `icon_as_template`
//! ——那是单色标记配合 macOS 系统着色的机制，用在彩色应用图标上会被遮罩成剪影。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

/// 托盘图标事件标识，与菜单项 id 统一
const TRAY_ID: &str = "readflow-tray";

/// 菜单项 id
mod ids {
    pub const SHOW: &str = "tray-show";
    pub const HIDE: &str = "tray-hide";
    pub const REFRESH: &str = "tray-refresh";
    pub const QUIT: &str = "tray-quit";
}

/// 把主窗口显示出来并置为焦点
///
/// 最小化状态必须先 `unminimize` 再 `show`：直接 `show` 在 Windows 上
/// 会让窗口保持最小化态，看起来像"点了没反应"。
///
/// # 参数
/// * `app` - Tauri 应用句柄
///
/// # 副作用
/// 显示、取消最小化并聚焦主窗口；窗口不存在时静默返回（正常情况下不会发生）
fn focus_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 隐藏主窗口
///
/// 隐藏而非关闭：关闭会销毁 WebView 实例，下次唤起需要整页重新加载，
/// 而阅读器用户常常把窗口收起、稍后继续读同一篇。
///
/// # 参数
/// * `app` - Tauri 应用句柄
fn hide_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

/// 左键单击：窗口可见则隐藏，不可见则唤起
///
/// # 参数
/// * `app` - Tauri 应用句柄
fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) {
    let visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    if visible {
        hide_main_window(app);
    } else {
        focus_main_window(app);
    }
}

/// 从托盘触发一次全量刷新
///
/// 复用与 `feeds_refresh` 命令相同的优先级：优先走全局 `REFRESH_MANAGER`
/// （带并发互斥，避免与定时刷新撞车），管理器尚未就绪时降级到直接刷新。
/// 两者都会逐源广播 `refresh-progress`，前端侧边栏的 n/m 进度与未读数照常更新，
/// 因此托盘刷新与界面刷新按钮的观感完全一致。
///
/// # 参数
/// * `app` - Tauri 应用句柄
fn refresh_from_tray(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = {
            let manager = crate::REFRESH_MANAGER.lock().await;
            match manager.as_ref() {
                Some(m) => m.manual_refresh(&handle).await,
                None => {
                    // 管理器未就绪（启动早期）：降级为直接刷新
                    match crate::db_pool().await {
                        Ok(pool) => {
                            crate::scheduler::refresh_all_feeds(&pool, Some(&handle)).await
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
            }
        };
        match result {
            Ok(n) => eprintln!("托盘触发刷新完成，新增 {} 篇文章", n),
            Err(e) => eprintln!("托盘触发刷新失败: {}", e),
        }
    });
}

/// 在应用启动流程中注册托盘图标
///
/// # 参数
/// * `app` - `tauri::Builder::setup` 传入的 `&tauri::App`
///
/// # 错误
/// 菜单构建失败或图标解码失败时返回错误。托盘不可用（如无桌面环境）
/// 不应导致整个应用启动失败——调用方需自行决定是否降级。
pub fn setup(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, ids::SHOW, "显示窗口", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, ids::HIDE, "隐藏窗口", true, None::<&str>)?;
    let refresh_item = MenuItem::with_id(app, ids::REFRESH, "刷新订阅源", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, ids::QUIT, "退出", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &hide_item,
            &sep1,
            &refresh_item,
            &sep2,
            &quit_item,
        ],
    )?;

    // 托盘图标复用应用图标（tauri.conf.json 的 icon 清单）：随 `tauri icon`
    // 工作流更新，本模块不持有任何图标资产、不做明暗切换。
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("应用默认图标缺失".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("ReadFlow")
        .menu(&menu)
        // 左键专用于 toggle 窗口，不弹菜单
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            ids::SHOW => focus_main_window(app),
            ids::HIDE => hide_main_window(app),
            ids::REFRESH => refresh_from_tray(app),
            ids::QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 只响应左键"按下"瞬间，避免按下+抬起各触发一次导致来回翻转
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
