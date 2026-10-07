//! 订阅源「按来源自动分组」
//!
//! # 职责
//! 把 URL 指向同一来源的多个订阅源自动归入同一个文件夹分组
//! （例如 7 个 wired.com 频道归入「WIRED」），让侧边栏在订阅数增长后依然可读。
//!
//! # 为什么"来源"不能直接取域名
//! RSSHub 是**中转实例**：`https://rsshub.app/anthropic/research` 与
//! `https://rsshub.app/eastmoney/report/industry` 的域名完全相同，真实来源却毫无关系。
//! 若按域名分组，这些源会被错误地并成一组「rsshub.app」。
//! 因此 RSSHub 形态的 URL 必须剥掉实例前缀、取路由第一段（平台名）作为来源标识。
//!
//! # 设计意图
//! - **阈值保护**：只有同一来源存在 [`MIN_FEEDS_PER_GROUP`] 个及以上订阅源时才建组。
//!   否则 20 个不同站点会产出 20 个单源分组，视觉上等于"未分类"却多出 20 个折叠头。
//! - **不覆盖用户意图**：自动分组只处理 `folder_id = 0`（未分类）的源；
//!   新增订阅时若调用方已显式指定文件夹，则完全跳过自动分组。
//! - **与实例无关**：RSSHub 源换了实例（官方实例不可用时自动切到备选实例）只是前缀变了，
//!   真实来源并未改变，因此判定按"主机名是不是 RSSHub 实例"来做，
//!   而不是死认某个固定的实例前缀。
//! - **命名可读**：分组名优先取组内源名的公共前缀（`WIRED - AI` / `WIRED - Gear`
//!   → `WIRED`），取不到公共前缀再退回来源 key（域名或 RSSHub 平台名）。
//! - **失败不阻塞主流程**：调用方（[`crate::feeds_add`]）对本模块的错误只记日志，
//!   自动分组属于"锦上添花"，不能因为它失败而让添加订阅整体失败。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::{Folder, Setting};
use crate::rsshub;

/// 一个自动分组所需的最小订阅源数量
///
/// 取 2 是"至少能看出分组价值"的下限：单源分组与"未分类"在视觉上没有区别，
/// 却会额外占据一个折叠头，反而降低侧边栏的信噪比。
pub const MIN_FEEDS_PER_GROUP: usize = 2;

/// 自动分组结果（返回给前端做结果提示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupResult {
    /// 本次**新建**的分组数量（复用已有分组不计入）
    pub created_folders: i64,
    /// 本次被归入分组的订阅源数量
    pub moved_feeds: i64,
    /// 涉及的分组名（新建 + 复用），已排序去重，便于前端直接展示
    pub group_names: Vec<String>,
}

/// 分组计算所需的订阅源最小字段集
///
/// 刻意不复用 [`crate::db::Feed`]：分组只关心 id / 名称 / URL / 现有归属，
/// 查全字段会白白拉回 icon、时间戳等用不到的重字段。
#[derive(Debug, Clone, sqlx::FromRow)]
struct FeedRow {
    id: i64,
    name: String,
    url: String,
    folder_id: i64,
}

/// 从订阅源 URL 推断「来源标识」
///
/// # 参数
/// * `url` - 订阅源 URL，支持四种形态：`rsshub://` 路由、配置实例下的地址、
///   任意 RSSHub 实例地址、普通 http(s) 地址
/// * `rsshub_base` - 设置项 `rsshub_base_url` 的值（未配置时传空串）
///
/// # 返回值
/// 小写的来源标识（域名 / RSSHub 平台名）；无法解析时返回 `None`
///
/// # 判定顺序
/// 1. `rsshub://<路由>` —— 取路由第一段；
/// 2. 以配置的实例地址开头 —— 剥掉实例前缀后取路由第一段（兼容实例带路径前缀的情形）；
/// 3. 主机名属于任一 RSSHub 实例 —— 剥掉 `协议://主机` 后取路径第一段；
/// 4. 其余 —— 取主机名，去掉 `www.` 前缀并转小写。
pub fn source_key(url: &str, rsshub_base: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // 形态 1：rsshub:// 路由协议（如 rsshub://twitter/user/elonmusk → twitter）
    if let Some(route) = url.strip_prefix("rsshub://") {
        return first_segment(route);
    }

    // 形态 2：配置的 RSSHub 实例（如 https://rsshub.app/anthropic/research → anthropic）。
    // 刻意排在主机名判定之前：实例地址可能带路径前缀（如 https://example.com/rsshub），
    // 那种情况下只有按完整前缀剥离才能得到真正的路由首段。
    let base = rsshub_base.trim().trim_end_matches('/');
    if !base.is_empty() {
        if let Some(rest) = url.strip_prefix(base) {
            // 必须校验边界：否则 `https://rsshub.app.evil.com/x` 会被误判成实例内的路由
            if rest.is_empty() || rest.starts_with('/') {
                // 容忍历史上产生的畸形 URL：实例前缀后又嵌套了一次 rsshub:// 协议头
                let rest = rest.trim_start_matches('/');
                let rest = rest.strip_prefix("rsshub://").unwrap_or(rest);
                return first_segment(rest);
            }
        }
    }

    // 形态 3：任意 RSSHub 实例（官方 / 备选 / 用户自建）。换实例只改变前缀、不改变来源，
    // 故按主机名判定，否则 https://rsshub.rssforever.com/anthropic/x 会被当成
    // 一个叫 "rsshub.rssforever.com" 的独立来源，与官方实例下的同名路由分不到一组。
    let (host, path) = rsshub::split_host_path(url);
    if !host.is_empty() && rsshub::is_rsshub_host(&host, rsshub_base) {
        return first_segment(&path);
    }

    // 形态 4：普通 URL —— split_host_path 已剥离协议、userinfo、端口与 www。
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// 取路径的第一段并转小写，空串返回 `None`
///
/// # 参数
/// * `rest` - 待切分的路径片段（首位斜杠可有可无）
fn first_segment(rest: &str) -> Option<String> {
    let segment = rest.trim_start_matches('/').split('/').next().unwrap_or("").trim();
    if segment.is_empty() {
        None
    } else {
        Some(segment.to_lowercase())
    }
}

/// 把源名切成词，供公共前缀比较
///
/// 以空白与常见分隔符切分（`WIRED - AI` → `["WIRED", "AI"]`），
/// 空词直接丢弃，避免连续分隔符产生的前缀误判。
///
/// # 参数
/// * `name` - 订阅源名称
fn tokenize(name: &str) -> Vec<String> {
    name.split(|c: char| c.is_whitespace() || c == '-' || c == '·' || c == '|' || c == '—')
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// 计算分组名：组内源名的公共前缀优先，取不到则退回来源 key
///
/// # 参数
/// * `members` - 同属一个来源的全部订阅源（顺序需稳定，调用方按 id 升序给出）
/// * `key` - 该来源的来源标识，作为兜底名称
///
/// # 返回值
/// 分组名。公共前缀不足 3 个字符时同样退回 `key`，避免产出 `a`、`AI` 之类的碎片名
///
/// # 为什么按"词"而不是按"字符"取前缀
/// 按字符取会把 `WIRED - AI` 与 `Wired` 的共同前缀算成 `WIRE` 这类半截词；
/// 按词对齐才能得到干净的分组名。
fn group_name(members: &[FeedRow], key: &str) -> String {
    let tokenized: Vec<Vec<String>> = members.iter().map(|m| tokenize(&m.name)).collect();
    // 公共前缀最长不可能超过"词数最少的那个源"
    let min_len = tokenized.iter().map(|t| t.len()).min().unwrap_or(0);

    let mut prefix = 0usize;
    for i in 0..min_len {
        let first = tokenized[0][i].to_lowercase();
        if tokenized.iter().all(|t| t[i].to_lowercase() == first) {
            prefix += 1;
        } else {
            break;
        }
    }
    if prefix == 0 {
        return key.to_string();
    }

    // 每个词位取"出现次数最多的原始大小写"，避免随口选到某个源而得到 Wired / wired。
    // 用 Vec 记录首次出现顺序（而非 HashMap），使平票时的选择可复现——
    // HashMap 迭代顺序不稳定，会让分组名在多次运行间跳变。
    let mut parts = Vec::with_capacity(prefix);
    for i in 0..prefix {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for t in &tokenized {
            match counts.iter_mut().find(|(w, _)| *w == t[i]) {
                Some(entry) => entry.1 += 1,
                None => counts.push((t[i].clone(), 1)),
            }
        }
        // rev() 是为了让平票时选中"首次出现"的写法（max_by_key 返回最后一个最大值）
        let best = counts
            .iter()
            .rev()
            .max_by_key(|(_, c)| *c)
            .map(|(w, _)| w.clone())
            .unwrap_or_default();
        parts.push(best);
    }

    let name = parts.join(" ");
    if name.chars().count() < 3 {
        key.to_string()
    } else {
        name
    }
}

/// 读取全部订阅源（按 id 升序，保证分组命名可复现）
///
/// # 错误
/// 数据库查询失败时返回错误信息
async fn load_feeds(pool: &SqlitePool) -> Result<Vec<FeedRow>, String> {
    sqlx::query_as::<_, FeedRow>("SELECT id, name, url, folder_id FROM feeds ORDER BY id ASC")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())
}

/// 取下一个可用的文件夹排序位（当前最大 position + 1）
///
/// # 错误
/// 数据库查询失败时返回错误信息
async fn next_position(pool: &SqlitePool) -> Result<i64, String> {
    sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(position), -1) + 1 FROM folders")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())
}

/// 按名称查找文件夹，不存在则新建
///
/// 复用同名文件夹是"自动分组不与用户手工分组打架"的关键：
/// 用户若已手工建了名为「WIRED」的分组，自动分组会直接归入它，而不是再建一个同名组。
///
/// # 参数
/// * `name` - 目标分组名
///
/// # 返回值
/// 文件夹 ID（已存在时为既有 ID）
///
/// # 错误
/// 查询或写入失败时返回错误信息
async fn find_or_create_folder(pool: &SqlitePool, name: &str) -> Result<i64, String> {
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM folders WHERE name = ? LIMIT 1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let position = next_position(pool).await?;
    Folder::create(pool, name, position).await
}

/// 新增订阅后的单条自动归组
///
/// 在 [`crate::feeds_add`] 把新源落库之后调用：若该来源在库中已有其它源
/// （即达到 [`MIN_FEEDS_PER_GROUP`]），就把新源归入对应的分组文件夹。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `feed_id` - 刚入库的订阅源 ID
/// * `url` - 该订阅源的 URL
///
/// # 返回值
/// 归组成功时返回分组名；未达阈值或无法判定来源时返回 `None`
///
/// # 错误
/// 读取设置、查询或写入失败时返回错误信息
pub async fn assign_source_group(
    pool: &SqlitePool,
    feed_id: i64,
    url: &str,
) -> Result<Option<String>, String> {
    let base = Setting::get(pool, "rsshub_base_url").await?.unwrap_or_default();
    let key = match source_key(url, &base) {
        Some(k) => k,
        None => return Ok(None),
    };

    // 此时新源已在库中，因此 members 天然包含它自己
    let members: Vec<FeedRow> = load_feeds(pool)
        .await?
        .into_iter()
        .filter(|f| source_key(&f.url, &base).as_deref() == Some(key.as_str()))
        .collect();
    if members.len() < MIN_FEEDS_PER_GROUP {
        return Ok(None);
    }

    let name = group_name(&members, &key);
    let folder_id = find_or_create_folder(pool, &name).await?;
    Folder::set_feed_folder(pool, feed_id, folder_id).await?;
    Ok(Some(name))
}

/// 一键整理存量订阅源：把同来源且尚未分组的源归入自动分组
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// 新建分组数、归入的源数与涉及的分组名
///
/// # 行为约定
/// - 只处理 `folder_id = 0`（未分类）的源，**不动**用户已手工归类到任何文件夹的源；
/// - 分组名已存在时复用，不重复创建；
/// - 组内若没有任何"未分类"的源，则连分组都不创建，避免留下空分组。
///
/// # 错误
/// 读取设置、查询或写入失败时返回错误信息
pub async fn auto_group(pool: &SqlitePool) -> Result<GroupResult, String> {
    let base = Setting::get(pool, "rsshub_base_url").await?.unwrap_or_default();

    // 按来源聚合。HashMap 的迭代顺序不稳定，故下方先对 key 排序再处理，
    // 保证同样的数据每次运行得到同样的分组顺序与 position。
    let mut buckets: HashMap<String, Vec<FeedRow>> = HashMap::new();
    for feed in load_feeds(pool).await? {
        if let Some(key) = source_key(&feed.url, &base) {
            buckets.entry(key).or_default().push(feed);
        }
    }

    // 已有分组按名称索引：既包含历史自动分组，也包含用户手工创建的同名分组
    let mut folder_by_name: HashMap<String, i64> = Folder::list_all(pool)
        .await?
        .into_iter()
        .map(|f| (f.name, f.id))
        .collect();
    let mut position = next_position(pool).await?;

    let mut created_folders = 0i64;
    let mut moved_feeds = 0i64;
    let mut group_names = Vec::new();

    let mut keys: Vec<String> = buckets.keys().cloned().collect();
    keys.sort();

    for key in keys {
        let members = &buckets[&key];
        if members.len() < MIN_FEEDS_PER_GROUP {
            continue;
        }

        // 待迁移成员：只挑未分类的源。若一个都没有，说明该组已被用户手工处理完毕，
        // 此时不创建分组，避免留下一个空的分组头。
        let pending: Vec<&FeedRow> = members.iter().filter(|m| m.folder_id == 0).collect();
        if pending.is_empty() {
            continue;
        }

        // 分组名用「组内全部源」计算，而不是只用待迁移的那部分——
        // 否则部分源已被手工归类时，算出的公共前缀会偏短，分组名不稳定。
        let name = group_name(members, &key);
        let folder_id = match folder_by_name.get(&name) {
            Some(id) => *id,
            None => {
                let id = Folder::create(pool, &name, position).await?;
                position += 1;
                folder_by_name.insert(name.clone(), id);
                created_folders += 1;
                id
            }
        };

        group_names.push(name);
        for member in pending {
            Folder::set_feed_folder(pool, member.id, folder_id).await?;
            moved_feeds += 1;
        }
    }

    group_names.sort();
    group_names.dedup();
    Ok(GroupResult {
        created_folders,
        moved_feeds,
        group_names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造测试用订阅源行，减少样板
    fn feed(id: i64, name: &str, url: &str) -> FeedRow {
        FeedRow {
            id,
            name: name.to_string(),
            url: url.to_string(),
            folder_id: 0,
        }
    }

    /// 普通域名：去 www、转小写、忽略路径
    #[test]
    fn source_key_plain_domain() {
        assert_eq!(
            source_key("https://www.wired.com/feed/rss", "").as_deref(),
            Some("wired.com")
        );
        assert_eq!(
            source_key("https://OpenAI.com/blog/rss.xml", "").as_deref(),
            Some("openai.com")
        );
    }

    /// RSSHub 实例 URL 取路由第一段，而不是实例域名
    #[test]
    fn source_key_rsshub_instance() {
        let base = "https://rsshub.app";
        assert_eq!(
            source_key("https://rsshub.app/anthropic/research", base).as_deref(),
            Some("anthropic")
        );
        // 畸形 URL：实例前缀后又嵌套了一层 rsshub:// 协议头
        assert_eq!(
            source_key("https://rsshub.app/rsshub://eastmoney/report/industry", base).as_deref(),
            Some("eastmoney")
        );
        // 非 RSSHub 形态的域名：退回域名判定（"other-rsshub.dev" 不以 rsshub. 开头）
        assert_eq!(
            source_key("https://other-rsshub.dev/anthropic/research", base).as_deref(),
            Some("other-rsshub.dev")
        );
    }

    /// 换了 RSSHub 实例仍算同一来源（按主机名判定，而不是死认配置的那一个实例）
    #[test]
    fn source_key_any_rsshub_instance() {
        // 配置的是官方实例，但用户直接粘了备选实例的地址
        let base = "https://rsshub.app";
        assert_eq!(
            source_key("https://rsshub.rssforever.com/anthropic/research", base).as_deref(),
            Some("anthropic")
        );
        // 非 rsshub.* 前缀的实例也在候选清单里，同样要识别
        assert_eq!(
            source_key("https://rss.qiuyuair.com/twitter/user/elonmusk", "").as_deref(),
            Some("twitter")
        );
        assert_eq!(
            source_key("https://rsshub.netlify.app/github/trending/daily", "").as_deref(),
            Some("github")
        );
        // 与官方实例下的同名路由归为同一个来源 key（"自动分组时能分到一组"的前提）
        assert_eq!(
            source_key("https://rsshub.app/github/trending/daily", base),
            source_key("https://rsshub.uneasy.win/github/trending/daily", base)
        );
        // 用户自建实例（配置里给出）同样按实例处理
        assert_eq!(
            source_key("https://my.proxy.dev/anthropic/research", "https://my.proxy.dev").as_deref(),
            Some("anthropic")
        );
        // 实例根路径没有路由可作来源
        assert_eq!(source_key("https://rsshub.app", base), None);
    }

    /// rsshub:// 路由协议取路由第一段
    #[test]
    fn source_key_rsshub_scheme() {
        assert_eq!(
            source_key("rsshub://twitter/user/elonmusk", "").as_deref(),
            Some("twitter")
        );
        assert_eq!(source_key("rsshub://", ""), None);
    }

    /// 前缀校验边界：实例地址的"相似域名"不应被当成实例内路由
    #[test]
    fn source_key_prefix_boundary() {
        assert_eq!(
            source_key("https://rsshub.app.evil.com/x/y", "https://rsshub.app").as_deref(),
            Some("rsshub.app.evil.com")
        );
    }

    /// 分组名：公共前缀优先，且取出现次数最多的大小写写法
    #[test]
    fn group_name_prefix_wins() {
        let members = vec![
            feed(1, "Wired", "https://www.wired.com/feed/rss"),
            feed(2, "WIRED - AI", "https://www.wired.com/feed/tag/ai/latest/rss"),
            feed(3, "WIRED - Gear", "https://www.wired.com/feed/category/gear/latest/rss"),
        ];
        // "WIRED" 出现 2 次、"Wired" 1 次 → 取 WIRED
        assert_eq!(group_name(&members, "wired.com"), "WIRED");
    }

    /// 分组名：无公共前缀时退回来源 key
    #[test]
    fn group_name_falls_back_to_key() {
        let members = vec![
            feed(1, "https://rsshub.app/eastmoney/report/industry", "https://rsshub.app/eastmoney/report/industry"),
            feed(2, "https://rsshub.app/rsshub://eastmoney/report/industry", "https://rsshub.app/rsshub://eastmoney/report/industry"),
        ];
        assert_eq!(group_name(&members, "eastmoney"), "eastmoney");
    }

    /// 端到端：在真实 SQLite 上跑 auto_group，覆盖建组 / 只迁移未分类 / 阈值 / 幂等四条规则
    #[tokio::test]
    async fn auto_group_end_to_end() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("内存库连接失败");
        // 只建本测试涉及的三张表。刻意不复用 db::create_tables——
        // 它需要 AppHandle 才能走到，而这里要的是零依赖的纯逻辑验证。
        // 列定义必须覆盖被测 SQL 真正会碰到的字段：Folder::set_feed_folder 会同时
        // 写 folder_id 与 updated_at，漏掉后者会让测试以"no such column"失败。
        for ddl in [
            "CREATE TABLE feeds (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, \
             url TEXT NOT NULL UNIQUE, folder_id INTEGER DEFAULT 0, \
             updated_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
            "CREATE TABLE folders (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, \
             position INTEGER DEFAULT 0)",
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("建表失败");
        }

        // 一个用户手工创建的分组，用于验证"不动手工归类"
        sqlx::query("INSERT INTO folders (name, position) VALUES ('我的收藏', 0)")
            .execute(&pool)
            .await
            .expect("插入文件夹失败");

        // 覆盖四种情形的语料：
        // - wired：2 个未分类 + 1 个已手工归类（组名仍应由全部成员算出 WIRED）
        // - openai：2 个未分类（应建组）
        // - techcrunch：单源（不建组）
        // - theverge：单源且已手工归类（既不建组也不搬动）
        let seeds: [(&str, &str, i64); 6] = [
            ("Wired", "https://www.wired.com/feed/rss", 0),
            ("WIRED - AI", "https://www.wired.com/feed/tag/ai/latest/rss", 0),
            ("WIRED - Gear", "https://www.wired.com/feed/category/gear/latest/rss", 1),
            ("OpenAI Blog", "https://openai.com/blog/rss.xml", 0),
            ("OpenAI News", "https://openai.com/news/rss.xml", 0),
            ("TechCrunch", "https://techcrunch.com/feed/", 0),
        ];
        for (name, url, folder_id) in seeds {
            sqlx::query("INSERT INTO feeds (name, url, folder_id) VALUES (?, ?, ?)")
                .bind(name)
                .bind(url)
                .bind(folder_id)
                .execute(&pool)
                .await
                .expect("插入订阅源失败");
        }
        sqlx::query("INSERT INTO feeds (name, url, folder_id) VALUES ('The Verge', ?, 1)")
            .bind("https://www.theverge.com/rss/index.xml")
            .execute(&pool)
            .await
            .expect("插入订阅源失败");

        let result = auto_group(&pool).await.expect("自动分组失败");
        // 建 2 组（WIRED / OpenAI）；迁移 4 个未分类源（wired 2 + openai 2）
        assert_eq!(result.created_folders, 2);
        assert_eq!(result.moved_feeds, 4);
        assert_eq!(
            result.group_names,
            vec!["OpenAI".to_string(), "WIRED".to_string()]
        );

        // 已手工归类的源（WIRED - Gear / The Verge）保持原文件夹不变
        for name in ["WIRED - Gear", "The Verge"] {
            let folder: i64 = sqlx::query_scalar("SELECT folder_id FROM feeds WHERE name = ?")
                .bind(name)
                .fetch_one(&pool)
                .await
                .expect("查询失败");
            assert_eq!(folder, 1, "{name} 不应被自动分组搬走");
        }

        // 未分类的同源源已归入各自分组，且两个分组互不相同
        let wired: i64 = sqlx::query_scalar("SELECT folder_id FROM feeds WHERE name = 'Wired'")
            .fetch_one(&pool)
            .await
            .expect("查询失败");
        let openai: i64 =
            sqlx::query_scalar("SELECT folder_id FROM feeds WHERE name = 'OpenAI Blog'")
                .fetch_one(&pool)
                .await
                .expect("查询失败");
        assert_ne!(wired, 0);
        assert_ne!(openai, 0);
        assert_ne!(wired, openai);

        // 单源来源保持未分类（阈值保护）
        let solo: i64 =
            sqlx::query_scalar("SELECT folder_id FROM feeds WHERE name = 'TechCrunch'")
                .fetch_one(&pool)
                .await
                .expect("查询失败");
        assert_eq!(solo, 0);

        // 幂等：再跑一次不应有任何变化（已归组的源不再是"未分类"）
        let again = auto_group(&pool).await.expect("二次自动分组失败");
        assert_eq!(again.created_folders, 0);
        assert_eq!(again.moved_feeds, 0);
        assert!(again.group_names.is_empty());
    }
}
