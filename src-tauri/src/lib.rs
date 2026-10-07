//! Tauri 命令入口点 - 前端与 Rust 后端之间的 IPC 边界
//!
//! # 职责
//! 本模块是整个后端的**唯一对外 API 层**，[`tauri::generate_handler!`] 中共注册 **77 个命令**，
//! 按业务域分为十九组（下方清单需与 [`run`] 里的 `generate_handler!` 保持一致，增删命令时同步）：
//! - Feed 命令（11）：[`feeds_list`] / [`feeds_add`] / [`feeds_update`] / [`feeds_delete`] /
//!   [`feeds_refresh`] / [`feeds_refresh_one`] / [`feeds_import_opml`] / [`feeds_export_opml`] /
//!   [`feeds_backfill_icons`] / [`feeds_set_folder`] / [`feeds_auto_group`]
//! - 文件夹命令（4）：[`folders_list`] / [`folders_create`] / [`folders_update`] / [`folders_delete`]
//! - Article 命令（9）：[`articles_list`] / [`article_get`] / [`article_read`] / [`article_set_read`] /
//!   [`article_set_progress`] / [`article_bookmark`] / [`articles_unread_count`] /
//!   [`articles_mark_all_read`] / [`articles_search`]
//! - 高亮命令（3）：[`highlights_list`] / [`highlights_create`] / [`highlights_delete`]
//! - 标签命令（5）：[`tags_list`] / [`tags_create`] / [`tags_update`] / [`tags_delete`] /
//!   [`article_tags_set`]
//! - 过滤规则命令（4）：[`filters_list`] / [`filters_create`] / [`filters_update`] / [`filters_delete`]
//! - Settings 命令（2）：[`settings_get_all`] / [`settings_set`]
//! - AI 命令（5）：[`ai_generate_summary`] / [`ai_translate_article`] / [`ai_daily_brief`] /
//!   [`ai_article_chat`] / [`ai_cancel`]（追问真取消：登记标记由 SSE 循环逐块消费）
//! - 导出命令（2）：[`article_export_obsidian`] / [`save_text_file`]
//! - 数据管理命令（5）：[`db_stats`] / [`stats_daily_read`] / [`db_backup`] / [`db_restore`] / [`db_cleanup`]
//! - 实体管理命令（5）：[`entities_list`] / [`entities_create`] / [`entities_delete`] /
//!   [`entities_toggle`] / [`entity_graph`]
//! - 智能评分命令（2）：[`score_articles`] / [`high_score_articles`]
//! - 研究事件命令（5）：[`research_extract_article`] / [`research_extract_batch`] /
//!   [`research_events_by_article`] / [`research_events_timeline`] / [`research_event_delete`]
//! - 研究报告命令（3）：[`research_report_generate`] / [`research_reports_list`] / [`research_report_read`]
//! - 自动化任务命令（6）：[`automation_tasks_list`] / [`automation_tasks_create`] /
//!   [`automation_tasks_update`] / [`automation_tasks_delete`] / [`automation_tasks_set_enabled`] /
//!   [`automation_run_now`]
//! - 系统字体命令（1）：[`system_fonts_list`]
//! - 外部链接命令（1）：[`open_external`]
//! - 窗口控制命令（3）：[`win_minimize`] / [`win_toggle_maximize`] / [`win_close`]
//! - 应用命令（1）：[`app_restart`]（数据库恢复后重启进程，让连接池切换到新库）
//!
//! 另有 [`run`] 中的启动流程编排。
//!
//! # 设计意图
//! - **薄边界**：命令本身不做业务逻辑，只负责"取参数 → 取连接池 → 调 `db`/`rss`/`ai` 层 → 回结果"。
//!   所有 SQL 与算法细节都在下层模块，便于本文件保持可读。
//! - **共享全局连接池**：启动时由 `tauri::async_runtime::spawn` 异步调用 [`db::init_db`]，
//!   成功后把 `SqlitePool` 写入全局 [`DB_POOL`]；此后每个命令在入口调用 [`db_pool()`]
//!   克隆出同一个池使用（[`feeds_refresh`] 另有一条经 [`REFRESH_MANAGER`] 的路径，
//!   管理器尚未就绪时同样降级到 `db_pool()`）。原因有三：
//!   1) 初始化是异步的，命令可能早于它到达；`DB_POOL` 用 `Option` 表达"尚未就绪"，
//!      此时 `db_pool()` 返回「数据库尚未初始化，请稍后重试」交给前端重试，
//!      而不是让每个命令各自建连接、各自把库初始化一遍；
//!   2) 命令之间无需共享事务状态，各自从池里取连接，并发安全性更好推理；
//!   3) SQLite 连接池本身会复用底层连接，取池的开销可忽略，
//!      换来的是命令之间彻底解耦。
//! - **参数命名约定**：Rust 侧统一使用 snake_case（如 `feed_id`、`target_language`）。
//!   Tauri v2 会在反序列化 invoke 参数时自动把前端的 camelCase 键映射过来
//!   （前端传 `feedId`、`articleId`、`targetLanguage` 即可正确命中），
//!   因此这里**不需要** `#[serde(rename_all = "camelCase")]`；
//!   反过来，返回值中的字段名会原样（snake_case）序列化给前端。
//! - **错误扁平化**：所有命令返回 `Result<T, String>`，错误串直接展示给用户，
//!   因此下层错误信息应尽量保持人类可读。
//! - **局部失败容忍**：刷新这类批量操作对单个订阅源的失败只用 `eprintln!` 记录，
//!   不中断整体流程，避免一个坏源阻塞所有源的更新。

// 本 crate 的文档是**内部文档**：`//!` 写在 crate 根（即"公开文档"），而命令函数本身都是私有项，
// 于是正文里大量 intra-doc 链接会被 rustdoc 逐条报 `private_intra_doc_links`（此前累计 49 条）。
// 这些链接在 `cargo doc --document-private-items` 下能正常跳转，属有意为之，故只豁免这一个 lint。
// 注意**失效链接仍由 `broken_intra_doc_links` 单独把关**（本次已实测：命令清单里 66 个名字全部
// 解析成功），所以豁免不会掩盖"链接写错"这类真问题。
#![allow(rustdoc::private_intra_doc_links)]

mod db;
mod net;
mod rss;
mod scheduler;
mod ai;
mod opml;
mod grouping;
mod rsshub;
mod fonts;
mod obsidian;
mod research;
mod tray;
mod notify;

use tauri::{AppHandle, Emitter, Manager};
// DialogExt 为 `AppHandle` 提供 `.dialog()`，用于弹系统「另存为」对话框（见 [`pick_save_path`]）
use tauri_plugin_dialog::DialogExt;
pub use db::{
    Feed, Article, ArticleSort, Setting, FilterRule, Folder, Highlight, Entity, ResearchEvent,
    AutomationTask, EventWithContext, Tag, EntityGraph, GraphNode, GraphEdge,
};
use std::sync::LazyLock;
use chrono::Utc;
use sqlx::Row;
use tokio::sync::Mutex;
use std::sync::Arc;

/// 全局刷新管理器（应用级单例）
///
/// 由于 setup 里的初始化是异步的，这里用 `Option` 表达"尚未就绪"的中间态：
/// 命令到来时若为 `None`，就降级为直连方案而不是阻塞等待。
/// 内层再包 [`Arc`] 是为了能在持锁结束后克隆出一份句柄交给后台任务，
/// 避免整段持有锁。
static REFRESH_MANAGER: LazyLock<Mutex<Option<Arc<scheduler::RefreshManager>>>> =
    LazyLock::new(|| Mutex::new(None));

/// 全局自动化任务管理器（应用级单例）
///
/// 与 REFRESH_MANAGER 同模式：setup 异步初始化完成后写入；
/// 任务 CRUD 命令通过 [`trigger_automation_reload`] 触发重注册。
/// 初始化前（None）执行任务 CRUD 不会重注册，下次启动自然生效。
static AUTOMATION_MANAGER: LazyLock<Mutex<Option<Arc<scheduler::AutomationManager>>>> =
    LazyLock::new(|| Mutex::new(None));

/// 全局数据库连接池（应用级单例）
///
/// 由于 setup 里的初始化是异步的，这里用 `Option` 表达"尚未就绪"的中间态：
/// 命令到来时若为 `None`，则返回错误提示用户稍后重试。
/// 使用全局单例可避免每个命令都重新建立连接，提高性能和稳定性。
static DB_POOL: LazyLock<Mutex<Option<sqlx::SqlitePool>>> =
    LazyLock::new(|| Mutex::new(None));

/// 正在运行的自动化任务 ID 集合（并发防护）
///
/// 实现已移至 [`research::RUNNING_TASKS`]：cron 回调（scheduler 模块）与
/// 手动触发命令需共享同一份占位集合，而两者分属不同模块，
/// 因此防重入状态放在 research 模块统一定义，此处不再重复维护。


/// 一次性修复历史上"名称为 URL"的订阅源
///
/// # 背景
/// 早期版本的"添加订阅源"在用户未填名称时直接把 URL 存为 `name`，
/// 造成侧边栏显示一长串链接。本函数扫描 `name` 等于 `url`（或以 `http` 开头）的行，
/// 抓取该源并读取 channel 的真实 `<title>` 后回写。
///
/// # 设计取舍
/// - 单个源抓取失败（超时 / 源已失效）只跳过该行，不影响其它源；
/// - 整体失败只记日志，不阻塞应用启动——这是纯"美化"性质的修复，不应影响可用性。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 无；修复条数通过 stderr 日志输出
///
/// # 错误
/// 数据库查询失败时返回错误信息（抓取失败不算错误）
async fn repair_url_named_feeds(pool: &sqlx::SqlitePool) -> Result<(), String> {
    // 找出所有"名称看起来就是链接"的订阅源
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, url FROM feeds WHERE name = url OR name LIKE 'http%'",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Ok(());
    }

    let mut fixed = 0usize;
    for (id, url) in rows {
        // 抓取失败（源失效 / 网络不通）直接跳过，保持原标题不变
        let parsed = match rss::fetch_feed(&url).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        let title = parsed.title.trim().to_string();
        // 拿不到有效标题、或标题本身就是个链接时不做无意义覆盖
        if title.is_empty() || title.starts_with("http") {
            continue;
        }

        sqlx::query("UPDATE feeds SET name = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(&title)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        fixed += 1;
    }

    if fixed > 0 {
        eprintln!("已修复 {} 个订阅源的显示名称", fixed);
    }
    Ok(())
}

/// Tauri 应用入口：装配插件、异步初始化后台设施并注册全部命令
///
/// # 启动流程
/// 1. 注册 shell 插件；
/// 2. `setup` 中派生一个异步任务做重活（初始化数据库 → 创建刷新管理器 → 启动定时刷新），
///    因此 `setup` 本身立即返回，不会拖慢窗口显示；
/// 3. `invoke_handler` 注册命令清单，二者互不影响。
///
/// # 注意
/// 步骤 2 的失败只写 stderr、不影响 app 启动——应用会以"无后台刷新"的降级状态继续运行，
/// 这是刻意为之：本地优先的阅读器即使调度器不可用，也应允许用户读写已有数据。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // 文件对话框插件：仅由 Rust 侧调用（导出 OPML / 数据库备份时让用户选保存路径）。
        // 未在 capabilities 中授权，因此 WebView 不能自行弹框——这是有意的收敛。
        .plugin(tauri_plugin_dialog::init())
        // 系统通知插件：同样只在 Rust 侧调用（后台刷新 / 自动化任务完成时提醒，见 [`notify`]），
        // capabilities 不授权，WebView 无法自行弹通知。
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // 注册系统托盘图标：左键切换窗口显隐，右键菜单含显示/隐藏/刷新/退出。
            // 失败只记日志不中断启动——无桌面环境（纯 headless）下没有托盘是正常的，
            // 不该因此让整个应用起不来。
            if let Err(e) = tray::setup(app) {
                eprintln!("注册托盘图标失败（不影响主功能）: {}", e);
            }

            // 异步初始化数据库并启动后台刷新。
            // AppHandle 需要保留：cron 回调与调度任务拿不到命令层的句柄，
            // 因此在这里注入 [`notify`] 的全局句柄（与 research::report::set_reports_dir
            // 同一思路），供后台完成时发送系统通知使用。
            let app_handle = app.handle().clone();
            notify::set_app_handle(app_handle.clone());
            tauri::async_runtime::spawn(async move {
                // 使用全局连接池来初始化数据库
                match db::init_db(&app_handle).await {
                    Ok(pool) => {
                        eprintln!("数据库初始化成功");

                        // 存储到全局变量供后续访问
                        {
                            let mut global_pool = DB_POOL.lock().await;
                            *global_pool = Some(pool.clone());
                        }

                        // 一次性数据修复：补齐历史上"名称为 URL"的订阅源标题。
                        // 早期版本的"添加订阅源"在用户未填名称时直接把 URL 存为 name，
                        // 这里在后台静默抓取真实 channel 标题并回写；失败则保持原样。
                        {
                            let repair_pool = pool.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = repair_url_named_feeds(&repair_pool).await {
                                    eprintln!("修复订阅源名称失败: {}", e);
                                }
                            });
                        }

                        // 创建刷新管理器（pool 克隆一份给 AutomationManager 备用；
                        // AppHandle 一并注入，供逐源刷新进度事件广播使用）
                        match scheduler::RefreshManager::new(pool.clone(), app_handle.clone()).await {
                            Ok(manager) => {
                                let manager = Arc::new(manager);

                                // 存储到全局变量供后续访问。
                                // 用额外的块限制 MutexGuard 的作用域：guard 在块尾即释放，
                                // 后续耗时操作（manager.start()）不会一直占着锁。
                                {
                                    let mut global = REFRESH_MANAGER.lock().await;
                                    *global = Some(manager.clone());
                                }

                                // 启动后台定时刷新
                                if let Err(e) = manager.start().await {
                                    eprintln!("启动定时刷新失败: {}", e);
                                } else {
                                    eprintln!("后台刷新调度器已启动");
                                }
                            }
                            Err(e) => eprintln!("创建刷新管理器失败: {}", e),
                        }

                        // 报告输出目录：解析一次注入 research::report（cron 回调拿不到
                        // AppHandle，目录必须在启动阶段解析好）
                        research::report::set_reports_dir(
                            db::get_data_dir(&app_handle).join("reports"),
                        );

                        // 创建自动化任务管理器并按任务表完成初始注册
                        match scheduler::AutomationManager::new(pool).await {
                            Ok(auto_manager) => {
                                let auto_manager = Arc::new(auto_manager);
                                {
                                    let mut global = AUTOMATION_MANAGER.lock().await;
                                    *global = Some(auto_manager.clone());
                                }
                                if let Err(e) = auto_manager.start().await {
                                    eprintln!("启动自动化任务调度器失败: {}", e);
                                }
                            }
                            Err(e) => eprintln!("创建自动化任务管理器失败: {}", e),
                        }
                    }
                    Err(e) => {
                        eprintln!("数据库初始化失败: {}", e);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Feed 相关命令
            feeds_list,
            feeds_add,
            feeds_delete,
            feeds_update,
            feeds_refresh,
            feeds_refresh_one,
            feeds_import_opml,
            feeds_export_opml,
            feeds_backfill_icons,
            feeds_set_folder,
            feeds_auto_group,
            folders_list,
            folders_create,
            folders_update,
            folders_delete,
            // Article 相关命令
            articles_list,
            article_get,
            article_read,
            article_set_read,
            article_set_progress,
            articles_mark_all_read,
            article_bookmark,
            articles_unread_count,
            articles_search,
            highlights_list,
            highlights_create,
            highlights_delete,
            // 标签命令
            tags_list,
            tags_create,
            tags_update,
            tags_delete,
            article_tags_set,
            // AI 相关命令
            ai_generate_summary,
            ai_translate_article,
            ai_daily_brief,
            ai_article_chat,
            ai_cancel,
            stats_daily_read,
            // 导出命令（Obsidian 笔记 / 通用文本另存为）
            article_export_obsidian,
            save_text_file,
            // 过滤规则命令
            filters_list,
            filters_create,
            filters_update,
            filters_delete,
            // Settings 相关命令
            settings_get_all,
            settings_set,
            // 数据管理命令
            db_stats,
            db_backup,
            db_restore,
            db_cleanup,
            // 应用控制：数据库恢复后重启进程以加载新库
            app_restart,
            // 系统字体（设置页字体下拉）
            system_fonts_list,
            // 外部链接
            open_external,
            // 窗口控制
            win_minimize,
            win_toggle_maximize,
            win_close,
            // 实体管理命令
            entities_list,
            entities_create,
            entities_delete,
            entities_toggle,
            entity_graph,
            // 智能评分命令
            score_articles,
            high_score_articles,
            // 研究事件提取命令
            research_extract_article,
            research_extract_batch,
            research_events_by_article,
            research_events_timeline,
            research_event_delete,
            // 自动化任务命令
            automation_tasks_list,
            automation_tasks_create,
            automation_tasks_update,
            automation_tasks_delete,
            automation_tasks_set_enabled,
            automation_run_now,
            // 报告生成 / 读取命令（应用内查看器）
            research_report_generate,
            research_reports_list,
            research_report_read,
        ])
        // 启动失败属于不可恢复错误，直接 panic 而不是静默退出，
        // 便于在日志里看到明确原因
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}

// ─── Feed Commands ─────────────────────────────────────────────────────────────

/// 获取所有订阅源列表
///
/// # 返回值
/// 全部订阅源（按名称排序，不含分页）
///
/// # 错误
/// 数据库连接失败或查询失败时返回错误信息
#[tauri::command]
async fn feeds_list() -> Result<Vec<Feed>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Feed::list_all(&pool).await
}

/// 添加新的订阅源
///
/// 支持 RSSHub 集成：当 `url` 不是以 `http://` / `https://` 开头时，
/// 视为 RSSHub **路由**（如 `/twitter/user/xxx` 或 `twitter/user/xxx`），
/// 由 [`normalize_feed_url`] 探测出可用实例后拼成完整 URL。
/// 实例地址缺省时回退官方实例 `rsshub.app`，因此不再要求用户先配置实例。
///
/// 名称自动补全：当 `name` 为空、或与 URL 相同时（用户在"添加订阅源"弹窗里没填名称），
/// 先用**域名兜底名**立即落库并返回 ID，真实标题交由后台任务抓取后回填。
/// 抓取失败（网络不可达 / 源格式错误）时保持兜底名，不阻塞添加入口。
///
/// 之所以不在命令内同步等待抓取：一次 `fetch_feed` 最坏要耗尽 30 秒超时，
/// 期间前端列表既没有新条目也没有任何反馈，用户会误以为添加失败
/// （只有等下一次手动刷新 `loadFeeds()` 才看得到）。先落库能让新源即时可见。
///
/// # 参数
/// * `name` - 订阅源名称（可为空，此时自动抓取）
/// * `url` - RSS/Atom URL，或 RSSHub 路由
/// * `folder_id` - 所属文件夹 ID（0 表示未分类）
///
/// # 返回值
/// 新订阅源的 ID
///
/// # 错误
/// URL 为空、与已有订阅源重复（违反 UNIQUE 约束）或数据库写入失败时返回错误信息
///
/// # 副作用
/// 传入裸路由时会探测 RSSHub 实例（可能在数秒内返回），并可能把选中的实例写回设置。
///
/// # 注意
/// 图标（`icon`）在添加时先落一个兜底值（源站点根路径的 `/favicon.ico`，见
/// [`rss::fallback_icon`]），保证新源立即可展示图标；channel 自带的更优图标
/// 交由后续刷新流程覆盖，避免为了取图标而阻塞添加操作。
/// 订阅源"后台补全"完成事件载荷（事件名 `feed-updated`）
///
/// 新增订阅源时命令只负责落库并返回 ID，真实标题 / 图标 / 首屏文章由后台任务
/// 抓取回填；回填结束后用本事件通知前端就地更新该条目（避免整表重载，
/// 也避免用户手动刷新才看到结果）。
///
/// 字段名用 camelCase 序列化，与前端 TypeScript 消费习惯一致。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FeedUpdated {
    /// 订阅源 ID
    feed_id: i64,
    /// 回填后的最终名称（抓不到标题时为域名兜底名）
    name: String,
    /// 回填后的图标地址
    icon: Option<String>,
    /// 首屏文章入库后的未读数
    unread_count: i64,
    /// 首屏入库的新增文章数
    new_count: i64,
    /// 后台抓取失败原因（成功时为 `None`）
    ///
    /// 由"修改订阅链接"场景驱动：用户把地址改错时后台抓取只会静默失败，
    /// 界面上没有任何反馈，故把错误一并广播出去让前端提示。
    /// 该字段非空时，`name` / `icon` / `unread_count` 均为占位值，前端不得采信。
    error: Option<String>,
}

#[tauri::command]
async fn feeds_add(
    app: tauri::AppHandle,
    name: String,
    url: String,
    folder_id: i64,
) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    // 裸路由（如 /bilibili/user/video/xx）在此拼上「设置 → 常规」里配置的 RSSHub 实例前缀
    let final_url = normalize_feed_url(&pool, &url).await?;

    // 名称缺失（空串或与 URL 相同）时先用域名兜底名落库，真实标题后台回填。
    // 这是"手动新增订阅显示链接而非名称"的修复点：此前直接把 URL 当名称落库。
    let needs_resolve = name.trim().is_empty() || name.trim() == final_url;
    let initial_name = if needs_resolve {
        domain_fallback_name(&final_url)
    } else {
        name.trim().to_string()
    };

    // 未拿到 channel 图标时先退回站点根路径 favicon，保证新源至少有个可见图标
    let icon = rss::fallback_icon(&final_url);
    let id = Feed::add(&pool, &initial_name, &final_url, icon.as_deref(), folder_id).await?;

    // 自动归组：同一来源（域名 / RSSHub 平台）的订阅源达到阈值时自动建组并归入新源。
    // 用户显式指定了文件夹（folder_id != 0）则不介入，尊重手工分类；
    // 设置项 `auto_group_by_source`（默认开）控制本行为，"0" 为关闭；
    // 键缺失或读取失败都按默认开启，保持既有行为不变。
    // 归组失败只记日志——它属于锦上添花，不能拖垮"添加订阅"这个主流程。
    let auto_group = match Setting::get(&pool, "auto_group_by_source").await {
        Ok(Some(v)) => v != "0",
        _ => true,
    };
    if folder_id == 0 && auto_group {
        if let Err(e) = grouping::assign_source_group(&pool, id, &final_url).await {
            eprintln!("订阅源 {} 自动归组失败: {}", id, e);
        }
    }

    // 后台补全：抓真实标题 / 图标 + 首屏文章入库，完成后广播 feed-updated。
    // 刻意不 await——添加动作必须立即返回，网络结果异步回填。
    spawn_feed_backfill(app, pool, id, final_url, initial_name, needs_resolve);

    Ok(id)
}

/// 从 URL 提取可读的兜底订阅源名称（去掉协议、路径、端口与 www 前缀）
///
/// 用于"真实标题尚未抓回来"的这几秒里先占位显示，比直接显示一长串 URL
/// 更符合侧边栏的视觉预期；解析不出域名时原样返回传入的 URL。
///
/// 切分规则**复用 [`crate::rsshub::split_host_path`]**（同一套"去协议 / userinfo /
/// 端口 / `www.`"逻辑，主机名按规范转小写），避免分组模块与本模块各维护一份、
/// 将来对畸形 URL 的判定口径漂移。
///
/// # 参数
/// * `url` - 订阅源完整 URL
///
/// # 返回值
/// 形如 `example.com` 的短名称；解析失败时返回原 URL
fn domain_fallback_name(url: &str) -> String {
    crate::rsshub::host_of(url).unwrap_or_else(|| url.to_string())
}

/// 删除订阅源
///
/// 级联删除该源下的所有文章（依赖 `feeds` → `articles` 的外键 ON DELETE CASCADE）
///
/// # 参数
/// * `id` - 订阅源 ID
///
/// # 错误
/// 数据库删除失败时返回错误信息
#[tauri::command]
async fn feeds_delete(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Feed::delete(&pool, id).await
}

/// 更新订阅源（重命名 / 移动文件夹 / 修改链接）
///
/// 链接变更时会解锁一次后台抓取，把新地址的真实标题、图标与文章拉回来；
/// 链接未变则只落库，不制造无谓的网络往返。
///
/// # 参数
/// * `app` - AppHandle，供后台抓取完成后广播 `feed-updated`
/// * `id` - 订阅源 ID
/// * `url` - 订阅源链接（支持裸 RSSHub 路由，见 [`normalize_feed_url`]）
/// * `name` - 新的名称；留空表示保持原名
/// * `folder_id` - 新的所属文件夹 ID（0 表示移出文件夹、归入"未分类"）
///
/// # 错误
/// - 链接为空
/// - 链接与其它订阅源重复（由 [`Feed::update`] 翻译成可读提示）
///
/// # 已抓取文章
/// 保留不变：旧文章按 `UNIQUE(feed_id, guid)` 留在原源下，新地址的文章去重追加，
/// 因此书签与已读状态不会因换链接而丢失。
#[tauri::command]
async fn feeds_update(
    app: tauri::AppHandle,
    id: i64,
    url: String,
    name: String,
    folder_id: i64,
) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let final_url = normalize_feed_url(&pool, &url).await?;

    // 取旧值：既用于判断"链接是否真的变了"，也用于"名称留空 = 保持原名"
    let old: Option<(String, String)> = sqlx::query_as("SELECT url, name FROM feeds WHERE id = ?")
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let (old_url, old_name) = old.ok_or_else(|| "订阅源不存在".to_string())?;

    let final_name = if name.trim().is_empty() {
        old_name.clone()
    } else {
        name.trim().to_string()
    };
    let url_changed = old_url != final_url;

    Feed::update(&pool, id, Some(&final_url), &final_name, folder_id).await?;

    // 仅链接变更才重抓：改名 / 换文件夹都不需要网络往返。
    // 名称未被用户改动（仍等于原名）时允许被新源真实标题回填，否则会出现
    // "地址已指向新站、名字还留在旧站"的错配。
    if url_changed {
        let resolve_name = final_name == old_name;
        spawn_feed_backfill(app, pool, id, final_url, final_name, resolve_name);
    }
    Ok(())
}

/// 把用户输入的订阅地址规范化为可直接抓取的完整 URL
///
/// 容忍两种输入形态：
/// - **完整 `http(s)://` 地址**：原样返回。用户给的就是最终地址，
///   因此既不补前缀，也不做实例探测（哪怕它指向的是某个 RSSHub 实例）；
/// - **裸路由**（如 `/bilibili/user/video/xx`）：交给 [`rsshub::resolve`]
///   按候选清单探测出可用实例，再拼成完整 URL。
///
/// 新增订阅与修改链接共用本函数，保证同一输入在两处的落库结果一致。
///
/// # 参数
/// * `pool` - 数据库连接池（读写设置项 `rsshub_base_url`）
/// * `url` - 用户输入的原始地址或路由
///
/// # 返回值
/// 规范化后的完整 URL
///
/// # 副作用
/// 裸路由场景下会发起实例探测请求（带进程内缓存，通常只探测一次），
/// 并在选中实例与当前设置不同时把结果写回 `rsshub_base_url`——
/// 这样「设置 → 常规」里显示的实例就是实际在用的那个，不必让用户自己排查。
///
/// # 错误
/// 输入为空时返回错误信息。注意：**"未配置实例地址"不再是错误**，
/// 缺省时会回退到官方实例 `rsshub.app`。
async fn normalize_feed_url(pool: &sqlx::SqlitePool, url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("订阅链接不能为空".to_string());
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(url.to_string());
    }

    // 路由统一以 / 开头后拼接实例前缀，容忍用户漏写首斜杠
    let route = if url.starts_with('/') {
        url.to_string()
    } else {
        format!("/{}", url)
    };

    // 实例地址：键缺失或为空都视为未配置，由 rsshub 回退到官方实例
    let configured = Setting::get(pool, "rsshub_base_url")
        .await?
        .unwrap_or_default();
    let instance = rsshub::resolve(&configured, &route).await;

    // 探测结果与设置不一致时写回（如官方实例掉线后自动切到了备选实例）。
    // 写回失败不影响本次拼接——实例已经选好了，只是设置页的显示会滞后，故只记日志。
    if instance != rsshub::normalize_instance(&configured) {
        if let Err(e) = Setting::set(pool, "rsshub_base_url", &instance).await {
            eprintln!("RSSHub 实例地址写回设置失败: {}", e);
        }
    }

    Ok(format!("{}{}", instance, route))
}

/// 订阅源后台补全：抓取并回填标题 / 图标 / 首屏文章 / 未读数
///
/// 「新增订阅」与「修改链接」共用这一条链路——两者都是"地址已落库、需要把该地址的
/// 真实信息拉回来"的场景。刻意用 `tokio::spawn` 而非 `await`：命令必须立即返回，
/// 网络结果异步回填（同步等待会让 UI 卡满整个 30s 超时）。
///
/// # 参数
/// * `app` - AppHandle，用于广播 `feed-updated`
/// * `pool` - 数据库连接池
/// * `id` - 订阅源 ID
/// * `url` - 待抓取的订阅源地址
/// * `name` - 落库时的名称（同时作为文章所属源名写入）
/// * `resolve_name` - 是否允许用抓回的真实标题覆盖名称（用户手填过名称时为 `false`）
///
/// # 副作用
/// - 成功：回写名称 / 图标 / 文章 / 未读数，并广播 `feed-updated`
/// - 失败：广播带 `error` 的 `feed-updated`，让前端提示"链接可能填错了"——
///   这是修改链接场景下用户唯一可见的反馈
fn spawn_feed_backfill(
    app: tauri::AppHandle,
    pool: sqlx::SqlitePool,
    id: i64,
    url: String,
    name: String,
    resolve_name: bool,
) {
    tokio::spawn(async move {
        match rss::fetch_feed(&url).await {
            Ok(parsed) => {
                let mut final_name = name.clone();
                // 1. 真实标题：仅在名称是兜底值 / 用户没改过时覆盖，绝不覆盖用户手填的名称
                if resolve_name {
                    let title = parsed.title.trim().to_string();
                    if !title.is_empty() && title != final_name {
                        final_name = title;
                        if let Err(e) = sqlx::query("UPDATE feeds SET name = ? WHERE id = ?")
                            .bind(&final_name)
                            .bind(id)
                            .execute(&pool)
                            .await
                        {
                            eprintln!("后台更新订阅源 {} 的名称失败: {}", id, e);
                        }
                    }
                }
                // 2. 图标：channel 自带图标优先于落库时的 favicon 兜底
                if let Some(ic) = &parsed.icon {
                    if !ic.trim().is_empty() {
                        if let Err(e) = Feed::update_icon(&pool, id, ic).await {
                            eprintln!("后台更新订阅源 {} 的图标失败: {}", id, e);
                        }
                    }
                }
                // 3. 首屏文章入库。改链接场景下旧文章仍在，重复 guid 会被 OR IGNORE 跳过，
                //    因此这里得到的是"新地址带来的新增文章数"。
                let new_count =
                    match rss::store_items(&pool, id, &final_name, &parsed.items).await {
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("后台存储订阅源 {} 的文章失败: {}", id, e);
                            0
                        }
                    };
                // 4. 标记抓取时间（表达"尝试过"，避免失效源被高频重试）
                if let Err(e) =
                    sqlx::query("UPDATE feeds SET last_fetch_at = CURRENT_TIMESTAMP WHERE id = ?")
                        .bind(id)
                        .execute(&pool)
                        .await
                {
                    eprintln!("后台更新订阅源 {} 的抓取时间失败: {}", id, e);
                }
                // 5. 重算未读数并写回。
                //    这里不复用 scheduler::emit_feed_progress：那个函数会顺带广播
                //    refresh-progress（done=1/total=1），会让侧边栏刷新按钮在
                //    非刷新场景下闪出 "1/1" 进度文案。
                let unread: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM articles WHERE feed_id = ? AND is_read = 0",
                )
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap_or(0);
                if let Err(e) = sqlx::query("UPDATE feeds SET unread_count = ? WHERE id = ?")
                    .bind(unread)
                    .bind(id)
                    .execute(&pool)
                    .await
                {
                    eprintln!("后台回写订阅源 {} 的未读数失败: {}", id, e);
                }
                // 6. 广播回填结果，前端就地更新条目（名称 / 图标 / 未读数）
                let payload = FeedUpdated {
                    feed_id: id,
                    name: final_name,
                    icon: parsed.icon.clone(),
                    unread_count: unread,
                    new_count,
                    error: None,
                };
                if let Err(e) = app.emit("feed-updated", payload) {
                    eprintln!("广播 feed-updated 事件失败: {}", e);
                }
            }
            // 抓取失败：条目已在库中，广播错误让前端提示。
            // 载荷里的 name 是库中现有名称，其余字段为占位值，前端见到 error 会跳过它们。
            Err(e) => {
                eprintln!("后台补全订阅源 {} 失败: {}", id, e);
                let payload = FeedUpdated {
                    feed_id: id,
                    name,
                    icon: None,
                    unread_count: 0,
                    new_count: 0,
                    error: Some(e),
                };
                if let Err(err) = app.emit("feed-updated", payload) {
                    eprintln!("广播 feed-updated 事件失败: {}", err);
                }
            }
        }
    });
}

/// 把订阅源移动到指定文件夹（folder_id=0 表示移出、归入"未分类"）
///
/// # 参数
/// * `feed_id` - 订阅源 ID
/// * `folder_id` - 目标文件夹 ID
///
/// # 错误
/// 数据库写入失败时返回错误信息
#[tauri::command]
async fn feeds_set_folder(feed_id: i64, folder_id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Folder::set_feed_folder(&pool, feed_id, folder_id).await
}

/// 列出全部文件夹（侧边栏分组用）
///
/// # 返回值
/// 文件夹列表（按 position、id 升序）
#[tauri::command]
async fn folders_list() -> Result<Vec<Folder>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Folder::list_all(&pool).await
}

/// 新建文件夹
///
/// # 参数
/// * `name` - 文件夹名称
/// * `position` - 显示顺序
///
/// # 返回值
/// 新文件夹 ID
#[tauri::command]
async fn folders_create(name: String, position: i64) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Folder::create(&pool, &name, position).await
}

/// 重命名 / 调整文件夹顺序
///
/// # 参数
/// * `id` - 文件夹 ID
/// * `name` - 新名称
/// * `position` - 新顺序
#[tauri::command]
async fn folders_update(id: i64, name: String, position: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Folder::update(&pool, id, &name, position).await
}

/// 删除文件夹（其下订阅源自动移回"未分类"）
///
/// # 参数
/// * `id` - 文件夹 ID
#[tauri::command]
async fn folders_delete(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Folder::delete(&pool, id).await
}

/// 按来源自动分组（一键整理存量订阅源）
///
/// 把 URL 指向同一来源（域名 / RSSHub 平台）且达到 2 个的未分类订阅源
/// 归入以该来源命名的分组；已手工归类到文件夹的源不受影响。详见 [`grouping`]。
///
/// # 返回值
/// 新建分组数、归入的源数与涉及的分组名，供前端提示"整理了几个分组"。
///
/// # 错误
/// 读取设置 / 查询 / 写入失败时返回错误信息
#[tauri::command]
async fn feeds_auto_group() -> Result<grouping::GroupResult, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    grouping::auto_group(&pool).await
}

/// 列出全部过滤规则（设置页管理用）
///
/// # 返回值
/// 规则列表（按优先级、id 升序）
#[tauri::command]
async fn filters_list() -> Result<Vec<FilterRule>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    FilterRule::list_all(&pool).await
}

/// 新建过滤规则
///
/// # 参数
/// * `name` / `scope` / `scope_id` / `field` / `op` / `value` / `action` / `action_arg`
///   / `enabled` / `priority`
/// * `action_arg` - 动作参数，仅 `action = "tag"`（自动打标签）时有意义，存标签名
///
/// # 返回值
/// 新规则 ID
#[tauri::command]
async fn filters_create(
    name: String,
    scope: String,
    scope_id: i64,
    field: String,
    op: String,
    value: String,
    action: String,
    action_arg: String,
    enabled: bool,
    priority: i64,
) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    FilterRule::create(&pool, &name, &scope, scope_id, &field, &op, &value, &action, &action_arg, enabled, priority).await
}

/// 更新过滤规则（按 id 全量覆盖可改字段）
#[tauri::command]
async fn filters_update(
    id: i64,
    name: String,
    scope: String,
    scope_id: i64,
    field: String,
    op: String,
    value: String,
    action: String,
    action_arg: String,
    enabled: bool,
    priority: i64,
) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    FilterRule::update(&pool, id, &name, &scope, scope_id, &field, &op, &value, &action, &action_arg, enabled, priority).await
}

/// 删除过滤规则
///
/// # 参数
/// * `id` - 规则 ID
#[tauri::command]
async fn filters_delete(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    FilterRule::delete(&pool, id).await
}

/// 列出某文章的全部高亮片段
///
/// # 参数
/// * `article_id` - 文章 ID
///
/// # 返回值
/// 高亮列表（按起点偏移升序）
#[tauri::command]
async fn highlights_list(article_id: i64) -> Result<Vec<Highlight>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Highlight::list_by_article(&pool, article_id).await
}

/// 新建高亮片段
///
/// # 参数
/// * `article_id` / `text` / `note` / `color` / `start_offset` / `end_offset`
///
/// # 返回值
/// 新高亮 ID
#[tauri::command]
async fn highlights_create(
    article_id: i64,
    text: String,
    note: String,
    color: String,
    start_offset: i64,
    end_offset: i64,
) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Highlight::create(&pool, article_id, &text, &note, &color, start_offset, end_offset).await
}

/// 删除高亮片段
///
/// # 参数
/// * `id` - 高亮 ID
#[tauri::command]
async fn highlights_delete(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Highlight::delete(&pool, id).await
}

/// 列出全部标签（含各自文章数）
///
/// # 返回值
/// 标签列表，按名称升序；`article_count` 为该标签下的文章数
#[tauri::command]
async fn tags_list() -> Result<Vec<Tag>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Tag::list_all(&pool).await
}

/// 新建标签（同名标签已存在时复用，不报错）
///
/// # 参数
/// * `name` - 标签名（会 trim；空名报错）
/// * `color` - 颜色（仅新建时生效）
///
/// # 返回值
/// 标签 ID（新建的或既有的）
#[tauri::command]
async fn tags_create(name: String, color: String) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Tag::create(&pool, &name, &color).await
}

/// 重命名 / 修改标签颜色
///
/// 重命名会同步把引用旧名的过滤规则改指新名（见 [`db::Tag::update`]）。
///
/// # 参数
/// * `id` - 标签 ID
/// * `name` - 新名称
/// * `color` - 新颜色
#[tauri::command]
async fn tags_update(id: i64, name: String, color: String) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Tag::update(&pool, id, &name, &color).await
}

/// 删除标签
///
/// 关联行由外键级联删除；引用该标签名的规则会被清空动作参数（规则本身保留）。
///
/// # 参数
/// * `id` - 标签 ID
#[tauri::command]
async fn tags_delete(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Tag::delete(&pool, id).await
}

/// 整组覆盖某篇文章的标签（打标面板提交入口）
///
/// # 参数
/// * `article_id` - 文章 ID
/// * `tag_ids` - 该文章最终应有的标签 ID 集合；传空数组表示清空全部标签
#[tauri::command]
async fn article_tags_set(article_id: i64, tag_ids: Vec<i64>) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Tag::set_for_article(&pool, article_id, &tag_ids).await
}

/// 全文搜索文章（标题 / 正文 / 摘要子串匹配，含中文）
///
/// # 参数
/// * `query` - 搜索词
/// * `limit` - 返回上限
///
/// # 返回值
/// 命中的文章列表（按相关度排序）
#[tauri::command]
async fn articles_search(query: String, limit: i64) -> Result<Vec<Article>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::search_articles(&pool, &query, limit).await
}

/// 为缺失图标的存量订阅源回填兜底图标
///
/// 历史数据的 `icon` 列为空（旧版本添加时尚未落图标），在应用启动时由前端调用一次：
/// 逐个为空图标源推导站点 `/favicon.ico` 地址并写库。只补空值、不覆盖已有图标，
/// channel 自带的更优图标仍由刷新流程负责。该操作纯本地推导，不发起任何网络请求。
///
/// # 返回值
/// 本次实际回填的订阅源数量
///
/// # 错误
/// 数据库连接、查询或写入失败时返回错误信息
#[tauri::command]
async fn feeds_backfill_icons() -> Result<usize, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let feeds = Feed::list_all(&pool).await?;
    let mut filled = 0usize;

    for feed in feeds {
        // 仅处理空图标（NULL 或空白串）的源，已有图标的源一律不动
        let missing = feed
            .icon
            .as_deref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
        if missing {
            if let Some(icon) = rss::fallback_icon(&feed.url) {
                Feed::update_icon(&pool, feed.id, &icon).await?;
                filled += 1;
            }
        }
    }

    Ok(filled)
}

/// 从 OPML 文件批量导入订阅源（文件由用户在系统对话框中选择）
///
/// **文件的选择与读取都在后端**：先弹系统「打开文件」对话框，用户取消则直接返回；
/// 选中后由 Rust 读盘、解析并入库。前端不做任何文件操作 —— 与 [`feeds_export_opml`] 对称，
/// 也免去了把文件内容经 IPC 传一遍的开销。
///
/// # 返回值
/// * `Ok(Some(ids))` - 新插入订阅源的 ID 列表；已存在（URL 重复）的源被自动忽略，不计入
/// * `Ok(None)` - 用户取消，未做任何改动
///
/// # 错误
/// 读取文件失败、OPML 格式损坏无法解析、或数据库写入失败时返回错误信息
#[tauri::command]
async fn feeds_import_opml(app: tauri::AppHandle) -> Result<Option<Vec<i64>>, String> {
    let Some(src) = pick_open_path(&app, "OPML 文件", &["opml", "xml"]).await? else {
        return Ok(None);
    };

    // 按字节读再用 lossy UTF-8 解码：与原前端 `file.text()` 的行为一致（非法字节替换而非报错），
    // 避免"以前能导入的 GBK/BOM 文件改后端后反而失败"
    let bytes = tokio::fs::read(&src)
        .await
        .map_err(|e| format!("读取 OPML 文件失败: {}", e))?;
    let content = String::from_utf8_lossy(&bytes);

    // 解析阶段与数据库无关，先单独解析，便于把格式错误尽早返回给用户
    let parsed = opml::parse_opml(&content)?;
    if parsed.is_empty() {
        // 空列表不算错误：可能是只含分类文件夹的 OPML，直接返回空结果
        return Ok(Some(Vec::new()));
    }

    let pool = db_pool().await.map_err(|e| e.to_string())?;
    // 名称缺失时回退为 URL，保证导入后条目可读
    let items: Vec<(String, String)> = parsed
        .into_iter()
        .map(|f| {
            let title = if f.title.is_empty() {
                f.xml_url.clone()
            } else {
                f.title
            };
            (title, f.xml_url)
        })
        .collect();

    let ids = Feed::import_batch(&pool, &items).await?;
    Ok(Some(ids))
}

// ─── 通用辅助：系统「另存为」对话框 ────────────────────────────────────────────

/// OPML 导出文件的默认名（仅作对话框初始值，用户可改）
const DEFAULT_OPML_FILENAME: &str = "readflow-subscriptions.opml";

/// 弹出系统「另存为」对话框，返回用户选定的绝对路径
///
/// 用 [`tokio::sync::oneshot`] 等待对话框的回调结果，而**不用** `blocking_save_file()`：
/// 命令跑在 tokio 工作线程上，阻塞该线程会挤占异步运行时。回调形式天然非阻塞，
/// 等待期间运行时仍能调度其它任务。
///
/// # 参数
/// * `app` - 应用句柄，用于取得对话框实例
/// * `default_name` - 对话框中预填的文件名（用户可改）
/// * `filter_name` - 文件类型过滤器的显示名（如「OPML 文件」）
/// * `extensions` - 允许的扩展名，不含点（如 `&["opml"]`）
///
/// # 返回值
/// * `Ok(Some(path))` - 用户选定的绝对路径
/// * `Ok(None)` - 用户取消；调用方应视为"什么都没发生"，不写盘也不报错
///
/// # 错误
/// 对话框异常关闭（回调未触发即断开），或所选路径无法解析为本地路径时返回错误信息
async fn pick_save_path(
    app: &tauri::AppHandle,
    default_name: &str,
    filter_name: &str,
    extensions: &[&str],
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(default_name)
        .add_filter(filter_name, extensions)
        .save_file(move |picked| {
            // 接收端可能已被丢弃（命令提前返回），发送失败无需处理
            let _ = tx.send(picked);
        });

    let picked = rx.await.map_err(|_| "保存对话框异常关闭".to_string())?;
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|e| format!("无法解析所选保存路径: {}", e))?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// 弹出系统「打开文件」对话框，返回用户选定的绝对路径
///
/// 与 [`pick_save_path`] 对称：同样用 [`tokio::sync::oneshot`] 等待对话框回调结果，
/// 而**不用** `blocking_pick_file()` —— 命令跑在 tokio 工作线程上，阻塞它会挤占异步运行时。
///
/// 与「另存为」的唯一差别是不设置初始文件名（打开语义下无意义），只挂文件类型过滤器。
///
/// # 参数
/// * `app` - 应用句柄，用于取得对话框实例
/// * `filter_name` - 文件类型过滤器的显示名（如「SQLite 数据库」）
/// * `extensions` - 允许的扩展名，不含点（如 `&["db"]`）
///
/// # 返回值
/// * `Ok(Some(path))` - 用户选定的绝对路径
/// * `Ok(None)` - 用户取消；调用方应视为"什么都没发生"，不读文件也不报错
///
/// # 错误
/// 对话框异常关闭（回调未触发即断开），或所选路径无法解析为本地路径时返回错误信息
async fn pick_open_path(
    app: &tauri::AppHandle,
    filter_name: &str,
    extensions: &[&str],
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter(filter_name, extensions)
        .pick_file(move |picked| {
            // 接收端可能已被丢弃（命令提前返回），发送失败无需处理
            let _ = tx.send(picked);
        });

    let picked = rx.await.map_err(|_| "打开对话框异常关闭".to_string())?;
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|e| format!("无法解析所选文件路径: {}", e))?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// 导出全部订阅源为 OPML 文件（保存路径由用户在系统对话框中选择）
///
/// 刻意**先弹对话框再查库**：用户取消时直接返回，省掉一次读库与序列化。
///
/// OPML 的 `category` 属性由**文件夹名**填充（文件夹是本应用唯一的分组语义），
/// 未归入文件夹的源不写该属性。
///
/// # 返回值
/// * `Ok(Some(path))` - 已写入文件的绝对路径（前端据此提示"已导出到 …"）
/// * `Ok(None)` - 用户取消，未写任何文件
///
/// # 错误
/// 数据库连接或查询失败、文件写入失败时返回错误信息
#[tauri::command]
async fn feeds_export_opml(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(dest) =
        pick_save_path(&app, DEFAULT_OPML_FILENAME, "OPML 文件", &["opml"]).await?
    else {
        return Ok(None);
    };

    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let feeds = Feed::list_all(&pool).await?;
    let folders = Folder::list_all(&pool).await?;
    let xml = opml::export_opml(&feeds, &folders);

    // 用 tokio 的文件 API：命令跑在异步运行时里，同步写会短暂阻塞工作线程
    tokio::fs::write(&dest, xml)
        .await
        .map_err(|e| format!("写入 OPML 文件失败: {}", e))?;
    Ok(Some(dest))
}

/// 把一段前端生成的文本存成本地文件（保存路径由用户在系统对话框中选择）
///
/// 存在的意义是守住"**文件读写只在 Rust 侧**"这条边界：浏览器环境里的
/// `Blob` + `a.download` 在 Tauri 的 WebView 中行为不可靠（不会弹系统保存框，
/// 也可能被静默拦下），而让前端自己写盘则等于把任意路径写入能力暴露给渲染进程。
/// 统一走系统「另存为」对话框，用户显式选定路径——写入范围因此天然受限于用户意图。
///
/// 与 [`feeds_export_opml`] 是同一条通道、同一套语义，只是内容由调用方给定。
/// 典型用途：导出文章问答的对话记录为 Markdown。
///
/// # 参数
/// * `app` - 应用句柄，用于弹出系统保存对话框
/// * `default_name` - 对话框中预填的文件名（用户可改）
/// * `filter_name` - 文件类型过滤器的显示名（如「Markdown 文件」）
/// * `extensions` - 允许的扩展名，不含点（如 `&["md"]`）
/// * `content` - 待写入的完整文本
///
/// # 返回值
/// * `Ok(Some(path))` - 已写入文件的绝对路径（前端据此提示"已导出到 …"）
/// * `Ok(None)` - 用户取消，未写任何文件
///
/// # 错误
/// 对话框异常关闭、路径无法解析或文件写入失败时返回错误信息
#[tauri::command]
async fn save_text_file(
    app: tauri::AppHandle,
    default_name: String,
    filter_name: String,
    extensions: Vec<String>,
    content: String,
) -> Result<Option<String>, String> {
    // 先弹对话框再做事：用户取消时直接返回，不做任何多余工作
    let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    let Some(dest) = pick_save_path(&app, &default_name, &filter_name, &ext_refs).await? else {
        return Ok(None);
    };

    tokio::fs::write(&dest, content)
        .await
        .map_err(|e| format!("写入文件失败: {}", e))?;
    Ok(Some(dest))
}

/// 手动触发所有订阅源刷新
///
/// 优先复用全局刷新管理器（它持有调度所需的频率与时间戳状态），
/// 若管理器尚未就绪则退化为一次性直连刷新。
///
/// # 返回值
/// 本次刷新新增的文章数量
///
/// # 错误
/// 管理器可用时：返回管理器的刷新结果；
/// 降级路径下：数据库连接失败或源列表读取失败时返回错误信息
///
/// # 注意
/// 无论走哪条路径，刷新过程都会逐源广播 `refresh-progress` 事件，
/// 前端据此实时更新侧边栏未读数与 n/m 进度；单个源抓取或入库失败只打印日志并继续，
/// 否则一个失效的 RSS 源会让整体刷新无法完成。
#[tauri::command]
async fn feeds_refresh(app: tauri::AppHandle) -> Result<i64, String> {
    // 优先使用全局管理器
    //
    // 用块包裹是为了让 MutexGuard 在块结束时释放再进入后续 await，
    // 否则 guard 会跨越整个降级路径持有锁，阻塞其它可能访问该全局量的命令。
    {
        if let Some(manager) = REFRESH_MANAGER.lock().await.as_ref() {
            return manager.manual_refresh(&app).await;
        }
    }

    // 降级方案：直接刷新（复用调度器的统一实现，逐源进度广播行为与管理器路径一致）
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    scheduler::refresh_all_feeds(&pool, Some(&app)).await
}

// ─── Article Commands ──────────────────────────────────────────────────────────

/// 获取指定订阅源的文章列表（分页 + 排序）
///
/// # 参数
/// * `feed_id` - 订阅源 ID；`null` 表示全部订阅源（统一时间线）
/// * `tag_id` - 标签 ID；`null` 表示不按标签筛选（可与 `feed_id` 叠加）
/// * `bookmarked` - `true` 时只返回已收藏的文章（书签视图）；`null` / `false` 均不筛选。
///   该范围必须在后端过滤——前端每页只有 50 条，收藏散落在上千条历史里时
///   靠「加载更多」逐步露出既慢又会让 `hasMore` 失真
/// * `unread` - `true` 时只返回未读的文章（未读视图）；`null` / `false` 均不筛选。
///   理由同 `bookmarked`：未读同样会散落在上千条历史里
/// * `sort` - 排序方式：`latest`（默认，最新）/ `score`（重要，按实体评分）/ `title`（名称）；
///   无法识别的值回退 `latest`，前端无需先做白名单校验
/// * `limit` - 每页数量
/// * `offset` - 偏移量
///
/// # 返回值
/// 文章列表，按 `sort` 指定的顺序排列（缺少排序字段的条目排在末尾）；每篇的 `tags` 已挂载
///
/// # 错误
/// 数据库连接或查询失败时返回错误信息
#[tauri::command]
async fn articles_list(
    feed_id: Option<i64>,
    tag_id: Option<i64>,
    bookmarked: Option<bool>,
    unread: Option<bool>,
    sort: Option<String>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Article>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let sort = ArticleSort::from_key(sort.as_deref().unwrap_or("latest"));
    Article::list_by_feed(&pool, feed_id, tag_id, bookmarked, unread, sort, limit, offset).await
}

/// 获取文章详情
///
/// # 参数
/// * `id` - 文章 ID
///
/// # 返回值
/// 文章存在时 `Some(article)`；不存在时 `None`（**不视为错误**，
/// 前端据此渲染空态，而非弹出错误提示）
///
/// # 错误
/// 数据库连接或查询失败时返回错误信息
#[tauri::command]
async fn article_get(id: i64) -> Result<Option<Article>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Article::get_by_id(&pool, id).await
}

/// 标记文章为已读
///
/// # 参数
/// * `id` - 文章 ID
///
/// # 注意
/// 此处不同步 `feeds.unread_count`，未读计数由刷新流程末尾的批量重算负责；
/// 前端的侧边栏数字因此可能存在短暂滞后，直到下次刷新。
///
/// # 错误
/// 数据库写入失败时返回错误信息
#[tauri::command]
async fn article_read(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Article::mark_as_read(&pool, id).await
}

/// 切换文章收藏状态
///
/// # 参数
/// * `id` - 文章 ID
///
/// # 返回值
/// 收藏后的状态（true=已收藏，false=已取消）
///
/// # 错误
/// 文章不存在或数据库写入失败时返回错误信息
#[tauri::command]
async fn article_bookmark(id: i64) -> Result<bool, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Article::toggle_bookmark(&pool, id).await
}

/// 统计未读文章数（可按订阅源 / 标签限定范围）
///
/// 供列表栏顶部「全部已读」按钮使用：单源 / 全部视图的未读数能从前端已有的
/// `feeds.unread_count` 直接拿到，但**标签视图没有预存数字**（标签横跨所有源），
/// 只能回后端数一次，否则按钮的计数会退化成"已加载那几页里的未读"。
///
/// # 参数
/// * `feed_id` - 订阅源 ID；`null` 表示不限源
/// * `tag_id` - 标签 ID；`null` 表示不限标签（两者可叠加）
///
/// # 返回值
/// 满足范围的未读文章条数
///
/// # 错误
/// 数据库连接或查询失败时返回错误信息
#[tauri::command]
async fn articles_unread_count(feed_id: Option<i64>, tag_id: Option<i64>) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Article::count_unread(&pool, feed_id, tag_id).await
}

/// 将文章导出为 Markdown 笔记写入用户配置的 Obsidian Vault 目录
///
/// 委托 `obsidian::export_to_vault` 完成「读取文章 → 拼装 Markdown → 落盘」全流程，
/// 本命令只做"取参数 → 建连接 → 调下层"的薄边界职责（符合命令层不写业务逻辑的规则）。
///
/// 详细的导出策略（Vault 路径 / 导出目录 / 译文优先 / frontmatter 结构）见
/// [`obsidian::export_to_vault`] 文档。
///
/// # 参数
/// * `article_id` - 待导出的文章 ID
///
/// # 返回值
/// 成功时返回人类可读的提示（含相对路径），如 `已导出到 ReadFlow/标题.md`
///
/// # 错误
/// 文章不存在、Vault 路径未配置、目录不可写或写入失败时返回错误信息（字符串）
#[tauri::command]
async fn article_export_obsidian(article_id: i64) -> Result<String, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    obsidian::export_to_vault(&pool, article_id).await
}

/// 精确设置文章的已读 / 未读状态
///
/// 见 [`db::Article::set_read`]：写入状态并同步所属订阅源的 `unread_count`，
/// 使正文工具条的"标记为已读/未读"按钮可以双向切换，且侧边栏未读角标即时更新。
///
/// # 参数
/// * `id` - 文章 ID
/// * `is_read` - true=标记为已读，false=标记为未读
///
/// # 错误
/// 数据库写入失败或所属源未读数重算失败时返回错误信息
#[tauri::command]
async fn article_set_read(id: i64, is_read: bool) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Article::set_read(&pool, id, is_read).await
}

/// 记录文章的阅读进度（正文滚动比例）
///
/// 见 [`db::Article::set_progress`]：由前端在正文滚动时节流上报，
/// 供重开该文时恢复上次的阅读位置。
///
/// **刻意不做参数校验、也不返回内容**：进度不是关键路径，前端静默失败即可；
/// 越界与非有限值统一由 `set_progress` 内部 clamp，避免边界逻辑在两处各写一遍。
///
/// # 参数
/// * `id` - 文章 ID
/// * `progress` - 滚动比例，0.0 = 顶部，1.0 = 底部
///
/// # 错误
/// 数据库写入失败时返回错误信息（前端忽略，不打断阅读）
#[tauri::command]
async fn article_set_progress(id: i64, progress: f64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Article::set_progress(&pool, id, progress).await
}

/// 批量标记文章为已读（文章列表顶部"全部已读"按钮触发）
///
/// 见 [`db::Article::mark_all_read`]：按 `feed_id` / `tag_id` 范围一次性置已读，
/// 并统一重算所有订阅源的未读数，保证批量操作后侧边栏数字立即归零。
///
/// # 参数
/// * `feed_id` - 订阅源 ID；`null` 表示不按源限定
/// * `tag_id` - 标签 ID；`null` 表示不按标签限定。标签视图下必须传入，
///   否则"全部已读"会越界清空全库未读
///
/// # 返回值
/// 实际被更新的文章行数（已读的不重复计入）
///
/// # 错误
/// 数据库写入或重算未读数失败时返回错误信息
#[tauri::command]
async fn articles_mark_all_read(
    feed_id: Option<i64>,
    tag_id: Option<i64>,
) -> Result<u64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Article::mark_all_read(&pool, feed_id, tag_id).await
}

/// 只刷新指定的单个订阅源（文章列表顶部"刷新当前源"按钮触发）
///
/// 与 [`feeds_refresh`]（全量刷新所有源）不同，本命令只抓取传入的这一个源，
/// 适合"当前列表明明有新内容却没显示"时的局部重试，避免把时间花在无关的源上。
/// 流程与全量刷新一致：抓取 → 入库（去重更新）→ 同步图标 → 更新 last_fetch_at → 重算该源未读数，
/// 并广播一次 `refresh-progress` 事件（done=1/total=1），驱动前端未读数即时更新。
///
/// # 参数
/// * `feed_id` - 待刷新的订阅源 ID
/// * `app` - 发起本次刷新的应用句柄（透传给进度事件广播）
///
/// # 返回值
/// 本次新增的文章条数
///
/// # 错误
/// 源不存在、抓取失败或写入失败时返回错误信息
#[tauri::command]
async fn feeds_refresh_one(feed_id: i64, app: tauri::AppHandle) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    // 先用 ID 取回该源的完整信息（url 等），源不存在时明确报错
    let feed = match Feed::get_by_id(&pool, feed_id).await? {
        Some(f) => f,
        None => return Err(format!("订阅源不存在: id={}", feed_id)),
    };

    match rss::fetch_feed(&feed.url).await {
        Ok(parsed) => {
            // 入库：store_items 内部按 UNIQUE(feed_id, guid) 去重，返回实际新增条数
            let count = rss::store_items(&pool, feed.id, &feed.name, &parsed.items)
                .await
                .map_err(|e| format!("存储 {} 失败: {}", feed.name, e))?;
            // 图标同步：仅在值变化时写库（与 feeds_refresh 同一约定，避免每轮无谓 UPDATE）
            if let Some(icon) = &parsed.icon {
                if feed.icon.as_deref().unwrap_or("") != icon {
                    if let Err(e) = Feed::update_icon(&pool, feed.id, icon).await {
                        eprintln!("更新 {} 的图标失败: {}", feed.name, e);
                    }
                }
            }
            // 标记本次尝试时间（"尝试过"而非"有新内容"，避免时效源被高频重试）
            sqlx::query("UPDATE feeds SET last_fetch_at = CURRENT_TIMESTAMP WHERE id = ?")
                .bind(feed.id)
                .execute(&pool)
                .await
                .map_err(|e| e.to_string())?;
            // 重算该源未读数、写回 feeds 表并广播单源进度事件（done=1/total=1）。
            // 复用调度器的 emit_feed_progress，与全量刷新的载荷格式保持一致；
            // 该函数对 SQL/事件失败自带日志兜底，不影响本次刷新结果
            scheduler::emit_feed_progress(&pool, Some(&app), feed.id, &feed.name, count, 1, 1).await;
            Ok(count)
        }
        // 抓取失败直接返回错误，由前端提示（不静默吞掉，否则用户以为刷新成功）
        Err(e) => Err(format!("抓取 {} 失败: {}", feed.name, e)),
    }
}

// ─── Settings Commands ─────────────────────────────────────────────────────────

/// 获取所有设置
///
/// # 返回值
/// 全部 key-value 条目（`settings` 表的全量快照）
///
/// # 错误
/// 数据库连接或查询失败时返回错误信息
#[tauri::command]
async fn settings_get_all() -> Result<Vec<Setting>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Setting::get_all(&pool).await
}

/// 设置或更新一个配置项
///
/// # 参数
/// * `key` - 设置键
/// * `value` - 设置值（原始字符串；复杂结构请由前端先序列化为 JSON 文本）
///
/// # 错误
/// 数据库写入失败时返回错误信息
///
/// # 注意
/// 底层走 UPSERT，同一 key 重复写入是幂等的更新而非插入。
/// 由于值不加校验，API Key 等敏感项也以明文落库。
#[tauri::command]
async fn settings_set(key: String, value: String) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    Setting::set(&pool, &key, &value).await
}

/// 获取全库数据统计
///
/// 委托 `db::get_stats` 一次性汇总订阅源数、文章总数、已读/未读/收藏分布，
/// 供前端"数据"标签页展示。结果只读、无副作用。
///
/// # 返回值
/// 包含各项计数的 [`db::Stats`] 快照
///
/// # 错误
/// 数据库查询失败时返回错误信息
#[tauri::command]
async fn db_stats() -> Result<db::Stats, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::get_stats(&pool).await
}

/// 数据库备份（把 SQLite 文件导出到用户选定的路径）
///
/// 与 OPML 导出同一套交互：先弹系统「另存为」对话框，再把数据库文件复制过去。
/// **不再经 Base64 走 IPC**——数据库可能达到数百 MB，Base64 会让体积膨胀约 1/3，
/// 还要在前后端之间多传一份；由 Rust 直接复制文件既省内存也省时间。
///
/// 直接复制主库文件成立的前提是**未启用 WAL**（本项目使用 SQLite 默认的 journal
/// 模式，所有数据都在主库文件内）；若将来改用 WAL，这里必须先做一次 checkpoint，
/// 否则会漏掉尚在 `-wal` 文件里的已提交数据。
///
/// # 返回值
/// * `Ok(Some(path))` - 已写入的备份文件绝对路径
/// * `Ok(None)` - 用户取消，未写任何文件
///
/// # 错误
/// 数据库文件读取或复制失败时返回错误信息
#[tauri::command]
async fn db_backup(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(dest) =
        pick_save_path(&app, &db::default_backup_file_name(), "SQLite 数据库", &["db"]).await?
    else {
        return Ok(None);
    };

    db::db_backup_to(&app, &dest).await?;
    Ok(Some(dest))
}

/// 数据库恢复（用指定的备份文件覆盖当前数据库）
///
/// **文件的选择与读取都在后端**：先弹系统「打开文件」对话框，用户取消则直接返回；
/// 选中后交给 [`db::db_restore_from`] 完成校验、留底与覆盖。与 [`db_backup`] 对称，
/// 不再把整个数据库 Base64 编码后经 IPC 传一遍（几百 MB 会膨胀约 1/3 并多占一份内存）。
///
/// 覆盖后已建立的连接池仍指向旧库，**必须重启应用**才能真正加载新数据；
/// 前端在展示完"已恢复"提示后调用 [`app_restart`] 完成重启。
///
/// # 返回值
/// * `Ok(Some(path))` - 实际用于恢复的备份文件绝对路径
/// * `Ok(None)` - 用户取消，未做任何改动
///
/// # 错误
/// 所选文件不是有效的 SQLite 数据库、所选文件就是当前库、留底或覆盖失败时返回错误信息
#[tauri::command]
async fn db_restore(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(src) = pick_open_path(&app, "SQLite 数据库", &["db"]).await? else {
        return Ok(None);
    };
    db::db_restore_from(&app, &src).await?;
    Ok(Some(src))
}

/// 重启应用进程
///
/// 数据库恢复后必须重启，才能让已建立的连接池（以及后台刷新调度器持有的长连接）
/// 切换到新库——否则旧连接可能把旧页缓存写回新文件，造成数据混杂。
///
/// 刻意**由前端在展示完提示之后再调用**，而不是在 [`db_restore`] 内部直接重启：
/// 命令的返回值需要先送达前端，否则用户看不到"已从 … 恢复"的反馈。
///
/// # 注意
/// 本命令不会正常返回：`AppHandle::restart` 的返回类型是 `!`，进程会被直接替换。
#[tauri::command]
fn app_restart(app: tauri::AppHandle) {
    app.restart();
}

/// 立即按当前保留策略清理旧文章
///
/// 把前端的 `keepDays` / `keepUnread` 透传给 [`db::cleanup_old_articles`]；
/// 通常前端会从已保存的设置回填这两个值，从而与后台自动清理使用同一套策略。
///
/// # 参数
/// * `keep_days` - 保留天数；0 表示永久保留，不执行删除
/// * `keep_unread` - 为 true 时未读文章不被清理（避免误删未看内容）
///
/// # 返回值
/// 被删除的文章条数（前端据此提示"已清理 N 篇"）
///
/// # 错误
/// 数据库删除或重算未读计数失败时返回错误信息
#[tauri::command]
async fn db_cleanup(keep_days: i64, keep_unread: bool) -> Result<u64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::cleanup_old_articles(&pool, keep_days, keep_unread).await
}

// ─── 系统字体命令 ─────────────────────────────────────────────────────────────

/// 列出本机已安装的系统字体族
///
/// 供「设置 → 常规」的「界面字体 / 内容字体」下拉使用：枚举本机字体目录并解析
/// 字体文件自身的族名（详见 [`fonts`] 模块）。扫描结果在进程内缓存，
/// 重复调用不会重复读盘。
///
/// # 返回值
/// 已去重排序的字体族列表：`name` 为 CSS `font-family` 用名，`label` 为展示名。
/// 扫描失败（字体目录不可读等）时返回空列表，前端下拉退化为仅显示内置预设，
/// 因此本命令刻意不返回 `Result` —— 字体枚举失败不该阻塞设置页渲染。
#[tauri::command]
async fn system_fonts_list() -> Vec<fonts::FontFamily> {
    fonts::system_fonts().await
}

/// 在系统默认浏览器中打开外部链接
///
/// 出于安全考虑，应用内的 WebView 不应直接导航到外部站点（会污染应用上下文、
/// 可能绕过 CSP 并把用户带到不可控页面）。这里使用系统命令直接在独立浏览器进程中打开链接。
///
/// # 参数
/// * `url` - 待打开的外部链接，必须是 `http://` 或 `https://`
///
/// # 错误
/// - URL 协议不在白名单内（非 http/https）：返回 `只支持 http/https 链接`
/// - 系统命令执行失败（例如无默认浏览器）：返回底层错误信息
///
/// # 注意
/// 协议白名单是关键安全边界：只放行 http/https，可杜绝把 `javascript:`、
/// `file:` 等伪协议直接交给系统打开器执行的风险。
#[tauri::command]
async fn open_external(url: String) -> Result<(), String> {
    // 协议白名单：仅放行 http/https，避免任意字符串被交给系统打开器执行
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("只支持 http/https 链接".to_string());
    }

    // 使用系统命令打开浏览器，避免 Tauri shell 插件的废弃 API
    let spawn_result = (|| -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(&url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("打开链接失败: {}", e))?;
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(&["/C", "start", "", &url])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("打开链接失败: {}", e))?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(&url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("打开链接失败: {}", e))?;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            return Err("不支持的操作系统".to_string());
        }
        Ok(())
    })();

    spawn_result
}

// ─── AI Commands ───────────────────────────────────────────────────────────────

/// AI 流式增量事件的载荷（事件名 `ai-stream`）
///
/// 模型每吐出一批文本就广播一次，让摘要卡片与译文正文"逐字浮现"，
/// 而不是让界面在整段生成期间完全静止。
/// 字段名用 camelCase 序列化，与前端 TypeScript 消费习惯一致
/// （同 `refresh-progress` / `feed-updated` 的既有约定）。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiStreamPayload<'a> {
    /// 前端生成并回传的请求标识，用于丢弃过期请求的迟到事件
    request_id: &'a str,
    /// 产物类型（`summary` / `translate`），界面据此决定渲染到哪块区域
    kind: &'a str,
    /// 文章 ID
    article_id: i64,
    /// 本批新增的文本（已按阈值合并过，不是逐 token）
    delta: &'a str,
}

/// 构造把流式增量转发为 `ai-stream` 事件的回调
///
/// 抽成函数而非在两个命令里各写一遍闭包：摘要与翻译的回调逻辑完全相同，
/// 只有 `kind` 不同，集中一处也便于统一"发送失败不致命"的处置。
/// 事件发送失败只记日志——界面拿不到增量最多是少了个逐字效果，
/// 而 `invoke` 的返回值里仍有完整文本，不该因此让整个生成流程失败。
///
/// # 参数
/// * `app_handle` - 应用句柄（Tauri 自动注入，不经 IPC 权限检查）
/// * `request_id` - 前端传来的请求标识，原样回传
/// * `kind` - 产物类型（`summary` / `translate`）
/// * `article_id` - 文章 ID
///
/// # 返回值
/// 可直接传给 [`ai::generate_summary`] / [`ai::translate_article`] 的增量回调
fn delta_sink(
    app_handle: AppHandle,
    request_id: String,
    kind: &'static str,
    article_id: i64,
) -> impl Fn(&str) + Send + Sync {
    move |delta: &str| {
        let payload = AiStreamPayload {
            request_id: &request_id,
            kind,
            article_id,
            delta,
        };
        if let Err(e) = app_handle.emit("ai-stream", payload) {
            eprintln!("发送 AI 流式事件失败（不影响生成结果）: {}", e);
        }
    }
}

/// 生成文章 AI 摘要
///
/// 依据 `provider_id` 从 settings 表读取该供应商的 url / key / model，
/// 组装 [`ai::AIProvider`] 后调用 [`ai::generate_summary`]。
/// 生成过程中模型吐出的每一批增量都会以 `ai-stream` 事件推给前端。
///
/// # 参数
/// * `article_id` - 文章 ID
/// * `provider_id` - AI 提供商标识，用于拼出三个配置键：
///   `ai_{provider_id}_url`、`ai_{provider_id}_key`、`ai_{provider_id}_model`
/// * `request_id` - 前端生成的请求标识，随每个流式事件回传
/// * `app_handle` - 应用句柄，用于发射流式事件
///
/// # 返回值
/// 生成的摘要文本（已同步写入 `articles.ai_summary`）
///
/// # 错误
/// - 任一项配置缺失：`AI provider URL/API key/model not configured`
/// - 下游 [`ai::generate_summary`] 的各类错误（文章不存在、网络失败、落库失败）
#[tauri::command]
async fn ai_generate_summary(
    article_id: i64,
    provider_id: String,
    request_id: String,
    app_handle: AppHandle,
) -> Result<String, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;

    // 获取 AI 提供商配置
    //
    // 逐项取值而非一次性聚合查询：settings 是扁平的 key-value 结构，
    // 分别读取可以让"哪一项缺失"在错误信息中被精确定位，便于用户排查。
    // 任一项缺失即刻短路返回错误，避免带着不完整的凭据去打网络请求。
    // 空串同样按"未配置"处理：密钥的种子值现在是空串，而 `Setting::get` 对空串
    // 返回的是 `Some("")` 而非 `None`。不加这层过滤，"没填密钥"就会变成一次
    // 注定 401 的网络请求，用户看到的是服务端错误而不是"请先配置"。
    let api_url = Setting::get(&pool, &format!("ai_{}_url", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider URL not configured")?;
    let api_key = Setting::get(&pool, &format!("ai_{}_key", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider API key not configured")?;
    let model = Setting::get(&pool, &format!("ai_{}_model", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider model not configured")?;

    let provider = ai::AIProvider {
        api_url,
        api_key,
        model,
    };

    // 把模型吐出的每一批增量转发成事件；requestId 由前端生成并回传，
    // 前端据此丢弃上一轮请求的迟到事件（例如连续点两次「重新生成」）
    let sink = delta_sink(app_handle, request_id, "summary", article_id);
    ai::generate_summary(&pool, article_id, &provider, &sink).await
}

/// 翻译文章内容
///
/// 与 [`ai_generate_summary`] 共用同一套"读配置 → 组装 provider → 调用 `ai` 层"的流程，
/// 也同样把生成过程中的增量以 `ai-stream` 事件推出（译文更长，逐字效果的价值更大）。
///
/// # 参数
/// * `article_id` - 文章 ID
/// * `provider_id` - AI 提供商配置键（拼键规则同 [`ai_generate_summary`]）
/// * `target_language` - 目标语言，以自然语言串原样填入 prompt（例如 "中文"）
/// * `request_id` - 前端生成的请求标识，随每个流式事件回传
/// * `app_handle` - 应用句柄，用于发射流式事件
///
/// # 返回值
/// 生成的译文（已同步写入 `articles.ai_translation`）
///
/// # 错误
/// 配置缺失时返回 `AI provider URL/API key/model not configured`；
/// 其余错误由下游 [`ai::translate_article`] 透传
#[tauri::command]
async fn ai_translate_article(
    article_id: i64,
    provider_id: String,
    target_language: String,
    request_id: String,
    app_handle: AppHandle,
) -> Result<String, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;

    // 获取 AI 提供商配置
    //
    // 与 ai_generate_summary 中完全相同的取值逻辑，保持两条链路行为一致：
    // 用户只需配置一次，摘要与翻译即可同时可用。
    // 空串同样按"未配置"处理：密钥的种子值现在是空串，而 `Setting::get` 对空串
    // 返回的是 `Some("")` 而非 `None`。不加这层过滤，"没填密钥"就会变成一次
    // 注定 401 的网络请求，用户看到的是服务端错误而不是"请先配置"。
    let api_url = Setting::get(&pool, &format!("ai_{}_url", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider URL not configured")?;
    let api_key = Setting::get(&pool, &format!("ai_{}_key", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider API key not configured")?;
    let model = Setting::get(&pool, &format!("ai_{}_model", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider model not configured")?;

    let provider = ai::AIProvider {
        api_url,
        api_key,
        model,
    };

    // 与摘要同构：kind 换成 translate，前端据此把增量渲染到正文而非摘要卡
    let sink = delta_sink(app_handle, request_id, "translate", article_id);
    ai::translate_article(&pool, article_id, &provider, &target_language, &sink).await
}

/// 每日 AI 简报
///
/// 聚合指定时间窗口内（`days` 天）未读文章，按标题+摘要拼成 prompt 交给大模型生成要点概览，
/// 结果写回 `settings.ai_daily_brief_last`（避免短时间内重复生成）。
/// 调用方可选择立即返回文本（同步模式）或后台异步生成（返回任务 ID，前端轮询结果）。
///
/// # 参数
/// * `days` - 回溯天数（默认 1）
///
/// # 返回值
/// 生成的简报文本（同步模式），失败时返回错误信息
#[tauri::command]
async fn ai_daily_brief(days: i64) -> Result<String, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;

    // 读取 AI 配置（复用与 ai_generate_summary 相同的路径）
    // 与上面两个 AI 命令同一约定：空串视为未配置（密钥种子值为空串）
    let api_url = Setting::get(&pool, "ai_agnes_url")
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI 提供商 URL 未配置")?;
    let api_key = Setting::get(&pool, "ai_agnes_key")
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI 提供商 API Key 未配置")?;
    let model = Setting::get(&pool, "ai_agnes_model")
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI 提供商 Model 未配置")?;

    let provider = ai::AIProvider {
        api_url,
        api_key,
        model,
    };

    // 取最近 N 天的未读文章（最多 30 篇，避免 prompt 过长）
    let cutoff = Utc::now()
        - chrono::Duration::days(days);
    let articles = sqlx::query_as::<_, Article>(
        "SELECT * FROM articles WHERE is_read = 0 AND published_at >= ? \
         ORDER BY published_at DESC LIMIT 30",
    )
    .bind(cutoff)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    if articles.is_empty() {
        return Ok("暂无未读文章，无需简报。".to_string());
    }

    // 拼 prompt：把每篇的标题 + 摘要组装成要点列表，让模型给出结构化概览
    let mut list = Vec::new();
    for a in &articles {
        let snippet = a.summary.chars().take(200).collect::<String>();
        list.push(format!("- 【{}】{}", a.title, snippet));
    }
    let prompt = format!(
        "你是一名智能资讯助手。请对以下最近 {} 天的未读 RSS 文章进行结构化摘要，\
         按主题分组输出要点，每条 1-2 句话。不要复述原文，给出结论与洞察。\n\n{}",
        days,
        list.join("\n")
    );

    // 复用 ai 模块的 `/chat/completions` 接口生成
    ai::generate_chat(&pool, &provider, &prompt).await
}

// ─── 文章对话追问 ──────────────────────────────────────────────────────────────

/// 文章追问时送入模型的正文上限（字符数）
///
/// 取 6000 字符：中文大致折合数千 token，加上实体 / 事件与多轮历史后仍稳定落在
/// 主流模型的上下文窗口内。RSS 正文动辄数万字，不设上限会让单次追问直接超窗失败。
const ARTICLE_CHAT_MAX_CHARS: usize = 6000;

/// 文章追问最多回带的历史消息条数
///
/// 只回带最近 20 条（10 轮问答）：多轮上下文能让追问更连贯，但全量回带会让 prompt
/// 随轮次线性膨胀，既贵也更容易触发超窗。更早的内容在 `system` 的文章材料里本就有据
/// 可查，丢弃最早的几轮没有实质损失。
const ARTICLE_CHAT_MAX_HISTORY: usize = 20;

/// 单条对话消息（前端 → 后端）
///
/// 与 `ai::Message` 分开定义：后者同时承担"请求体序列化"与"响应反序列化"两个角色
/// 且字段全 `pub`；这里只作为命令的入参契约，独立结构体能把字段校验与命名转换收在
/// IPC 边界上，前端字段名调整也不会波及 AI 模块的内部结构。
#[derive(serde::Deserialize)]
struct ChatTurn {
    /// 角色：`user`（用户提问）或 `assistant`（模型回答）
    role: String,
    /// 消息正文
    content: String,
}

/// 就当前文章与模型进行多轮对话追问
///
/// 把"文章正文 + 该文章命中的关注实体 + 已提取的研究事件"组装成一条 `system` 消息，
/// 其后接前端回带的历史轮次，一并交给 [`ai::chat_with_messages`]。
///
/// # 为什么不写库
/// 摘要 / 翻译是**可复用产物**（生成一次就应长期复用），故落库缓存；追问的会话是
/// **私有草稿**，是否留痕应由用户决定——前端持有会话、按需导出 Markdown。
/// 因此本命令对数据库是只读的，没有任何副作用。
///
/// # 参数
/// * `article_id` - 文章 ID
/// * `history` - 历史轮次（含本轮提问，末条应为 `user`）；仅取最近
///   [`ARTICLE_CHAT_MAX_HISTORY`] 条
/// * `provider_id` - AI 提供商标识（拼键规则同 [`ai_generate_summary`]）
/// * `request_id` - 前端生成的请求标识，随每个流式事件回传（`kind` 为 `chat`）
/// * `app_handle` - 应用句柄，用于发射流式事件
///
/// # 返回值
/// 模型生成的回答文本
///
/// # 错误
/// - 文章不存在：`Article not found`
/// - AI 配置缺失：`AI provider URL/API key/model not configured`
/// - 网络失败 / HTTP 非 2xx / 响应结构异常 / 空结果（由 [`ai::chat_with_messages`] 透传）
#[tauri::command]
async fn ai_article_chat(
    article_id: i64,
    history: Vec<ChatTurn>,
    provider_id: String,
    request_id: String,
    app_handle: AppHandle,
) -> Result<String, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let provider = ai::load_provider(&pool, &provider_id).await?;

    // 取文章；取不到时保持与 ai_generate_summary 相同的错误文案
    let article = sqlx::query_as::<_, Article>("SELECT * FROM articles WHERE id = ? LIMIT 1")
        .bind(article_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Article not found")?;

    // 正文是入库时经 ammonia 消毒的 HTML，带标签直接送模型会白耗大量 token
    let plain = ai::html_to_text(&article.content);
    let (text, truncated) = ai::truncate_chars(&plain, ARTICLE_CHAT_MAX_CHARS);

    // 命中实体：判定口径与 `batch_score_entities` 完全一致（实体名在「标题 + 摘要」
    // 中做子串匹配）。保持同口径很重要——否则界面上的"重要度"与追问时的"上下文"
    // 会各说各话，用户看到的实体列表和模型看到的对不上
    let entities = Entity::list_enabled(&pool).await?;
    let haystack = format!("{} {}", article.title, article.summary);
    let hit_entities: Vec<String> = entities
        .iter()
        .filter(|e| !e.name.trim().is_empty() && haystack.contains(&e.name))
        .map(|e| e.name.clone())
        .collect();

    // 研究事件：只取该文章的，并把 entity_id 映射回实体名（一次性建表，避免逐条回查）
    let events = ResearchEvent::list_by_article(&pool, article_id).await?;
    let name_by_id: std::collections::HashMap<i64, String> = Entity::list_all(&pool)
        .await?
        .into_iter()
        .map(|e| (e.id, e.name))
        .collect();
    let event_lines: Vec<String> = events
        .iter()
        .map(|ev| {
            let entity = name_by_id
                .get(&ev.entity_id)
                .map(|s| s.as_str())
                .unwrap_or("未知实体");
            // 日期可能为空（抽取时原文未给）：此时省略括号，而不是留一对空括号
            if ev.event_date.trim().is_empty() {
                format!("[{}] {}：{}", ev.event_type, entity, ev.fact)
            } else {
                format!(
                    "[{}] {}：{}（{}）",
                    ev.event_type, entity, ev.fact, ev.event_date
                )
            }
        })
        .collect();

    let prompt = ai::build_article_chat_prompt(&ai::ArticleChatContext {
        title: &article.title,
        text: &text,
        truncated,
        entities: &hit_entities,
        events: &event_lines,
    });

    // 组装完整消息数组：system 承载全部材料，其后是回带的历史（末条即本轮提问）。
    // 只保留最近 N 条，防止 prompt 随对话轮次无限膨胀
    let mut messages = Vec::with_capacity(history.len().min(ARTICLE_CHAT_MAX_HISTORY) + 1);
    messages.push(ai::Message {
        role: "system".to_string(),
        content: prompt,
    });
    for turn in &history[history.len().saturating_sub(ARTICLE_CHAT_MAX_HISTORY)..] {
        messages.push(ai::Message {
            role: turn.role.clone(),
            content: turn.content.clone(),
        });
    }

    // 温度 0.3：追问要"贴着材料说"，比创作更确定，但保留一点自然措辞的余地。
    // request_id 同时交给 delta_sink（事件过滤）与 chat_with_messages（取消检查），
    // 前者按值收编进闭包，后者只需借用，故先 clone 一份
    let sink = delta_sink(app_handle, request_id.clone(), "chat", article_id);
    ai::chat_with_messages(&provider, &messages, Some(0.3), &sink, Some(&request_id)).await
}

/// 取消一次进行中的 AI 流式任务
///
/// 在 [`ai::CANCELLED`] 登记取消标记，SSE 循环逐块检查到即中断请求并返回
/// [`ai::CANCELLED_MSG`] 错误——服务端随即停止生成，真正省下未完成的 token。
/// 目前仅「文章追问」（[`ai_article_chat`]）接入了循环检查；对单轮链路
/// （摘要 / 翻译 / 简报 / 提取）登记标记无副作用，但也不会生效。
///
/// # 参数
/// * `request_id` - 要取消的请求 ID（发起追问时由前端 `newRequestId()` 生成）
#[tauri::command]
fn ai_cancel(request_id: String) {
    ai::cancel_request(&request_id);
}

/// 统计每日已读（用于阅读洞察）
///
/// 返回最近 N 天（默认 7 天）每天的已读数，供前端渲染折线图。
///
/// # 参数
/// * `days` - 天数（默认 7）
///
/// # 返回值
/// 每日已读数组，每项包含日期与数量
#[tauri::command]
async fn stats_daily_read(days: i64) -> Result<Vec<(String, i64)>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let start = Utc::now() - chrono::Duration::days(days - 1);
    let rows = sqlx::query(
        "SELECT DATE(created_at) as day, COUNT(*) as cnt \
         FROM articles WHERE is_read = 1 AND created_at >= ? \
         GROUP BY day ORDER BY day ASC",
    )
    .bind(start)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|r| (r.get::<String, _>("day"), r.get::<i64, _>("cnt")))
        .collect())
}

// ─── 实体管理命令 ──────────────────────────────────────────────────────────────

/// 实体关系图的默认边数上限
///
/// 图的边数随"关注实体数"呈平方级增长，若不设上限，实体较多的库里一次查询
/// 就可能返回上千条边，前端 SVG 渲染与力导向迭代都会明显卡顿。取 300 是
/// "看得出全局结构"与"渲染不卡"之间的折中：当前全库实测仅 113 条边，
/// 远未触及上限，故该值不会悄悄截断用户的真实数据。
const DEFAULT_MAX_GRAPH_EDGES: i64 = 300;

/// 获取全部已启用实体列表
///
/// # 返回值
/// 实体列表，按创建时间升序
#[tauri::command]
async fn entities_list() -> Result<Vec<db::Entity>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::Entity::list_enabled(&pool).await
}

/// 新增实体
///
/// # 参数
/// * `name` - 实体名称（如 "腾讯"、"GOOG"）
/// * `entity_type` - 实体类型（company/person/product/industry/other）
///
/// # 返回值
/// 新增实体的 ID
#[tauri::command]
async fn entities_create(name: String, entity_type: String) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::Entity::create(&pool, &name, &entity_type).await
}

/// 删除实体
///
/// # 参数
/// * `id` - 实体 ID
#[tauri::command]
async fn entities_delete(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::Entity::delete(&pool, id).await
}

/// 切换实体启用状态
///
/// # 参数
/// * `id` - 实体 ID
///
/// # 返回值
/// 切换后的启用状态（true = 已启用）
#[tauri::command]
async fn entities_toggle(id: i64) -> Result<bool, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::Entity::toggle_enabled(&pool, id).await
}

/// 构建实体共现图（研究工作台「关系图」页签）
///
/// 见 [`db::Entity::graph`]：两个实体同时出现在同一篇文章的标题或摘要中即连边，
/// 权重为共现文章数。判定口径与 [`score_articles`]、智能摘要分组完全一致。
/// 纯内存计算、零 AI 依赖、不读写任何新表。
///
/// # 参数
/// * `days` - 时间窗（只看最近多少天的文章）；`null` 或 `<= 0` 表示不限时间
/// * `min_weight` - 最小共现数；`null` 取 1（保留全部共现关系）
/// * `max_edges` - 边数上限；`null` 取 300。超出时按权重降序截断，
///   返回体的 `truncated` 会置位，界面据此提示"仅显示最重要的 N 条关系"
///
/// # 返回值
/// [`db::EntityGraph`]：节点（含命中文章数）+ 边（按权重降序）+ 参与扫描的文章数
///
/// # 错误
/// 实体查询或文章查询失败时返回错误信息
#[tauri::command]
async fn entity_graph(
    days: Option<i64>,
    min_weight: Option<i64>,
    max_edges: Option<i64>,
) -> Result<db::EntityGraph, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    // 默认上限 300：即便库很大也不至于让前端一次渲染上千条线；
    // 置 0 表示显式不限制（前端当前不暴露该分支，仅供调试与测试使用）
    let limit = max_edges.unwrap_or(DEFAULT_MAX_GRAPH_EDGES);
    db::Entity::graph(&pool, days.unwrap_or(0), min_weight.unwrap_or(1), limit).await
}

// ─── 智能评分命令 ──────────────────────────────────────────────────────────────

/// 批量计算文章实体匹配评分
///
/// 遍历所有已启用实体，统计每篇文章标题+摘要中的命中数，
/// 映射到 0-100 分后写入 `entity_score`。仅更新尚未评分的文章。
///
/// # 参数
/// * `article_ids` - 指定文章 ID 列表；为空时全库评分
///
/// # 返回值
/// 实际更新的文章条数
#[tauri::command]
async fn score_articles(article_ids: Option<Vec<i64>>) -> Result<u64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let ids = article_ids.unwrap_or_default();
    db::batch_score_entities(&pool, ids).await
}

/// 获取高分文章，按「关注实体」分组（智能摘要弹窗的数据源）
///
/// 分组在 Rust 侧完成：命中的判定规则与评分完全一致（实体名出现在标题或摘要中），
/// 前端拿到的就是可直接渲染的分组列表，无需再猜"这篇文章命中了谁"。
///
/// # 参数
/// * `days` - 回溯天数（默认 7）
/// * `threshold` - 最低评分阈值（默认 60）
///
/// # 返回值
/// 按文章数倒序排列的实体分组；没有已启用实体时返回空集合（前端显示引导语）
///
/// # 错误
/// 数据库查询失败时返回错误信息
#[tauri::command]
async fn high_score_articles(
    days: i64,
    threshold: i64,
) -> Result<Vec<db::EntityArticleGroup>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    db::get_high_score_articles_grouped(&pool, days, threshold).await
}

// ─── 窗口控制命令 ─────────────────────────────────────────────────────────────
/// 最小化当前窗口
///
/// # 参数
/// * `_app` - Tauri 应用句柄（保留参数，便于后续扩展）
#[tauri::command]
async fn win_minimize(_app: tauri::AppHandle) -> Result<(), String> {
    let window = _app.get_webview_window("main").ok_or("主窗口未找到")?;
    window.minimize().map_err(|e| e.to_string())
}

/// 最大化 / 还原窗口切换
///
/// 查询当前窗口最大化状态，若已最大化则还原，否则最大化。
#[tauri::command]
async fn win_toggle_maximize(_app: tauri::AppHandle) -> Result<bool, String> {
    let window = _app.get_webview_window("main").ok_or("主窗口未找到")?;
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        window.maximize().map_err(|e| e.to_string())?;
        Ok(true)
    }
}

/// 关闭当前窗口（退出应用）
#[tauri::command]
async fn win_close(_app: tauri::AppHandle) -> Result<(), String> {
    let window = _app.get_webview_window("main").ok_or("主窗口未找到")?;
    window.close().map_err(|e| e.to_string())
}

// ─── 全局数据库连接池 ──────────────────────────────────────────────────────────
/// 获取全局数据库连接池
///
/// 在初始化完成后返回连接池；如果尚未就绪（如首次启动期间有命令抢先到达），
/// 则返回一个友好的错误提示，让前端稍后重试即可。
///
/// # 返回值
/// * `Ok(pool)` - 可用的连接池
/// * `Err(msg)` - 连接池尚未就绪
async fn db_pool() -> Result<sqlx::SqlitePool, String> {
    let guard = DB_POOL.lock().await;
    guard
        .clone()
        .ok_or_else(|| "数据库尚未初始化，请稍后重试".to_string())
}

// ─── 研究事件提取命令 ─────────────────────────────────────────────────────────

/// 对单篇文章执行研究信息提取（阅读场景手动触发）
///
/// 提取 → 归一化 → 入库一条龙，返回新增事件数。
/// 入库成功后写 `ai_extracted_at`，与批量任务同一口径：否则这篇文章在下一轮
/// 批量提取里会被当成"没处理过"再问一次模型（白花一次 token）。
///
/// # 参数
/// * `article_id` - 文章 ID
/// * `provider_id` - AI 提供商标识；缺省用 agnes
///
/// # 返回值
/// 新增事件条数
///
/// # 错误
/// 提供商配置缺失、网络请求失败或入库失败时返回错误信息
#[tauri::command]
async fn research_extract_article(
    article_id: i64,
    provider_id: Option<String>,
) -> Result<u64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let provider_id = provider_id.unwrap_or_else(|| "agnes".to_string());
    let provider = ai::load_provider(&pool, &provider_id).await?;
    let events = research::extract::extract_article(&pool, article_id, &provider).await?;
    let inserted = research::normalize::normalize_and_store_events(
        &pool,
        article_id,
        &events,
        &provider.model,
    )
    .await?;
    // 置位失败不影响本次结果（事件已经入库），只提示一下
    if let Err(e) = research::extract::mark_extracted(&pool, article_id).await {
        eprintln!("标记文章 {} 提取时间失败: {}", article_id, e);
    }
    Ok(inserted)
}

/// 手动补跑一次批量提取（程序未全天运行时，用户可在任意时间触发）
///
/// 复用"智能提取"任务与 cron 完全相同的执行路径
/// （[`research::run_task_once`] → [`research::run_batch_task`]），
/// 参数来自任务 config，运行结果同样回写任务行——手动补跑等价于"提前跑一次 cron"。
///
/// # 返回值
/// 结果描述（处理篇数与新增事件数）
///
/// # 错误
/// 找不到批量提取任务、提供商未配置或提取失败时返回错误信息
#[tauri::command]
async fn research_extract_batch() -> Result<String, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let task = AutomationTask::list_all(&pool)
        .await?
        .into_iter()
        .find(|t| t.task_type == "batch_extract")
        .ok_or("未找到批量提取任务，请先在设置中创建")?;
    research::run_task_once(&pool, &task).await
}

/// 查询某篇文章已抽取的事件
///
/// # 参数
/// * `article_id` - 文章 ID
///
/// # 返回值
/// 该文章的事件列表
#[tauri::command]
async fn research_events_by_article(article_id: i64) -> Result<Vec<ResearchEvent>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    ResearchEvent::list_by_article(&pool, article_id).await
}

/// 查询带上下文的事件时间线（JOIN 实体名与文章标题，供时间线面板展示）
///
/// # 参数
/// * `days` - 时间窗口天数（0 表示全部）
/// * `entity_id` - 实体筛选（None 表示全部实体）
///
/// # 返回值
/// 带实体名与来源文章信息的事件列表
///
/// # 错误
/// 数据库查询失败时返回错误信息
#[tauri::command]
async fn research_events_timeline(
    days: i64,
    entity_id: Option<i64>,
) -> Result<Vec<EventWithContext>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let mut rows = ResearchEvent::list_with_context(&pool, days).await?;
    // 实体筛选在应用层做：事件量级为百级，一次全量取回再过滤
    // 比为可选条件拼接两份 SQL 更简单且不影响索引使用
    if let Some(eid) = entity_id {
        rows.retain(|r| r.entity_id == eid);
    }
    Ok(rows)
}

/// 删除单条研究事件
///
/// 物理删除；事件不存在时静默成功（幂等），便于前端在删除后直接移出本地列表
/// 而无需处理"已被别人删掉"的分支。
///
/// # 参数
/// * `event_id` - 事件 ID
///
/// # 错误
/// 数据库删除失败时返回错误信息
#[tauri::command]
async fn research_event_delete(event_id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    ResearchEvent::delete(&pool, event_id).await?;
    Ok(())
}

// ─── 自动化任务命令 ───────────────────────────────────────────────────────────

/// 获取全部自动化任务
///
/// # 返回值
/// 全部任务列表（含最近运行状态）
#[tauri::command]
async fn automation_tasks_list() -> Result<Vec<AutomationTask>, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    AutomationTask::list_all(&pool).await
}

/// 新建自动化任务
///
/// # 参数
/// * `name` - 任务名称
/// * `task_type` - 任务类型（batch_extract / daily_report / weekly_report）
/// * `cron_expr` - cron 表达式
/// * `config` - JSON 配置
/// * `enabled` - 是否启用
///
/// # 返回值
/// 新任务 ID
#[tauri::command]
async fn automation_tasks_create(
    name: String,
    task_type: String,
    cron_expr: String,
    config: String,
    enabled: bool,
) -> Result<i64, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let id = AutomationTask::create(&pool, &name, &task_type, &cron_expr, &config, enabled).await?;
    // 写库成功后立即重注册调度，让新任务在本运行期内就生效
    trigger_automation_reload().await;
    Ok(id)
}

/// 更新自动化任务的可编辑字段
///
/// # 参数
/// * `id` - 任务 ID
/// * `name` / `cron_expr` / `config` - 可选的新值（None 表示保持不变）
#[tauri::command]
async fn automation_tasks_update(
    id: i64,
    name: Option<String>,
    cron_expr: Option<String>,
    config: Option<String>,
) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    AutomationTask::update(
        &pool,
        id,
        name.as_deref(),
        cron_expr.as_deref(),
        config.as_deref(),
    )
    .await?;
    trigger_automation_reload().await;
    Ok(())
}

/// 删除自动化任务
///
/// # 参数
/// * `id` - 任务 ID
#[tauri::command]
async fn automation_tasks_delete(id: i64) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    AutomationTask::delete(&pool, id).await?;
    trigger_automation_reload().await;
    Ok(())
}

/// 启用 / 停用自动化任务
///
/// # 参数
/// * `id` - 任务 ID
/// * `enabled` - 目标状态
#[tauri::command]
async fn automation_tasks_set_enabled(id: i64, enabled: bool) -> Result<(), String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    AutomationTask::set_enabled(&pool, id, enabled).await?;
    trigger_automation_reload().await;
    Ok(())
}

/// 立即运行一个自动化任务（手动触发入口）
///
/// 与 cron 触发走同一执行器 [`research::run_task_once`]，运行结果同样回写任务行；
/// 通过共享的 [`research::RUNNING_TASKS`] 防重入——同一任务并发触发第二次直接报错返回。
///
/// # 参数
/// * `task_id` - 任务 ID
///
/// # 返回值
/// 执行结果描述
///
/// # 错误
/// 任务不存在、已在运行中或执行失败时返回错误信息
#[tauri::command]
async fn automation_run_now(task_id: i64) -> Result<String, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;
    let task = AutomationTask::find_by_id(&pool, task_id)
        .await?
        .ok_or_else(|| format!("任务不存在: {}", task_id))?;

    // 防重入：先占位再执行；insert 返回 false 说明该任务正在运行
    {
        let mut running = research::RUNNING_TASKS.lock().await;
        if !running.insert(task_id) {
            return Err("该任务正在运行中，请等待完成".to_string());
        }
    }
    // 无论成败都释放占位（defer 风格：用变量记住执行结果，解锁后再回写状态）
    let result = research::run_task_once(&pool, &task).await;
    {
        let mut running = research::RUNNING_TASKS.lock().await;
        running.remove(&task_id);
    }
    result
}

/// 触发自动化任务的重注册（任务写库成功后调用）
///
/// 管理器尚未初始化（应用启动早期）时静默跳过：下次启动会按表重新注册，
/// 行为一致。reload 失败只记日志不返回错误——调度异常不应掩盖写库成功的事实。
async fn trigger_automation_reload() {
    let guard = AUTOMATION_MANAGER.lock().await;
    if let Some(manager) = guard.as_ref() {
        if let Err(e) = manager.reload().await {
            eprintln!("自动化任务重注册失败: {}", e);
        }
    }
}



// ─── 报告生成 / 读取命令（应用内查看器） ──────────────────────────────────────

/// 手动生成报告的结果
///
/// `file_name` 仅在报告写入应用数据目录（app 通道）时有值，供前端生成后
/// 直接打开；Obsidian 通道的报告落在用户 Vault 内，应用查看器读不到，取 `None`。
#[derive(serde::Serialize)]
struct ReportGenResult {
    /// 应用内可见的报告文件名（Obsidian 通道为 None）
    file_name: Option<String>,
    /// 结果描述（含落盘位置与事件统计）
    message: String,
}

/// 手动生成一份研究报告（不需要任何自动化任务）
///
/// 与自动化任务共用 [`research::report::generate_report`] 核心实现，区别只在
/// 配置来源（前端参数而非任务行）且不回写任务运行状态——因此"程序不常驻"
/// 的场景不必为了出一份报告而先去建任务。
///
/// 文件名按类型与日期生成，同一时间窗口重复生成会**覆盖**同名文件
/// （日报按日期、周报按 ISO 周号），同一天只保留最新一份。
///
/// # 参数
/// * `kind` - 报告类型：`daily` / `weekly`
/// * `days` - 覆盖天数（1..=90）
/// * `channel` - 输出通道：`app`（应用内目录）/ `obsidian`（仅 Vault）/
///   `both`（应用内 + Vault 双写）
///
/// # 返回值
/// 生成结果（文件名 + 描述）
///
/// # 错误
/// 参数非法、Obsidian Vault 未配置或生成失败时返回错误信息
#[tauri::command]
async fn research_report_generate(
    kind: String,
    days: i64,
    channel: String,
) -> Result<ReportGenResult, String> {
    let pool = db_pool().await.map_err(|e| e.to_string())?;

    // 参数校验前置于生成：非法值直接拒绝，避免产出一份语义不明的报告
    let report_kind = research::report::ReportKind::parse(&kind)
        .ok_or_else(|| format!("未知报告类型: {}（支持 daily / weekly）", kind))?;
    if !(1..=90).contains(&days) {
        return Err(format!("覆盖天数须在 1-90 之间，当前为 {}", days));
    }
    let channel = channel.trim().to_ascii_lowercase();
    if !matches!(channel.as_str(), "app" | "obsidian" | "both") {
        return Err(format!(
            "未知输出通道: {}（支持 app / obsidian / both）",
            channel
        ));
    }
    // Obsidian 通道的报告只落在 Vault 里，应用内查看器读不到该文件；
    // app 与 both 都有应用内那一份，因此都回传文件名供查看器打开
    let in_app_dir = channel != "obsidian";

    let config = serde_json::json!({
        "channel": channel.as_str(),
        "days": days,
    })
    .to_string();
    let (file_name, message) =
        research::report::generate_report(&pool, &config, report_kind).await?;

    Ok(ReportGenResult {
        file_name: in_app_dir.then_some(file_name),
        message,
    })
}

/// 应用内可见的单份报告元信息
///
/// 只含列表展示所需字段，内容经 [`research_report_read`] 按需读取，
/// 避免"列表一次拉全部正文"的无谓开销。
#[derive(serde::Serialize)]
struct ReportMeta {
    /// 文件名（不含路径，读取时回传作定位键）
    name: String,
    /// 文件大小（字节）
    size: u64,
    /// 最后修改时间（本地时区，展示用）
    modified: String,
}

/// 列出应用数据目录 reports/ 下的全部报告文件
///
/// 只认 `.md` 文件并按修改时间倒序（最新在前）；目录不存在视为"暂无报告"
/// 而非错误——首次启动尚未生成任何报告是正常状态。
///
/// # 返回值
/// 报告元信息列表（倒序）
///
/// # 错误
/// 目录读取失败（非不存在）时返回错误信息
#[tauri::command]
async fn research_reports_list() -> Result<Vec<ReportMeta>, String> {
    // 目录不存在 = 还没有报告，返回空列表而不是报错
    let dir = match research::report::reports_dir() {
        Some(d) => d,
        None => return Ok(Vec::new()),
    };
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut metas: Vec<ReportMeta> = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("读取报告目录失败: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("遍历报告目录失败: {}", e))?;
        let path = entry.path();
        // 只收录 .md 文件，跳过子目录与临时文件
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let meta = entry
            .metadata()
            .map_err(|e| format!("读取报告元信息失败: {}", e))?;
        let modified = meta
            .modified()
            .ok()
            .map(|t| {
                chrono::DateTime::<chrono::Local>::from(t)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        metas.push(ReportMeta {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            size: meta.len(),
            modified,
        });
    }
    // 修改时间倒序：最新生成的报告排在最前
    metas.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(metas)
}

/// 读取指定报告的 Markdown 内容
///
/// # 参数
/// * `name` - 报告文件名（来自 [`research_reports_list`]，不含路径）
///
/// # 返回值
/// 报告全文（UTF-8 Markdown）
///
/// # 错误
/// 文件名含路径分隔符（防目录穿越）、报告目录未初始化或读取失败时返回错误信息
#[tauri::command]
async fn research_report_read(name: String) -> Result<String, String> {
    // 安全校验：拒绝任何路径分隔符，确保只能读 reports/ 下的直接子文件
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("非法的报告文件名".to_string());
    }
    let dir = research::report::reports_dir().ok_or("报告目录未初始化，请重启应用")?;
    let path = dir.join(&name);
    std::fs::read_to_string(&path).map_err(|e| format!("读取报告失败: {}", e))
}
