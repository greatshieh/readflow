//! AI 摘要与翻译模块 - 通过 OpenAI 兼容接口生成文章 AI 产物
//!
//! # 职责
//! 本模块负责调用外部大模型服务，为文章生成两类产物：
//! - [`generate_summary`]：生成 100 字以内的中文摘要
//! - [`translate_article`]：把文章翻译为目标语言
//!
//! 两者都会把结果**写回 `articles` 表**（这是本模块唯一的副作用），
//! 由前端在下次拉取文章时读取，因此调用方无需再自行持久化。
//!
//! # 设计意图
//! - **协议选择：OpenAI 兼容的 `/chat/completions` 格式。**
//!   该格式已成为事实上的行业标准，各家服务商（兼容 OpenAI 的网关、
//!   本地推理服务等）均提供同构的接口，选定它意味着用户只需在设置里
//!   换一个 URL / Key 即可切换供应商，本模块无需为每个厂商写一套适配。
//!   对应的请求/响应结构体见 [`ChatRequest`] / [`ChatResponse`]，
//!   字段名与 OpenAI 官方文档一一对应。
//! - **凭据传递：Authorization Bearer。**
//!   沿用该协议约定的 Bearer Token 认证方式，API Key 只随请求头发出，
//!   不进入 URL 查询串（避免被网关日志、Referer 之类的旁路记录）。
//! - **无状态调用者**：模块的入参是已组装好的 [`AIProvider`]，
//!   由 lib.rs 的 AI command 从 settings 表读取配置后传入；
//!   本模块不直接读设置、不做配置校验，保持可被单独复用和测试。
//! - **单一 HTTP 出口 + 流式**：全部模型调用都经 [`chat_stream`] 发出，
//!   凭据传递、状态码校验、空结果判定因此只有一份实现。
//!   请求一律带 `stream: true`，增量经 [`DeltaSink`] 回调上抛——
//!   本模块不依赖 Tauri 事件系统，"谁消费增量"由调用方决定，
//!   因此仍可被单独复用与测试（不需要中间态的调用方传空回调即可）。
//! - **错误扁平化**：统一返回 `Result<_, String>`，把 reqwest / sqlx 的
//!   错误转成可读字符串，直接交由 Tauri command 序列化给前端展示。
//!
//! # 注意
//! 本模块不会限制送入模型的正文长度，超长文章可能触发服务商的 token
//! 上限；失败重试仍未实现，属于已知的演进方向。
//! **超时已实现**（见 [`client`]）：早期版本每个调用点都 `Client::new()`，
//! 而 reqwest 默认不设总超时，模型侧一旦不响应，摘要 / 翻译 / 简报 / 研究提取
//! 都会永久挂起——前端既拿不到结果也拿不到错误。
//! **HTTP 状态必须显式校验**（`error_for_status`）：错误响应体同样能被解析成
//! "structure 合法但内容为空"的结果，若不拦截就会把空摘要当成生成成功写库。

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// AI 请求的总超时
///
/// 取 60 秒：足够覆盖一次正常的长文摘要 / 翻译（含服务端排队），
/// 又不会让"半开连接"演变成命令永久挂起。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// 建立连接的超时（DNS + TCP + TLS）
///
/// 明显短于总超时：连不上就应当尽快失败，把时间预算留给真正在跑的推理。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 全局共享的 HTTP 客户端
///
/// `reqwest::Client` 内部持有连接池，每次调用都新建等于每轮请求都重做 TLS 握手；
/// 用 `OnceLock` 惰性构建并复用。
static CLIENT: OnceLock<Client> = OnceLock::new();

/// 取共享的 AI HTTP 客户端（首次调用时构建）
///
/// # 返回值
/// 带超时配置的客户端引用。构建失败时退化为不带超时的默认客户端并打印日志——
/// 构建失败只可能源于不合法的 TLS 后端配置，此时任何请求都会立刻失败并回传可读错误，
/// 不值得为此让所有 AI 命令都改成 `Result` 返回。
fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .unwrap_or_else(|e| {
                eprintln!("构建 AI HTTP 客户端失败，退化为默认客户端: {}", e);
                Client::new()
            })
    })
}

/// AI 聊天补全请求体
///
/// 序列化后作为 POST 的 JSON body，字段名与 OpenAI `/chat/completions`
/// 的请求规格严格一致（额外的 `temperature` 等参数一律不传，交由服务端默认行为）。
#[derive(Serialize)]
pub struct ChatRequest {
    /// 模型标识符，对应 API 字段 `model`（例如 `gpt-4o-mini`），决定计费与能力
    pub model: String,
    /// 对话消息列表，对应 API 字段 `messages`
    pub messages: Vec<Message>,
    /// 采样温度（0.0-2.0，越低越确定）
    ///
    /// 结构化提取类任务（JSON 输出）传 0~0.1 以最大化格式遵从度；
    /// 摘要 / 翻译不传此字段（序列化时跳过），请求体保持与既有行为完全一致，
    /// 由服务端使用其默认温度。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// 是否以 SSE 流式返回，对应 API 字段 `stream`
    ///
    /// 恒为 `true`：本项目的模型调用全部走流式，因为"边生成边显示"本身就是
    /// 摘要 / 翻译这类交互场景的体验主体——非流式时浏览器要等模型把全文写完
    /// 才收到第一个字节，长文翻译期间的界面是完全静止的。
    /// 少数网关会忽略该字段、直接返回整包 JSON，[`chat_stream`] 按响应的
    /// `Content-Type` 兜底识别，因此不传 `false` 也不会出错。
    pub stream: bool,
}

/// 单条对话消息
///
/// 既用于请求（仅序列化的必要字段），也用于解析响应中的 `choices[].message`；
/// 因此同时实现了 `Serialize` 和 `Deserialize`。
#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    /// 角色，对应 API 字段 `role`（`system` / `user` / `assistant`）。
    ///
    /// 一问一答的场景（摘要 / 翻译 / 简报 / 提取）只用 `"user"`——任务约束已完整
    /// 写在 prompt 正文里，不额外引入 `system` 角色，从而保持请求体最小、各兼容
    /// 服务的差异最小。文章追问则必需 `system`（承载文章上下文与回答约束）
    /// 与 `assistant`（回填历史回答）才能维持多轮语义，故不再限于单一角色。
    pub role: String,
    /// 消息正文，对应 API 字段 `content`
    pub content: String,
}

/// AI 聊天补全响应体
///
/// 只声明真正需要的字段，服务端返回的其他字段（usage、created、id 等）
/// 由 serde 默认忽略，不会因为新增字段而解析失败。
#[derive(Deserialize)]
pub struct ChatResponse {
    /// 候选回复列表，对应 API 字段 `choices`
    pub choices: Vec<Choice>,
}

/// 单个候选回复，对应 API 响应中的 `choices[]` 元素
#[derive(Deserialize)]
pub struct Choice {
    /// 该候选回复的消息体，对应 API 字段 `choices[].message`
    pub message: Message,
}

/// 流式响应中的单帧，对应一条 SSE `data:` 行的 JSON 体
///
/// 与整包 [`ChatResponse`] 的唯一结构差别是增量所在的位置：整包在
/// `choices[].message.content`，流式在 `choices[].delta.content`。
/// 首帧（只带 `{"role":"assistant"}`）与末帧（只带 `{"finish_reason":"stop"}`）
/// 的 `delta` 可能是空对象或整体缺省，因此每个字段都加了 `#[serde(default)]`——
/// **字段缺失不等于解析失败**，否则正常的首尾帧会被当成协议错误。
#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

/// 流式帧中的候选元素
#[derive(Deserialize)]
struct StreamChoice {
    /// 增量消息体，对应 API 字段 `choices[].delta`
    #[serde(default)]
    delta: StreamDelta,
}

/// 流式增量消息体
#[derive(Deserialize, Default)]
struct StreamDelta {
    /// 本帧新增的正文；空对象帧解析为 `None`
    #[serde(default)]
    content: Option<String>,
}

/// AI 提供商配置
///
/// 一次调用所需的全部凭据与端点信息。由于 [`AIProvider`] 持有明文 API Key，
/// 它只在进程内部传递，**不会**被序列化进任何落库的字段（见下方写库语句：
/// 只持久化 `ai_model`，不持久化 api_key）。
#[derive(Debug, Clone)]
pub struct AIProvider {
    /// AI 提供商的 Base URL（例如 `https://apihub.agnes-ai.com/v1`）
    ///
    /// 注意：这里存的是**基础地址**，不含 `/chat/completions` 路径；
    /// 真正的请求端点由 [`build_endpoint`] 自动补全，因此用户在设置界面
    /// 只需填到 `/v1` 即可，无需关心具体路径，降低配置出错概率。
    pub api_url: String,
    /// API 密钥，随 Authorization 头发出
    pub api_key: String,
    /// 默认使用的模型 ID
    pub model: String,
}

/// 生成文章摘要（使用自定义 prompt）
///
/// 与 [`generate_summary`] 的差别仅在 prompt 由调用方传入而非内置：
/// 这样 Feed 级别的"配方系统"才能在不修改 ai.rs 内部默认行为的前提下，
/// 为特定订阅源注入个性化指令。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_id` - 文章 ID
/// * `provider` - AI 提供商配置
/// * `custom_prompt` - 调用方已组装好的完整 prompt（含指令 + 正文）
/// * `on_delta` - 流式增量回调
///
/// # 返回值
/// 生成的摘要文本（已写回数据库）
///
/// # 错误
/// 与 [`generate_summary`] 完全一致
pub async fn generate_summary_custom(
    pool: &SqlitePool,
    article_id: i64,
    provider: &AIProvider,
    custom_prompt: &str,
    on_delta: DeltaSink<'_>,
) -> Result<String, String> {
    // 检查文章是否存在（仅验证ID存在性，无需加载完整字段）
    sqlx::query("SELECT 1 FROM articles WHERE id = ? LIMIT 1")
        .bind(article_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Article not found")?;

    // 使用调用方传入的自定义 prompt，不再拼接全局默认
    let summary = chat_stream(provider, custom_prompt, None, on_delta).await?;

    sqlx::query(
        "UPDATE articles SET has_ai_summary = 1, ai_summary = ?, ai_model = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(&summary)
    .bind(&provider.model)
    .bind(article_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(summary)
}

/// 生成文章摘要
///
/// 读取文章原文，交由大模型生成 100 字以内的中文摘要，并把结果写回数据库。
///
/// # 副作用
/// 成功后会执行 `UPDATE articles SET has_ai_summary = 1, ai_summary = ?, ai_model = ?`，
/// 即"生成即落库"。这一设计使摘要结果可被缓存复用：下次打开同一篇文章时，
/// 前端看到 `has_ai_summary` 为真即可直接展示，无需再次付费调用接口。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_id` - 文章 ID
/// * `provider` - AI 提供商配置（端点、密钥、模型）
/// * `on_delta` - 流式增量回调；界面需要"边写边看"时传入事件发射闭包，
///   不需要中间态的场景（测试、后台任务）传 `&noop` 即可
///
/// # 返回值
/// 模型生成的摘要文本
///
/// # 错误
/// - `Article not found`：文章不存在
/// - 网络不可达 / DNS 失败 / 连接被拒等 reqwest 错误
/// - 响应体不符合 [`ChatResponse`] / [`StreamChunk`] 结构时的反序列化错误
/// - 回写数据库的 sqlx 错误
#[allow(dead_code)]
pub async fn generate_summary(
    pool: &SqlitePool,
    article_id: i64,
    provider: &AIProvider,
    on_delta: DeltaSink<'_>,
) -> Result<String, String> {
    // 用 LIMIT 1 而非 fetch_optional 的完整表扫描语义，命中主键后即刻返回
    // 取不到文章时直接短路为英文错误串前端按原样展示（此处保持既有文案不变）
    // 获取文章内容
    let article = sqlx::query_as::<_, crate::db::Article>(
        "SELECT * FROM articles WHERE id = ? LIMIT 1"
    )
    .bind(article_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let article = article.ok_or("Article not found")?;
    // 摘要与正文拼在一起送模型：RSS 的 description 常常已是凝练摘要，
    // 正文才是主体，两者互补可以让模型同时掌握背景与细节。
    // 关键：入库的 content 是 HTML，直接送模型既浪费 token 又会让翻译把标签打乱，
    // 因此先 html_to_text 剥成纯文本；模型拿到的就是干净的语义内容。
    let raw = format!("{}\n\n{}", article.summary, article.content);
    let content = html_to_text(&raw);

    // 构建提示词
    //
    // prompt 设计要点：
    // 1) 前置任务描述（"生成摘要"）再放正文，符合指令跟随类模型的习惯，
    //    也让模型在读到长正文前先明确目标；
    // 2) 显式给出长度约束"100 字以内"——不设上限时模型倾向输出冗长段落，
    //    而阅读器列表页空间有限，需要短摘要；
    // 3) "简洁明了、突出核心内容"约束风格，抑制模型复述原文导语；
    // 4) 用 \n\n 把指令与正文分隔，降低正文被误认为指令的一部分的概率。
    let prompt = format!(
        "请为以下文章生成一段100字以内的摘要，要求简洁明了，突出核心内容：\n\n{}",
        content
    );

    // 发送请求：走流式，边收边回调。
    // 状态码校验、SSE 解析、非流式回退、空结果拦截全部收敛在 chat_stream 内部，
    // 摘要 / 翻译 / 简报 / 提取四条链路共用同一份实现，行为不可能再各自漂移。
    let summary = chat_stream(provider, &prompt, None, on_delta).await?;

    // 更新数据库
    //
    // 写回时机选择在"拿到结果之后、返回给前端之前"同步完成：
    // 保证命令返回时结果已持久化，前端随后的刷新不会读到旧值。
    // has_ai_summary 与 ai_summary 冗余存储（前者供列表页快速判断，
    // 避免为了判定是否存在摘要而读取大文本字段）。
    // ai_model 记录生成所用模型，便于事后溯源与"换模型后重生成"的判断。
    sqlx::query(
        "UPDATE articles SET has_ai_summary = 1, ai_summary = ?, ai_model = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(&summary)
    .bind(&provider.model)
    .bind(article_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(summary)
}

/// 翻译文章内容
///
/// 读取文章原文，交由大模型翻译成目标语言，并把译文写回数据库。
///
/// # 副作用
/// 成功后会执行 `UPDATE articles SET has_ai_translation = 1, ai_translation = ?, ai_model = ?`。
/// 注意 `ai_model` 是与摘要共用的列，因此摘要与翻译由不同模型完成时，
/// 该列最终记录的是后一次调用的模型。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_id` - 文章 ID
/// * `provider` - AI 提供商配置
/// * `target_language` - 目标语言（自然语言串，例如 "中文"，会被直接填入 prompt）
/// * `on_delta` - 流式增量回调；译文通常很长，这条链路的流式收益最大
///
/// # 返回值
/// 模型生成的译文全文（已消毒）
///
/// # 错误
/// - `Article not found`：文章不存在
/// - 网络层错误、响应反序列化错误、回写数据库错误（同 [`generate_summary`]）
pub async fn translate_article(
    pool: &SqlitePool,
    article_id: i64,
    provider: &AIProvider,
    target_language: &str,
    on_delta: DeltaSink<'_>,
) -> Result<String, String> {
    // 获取文章内容
    let article = sqlx::query_as::<_, crate::db::Article>(
        "SELECT * FROM articles WHERE id = ? LIMIT 1"
    )
    .bind(article_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let article = article.ok_or("Article not found")?;
    // 译文需要保留原文排版：直接把入库时的 HTML 正文交给模型，而不是像摘要那样剥成纯文本。
    // 与摘要不同——摘要只需语义，翻译要"像原文一样排版"，因此保留 <p>/<h1>/<ul>/<img> 等标签，
    // 让前端用同一套 .content-body :deep() 样式还原出与原文一致的标题、列表、图片结构。
    // 当 content 为空（如 Hacker News 仅提供 summary）时回退到纯文本 summary，
    // 此时译文自然也没有标签结构（与原文展示一致）。
    let content = if article.content.trim().is_empty() {
        article.summary.clone()
    } else {
        article.content.clone()
    };

    // 构建提示词
    //
    // 关键约束：要求模型"只翻译标签内可见文本、保持所有 HTML 标签与结构不变"，
    // 这样返回的译文仍是带标签的 HTML，前端 v-html 后由 .content-body 的 :deep() 样式
    // 还原出与原文一致的排版。同时禁止输出解释性文字或 ```代码围栏```，避免污染译文。
    // 目标语言固定使用自然语言（"中文"），兼容各模型的理解，无需 ISO 代码。
    let prompt = format!(
        "请将以下 HTML 文章翻译为{}，保持原文意思不变。\n\
         要求：保留所有 HTML 标签、属性与结构（如 <p>、<h1>、<ul>、<img> 等），\
         只翻译标签内的可见文本，不要新增、删除或修改任何标签，不要添加任何解释文字或代码围栏。\n\n\
         {}",
        target_language, content
    );

    // 发送请求：与摘要共用同一个出口，认证方式、状态码校验、SSE 解析完全一致。
    // 注意空结果的报错文案统一为"未返回内容"（原为"未返回译文"）——
    // 出口只有一份，不再为每条链路维护各自的措辞。
    let raw_translation = chat_stream(provider, &prompt, None, on_delta).await?;

    // 模型返回的是"保留标签的 HTML"，必须经与正文相同的消毒流程再入库：
    // 剥离 <script> / on* 事件 / 危险协议，但保留 p/h1/ul/img 等排版标签，
    // 这样既还原了原文排版，又杜绝了译文里夹带的脚本注入（XSS 防护不降级）。
    let translation = crate::rss::sanitize_html(&raw_translation);

    // 消毒后可能变空（模型只回了标签或纯脚本），此时同样按失败处理而不是写入空译文
    if translation.trim().is_empty() {
        return Err("AI 译文经安全清洗后为空，已放弃写入".to_string());
    }

    // 更新数据库
    //
    // 只置 has_ai_translation 位、不动 has_ai_summary：两种 AI 产物相互独立，
    // 前端据此分别控制"显示摘要"/"切换到译文"的入口。
    sqlx::query(
        "UPDATE articles SET has_ai_translation = 1, ai_translation = ?, ai_model = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    )
    .bind(&translation)
    .bind(&provider.model)
    .bind(article_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(translation)
}

/// 将用户配置的"基础地址"规范化为 OpenAI 兼容的聊天补全端点
///
/// 用户在设置里填写的是 Base URL（例如 `https://apihub.agnes-ai.com/v1`），
/// 真正的请求地址需要拼上 `/chat/completions`。本函数做两件事：
/// 1. 去掉末尾多余的斜杠，避免拼出 `//`；
/// 2. 若已带有 `/chat/completions` 则原样返回（兼容用户直接粘贴完整地址的情况）。
///
/// # 参数
/// * `base` - 用户配置的 Base URL
///
/// # 返回值
/// 可直接用于 `client.post()` 的完整端点字符串
fn build_endpoint(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{}/chat/completions", base)
    }
}

/// 流式增量回调
///
/// 每收到**一批**新增文本调用一次（不是每个 token 一次，见 [`DELTA_FLUSH_CHARS`]）。
/// 用 `dyn` 而非泛型参数：`generate_summary` / `translate_article` 是对外 API，
/// 加类型参数会迫使所有调用方携带生命周期标注，而这里并无为每种回调单态化的必要。
/// `Send + Sync` 是硬要求——回调要跨 `await` 点持有，且会被克隆进 Tauri 的事件发射路径。
pub type DeltaSink<'a> = &'a (dyn Fn(&str) + Send + Sync);

/// 增量合并阈值（字符数）
///
/// SSE 的粒度是 token 级的，一篇长文译文会产生数百个事件，而每个事件都要走一次
/// IPC + 一次 Vue 响应式更新。攒够这么多字符再发一次，观感上仍是"逐字浮现"，
/// 事件量却降一个数量级。
const DELTA_FLUSH_CHARS: usize = 16;

/// 被取消请求的固定错误文案
///
/// 前端据此（配合"该请求是否已被 stop() 接管"）把取消与真实失败区分开：
/// 取消不该在界面上显示为红色错误。此串与 `stores/articleChat.ts` 无强耦合——
/// 前端靠 requestId 归属判断，文案仅供兜底展示。
pub const CANCELLED_MSG: &str = "已取消本次生成";

/// 已登记取消的流式任务（request_id 集合）
///
/// 为什么用 `HashSet<String>` 而不是 `CancellationToken` 式的句柄表：
/// 取消是**单向一次性**动作（标记 → 循环消费 → 移除），不需要跨线程唤醒原语；
/// 且取消方（`ai_cancel` 命令）与被取消方（SSE 循环）之间只共享一个字符串键，
/// 与前端现有的 requestId 事件过滤机制天然对齐。
static CANCELLED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

fn cancelled_registry() -> &'static Mutex<std::collections::HashSet<String>> {
    CANCELLED.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// 登记取消一个进行中的流式 AI 任务
///
/// 由 `ai_cancel` 命令调用。重复登记无害（HashSet 语义）；
/// 若任务早已结束，登记会残留到下一次同 ID 调用——request_id 由前端
/// `newRequestId()` 生成、保证不复用，故实际不会发生。
pub fn cancel_request(request_id: &str) {
    cancelled_registry()
        .lock()
        .expect("CANCELLED 锁中毒")
        .insert(request_id.to_string());
}

/// 查询并**消费**一次取消标记（命中即移除，保证一次性语义）
fn take_cancelled(request_id: &str) -> bool {
    cancelled_registry()
        .lock()
        .expect("CANCELLED 锁中毒")
        .remove(request_id)
}

/// 静默清除取消标记（任务正常收尾时的兜底清理）
fn clear_cancelled(request_id: &str) {
    cancelled_registry()
        .lock()
        .expect("CANCELLED 锁中毒")
        .remove(request_id);
}

/// 模型返回空内容时的统一报错
///
/// 四条链路共用同一出口，因此也共用同一句话（此前翻译链路单独写作"未返回译文"）。
const EMPTY_REPLY_MSG: &str = "AI 未返回内容（请检查 API Key、额度或模型名）";

/// SSE 增量解析器
///
/// 网络分块不由我们控制：一个 `data: {...}` 行可能被切在任意位置，甚至切在
/// `"da"` 与 `"ta: "` 之间。因此必须维护一份缓冲，只消费已换行结尾的完整行，
/// 残余留到下一个 chunk 再拼。**这是流式实现里唯一容易出错的地方，故单独成型，
/// 以便脱离网络做单元测试。**
///
/// 缓冲的是**字节**而非字符串：中文正文的 UTF-8 编码是 3 字节，TCP 完全可能把
/// 一个汉字切在两个 chunk 之间。若先对每个 chunk 各自做 `from_utf8_lossy`，
/// 边界上的汉字就会被替换成 U+FFFD 永久损坏；只在凑齐一整行之后才解码，
/// 多字节字符便不可能被切断。
#[derive(Default)]
struct SseParser {
    /// 尚未形成完整行的残余字节（跨 chunk 拼接用）
    buf: Vec<u8>,
    /// 是否已收到 `data: [DONE]`
    done: bool,
}

impl SseParser {
    /// 建立一个空的解析器
    fn new() -> Self {
        Self::default()
    }

    /// 喂入一段原始字节，返回本次新增的正文
    ///
    /// # 参数
    /// * `chunk` - 本次从响应体读到的原始字节（可能是半行、整行，乃至多个事件）
    ///
    /// # 返回值
    /// 本次新解析出的正文增量；无新增（心跳、注释行、空 delta 帧）时为空串
    ///
    /// # 错误
    /// `data:` 行不是合法 JSON 时返回解析错误——宁可明确失败，也不要把协议异常
    /// 静默吞掉，让用户拿到一段缺字的译文却看不出哪里出了问题
    fn push(&mut self, chunk: &[u8]) -> Result<String, String> {
        if self.done {
            return Ok(String::new());
        }
        self.buf.extend_from_slice(chunk);

        let mut out = String::new();
        // 只处理完整行；末尾不完整的那段留在 buf 里等下一个 chunk
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            // drain 出含换行符在内的整行，去掉末尾的 \n（CRLF 再去掉 \r）
            let raw: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw[..raw.len() - 1]);
            let line = line.trim_end_matches('\r');

            // SSE 字段语法：空行是事件分隔符，':' 开头是注释（不少服务端用 `: ping`
            // 做心跳），只有 `data:` 承载正文，其余（event: / id: / retry:）一律忽略
            let Some(rest) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = rest.trim();
            if payload.is_empty() {
                continue;
            }
            if payload == "[DONE]" {
                self.done = true;
                break;
            }
            let frame: StreamChunk =
                serde_json::from_str(payload).map_err(|e| format!("解析流式响应失败: {}", e))?;
            // 首帧只带 role、末帧只带 finish_reason，二者都没有 content，跳过即可
            if let Some(text) = frame.choices.first().and_then(|c| c.delta.content.as_deref()) {
                out.push_str(text);
            }
        }
        Ok(out)
    }

    /// 是否已收到结束标记
    fn is_done(&self) -> bool {
        self.done
    }
}

/// 发起一次多轮聊天补全（**本模块唯一的 HTTP 出口**）
///
/// 摘要 / 翻译 / 简报 / 提取 / 文章追问五条链路全部经此函数访问模型，因此凭据传递
/// 方式、状态码校验、空结果判定只存在一份实现，不可能再各自漂移。
/// 与"一问一答"的唯一差别是入参形态：调用方自行组装完整消息数组（可含 `system`
/// 与多轮 `user`/`assistant`），本函数不改写、不裁剪，原样送进请求体。
///
/// 行为要点：
/// 1. **请求流式**（`stream: true`），再按响应的 `Content-Type` 判定真实形式——
///    若网关忽略该参数、直接返回整包 JSON，则回退为整包解析后一次性回调。
///    没有这层兜底，换个服务商就会从"没有逐字效果"恶化成"完全没有结果"。
/// 2. **增量合并**：攒够 [`DELTA_FLUSH_CHARS`] 个字符才回调一次，流结束时冲刷残余。
/// 3. **空结果即失败**：`trim()` 后为空一律返回 `Err`，交由调用方决定不落库。
///    这条与既有铁律一致——把空文本写成"已生成"，会让该文章的功能永久失效。
///
/// # 参数
/// * `provider` - AI 提供商配置（端点、密钥、模型）
/// * `messages` - 完整对话消息数组，按时间顺序排列（末条应为待回答的 `user` 消息）
/// * `temperature` - 采样温度；`None` 时不携带该字段（服务端默认）
/// * `on_delta` - 增量回调；不需要中间态时传空闭包
/// * `request_id` - 取消标记的键；`Some` 时每个数据块到达后都检查一次
///   [`CANCELLED`] 登记，命中即中断请求。仅支持真取消的链路（文章追问）传
///   `Some`，其余单轮链路传 `None`——整包 JSON 路径与传 `None` 时均不可取消。
///
/// # 返回值
/// 模型生成的完整文本（已去除首尾空白）
///
/// # 错误
/// - 网络不可达 / DNS 失败 / 连接被拒等 reqwest 错误
/// - HTTP 状态非 2xx（如密钥失效、额度用尽、模型名错误）
/// - 响应不符合 OpenAI 兼容结构（整包或 SSE 帧）时的反序列化错误
/// - 返回内容为空
/// - 被取消时返回 [`CANCELLED_MSG`]
pub async fn chat_with_messages(
    provider: &AIProvider,
    messages: &[Message],
    temperature: Option<f32>,
    on_delta: DeltaSink<'_>,
    request_id: Option<&str>,
) -> Result<String, String> {
    // 用基础地址拼出完整的聊天补全端点（自动补 /chat/completions）
    let endpoint = build_endpoint(&provider.api_url);
    let mut response = client()
        .post(&endpoint)
        // OpenAI 兼容协议的约定：Bearer scheme + 明文 token，空格分隔。
        // 不使用 query 参数传 key，避免凭据出现在 URL 中被日志捕获
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json")
        .json(&ChatRequest {
            model: provider.model.clone(),
            // 消息数组原样送入：多轮语义完全由调用方决定，本函数不插入任何自己的内容
            messages: messages.to_vec(),
            temperature,
            stream: true,
        })
        .send()
        .await
        .map_err(|e| e.to_string())?
        // 必须显式校验状态码：4xx/5xx 的错误响应体同样可能被解析成结构合法的
        // 空回复，那样就等于把失败当成功写库。摘要与翻译都靠这一行兜住
        .error_for_status()
        .map_err(|e| format!("AI 服务返回错误: {}", e))?;

    // 判定真实响应形式：只有明确是 text/event-stream 才走 SSE，
    // 其余（含网关忽略 stream 而返回的 application/json）一律按整包处理
    let is_sse = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false);

    if !is_sse {
        let chat_response: ChatResponse = response.json().await.map_err(|e| e.to_string())?;
        let text = chat_response
            .choices
            .first()
            .map(|c| c.message.content.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| EMPTY_REPLY_MSG.to_string())?;
        // 整包回退同样回调一次：上层无需区分流式与否，一律按"最后收到一整段"处理，
        // 界面逻辑因此只有一条路径
        on_delta(&text);
        return Ok(text);
    }

    let mut parser = SseParser::new();
    let mut full = String::new();
    // 待冲刷的增量：攒够阈值就发一次，避免逐 token 打爆 IPC
    let mut pending = String::new();

    // chunk() 在 reqwest 0.11 不受 `stream` 特性门控（唯一门控在 bytes_stream），
    // 因此实现流式无需引入任何新依赖
    while !parser.is_done() {
        // 取消检查放在每个数据块到达后：SSE 块粒度通常在几十毫秒级，
        // 用户点「停止」到请求真正中断的延迟体感上接近即时。
        // 显式 return 丢弃 response —— reqwest 在 Response drop 时会中断底层连接，
        // 服务端（以及计费方）随即停止生成，这才是"真取消"要省的 token。
        if let Some(rid) = request_id {
            if take_cancelled(rid) {
                return Err(CANCELLED_MSG.to_string());
            }
        }
        let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? else {
            break;
        };
        let delta = parser.push(&chunk)?;
        if delta.is_empty() {
            continue;
        }
        full.push_str(&delta);
        pending.push_str(&delta);
        if pending.chars().count() >= DELTA_FLUSH_CHARS {
            on_delta(&pending);
            pending.clear();
        }
    }
    // 流末尾不足一个阈值的残余必须补发，否则界面显示的文本会比实际少一截
    if !pending.is_empty() {
        on_delta(&pending);
    }

    // 正常收尾时兜底清理取消标记：若标记在最后一个数据块之后才登记
    //（比如用户恰在完成瞬间点了停止），它不会被循环消费，留着会误伤下一轮同 ID 请求
    //——虽然 request_id 理论上不复用，防御性清掉不花任何成本
    if let Some(rid) = request_id {
        clear_cancelled(rid);
    }

    let text = full.trim().to_string();
    if text.is_empty() {
        return Err(EMPTY_REPLY_MSG.to_string());
    }
    Ok(text)
}

/// 发起一次单轮聊天补全（[`chat_with_messages`] 的薄封装）
///
/// 把一段完整提示词包装成单条 `user` 消息后委托给 [`chat_with_messages`]。
/// 摘要 / 翻译 / 简报 / 提取这些"一问一答"场景继续走这里，从而与多轮追问共用
/// 同一条 HTTP 出口——凭据传递、状态码校验、空结果判定仍然只有一份实现。
///
/// # 参数
/// * `provider` - AI 提供商配置（端点、密钥、模型）
/// * `prompt` - 发送给模型的完整提示词
/// * `temperature` - 采样温度；`None` 时不携带该字段（服务端默认）
/// * `on_delta` - 增量回调；不需要中间态时传空闭包
///
/// # 返回值
/// 模型生成的完整文本（已去除首尾空白）
///
/// # 错误
/// 与 [`chat_with_messages`] 完全一致（网络错误 / 非 2xx / 反序列化失败 / 空结果）
async fn chat_stream(
    provider: &AIProvider,
    prompt: &str,
    temperature: Option<f32>,
    on_delta: DeltaSink<'_>,
) -> Result<String, String> {
    // 单轮请求就是"只有一条 user 消息"的多轮请求，无需再写一份请求体构造。
    // 单轮链路（摘要 / 翻译 / 简报 / 提取）没有取消入口，request_id 传 None
    let messages = [Message {
        role: "user".to_string(),
        content: prompt.to_string(),
    }];
    chat_with_messages(provider, &messages, temperature, on_delta, None).await
}

/// 按「字符」截断文本，并回报是否真的发生了截断
///
/// 刻意用字符而非字节计数：中文正文里一个汉字占 3 字节，按字节切会把最后一个字
/// 劈成半个 UTF-8 序列，`&s[..n]` 会直接 panic。`chars().take()` 天然落在字符
/// 边界上，是唯一安全且无需额外处理的写法。
///
/// # 参数
/// * `text` - 原始文本
/// * `max_chars` - 保留的最大字符数
///
/// # 返回值
/// 元组 `(截断后的文本, 是否被截断)`；未超限时原样返回且第二项为 `false`
pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    (text.chars().take(max_chars).collect(), true)
}

/// 文章追问的上下文素材
///
/// 由调用方（命令层）从数据库取好并完成加工：正文已剥标签、已截断，实体是命中的
/// 关注实体名，事件是一行式描述。本结构只承载"喂给模型什么"，不含任何数据库类型，
/// 因此 prompt 组装可以被纯函数化测试——不必起数据库、也不必发网络请求。
pub(crate) struct ArticleChatContext<'a> {
    /// 文章标题
    pub title: &'a str,
    /// 正文纯文本（已剥标签、已按字符截断）
    pub text: &'a str,
    /// 正文是否因超长被截断（决定是否附加截断提示）
    pub truncated: bool,
    /// 该文章命中的关注实体名（判定口径与 `batch_score_entities` 一致）
    pub entities: &'a [String],
    /// 该文章已提取的研究事件，一行一条（如 `[ma] 腾讯：收购某公司（2026-09-30）`）
    pub events: &'a [String],
}

/// 文章追问的回答约束（固定追加在材料末尾）
///
/// 刻意**不逐字列举段名**：命中实体 / 研究事件这两段在为空时会被整段省略，
/// 若约束里点名了不存在的段落，模型会去猜一段根本没给出的材料。
/// 统一说"上方列出的材料"，与段落是否存在无关，恒为真。
const ROLE_INSTRUCTION: &str = "你是这篇文章的阅读助手，帮助读者理解上方材料。请遵守：\n\
1. 只依据上方列出的材料作答，不要引入外部知识或自行补充事实；\n\
2. 材料中没有的信息，直接说明\"文章未提及\"，不要推测或编造；\n\
3. 引用研究事件时以其给出的实体与事件类型为准，不要改变其口径；\n\
4. 回答简洁，使用中文，必要时分点列出，不要复述整段原文。";

/// 组装文章追问的 `system` 提示词
///
/// 把"角色约束 + 材料"压成一条 `system` 消息：材料之后才是多轮 `user`/`assistant`，
/// 这样历史轮次不必重复携带正文，token 开销与用户看到的对话长度成正比而非乘积。
///
/// 空段落（该文章没有命中实体 / 没有研究事件）**整段省略**，不写"（无）"这类占位
/// 文本——那既会被模型当成一条真实信息，也白白占用 token。
///
/// # 参数
/// * `ctx` - 上下文素材（标题 / 正文 / 命中实体 / 研究事件）
///
/// # 返回值
/// 可直接作为 `system` 消息内容的提示词
pub(crate) fn build_article_chat_prompt(ctx: &ArticleChatContext<'_>) -> String {
    let mut sections: Vec<String> = Vec::new();

    sections.push(format!("【文章标题】\n{}", ctx.title));

    // 正文可能为空（部分 RSS 源只给摘要）：明确标注而不是留一片空白让模型瞎猜
    let body = if ctx.text.trim().is_empty() {
        "（该文章没有可用正文，请只依据标题作答，并在回答中说明信息有限）".to_string()
    } else if ctx.truncated {
        format!("{}…\n（正文过长，以上仅为前段摘录）", ctx.text)
    } else {
        ctx.text.to_string()
    };
    sections.push(format!("【文章正文】\n{}", body));

    if !ctx.entities.is_empty() {
        sections.push(format!("【命中的关注实体】\n{}", ctx.entities.join("、")));
    }

    if !ctx.events.is_empty() {
        let lines = ctx
            .events
            .iter()
            .map(|e| format!("- {}", e))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("【已从该文章提取的研究事件】\n{}", lines));
    }

    format!("{}\n\n{}", sections.join("\n\n"), ROLE_INSTRUCTION)
}

/// 将 HTML 片段转换为纯文本
///
/// 入库的 `content` 是经消毒的 HTML（保留标签），直接送模型会有两个问题：
/// - 标签本身占用大量 token，且常包含无语义的样式/结构；
/// - 翻译时标签会被当作正文一起翻译，导致结构错乱。
/// 因此送模型前先剥掉标签、解码常见实体、压缩空白，只保留语义内容。
///
/// 实现说明：用简单的状态机逐字符扫描（遇到 `<` 进入标签态、遇到 `>` 退出），
/// 不引入额外依赖；实体只解码最常见的几个，足以覆盖 RSS 正文场景。
///
/// # 参数
/// * `html` - 原始 HTML 片段
///
/// # 返回值
/// 去标签、去冗余空白后的纯文本
pub(crate) fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            // 进入标签：后续字符直到 '>' 都跳过
            '<' => in_tag = true,
            // 退出标签：恢复正文
            '>' => in_tag = false,
            // 非标签态的字符才保留
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }

    // 解码最常见实体，避免模型读到 &amp; 这类转义
    out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // 压缩空白：HTML 的换行/缩进对模型无意义，折叠成单空格更易读也省 token
    out.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string()
}

/// 通用 AI 对话生成（开放 prompt，不绑定特定任务）
///
/// [`chat_stream`] 的薄封装：入参是调用方自己组装好的 `prompt`，
/// 适用于简报生成、问答、翻译扩展等场景。
/// 本函数**不回写数据库**，仅返回模型生成的文本——调用方自行决定持久化策略。
///
/// # 参数
/// * `pool` - 数据库连接池（目前未使用，保留以与 [`generate_summary`] 签名对齐，
///           未来可在 prompt 构建阶段读取历史对话或上下文时用到）
/// * `provider` - AI 提供商配置（端点、密钥、模型）
/// * `prompt` - 发送给模型的完整提示词
///
/// # 返回值
/// 模型生成的文本内容（已去除首尾空白）
///
/// # 错误
/// - 网络不可达 / DNS 失败 / 连接被拒等 reqwest 错误
/// - HTTP 状态非 2xx（如密钥失效、额度用尽、模型名错误）
/// - 响应体不符合 [`ChatResponse`]（整包）或 [`StreamChunk`]（SSE 帧）结构
/// - 返回内容为空：调用方（如日报生成）无法区分"空内容"与"成功"，
///   故一律按失败处理，避免把空结果当成有效产物继续下游流程
pub async fn generate_chat(
    _pool: &SqlitePool,
    provider: &AIProvider,
    prompt: &str,
) -> Result<String, String> {
    // 后台链路（简报 / 报告）没有界面可更新，传空回调即"不订阅增量"。
    // 注意这不是"换一条非流式实现"：请求体仍带 `stream: true`，
    // 收尾语义（trim 非空才算成功、否则不落库）与非流式完全一致。
    let noop = |_: &str| {};
    chat_stream(provider, prompt, None, &noop).await
}

/// 发起一次带温度参数的聊天补全（结构化提取等确定性任务专用）
///
/// 与 [`generate_chat`] 的区别仅在可显式指定 `temperature`：
/// 信息提取要求输出确定且格式稳定，传 0~0.1 可显著提升 JSON 遵从度；
/// 摘要 / 翻译直接调用 [`chat_stream`] 并传 `None`（不携带温度字段，保持既有行为）。
///
/// # 参数
/// * `provider` - AI 提供商配置
/// * `prompt` - 完整提示词（任务约束 + 输入内容一体）
/// * `temperature` - 采样温度；`None` 时不携带该字段（服务端默认）
///
/// # 返回值
/// 模型生成的文本内容（已去除首尾空白）
///
/// # 错误
/// - 网络不可达 / DNS 失败 / 连接被拒等 reqwest 错误
/// - HTTP 状态非 2xx
/// - 响应体不符合 [`ChatResponse`]（整包）或 [`StreamChunk`]（SSE 帧）结构
/// - 返回内容为空：结构化提取拿不到内容时应当走"这批失败"而非"提取到 0 条"，
///   故按失败返回，交由调用方决定是否跳过该条
pub async fn chat_completion(
    provider: &AIProvider,
    prompt: &str,
    temperature: Option<f32>,
) -> Result<String, String> {
    // 结构化提取跑在后台任务里，逐字回传没有消费方，同 generate_chat 传空回调
    let noop = |_: &str| {};
    chat_stream(provider, prompt, temperature, &noop).await
}

/// 按 provider_id 从 settings 表装配 AI 提供商配置
///
/// settings 是扁平的 key-value 结构（键前缀 `ai_{id}_url/key/model`），
/// 逐项取值而非聚合查询：任一项缺失即刻短路，错误信息可精确定位缺哪一项。
/// 摘要 / 翻译 / 研究提取三条链路共用，保证装配语义一致。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `provider_id` - 提供商标识（如 `agnes`、`deepseek`）
///
/// # 返回值
/// 装配好的提供商配置
///
/// # 错误
/// url / key / model 任一未配置时返回错误信息
pub async fn load_provider(
    pool: &SqlitePool,
    provider_id: &str,
) -> Result<AIProvider, String> {
    // 空串同样按"未配置"处理：密钥的种子值现在是空串，而 `Setting::get` 对空串
    // 返回的是 `Some("")` 而非 `None`。不加这层过滤，"没填密钥"就会变成一次
    // 注定 401 的网络请求（研究提取等后台任务会因此反复失败而不是明确报未配置）。
    let api_url = crate::db::Setting::get(pool, &format!("ai_{}_url", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider URL not configured")?;
    let api_key = crate::db::Setting::get(pool, &format!("ai_{}_key", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider API key not configured")?;
    let model = crate::db::Setting::get(pool, &format!("ai_{}_model", provider_id))
        .await?
        .filter(|v| !v.trim().is_empty())
        .ok_or("AI provider model not configured")?;
    Ok(AIProvider {
        api_url,
        api_key,
        model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一条 SSE 增量帧（`data:` 行 + 空行分隔）
    ///
    /// # 参数
    /// * `text` - 本帧携带的正文
    ///
    /// # 返回值
    /// 可直接喂给 [`SseParser::push`] 的完整帧文本
    fn frame(text: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
            text
        )
    }

    /// 按给定字节位置把原始文本切成多段依次喂入，返回累积正文
    ///
    /// 切点可以落在多字节字符中间——这正是要验证的场景。
    ///
    /// # 参数
    /// * `raw` - 完整 SSE 文本
    /// * `cuts` - 字节切点（需递增且不超过 `raw.len()`）
    ///
    /// # 返回值
    /// 各次 `push` 返回值的拼接
    fn feed_split(raw: &str, cuts: &[usize]) -> String {
        let mut parser = SseParser::new();
        let bytes = raw.as_bytes();
        let mut out = String::new();
        let mut prev = 0;
        for &cut in cuts {
            out.push_str(&parser.push(&bytes[prev..cut]).expect("分片解析不应失败"));
            prev = cut;
        }
        out.push_str(&parser.push(&bytes[prev..]).expect("末尾分片不应失败"));
        out
    }

    #[test]
    fn sse_parser_single_frame() {
        let mut parser = SseParser::new();
        assert_eq!(parser.push(frame("你好").as_bytes()).unwrap(), "你好");
        assert!(!parser.is_done());
    }

    #[test]
    fn sse_parser_accumulates_multiple_frames() {
        let raw = format!("{}{}", frame("第一段"), frame("第二段"));
        let mut parser = SseParser::new();
        assert_eq!(parser.push(raw.as_bytes()).unwrap(), "第一段第二段");
    }

    #[test]
    fn sse_parser_ignores_trailing_partial_line() {
        // 半行必须留到下一块再拼，不能提前当完整行解析（否则 JSON 必然解析失败）
        let mut parser = SseParser::new();
        assert_eq!(
            parser.push(b"data: {\"choices\":[{\"delta\":{\"cont").unwrap(),
            ""
        );
        assert_eq!(
            parser
                .push("ent\":\"补全\"}}]}\n\n".as_bytes())
                .unwrap(),
            "补全"
        );
    }

    #[test]
    fn sse_parser_survives_arbitrary_chunk_boundary() {
        // 逐字节喂入是"任意切分"的最强形式：连 "da"/"ta: " 之间、
        // 汉字三字节序列的中间都会被切开
        let raw = format!("{}data: [DONE]\n\n", frame("逐字节也不能坏"));
        let cuts: Vec<usize> = (1..raw.len()).collect();
        assert_eq!(feed_split(&raw, &cuts), "逐字节也不能坏");
    }

    #[test]
    fn sse_parser_keeps_multibyte_across_boundary() {
        // 直接从"汉"的 UTF-8 首字节之后切开，验证字节缓冲确实避免了 U+FFFD
        let raw = frame("汉字");
        let han = raw.find('汉').expect("测试样本应含汉字");
        assert_eq!(feed_split(&raw, &[han + 1]), "汉字");
    }

    #[test]
    fn sse_parser_stops_at_done() {
        let raw = format!("{}data: [DONE]\n\n{}", frame("A"), frame("B"));
        let mut parser = SseParser::new();
        assert_eq!(parser.push(raw.as_bytes()).unwrap(), "A");
        assert!(parser.is_done());
        // 结束标记之后的内容一律丢弃，避免把服务端收尾杂音混进正文
        assert_eq!(
            parser
                .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"C\"}}]}\n\n")
                .unwrap(),
            ""
        );
    }

    #[test]
    fn sse_parser_ignores_keepalive_and_unknown_fields() {
        // ':' 开头是注释（心跳），event/id/retry 不承载正文，空 data 帧也无内容
        let raw = ": ping\n\nevent: message\nid: 1\nretry: 100\ndata:\n\n";
        let mut parser = SseParser::new();
        assert_eq!(parser.push(raw.as_bytes()).unwrap(), "");
        assert!(!parser.is_done());
    }

    #[test]
    fn sse_parser_skips_frames_without_content() {
        // 真实流的首尾帧：首帧只有 role，末帧只有 finish_reason，二者都无 content。
        // 缺字段必须当"无增量"而非"解析失败"，否则每一条流都会在首帧报错。
        let raw = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"正文\"}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let mut parser = SseParser::new();
        assert_eq!(parser.push(raw.as_bytes()).unwrap(), "正文");
        assert!(!parser.is_done());
    }

    #[test]
    fn sse_parser_handles_crlf() {
        let raw = "data: {\"choices\":[{\"delta\":{\"content\":\"CRLF\"}}]}\r\n\r\n";
        let mut parser = SseParser::new();
        assert_eq!(parser.push(raw.as_bytes()).unwrap(), "CRLF");
    }

    #[test]
    fn sse_parser_accepts_data_without_space() {
        // 规范写的是 `data: `，但 `data:{...}` 同样常见，不能只认带空格的形式
        let raw = "data:{\"choices\":[{\"delta\":{\"content\":\"紧凑\"}}]}\n\n";
        let mut parser = SseParser::new();
        assert_eq!(parser.push(raw.as_bytes()).unwrap(), "紧凑");
    }

    #[test]
    fn sse_parser_reports_invalid_json() {
        // 协议异常必须显式失败：静默吞掉会让用户拿到一段缺字的译文而无从察觉
        let mut parser = SseParser::new();
        let err = parser.push(b"data: {not json}\n\n").unwrap_err();
        assert!(
            err.contains("解析流式响应失败"),
            "错误信息应可定位，实际: {}",
            err
        );
    }

    #[test]
    fn build_endpoint_appends_path() {
        assert_eq!(
            build_endpoint("https://api.example.com/v1"),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn build_endpoint_trims_trailing_slash() {
        // 用户从别处复制地址时常带尾斜杠，不去掉就会拼出 "//chat/completions"
        assert_eq!(
            build_endpoint("https://api.example.com/v1/"),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn build_endpoint_keeps_complete_url() {
        let full = "https://api.example.com/v1/chat/completions";
        assert_eq!(build_endpoint(full), full);
    }

    // ─── 文章追问：上下文组装 ──────────────────────────────────────────────────

    #[test]
    fn truncate_chars_passes_short_text_through() {
        let (out, cut) = truncate_chars("很短的正文", 10);
        assert_eq!(out, "很短的正文");
        assert!(!cut, "未超限时不应报截断");
    }

    #[test]
    fn truncate_chars_keeps_cjk_intact() {
        // 按字节切会把汉字劈成半个 UTF-8 序列（`&s[..n]` 直接 panic），
        // 这里显式验证按字符截断：取 3 个字必须正好是前 3 个汉字
        let (out, cut) = truncate_chars("一二三四五", 3);
        assert_eq!(out, "一二三");
        assert!(cut, "超限时应当报截断");
        // 结果必须是合法字符串（能正常比较即已证明未落在半个字符上）
        assert_eq!(out.chars().count(), 3);
    }

    #[test]
    fn article_chat_prompt_includes_all_sections() {
        let entities = vec!["腾讯".to_string(), "OpenAI".to_string()];
        let events = vec!["[ma] 腾讯：收购某公司（2026-09-30）".to_string()];
        let prompt = build_article_chat_prompt(&ArticleChatContext {
            title: "腾讯发布财报",
            text: "营收同比增长。",
            truncated: false,
            entities: &entities,
            events: &events,
        });

        assert!(prompt.contains("【文章标题】\n腾讯发布财报"));
        assert!(prompt.contains("【文章正文】\n营收同比增长。"));
        assert!(prompt.contains("【命中的关注实体】\n腾讯、OpenAI"));
        assert!(prompt.contains("- [ma] 腾讯：收购某公司（2026-09-30）"));
        // 回答约束必须始终存在，否则模型会自由发挥
        assert!(prompt.contains("阅读助手"));
        assert!(prompt.contains("文章未提及"));
    }

    #[test]
    fn article_chat_prompt_omits_empty_sections() {
        // 没有命中实体 / 没有研究事件时，整段省略而不是写"（无）"——
        // 占位文本会被模型当成一条真实信息。
        //
        // 断言必须带上段名后的换行：段名本身也可能合法地出现在约束文本里，
        // 只匹配段名的话，"约束提到了该段名"会被误判成"材料里插入了这一段"
        // （本测试初版就因此假失败过）。
        let prompt = build_article_chat_prompt(&ArticleChatContext {
            title: "无关文章",
            text: "正文。",
            truncated: false,
            entities: &[],
            events: &[],
        });

        assert!(!prompt.contains("【命中的关注实体】\n"));
        assert!(!prompt.contains("【已从该文章提取的研究事件】\n"));
        assert!(prompt.contains("【文章标题】"));
    }

    #[test]
    fn article_chat_prompt_marks_truncated_body() {
        let prompt = build_article_chat_prompt(&ArticleChatContext {
            title: "长文",
            text: "前段",
            truncated: true,
            entities: &[],
            events: &[],
        });
        assert!(prompt.contains("前段…"));
        assert!(prompt.contains("以上仅为前段摘录"));
    }

    #[test]
    fn article_chat_prompt_handles_empty_body() {
        // 部分 RSS 源只给标题与摘要，正文为空。此时必须显式告知模型信息有限，
        // 而不是留一段空白（模型会自行脑补出一篇不存在的正文）
        let prompt = build_article_chat_prompt(&ArticleChatContext {
            title: "只有标题",
            text: "   ",
            truncated: false,
            entities: &[],
            events: &[],
        });
        assert!(prompt.contains("没有可用正文"));
        assert!(!prompt.contains("【文章正文】\n   "));
    }

    #[test]
    fn cancel_registry_take_is_one_shot() {
        // 取消标记必须是一次性的：SSE 循环"消费"到标记后中断请求，
        // 若 remove 误写成 peek，同一标记会被下一轮请求误命中
        let rid = "test-cancel-take";
        assert!(!take_cancelled(rid), "未登记时不应命中");
        cancel_request(rid);
        assert!(take_cancelled(rid), "登记后首次消费应命中");
        assert!(!take_cancelled(rid), "消费一次后应失效");
    }

    #[test]
    fn cancel_registry_clear_is_idempotent() {
        // 正常收尾的兜底清理：标记不存在时静默通过，不能 panic
        let rid = "test-cancel-clear";
        cancel_request(rid);
        clear_cancelled(rid);
        clear_cancelled(rid); // 重复清理无害
        assert!(!take_cancelled(rid));
    }
}
