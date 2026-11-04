//! 周期报告：按自然周期（日 / 周 / 月）汇总窗口内**全部文章**，交模型归纳后分节渲染
//!
//! # 与 [`super::report`] 的分工
//! `report.rs` 是**事件驱动**的研究报告（按实体分节、数据源是 `research_events`、
//! 生成期落盘到 `reports/` 或 Obsidian）。本模块是**时间驱动**的周期报告：
//! 不区分研究实体、数据源是 `articles`、正文只入库（导出时才写文件）。
//! 两条链路互不调用，唯一共享的是 AI 提供商装配（[`crate::ai::load_provider`]）。
//!
//! # 为什么正文必须入库
//! 旧报告是"纯文件"，列表靠解析文件名，元信息只有 size / mtime。周期报告需要
//! 「篇数 / 主题数 / 源数 / 引用源 → article_id」这些结构化字段，其中
//! **引用源要能点回原文**，只能靠 `article_id`，文件里的文字做不到。
//!
//! # 一次调用而不是分批
//! 每篇只喂「标题 + 摘要前 200 字」，窗口内最多 150 篇，约 30–60 秒一次调用完成。
//! 分批两阶段质量更好，但 2–4 分钟的等待会毁掉日报的使用场景（决策⑫）。
//!
//! # 参考
//! 交互与数据模型的定稿设计记录：本地 `deliverables/design/report-center/README.md` §5 / §6
//! （该目录不随仓库分发，clone 后不存在属正常现象）。

use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::ai;

use super::TaskOutcome;

/// 单份报告纳入的文章上限
///
/// 超出时按 `published_at` 倒序截断（保最近的），并在正文注明「已纳入 N / 共 M 篇」。
/// 上限的作用是给上下文长度兜底，不是给"重要性排序"留接口——`entity_score` 默认
/// NULL（只有跑过批量提取才有值），按它排序等于随机取样。
pub const MAX_ARTICLES: usize = 150;

/// 单篇摘要送入模型的最大字符数
const SUMMARY_CHARS: usize = 200;

/// 摘要为空时退回正文纯文本的最大字符数
const CONTENT_FALLBACK_CHARS: usize = 600;

/// 入库正文的硬上限（字符）
///
/// prompt 里写"别超过"是拦不住的，必须在代码里硬截。留足余量给节标题与引用源清单。
const MAX_MARKDOWN_CHARS: usize = 60_000;

/// 要点条数上限（决策⑦：3–7 条段落块）
const MAX_HIGHLIGHTS: usize = 7;

/// 主题节数上限（决策③：3–7 个主题）
const MAX_TOPICS: usize = 7;

/// 单条要点的字符上限（超出硬截）
const MAX_HIGHLIGHT_CHARS: usize = 200;

/// 单个主题摘要的字符上限
const MAX_TOPIC_SUMMARY_CHARS: usize = 600;

/// 单个关键事实的字符上限
const MAX_FACT_CHARS: usize = 60;

/// 每个主题最多保留的关键事实条数
const MAX_FACTS: usize = 5;

/// 「本期值得跟进」的条数上限
///
/// 卡在 5 而不是 10，是刻意的：跟进清单的作用是**收敛注意力**。
/// 一天列十个"值得关注"，等于什么都没说；三项以内才会真的被打开看。
const MAX_FOLLOWUPS: usize = 5;

/// 单条跟进理由的字符上限（一句话说清"变化"）
const MAX_FOLLOWUP_REASON_CHARS: usize = 120;

/// 默认 AI 提供商标识（与 `extract` / `report` 的缺省一致；设置页目前只维护这一个）
const DEFAULT_PROVIDER: &str = "agnes";

/// 生成进度回调：`(阶段标识, 已完成, 总数)`
///
/// 前端只渲染后端报上来的进度，不做假进度条——假进度在 30 秒后一定与真实进度错位。
/// 总数未知时传 0。
pub type ProgressSink<'a> = &'a (dyn Fn(&str, usize, usize) + Send + Sync);

// ─── 周期类型与窗口 ────────────────────────────────────────────────────────────

/// 报告周期粒度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodKind {
    /// 日报：自然窗口 1 天
    Daily,
    /// 周报：自然窗口 1 周（周一 00:00 起）
    Weekly,
    /// 月报：自然窗口 1 个自然月（1 日 00:00 起）
    Monthly,
}

impl PeriodKind {
    /// 从外部字符串解析（大小写与首尾空白不敏感）
    ///
    /// 解析失败返回 `None` 而不是回退默认值：用户选了个不存在的粒度时报错，
    /// 比静默产出一份日报要好。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "daily" => Some(PeriodKind::Daily),
            "weekly" => Some(PeriodKind::Weekly),
            "monthly" => Some(PeriodKind::Monthly),
            _ => None,
        }
    }

    /// 中文名（报告标题与文件名前缀用）
    pub fn label(self) -> &'static str {
        match self {
            PeriodKind::Daily => "日报",
            PeriodKind::Weekly => "周报",
            PeriodKind::Monthly => "月报",
        }
    }

    /// 文件名前缀（导出默认名用，如「周期日报-2026-10-30」）
    fn file_prefix(self) -> &'static str {
        match self {
            PeriodKind::Daily => "周期日报",
            PeriodKind::Weekly => "周期周报",
            PeriodKind::Monthly => "周期月报",
        }
    }

    /// 数据库 `kind` 列的取值
    pub fn as_str(self) -> &'static str {
        match self {
            PeriodKind::Daily => "daily",
            PeriodKind::Weekly => "weekly",
            PeriodKind::Monthly => "monthly",
        }
    }

    /// 从数据库 `kind` 列反解（列值只由 [`Self::as_str`] 写入，故必然命中）
    fn from_db(raw: &str) -> Self {
        Self::parse(raw).unwrap_or(PeriodKind::Daily)
    }
}

/// 窗口口径
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    /// 自然周期：今天 / 本周 / 本月（前端翻页时也走这个口径，只是把日期算好后传上来）
    Natural,
    /// 自定义区间：起止由用户选，两端都由前端算好传上来
    Custom,
}

impl WindowMode {
    /// 从外部字符串解析
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "natural" => Some(WindowMode::Natural),
            "custom" | "rolling" => Some(WindowMode::Custom),
            _ => None,
        }
    }

    /// 数据库 `window_mode` 列的取值
    pub fn as_str(self) -> &'static str {
        match self {
            WindowMode::Natural => "natural",
            WindowMode::Custom => "custom",
        }
    }
}

/// 一个已解析的周期窗口
///
/// 两端语义**不一致**，这是最容易踩的一处：`from` 含、`to` **不含**（左闭右开）。
/// 传参时"截止到 10-30"是包含 10-30 的，落库时统一加一天变成排他上界，
/// 否则 10-30 23:00 的文章会被算进 10-31 的报告里。
#[derive(Debug, Clone, Copy)]
pub struct PeriodWindow {
    /// 起点（含），UTC
    pub from: DateTime<Utc>,
    /// 终点（不含），UTC
    pub to: DateTime<Utc>,
}

impl PeriodWindow {
    /// 窗口的人读描述（写进报告正文的副标题）
    fn label(&self, kind: PeriodKind) -> String {
        let from = self.from.with_timezone(&Local);
        let to_last = (self.to - Duration::seconds(1)).with_timezone(&Local);
        match kind {
            PeriodKind::Daily => from.format("%Y年%m月%d日").to_string(),
            PeriodKind::Weekly => format!(
                "{} – {}",
                from.format("%m月%d日"),
                to_last.format("%m月%d日")
            ),
            PeriodKind::Monthly => from.format("%Y年%m月").to_string(),
        }
    }
}

/// 解析（或推算）一个周期窗口
///
/// # 规则
/// 1. `start` 与 `end` 都给 → 以其为准（前端翻页 / 自定义区间走这条），只做合法性校验；
/// 2. 都不给 → 按 `now` 推算自然窗口（定时任务走这条，它拿不到前端上下文）；
/// 3. 只给一个 → 报错，不猜。
///
/// # 参数
/// * `kind` - 周期粒度（决定自然窗口的跨度）
/// * `mode` - 窗口口径；`Custom` 必须带起止
/// * `start` - 起点（RFC3339，UTC）
/// * `end` - 终点（RFC3339，UTC，**排他**）
/// * `now` - 推算自然窗口的基准时刻（测试注入用，生产传 `Local::now()`）
///
/// # 错误
/// 时间串无法解析、起止缺一、`from >= to`，或 `Custom` 未带起止时返回错误信息
pub fn resolve_window(
    kind: PeriodKind,
    mode: WindowMode,
    start: Option<&str>,
    end: Option<&str>,
    now: DateTime<Local>,
) -> Result<PeriodWindow, String> {
    match (start, end) {
        (Some(s), Some(e)) => {
            let from = parse_rfc3339(s, "起始时间")?;
            let to = parse_rfc3339(e, "结束时间")?;
            if from >= to {
                return Err("结束时间必须晚于起始时间".to_string());
            }
            Ok(PeriodWindow { from, to })
        }
        (None, None) => {
            if mode == WindowMode::Custom {
                return Err("自定义区间必须同时指定起始与结束时间".to_string());
            }
            Ok(natural_window(kind, now))
        }
        _ => Err("起始与结束时间必须同时提供或同时留空".to_string()),
    }
}

/// 解析一个 RFC3339 时间串
fn parse_rfc3339(raw: &str, label: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| format!("{}格式不合法（需 RFC3339）: {}", label, e))
}

/// 按本地时区推算"当前这一期"的自然窗口
///
/// 日：今天 00:00 → 明天 00:00；周：本周一 00:00 → 下周一 00:00；
/// 月：本月 1 日 00:00 → 下月 1 日 00:00。全部按**本地时区**切，再转 UTC 存储——
/// 用 UTC 切会让东八区的用户在 08:00 前看到"昨天的日报"。
fn natural_window(kind: PeriodKind, now: DateTime<Local>) -> PeriodWindow {
    let date = now.date_naive();
    let day_start = |d: chrono::NaiveDate| -> DateTime<Utc> {
        Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).expect("00:00 必然合法"))
            .single()
            .unwrap_or_else(|| now)
            .with_timezone(&Utc)
    };

    let (from_date, to_date) = match kind {
        PeriodKind::Daily => (date, date + Duration::days(1)),
        PeriodKind::Weekly => {
            // num_days_from_monday：周一 = 0，据此回退到本周一
            let back = date.weekday().num_days_from_monday() as i64;
            let monday = date - Duration::days(back);
            (monday, monday + Duration::days(7))
        }
        PeriodKind::Monthly => {
            let first = date.with_day(1).expect("每月都有 1 号");
            let next = if first.month() == 12 {
                chrono::NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
            } else {
                chrono::NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
            }
            .expect("下月 1 号必然合法");
            (first, next)
        }
    };
    PeriodWindow {
        from: day_start(from_date),
        to: day_start(to_date),
    }
}

// ─── 文章选取 ──────────────────────────────────────────────────────────────────

/// 窗口内的一篇文章（送模型 + 落引用源都用它）
#[derive(Debug, Clone, FromRow)]
pub struct PeriodArticle {
    /// 文章 ID（引用源跳转的唯一依据）
    pub id: i64,
    /// 标题
    pub title: String,
    /// 摘要（RSS 自带 description 清洗而来；可能为空）
    pub summary: String,
    /// 正文 HTML（摘要为空时退化为纯文本取前 N 字）
    pub content: String,
    /// 原文链接
    pub link: String,
    /// 发布时间（部分源不提供，此时为 NULL，排序时排在末尾）
    pub published_at: Option<DateTime<Utc>>,
    /// 订阅源名称（引用源展示用）
    pub feed_name: String,
}

/// 窗口内的文章统计（空周期态用）
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Coverage {
    /// 窗口内文章总数
    pub articles: i64,
    /// 窗口内涉及的订阅源数
    pub feeds: i64,
}

/// 统计窗口内的文章与订阅源数量
///
/// 用于"这一天还没有报告"的空态：先把"有没有素材"告诉用户，避免点了生成才发现 0 篇。
pub async fn coverage(
    pool: &SqlitePool,
    window: PeriodWindow,
) -> Result<Coverage, String> {
    let row: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COUNT(DISTINCT feed_id) FROM articles \
         WHERE published_at >= ? AND published_at < ?",
    )
    .bind(window.from)
    .bind(window.to)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(Coverage {
        articles: row.0,
        feeds: row.1,
    })
}

/// 取窗口内的文章（按发布时间倒序，最多 [`MAX_ARTICLES`] 篇）
///
/// 排序只用 `published_at`：`entity_score` 默认 NULL，NULL 参与排序等于随机取样。
/// 没有发布时间的行排最后（`IS NULL` 在 DESC 里本就靠后，这里显式写出意图）。
async fn select_articles(
    pool: &SqlitePool,
    window: PeriodWindow,
    limit: usize,
) -> Result<Vec<PeriodArticle>, String> {
    sqlx::query_as::<_, PeriodArticle>(
        "SELECT a.id, COALESCE(a.title, '') AS title, COALESCE(a.summary, '') AS summary, \
                COALESCE(a.content, '') AS content, COALESCE(a.link, '') AS link, \
                a.published_at, COALESCE(f.name, '') AS feed_name \
         FROM articles a LEFT JOIN feeds f ON f.id = a.feed_id \
         WHERE a.published_at >= ? AND a.published_at < ? \
         ORDER BY a.published_at IS NULL, a.published_at DESC, a.id DESC \
         LIMIT ?",
    )
    .bind(window.from)
    .bind(window.to)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())
}

// ─── 模型输出 ──────────────────────────────────────────────────────────────────

/// 模型原始输出（字段全部可缺，校验集中在 [`normalize`]）
#[derive(Debug, Clone, Deserialize)]
struct RawReport {
    /// 报告标题
    #[serde(default)]
    title: String,
    /// 一句话结论
    #[serde(default)]
    lead: String,
    /// 要点（3–7 条）
    #[serde(default)]
    highlights: Vec<RawHighlight>,
    /// 主题节（3–7 个）
    #[serde(default)]
    topics: Vec<RawTopic>,
    /// 本期值得跟进（0–5 个，见 [`RawFollowup`]）
    #[serde(default)]
    followups: Vec<RawFollowup>,
}

/// 模型给出的一个"值得跟进"的候选实体
///
/// 与 topics 的区别：topics 是**已经发生的事**的分组，followups 是
/// **接下来该盯什么**的提名。前者是对材料的归纳，后者是对注意力的分配，
/// 所以 `reason` 必须写"变化"（加速/转向/背离）而不是"出现了"——
/// 只是"出现了"的东西每天都有，不构成跟进理由。
#[derive(Debug, Clone, Deserialize)]
struct RawFollowup {
    /// 实体/标的名（尽量用原文里的规范名）
    #[serde(default)]
    name: String,
    /// 为什么要盯它：一句话说清"变化"
    #[serde(default)]
    reason: String,
    /// 引用源编号
    #[serde(default)]
    refs: Vec<i64>,
}

/// 模型给出的一条要点
#[derive(Debug, Clone, Deserialize)]
struct RawHighlight {
    /// 重要性级别：high / medium / low
    #[serde(default)]
    level: String,
    /// 要点正文
    #[serde(default)]
    text: String,
    /// 引用源编号（1 基，对应 prompt 里的文章序号）
    #[serde(default)]
    refs: Vec<i64>,
}

/// 模型给出的一个主题节
#[derive(Debug, Clone, Deserialize)]
struct RawTopic {
    /// 主题名
    #[serde(default)]
    name: String,
    /// 主题摘要
    #[serde(default)]
    summary: String,
    /// 关键事实
    #[serde(default)]
    facts: Vec<String>,
    /// 引用源编号
    #[serde(default)]
    refs: Vec<i64>,
}

/// 校验后的要点（级别已收敛为三档）
#[derive(Debug, Clone)]
struct Highlight {
    /// 三档重要性：high / medium / low
    level: Level,
    /// 正文（已截断）
    text: String,
    /// 已过滤到合法范围的引用编号
    refs: Vec<i64>,
}

/// 校验后的主题节
#[derive(Debug, Clone)]
struct Topic {
    /// 主题名
    name: String,
    /// 摘要
    summary: String,
    /// 关键事实
    facts: Vec<String>,
    /// 引用源编号
    refs: Vec<i64>,
}

/// 要点重要性三档（落到三种 callout 类型，见 [`Level::callout`]）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// 重点
    High,
    /// 关注
    Medium,
    /// 留意
    Low,
}

impl Level {
    /// 从模型给的字符串解析；非法值一律降级为低（不报错、不丢条目）
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "high" => Level::High,
            "medium" => Level::Medium,
            _ => Level::Low,
        }
    }

    /// 对应的 Obsidian callout 类型
    ///
    /// 三个类型名刻意用 danger / warning / note 而不是自定义 `[!point-high]`：
    /// 自定义类型在 Obsidian 里会退化成裸标签，导出到别处就丢了语义。
    fn callout(self) -> &'static str {
        match self {
            Level::High => "danger",
            Level::Medium => "warning",
            Level::Low => "note",
        }
    }
}

/// 校验后的"值得跟进"条目
#[derive(Debug, Clone)]
struct Followup {
    /// 实体/标的名（已截断、去重）
    name: String,
    /// 跟进理由（已截断）
    reason: String,
    /// 引用源编号
    refs: Vec<i64>,
}

/// 校验后的完整报告
#[derive(Debug, Clone)]
struct Report {
    /// 标题（模型给的，空则用窗口标签兜底）
    title: String,
    /// 一句话结论
    lead: String,
    /// 要点
    highlights: Vec<Highlight>,
    /// 主题节
    topics: Vec<Topic>,
    /// 本期值得跟进（可能为空：素材少时不该硬凑）
    followups: Vec<Followup>,
}

/// 按字符（非字节）安全截断
///
/// 直接 `&s[..n]` 会劈裂 UTF-8 而 panic —— 中文报告里几乎必现。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// 从模型原始输出里提取 JSON 对象
///
/// 兼容两种常见的"不听话"：包了 ```json 围栏、或前后带解释文字。
fn extract_json(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(s), Some(e)) if e > s => Some(&trimmed[s..=e]),
        _ => None,
    }
}

/// 把模型输出解析并校验成可渲染的报告
///
/// 校验规则（缺一不可，见 README §6）：
/// 1. `refs` 越界编号**直接丢弃** —— 防模型编引用；
/// 2. 主题与要点数量裁剪到上限，空条目丢弃；
/// 3. `level` 非法降级为 `low`，不整条丢弃；
/// 4. 文本超长在代码里硬截 —— prompt 约束拦不住。
///
/// # 参数
/// * `raw` - 模型原始文本
/// * `article_count` - 本次送入模型的文章数（引用编号的合法上界）
/// * `fallback_title` - 模型没给标题时的兜底标题
///
/// # 返回值
/// `Some(report)` 表示至少解析出了结构；`None` 表示完全不是 JSON
fn normalize(raw: &str, article_count: usize, fallback_title: &str) -> Option<Report> {
    let json = extract_json(raw)?;
    let parsed: RawReport = serde_json::from_str(json).ok()?;

    let clean_refs = |refs: &[i64]| -> Vec<i64> {
        let mut out: Vec<i64> = refs
            .iter()
            .copied()
            .filter(|n| *n >= 1 && (*n as usize) <= article_count)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    };

    let highlights: Vec<Highlight> = parsed
        .highlights
        .iter()
        .filter(|h| !h.text.trim().is_empty())
        .take(MAX_HIGHLIGHTS)
        .map(|h| Highlight {
            level: Level::parse(&h.level),
            text: truncate_chars(h.text.trim(), MAX_HIGHLIGHT_CHARS),
            refs: clean_refs(&h.refs),
        })
        .collect();

    let topics: Vec<Topic> = parsed
        .topics
        .iter()
        .filter(|t| !t.name.trim().is_empty())
        .take(MAX_TOPICS)
        .map(|t| Topic {
            name: truncate_chars(t.name.trim(), 60),
            summary: truncate_chars(t.summary.trim(), MAX_TOPIC_SUMMARY_CHARS),
            facts: t
                .facts
                .iter()
                .map(|f| f.trim())
                .filter(|f| !f.is_empty())
                .take(MAX_FACTS)
                .map(|f| truncate_chars(f, MAX_FACT_CHARS))
                .collect(),
            refs: clean_refs(&t.refs),
        })
        .collect();

    let title = if parsed.title.trim().is_empty() {
        fallback_title.to_string()
    } else {
        truncate_chars(parsed.title.trim(), 60)
    };

    // 「值得跟进」的清洗比 topics 更严：宁可少给，不可给假。
    // 名字或理由任一为空的条目直接丢——一条没有理由的"建议跟进"
    // 对用户没有任何信息量，只会稀释真正有变化的那两三条。
    let mut seen_names: Vec<String> = Vec::new();
    let followups: Vec<Followup> = parsed
        .followups
        .iter()
        .filter(|f| !f.name.trim().is_empty() && !f.reason.trim().is_empty())
        .take(MAX_FOLLOWUPS)
        .filter_map(|f| {
            let name = truncate_chars(f.name.trim(), 40);
            // 同一个实体只留第一条：模型偶尔会为同一标的写两个角度
            let key = name.to_lowercase();
            if seen_names.contains(&key) {
                return None;
            }
            seen_names.push(key);
            Some(Followup {
                reason: truncate_chars(f.reason.trim(), MAX_FOLLOWUP_REASON_CHARS),
                name,
                refs: clean_refs(&f.refs),
            })
        })
        .collect();

    Some(Report {
        title,
        lead: truncate_chars(parsed.lead.trim(), 160),
        highlights,
        topics,
        followups,
    })
}

/// 拼装送模的提示词
///
/// 每篇只给「标题 + 摘要前 200 字」：窗口内 100 篇 × 8000 字全文 ≈ 100 万 token，
/// 必定超限；聚类只需要摘要。摘要为空时退化为正文纯文本前 600 字，
/// 两者都空就只给标题并显式标注"信息不足"，否则模型会把它当成有内容的文章硬编。
fn build_prompt(kind: PeriodKind, window: PeriodWindow, articles: &[PeriodArticle]) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(articles.len());
    for (idx, a) in articles.iter().enumerate() {
        let digest = if !a.summary.trim().is_empty() {
            truncate_chars(a.summary.trim(), SUMMARY_CHARS)
        } else {
            let text = ai::html_to_text(&a.content);
            if text.trim().is_empty() {
                "（该篇仅有标题，无摘要与正文）".to_string()
            } else {
                truncate_chars(text.trim(), CONTENT_FALLBACK_CHARS)
            }
        };
        // 换行会把一条记录拆成两行，破坏"一行一篇"的可读性
        let digest = digest.replace('\n', " ");
        lines.push(format!("{}. {}｜{}｜{}", idx + 1, a.title.trim(), a.feed_name, digest));
    }

    format!(
        "你是一位资深行业研究员，正在为「{label}」写一份面向研究员的周期综述。\n\
         覆盖范围：{period}（共 {count} 篇，编号 1–{count}）。\n\n\
         下面是这批文章的标题、来源与摘要：\n{list}\n\n\
         请完成三件事，只输出 JSON：\n\
         1) title：一句话标题，点出本期最主要的共振点，不超过 30 字；\n\
         2) lead：一句话结论，不超过 60 字；\n\
         3) highlights：3–7 条要点，每条 60–140 字，按重要性排序，\
         level 取值只能是 high / medium / low；\n\
         4) topics：3–7 个主题，把内容相近的文章聚在一起，\
         每个主题含 name（不超过 30 字）、summary（3–4 句）、facts（2–5 条带数字的关键事实，每条不超过 40 字）、refs；\n\
         5) followups：本期**值得接下来继续盯**的标的（0–5 个），\
         每个含 name（实体或标的的规范名）、reason（为什么值得跟进）、refs。\n\n\
         关于 followups 的严格要求：\n\
         - reason 必须写出**变化**（例如「出货增速由 30% 降到 8%」「首次提到海外产能」\
         「与上期判断方向相反」），不能只是「出现了某公司的新闻」——\
         只是出现了的东西每天都有，不构成跟进理由；\n\
         - 没有真实变化就返回空数组 []，不要为了凑数而列；\n\
         - 不要与 topics 重复罗列，followups 关注的是「接下来该看什么」而不是「这期发生了什么」。\n\n\
         硬约束：\n\
         - refs 只能填上面给出的文章编号，不得编造编号；\n\
         - 只能依据上述材料，材料没写的数据一律不提；\n\
         - 不要输出 Markdown 代码块或任何解释文字，直接输出 JSON：\n\
         {{\"title\":\"...\",\"lead\":\"...\",\"highlights\":[{{\"level\":\"high\",\"text\":\"...\",\"refs\":[1,4]}}],\
         \"topics\":[{{\"name\":\"...\",\"summary\":\"...\",\"facts\":[\"...\"],\"refs\":[1,4]}}],\
         \"followups\":[{{\"name\":\"...\",\"reason\":\"...\",\"refs\":[2]}}]}}",
        label = kind.label(),
        period = window.label(kind),
        count = articles.len(),
        list = lines.join("\n")
    )
}

// ─── Markdown 渲染 ─────────────────────────────────────────────────────────────

/// 把校验后的报告渲染成 Markdown
///
/// 结构 = 标题 + 结论 callout + 三条要点段落块 + N 个主题节 + 引用源清单。
/// 要点直接写成 callout（`[!danger]` / `[!warning]` / `[!note]`），
/// 一份 md 同时喂三种输出（应用内阅读、导出 md、导出 html），没有第二份真相。
///
/// 引用编号写成纯文本 `[n]`：前端在渲染后把它们替换成可点按钮，
/// 而导出的 md / html 里就是可读的编号，不会留下指向不存在锚点的死链。
fn render_markdown(
    kind: PeriodKind,
    window: PeriodWindow,
    report: &Report,
    articles: &[PeriodArticle],
    total_articles: i64,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", report.title));

    // 覆盖范围说明：篇数少于窗口内总数时明说截断，避免用户以为全量都进来了
    if (articles.len() as i64) < total_articles {
        out.push_str(&format!(
            "> 已纳入 {} / 共 {} 篇文章（按时间倒序截断）。\n\n",
            articles.len(),
            total_articles
        ));
    }

    if !report.lead.is_empty() {
        out.push_str(&format!("> [!summary] {}\n\n", report.lead));
    }

    let level_label = |l: Level| -> &'static str {
        match l {
            Level::High => "重点",
            Level::Medium => "关注",
            Level::Low => "留意",
        }
    };
    for h in &report.highlights {
        out.push_str(&format!(
            "> [!{}] {}\n> {}{}\n\n",
            h.level.callout(),
            level_label(h.level),
            h.text,
            ref_suffix(&h.refs)
        ));
    }

    for (i, t) in report.topics.iter().enumerate() {
        out.push_str(&format!("## {}\n\n", t.name));
        if !t.summary.is_empty() {
            out.push_str(&format!("{}\n\n", t.summary));
        }
        if !t.facts.is_empty() {
            for f in &t.facts {
                out.push_str(&format!("- {}\n", f));
            }
            out.push('\n');
        }
        if !t.refs.is_empty() {
            out.push_str(&format!("来源：{}\n\n", ref_list(&t.refs)));
        }
        // 主题之间用分隔线断开，与旧报告的版式一致
        if i + 1 < report.topics.len() {
            out.push_str("---\n\n");
        }
    }

    // 「本期值得跟进」放在引用源之前、全部主题之后。
    //
    // 位置是有讲究的：读者看完所有主题，脑子里已经在问"那我接下来该看什么"，
    // 此刻给这份清单正好接上；放在开头会变成"还没看内容就被指路"，
    // 放在文末之后（引用源之后）则会被当作附录跳过。
    //
    // 段标题用「接下来盯什么」而不是「值得跟进」：后者像系统通知，
    // 前者是一个动作邀请。这里不用 callout——跟进清单是待办，不是提示。
    if !report.followups.is_empty() {
        out.push_str("---\n\n## 接下来盯什么\n\n");
        out.push_str("> 这些不是因为「出现了新闻」，而是因为**出现了变化**。\n\n");
        for f in &report.followups {
            out.push_str(&format!(
                "- **{}**：{}{}\n",
                f.name,
                f.reason,
                ref_suffix(&f.refs)
            ));
        }
        out.push('\n');
    }

    out.push_str("---\n\n## 引用源\n\n");
    for (idx, a) in articles.iter().enumerate() {
        let when = a
            .published_at
            .map(|t| {
                t.with_timezone(&Local)
                    .format("%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        let meta = if when.is_empty() {
            a.feed_name.clone()
        } else {
            format!("{} · {}", a.feed_name, when)
        };
        out.push_str(&format!("{}. {} — {}\n", idx + 1, a.title.trim(), meta));
    }
    out.push('\n');
    out.push_str(&format!("_{} · {}_\n", kind.label(), window.label(kind)));

    if out.chars().count() > MAX_MARKDOWN_CHARS {
        truncate_chars(&out, MAX_MARKDOWN_CHARS)
    } else {
        out
    }
}

/// 把引用编号渲染成 `[1] [4]` 形式（空集返回空串）
fn ref_list(refs: &[i64]) -> String {
    refs.iter()
        .map(|n| format!("[{}]", n))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 要点末尾的引用后缀：前置一个空格，无引用时为空串
fn ref_suffix(refs: &[i64]) -> String {
    if refs.is_empty() {
        String::new()
    } else {
        format!(" {}", ref_list(refs))
    }
}

// ─── 落库 ──────────────────────────────────────────────────────────────────────

/// 列表用的报告摘要
///
/// # 为什么加 `rename_all = "camelCase"`
/// 前端 `stores/reportPeriod.ts` 的 `PeriodSummary` 接口是按 camelCase 声明的
/// （`articleCount` / `createdAt` / `isRead` …），而 serde 默认输出 snake_case。
/// 两边不一致时**不报错**：字段静默变成 `undefined`，界面上表现为数字格子全空、
/// 日期显示 `NaN年NaN月NaN日`。2026-10-30 真机验收时正是这个症状。
///
/// 注意：本 crate 的总体约定（见 `lib.rs` 文件头）是返回值原样 snake_case，
/// `Article` / `Feed` 等存量模型都遵循它；周期报告是新增域，其前端类型自始至终
/// 按 camelCase 写，故在这里显式对齐，**不要只改一侧**。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodSummary {
    /// 报告 ID
    pub id: i64,
    /// 周期粒度：daily / weekly / monthly
    pub kind: String,
    /// 窗口口径：natural / custom
    pub window_mode: String,
    /// 窗口起点（RFC3339，含）
    pub period_start: String,
    /// 窗口终点（RFC3339，不含）
    pub period_end: String,
    /// 标题
    pub title: String,
    /// 一句话结论
    pub lead: String,
    /// 纳入的文章数
    pub article_count: i64,
    /// 主题数
    pub topic_count: i64,
    /// 来源数
    pub feed_count: i64,
    /// 是否已读
    pub is_read: bool,
    /// 状态：generating / ready / failed
    pub status: String,
    /// 失败原因（status=failed 时有值）
    pub error: String,
    /// 生成时间（本地时区，展示用）
    pub created_at: String,
    /// 生成所用模型
    pub model: String,
}

/// 引用源条目
///
/// 字段命名同 `PeriodSummary`：前端按 camelCase 读，故显式重命名。
/// `ref_no` 漏掉会连带出更难查的症状——`refNo` 全为 `undefined` 时，
/// 右栏的 `flashSource === s.refNo` 变成 `undefined === undefined`，
/// 点任意一条都让**所有条目**同时高亮（表现为"点一个却全选了"）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodSource {
    /// 正文里显示的编号（1 基，与原文章下标一致，允许跳号）
    pub ref_no: i64,
    /// 文章 ID；文章被清理后为 NULL，前端据此禁用「打开全文」
    pub article_id: Option<i64>,
    /// 标题快照
    pub title: String,
    /// 订阅源名称快照
    pub feed_name: String,
    /// 发布时间快照
    pub published: String,
    /// 原文链接快照
    pub url: String,
    /// 归属主题序号（1 基，用于按节分组）
    pub topic_no: Option<i64>,
}

/// 周期报告的一条"值得跟进"
///
/// 字段名走 camelCase（与 `PeriodSummary` / `PeriodSource` 一致）：
/// 本 crate 默认 snake_case，但周期报告是新增域，前端按 camelCase 声明；
/// 两边不一致**不会编译报错**，只表现为前端拿到 undefined。改字段时同步前端类型。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodFollowup {
    /// 行 ID
    pub id: i64,
    /// 候选实体名（模型给的规范名）
    pub entity_name: String,
    /// 为什么值得跟进（写的是"变化"）
    pub reason: String,
    /// 命中的已有实体 ID（NULL = 还没跟踪）
    pub entity_id: Option<i64>,
    /// 是否已点过「跟踪」
    pub tracked: bool,
}

/// 阅读页一次取全的载荷
#[derive(Debug, Clone, Serialize)]
pub struct PeriodDetail {
    /// 摘要（列表同款字段）
    pub summary: PeriodSummary,
    /// 正文 Markdown
    pub markdown: String,
    /// 引用源
    pub sources: Vec<PeriodSource>,
    /// 本期值得跟进（可能为空；为空时前端整段不渲染）
    pub followups: Vec<PeriodFollowup>,
}

/// `report_periods` 的行映射（内部用，字段与 DDL 一一对应）
#[derive(Debug, Clone, FromRow)]
struct PeriodRow {
    id: i64,
    kind: String,
    window_mode: String,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
    title: String,
    lead: String,
    article_count: i64,
    topic_count: i64,
    feed_count: i64,
    model: String,
    status: String,
    error: String,
    markdown: String,
    is_read: i64,
    created_at: DateTime<Utc>,
}

const ROW_COLUMNS: &str = "id, kind, window_mode, period_start, period_end, title, lead, \
     article_count, topic_count, feed_count, model, status, error, markdown, is_read, created_at";

impl PeriodRow {
    /// 转成对外的摘要结构
    fn to_summary(&self) -> PeriodSummary {
        PeriodSummary {
            id: self.id,
            kind: self.kind.clone(),
            window_mode: self.window_mode.clone(),
            period_start: self.period_start.to_rfc3339(),
            period_end: self.period_end.to_rfc3339(),
            title: self.title.clone(),
            lead: self.lead.clone(),
            article_count: self.article_count,
            topic_count: self.topic_count,
            feed_count: self.feed_count,
            is_read: self.is_read != 0,
            status: self.status.clone(),
            error: self.error.clone(),
            created_at: self
                .created_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            model: self.model.clone(),
        }
    }
}

/// 列出某一粒度的报告（按生成时间倒序）
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `kind` - 周期粒度
/// * `limit` - 最多返回条数（前端一次拉 50 条足够翻）
pub async fn list(
    pool: &SqlitePool,
    kind: PeriodKind,
    limit: i64,
) -> Result<Vec<PeriodSummary>, String> {
    let sql = format!(
        "SELECT {} FROM report_periods WHERE kind = ? ORDER BY period_start DESC, id DESC LIMIT ?",
        ROW_COLUMNS
    );
    let rows = sqlx::query_as::<_, PeriodRow>(&sql)
        .bind(kind.as_str())
        .bind(limit.clamp(1, 500))
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(PeriodRow::to_summary).collect())
}

/// 读一份报告的完整内容（正文 + 引用源）
///
/// # 错误
/// 报告不存在时返回错误信息
pub async fn get(pool: &SqlitePool, id: i64) -> Result<PeriodDetail, String> {
    let sql = format!("SELECT {} FROM report_periods WHERE id = ?", ROW_COLUMNS);
    let row = sqlx::query_as::<_, PeriodRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "该报告已不存在".to_string())?;

    let sources = sqlx::query_as::<_, (i64, Option<i64>, String, String, String, String, Option<i64>)>(
        "SELECT ref_no, article_id, title, feed_name, published, url, topic_no \
         FROM report_sources WHERE report_id = ? ORDER BY ref_no",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .map(|r| PeriodSource {
        ref_no: r.0,
        article_id: r.1,
        title: r.2,
        feed_name: r.3,
        published: r.4,
        url: r.5,
        topic_no: r.6,
    })
    .collect();

    let followups = sqlx::query_as::<_, (i64, String, String, Option<i64>, i64)>(
        "SELECT id, entity_name, reason, entity_id, tracked \
         FROM report_followups WHERE report_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .map(|r| PeriodFollowup {
        id: r.0,
        entity_name: r.1,
        reason: r.2,
        entity_id: r.3,
        tracked: r.4 != 0,
    })
    .collect();

    Ok(PeriodDetail {
        summary: row.to_summary(),
        markdown: row.markdown,
        sources,
        followups,
    })
}

/// 设置已读状态
pub async fn set_read(pool: &SqlitePool, id: i64, read: bool) -> Result<(), String> {
    sqlx::query("UPDATE report_periods SET is_read = ? WHERE id = ?")
        .bind(if read { 1 } else { 0 })
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 删除一份报告（引用源随外键 CASCADE 删除）
///
/// 只删库行：正文从来不曾落盘（决策⑭），没有对应文件要清。
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
    // 显式删子表：`PRAGMA foreign_keys` 未在每个连接上开启时 CASCADE 不生效，
    // 留一堆孤儿 source 会让后续按 report_id 的统计悄悄偏大。
    sqlx::query("DELETE FROM report_sources WHERE report_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    // 跟进清单同理：不清会留下一堆指向已删报告的孤儿行
    sqlx::query("DELETE FROM report_followups WHERE report_id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM report_periods WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 把一条「值得跟进」落成真正的实体（用户点了「跟踪」）
///
/// # 为什么实体只能由人点出来
/// 让 AI 自动把所有被提到的公司都加成实体库，等于把实体库变成第二份文章列表，
/// 而实体库的价值恰恰在于"我已经筛过了"。所以这里只处理用户点过的那一条。
///
/// # 幂等
/// 已经 tracked 的条目重复点不报错，直接返回已有 entity_id。
///
/// # 参数
/// * `pool` - 连接池
/// * `followup_id` - `report_followups` 行 ID
///
/// # 返回值
/// 实体 ID（新建或已存在）
///
/// # 错误
/// 行不存在、数据库写入失败时返回错误信息
pub async fn track_followup(pool: &SqlitePool, followup_id: i64) -> Result<i64, String> {
    let row: Option<(String, Option<i64>)> = sqlx::query_as(
        "SELECT entity_name, entity_id FROM report_followups WHERE id = ?",
    )
    .bind(followup_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let (name, existing) = row.ok_or_else(|| "这条跟进已不存在".to_string())?;

    let entity_id = match existing {
        Some(eid) => eid,
        None => {
            // 类型先统一记 `company`：模型只给了名字，猜行业类型会猜错，
            // 而错误类型会污染后续的关系图分组。用户在实体管理里改一次即可。
            let eid = crate::db::Entity::create(pool, &name, "company").await?;
            sqlx::query("UPDATE report_followups SET entity_id = ?, tracked = 1 WHERE id = ?")
                .bind(eid)
                .bind(followup_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(eid);
        }
    };

    sqlx::query("UPDATE report_followups SET tracked = 1 WHERE id = ?")
        .bind(followup_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(entity_id)
}

/// 导出的载荷（正文 + 默认文件名，不含扩展名）
pub struct ExportPayload {
    /// 报告正文 Markdown（纯正文，供前端渲染 HTML 预览）
    pub markdown: String,
    /// 带投影标记的完整文档（md 落盘用：含 frontmatter 与分块标记）
    ///
    /// 与 `markdown` 分开是必须的：前端会把 `markdown` 交给 `renderMarkdown` 渲染成
    /// HTML，而渲染器会转义所有 `<`——把带 `<!-- readflow:begin:... -->` 的文本丢进去，
    /// 用户就会在报告里看到一堆标记原文。
    pub document: String,
    /// 默认文件名（不含扩展名）
    pub default_name: String,
}

/// 取一份报告的导出载荷
///
/// 只取源正文、不渲染：格式差异（是否多走一步 `renderMarkdown`）由前端决定，
/// 这样两类报告共用同一条导出管线（README §7.1）。
pub async fn export_payload(pool: &SqlitePool, id: i64) -> Result<ExportPayload, String> {
    let sql = format!("SELECT {} FROM report_periods WHERE id = ?", ROW_COLUMNS);
    let row = sqlx::query_as::<_, PeriodRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "该报告已不存在".to_string())?;
    let kind = PeriodKind::from_db(&row.kind);
    let local_from = row.period_start.with_timezone(&Local);
    let stamp = match kind {
        PeriodKind::Daily => local_from.format("%Y-%m-%d").to_string(),
        PeriodKind::Weekly => {
            format!(
                "{}-W{:02}",
                local_from.format("%Y"),
                local_from.iso_week().week()
            )
        }
        PeriodKind::Monthly => local_from.format("%Y-%m").to_string(),
    };
    Ok(ExportPayload {
        document: build_export_document(kind, &row, &stamp),
        markdown: row.markdown,
        default_name: format!("{}-{}", kind.file_prefix(), stamp),
    })
}

/// 把库里存的整篇正文包成带投影标记的导出文档
///
/// # 为什么要包
/// 导出到用户目录的 md 会被用户直接编辑（写批注、加判断）。没有块边界时，同名文件
/// 重新导出就是整篇覆盖 = 批注消失。块边界让 [`crate::projection::project_document`]
/// 只替换块内、块外逐字节不动。
///
/// # 为什么只能按文本切段
/// `report_periods` 只存渲染好的 `markdown`，没有结构化的 topics/followups，所以这里
/// 按 [`render_markdown`] 自己写出的段标题切。这些锚点是渲染器的固定字符串、不是 AI
/// 生成的内容，因此可靠；唯一撞车可能是 AI 把某个主题命名为「接下来盯什么」，概率极低，
/// 且即使切错也只是块边界偏移——**用户写在块外的东西仍然安全**，因为块外一律不动。
///
/// 找不到「引用源」段时（例如正文被 `MAX_MARKDOWN_CHARS` 截断）整篇作为一个块，
/// 绝不因为结构对不上而丢内容。
///
/// # 参数
/// * `kind` - 报告类型（决定 `type` 与 `readflow_id` 的类型段）
/// * `row` - 报告行（取正文与窗口边界）
/// * `stamp` - 周期标识（与默认文件名同源，保证"文件叫什么"与"文件里写着我是谁"一致）
///
/// # 返回值
/// 可直接交给 [`crate::projection::project_document`] 的完整文档
fn build_export_document(kind: PeriodKind, row: &PeriodRow, stamp: &str) -> String {
    let kind_slug = kind.as_str();
    let frontmatter = format!(
        "type: period-{kind_slug}\nreadflow_id: report:period:{kind_slug}:{stamp}\n\
         period_start: {}\nperiod_end: {}\n",
        row.period_start.with_timezone(&Local).format("%Y-%m-%d"),
        row.period_end.with_timezone(&Local).format("%Y-%m-%d"),
    );
    let blocks = split_into_blocks(&row.markdown);
    // assemble 要 `&[(&str, &str)]`，这里把持有所有权的 String 借出去，避免复制正文
    let borrowed: Vec<(&str, &str)> = blocks
        .iter()
        .map(|(name, inner)| (*name, inner.as_str()))
        .collect();
    crate::projection::assemble(&frontmatter, &borrowed)
}

/// 按渲染器自己写出的段标题，把整篇正文切成标记块
///
/// 段序固定：`header`（标题 + 覆盖范围说明）→ `brief`（摘要 callout + 要点）→
/// `body`（各主题）→ `followups`（接下来盯什么）→ `sources`（引用源与页脚）。
/// 块名固定不随内容变化，因此重新生成时不会错位。
///
/// # 参数
/// * `markdown` - 库里存的整篇正文
///
/// # 返回值
/// 按文件内顺序排列的 `(块名, 块内 Markdown)`
fn split_into_blocks(markdown: &str) -> Vec<(&'static str, String)> {
    // 用 rfind：真正的段标题在正文靠后，「接下来盯什么」若被 AI 用作主题名会出现在更早处
    let Some(sources_at) = markdown.rfind("\n## 引用源\n") else {
        return vec![("body", markdown.to_string())];
    };
    let follow_at = markdown[..sources_at].rfind("\n## 接下来盯什么\n");
    // 正文段的终点：有跟进清单就是它，否则直接到引用源
    let topics_end = follow_at.unwrap_or(sources_at);
    // 正文段的起点：**第一个二级标题**。不能拿 topics_end 当起点——没有主题时
    // 会把「标题 + 摘要」也划进正文段，header 就成了空块。
    let body_start = markdown[..topics_end]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(topics_end);

    let mut blocks: Vec<(&'static str, String)> = Vec::with_capacity(5);
    // 摘要 callout 的起点：`> [!` 是渲染器写的 callout 固定开头，
    // 而「> 已纳入 x / 共 y 篇」这类普通引用块不算，所以不能只认 `> `
    match markdown[..body_start].find("\n> [!") {
        Some(i) => {
            let brief_at = i + 1;
            blocks.push(("header", markdown[..brief_at].to_string()));
            blocks.push(("brief", markdown[brief_at..body_start].to_string()));
        }
        None => blocks.push(("header", markdown[..body_start].to_string())),
    }
    // 空段不产出：`render_block` 会写出一个没有内容的空块，读起来是噪音
    if body_start < topics_end {
        blocks.push(("body", markdown[body_start..topics_end].to_string()));
    }
    // 段的首尾必须严丝合缝：起点含前导换行、终点不含下一段的前导换行，
    // 这样各段首尾相接连起来才逐字节等于原文（有单测锁住）
    if let Some(f) = follow_at {
        blocks.push(("followups", markdown[f..sources_at].to_string()));
    }
    blocks.push(("sources", markdown[sources_at..].to_string()));
    blocks
}

/// 启动时收尾「生成中」的孤儿行
///
/// # 为什么需要
/// 生成是「先落一行 `status='generating'` 占位 → 再调模型 → 成功才写正文」。
/// 若这次生成期间应用被强杀（托盘退出、崩溃、断电），那一行会永远停在
/// `generating`：列表里挂着一个永远不完成的条目，点进去没有正文也没有报错。
/// 进程内的 [`mark_failed`] 兜不到这种情况 —— 只能在下次启动时统一收尾。
///
/// # 副作用
/// 把残留行标成 `failed` 并写一句人话原因，用户看到的是「生成被中断」而不是
/// 一个假死的 spinner；重新生成走同一窗口的 `UPDATE` 覆盖，不会堆垃圾行。
///
/// # 参数
/// * `pool` - 数据库连接池
///
/// # 返回
/// 被杀的行数；0 表示没有孤儿（正常情况）
pub async fn recover_orphans(pool: &SqlitePool) -> Result<u64, String> {
    let res = sqlx::query(
        "UPDATE report_periods SET status = 'failed', error = ? WHERE status = 'generating'",
    )
    .bind("生成被中断（应用已退出），请重新生成")
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(res.rows_affected())
}

// ─── 生成 ──────────────────────────────────────────────────────────────────────

/// 生成一份周期报告
///
/// # 流程
/// 1. 解析窗口 → 统计窗口内总数 → 取文章（最多 150 篇）；
/// 2. 装配 AI 提供商（缺配置时报"去哪儿配"，不产出半份报告）；
/// 3. 先落一行 `status='generating'` 占位（同时拿到 id 作取消键），
///    同一周期重复生成走 UPDATE 覆盖（`UNIQUE` 约束，决策⑭ 的覆盖语义）；
/// 4. 一次调用模型 → 解析校验 → 渲染 md；
/// 5. 回写正文与统计、重写引用源、置 `ready`。任一步失败置 `failed` + 原因。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `kind` - 周期粒度
/// * `mode` - 窗口口径
/// * `start` / `end` - 起止（RFC3339，`end` 排他）；都为空时按自然周期推算
/// * `provider_id` - AI 提供商标识；空串用默认值
/// * `cancel_key` - 取消键（前端生成的请求 ID）；`None` 表示不可取消（定时任务走这条）
/// * `progress` - 进度回调（阶段标识, 已完成, 总数）
///
/// # 返回值
/// 新报告的 ID
///
/// # 错误
/// 窗口非法、周期内没有文章、AI 未配置、模型调用失败或返回无法解析时返回错误信息
#[allow(clippy::too_many_arguments)]
pub async fn generate(
    pool: &SqlitePool,
    kind: PeriodKind,
    mode: WindowMode,
    start: Option<&str>,
    end: Option<&str>,
    provider_id: &str,
    cancel_key: Option<&str>,
    progress: ProgressSink<'_>,
) -> Result<i64, String> {
    let window = resolve_window(kind, mode, start, end, Local::now())?;

    progress("collect", 0, 0);
    let cover = coverage(pool, window).await?;
    if cover.articles == 0 {
        return Err(format!("{}没有任何文章，无法生成报告", window.label(kind)));
    }
    let articles = select_articles(pool, window, MAX_ARTICLES).await?;
    progress("collect", articles.len(), articles.len());

    let provider_key = if provider_id.trim().is_empty() {
        DEFAULT_PROVIDER
    } else {
        provider_id.trim()
    };
    let provider = ai::load_provider(pool, provider_key).await.map_err(|_| {
        format!(
            "需要配置 AI 提供商（{}）的 URL / Key / 模型，请在「设置 → AI」中完善",
            provider_key
        )
    })?;

    let report_id = upsert_generating(pool, kind, mode, window).await?;

    progress("cluster", 0, 0);
    let prompt = build_prompt(kind, window, &articles);
    let messages = vec![ai::Message {
        role: "user".to_string(),
        content: prompt,
    }];
    let noop = |_: &str| {};
    let raw = match ai::chat_with_messages(
        &provider,
        &messages,
        Some(0.3),
        &noop,
        cancel_key,
    )
    .await
    {
        Ok(text) => text,
        Err(e) => {
            let msg = if e == ai::CANCELLED_MSG {
                "已取消本次生成".to_string()
            } else {
                e
            };
            mark_failed(pool, report_id, &msg).await;
            return Err(msg);
        }
    };

    progress("compose", 0, 0);
    let fallback = format!("{} · {}", kind.label(), window.label(kind));
    let Some(report) = normalize(&raw, articles.len(), &fallback) else {
        let msg = "模型返回的内容不是合法 JSON，本次生成作废".to_string();
        mark_failed(pool, report_id, &msg).await;
        return Err(msg);
    };
    if report.topics.is_empty() && report.highlights.is_empty() {
        let msg = "模型没有给出任何要点或主题，本次生成作废".to_string();
        mark_failed(pool, report_id, &msg).await;
        return Err(msg);
    }

    // 只把被引用的文章写进引用源：refs 里的编号是文章在 prompt 里的原始下标，
    // 直接沿用即可与正文编号对上（允许跳号，UNIQUE 约束不受影响）
    let cited = collect_cited(&report);
    let markdown = render_markdown(kind, window, &report, &articles, cover.articles);

    progress("sources", cited.len(), cited.len());
    finalize(
        pool,
        report_id,
        &report,
        &markdown,
        &articles,
        &cited,
        cover.feeds,
        &provider.model,
    )
    .await?;
    Ok(report_id)
}

/// 收集正文里被引用的全部编号（升序去重）
fn collect_cited(report: &Report) -> Vec<i64> {
    let mut all: Vec<i64> = report
        .highlights
        .iter()
        .flat_map(|h| h.refs.iter().copied())
        .chain(report.topics.iter().flat_map(|t| t.refs.iter().copied()))
        // 跟进段也会带 [n]：漏了它就会出现正文有编号、引用源清单里没有的断链
        .chain(report.followups.iter().flat_map(|f| f.refs.iter().copied()))
        .collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// 插入或覆盖占位行，返回报告 ID
///
/// `UNIQUE(kind, window_mode, period_start)` + `ON CONFLICT DO UPDATE` 实现"同一期
/// 只有一份、重新生成即覆盖"。**覆盖会丢历史版本**，所以 UI 上必须明说（决策⑭）。
async fn upsert_generating(
    pool: &SqlitePool,
    kind: PeriodKind,
    mode: WindowMode,
    window: PeriodWindow,
) -> Result<i64, String> {
    sqlx::query(
        "INSERT INTO report_periods \
            (kind, window_mode, period_start, period_end, title, status, error) \
         VALUES (?, ?, ?, ?, ?, 'generating', '') \
         ON CONFLICT(kind, window_mode, period_start) DO UPDATE SET \
            status = 'generating', error = '', title = excluded.title",
    )
    .bind(kind.as_str())
    .bind(mode.as_str())
    .bind(window.from)
    .bind(window.to)
    .bind(format!("{} · {}", kind.label(), window.label(kind)))
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM report_periods WHERE kind = ? AND window_mode = ? AND period_start = ?",
    )
    .bind(kind.as_str())
    .bind(mode.as_str())
    .bind(window.from)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(id)
}

/// 生成失败：置状态与原因（正文保持为空，不留半份报告）
async fn mark_failed(pool: &SqlitePool, id: i64, error: &str) {
    let _ = sqlx::query("UPDATE report_periods SET status = 'failed', error = ? WHERE id = ?")
        .bind(truncate_chars(error, 500))
        .bind(id)
        .execute(pool)
        .await
        // 回写失败不影响主流程：错误已经通过返回值传到前端了
        .map(|_| ());
}

/// 回写正文、统计与引用源，并置 `ready`
#[allow(clippy::too_many_arguments)]
async fn finalize(
    pool: &SqlitePool,
    id: i64,
    report: &Report,
    markdown: &str,
    articles: &[PeriodArticle],
    cited: &[i64],
    feed_count: i64,
    model: &str,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query(
        "UPDATE report_periods SET title = ?, lead = ?, article_count = ?, topic_count = ?, \
            feed_count = ?, model = ?, markdown = ?, status = 'ready', error = '', \
            created_at = CURRENT_TIMESTAMP, is_read = 0 WHERE id = ?",
    )
    .bind(&report.title)
    .bind(&report.lead)
    .bind(articles.len() as i64)
    .bind(report.topics.len() as i64)
    .bind(feed_count)
    .bind(model)
    .bind(markdown)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // 覆盖式重写：先清旧引用源，避免上一版多出来的编号残留
    sqlx::query("DELETE FROM report_sources WHERE report_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for ref_no in cited {
        let Some(article) = articles.get((*ref_no - 1) as usize) else {
            continue;
        };
        // 归属主题：取第一个引用该篇的主题序号；没有则 NULL
        let topic_no = report
            .topics
            .iter()
            .position(|t| t.refs.contains(ref_no))
            .map(|i| i as i64 + 1);
        sqlx::query(
            "INSERT INTO report_sources \
                (report_id, ref_no, article_id, title, feed_name, published, url, topic_no) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(ref_no)
        .bind(article.id)
        .bind(&article.title)
        .bind(&article.feed_name)
        .bind(
            article
                .published_at
                .map(|t| t.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default(),
        )
        .bind(&article.link)
        .bind(topic_no)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    // 「本期值得跟进」：同样覆盖式重写。
    //
    // 这里顺手把 name 与 entities 表做一次**宽松匹配**，命中就回填 entity_id：
    //   - 匹配上 → 前端能直接显示"已跟踪"，点按钮不会重复建实体；
    //   - 匹配不上 → entity_id 留 NULL，点「跟踪」时才真正创建实体。
    // 匹配只在写入时做一次，不在读取时算：用户之后手动改了实体名，
    // 不该让历史报告的关联跟着变（报告是快照，不是视图）。
    sqlx::query("DELETE FROM report_followups WHERE report_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for f in &report.followups {
        let hit: Option<i64> =
            sqlx::query_scalar("SELECT id FROM entities WHERE lower(name) = lower(?) LIMIT 1")
                .bind(&f.name)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT OR IGNORE INTO report_followups (report_id, entity_name, reason, entity_id)              VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(&f.name)
        .bind(&f.reason)
        .bind(hit)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// 定时任务入口：从任务 config 解析参数并生成当天那一期
///
/// config 形如 `{"kind":"daily","provider":"agnes","window_mode":"natural"}`；
/// 缺项回退默认值。cron 触发的路径拿不到前端上下文，窗口由后端按自然周期推算。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `config` - 任务 config JSON
///
/// # 返回值
/// 任务执行结果（成功 / 未配置 / 失败）
pub async fn run_period_task(pool: &SqlitePool, config: &str) -> TaskOutcome {
    let parsed: serde_json::Value =
        serde_json::from_str(config).unwrap_or(serde_json::json!({}));
    let kind_raw = parsed
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("daily");
    let Some(kind) = PeriodKind::parse(kind_raw) else {
        return TaskOutcome::Failed(format!("未知的报告周期: {}", kind_raw));
    };
    let provider = parsed
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_PROVIDER);
    let noop = |_: &str, _: usize, _: usize| {};
    match generate(
        pool,
        kind,
        WindowMode::Natural,
        None,
        None,
        provider,
        None,
        &noop,
    )
    .await
    {
        Ok(id) => TaskOutcome::Success(format!("已生成{}（#{id}）", kind.label())),
        Err(e) if e.contains("需要配置 AI 提供商") => TaskOutcome::Unconfigured(e),
        Err(e) => TaskOutcome::Failed(e),
    }
}

// ─── 通知类：新增任务时的默认配置 ──────────────────────────────────────────────

/// 周期日报任务的默认 cron（每天 08:00，与旧「每日研究报告」同一时刻）
pub const DEFAULT_DAILY_CRON: &str = "0 0 8 * * *";

/// 周期日报任务的默认 config
pub fn default_daily_config() -> String {
    serde_json::json!({
        "kind": "daily",
        "provider": DEFAULT_PROVIDER,
        "window_mode": "natural"
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个固定的本地时刻（2026-10-30 12:00 东八区）
    fn local_now() -> DateTime<Local> {
        let utc = Utc
            .with_ymd_and_hms(2026, 10, 30, 4, 0, 0)
            .single()
            .expect("构造基准时刻");
        utc.with_timezone(&Local)
    }

    #[test]
    fn daily_window_is_local_midnight_to_next() {
        let w = natural_window(PeriodKind::Daily, local_now());
        // 东八区 10-30 00:00 == UTC 10-29 16:00
        assert_eq!(w.from.to_rfc3339(), "2026-10-29T16:00:00+00:00");
        assert_eq!(w.to.to_rfc3339(), "2026-10-30T16:00:00+00:00");
    }

    #[test]
    fn weekly_window_starts_monday() {
        // 2026-10-30 是周五 → 本周一为 10-26
        let w = natural_window(PeriodKind::Weekly, local_now());
        assert_eq!(
            w.from.with_timezone(&Local).format("%Y-%m-%d").to_string(),
            "2026-10-26"
        );
        assert_eq!(
            w.to.with_timezone(&Local).format("%Y-%m-%d").to_string(),
            "2026-11-02"
        );
    }

    #[test]
    fn monthly_window_crosses_year() {
        // 2026-12-15 → 12-01 起、2027-01-01 止
        let utc = Utc
            .with_ymd_and_hms(2026, 12, 15, 4, 0, 0)
            .single()
            .expect("构造时刻");
        let w = natural_window(PeriodKind::Monthly, utc.with_timezone(&Local));
        assert_eq!(
            w.from.with_timezone(&Local).format("%Y-%m-%d").to_string(),
            "2026-12-01"
        );
        assert_eq!(
            w.to.with_timezone(&Local).format("%Y-%m-%d").to_string(),
            "2027-01-01"
        );
    }

    #[test]
    fn explicit_window_wins_and_must_be_ordered() {
        let w = resolve_window(
            PeriodKind::Daily,
            WindowMode::Natural,
            Some("2026-10-25T00:00:00+08:00"),
            Some("2026-10-26T00:00:00+08:00"),
            local_now(),
        )
        .expect("合法区间应解析成功");
        assert_eq!(w.from.to_rfc3339(), "2026-10-24T16:00:00+00:00");

        let err = resolve_window(
            PeriodKind::Daily,
            WindowMode::Custom,
            Some("2026-10-26T00:00:00+08:00"),
            Some("2026-10-25T00:00:00+08:00"),
            local_now(),
        );
        assert!(err.is_err());
    }

    #[test]
    fn custom_window_requires_both_ends() {
        assert!(
            resolve_window(PeriodKind::Weekly, WindowMode::Custom, None, None, local_now()).is_err()
        );
        assert!(resolve_window(
            PeriodKind::Weekly,
            WindowMode::Custom,
            Some("2026-10-01T00:00:00Z"),
            None,
            local_now()
        )
        .is_err());
    }

    #[test]
    fn out_of_range_refs_are_dropped() {
        let raw = r#"{"title":"t","lead":"l","highlights":[{"level":"weird","text":"x","refs":[1,9,0,-3,1]}],"topics":[{"name":"n","summary":"s","facts":["f"],"refs":[2,99]}]}"#;
        let r = normalize(raw, 5, "fallback").expect("应解析成功");
        assert_eq!(r.highlights[0].level, Level::Low, "非法级别降级为 low");
        assert_eq!(r.highlights[0].refs, vec![1], "越界/重复编号被丢弃并去重");
        assert_eq!(r.topics[0].refs, vec![2]);
    }

    #[test]
    fn non_json_falls_back_to_none() {
        assert!(normalize("抱歉，我无法完成", 10, "f").is_none());
        assert!(normalize("```json\n{\"title\":\"a\"}\n```", 10, "f").is_some());
    }

    #[test]
    fn markdown_contains_three_callouts_and_sources() {
        let raw = r#"{"title":"T","lead":"L","highlights":[
            {"level":"high","text":"H1","refs":[1]},
            {"level":"medium","text":"H2","refs":[2]},
            {"level":"low","text":"H3","refs":[]}],
            "topics":[{"name":"主题一","summary":"S","facts":["F1"],"refs":[1,2]}]}"#;
        let r = normalize(raw, 2, "f").expect("应解析成功");
        let window = PeriodWindow {
            from: Utc.with_ymd_and_hms(2026, 10, 29, 16, 0, 0).single().unwrap(),
            to: Utc.with_ymd_and_hms(2026, 10, 30, 16, 0, 0).single().unwrap(),
        };
        let articles = vec![
            PeriodArticle {
                id: 1,
                title: "文章一".into(),
                summary: "s".into(),
                content: String::new(),
                link: "https://a/1".into(),
                published_at: None,
                feed_name: "源一".into(),
            },
            PeriodArticle {
                id: 2,
                title: "文章二".into(),
                summary: "s".into(),
                content: String::new(),
                link: "https://a/2".into(),
                published_at: None,
                feed_name: "源二".into(),
            },
        ];
        let md = render_markdown(PeriodKind::Daily, window, &r, &articles, 10);
        assert!(md.contains("> [!danger] 重点"));
        assert!(md.contains("> [!warning] 关注"));
        assert!(md.contains("> [!note] 留意"));
        assert!(md.contains("## 主题一"));
        assert!(md.contains("## 引用源"));
        assert!(md.contains("已纳入 2 / 共 10 篇"), "截断要明说");
        assert!(md.contains("1. 文章一"), "引用源清单按编号排列");
    }

    /// 切块后的各段首尾相接必须等于原文——切分只许移动边界，不许丢字节
    #[test]
    fn split_into_blocks_preserves_every_byte() {
        let raw = r#"{"title":"T","lead":"L","highlights":[{"level":"high","text":"H1","refs":[1]}],
            "topics":[{"name":"主题一","summary":"S","facts":["F1"],"refs":[1]}],
            "followups":[{"name":"宁德时代","reason":"R","refs":[1]}]}"#;
        let r = normalize(raw, 1, "f").expect("应解析成功");
        let window = PeriodWindow {
            from: Utc.with_ymd_and_hms(2026, 10, 29, 16, 0, 0).single().unwrap(),
            to: Utc.with_ymd_and_hms(2026, 10, 30, 16, 0, 0).single().unwrap(),
        };
        let articles = vec![PeriodArticle {
            id: 1,
            title: "文章一".into(),
            summary: "s".into(),
            content: String::new(),
            link: "https://a/1".into(),
            published_at: None,
            feed_name: "源一".into(),
        }];
        let md = render_markdown(PeriodKind::Daily, window, &r, &articles, 1);

        let blocks = split_into_blocks(&md);
        let names: Vec<&str> = blocks.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec!["header", "brief", "body", "followups", "sources"],
            "段序必须固定，重新生成时才不会错位"
        );
        assert_eq!(
            blocks.iter().map(|(_, s)| s.as_str()).collect::<String>(),
            md,
            "各段拼接必须逐字节还原原文"
        );
        assert!(blocks[0].1.starts_with("# T"), "header 应从标题开始");
        assert!(blocks[1].1.starts_with("> [!summary]"), "brief 应从摘要 callout 开始");
        assert!(
            blocks[3].1.trim_start().starts_with("## 接下来盯什么"),
            "followups 段以段标题开头"
        );
        assert!(
            blocks[4].1.trim_start().starts_with("## 引用源"),
            "sources 段以段标题开头"
        );
    }

    /// 没有跟进清单时只有四段，且不能因为少一段就把引用源吞进正文
    #[test]
    fn split_into_blocks_without_followups() {
        let md = "# 标题\n\n> [!summary] 导语\n\n## 主题\n\n内容\n\n---\n\n## 引用源\n\n1. 文章\n";
        let blocks = split_into_blocks(md);
        let names: Vec<&str> = blocks.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["header", "brief", "body", "sources"]);
        assert_eq!(
            blocks.iter().map(|(_, s)| s.as_str()).collect::<String>(),
            md
        );
    }

    /// 结构对不上（例如正文被硬截断）时整篇作为一个块，绝不丢内容
    #[test]
    fn split_into_blocks_falls_back_to_single_block() {
        let md = "# 标题\n\n## 主题\n\n被截断的正文";
        let blocks = split_into_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "body");
        assert_eq!(blocks[0].1, md);
    }

    /// 没有摘要 callout 时 header 段吃掉开头，不产生空的 brief 块
    #[test]
    fn split_into_blocks_without_brief() {
        let md = "# 标题\n\n## 主题\n\n内容\n\n---\n\n## 引用源\n\n1. 文章\n";
        let blocks = split_into_blocks(md);
        let names: Vec<&str> = blocks.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["header", "body", "sources"]);
        assert!(
            !blocks[0].1.contains("## 主题"),
            "header 不该吃掉正文：{}",
            blocks[0].1
        );
        assert_eq!(
            blocks.iter().map(|(_, s)| s.as_str()).collect::<String>(),
            md
        );
    }

    /// 端到端：导出文档经两轮投影后，用户写在块间的批注仍在、块内内容已更新
    ///
    /// 这条串起来的正是"周期日报重新生成会不会吃掉我在 Obsidian 里写的话"这个真实问题。
    #[test]
    fn export_document_round_trip_keeps_user_note() {
        let dir = std::env::temp_dir().join(format!("readflow-period-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录失败");
        let path = dir.join("周期日报-2026-11-01.md");

        let build = |body: &str| {
            let blocks = split_into_blocks(body);
            let borrowed: Vec<(&str, &str)> = blocks
                .iter()
                .map(|(n, s)| (*n, s.as_str()))
                .collect();
            crate::projection::assemble(
                "type: period-daily\nreadflow_id: report:period:daily:2026-11-01\n",
                &borrowed,
            )
        };

        let v1 = "# 研究日报 · 11-01\n\n> [!summary] 导语\n\n## 主题\n\n内容 A\n\n---\n\n## 引用源\n\n1. 文章\n";
        crate::projection::project_document(&path, &build(v1)).expect("首次导出失败");

        // 用户在「主题」与「引用源」之间写一句
        let old = std::fs::read_to_string(&path).expect("读回失败");
        std::fs::write(
            &path,
            old.replace(
                "<!-- readflow:end:body -->",
                "<!-- readflow:end:body -->\n\n我的批注：这条判断我不同意。\n",
            ),
        )
        .expect("写回夹具失败");

        // 重新生成：正文变了
        let v2 = "# 研究日报 · 11-01\n\n> [!summary] 导语\n\n## 主题\n\n内容 B（已更新）\n\n---\n\n## 引用源\n\n1. 文章\n";
        crate::projection::project_document(&path, &build(v2)).expect("二次导出失败");

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert!(after.contains("内容 B（已更新）"), "块内应更新：{after}");
        assert!(
            after.contains("我的批注：这条判断我不同意。"),
            "块外批注必须保留：{after}"
        );
        assert!(
            after.contains("readflow_id: report:period:daily:2026-11-01"),
            "身份键要写进 frontmatter：{after}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn long_text_is_hard_truncated() {        let long = "字".repeat(5000);
        let out = truncate_chars(&long, 100);
        assert_eq!(out.chars().count(), 101, "100 字 + 省略号");
        assert!(out.ends_with('…'));
    }

    // ─── 落库路径（内存库，不碰 AI）──────────────────────────────────────────

    use sqlx::SqlitePool;

    /// 建一个带真实 schema 的内存库
    ///
    /// 手抄建表语句会在 schema 演进时静默漂移，故直接调 `create_tables`。
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("建内存库失败");
        crate::db::create_tables(&pool)
            .await
            .expect("建表失败");
        pool
    }

    /// 插入一条订阅源并返回 id
    async fn insert_feed(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO feeds (name, url) VALUES (?, ?) RETURNING id")
            .bind(name)
            .bind(format!("https://example.com/{}", name))
            .fetch_one(pool)
            .await
            .expect("插入订阅源失败")
    }

    /// 插入一条文章并返回 id
    async fn insert_article(
        pool: &SqlitePool,
        feed_id: i64,
        title: &str,
        published: DateTime<Utc>,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO articles (feed_id, guid, link, title, summary, content, published_at) \
             VALUES (?, ?, ?, ?, '', '', ?) RETURNING id",
        )
        .bind(feed_id)
        .bind(format!("guid-{}", title))
        .bind(format!("https://example.com/a/{}", title))
        .bind(title)
        .bind(published)
        .fetch_one(pool)
        .await
        .expect("插入文章失败")
    }

    #[tokio::test]
    async fn coverage_only_counts_inside_window() {
        let pool = test_pool().await;
        let feed = insert_feed(&pool, "源一").await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        insert_article(&pool, feed, "窗口内", day + Duration::hours(1)).await;
        insert_article(&pool, feed, "窗口外", day + Duration::days(3)).await;

        let window = PeriodWindow {
            from: day,
            to: day + Duration::days(1),
        };
        let cover = coverage(&pool, window).await.expect("统计失败");
        assert_eq!(cover.articles, 1, "窗口外文章不能计入");
        assert_eq!(cover.feeds, 1);
    }

    #[tokio::test]
    async fn articles_are_newest_first_and_capped() {
        let pool = test_pool().await;
        let feed = insert_feed(&pool, "源一").await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        for i in 0..5 {
            insert_article(&pool, feed, &format!("a{}", i), day + Duration::hours(i)).await;
        }
        let window = PeriodWindow {
            from: day,
            to: day + Duration::days(1),
        };
        let rows = select_articles(&pool, window, 3).await.expect("查询失败");
        assert_eq!(rows.len(), 3, "limit 生效");
        assert_eq!(rows[0].title, "a4", "按发布时间倒序（最新在前）");
        assert_eq!(rows[0].feed_name, "源一", "订阅源名来自 JOIN");
    }

    #[tokio::test]
    async fn finalize_writes_sources_and_export_name() {
        let pool = test_pool().await;
        let feed = insert_feed(&pool, "财新").await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        insert_article(&pool, feed, "文章一", day + Duration::hours(1)).await;
        insert_article(&pool, feed, "文章二", day + Duration::hours(2)).await;
        let window = PeriodWindow {
            from: day,
            to: day + Duration::days(1),
        };
        let id = upsert_generating(&pool, PeriodKind::Daily, WindowMode::Natural, window)
            .await
            .expect("插入占位行失败");

        let report = Report {
            title: "标题".into(),
            lead: "结论".into(),
            highlights: vec![Highlight {
                level: Level::High,
                text: "要点".into(),
                refs: vec![2],
            }],
            topics: vec![Topic {
                name: "主题".into(),
                summary: "摘要".into(),
                facts: vec!["事实".into()],
                refs: vec![2, 1],
            }],
            followups: vec![Followup {
                name: "宁德时代".into(),
                reason: "出货增速下滑".into(),
                refs: vec![1],
            }],
        };
        let articles = select_articles(&pool, window, MAX_ARTICLES)
            .await
            .expect("查询失败");
        let md = render_markdown(PeriodKind::Daily, window, &report, &articles, 2);
        finalize(&pool, id, &report, &md, &articles, &[1, 2], 1, "model-x")
            .await
            .expect("回写失败");

        let detail = get(&pool, id).await.expect("读取失败");
        assert_eq!(detail.summary.article_count, 2);
        assert_eq!(detail.summary.topic_count, 1);
        assert_eq!(detail.summary.status, "ready");
        assert_eq!(detail.sources.len(), 2);
        assert_eq!(detail.sources[0].ref_no, 1, "引用源按编号升序");
        assert_eq!(detail.sources[1].topic_no, Some(1), "被主题引用的篇目归属该主题");
        assert!(!detail.summary.is_read);

        set_read(&pool, id, true).await.expect("置已读失败");
        assert!(get(&pool, id).await.expect("再读失败").summary.is_read);

        let payload = export_payload(&pool, id).await.expect("导出载荷失败");
        assert!(payload.default_name.starts_with("周期日报-2026-10-30"));

        delete(&pool, id).await.expect("删除失败");
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_sources")
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        assert_eq!(left, 0, "删报告要连带清掉引用源");
        assert!(get(&pool, id).await.is_err());
    }

    #[tokio::test]
    async fn regenerate_overwrites_same_period() {
        let pool = test_pool().await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        let window = PeriodWindow {
            from: day,
            to: day + Duration::days(1),
        };
        let a = upsert_generating(&pool, PeriodKind::Daily, WindowMode::Natural, window)
            .await
            .expect("首次插入失败");
        let b = upsert_generating(&pool, PeriodKind::Daily, WindowMode::Natural, window)
            .await
            .expect("二次插入失败");
        assert_eq!(a, b, "同一期只能有一行，重生成即覆盖");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_periods")
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn list_filters_by_kind() {
        let pool = test_pool().await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        for (kind, offset) in [(PeriodKind::Daily, 0), (PeriodKind::Weekly, 5)] {
            let window = PeriodWindow {
                from: day + Duration::days(offset),
                to: day + Duration::days(offset + 1),
            };
            upsert_generating(&pool, kind, WindowMode::Natural, window)
                .await
                .expect("插入失败");
        }
        let daily = list(&pool, PeriodKind::Daily, 10).await.expect("列表失败");
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0].kind, "daily");
        let weekly = list(&pool, PeriodKind::Weekly, 10).await.expect("列表失败");
        assert_eq!(weekly.len(), 1);
        assert_eq!(weekly[0].kind, "weekly");
    }

    /// 强杀遗留的 generating 行必须在启动时变成 failed
    #[tokio::test]
    async fn recover_orphans_marks_stale_generating() {
        let pool = test_pool().await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        let window = PeriodWindow {
            from: day,
            to: day + Duration::days(1),
        };
        let id = upsert_generating(&pool, PeriodKind::Daily, WindowMode::Natural, window)
            .await
            .expect("插入失败");

        // 模拟上一次运行被强杀：这一行停在 generating
        let killed = recover_orphans(&pool).await.expect("收尾失败");
        assert_eq!(killed, 1, "残留行应被收尾");

        let detail = get(&pool, id).await.expect("读取失败");
        assert_eq!(detail.summary.status, "failed");
        assert!(!detail.summary.error.is_empty(), "失败要有可读原因");

        // 幂等：再跑一次没有可收尾的行
        assert_eq!(recover_orphans(&pool).await.expect("二次收尾失败"), 0);
        // 已完成的报告不受影响
        let ready: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_periods WHERE status = 'generating'")
            .fetch_one(&pool)
            .await
            .expect("统计失败");
        assert_eq!(ready, 0);
    }

    /// 序列化契约：跨边界的键名必须是 camelCase
    ///
    /// 这条守的是**编译期抓不到**的一类错：前后端字段名不一致时接口照样成功返回，
    /// 只是前端读到的全是 `undefined`（数字格子空、日期 `NaN`、引用源编号全空且
    /// 点一条就全高亮）。新增字段时同步改前端接口，别只改一侧。
    #[test]
    fn summary_and_source_serialize_as_camel_case() {
        let summary = PeriodSummary {
            id: 1,
            kind: "daily".into(),
            window_mode: "natural".into(),
            period_start: "2026-10-30T00:00:00+08:00".into(),
            period_end: "2026-10-31T00:00:00+08:00".into(),
            title: "标题".into(),
            lead: "结论".into(),
            article_count: 150,
            topic_count: 5,
            feed_count: 30,
            is_read: false,
            status: "ready".into(),
            error: String::new(),
            created_at: "2026-10-30 21:00".into(),
            model: "agnes-2.5-flash".into(),
        };
        let v = serde_json::to_value(&summary).expect("序列化失败");
        for key in [
            "articleCount",
            "topicCount",
            "feedCount",
            "isRead",
            "windowMode",
            "periodStart",
            "periodEnd",
            "createdAt",
        ] {
            assert!(v.get(key).is_some(), "缺字段 {key}");
        }
        assert!(
            v.get("article_count").is_none(),
            "不该出现 snake_case 键：前端一律按 camelCase 读"
        );

        let src = PeriodSource {
            ref_no: 7,
            article_id: Some(3),
            title: "标题".into(),
            feed_name: "源".into(),
            published: "2026-10-30 15:50".into(),
            url: "https://example.com".into(),
            topic_no: Some(2),
        };
        let v = serde_json::to_value(&src).expect("序列化失败");
        for key in ["refNo", "articleId", "feedName", "topicNo"] {
            assert!(v.get(key).is_some(), "缺字段 {key}");
        }
        assert_eq!(v["refNo"], 7);
    }

    /// PeriodFollowup 同样必须是 camelCase（前端按 entityName / entityId 读）
    #[test]
    fn followup_serializes_as_camel_case() {
        let f = PeriodFollowup {
            id: 3,
            entity_name: "宁德时代".into(),
            reason: "出货增速由 30% 降到 8%".into(),
            entity_id: None,
            tracked: false,
        };
        let v = serde_json::to_value(&f).expect("序列化失败");
        for key in ["entityName", "entityId", "tracked", "reason"] {
            assert!(v.get(key).is_some(), "缺字段 {key}");
        }
        assert!(v.get("entity_name").is_none(), "不该出现 snake_case 键");
        // null 与 false 都要能被前端区分"未跟踪"与"跟踪了但实体丢失"
        assert!(v["entityId"].is_null());
        assert_eq!(v["tracked"], false);
    }

    /// 跟进条目的清洗：空名/空理由丢弃、同名去重、超量截断
    #[test]
    fn normalize_drops_empty_and_duplicate_followups() {
        let raw = r#"{
            "title": "T", "lead": "L",
            "highlights": [],
            "topics": [{"name": "主题", "summary": "s", "facts": [], "refs": [1]}],
            "followups": [
                {"name": "宁德时代", "reason": "出货增速下滑", "refs": [1]},
                {"name": "宁德时代", "reason": "重复出现的同名", "refs": [1]},
                {"name": "", "reason": "没有名字", "refs": []},
                {"name": "有名字没理由", "reason": "   ", "refs": []},
                {"name": "比亚迪", "reason": "首次提到海外产能", "refs": [99]}
            ]
        }"#;
        let report = normalize(raw, 3, "兜底").expect("应解析成功");
        assert_eq!(report.followups.len(), 2, "重复名与空字段都要被丢掉");
        assert_eq!(report.followups[0].name, "宁德时代");
        assert_eq!(report.followups[1].name, "比亚迪");
        // 越界的引用编号要被清掉，否则正文里的 [99] 会指向不存在的引用源
        assert!(report.followups[1].refs.is_empty());
    }

    /// 没有 followups 字段的旧格式输出仍应解析成功（向后兼容）
    #[test]
    fn normalize_tolerates_missing_followups() {
        let raw = r#"{
            "title": "T", "lead": "L",
            "highlights": [],
            "topics": [{"name": "主题", "summary": "s", "facts": [], "refs": [1]}]
        }"#;
        let report = normalize(raw, 3, "兜底").expect("应解析成功");
        assert!(report.followups.is_empty());
    }

    /// 跟踪：未命中实体时新建，重复点幂等，已跟踪的条目返回同一实体
    #[tokio::test]
    async fn track_followup_creates_entity_once_and_is_idempotent() {
        let pool = test_pool().await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        let id = upsert_generating(
            &pool,
            PeriodKind::Daily,
            WindowMode::Natural,
            PeriodWindow {
                from: day,
                to: day + Duration::days(1),
            },
        )
        .await
        .expect("插入占位行失败");
        // 直接写两条跟进：一条已命中实体，一条没有
        sqlx::query("INSERT INTO entities (name, entity_type) VALUES ('比亚迪', 'company')")
            .execute(&pool)
            .await
            .expect("插入实体失败");
        let hit: i64 = sqlx::query_scalar("SELECT id FROM entities WHERE name = '比亚迪'")
            .fetch_one(&pool)
            .await
            .expect("查实体失败");
        let f1: i64 = sqlx::query_scalar(
            "INSERT INTO report_followups (report_id, entity_name, reason, entity_id) \
             VALUES (?, '比亚迪', 'r', ?) RETURNING id",
        )
        .bind(id)
        .bind(hit)
        .fetch_one(&pool)
        .await
        .expect("插入跟进失败");
        let f2: i64 = sqlx::query_scalar(
            "INSERT INTO report_followups (report_id, entity_name, reason) \
             VALUES (?, '宁德时代', 'r') RETURNING id",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("插入跟进失败");

        // 已命中的：直接标记，不新建实体
        let eid = track_followup(&pool, f1).await.expect("跟踪失败");
        assert_eq!(eid, hit, "已命中实体时不应新建");
        let tracked: i64 = sqlx::query_scalar("SELECT tracked FROM report_followups WHERE id = ?")
            .bind(f1)
            .fetch_one(&pool)
            .await
            .expect("读 tracked 失败");
        assert_eq!(tracked, 1);

        // 未命中的：新建实体并回填
        let eid2 = track_followup(&pool, f2).await.expect("跟踪失败");
        let name: String = sqlx::query_scalar("SELECT name FROM entities WHERE id = ?")
            .bind(eid2)
            .fetch_one(&pool)
            .await
            .expect("读实体失败");
        assert_eq!(name, "宁德时代");

        // 幂等：再点一次返回同一实体，且实体表不重复
        let again = track_followup(&pool, f2).await.expect("二次跟踪失败");
        assert_eq!(again, eid2);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities")
            .fetch_one(&pool)
            .await
            .expect("计数失败");
        assert_eq!(count, 2, "只有比亚迪与宁德时代两个实体");
    }

    /// 删除报告要显式清掉跟进行（PRAGMA foreign_keys 不可依赖）
    #[tokio::test]
    async fn delete_report_removes_followups() {
        let pool = test_pool().await;
        let day = Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).single().unwrap();
        let id = upsert_generating(
            &pool,
            PeriodKind::Daily,
            WindowMode::Natural,
            PeriodWindow {
                from: day,
                to: day + Duration::days(1),
            },
        )
        .await
        .expect("插入占位行失败");
        sqlx::query(
            "INSERT INTO report_followups (report_id, entity_name, reason) VALUES (?, 'X', 'r')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .expect("插入跟进失败");

        delete(&pool, id).await.expect("删除失败");
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM report_followups")
            .fetch_one(&pool)
            .await
            .expect("计数失败");
        assert_eq!(left, 0, "跟进行必须被显式删除，否则成孤儿");
    }

    /// 跟进段落的引用必须计入引用源清单，否则正文出现断链编号
    #[test]
    fn collect_cited_includes_followup_refs() {
        let report = Report {
            title: "T".into(),
            lead: String::new(),
            highlights: vec![],
            topics: vec![],
            followups: vec![Followup {
                name: "X".into(),
                reason: "r".into(),
                refs: vec![2, 5],
            }],
        };
        assert_eq!(collect_cited(&report), vec![2, 5]);
    }
}
