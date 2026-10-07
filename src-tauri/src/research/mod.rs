//! 研究信息提取流水线
//!
//! 把信源文章转化为可筛选、可对比的**结构化事件库**（而非摘要或情绪评分）。
//! 流程：文章 → LLM 结构化提取（[`extract`]）→ 实体归一化入库（[`normalize`]）。
//!
//! 设计要点：
//! - 输出为固定维度的 JSON 事件（canonical 实体 + 事件类型 + 客观事实 + 原文证据），
//!   缺省维度留空，禁止概括性描述；
//! - evidence 强制回填原文片段，作为溯源与反幻觉锚点；
//! - 实体经 canonical/alias 归一后落库，"字节跳动/字节"合并为同一实体。
//!
//! # 模块结构
//! - [`extract`]：prompt 组装、JSON 容错解析、单篇/批量提取
//! - [`normalize`]：实体归一化（canonical/别名匹配、未命中自动新建）与事件落库
//! - [`report`]：日报/周报 Markdown 渲染与多通道输出（应用内 / Obsidian / 文件）
//!
//! 本模块同时承载自动化任务的**统一执行器**（[`run_task_once`]）：
//! cron 调度与手动触发共用同一条执行路径，运行结果统一回写任务行。

pub mod extract;
pub mod normalize;
pub mod report;

use std::collections::HashSet;
use std::sync::LazyLock;
use tokio::sync::Mutex;

use crate::db::AutomationTask;

/// 正在运行的自动化任务 ID 集合（并发防护，跨 cron 与手动触发共享）
///
/// 同一任务不允许并发执行两次：批量提取重入会造成重复 API 调用与数据竞争，
/// 报告任务重入会写出重复文件。运行前插入、结束后移除；
/// 持有期间再次触发直接跳过（cron 跳过时记日志，手动触发返回错误提示）。
pub static RUNNING_TASKS: LazyLock<Mutex<HashSet<i64>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// 一次任务执行的结果（区分"未配置"与"失败"，驱动设置页健康度展示）
///
/// - `Success`：正常完成，回写 success；
/// - `Unconfigured`：缺前置配置（如 AI 提供商未填、Vault 路径为空），
///   回写 unconfigured 并携带"去哪里补配置"的引导信息——这不是故障，
///   单独标记可避免用户把配置问题误判为程序错误；
/// - `Failed`：执行中出错，回写 failed 并携带原因。
#[derive(Debug, Clone)]
pub enum TaskOutcome {
    /// 执行成功，携带结果描述
    Success(String),
    /// 缺前置配置，携带引导信息
    Unconfigured(String),
    /// 执行失败，携带错误原因
    Failed(String),
}

/// 单篇文章抽取出的结构化事件（模型原始输出，实体尚未归一化）
///
/// 与 [`crate::db::ResearchEvent`] 的区别：此处的实体只有名称（含别称），
/// 需经 [`normalize::normalize_and_store_events`] 归一化为 `entity_id` 后才能入库。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractedEvent {
    /// 实体规范名（如 "字节跳动"），同一实体多种写法时模型应给出最完整的一个
    pub canonical_name: String,
    /// 文中出现的其他别称（如 ["字节", "ByteDance"]），归一化时并入实体别名
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 实体类型：company / person / product / industry / other
    #[serde(default)]
    pub entity_type: String,
    /// 事件类型（固定枚举，见 [`extract::EVENT_TYPES`]）
    pub event_type: String,
    /// 事件发生日期（ISO 8601 如 "2026-09-20"；未知留空）
    #[serde(default)]
    pub event_date: String,
    /// 客观事实一句话（非概括、非评价）
    pub fact: String,
    /// 原文片段（证据，用于溯源与反幻觉校验）
    #[serde(default)]
    pub evidence: String,
}

/// 执行一次自动化任务（cron 与手动触发的公共执行器）
///
/// 按 task_type 分发到对应实现；执行结果（无论成败）统一回写任务的
/// last_run_at / last_status / last_message，驱动设置页的任务健康度展示。
/// "未配置"状态由各执行器通过 [`TaskOutcome::Unconfigured`] 上报。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `task` - 待执行的任务
///
/// # 返回值
/// 执行成功时返回结果描述；未配置 / 失败时返回引导或错误信息
///
/// # 错误
/// 执行失败或缺少前置配置时返回错误信息（状态已同步回写任务行）
pub async fn run_task_once(
    pool: &sqlx::SqlitePool,
    task: &AutomationTask,
) -> Result<String, String> {
    let outcome = match task.task_type.as_str() {
        "batch_extract" => run_batch_task(pool, task).await,
        // 报告执行器只需要 config：任务行与运行状态回写由本函数统一处理
        "daily_report" => report::run_report_task(pool, &task.config, report::ReportKind::Daily).await,
        "weekly_report" => {
            report::run_report_task(pool, &task.config, report::ReportKind::Weekly).await
        }
        other => TaskOutcome::Failed(format!("未知任务类型: {}", other)),
    };

    // 统一回写运行结果；回写失败不影响主流程（状态展示是尽力而为）
    let (status, message) = match &outcome {
        TaskOutcome::Success(m) => ("success", m.clone()),
        TaskOutcome::Unconfigured(m) => ("unconfigured", m.clone()),
        TaskOutcome::Failed(m) => ("failed", m.clone()),
    };
    let _ = AutomationTask::mark_run_result(pool, task.id, status, &message).await;

    // 后台回执：cron 触发的任务没人盯着窗口，任务结束无论成败都值得提醒一句。
    // 「未配置」刻意不发——配置缺失会在每个 cron 周期重复刷屏，
    // 这类引导信息留在任务健康度面板里看即可。
    // 通知内部自带三重门（窗口聚焦 / 开关关闭都会静默跳过），这里无需判断。
    match &outcome {
        TaskOutcome::Success(m) => crate::notify::task_result(pool, &task.name, true, m).await,
        TaskOutcome::Failed(m) => crate::notify::task_result(pool, &task.name, false, m).await,
        TaskOutcome::Unconfigured(_) => {}
    }

    match outcome {
        TaskOutcome::Success(m) => Ok(m),
        TaskOutcome::Unconfigured(m) | TaskOutcome::Failed(m) => Err(m),
    }
}

/// 执行批量提取任务
///
/// 从任务 config 解析 provider / min_score / batch_limit（缺项回退默认值）；
/// 提供商配置缺失归为 Unconfigured（引导用户先配置），提取报错归为 Failed。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `task` - 任务行（取 config）
///
/// # 返回值
/// 任务执行结果
async fn run_batch_task(pool: &sqlx::SqlitePool, task: &AutomationTask) -> TaskOutcome {
    let parsed: serde_json::Value =
        serde_json::from_str(&task.config).unwrap_or(serde_json::json!({}));
    let provider_id = parsed
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("agnes");
    let min_score = parsed.get("min_score").and_then(|v| v.as_i64()).unwrap_or(40);
    let batch_limit = parsed
        .get("batch_limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(50);

    // 提供商配置缺失属于"未配置"而非"失败"：引导信息直接告诉用户去哪里补配置
    let provider = match crate::ai::load_provider(pool, provider_id).await {
        Ok(p) => p,
        Err(_) => {
            return TaskOutcome::Unconfigured(format!(
                "需要配置 AI 提供商（{}）的 URL / Key / 模型，请在「设置 → AI」中完善",
                provider_id
            ));
        }
    };

    match extract::extract_batch(pool, &provider, min_score, batch_limit).await {
        Ok((processed, events)) => TaskOutcome::Success(format!(
            "处理 {} 篇文章，新增 {} 条事件（阈值 {}，模型 {}）",
            processed, events, min_score, provider.model
        )),
        Err(e) => TaskOutcome::Failed(e),
    }
}
