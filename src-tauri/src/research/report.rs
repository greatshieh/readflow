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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{Datelike, Local};
use sqlx::SqlitePool;

use crate::ai::{self, AIProvider};
use crate::db::{CustomEventType, EventWithContext, ResearchEvent, Setting};
use crate::util::{html_escape, sanitize_url};

use super::{TaskOutcome, extract::PRESET_DOMAINS};

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
    /// AI 提供商标识：用于生成 AI 叙述（综述/评论/建议），缺省 `agnes`，
    /// 与抽取链路（research/mod.rs 的 provider 解析）保持一致
    provider_id: String,
    /// 导出瞬间的界面主题快照（亮/暗 + 强调色十六进制值）。
    ///
    /// # 为什么报告需要它
    /// 报告是**离线单文件 HTML**，脱离 App 独立打开，无法读取运行时的
    /// `data-theme` / `data-accent` 属性。此前只能靠 `@media (prefers-color-scheme)`
    /// 猜测系统主题，于是「应用里选了亮色、系统是暗色」这类组合会渲染成另一套配色，
    /// 与用户导出时看到的界面不一致。把此刻的主题与主色写进 HTML 的 `:root`，
    /// 报告就与界面严格同源；缺失时仍回退媒体查询，故定时任务（无前端上下文）
    /// 生成的老路径行为不变。
    theme: Option<ThemeSnapshot>,
}

/// 报告 HTML 的主题快照
///
/// 由前端（`appearance` store）读出当前 `data-theme` 与强调色主色后注入。
/// 两个字段都允许缺失：任一缺失即整体回退到 `prefers-color-scheme` 兜底，
/// 避免半截主题导致报告配色错乱（比"不注入"更糟）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ThemeSnapshot {
    /// 主题模式：`light` / `dark`
    pub mode: String,
    /// 强调色主色的十六进制值（如 `#93a6c0`），写入 CSS 变量后即与界面同色
    pub accent: String,
}

impl ThemeSnapshot {
    /// 校验快照可用性：模式必须是 light/dark，且主色必须是安全的十六进制色值
    ///
    /// # 为什么校验
    /// 该值会被直接拼进 HTML 的 `:root` CSS。虽是本地生成的文件，但主色来自
    /// 前端拼装的 JSON——一旦被篡改成 `red; } body { display:none` 之类，
    /// 就会污染生成的样式表。故只接受 `#` + 3/6 位十六进制，长度与字符集双重限制。
    ///
    /// # 返回值
    /// `Some(snapshot)` 表示可用；任一条件不满足则返回 `None`（调用方回退媒体查询）
    fn validated(&self) -> Option<ThemeSnapshot> {
        let mode_ok = matches!(self.mode.as_str(), "light" | "dark");
        let accent_ok = {
            let a = self.accent.trim();
            let hex = a.strip_prefix('#').unwrap_or("");
            matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
        };
        if mode_ok && accent_ok {
            Some(self.clone())
        } else {
            None
        }
    }

    /// 展开成写进 HTML `<style>` 的 CSS 声明
    ///
    /// # 返回值
    /// 可直接插入 `:root { ... }` 的声明串（`--rp-mode` / `--rp-accent` 两行）；
    /// 无有效快照时返回空串，调用方据此保留 `prefers-color-scheme` 兜底分支。
    fn to_css_vars(&self) -> String {
        match self.validated() {
            Some(t) => format!(
                "--rp-mode: {};\n  --rp-accent: {};\n  --rp-bg: {};\n  --rp-surface: {};\n  --rp-text-1: {};\n  --rp-text-2: {};\n  --rp-border: {};",
                match t.mode.as_str() {
                    "dark" => "#16171a",
                    _ => "#f5f9ff",
                },
                t.accent.trim(),
                match t.mode.as_str() {
                    "dark" => "#16171a",
                    _ => "#f5f9ff",
                },
                match t.mode.as_str() {
                    "dark" => "#1e1f22",
                    _ => "#ffffff",
                },
                match t.mode.as_str() {
                    "dark" => "#e8eaed",
                    _ => "#191c22",
                },
                match t.mode.as_str() {
                    "dark" => "#c2c7cf",
                    _ => "#565d6b",
                },
                match t.mode.as_str() {
                    "dark" => "#35373c",
                    _ => "#e3e8ef",
                }
            ),
            None => String::new(),
        }
    }
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
        // AI 叙述的 provider：缺省 agnes（与抽取链路一致），未配置时叙述降级为占位
        provider_id: parsed
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("agnes")
            .to_string(),
        // 界面主题快照：可选。缺失或非法时为 None，HTML 回退 prefers-color-scheme
        theme: parsed
            .get("theme")
            .and_then(|v| serde_json::from_value::<ThemeSnapshot>(v.clone()).ok())
            .and_then(|t| t.validated()),
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

    // 聚合事件：窗口内全部事件（报告生成走全量，新筛选参数留空表示不过滤）
    let rows = ResearchEvent::list_with_context(pool, config.days, None, None, None)
        .await
        .map_err(|e| format!("读取事件失败: {}", e))?;

    // 情感聚合（按置信度加权）与事件类型中文标签，供 HTML 报告使用
    let agg = aggregate_sentiment(&rows);
    let labels = build_event_type_labels(pool).await;

    // AI 叙述（综述/评论/建议）：复用应用已配置的模型；provider 解析失败或 LLM
    // 调用失败均降级为 None，渲染时以占位卡替代，不阻断报告导出
    let narration = match ai::load_provider(pool, &config.provider_id).await {
        Ok(provider) => generate_narration(&provider, kind, config.days, &rows).await,
        Err(_) => None,
    };

    let markdown = render_report(kind, config.days, &rows);
    let html = render_report_html(
        kind,
        config.days,
        &rows,
        &agg,
        &labels,
        narration.as_ref(),
        config.theme.as_ref(),
    );

    // 按通道分发落盘（markdown 与 html 一并写出，Vault/应用目录各保留同名 sidecar）
    let path_desc = match config.channel.as_str() {
        "obsidian" => {
            deliver_to_obsidian(pool, kind, config.vault_subpath.as_deref(), &markdown, &html)
                .await?
        }
        // app 与 file 通道现阶段都写入应用数据目录的 reports/：
        // app 通道的"应用内查看"由报告查看器读取同一目录
        "app" | "file" => deliver_to_app_dir(kind, &markdown, &html)?,
        // 双写：先解析 Vault 目标目录（含配置校验）再落两处，避免 Vault 缺配置时
        // 留下"应用内已更新、Vault 没有"的不一致状态
        "both" => {
            let vault_dir = resolve_vault_dir(pool, config.vault_subpath.as_deref()).await?;
            let app_desc = deliver_to_app_dir(kind, &markdown, &html)?;
            let vault_path = write_report_to(&vault_dir, kind, &markdown, &html)?;
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

/// 预设事件类型 → 中文标签（与前端 `eventTypeLabels.ts` 的 `PRESET_EVENT_TYPES` 同源）
///
/// 仅作展示用途；自定义类型（custom_event_types 表）由用户在设置页录入，其 label 在
/// [`build_event_type_labels`] 中并入，故此处无需覆盖。
const PRESET_EVENT_TYPE_LABELS: &[(&str, &str)] = &[
    ("product", "产品"), ("executive", "高管"), ("ma", "并购"), ("regulatory", "监管"),
    ("strategy", "战略"), ("competition", "竞争"), ("industry_signal", "行业信号"),
    ("paper", "论文"), ("grant", "经费"), ("clinical", "临床"), ("conference", "会议"),
    ("hire", "人才"), ("collaboration", "合作"), ("patent", "专利"), ("dataset", "数据集"),
    ("funding", "融资"), ("legislation", "立法"), ("enforcement", "执法"),
    ("consultation", "征求意见"), ("guideline", "指南"),
    ("earnings", "财报"), ("rating", "评级"), ("ipo", "上市"), ("dividend", "分红"),
];

/// 情感聚合结果（按 sentiment_confidence 加权）
struct SentimentAgg {
    /// 利好加权分
    positive: f64,
    /// 利空加权分
    negative: f64,
    /// 中性加权分
    neutral: f64,
    /// 主导倾向：positive / negative / neutral
    dominant: String,
}

/// 按置信度加权聚合情感（修正此前未加权的 COUNT 口径）
///
/// 每条事件的情感权重取其 `sentiment_confidence`，neutral 亦计入权重但不影响多空方向。
/// 主导倾向由多空加权差占总量比例决定，差异过小时归为中性，避免被少数强置信事件带偏。
///
/// # 参数
/// * `rows` - 窗口内的事件行
///
/// # 返回值
/// 加权后的情感聚合
fn aggregate_sentiment(rows: &[EventWithContext]) -> SentimentAgg {
    let mut positive = 0.0_f64;
    let mut negative = 0.0_f64;
    let mut neutral = 0.0_f64;
    for r in rows {
        let w = r.sentiment_confidence.max(0.0);
        match r.sentiment.as_str() {
            "positive" => positive += w,
            "negative" => negative += w,
            _ => neutral += w,
        }
    }
    let total = positive + negative + neutral;
    let dominant = if total == 0.0 {
        "neutral".to_string()
    } else if positive > negative && (positive - negative) / total > 0.1 {
        "positive".to_string()
    } else if negative > positive && (negative - positive) / total > 0.1 {
        "negative".to_string()
    } else {
        "neutral".to_string()
    };
    SentimentAgg { positive, negative, neutral, dominant }
}

/// 情感计数（Markdown 摘要用，未加权）
///
/// # 参数
/// * `rows` - 窗口内的事件行
///
/// # 返回值
/// (利好数, 利空数, 中性数)
fn tally_sentiment(rows: &[EventWithContext]) -> (usize, usize, usize) {
    let mut pos = 0;
    let mut neg = 0;
    let mut neu = 0;
    for r in rows {
        match r.sentiment.as_str() {
            "positive" => pos += 1,
            "negative" => neg += 1,
            _ => neu += 1,
        }
    }
    (pos, neg, neu)
}

/// 构建事件类型 → 中文标签映射（预设标签 + 自定义类型查表）
///
/// 自定义类型来自 `custom_event_types` 表，缺省回退为其 value（与前端 `mergeEffectiveEventTypes`
/// 行为一致）；预设标签与前端同源，保证报告与界面展示统一。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回值
/// value → 中文标签 的映射
async fn build_event_type_labels(pool: &SqlitePool) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = PRESET_EVENT_TYPE_LABELS
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    if let Ok(custom) = CustomEventType::list(pool).await {
        for ct in custom {
            map.entry(ct.value).or_insert(ct.label);
        }
    }
    map
}

/// 由情感聚合推导风险等级（用于 HTML 报告顶部徽标）
///
/// # 参数
/// * `agg` - 加权情感聚合
///
/// # 返回值
/// (等级样式类 low/mid/high, 人类可读的风险描述)
fn risk_from_agg(agg: &SentimentAgg) -> (&'static str, String) {
    let total = agg.positive + agg.negative + agg.neutral;
    if total == 0.0 {
        return ("mid", "未知（无情感标注）".to_string());
    }
    let neg_ratio = agg.negative / total;
    let pct = (neg_ratio * 100.0).round() as i64;
    if neg_ratio >= 0.4 {
        ("high", format!("高（利空占比 {}%）", pct))
    } else if neg_ratio >= 0.2 {
        ("mid", format!("中（利空占比 {}%）", pct))
    } else {
        ("low", format!("低（利空占比 {}%）", pct))
    }
}

/// AI 生成的研究报告叙述（综述 / 评论 / 建议）
#[derive(Debug, Clone, serde::Deserialize)]
struct Narration {
    /// 综述：近期主要动态归纳
    summary: String,
    /// 评论：事件的影响解读
    commentary: String,
    /// 建议：应重点跟踪的信号与催化剂
    recommendations: String,
}

/// 调用已配置的 AI 模型，基于结构化事件生成叙述（综述/评论/建议）
///
/// 仅把压缩后的事件清单（id / 实体 / 类型 / 日期 / 事实 / 情感）喂给模型，
/// 并要求其引用事件 id 反幻觉。模型调用失败、超时或返回非 JSON 时返回 `None`，
/// 由调用方降级为占位卡，不阻断报告导出。
///
/// # 参数
/// * `provider` - 已装配的 AI 提供商配置
/// * `kind` - 报告类型（仅用于提示文案）
/// * `days` - 覆盖天数（仅用于提示文案）
/// * `rows` - 窗口内的事件行（超长历史截断到最近 200 条）
///
/// # 返回值
/// 解析成功的叙述；任何环节出错时返回 `None`
async fn generate_narration(
    provider: &AIProvider,
    _kind: ReportKind,
    days: i64,
    rows: &[EventWithContext],
) -> Option<Narration> {
    // 超长历史截断到最近 200 条，避免超出上下文；按日期倒序取最新
    let mut window: Vec<&EventWithContext> = rows.iter().collect();
    window.sort_by(|a, b| b.event_date.cmp(&a.event_date));
    window.truncate(200);

    let events: Vec<serde_json::Value> = window
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "entity": r.entity_name,
                "type": r.event_type,
                "date": if r.event_date.is_empty() { r.created_at.format("%Y-%m-%d").to_string() } else { r.event_date.clone() },
                "fact": r.fact,
                "sentiment": r.sentiment,
            })
        })
        .collect();

    let period = format!("近 {} 天", days);
    let prompt = format!(
        "你是一位资深行业研究员。以下是过去{}从 RSS 抽取的结构化事件（JSON 数组），每条含 id、实体、类型、日期、事实、情感（positive/negative/neutral）：\n{}\n\n请仅基于上述数据撰写研究报告的三段内容：\n1) summary（综述）：概述近期主要动态；\n2) commentary（评论）：这些事件意味着什么、对基本面或竞争格局的影响；\n3) recommendations（建议）：研究者应重点跟踪哪些信号与催化剂。\n要求：不得编造数据；可在文中用 [事件#id] 引用具体事件。\n输出严格 JSON，不要包含任何解释文字或 Markdown 代码块标记：{{\"summary\":\"...\",\"commentary\":\"...\",\"recommendations\":\"...\"}}",
        period,
        serde_json::to_string_pretty(&events).unwrap_or_default()
    );
    let raw = ai::chat_completion(provider, &prompt, Some(0.3)).await.ok()?;
    parse_narration(&raw)
}

/// 从模型原始输出中解析叙述 JSON（容错：剥离 ```json 围栏、截取首个 {…}）
///
/// # 参数
/// * `raw` - 模型原始返回文本
///
/// # 返回值
/// 解析成功的叙述；格式不符时返回 `None`
fn parse_narration(raw: &str) -> Option<Narration> {
    let trimmed = raw.trim();
    let json_str = match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(s), Some(e)) if e > s => &trimmed[s..=e],
        _ => return None,
    };
    serde_json::from_str::<Narration>(json_str).ok()
}

/// 自包含 HTML 研究报告的内联样式
///
/// 使用与应用一致的真实设计 token 名称（`--primary` / `--text-primary` 等，而非不存在的
/// `--accent` / `--text`）；附带 `@media (prefers-color-scheme: dark)` 深色配色与
/// `@media print` 打印优化，保证浅色/深色下可读、可导出 PDF。
const HTML_STYLE: &str = r#"
:root {
  --bg: #f5f6f8;
  --surface: #ffffff;
  --surface-2: #f7f8fa;
  --text-1: #1f2329;
  --text-2: #434a54;
  --text-3: #8a929e;
  --border: #e3e6ea;
  --primary: #3b6ef5;
  --up: #cf1322;            /* 利多 / 涨 —— 红（A 股惯例） */
  --up-bg: #fff1f0;
  --up-bd: #ffccc7;
  --down: #389e0d;          /* 利空 / 跌 —— 绿 */
  --down-bg: #f6ffed;
  --down-bd: #b7eb8f;
  --neutral: #6b7280;
  --neutral-bg: #f0f2f5;
  --neutral-bd: #d9dde3;
  --shadow: 0 2px 10px rgba(0,0,0,0.05);
  --radius: 14px;
  /* ── 科技风语汇（与实体仪表盘的 .tech-* 家族对齐）──────────────────
     以下四组由 render_report_html 在生成时注入**界面主题快照**
     （data-theme + 强调色主色）；未注入时保持空串，
     于是本段回落到下方 prefers-color-scheme 分支的取值——
     即「导出瞬间的界面配色」优先，系统主题仅作兜底。 */
  --rp-bg: var(--bg);
  --rp-surface: var(--surface);
  --rp-accent: var(--primary);
  --rp-text: var(--text-1);
  --rp-border: var(--border);
  --rp-shadow: var(--shadow);
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", "Hiragino Sans GB", sans-serif;
  background: var(--bg); color: var(--text-1); line-height: 1.6; padding: 24px;
}
.wrap { max-width: 920px; margin: 0 auto; }
/* Hero 头部 */
.hero {
  background: linear-gradient(135deg, #1f2329 0%, #2b3440 100%);
  color: #fff; border-radius: 16px; padding: 28px 32px; margin-bottom: 22px;
  box-shadow: 0 6px 24px rgba(0,0,0,0.12);
}
.hero h1 { font-size: 26px; font-weight: 700; letter-spacing: .5px; }
.hero .sub { font-size: 13px; color: #aeb6c0; margin-top: 6px; }
.hero .meta { display: flex; flex-wrap: wrap; gap: 10px; margin-top: 16px; }
.chip {
  background: rgba(255,255,255,0.10); border: 1px solid rgba(255,255,255,0.18);
  color: #e8edf3; font-size: 12px; padding: 5px 11px; border-radius: 20px;
}
.chip b { color: #ff7a45; }
/* 统计卡片 */
.stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; margin-bottom: 24px; }
.stat {
  background: var(--surface); border-radius: 12px; padding: 14px 16px;
  border-left: 4px solid var(--neutral); box-shadow: var(--shadow);
}
.stat .v { font-size: 20px; font-weight: 700; color: var(--neutral); }
.stat .l { font-size: 12px; color: var(--text-3); margin-top: 3px; }
.stat.total { border-left-color: var(--primary); }
.stat.total .v { color: var(--primary); }
.stat.up { border-left-color: var(--up); }
.stat.up .v { color: var(--up); }
.stat.down { border-left-color: var(--down); }
.stat.down .v { color: var(--down); }
.stat.neu { border-left-color: var(--neutral); }
.stat.neu .v { color: var(--neutral); }
/* 分节卡片 */
.sec { background: var(--surface); border-radius: var(--radius); padding: 22px 26px; margin-bottom: 20px; box-shadow: var(--shadow); }
.sec h2 { font-size: 19px; margin-bottom: 4px; display: flex; align-items: center; gap: 8px; }
.sec h2 .idx {
  background: var(--up); color: #fff; font-size: 13px; width: 26px; height: 26px;
  border-radius: 7px; display: inline-flex; align-items: center; justify-content: center;
}
.sec .note { font-size: 12px; color: var(--text-3); margin-bottom: 16px; }
/* AI 研究叙述 */
.narr { display: flex; flex-direction: column; gap: 14px; }
.narr-h { font-size: 13px; font-weight: 700; color: var(--text-2); margin-bottom: 4px; padding-left: 9px; border-left: 3px solid var(--primary); }
.prose { font-size: 14px; color: var(--text-1); white-space: pre-wrap; }
.placeholder {
  background: repeating-linear-gradient(45deg,#fafbfc,#fafbfc 10px,#f2f4f7 10px,#f2f4f7 20px);
  border: 1px dashed #c4cad3; border-radius: 10px; padding: 14px 16px; color: #8a929e; font-size: 13px; font-style: normal;
}
/* 时间线 */
.tl { position: relative; padding-left: 26px; }
.tl::before { content: ""; position: absolute; left: 7px; top: 4px; bottom: 4px; width: 2px; background: var(--border); }
.ev { position: relative; margin-bottom: 18px; }
.ev::before {
  content: ""; position: absolute; left: -23px; top: 5px; width: 12px; height: 12px;
  border-radius: 50%; background: var(--up); border: 2px solid var(--surface); box-shadow: 0 0 0 2px var(--up);
}
.ev.neg::before { background: var(--down); box-shadow: 0 0 0 2px var(--down); }
.ev.neu::before { background: var(--neutral); box-shadow: 0 0 0 2px var(--neutral); }
.ev .date { font-size: 12px; font-weight: 700; color: var(--text-1); }
.ev .tag { font-size: 11px; padding: 1px 7px; border-radius: 4px; margin-left: 8px; vertical-align: middle; }
.tag.up { background: var(--up-bg); color: var(--up); border: 1px solid var(--up-bd); }
.tag.down { background: var(--down-bg); color: var(--down); border: 1px solid var(--down-bd); }
.tag.neu { background: var(--neutral-bg); color: var(--neutral); border: 1px solid var(--neutral-bd); }
.ev .badge { font-size: 11px; padding: 1px 7px; border-radius: 4px; margin-left: 8px; vertical-align: middle; background: var(--surface-2); color: var(--text-2); border: 1px solid var(--border); }
.ev .ent { font-size: 12px; color: var(--text-3); margin: 4px 0 2px; }
.ev .desc { font-size: 13.5px; color: var(--text-2); }
.ev .evidence { font-size: 12px; color: var(--text-3); margin-top: 3px; }
.ev .src { font-size: 11.5px; color: var(--text-3); margin-top: 5px; }
.ev .src a { color: var(--primary); text-decoration: none; }
.ev .src a:hover { text-decoration: underline; }
/* 按类型分组 */
.type-group { margin-bottom: 14px; }
/* 分组序号：等宽字体 + 主色描边，对齐仪表盘的 .tech-index 语汇 */
.type-head .idx {
  font-size: 11px; color: var(--rp-accent); border: 1px solid var(--rp-accent);
  border-radius: 6px; padding: 1px 7px; font-variant-numeric: tabular-nums;
}
.type-head { font-size: 15px; margin: 14px 0 6px; color: var(--text-1); display: flex; align-items: center; gap: 8px; }
/* 计数徽标：贴行尾。flex-shrink:0 + nowrap 防止末位被压缩折行 */
.type-head .badge { background: var(--surface-2); color: var(--text-2); border: 1px solid var(--border); font-size: 11px; padding: 1px 7px; border-radius: 999px; margin-left: auto; flex-shrink: 0; white-space: nowrap; }
.type-list { margin: 0; padding-left: 18px; }
.type-list li { margin: 4px 0; font-size: 13px; color: var(--text-2); }
.type-list li a { color: var(--rp-accent); text-decoration: none; }
/* 事件卡片：左侧强调竖条（仪表盘 .tech-card::before 的同构语汇） */
.ev-card { position: relative; overflow: hidden; border-left: 3px solid var(--rp-accent); padding-left: 12px; }
.muted { color: var(--text-3); font-size: 13px; }
/* 风险提示 + 页脚 */
.disc { font-size: 11.5px; color: var(--text-3); border-top: 1px solid var(--border); padding-top: 14px; margin-top: 8px; line-height: 1.7; }
.disc b { color: var(--text-2); }
.footer { text-align: center; font-size: 11px; color: #aab0b8; margin-top: 10px; }
/* 深色模式 */
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #16171a;
    --surface: #1e1f22;
    --surface-2: #26282c;
    --text-1: #e8eaed;
    --text-2: #c2c7cf;
    --text-3: #8a929e;
    --border: #35373c;
    --primary: #6b9bff;
    --up: #ff6b5e;
    --up-bg: rgba(255,107,94,0.14);
    --up-bd: rgba(255,107,94,0.40);
    --down: #5fd07a;
    --down-bg: rgba(95,208,122,0.14);
    --down-bd: rgba(95,208,122,0.40);
    --neutral: #9aa1ac;
    --neutral-bg: rgba(154,161,172,0.12);
    --neutral-bd: rgba(154,161,172,0.35);
    --shadow: 0 2px 10px rgba(0,0,0,0.35);
  }
  body { background: #16171a; }
  .hero { background: linear-gradient(135deg, #0f1216 0%, #1c222b 100%); }
  .placeholder { background: repeating-linear-gradient(45deg,#222426,#222426 10px,#1b1d1f 10px,#1b1d1f 20px); border-color: #3a3d42; color: #9aa1ac; }
  .chip b { color: #ffa070; }
}
/* 打印优化 */
@media print {
  body { background: #fff; padding: 0; }
  .wrap { max-width: none; }
  .hero, .sec, .stat { box-shadow: none; }
  .hero { -webkit-print-color-adjust: exact; print-color-adjust: exact; }
}
"#;

/// 渲染自包含 HTML 研究报告
///
/// 与 [`render_report`]（Markdown）并列输出：在 Obsidian / 应用目录各落一份同名
/// `.html` sidecar。HTML 为单文件自包含（内联 CSS），所有动态文本经 [`html_escape`]、
/// 来源 URL 经 [`sanitize_url`]。当 `narration` 为 `None`（AI 不可用 / 调用失败）时，
/// 综述/评论/建议三栏渲染占位卡，不阻断报告生成。
///
/// # 参数
/// * `kind` - 报告类型
/// * `days` - 覆盖天数
/// * `rows` - 窗口内的事件行
/// * `agg` - 加权情感聚合
/// * `labels` - 事件类型 → 中文标签映射
/// * `narration` - AI 叙述（可能为 `None`）
///
/// # 返回值
/// 完整 HTML 文本
fn render_report_html(
    kind: ReportKind,
    days: i64,
    rows: &[EventWithContext],
    agg: &SentimentAgg,
    labels: &HashMap<String, String>,
    narration: Option<&Narration>,
    theme: Option<&ThemeSnapshot>,
) -> String {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let period = format!("近 {} 天", days);
    let entities = count_entities(rows);

    // 标签解析：优先用调用方传入的映射（含自定义类型），缺失时回退到内置预设标签，
    // 再不行才原样返回 value；保证即便 labels 为空，预设事件类型也始终显示中文。
    let label_of = |v: &str| -> String {
        if let Some(l) = labels.get(v) {
            return l.clone();
        }
        PRESET_EVENT_TYPE_LABELS
            .iter()
            .find(|(k, _)| *k == v)
            .map(|(_, l)| (*l).to_string())
            .unwrap_or_else(|| v.to_string())
    };

    let (_, risk_label) = risk_from_agg(agg);
    let sentiment_word = match agg.dominant.as_str() {
        "positive" => "偏多",
        "negative" => "偏空",
        _ => "中性",
    };

    // 时间线：按事件日期倒序（空日期回退入库日期，已在排序前规整）
    let mut timeline: Vec<&EventWithContext> = rows.iter().collect();
    timeline.sort_by(|a, b| {
        let da = if a.event_date.is_empty() {
            a.created_at.format("%Y-%m-%d").to_string()
        } else {
            a.event_date.clone()
        };
        let db = if b.event_date.is_empty() {
            b.created_at.format("%Y-%m-%d").to_string()
        } else {
            b.event_date.clone()
        };
        db.cmp(&da)
    });

    // 按类型分组（稳定顺序：按中文标签排序）
    let mut by_type: HashMap<String, Vec<&EventWithContext>> = HashMap::new();
    for r in rows {
        by_type.entry(r.event_type.clone()).or_default().push(r);
    }
    let mut type_keys: Vec<String> = by_type.keys().cloned().collect();
    type_keys.sort_by(|a, b| label_of(a).cmp(&label_of(b)));

    // 叙述 HTML（占位或内容）
    let (summary_html, commentary_html, recommendations_html) = match narration {
        Some(n) => (
            html_escape(&n.summary),
            html_escape(&n.commentary),
            html_escape(&n.recommendations),
        ),
        None => (
            "<p class=\"placeholder\">本报告的综述由 AI 生成，当前模型不可用，已留空。</p>".to_string(),
            "<p class=\"placeholder\">本报告的评论由 AI 生成，当前模型不可用，已留空。</p>".to_string(),
            "<p class=\"placeholder\">本报告的建议由 AI 生成，当前模型不可用，已留空。</p>".to_string(),
        ),
    };

    let mut s = String::new();
    s.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    s.push_str(&format!(
        "<title>{} · {}</title>\n",
        html_escape(&format!("研究{}", kind.label())),
        html_escape(&today)
    ));
    s.push_str("<style>\n");
    s.push_str(HTML_STYLE);
    // 主题快照注入：放在 HTML_STYLE **之后**，用 `:root` 同优先级覆盖其默认取值，
    // 于是界面配色（而非系统配色）胜出；未注入时不写任何声明，兜底分支自然生效。
    if let Some(theme) = theme {
        let vars = theme.to_css_vars();
        if !vars.is_empty() {
            s.push_str(&format!("\n:root {{\n  {}\n}}\n", vars));
        }
    }
    s.push_str("</style>\n</head>\n<body>\n<div class=\"wrap\">\n");

    // Hero 头部
    s.push_str(&format!(
        "<div class=\"hero\">\n<h1>研究{} · {}</h1>\n<div class=\"sub\">覆盖{} · {} 条事件 · {} 个实体</div>\n<div class=\"meta\">\n",
        html_escape(kind.label()),
        html_escape(&today),
        html_escape(&period),
        rows.len(),
        entities
    ));
    s.push_str(&format!(
        "<span class=\"chip\">情感倾向：<b>{}</b></span>\n",
        html_escape(sentiment_word)
    ));
    s.push_str(&format!(
        "<span class=\"chip\">风险等级：<b>{}</b></span>\n",
        html_escape(&risk_label)
    ));
    s.push_str(&format!(
        "<span class=\"chip\">生成于 <b>{}</b></span>\n",
        html_escape(&today)
    ));
    s.push_str("</div>\n</div>\n");

    // 统计卡片
    let (pos, neg, neu) = tally_sentiment(rows);
    s.push_str("<div class=\"stats\">\n");
    s.push_str(&format!(
        "<div class=\"stat total\"><div class=\"v\">{}</div><div class=\"l\">事件总数</div></div>\n",
        rows.len()
    ));
    s.push_str(&format!(
        "<div class=\"stat up\"><div class=\"v\">{}</div><div class=\"l\">利好（利多）</div></div>\n",
        pos
    ));
    s.push_str(&format!(
        "<div class=\"stat down\"><div class=\"v\">{}</div><div class=\"l\">利空（看跌）</div></div>\n",
        neg
    ));
    s.push_str(&format!(
        "<div class=\"stat neu\"><div class=\"v\">{}</div><div class=\"l\">中性</div></div>\n",
        neu
    ));
    s.push_str("</div>\n");

    // 研究叙述（综述 / 评论 / 建议）
    s.push_str("<div class=\"sec\">\n");
    s.push_str("<h2><span class=\"idx\">01</span> AI 研究叙述</h2>\n");
    s.push_str("<div class=\"note\">由应用已配置模型基于抽取的结构化事件生成；模型不可用或调用失败时以下三栏留空占位。</div>\n");
    s.push_str("<div class=\"narr\">\n");
    s.push_str(&format!(
        "<div class=\"narr-block\"><div class=\"narr-h\">综述</div><div class=\"prose\">{}</div></div>\n",
        summary_html
    ));
    s.push_str(&format!(
        "<div class=\"narr-block\"><div class=\"narr-h\">评论</div><div class=\"prose\">{}</div></div>\n",
        commentary_html
    ));
    s.push_str(&format!(
        "<div class=\"narr-block\"><div class=\"narr-h\">建议</div><div class=\"prose\">{}</div></div>\n",
        recommendations_html
    ));
    s.push_str("</div>\n</div>\n");

    // 时间线视图
    s.push_str("<div class=\"sec\">\n");
    s.push_str("<h2><span class=\"idx\">02</span> 时间线</h2>\n");
    s.push_str("<div class=\"note\">按事件日期倒序；配色：<span style=\"color:var(--up)\">红 = 利多 / 看涨</span>，<span style=\"color:var(--down)\">绿 = 利空 / 看跌</span>。</div>\n");
    if timeline.is_empty() {
        s.push_str("<p class=\"muted\">本期无事件。</p>\n");
    } else {
        s.push_str("<div class=\"tl\">\n");
        for r in &timeline {
            let date = if r.event_date.is_empty() {
                r.created_at.format("%Y-%m-%d").to_string()
            } else {
                r.event_date.clone()
            };
            let badge_label = label_of(&r.event_type);
            let (dot_class, tag_class, sent_label) = match r.sentiment.as_str() {
                "positive" => ("", "up", "利多"),
                "negative" => ("neg", "down", "利空"),
                _ => ("neu", "neu", "中性"),
            };
            let ev_class = if dot_class.is_empty() {
                "ev".to_string()
            } else {
                format!("ev {}", dot_class)
            };
            let fact = html_escape(&r.fact);
            let evidence = if r.evidence.is_empty() {
                String::new()
            } else {
                format!("<div class=\"evidence\">证据：{}</div>", html_escape(&r.evidence))
            };
            let source = match (&r.article_title, &r.article_link) {
                (Some(t), Some(l)) if !l.is_empty() => format!(
                    "<div class=\"src\">来源：<a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a></div>",
                    sanitize_url(l),
                    html_escape(t)
                ),
                _ => String::new(),
            };
            s.push_str(&format!(
                "<div class=\"{}\"><div class=\"date\">{}<span class=\"tag {}\">{}</span><span class=\"badge type-{}\">{}</span></div><div class=\"ent\">{}</div><div class=\"desc\">{}</div>{}{}</div>\n",
                ev_class,
                html_escape(&date),
                tag_class,
                sent_label,
                html_escape(&r.event_type),
                html_escape(&badge_label),
                html_escape(&r.entity_name),
                fact,
                evidence,
                source
            ));
        }
        s.push_str("</div>\n");
    }
    s.push_str("</div>\n");

    // 按类型视图
    s.push_str("<div class=\"sec\">\n");
    s.push_str("<h2><span class=\"idx\">03</span> 按类型</h2>\n");
    if type_keys.is_empty() {
        s.push_str("<p class=\"muted\">本期无事件。</p>\n");
    } else {
        for (ti, tk) in type_keys.iter().enumerate() {
            let label = label_of(tk);
            // 分组序号（两位补零）+ 右侧计数徽标：对齐实体仪表盘的 .tech-index / .tech-count 语汇。
            // 序号是位置锚点而非类型编码，故用 enumerate 的序号而非 tk 派生。
            s.push_str(&format!(
                "<div class=\"type-group\"><h3 class=\"type-head type-{}\"><span class=\"idx\">{:02}</span><span>{}</span><span class=\"badge\">{} 条</span></h3>\n<ul class=\"type-list\">\n",
                html_escape(tk),
                ti + 1,
                html_escape(&label),
                by_type[tk].len()
            ));
            let mut items = by_type[tk].clone();
            items.sort_by(|a, b| b.event_date.cmp(&a.event_date));
            for r in &items {
                let fact = html_escape(&r.fact);
                let date = if r.event_date.is_empty() {
                    r.created_at.format("%Y-%m-%d").to_string()
                } else {
                    r.event_date.clone()
                };
                let source = match (&r.article_title, &r.article_link) {
                    (Some(t), Some(l)) if !l.is_empty() => format!(
                        "<a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a>",
                        sanitize_url(l),
                        html_escape(t)
                    ),
                    _ => "（无来源链接）".to_string(),
                };
                s.push_str(&format!(
                    "<li>{} · {} — {}</li>\n",
                    html_escape(&date),
                    source,
                    fact
                ));
            }
            s.push_str("</ul></div>\n");
        }
    }
    s.push_str("</div>\n");

    // 风险提示 + 页脚
    s.push_str("<div class=\"disc\"><b>风险提示：</b>本内容由 AI 基于 RSS 抽取的结构化事件自动生成，可能包含延迟、缺失或口径差异；所列情感倾向与风险等级仅供研究参考，不构成投资建议或收益承诺。请独立、审慎决策并核对原始来源。</div>\n");
    s.push_str(&format!(
        "<div class=\"footer\">单文件 HTML · 全部样式内联 · 无外部依赖 · 生成于 {}</div>\n",
        html_escape(&today)
    ));

    s.push_str("</div>\n</body>\n</html>\n");
    s
}

/// 渲染报告 Markdown（Obsidian 惯例格式）
///
/// 结构：YAML frontmatter（type / date / period / events / entities / tags）
/// → 一级标题 → `[!summary]` 统计 callout（含情感倾向统计）→ 按实体分组的二级标题（双链）+
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
    let today = Local::now().format("%Y-%m-%d").to_string();
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
    let tally = tally_sentiment(rows);
    md.push_str(&format!(
        "> [!summary] 覆盖{}\n> 共 **{}** 条事件 · **{}** 个实体\n> 情感倾向：利好 {} · 利空 {} · 中性 {}\n",
        period,
        rows.len(),
        entities,
        tally.0, tally.1, tally.2
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

/// 报告文件基名（不含扩展名，markdown 与 html 共用，保证同目录并列）
fn report_basename(kind: ReportKind) -> String {
    let now = Local::now();
    match kind {
        ReportKind::Daily => format!("研究日报-{}", now.format("%Y-%m-%d")),
        ReportKind::Weekly => {
            // ISO 周数命名，便于跨年归档排序
            format!("研究周报-{}-W{:02}", now.year(), now.iso_week().week())
        }
    }
}

/// 生成报告文件名（markdown 版，html 版本由 [`report_basename`] 加 .html 派生）
fn report_filename(kind: ReportKind) -> String {
    format!("{}.md", report_basename(kind))
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
fn deliver_to_app_dir(kind: ReportKind, markdown: &str, html: &str) -> Result<String, String> {
    let dir = REPORTS_DIR
        .get()
        .ok_or("报告目录未初始化，请重启应用")?;
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建报告目录失败: {}", e))?;
    }
    let base = report_basename(kind);
    let md_path = dir.join(format!("{}.md", base));
    let html_path = dir.join(format!("{}.html", base));
    std::fs::write(&md_path, markdown).map_err(|e| format!("写入报告文件失败: {}", e))?;
    std::fs::write(&html_path, html).map_err(|e| format!("写入 HTML 报告失败: {}", e))?;
    Ok(format!("已写入 {} 与同名 HTML 报告", md_path.display()))
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
fn write_report_to(dir: &Path, kind: ReportKind, markdown: &str, html: &str) -> Result<PathBuf, String> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建报告目录失败: {}", e))?;
    }
    let base = report_basename(kind);
    let md_path = dir.join(format!("{}.md", base));
    let html_path = dir.join(format!("{}.html", base));
    std::fs::write(&md_path, markdown).map_err(|e| format!("写入报告文件失败: {}", e))?;
    std::fs::write(&html_path, html).map_err(|e| format!("写入 HTML 报告失败: {}", e))?;
    Ok(md_path)
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
    html: &str,
) -> Result<String, String> {
    let dir = resolve_vault_dir(pool, subpath).await?;
    let path = write_report_to(&dir, kind, markdown, html)?;
    Ok(format!("已写入 Obsidian Vault: {}", path.display()))
}

/// 预设领域枚举的导出（供 Phase C 前端筛选 UI 对齐文案）
///
/// 当前仅引用以避免 dead_code 警告；预设领域的类型清单由前端 `eventTypeLabels.ts` 同源维护。
const _: &[(&str, &[&str])] = PRESET_DOMAINS;

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

        let err = deliver_to_obsidian(&pool, ReportKind::Daily, None, "x", "")
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
            sentiment: "neutral".to_string(),
            sentiment_confidence: 0.0,
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

    /// HTML 报告：叙述为 None 时三栏渲染占位卡，且结构完整
    #[test]
    fn render_report_html_placeholder_when_narration_none() {
        let rows = [sample_event("英伟达")];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let html = render_report_html(ReportKind::Daily, 1, &rows, &agg, &labels, None, None);
        assert!(html.contains("<!DOCTYPE html>"), "应是完整 HTML");
        assert!(html.contains("综述"), "应有综述章节");
        assert!(html.contains("评论"), "应有评论章节");
        assert!(html.contains("建议"), "应有建议章节");
        assert!(html.contains("当前模型不可用，已留空"), "叙述缺失时应占位");
        assert!(html.contains("时间线"), "应有时间线视图");
        assert!(html.contains("按类型"), "应有类型视图");
    }

    /// HTML 报告：叙述存在时三栏含真实内容
    #[test]
    fn render_report_html_has_sections_when_narration_present() {
        let rows = [sample_event("英伟达")];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let narration = Narration {
            summary: "近期动态汇总".to_string(),
            commentary: "对竞争格局有影响".to_string(),
            recommendations: "关注评级变化".to_string(),
        };
        let html = render_report_html(ReportKind::Daily, 1, &rows, &agg, &labels, Some(&narration), None);
        assert!(html.contains("近期动态汇总"), "综述内容应出现");
        assert!(html.contains("对竞争格局有影响"), "评论内容应出现");
        assert!(html.contains("关注评级变化"), "建议内容应出现");
    }

    /// HTML 报告：动态字段必须转义，防 XSS
    #[test]
    fn render_report_html_escapes_evidence_and_source() {
        let mut row = sample_event("英伟达");
        row.fact = "<script>alert(1)</script>".to_string();
        row.evidence = "数据来自 <img src=x onerror=alert(1)>".to_string();
        row.article_title = Some("某媒体报道".to_string());
        row.article_link = Some("javascript:alert(1)".to_string());
        let rows = [row];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let html = render_report_html(ReportKind::Daily, 1, &rows, &agg, &labels, None, None);
        assert!(!html.contains("<script>alert(1)</script>"), "事实不应原样注入");
        assert!(!html.contains("<img src=x"), "证据不应原样注入");
        assert!(html.contains("&lt;script&gt;"), "事实应被转义");
        assert!(html.contains("href=\"#\""), "危险 scheme 应被替换为 #");
    }

    /// HTML 报告：时间线按日期倒序、类型视图按类型分组
    #[test]
    fn render_report_html_groups_by_timeline_and_type() {
        let mut a = sample_event("英伟达");
        a.event_date = "2026-10-01".to_string();
        a.event_type = "product".to_string();
        let mut b = sample_event("AMD");
        b.event_date = "2026-10-05".to_string();
        b.event_type = "rating".to_string();
        let rows = [a, b];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let html = render_report_html(ReportKind::Daily, 7, &rows, &agg, &labels, None, None);
        // 时间线中较新的 10-05 应出现在 10-01 之前（倒序）
        let pos_new = html.find("2026-10-05").unwrap();
        let pos_old = html.find("2026-10-01").unwrap();
        assert!(pos_new < pos_old, "时间线应按日期倒序");
        assert!(html.contains("rating"), "类型视图应出现事件类型");
    }

    /// HTML 报告：finance 事件使用中文标签（ipo → 上市）
    #[test]
    fn render_report_html_finance_event_has_chinese_label() {
        let mut row = sample_event("某公司");
        row.event_type = "ipo".to_string();
        let rows = [row];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let html = render_report_html(ReportKind::Daily, 1, &rows, &agg, &labels, None, None);
        assert!(html.contains("上市"), "ipo 应显示中文标签「上市」");
        assert!(html.contains("type-ipo"), "ipo 应有专属语义配色类");
    }

    /// 主题快照：合法输入应注入到 HTML 的 :root，覆盖系统主题兜底
    #[test]
    fn theme_snapshot_injected_into_html_root() {
        let rows = [sample_event("英伟达")];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let theme = ThemeSnapshot {
            mode: "dark".to_string(),
            accent: "#ff8800".to_string(),
        };
        let html = render_report_html(
            ReportKind::Daily,
            1,
            &rows,
            &agg,
            &labels,
            None,
            Some(&theme),
        );
        assert!(html.contains("--rp-accent: #ff8800"), "强调色应写入 CSS 变量");
        assert!(html.contains("--rp-bg: #16171a"), "暗色底应写入 CSS 变量");
    }

    /// 主题快照：非法强调色（CSS 注入尝试）必须被拒绝，回退系统主题兜底
    ///
    /// 主色会被直接拼进 HTML 的 `<style>`，若不校验，一个形如
    /// `red; } body { display:none` 的值就能污染整个样式表。
    #[test]
    fn theme_snapshot_rejects_malicious_accent() {
        let rows = [sample_event("英伟达")];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let theme = ThemeSnapshot {
            mode: "dark".to_string(),
            accent: "red; } body { display:none".to_string(),
        };
        let html = render_report_html(
            ReportKind::Daily,
            1,
            &rows,
            &agg,
            &labels,
            None,
            Some(&theme),
        );
        assert!(
            !html.contains("display:none"),
            "非法强调色不得进入样式表（CSS 注入）"
        );
        // 断言的是"注入值未被写入"，而非 `--rp-accent` 变量名不存在——
        // HTML_STYLE 的 :root 里本就有 `--rp-accent: var(--primary)` 作为兜底声明，
        // 它在注入路径下保持不变正是期望行为（回退到系统主题）。
        assert!(
            !html.contains("--rp-accent: red"),
            "非法强调色不得被写为 CSS 变量值"
        );
    }

    /// 主题快照：合法 3 位十六进制缩写应被接受
    #[test]
    fn theme_snapshot_accepts_short_hex() {
        let rows = [sample_event("英伟达")];
        let agg = aggregate_sentiment(&rows);
        let labels = HashMap::new();
        let theme = ThemeSnapshot {
            mode: "light".to_string(),
            accent: "#abc".to_string(),
        };
        let html = render_report_html(
            ReportKind::Daily,
            1,
            &rows,
            &agg,
            &labels,
            None,
            Some(&theme),
        );
        assert!(html.contains("--rp-accent: #abc"), "3 位缩写色应被接受");
    }

    /// 配置解析：theme 字段缺失时为 None（定时任务路径保持原行为）
    #[test]
    fn parse_report_config_theme_defaults_none() {
        let cfg = parse_report_config(r#"{"channel":"app","days":3}"#, ReportKind::Daily);
        assert!(cfg.theme.is_none(), "未提供 theme 时应为 None");
        assert_eq!(cfg.days, 3);
    }

    /// 配置解析：合法 theme 被接收
    #[test]
    fn parse_report_config_theme_parsed() {
        let json = r##"{"channel":"app","theme":{"mode":"dark","accent":"#93a6c0"}}"##;
        let cfg = parse_report_config(json, ReportKind::Daily);
        let t = cfg.theme.expect("合法 theme 应被解析");
        assert_eq!(t.mode, "dark");
        assert_eq!(t.accent, "#93a6c0");
    }

    /// 配置解析：非法 theme（模式不合法）应被丢弃而非让整个配置失败
    #[test]
    fn parse_report_config_rejects_invalid_theme_mode() {
        let json = r##"{"channel":"app","days":5,"theme":{"mode":"rainbow","accent":"#93a6c0"}}"##;
        let cfg = parse_report_config(json, ReportKind::Daily);
        assert!(cfg.theme.is_none(), "非法模式应被拒绝");
        assert_eq!(cfg.days, 5, "拒绝主题不应影响其它字段");
    }

    /// 叙述生成失败（非法 provider 导致 LLM 调用报错）时降级为 None，不抛错
    #[tokio::test]
    async fn generate_narration_returns_none_on_llm_error() {
        let bad = AIProvider {
            api_url: "http://127.0.0.1:0/none".to_string(),
            api_key: "x".to_string(),
            model: "m".to_string(),
        };
        let rows = [sample_event("英伟达")];
        let got = generate_narration(&bad, ReportKind::Daily, 1, &rows).await;
        assert!(got.is_none(), "LLM 调用失败应降级为 None 而非 panic");
    }

    /// obsidian 通道：Vault 内同时落 .md 与 .html 两份 sidecar
    #[tokio::test]
    async fn obsidian_channel_writes_html_sidecar() {
        let root = std::env::temp_dir().join(format!("readflow-html-{}", std::process::id()));
        let vault = root.join("vault");
        std::fs::create_dir_all(&vault).expect("建 Vault 目录失败");

        let pool = setup_with(&[
            ("obsidian_vault_path", vault.to_str().unwrap()),
            ("obsidian_export_folder", "ReadFlow"),
        ])
        .await;

        let (file_name, _) =
            generate_report(&pool, r#"{"channel":"obsidian","days":1}"#, ReportKind::Daily)
                .await
                .expect("obsidian 落盘失败");

        let base = file_name.trim_end_matches(".md");
        let dir = vault.join("ReadFlow");
        assert!(dir.join(format!("{}.md", base)).is_file(), "Vault Markdown 未落盘");
        assert!(dir.join(format!("{}.html", base)).is_file(), "Vault HTML 未落盘");
        std::fs::remove_dir_all(&root).ok();
    }
}
