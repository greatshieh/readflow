//! 研究报告：日报 / 周报的 Markdown 渲染与多通道输出
//!
//! 职责：从事件库聚合时间窗口内的事件 → 按实体分组渲染 Markdown →
//! 按 config 中的 channel 写入目标位置（应用内数据目录 / Obsidian Vault / 两者）。
//!
//! 设计要点：
//! - 报告内容是**事件列表**而非摘要：每条事件含日期、类型、事实、证据引用与来源链接，
//!   保持与事件库一致的可溯源语义；
//! - 正文按 **Obsidian 惯例**渲染（YAML frontmatter + callout + 实体双链）：
//!   报告最终与用户既有笔记共存于同一个 Vault，frontmatter 供 dataview 按类型建视图，
//!   双链供实体笔记的反链面板自动聚合。应用内查看器是等宽纯文本展示，由前端
//!   `reportMarkdown.ts::stripFrontmatter` 在**展示时**剥掉头部——
//!   磁盘上的文件始终保留完整格式，只有界面呈现会被裁剪；
//! - 输出通道由任务 config 决定（channel 字段：`app` / `obsidian` / `both`），
//!   缺配置时报 Unconfigured 引导用户，而不是静默失败；
//! - 报告文件统一落盘到应用数据目录 `reports/`（app/file/both 通道），
//!   Phase C 的应用内报告查看器从这里读取；obsidian 与 both 通道额外写入用户 Vault；
//! - `both` 是"应用内 + Vault"双写：既保留可被查看器读取的那份，又同步进 Vault。
//!   它**先校验 Vault 配置再写任何文件**，避免留下"应用内已更新、Vault 没有"的不一致状态。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{Datelike, Local};
use sqlx::SqlitePool;

use crate::db::{EventWithContext, ResearchEvent, Setting};

use super::{TaskOutcome, extract::EVENT_TYPES};

/// 报告任务的应用数据目录（启动时由 lib.rs 注入，cron 与命令共用）
///
/// "file"/"app" 通道的落盘位置依赖 Tauri 路径解析（需要 AppHandle），
/// 而 cron 回调里拿不到句柄，因此在启动阶段解析一次存入全局。
static REPORTS_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Obsidian 相关设置键（与 `obsidian.rs` 文章导出共用同一组配置，
/// 使报告与文章笔记的落点一致；文章导出优先，报告仅做读取）
const KEY_VAULT_PATH: &str = "obsidian_vault_path";
const KEY_EXPORT_FOLDER: &str = "obsidian_export_folder";
/// 子目录兜底名（用户既未在 config 指定、也未配导出目录时使用）
const DEFAULT_EXPORT_FOLDER: &str = "ReadFlow";

/// 注册报告输出目录（应用启动时调用一次）
///
/// # 参数
/// * `dir` - 应用数据目录下的 `reports` 子目录路径
pub fn set_reports_dir(dir: PathBuf) {
    let _ = REPORTS_DIR.set(dir);
}

/// 读取已注册的报告输出目录（应用内报告查看器使用）
///
/// # 返回值
/// 报告目录路径；尚未注册（启动早期）时返回 `None`
pub fn reports_dir() -> Option<&'static PathBuf> {
    REPORTS_DIR.get()
}

/// 报告类型（决定默认时间窗口与文件命名前缀）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReportKind {
    /// 日报：默认覆盖近 1 天
    Daily,
    /// 周报：默认覆盖近 7 天
    Weekly,
}

impl ReportKind {
    /// 报告标题中的类型名（日报 / 周报）
    fn label(self) -> &'static str {
        match self {
            ReportKind::Daily => "日报",
            ReportKind::Weekly => "周报",
        }
    }

    /// frontmatter 的 `type` 值（供 Obsidian dataview 按报告类型建视图）
    fn slug(self) -> &'static str {
        match self {
            ReportKind::Daily => "research-daily",
            ReportKind::Weekly => "research-weekly",
        }
    }

    /// frontmatter 的 `tags` 里附加的中文标签（便于在 Obsidian 标签面板归类）
    fn tag(self) -> &'static str {
        match self {
            ReportKind::Daily => "研究日报",
            ReportKind::Weekly => "研究周报",
        }
    }

    /// config 未显式给出 days 时的默认时间窗口（天）
    fn default_days(self) -> i64 {
        match self {
            ReportKind::Daily => 1,
            ReportKind::Weekly => 7,
        }
    }

    /// 从外部字符串解析报告类型（手动生成命令的入参）
    ///
    /// 大小写与首尾空白均不敏感。解析失败时返回 `None` 而非回退默认值——
    /// 手动生成场景下"用户选了个不存在的类型"应当报错，而不是静默产出一份日报。
    ///
    /// # 参数
    /// * `raw` - 类型标识：`daily` 或 `weekly`
    ///
    /// # 返回值
    /// 对应的报告类型；无法识别时返回 `None`
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "daily" => Some(ReportKind::Daily),
            "weekly" => Some(ReportKind::Weekly),
            _ => None,
        }
    }
}

/// 报告生成配置（从 config JSON 解析而来，缺项回退默认值）
struct ReportConfig {
    /// 输出通道：`app` / `file`（应用数据目录 reports/）、`obsidian`（仅 Vault）
    /// 或 `both`（应用数据目录 + Vault 双写）
    channel: String,
    /// 聚合事件的时间窗口（天）
    days: i64,
    /// Vault 内的子目录；`None` 表示 config 未指定，此时回退「设置 → Obsidian」
    /// 的导出目录，使报告与文章笔记落在同一处
    vault_subpath: Option<String>,
}

/// 解析报告 config JSON
///
/// 允许只配部分字段：缺失或非法的项一律回退默认值，因此前端手动生成时
/// 只需拼它真正关心的键，不必复刻全部字段。
///
/// # 参数
/// * `config_json` - 任务 config 或前端拼装的 JSON 字符串（非法时按空对象处理）
/// * `kind` - 报告类型（决定 days 的默认值）
///
/// # 返回值
/// 解析完成的配置
fn parse_report_config(config_json: &str, kind: ReportKind) -> ReportConfig {
    let parsed: serde_json::Value =
        serde_json::from_str(config_json).unwrap_or(serde_json::json!({}));
    ReportConfig {
        channel: parsed
            .get("channel")
            .and_then(|v| v.as_str())
            .unwrap_or("app")
            .to_string(),
        days: parsed
            .get("days")
            .and_then(|v| v.as_i64())
            .filter(|&d| d >= 1)
            .unwrap_or_else(|| kind.default_days()),
        // 留空等同未指定：落入「设置 → Obsidian」的导出目录
        vault_subpath: parsed
            .get("vault_subpath")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    }
}

/// 生成一份报告并落盘（cron 任务与手动生成共用的核心实现）
///
/// 流程：解析 config → 聚合事件 → 渲染 Markdown → 按 channel 落盘。
/// 与 [`run_report_task`] 的区别在于**不涉及任务行**：既不从 `automation_tasks`
/// 取配置，也不回写运行状态，因此"手动生成报告"无需先创建任何任务。
///
/// 文件名由 [`report_filename`] 按类型与日期确定，因此同一时间窗口重复生成
/// 会覆盖同名文件（日报按日期、周报按 ISO 周号）——同一天只保留最新一份。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `config_json` - 报告配置 JSON（channel / days / vault_subpath，缺项回退默认值）
/// * `kind` - 报告类型（日报 / 周报）
///
/// # 返回值
/// `(报告文件名, 结果描述)`；应用内通道的文件名可直接交给报告读取命令使用
///
/// # 错误
/// 读事件失败、通道非法、Vault 未配置（错误信息以"未配置"开头）或写盘失败时返回错误信息
pub async fn generate_report(
    pool: &SqlitePool,
    config_json: &str,
    kind: ReportKind,
) -> Result<(String, String), String> {
    let config = parse_report_config(config_json, kind);

    // 聚合事件：窗口内全部事件（entity_ids / event_types 过滤留待筛选 UI 提供时启用；
    // 当前所有路径都走全量，行为可预期）
    let rows = ResearchEvent::list_with_context(pool, config.days)
        .await
        .map_err(|e| format!("读取事件失败: {}", e))?;

    let markdown = render_report(kind, config.days, &rows);

    // 按通道分发落盘
    let path_desc = match config.channel.as_str() {
        "obsidian" => {
            deliver_to_obsidian(pool, kind, config.vault_subpath.as_deref(), &markdown).await?
        }
        // app 与 file 通道现阶段都写入应用数据目录的 reports/：
        // app 通道的"应用内查看"由报告查看器读取同一目录
        "app" | "file" => deliver_to_app_dir(kind, &markdown)?,
        // 双写：先解析 Vault 目标目录（含配置校验）再落两处，避免 Vault 缺配置时
        // 留下"应用内已更新、Vault 没有"的不一致状态
        "both" => {
            let vault_dir = resolve_vault_dir(pool, config.vault_subpath.as_deref()).await?;
            let app_desc = deliver_to_app_dir(kind, &markdown)?;
            let vault_path = write_report_to(&vault_dir, kind, &markdown)?;
            format!("{}；另写入 Obsidian Vault: {}", app_desc, vault_path.display())
        }
        other => {
            return Err(format!(
                "未知输出通道: {}（支持 app / obsidian / both / file）",
                other
            ))
        }
    };

    let message = format!(
        "{}已生成：{} 条事件 / {} 个实体，{}",
        kind.label(),
        rows.len(),
        count_entities(&rows),
        path_desc
    );
    Ok((report_filename(kind), message))
}

/// 执行一次报告任务（daily_report / weekly_report 的执行器）
///
/// 是 [`generate_report`] 的薄封装，只负责把结果映射为任务健康度三态：
/// Obsidian 缺 Vault 路径（错误信息以"未配置"开头）归为 `Unconfigured`
/// 引导用户补配置，其余错误归为 `Failed`。这样自动化任务与手动生成
/// 共享同一份生成逻辑，两边的报告内容与文案不会分叉。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `config` - 任务 config JSON
/// * `kind` - 报告类型（日报 / 周报）
///
/// # 返回值
/// 任务执行结果（成功时描述写入位置与事件统计）
pub async fn run_report_task(pool: &SqlitePool, config: &str, kind: ReportKind) -> TaskOutcome {
    match generate_report(pool, config, kind).await {
        // 文件名对任务执行器无用（状态展示只需要描述），丢弃即可
        Ok((_, message)) => TaskOutcome::Success(message),
        Err(e) if e.starts_with("未配置") => TaskOutcome::Unconfigured(e),
        Err(e) => TaskOutcome::Failed(e),
    }
}

/// 统计事件行中的去重实体数（用于结果描述）
fn count_entities(rows: &[EventWithContext]) -> usize {
    let mut names: Vec<&str> = rows.iter().map(|r| r.entity_name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    names.len()
}

/// 渲染报告 Markdown（Obsidian 惯例格式）
///
/// 结构：YAML frontmatter（type / date / period / events / entities / tags）
/// → 一级标题 → `[!summary]` 统计 callout → 按实体分组的二级标题（双链）+
/// 事件列表（类型 / 日期 / 事实）+ `[!quote]` 证据 callout（内含来源链接）。
///
/// 之所以按 Obsidian 惯例渲染：报告最终落在用户 Vault 里与既有笔记共存，
/// frontmatter 让 dataview 能按报告类型建视图，双链让实体笔记的反链面板
/// 自动聚合"提到该实体的所有报告"——这两件事纯 Markdown 给不了。
///
/// 无事件时仍输出完整 frontmatter，只把正文换成提示 callout，
/// 保持骨架稳定以便跨期归档对比。
///
/// # 参数
/// * `kind` - 报告类型
/// * `days` - 覆盖天数
/// * `rows` - 窗口内的事件行（已按实体名排序）
///
/// # 返回值
/// 完整 Markdown 文本
fn render_report(kind: ReportKind, days: i64, rows: &[EventWithContext]) -> String {
    let today = Local::now().format("%Y-%m-%d");
    let period = format!("近 {} 天", days);
    let entities = count_entities(rows);

    // frontmatter：period 含空格故加引号，其余取值均为无歧义字面量或数字，
    // 不会被 YAML 误解析
    let mut md = format!(
        "---\ntype: {}\ndate: {}\nperiod: \"{}\"\nevents: {}\nentities: {}\ntags: [readflow, {}]\n---\n\n",
        kind.slug(),
        today,
        period,
        rows.len(),
        entities,
        kind.tag()
    );

    md.push_str(&format!("# 研究{} · {}\n\n", kind.label(), today));
    // 结尾只留一个换行：实体循环自己以 `\n## ` 开头，两者恰好凑成组前空行
    md.push_str(&format!(
        "> [!summary] 覆盖{}\n> 共 **{}** 条事件 · **{}** 个实体\n",
        period,
        rows.len(),
        entities
    ));

    if rows.is_empty() {
        md.push_str("> [!info] 本期无动态\n> 时间窗口内没有已提取的研究事件。\n");
        return md;
    }

    // 按实体分组（rows 已按实体名排序，直接相邻聚合即可，无需 HashMap 保序）
    let mut current_entity = "";
    for row in rows {
        if row.entity_name != current_entity {
            current_entity = &row.entity_name;
            md.push_str(&format!("\n## {}\n\n", entity_heading(&row.entity_name)));
        }
        // 事件行：类型 + 日期（未知回退入库日期）+ 事实
        let date_desc = if row.event_date.is_empty() {
            row.created_at.format("%Y-%m-%d").to_string()
        } else {
            row.event_date.clone()
        };
        md.push_str(&format!(
            "- **[{}]** {} — {}\n",
            row.event_type, date_desc, row.fact
        ));

        // 来源链接：文章可能已被保留策略清理（LEFT JOIN 为 NULL），此时省略
        let source = match (&row.article_title, &row.article_link) {
            (Some(title), Some(link)) if !link.is_empty() => {
                Some(format!("— [{}]({})", truncate_title(title), link))
            }
            _ => None,
        };

        // 证据引用块：反幻觉锚点，人工抽检的入口。用 callout 让它在长报告里
        // 从事件列表中跳出来；来源附在同一块内，作为该条引用的出处
        if row.evidence.is_empty() {
            if let Some(src) = source {
                md.push_str(&format!("  {}\n", src));
            }
        } else {
            md.push_str(&format!("  > [!quote] {}\n", row.evidence.replace('\n', " ")));
            if let Some(src) = source {
                md.push_str(&format!("  > {}\n", src));
            }
        }
    }

    md
}

/// 把实体名渲染为 Obsidian 双链（不安全时退化为纯文本）
///
/// # 参数
/// * `name` - 实体规范名
///
/// # 返回值
/// 可直接写进标题的文本
fn entity_heading(name: &str) -> String {
    if is_wikilink_safe(name) {
        format!("[[{}]]", name)
    } else {
        name.to_string()
    }
}

/// 判断实体名能否安全地放进 Obsidian 双链
///
/// `[` `]` 会破坏链接语法本身，`#` `|` `^` 则被 Obsidian 解析为标题锚点、
/// 别名分隔与块引用，从而指向别的目标。含这些字符时宁可不产生链接，
/// 也不产生指向错误笔记的链接。
///
/// # 参数
/// * `name` - 实体规范名
///
/// # 返回值
/// 可安全用于 `[[...]]` 时为 `true`
fn is_wikilink_safe(name: &str) -> bool {
    !name.is_empty() && !name.contains(['[', ']', '#', '|', '^'])
}

/// 截断过长的文章标题（来源链接展示用）
fn truncate_title(title: &str) -> String {
    if title.chars().count() > 30 {
        format!("{}…", title.chars().take(30).collect::<String>())
    } else {
        title.to_string()
    }
}

/// 生成报告文件名（含类型与日期，避免覆盖历史报告）
fn report_filename(kind: ReportKind) -> String {
    let now = Local::now();
    match kind {
        ReportKind::Daily => format!("研究日报-{}.md", now.format("%Y-%m-%d")),
        ReportKind::Weekly => {
            // ISO 周数命名，便于跨年归档排序
            format!("研究周报-{}-W{:02}.md", now.year(), now.iso_week().week())
        }
    }
}

/// 落盘到应用数据目录（app / file 通道）
///
/// # 参数
/// * `kind` - 报告类型（决定文件名）
/// * `markdown` - 报告内容
///
/// # 返回值
/// 成功时返回展示用路径描述
///
/// # 错误
/// 报告目录未初始化（启动异常）或文件写入失败时返回错误信息
fn deliver_to_app_dir(kind: ReportKind, markdown: &str) -> Result<String, String> {
    let dir = REPORTS_DIR
        .get()
        .ok_or("报告目录未初始化，请重启应用")?;
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建报告目录失败: {}", e))?;
    }
    let filename = report_filename(kind);
    let path = dir.join(&filename);
    std::fs::write(&path, markdown).map_err(|e| format!("写入报告文件失败: {}", e))?;
    Ok(format!("已写入 {}", path.display()))
}

/// 解析 Obsidian 通道的目标目录（Vault 根 / 子目录）
///
/// 目录优先级：config 的 `vault_subpath`（显式指定最高）→「设置 → Obsidian」的
/// 导出目录（与文章导出共用，使报告与文章笔记落在同一处）→ 兜底 `ReadFlow`。
///
/// 本函数同时承担**配置校验**：Vault 路径未配置时即以"未配置"开头报错，
/// 因此 `both` 通道可以在写任何文件之前先调用它，避免产出半成品。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `subpath` - config 指定的子目录；`None` 表示未指定而回退设置项
///
/// # 返回值
/// Vault 内的目标目录（尚未创建）
///
/// # 错误
/// Vault 路径未配置时返回以"未配置"开头的引导信息
async fn resolve_vault_dir(pool: &SqlitePool, subpath: Option<&str>) -> Result<PathBuf, String> {
    let vault_path = Setting::get(pool, KEY_VAULT_PATH)
        .await?
        .filter(|s| !s.trim().is_empty())
        .ok_or("未配置 Obsidian Vault 路径，请到「设置 → Obsidian」填写后重试")?;

    // 子目录：config 优先，其次沿用文章导出目录，最后兜底默认名
    let folder = match subpath.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => Setting::get(pool, KEY_EXPORT_FOLDER)
            .await?
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_EXPORT_FOLDER.to_string()),
    };

    Ok(PathBuf::from(vault_path.trim()).join(folder))
}

/// 把报告写入指定目录（必要时创建目录）
///
/// # 参数
/// * `dir` - 目标目录
/// * `kind` - 报告类型（决定文件名）
/// * `markdown` - 报告内容
///
/// # 返回值
/// 成功时返回写入的文件路径
///
/// # 错误
/// 目录创建失败或文件写入失败时返回错误信息
fn write_report_to(dir: &Path, kind: ReportKind, markdown: &str) -> Result<PathBuf, String> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建报告目录失败: {}", e))?;
    }
    let path = dir.join(report_filename(kind));
    std::fs::write(&path, markdown).map_err(|e| format!("写入报告文件失败: {}", e))?;
    Ok(path)
}

/// 落盘到 Obsidian Vault（obsidian 通道）
///
/// 写入 `Vault 根 / 子目录 / 文件名`；Vault 路径与子目录的解析规则见
/// [`resolve_vault_dir`]（与文章导出共用「设置 → Obsidian」配置），
/// 缺失时报 Unconfigured 引导用户补配置。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `kind` - 报告类型（决定文件名）
/// * `subpath` - config 指定的 Vault 子目录；`None` 表示回退设置项
/// * `markdown` - 报告内容
///
/// # 返回值
/// 成功时返回展示用路径描述
///
/// # 错误
/// Vault 路径未配置（以"未配置"开头）或文件写入失败时返回错误信息
async fn deliver_to_obsidian(
    pool: &SqlitePool,
    kind: ReportKind,
    subpath: Option<&str>,
    markdown: &str,
) -> Result<String, String> {
    let dir = resolve_vault_dir(pool, subpath).await?;
    let path = write_report_to(&dir, kind, markdown)?;
    Ok(format!("已写入 Obsidian Vault: {}", path.display()))
}

/// 事件类型枚举的导出（供 Phase C 前端筛选 UI 对齐文案）
///
/// 当前仅引用以避免 dead_code 警告；Phase C 的筛选器直接复用该常量。
const _: [&str; 7] = EVENT_TYPES;

#[cfg(test)]
mod tests {
    //! 报告配置解析与多通道落盘的行为验证
    //!
    //! 建表一律走 [`crate::db::create_tables`]，夹具与真实 schema 不会漂移；
    //! 落盘用例使用进程临时目录，结束后清理。

    use super::*;
    use crate::db::create_tables;
    use chrono::Utc;

    /// 建真实 schema 的内存库，并按 (key, value) 写入一组设置
    async fn setup_with(settings: &[(&str, &str)]) -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("建内存库失败");
        create_tables(&pool).await.expect("建表失败");
        for (k, v) in settings {
            Setting::set(&pool, k, v).await.expect("写设置失败");
        }
        pool
    }

    /// 配置解析：缺项回退默认值，空白子目录等同未指定
    #[test]
    fn parse_config_falls_back_to_defaults() {
        let c = parse_report_config("", ReportKind::Daily);
        assert_eq!(c.channel, "app");
        assert_eq!(c.days, 1);
        assert_eq!(c.vault_subpath, None);

        // 非法 JSON 按空对象处理；days < 1 回退为该类型的默认窗口
        let c = parse_report_config(
            r#"{"channel":"both","days":0,"vault_subpath":"   "}"#,
            ReportKind::Weekly,
        );
        assert_eq!(c.channel, "both");
        assert_eq!(c.days, 7);
        assert_eq!(c.vault_subpath, None);
    }

    /// Vault 子目录优先级：config 指定 →「设置 → Obsidian」导出目录 → 兜底 ReadFlow
    #[tokio::test]
    async fn vault_dir_priority() {
        let pool = setup_with(&[
            ("obsidian_vault_path", "/vault"),
            ("obsidian_export_folder", "Notes"),
        ])
        .await;

        assert_eq!(
            resolve_vault_dir(&pool, None).await.unwrap(),
            PathBuf::from("/vault/Notes")
        );
        assert_eq!(
            resolve_vault_dir(&pool, Some("自定义")).await.unwrap(),
            PathBuf::from("/vault/自定义")
        );
        assert_eq!(
            resolve_vault_dir(&pool, Some("  ")).await.unwrap(),
            PathBuf::from("/vault/Notes"),
            "空白子目录应回退到导出目录"
        );

        // 导出目录为空（用户没配）→ 兜底默认名
        Setting::set(&pool, "obsidian_export_folder", "")
            .await
            .unwrap();
        assert_eq!(
            resolve_vault_dir(&pool, None).await.unwrap(),
            PathBuf::from("/vault/ReadFlow")
        );
    }

    /// Vault 未配置：报错以"未配置"开头，任务侧据此归为 Unconfigured 引导补配置
    #[tokio::test]
    async fn missing_vault_is_unconfigured() {
        let pool = setup_with(&[]).await;

        let err = deliver_to_obsidian(&pool, ReportKind::Daily, None, "x")
            .await
            .unwrap_err();
        assert!(err.starts_with("未配置"), "实际错误信息: {}", err);

        let outcome = run_report_task(&pool, r#"{"channel":"obsidian"}"#, ReportKind::Daily).await;
        assert!(matches!(outcome, TaskOutcome::Unconfigured(_)));
    }

    /// 双写通道：应用内目录与 Vault 各落一份同名报告，缺 Vault 配置时一份都不写
    #[tokio::test]
    async fn both_channel_writes_app_dir_and_vault() {
        let root = std::env::temp_dir().join(format!("readflow-report-{}", std::process::id()));
        let app_dir = root.join("reports");
        let vault = root.join("vault");
        std::fs::create_dir_all(&vault).expect("建 Vault 目录失败");
        // 报告目录是进程级全局：测试进程内首次设置即生效
        let _ = REPORTS_DIR.set(app_dir.clone());

        let pool = setup_with(&[
            ("obsidian_vault_path", vault.to_str().unwrap()),
            ("obsidian_export_folder", "ReadFlow"),
        ])
        .await;

        // 缺 Vault 配置时应"一份都不写"（先校验再落盘）：单开一个库验证
        let bare = setup_with(&[]).await;
        assert!(
            generate_report(&bare, r#"{"channel":"both","days":1}"#, ReportKind::Daily)
                .await
                .is_err(),
            "Vault 未配置时 both 通道应直接失败"
        );
        assert!(
            !app_dir.exists() || std::fs::read_dir(&app_dir).unwrap().count() == 0,
            "失败路径不应留下应用内报告"
        );

        let (file_name, message) =
            generate_report(&pool, r#"{"channel":"both","days":1}"#, ReportKind::Daily)
                .await
                .expect("双写失败");

        assert!(
            app_dir.join(&file_name).is_file(),
            "应用内报告未落盘: {}",
            app_dir.display()
        );
        assert!(
            vault.join("ReadFlow").join(&file_name).is_file(),
            "Vault 报告未落盘: {}",
            vault.display()
        );
        assert!(message.contains("Obsidian Vault"), "描述应含 Vault 落点: {}", message);

        std::fs::remove_dir_all(&root).ok();
    }

    /// 构造一条最小事件行（只填被测字段，其余取固定占位）
    fn sample_event(entity: &str) -> EventWithContext {
        EventWithContext {
            entity_name: entity.to_string(),
            id: 1,
            article_id: 7,
            entity_id: 3,
            event_type: "product".to_string(),
            event_date: "2026-10-05".to_string(),
            fact: "发布新一代加速卡".to_string(),
            evidence: String::new(),
            source_model: "agnes".to_string(),
            created_at: Utc::now(),
            article_title: None,
            article_link: None,
        }
    }

    /// 报告带 Obsidian frontmatter：六个字段齐备，且 type / tags 随报告类型变化
    #[test]
    fn report_has_obsidian_frontmatter() {
        let rows = [sample_event("英伟达")];
        let md = render_report(ReportKind::Daily, 1, &rows);

        assert!(
            md.starts_with("---\ntype: research-daily\n"),
            "首部应是 frontmatter: {}",
            md
        );
        assert!(md.contains("period: \"近 1 天\""), "{}", md);
        assert!(md.contains("events: 1"), "{}", md);
        assert!(md.contains("entities: 1"), "{}", md);
        assert!(md.contains("tags: [readflow, 研究日报]"), "{}", md);

        let weekly = render_report(ReportKind::Weekly, 7, &rows);
        assert!(weekly.contains("type: research-weekly"), "{}", weekly);
        assert!(weekly.contains("period: \"近 7 天\""), "{}", weekly);
        assert!(weekly.contains("tags: [readflow, 研究周报]"), "{}", weekly);
    }

    /// 实体标题用双链，使实体笔记的反链面板能聚合所有相关报告
    #[test]
    fn report_heads_entities_with_wikilinks() {
        let md = render_report(ReportKind::Daily, 1, &[sample_event("英伟达")]);
        assert!(md.contains("## [[英伟达]]"), "{}", md);
    }

    /// 含双链保留字符的实体名退化为纯文本，避免链接指向错误目标
    #[test]
    fn report_falls_back_for_unsafe_entity_names() {
        for name in ["A#B", "A|B", "A^B", "[A]"] {
            let md = render_report(ReportKind::Daily, 1, &[sample_event(name)]);
            assert!(
                md.contains(&format!("## {}\n", name)),
                "实体名 {} 应退化为纯文本: {}",
                name,
                md
            );
            assert!(!md.contains("[["), "实体名 {} 不应产生双链: {}", name, md);
        }
    }

    /// 证据渲染为 [!quote] callout，来源链接并入同一块作为出处；换行压成空格
    #[test]
    fn report_renders_evidence_as_quote_callout() {
        let mut row = sample_event("英伟达");
        row.evidence = "黄仁勋表示\n新卡将于年底量产".to_string();
        row.article_title = Some("英伟达发布新卡".to_string());
        row.article_link = Some("https://example.com/a".to_string());

        let md = render_report(ReportKind::Daily, 1, &[row]);

        assert!(
            md.contains("  > [!quote] 黄仁勋表示 新卡将于年底量产"),
            "证据应压平换行并置于 callout: {}",
            md
        );
        assert!(
            md.contains("  > — [英伟达发布新卡](https://example.com/a)"),
            "来源应并入引用块: {}",
            md
        );
    }

    /// 无事件时保留完整 frontmatter 与骨架，只把正文换成提示 callout
    #[test]
    fn report_without_events_keeps_frontmatter() {
        let md = render_report(ReportKind::Weekly, 7, &[]);

        assert!(md.starts_with("---\ntype: research-weekly\n"), "{}", md);
        assert!(md.contains("events: 0"), "{}", md);
        assert!(md.contains("entities: 0"), "{}", md);
        assert!(md.contains("# 研究周报 · "), "{}", md);
        assert!(md.contains("> [!info] 本期无动态"), "{}", md);
    }
}
