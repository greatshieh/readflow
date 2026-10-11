//! 后台定时刷新管理器
//!
//! # 职责
//! 在应用运行期间自动拉取订阅源，让用户"打开就有新内容"：
//! - 基于 `tokio-cron-scheduler` 注册周期性任务（[`RefreshManager::start`]）
//! - 提供手动触发入口，供前端"立即刷新"按钮调用（[`RefreshManager::manual_refresh`]）
//! - 统一执行一轮全量刷新并回写未读数（[`refresh_all_feeds`]）
//! - 每日凌晨 3 点按保留策略自动清理旧文章（调用 [`crate::db::run_retention_cleanup`]）
//!
//! # 设计意图
//! - **失败隔离**：单个源的抓取或入库失败只打印日志，不影响同一轮中其它源，
//!   避免一个失效 Feed 让整次刷新中断。
//! - **进度实时反馈**：每处理完一个源就立即重算该源 `unread_count` 并向前端广播
//!   `refresh-progress` 事件（新文章数 / 未读数 / 进度 n/m），侧边栏数字随之实时变化；
//!   循环末尾仍保留一次全量重算作为兜底，保证最终一致性。
//! - **共享连接池**：`SqlitePool` 本身是廉价的克隆句柄（内部 Arc），
//!   因此调度任务里直接 `clone()` 后 `move` 进异步闭包即可安全跨任务使用。
//! - **刷新互斥**：全量刷新同一时刻只允许一轮（[`REFRESH_RUNNING`]），
//!   定时任务撞上手动刷新时静默跳过，避免同一批源被并发抓取两遍。
//! - **无阻塞启动**：`start()` 只负责注册与启动调度器，不等待首轮刷新，
//!   避免拖慢应用启动。

use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::db::{Entity, Feed, Setting, run_retention_cleanup};
use crate::rss::{fetch_feed, store_items};

/// 全量刷新正在进行中时返回的错误文案
///
/// 单独提为常量是为了让调用方能识别这一种"非故障"的失败：定时任务撞上手动刷新时
/// 应当静默跳过（用户正在看着进度条），而不是打印一条"定时刷新失败"。
pub(crate) const REFRESH_BUSY: &str = "已有刷新任务在进行中，请稍候";

/// 全量刷新的重入标志
///
/// 定时任务与手动刷新共用同一段逻辑，两个触发源完全可能重叠（用户点"立即刷新"时
/// 恰好定时任务到点）。并发跑同一批源会让同一个 URL 被重复请求、进度条在中端来回
/// 跳变。用一个进程级标志位做互斥，而不是给 `RefreshManager` 加字段——手动刷新的
/// 入口可能来自不同的 manager 实例（见 `lib.rs` 的降级路径）。
static REFRESH_RUNNING: AtomicBool = AtomicBool::new(false);

/// 刷新权的占位凭证，析构时自动释放标志位
///
/// 用 RAII 而不是"函数末尾手工复位"：`refresh_all_feeds` 内部有多处 `?` 提前返回，
/// 手工复位必然漏掉某条路径，一次异常就把刷新永久锁死。
struct RefreshGuard;

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESH_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// 尝试取得刷新权
///
/// # 返回值
/// 取到返回守卫（丢弃即释放）；已有刷新在跑时返回 `None`
fn try_acquire_refresh() -> Option<RefreshGuard> {
    REFRESH_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
        .then_some(RefreshGuard)
}

/// 单个订阅源刷新完成的进度事件载荷（事件名 `refresh-progress`）
///
/// 每处理完一个订阅源就向前端广播一次，让侧边栏的未读数与"n/m"进度指示
/// 在刷新过程中实时更新，而不是等整轮结束后一次性跳变。
/// 字段名用 camelCase 序列化，与前端 TypeScript 消费习惯一致。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProgress {
    /// 刚完成刷新的订阅源 ID
    pub feed_id: i64,
    /// 订阅源名称（前端提示用）
    pub feed_name: String,
    /// 本次刷新该源新增的文章数
    pub new_count: i64,
    /// 重算后该源的未读文章数
    pub unread_count: i64,
    /// 已完成的源序号（从 1 起）
    pub done: usize,
    /// 本轮待刷新的源总数
    pub total: usize,
}

/// 后台刷新管理器
///
/// 持有数据库连接池，是定时刷新与手动刷新共用的入口。
/// 目前是无状态句柄：所有刷新逻辑都在 [`refresh_all_feeds`] 中，
/// 因此多次创建本结构体互不影响。
pub struct RefreshManager {
    /// 数据库连接池
    pool: SqlitePool,
    /// 应用句柄（用于向前端广播逐源刷新进度事件；
    /// 定时任务回调拿不到命令层的 AppHandle，必须在创建时持有）
    app_handle: AppHandle,
}

impl RefreshManager {
    /// 创建新的刷新管理器
    ///
    /// 当前没有需要异步准备的资源，签名保留 `async` 以便未来加入
    /// "启动时恢复调度状态"之类的初始化而不破坏调用方。
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    /// * `app_handle` - 应用句柄（广播 `refresh-progress` 事件用）
    ///
    /// # 返回值
    /// 初始化完成的刷新管理器
    pub async fn new(pool: SqlitePool, app_handle: AppHandle) -> Result<Self, String> {
        Ok(RefreshManager { pool, app_handle })
    }

    /// 启动后台定时刷新任务
    ///
    /// 注册一个 cron 任务，按表达式周期性触发 [`refresh_all_feeds`]。
    /// 任务内部自行捕获错误并打印日志——调度器无法接收任务返回值，
    /// 把错误抛出只会被静默吞掉。
    ///
    /// 默认每小时刷新一次所有订阅源
    ///
    /// # 返回值
    /// 调度器创建、任务注册与启动全部成功时返回 `Ok(())`
    ///
    /// # 错误
    /// 调度器初始化失败、cron 表达式非法或任务注册失败时返回错误信息
    pub async fn start(&self) -> Result<(), String> {
        let scheduler = JobScheduler::new().await.map_err(|e| e.to_string())?;
        // 克隆连接池句柄与应用句柄交给任务闭包：闭包需要 'static，不能直接借用 self
        let pool = self.pool.clone();
        let app_handle = self.app_handle.clone();

        // 读取用户配置的刷新频率（分钟）；缺失或解析失败回退到默认 60 分钟。
        // 该设置由"设置 → 常规"写入 settings 表，seed 阶段已写入默认值。
        let interval_min: i64 = Setting::get(&self.pool, "refresh_interval_minutes")
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|&m| m >= 1)
            .unwrap_or(60);

        // 由分钟数推导 cron 表达式（cron 字段顺序：秒 分 时 日 月 周）：
        // - 整除 60 且 >= 60 → 按"每 N 小时"触发（如 360 分钟 → 每 6 小时）；
        // - 否则 → 按"每 N 分钟"触发（如 30 分钟 → 每 30 分钟）。
        let cron = if interval_min >= 60 && interval_min % 60 == 0 {
            format!("0 0 */{} * * ?", interval_min / 60)
        } else {
            format!("0 */{} * * * ?", interval_min)
        };
        eprintln!("后台定时刷新频率：每 {} 分钟", interval_min);

        let job = Job::new_async(cron.as_str(), move |_uuid, _l| {
            let pool = pool.clone();
            let app_handle = app_handle.clone();
            Box::pin(async move {
                if let Err(e) = refresh_all_feeds(&pool, Some(&app_handle)).await {
                    // 撞上用户手动刷新属正常避让，不算失败，也不该刷屏
                    if e != REFRESH_BUSY {
                        eprintln!("定时刷新失败: {}", e);
                    }
                }
            })
        })
        .map_err(|e| e.to_string())?;

        scheduler.add(job).await.map_err(|e| e.to_string())?;

        // 注册第二个定时任务：每日凌晨 3 点执行文章保留策略清理。
        // 复用 `run_retention_cleanup`，它会读取 settings 表里的
        // `article_retention_days` / `retention_keep_unread` 决定清理范围。
        // 收藏与（可选）未读文章始终被保护，避免误删有价值内容。
        let pool2 = self.pool.clone();
        let retention_job = Job::new_async("0 0 3 * * ?", move |_uuid, _l| {
            let pool2 = pool2.clone();
            Box::pin(async move {
                match run_retention_cleanup(&pool2).await {
                    Ok(0) => {} // 0 条表示未达清理条件（如永久保留），无需打印
                    Ok(n) => eprintln!("定时清理旧文章：已删除 {} 篇", n),
                    Err(e) => eprintln!("定时清理旧文章失败: {}", e),
                }
            })
        })
        .map_err(|e| e.to_string())?;
        scheduler.add(retention_job).await.map_err(|e| e.to_string())?;

        scheduler.start().await.map_err(|e| e.to_string())?;

        eprintln!("后台定时刷新已启动，cron = {}", cron);
        Ok(())
    }

    /// 手动触发一次全量刷新
    ///
    /// 与定时任务共用 [`refresh_all_feeds`]，因此行为完全一致：
    /// 逐个源抓取、落库、逐源广播进度事件并重算未读数。
    ///
    /// # 参数
    /// * `app` - 发起本次刷新的命令层应用句柄（透传给进度事件广播）
    ///
    /// # 返回值
    /// 本次刷新新增的文章数量
    ///
    /// # 错误
    /// 订阅源列表读取失败、抓取时间回写失败或未读数重算失败时返回错误信息
    pub async fn manual_refresh(&self, app: &AppHandle) -> Result<i64, String> {
        refresh_all_feeds(&self.pool, Some(app)).await
    }
}

/// 刷新所有订阅源并实时广播逐源进度
///
/// 一轮刷新的完整流程：
/// 1. 取出全部订阅源；
/// 2. 逐个抓取并入库，累计真实新增条数；
/// 3. 成功抓取后回写该源的 `last_fetch_at`；
/// 4. 每处理完一个源（无论成功失败）立即重算该源 `unread_count` 并广播
///    `refresh-progress` 事件，前端侧边栏随之实时更新；
/// 5. 循环结束后用子查询一次性重算所有源的 `unread_count` 兜底，
///    覆盖清理/并发写库等造成的偏差，保证最终一致性。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `app` - 应用句柄；`None` 时只刷库不发事件（预留：无窗口环境下调用）
///
/// # 返回值
/// 本轮新增的文章总数（单源失败只影响该源，不计入但不中断）
///
/// # 错误
/// - 已有一轮刷新在跑时返回 [`REFRESH_BUSY`]（非故障，见该常量说明）
/// - 源列表读取、`last_fetch_at` 回写或未读数重算的 SQL 失败时返回错误信息
pub(crate) async fn refresh_all_feeds(
    pool: &SqlitePool,
    app: Option<&AppHandle>,
) -> Result<i64, String> {
    // 占位失败即说明另一轮在跑，直接让位：并发抓同一批源只会浪费带宽并让进度条错乱
    let _guard = try_acquire_refresh().ok_or(REFRESH_BUSY)?;

    let feeds = Feed::list_all(pool).await?;
    let total = feeds.len();
    let mut total_new = 0i64;
    // 本轮所有新文章的（标题, 摘要），供刷新结束后做实体感知通知
    let mut new_texts: Vec<(String, String)> = Vec::new();
    // 已处理完的源计数（成功与失败都推进，让前端 n/m 能走满）
    let mut done = 0usize;

    for feed in feeds {
        done += 1;
        match fetch_feed(&feed.url).await {
            Ok(parsed) => {
                // 存储新文章；单个源入库失败只记录，继续处理后面的源
                let new_count = match store_items(pool, feed.id, &feed.name, &parsed.items).await {
                    Ok(count) => {
                        total_new += count;
                        count
                    }
                    Err(e) => {
                        eprintln!("存储 {} 的文章失败: {}", feed.name, e);
                        0
                    }
                };
                // 取回本次新文章的（标题, 摘要），供刷新结束后做"命中关注实体"的感知通知
                new_texts.extend(fetch_new_article_texts(pool, feed.id, new_count).await);

                // 图标同步：fetch_feed 已保证 icon 非空（channel 图标优先、favicon 兜底）；
                // 与库中现有值不同才写库，避免每轮刷新产生无谓的 UPDATE
                if let Some(icon) = &parsed.icon {
                    if feed.icon.as_deref().unwrap_or("") != icon {
                        if let Err(e) = Feed::update_icon(pool, feed.id, icon).await {
                            eprintln!("更新 {} 的图标失败: {}", feed.name, e);
                        }
                    }
                }

                // 更新最后抓取时间（用 SQLite 的 CURRENT_TIMESTAMP，避免 Rust 侧时区差异）
                sqlx::query("UPDATE feeds SET last_fetch_at = CURRENT_TIMESTAMP WHERE id = ?")
                    .bind(feed.id)
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;

                // 该源处理完毕：立即重算未读数并广播进度
                emit_feed_progress(pool, app, feed.id, &feed.name, new_count, done, total).await;
            }
            // 抓取失败（超时 / 解析失败）同样只记录，保证整轮刷新不被拖垮；
            // 仍广播一次进度（new_count=0），让前端 n/m 不至于停在中途
            Err(e) => {
                eprintln!("抓取 {} 失败: {}", feed.name, e);
                emit_feed_progress(pool, app, feed.id, &feed.name, 0, done, total).await;
            }
        }
    }

    // 兜底：更新所有订阅源的未读文章数（一次批量语句覆盖全表，
    // 纠正逐源重算之间可能发生的并发偏差，如清理任务同时删文）
    sqlx::query(
        "UPDATE feeds SET unread_count = (
            SELECT COUNT(*) FROM articles
            WHERE articles.feed_id = feeds.id AND articles.is_read = 0
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 有新内容才提醒：零新增也弹一条就是纯噪音。
    // 窗口是否聚焦、用户是否开着通知由 [`crate::notify::send`] 内部判断，
    // 这里只管"本轮有没有值得一说的事"。定时刷新、手动刷新、托盘刷新三条路径共用此处。
    if total_new > 0 {
        // 实体感知通知：命中关注实体的新文给出针对性提示，否则回退到通用计数提示。
        // 窗口是否聚焦、通知开关是否开启由 [`crate::notify::send`] 内部判定。
        let hits = new_article_entity_hits(pool, &new_texts).await;
        let body = if hits.is_empty() {
            format!("刷新完成，发现 {} 篇新文章", total_new)
        } else {
            format!(
                "发现 {} 篇新文，涉及你关注的：{}",
                total_new,
                hits.join("、")
            )
        };
        crate::notify::send(pool, "ReadFlow", &body).await;
    }

    Ok(total_new)
}

// ─── 新文实体感知通知辅助 ─────────────────────────────────────────────────────

/// 取回某订阅源本次新插入文章的（标题, 摘要）文本
///
/// `store_items` 只回传新增条数，而实体感知通知需要文章正文来做"是否命中关注实体"的
/// 子串匹配。文章 `id` 自增单调，且单个源的入库在同一次同步调用内完成、期间没有其它
/// 写入，因此按 `id DESC LIMIT new_count` 取回的恰好是本次新插入的那些。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `feed_id` - 订阅源 ID
/// * `new_count` - 本次新增条数（≤0 时直接返回空，避免无意义查询）
///
/// # 返回值
/// 新文章的 (标题, 摘要) 列表；查询失败返回空（通知属锦上添花，失败不应中断刷新）
async fn fetch_new_article_texts(
    pool: &SqlitePool,
    feed_id: i64,
    new_count: i64,
) -> Vec<(String, String)> {
    if new_count <= 0 {
        return Vec::new();
    }
    sqlx::query_as::<_, (String, String)>(
        "SELECT title, COALESCE(summary, '') FROM articles WHERE feed_id = ? ORDER BY id DESC LIMIT ?",
    )
    .bind(feed_id)
    .bind(new_count)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// 在新增文章文本中找出命中"关注实体"的实体展示名
///
/// 实现说明：取全部 `enabled = 1` 的关注实体，把每个实体的 `name` 与其 `aliases`
/// （JSON 数组字符串，如 `["字节","ByteDance"]`）解析为小写检索词（跳过单字减噪），
/// 再对每篇新文的"标题+摘要"做小写子串命中。命中的实体展示名去重后最多保留 3 个，
/// 用于拼装通知正文（"涉及你关注的：X、Y"）。
///
/// 这是首版启发式匹配：不依赖 AI、零额外请求，代价是可能误命中常见词（如人名"张伟"）。
/// 后续可升级为与 `entity_score` 评分联动（见 P1-F 实体级告警）。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `texts` - 新文章的 (标题, 摘要) 列表
///
/// # 返回值
/// 被命中的关注实体展示名（去重、最多 3 个）；无关注实体或无命中返回空
async fn new_article_entity_hits(
    pool: &SqlitePool,
    texts: &[(String, String)],
) -> Vec<String> {
    if texts.is_empty() {
        return Vec::new();
    }
    let entities = match Entity::list_enabled(pool).await {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    if entities.is_empty() {
        return Vec::new();
    }

    // 预构建检索词：实体名 + 别称，统一小写、跳过单字减噪；记录展示名以便回写命中
    let mut entity_terms: Vec<(String, Vec<String>)> = Vec::new();
    for e in &entities {
        let mut terms: Vec<String> = Vec::new();
        let name_lower = e.name.trim().to_lowercase();
        if name_lower.chars().count() >= 2 {
            terms.push(name_lower);
        }
        if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&e.aliases) {
            for a in aliases {
                let a_lower = a.trim().to_lowercase();
                if a_lower.chars().count() >= 2 {
                    terms.push(a_lower);
                }
            }
        }
        if !terms.is_empty() {
            entity_terms.push((e.name.clone(), terms));
        }
    }
    if entity_terms.is_empty() {
        return Vec::new();
    }

    let mut hits: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (title, summary) in texts {
        if hits.len() >= 3 {
            break;
        }
        let haystack = format!("{} {}", title, summary).to_lowercase();
        for (display, terms) in &entity_terms {
            if terms.iter().any(|t| haystack.contains(t)) {
                hits.insert(display.clone());
                if hits.len() >= 3 {
                    break;
                }
            }
        }
    }
    hits.into_iter().collect()
}

#[cfg(test)]
mod new_article_notify_tests {
    //! 新文实体感知匹配的逻辑验证
    //!
    //! 仅测纯逻辑（[`new_article_entity_hits`]）：命中/不命中/禁用实体不参与，均不依赖
    //! 真实通知守护。内存库复用正式 `create_tables`，`entities` 表结构与正式库一致。

    use super::*;
    use crate::db::{create_tables, Entity};

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("建内存库失败");
        create_tables(&pool).await.expect("建表失败");
        pool
    }

    /// 启用实体应被命中；其别称（JSON 数组）也应命中；禁用实体不参与匹配
    #[tokio::test]
    async fn hits_match_enabled_entities_and_aliases() {
        let pool = setup().await;
        // 两个启用实体
        Entity::create(&pool, "新元科技", "company").await.unwrap();
        Entity::create(&pool, "星河AI", "product").await.unwrap();
        // 给"星河AI"写入别称 ["星河","GalaxyAI"]
        sqlx::query("UPDATE entities SET aliases = ? WHERE name = ?")
            .bind(r#"["星河","GalaxyAI"]"#)
            .bind("星河AI")
            .execute(&pool)
            .await
            .unwrap();
        // 禁用实体：应完全不参与匹配
        let disabled = Entity::create(&pool, "旁观者", "person").await.unwrap();
        sqlx::query("UPDATE entities SET enabled = 0 WHERE id = ?")
            .bind(disabled)
            .execute(&pool)
            .await
            .unwrap();

        let texts = vec![
            ("新元科技发布三季度财报".to_string(), "营收增长".to_string()),
            ("星河AI 推出新模型".to_string(), "GalaxyAI 亮相".to_string()),
            ("无关标题".to_string(), "无关正文".to_string()),
        ];
        let hits = new_article_entity_hits(&pool, &texts).await;
        assert!(hits.iter().any(|h| h == "新元科技"), "应命中新元科技");
        assert!(hits.iter().any(|h| h == "星河AI"), "应命中星河AI（含别称）");
        assert!(
            !hits.iter().any(|h| h == "旁观者"),
            "禁用实体不应被命中"
        );
        assert!(hits.len() <= 3, "命中数不应超过 3");
    }

    /// 没有关注实体时，任何新文都不应产生命中
    #[tokio::test]
    async fn no_hits_when_no_enabled_entities() {
        let pool = setup().await;
        let texts = vec![("随便一篇".to_string(), "正文".to_string())];
        assert!(new_article_entity_hits(&pool, &texts).await.is_empty());
    }
}

/// 重算单个源的未读数、写回 feeds 表并向前端广播进度事件
///
/// 同时被 [`refresh_all_feeds`]（逐源循环）与 `feeds_refresh_one` 命令（单源刷新）
/// 复用，保证两条路径的未读数写回与事件载荷格式完全一致。
///
/// 失败容忍策略：SQL/事件发送失败只记日志不向上传播——
/// 进度广播属于锦上添花，不应让一次局部失败中断整轮刷新；
/// 且循环末尾的全量重算兜底会纠正任何漏写的未读数。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `app` - 应用句柄；`None` 时只写库不发事件
/// * `feed_id` - 刚处理完的订阅源 ID
/// * `feed_name` - 订阅源名称（事件载荷）
/// * `new_count` - 本次该源新增的文章数
/// * `done` - 已完成源序号（从 1 起）
/// * `total` - 本轮源总数
pub(crate) async fn emit_feed_progress(
    pool: &SqlitePool,
    app: Option<&AppHandle>,
    feed_id: i64,
    feed_name: &str,
    new_count: i64,
    done: usize,
    total: usize,
) {
    // 先 COUNT 再回写（而非 UPDATE + 查询），两条语句都极轻量：
    // 该源文章数通常为几十到几百，单次 COUNT 的代价可忽略
    let unread: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM articles WHERE feed_id = ? AND is_read = 0",
    )
    .bind(feed_id)
    .fetch_one(pool)
    .await
    {
        Ok(n) => n,
        Err(e) => {
            eprintln!("重算订阅源 {} 的未读数失败: {}", feed_id, e);
            return;
        }
    };

    // 把重算结果写回 feeds 表，让数据库侧的未读数也保持实时一致
    if let Err(e) = sqlx::query("UPDATE feeds SET unread_count = ? WHERE id = ?")
        .bind(unread)
        .bind(feed_id)
        .execute(pool)
        .await
    {
        eprintln!("回写订阅源 {} 的未读数失败: {}", feed_id, e);
    }

    // 广播进度事件；窗口缺失等发送失败只记日志
    if let Some(app) = app {
        let payload = RefreshProgress {
            feed_id,
            feed_name: feed_name.to_string(),
            new_count,
            unread_count: unread,
            done,
            total,
        };
        if let Err(e) = app.emit("refresh-progress", payload) {
            eprintln!("发送刷新进度事件失败: {}", e);
        }
    }
}

// ─── 自动化任务管理器 ─────────────────────────────────────────────────────────

use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use crate::db::AutomationTask;
use crate::research;

/// 自动化任务管理器（任务即数据的动态调度器）
///
/// 启动时读取 `automation_tasks` 表，为每个 enabled 任务注册 cron Job；
/// 任务的增删改/启停由命令层调用 [`AutomationManager::reload`] 触发全量
/// 重注册（任务数量级为个位数，全量重建的成本可忽略，换取实现简单可靠）。
///
/// 与 [`RefreshManager`] 各自持有独立的 `JobScheduler` 实例（故障域隔离）：
/// 自动化任务注册失败（如 cron 表达式非法）不影响订阅刷新，反之亦然。
///
/// cron 与手动触发共用 [`research::run_task_once`] 执行器，
/// 并通过共享的 [`research::RUNNING_TASKS`] 防重入：
/// cron 触发撞上手动运行时直接跳过本次（手动那次负责回写状态）。
pub struct AutomationManager {
    /// 数据库连接池（reload 时重新读表）
    pool: SqlitePool,
    /// 底层调度器句柄（注册后启动；reload 时在其上增删 Job）
    scheduler: TokioMutex<Option<JobScheduler>>,
    /// 当前已注册的 Job GUID 列表（reload 时逐个移除）
    jobs: TokioMutex<Vec<Uuid>>,
}

impl AutomationManager {
    /// 创建管理器并按数据库中的任务表完成初始注册
    ///
    /// # 参数
    /// * `pool` - 数据库连接池
    ///
    /// # 返回值
    /// 完成初始注册的管理器
    ///
    /// # 错误
    /// 调度器创建失败时返回错误信息；单个任务注册失败只记日志跳过
    pub async fn new(pool: SqlitePool) -> Result<Self, String> {
        let scheduler = JobScheduler::new().await.map_err(|e| e.to_string())?;
        let manager = AutomationManager {
            pool,
            scheduler: TokioMutex::new(Some(scheduler)),
            jobs: TokioMutex::new(Vec::new()),
        };
        manager.register_all().await;
        Ok(manager)
    }

    /// 启动调度器（注册过的任务开始按 cron 触发）
    ///
    /// # 错误
    /// 调度器启动失败时返回错误信息
    pub async fn start(&self) -> Result<(), String> {
        let guard = self.scheduler.lock().await;
        if let Some(scheduler) = guard.as_ref() {
            scheduler.start().await.map_err(|e| e.to_string())?;
            eprintln!("自动化任务调度器已启动");
        }
        Ok(())
    }

    /// 全量重注册：移除现有 Job 后按任务表当前状态重建
    ///
    /// 由任务 CRUD 命令在写库成功后调用，保证"表里是什么样，调度就是什么样"。
    /// 未启用的任务不注册（但仍可经 `automation_run_now` 手动运行）。
    ///
    /// # 错误
    /// 底层调度器缺失或 Job 移除/添加失败时返回错误信息
    pub async fn reload(&self) -> Result<(), String> {
        let mut jobs = self.jobs.lock().await;
        let guard = self.scheduler.lock().await;
        let scheduler = guard
            .as_ref()
            .ok_or_else(|| "自动化任务调度器未初始化".to_string())?;

        // 移除现有全部 Job（幂等：集合为空时无操作）；remove 接收 &Uuid
        for guid in jobs.drain(..) {
            if let Err(e) = scheduler.remove(&guid).await {
                eprintln!("移除自动化任务 Job 失败（继续重注册）: {}", e);
            }
        }

        self.register_all().await;
        Ok(())
    }

    /// 读取任务表并把所有 enabled 任务注册进调度器
    ///
    /// 单个任务注册失败（典型：cron 表达式非法）只记日志跳过，
    /// 不让一条坏配置拖垮其余任务的调度。
    async fn register_all(&self) {
        let guard = self.scheduler.lock().await;
        let Some(scheduler) = guard.as_ref() else {
            return;
        };

        let tasks = match AutomationTask::list_all(&self.pool).await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("读取自动化任务失败，本轮不注册: {}", e);
                return;
            }
        };

        let mut jobs = self.jobs.lock().await;
        for task in tasks {
            if !task.enabled {
                continue;
            }
            match register_task(scheduler, &self.pool, &task).await {
                Ok(guid) => {
                    jobs.push(guid);
                    eprintln!("已注册自动化任务「{}」cron={}", task.name, task.cron_expr);
                }
                Err(e) => {
                    eprintln!("注册自动化任务「{}」失败（跳过）: {}", task.name, e);
                }
            }
        }
    }
}

/// 为单个任务创建并注册 cron Job
///
/// Job 回调：抢 [`research::RUNNING_TASKS`] 占位 → 执行 → 释放；
/// 抢占失败（已在运行）说明有另一次触发在跑，本次静默跳过——
/// 运行状态由持有占位的那次触发负责回写，避免互相覆盖。
///
/// # 参数
/// * `scheduler` - 目标调度器
/// * `pool` - 数据库连接池
/// * `task` - 待注册的任务
///
/// # 返回值
/// 注册成功的 Job GUID（reload 移除时使用）
///
/// # 错误
/// cron 表达式非法或注册失败时返回错误信息
async fn register_task(
    scheduler: &JobScheduler,
    pool: &SqlitePool,
    task: &AutomationTask,
) -> Result<Uuid, String> {
    // Job 闭包要求 'static 且每次触发都要产出独立 Future：
    // 参照 RefreshManager 的写法，在闭包体内 clone 连接池与任务副本，
    // 再 move 进 async 块，避免借用局部引用的编译错误
    let task_for_job = task.clone();
    let job_pool = pool.clone();
    let job = Job::new_async(task.cron_expr.as_str(), move |_uuid, _l| {
        let pool = job_pool.clone();
        let task = task_for_job.clone();
        Box::pin(async move {
            // 防重入：insert 成功才执行；失败说明任务正在运行，跳过本次 cron 触发
            let acquired = {
                let mut running = research::RUNNING_TASKS.lock().await;
                running.insert(task.id)
            };
            if !acquired {
                eprintln!("自动化任务「{}」正在运行，本次定时触发跳过", task.name);
                return;
            }
            let _ = research::run_task_once(&pool, &task).await;
            let mut running = research::RUNNING_TASKS.lock().await;
            running.remove(&task.id);
        })
    })
    .map_err(|e| format!("cron 表达式非法或注册失败: {}", e))?;

    let guid = job.guid();
    scheduler.add(job).await.map_err(|e| e.to_string())?;
    Ok(guid)
}
