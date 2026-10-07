//! Obsidian 单向导出
//!
//! 把一篇文章生成为 Markdown 笔记并写入用户配置的 Obsidian Vault 目录。
//! 这是开发计划 Phase 6「Obsidian 集成」的核心交付：本地优先、单向（只把文章落盘到 Vault，
//! 不回读 Obsidian 内部状态）。导出机制采用「写文件到 Vault 目录」方案（用户在设置里手动
//! 填写 Vault 根路径），因此不依赖 Obsidian 进程是否运行、可离线、可批量。
//!
//! # 模块职责
//! - [`html_to_markdown`]：把后端清洗过的 HTML 正文转成易读的 Markdown。为避免为此引入
//!   第三方 HTML 解析依赖（它们提供的是 tokenizer / tree-builder 驱动，仍需自建便捷层），
//!   这里用一份轻量、零额外依赖的迷你 HTML 解析器（字节扫描 + 递归下降）处理常见块级/行内标签，
//!   足够覆盖 RSS 正文。
//! - [`sanitize_filename`]：清理标题中的非法文件名字符并限长。
//! - [`export_to_vault`]：拼装 YAML frontmatter + 可选 AI 摘要 + 正文，并在 Vault 目录下落盘，
//!   返回人类可读的成功消息。
//!
//! # 设计意图
//! - **正文格式**：RSS 正文是 HTML 片段，Obsidian 虽能渲染 HTML，但原生 Markdown 才是 Vault 的
//!   一等公民（标题层级、列表、代码块、链接都能被图谱与搜索识别），故导出时转为 Markdown。
//! - **原文 + 译文同文件（可选）**：原文始终导出（`content` 优先，回退 `summary`）；当用户开启
//!   `obsidian_use_translation` 且文章已有 AI 译文时，译文以独立的「译文」节一并写入同一篇笔记，
//!   便于对照阅读。译文缺失时只导出原文，保证笔记不为空。
//! - **图片本地化（本期未做）**：计划 6.2 标注图片本地化为可选，本期保留原文 http(s) 链接，
//!   不下载图片到 Vault——避免大文件 IO 与网络依赖，后续可加开关开启。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use sqlx::SqlitePool;

use crate::db::{Article, Feed, Setting};

/// 导出所需的设置键（与前端 settings 表单、数据库 settings 表一一对应）
const KEY_VAULT_PATH: &str = "obsidian_vault_path";
const KEY_EXPORT_FOLDER: &str = "obsidian_export_folder";
const KEY_USE_TRANSLATION: &str = "obsidian_use_translation";
/// 导出目录默认值（当用户未配置时，文章落到 Vault 根的 ReadFlow/ 下）
const DEFAULT_EXPORT_FOLDER: &str = "ReadFlow";

/// 把文章导出为 Markdown 笔记写入 Vault
///
/// 流程：读文章 → 读来源名 → 读导出设置 → 转换原文（与可选译文）→ 拼装 Markdown
/// （原文恒有、译文可选）→ 在 `Vault/导出目录` 下创建 `.md` 文件 → 返回成功消息。
///
/// # 参数
/// * `pool` - 数据库连接池
/// * `article_id` - 文章 ID
///
/// # 返回值
/// 成功时返回人类可读的提示（含相对路径）；失败返回错误字符串
///
/// # 错误
/// 文章不存在、Vault 路径未配置或不存在、目录不可写、文件写入失败时返回错误信息
pub async fn export_to_vault(pool: &SqlitePool, article_id: i64) -> Result<String, String> {
    // 1. 读取文章本体（含正文 HTML 与 AI 产物）；缺失即视为非法 ID
    let article = Article::get_by_id(pool, article_id)
        .await?
        .ok_or_else(|| "文章不存在".to_string())?;

    // 2. 来源名：用于 frontmatter 的 source 字段；查不到（源已被删）时回退为「未知来源」
    let source = Feed::name_by_id(pool, article.feed_id)
        .await?
        .unwrap_or_else(|| "未知来源".to_string());

    // 3. 导出设置：Vault 路径必填（否则无法落盘）；导出目录与译文开关有默认值
    let vault_path = Setting::get(pool, KEY_VAULT_PATH)
        .await?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "未配置 Obsidian Vault 路径，请到「设置 → Obsidian」填写".to_string()
        })?;
    let export_folder = Setting::get(pool, KEY_EXPORT_FOLDER)
        .await?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_EXPORT_FOLDER.to_string());
    let use_translation = Setting::get(pool, KEY_USE_TRANSLATION)
        .await?
        .map(|v| v == "1")
        .unwrap_or(false);

    // 4. 转换正文：原文始终导出（content 优先，回退 summary，部分源只提供 summary）；
    //    译文仅在开启开关且文章已有 AI 译文时转换，作为独立的「译文」节追加到同一篇笔记。
    //    —— 即「原文 + 译文」同文件，便于对照阅读。
    let orig_html = if !article.content.is_empty() {
        article.content.clone()
    } else {
        article.summary.clone()
    };
    let orig_md = html_to_markdown(&orig_html);

    // 译文：开启 obsidian_use_translation 且已有译文时转换；否则为 None，build_markdown 据此省略译文节
    let trans_md = if use_translation {
        article
            .ai_translation
            .clone()
            .filter(|s| !s.is_empty())
            .map(|h| html_to_markdown(&h))
    } else {
        None
    };

    // 5. 拼装 Markdown（YAML frontmatter + 可选 AI 摘要 callout + 原文 + 可选译文节）
    let markdown = build_markdown(&article, &source, &orig_md, trans_md.as_deref());

    // 6. 落盘：Vault 根 / 导出目录；目录不存在则创建（用户期望导出到该目录）
    let target_dir = PathBuf::from(&vault_path).join(&export_folder);
    if !target_dir.exists() {
        fs::create_dir_all(&target_dir).map_err(|e| format!("创建导出目录失败：{e}"))?;
    }
    let filename = format!("{}.md", sanitize_filename(&article.title));
    let file_path = target_dir.join(&filename);
    fs::write(&file_path, markdown).map_err(|e| format!("写入文件失败：{e}"))?;

    Ok(format!("已导出到 {}/{}", export_folder, filename))
}

/// 拼装完整 Markdown 文本
///
/// 结构：`---` YAML frontmatter（title / source / url / author / published / tags）
/// + 可选「AI 摘要」callout（Obsidian 原生语法）+ 「原文」节 + 可选「译文」节。
/// 原文恒有、译文可选——二者同处一篇笔记，便于对照阅读。
///
/// # 参数
/// * `article` - 文章数据
/// * `source` - 来源名（已查得或回退值）
/// * `orig_md` - 原文 Markdown（恒有，content 回退 summary 后得到）
/// * `trans_md` - 译文 Markdown（可选：仅当开启译文开关且文章已有译文时传入）
///
/// # 返回值
/// 完整的 `.md` 文件字符串
fn build_markdown(article: &Article, source: &str, orig_md: &str, trans_md: Option<&str>) -> String {
    // YAML frontmatter：值统一用双引号包裹，规避标题/链接里的冒号、井号被误判为 YAML 语法
    let mut fm = String::from("---\n");
    fm.push_str(&format!("title: {}\n", yaml_str(&article.title)));
    fm.push_str(&format!("source: {}\n", yaml_str(source)));
    if !article.link.is_empty() {
        fm.push_str(&format!("url: {}\n", yaml_str(&article.link)));
    }
    if let Some(author) = &article.author {
        if !author.is_empty() {
            fm.push_str(&format!("author: {}\n", yaml_str(author)));
        }
    }
    if let Some(dt) = article.published_at {
        // %Y/%m/%d 需要 Datelike，%H/%M 需要 Timelike，二者均已导入
        fm.push_str(&format!("published: {}\n", dt.format("%Y-%m-%d %H:%M")));
    }
    fm.push_str("tags: [readflow]\n");
    fm.push_str("---\n\n");

    let mut out = fm;
    // 若有 AI 摘要，以 Obsidian callout 形式放在正文前，便于快速回顾
    if let Some(summary) = &article.ai_summary {
        if !summary.is_empty() {
            out.push_str("> [!abstract] AI 摘要\n");
            for line in summary.lines() {
                out.push_str(&format!("> {}\n", line));
            }
            out.push('\n');
        }
    }
    // 原文节：恒有
    out.push_str("## 原文\n\n");
    out.push_str(orig_md.trim());
    out.push('\n');
    // 译文节：可选（开启开关且已有译文时追加）
    if let Some(tm) = trans_md {
        if !tm.trim().is_empty() {
            out.push_str("\n\n## 译文\n\n");
            out.push_str(tm.trim());
            out.push('\n');
        }
    }
    out
}

/// 把字符串转成 YAML 安全的双引号字面量
///
/// 转义反斜杠与双引号（其余字符保留），再裹一层双引号，避免值里出现冒号/井号破坏 frontmatter。
///
/// # 参数
/// * `s` - 原始值
///
/// # 返回值
/// 包裹后的字面量，例如 `标题: "Hello: World"`
fn yaml_str(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "'");
    format!("\"{}\"", escaped)
}

/// 清理文件名中的非法字符并限长
///
/// 替换各平台文件系统禁止的字符（`/ \ : * ? " < > |`）为下划线，换行/制表转空格，
/// 末尾去空白，超长截断到 120 字符；结果为空时回退为 `untitled`。
///
/// # 参数
/// * `name` - 原始标题
///
/// # 返回值
/// 可用的文件名主体（不含扩展名）
fn sanitize_filename(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            '\n' | '\r' | '\t' => ' ',
            c => c,
        })
        .collect();
    let trimmed = replaced.trim();
    let truncated: String = trimmed.chars().take(120).collect();
    if truncated.is_empty() {
        "untitled".to_string()
    } else {
        truncated
    }
}

/// 把（已清洗的）HTML 片段转换为 Markdown 文本
///
/// 采用零额外依赖的迷你解析器（见 [`Parser`]）：字节扫描识别标签，对常见块级
/// （`h1-6 / p / div / blockquote / ul / ol / li / pre / br / hr`）与行内标签
/// （`a / img / strong / em / code / del`）做对应 Markdown 转换，其余标签递归其子节点、
/// 丢弃标签本身只保留文本。转换后压缩多余空行，保证笔记可读。
///
/// # 参数
/// * `html` - 后端清洗后的 HTML 正文片段
///
/// # 返回值
/// 对应的 Markdown 文本（可能为空，调用方已做回退）
pub fn html_to_markdown(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    let mut p = Parser {
        bytes: html.as_bytes(),
        pos: 0,
    };
    let mut out = String::new();
    p.parse_children(&mut out, None, false);
    clean_blank_lines(&mut out);
    out.trim().to_string()
}

/// 迷你 HTML 解析器（字节扫描 + 递归下降）
///
/// 不追求完整 HTML5 规范，只覆盖 RSS 正文常见的标签集合；对畸形嵌套做尽量的容错
/// （未匹配的闭合标签直接忽略，不影响后续内容）。
struct Parser<'a> {
    /// 待解析的 HTML 字节序列（HTML 标签均为 ASCII，文本段单独做 UTF-8 解码）
    bytes: &'a [u8],
    /// 当前扫描位置（字节下标）
    pos: usize,
}

impl<'a> Parser<'a> {
    /// 从 `pos` 起查找字节 `b` 的**绝对**下标
    fn find(&self, b: u8) -> Option<usize> {
        self.bytes[self.pos..]
            .iter()
            .position(|&c| c == b)
            .map(|i| self.pos + i)
    }

    /// 判断从 `pos` 起的剩余内容是否以 `s` 开头
    fn peek(&self, s: &str) -> bool {
        self.bytes[self.pos..].starts_with(s.as_bytes())
    }

    /// 解析当前位置（应位于 `<`）的一个标签
    ///
    /// 处理三类标签：自闭合（`/>`）、闭合（`</name>`）、普通开始标签。
    /// 返回「是否为闭合/自闭合标签」「标签名（小写）」「属性表（键已转小写）」，
    /// 并把 `pos` 推进到该标签之后。
    fn parse_tag(&mut self) -> (bool, String, HashMap<String, String>) {
        let mut closing = false;
        let mut self_closing = false;
        // 跳过 '<'，自闭合检测在属性阶段进行
        self.pos += 1;
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'/' {
            closing = true;
            self.pos += 1;
        }
        // 读取标签名（允许字母数字与连字符）
        let start = self.pos;
        while self.pos < self.bytes.len() && is_name_char(self.bytes[self.pos]) {
            self.pos += 1;
        }
        let name = std::str::from_utf8(&self.bytes[start..self.pos])
            .unwrap_or("")
            .to_ascii_lowercase();

        let mut attrs = HashMap::new();
        if !closing {
            // 解析属性，直到遇到 '>'（普通结束）或 '/>'（自闭合）
            loop {
                while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                    self.pos += 1;
                }
                if self.pos >= self.bytes.len() {
                    break;
                }
                if self.bytes[self.pos] == b'>' {
                    self.pos += 1;
                    break;
                }
                if self.bytes[self.pos] == b'/' {
                    self_closing = true;
                    self.pos += 1;
                    if self.pos < self.bytes.len() && self.bytes[self.pos] == b'>' {
                        self.pos += 1;
                    }
                    break;
                }
                // 属性名
                let an = self.pos;
                while self.pos < self.bytes.len()
                    && self.bytes[self.pos] != b'='
                    && !self.bytes[self.pos].is_ascii_whitespace()
                    && self.bytes[self.pos] != b'>'
                    && self.bytes[self.pos] != b'/'
                {
                    self.pos += 1;
                }
                let attr_name = std::str::from_utf8(&self.bytes[an..self.pos])
                    .unwrap_or("")
                    .to_ascii_lowercase();
                while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                    self.pos += 1;
                }
                // 属性值（可选，可能引号包裹或裸值）
                let mut val = String::new();
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                    self.pos += 1;
                    while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                        self.pos += 1;
                    }
                    if self.pos < self.bytes.len() {
                        let q = self.bytes[self.pos];
                        if q == b'"' || q == b'\'' {
                            self.pos += 1;
                            let vs = self.pos;
                            while self.pos < self.bytes.len() && self.bytes[self.pos] != q {
                                self.pos += 1;
                            }
                            val = std::str::from_utf8(&self.bytes[vs..self.pos])
                                .unwrap_or("")
                                .to_string();
                            if self.pos < self.bytes.len() {
                                self.pos += 1;
                            }
                        } else {
                            let vs = self.pos;
                            while self.pos < self.bytes.len()
                                && !self.bytes[self.pos].is_ascii_whitespace()
                                && self.bytes[self.pos] != b'>'
                            {
                                self.pos += 1;
                            }
                            val = std::str::from_utf8(&self.bytes[vs..self.pos])
                                .unwrap_or("")
                                .to_string();
                        }
                    }
                }
                if !attr_name.is_empty() {
                    attrs.insert(attr_name, val);
                }
            }
        } else {
            // 闭合标签：消费到 '>'
            if let Some(g) = self.find(b'>') {
                self.pos = g + 1;
            } else {
                self.pos = self.bytes.len();
            }
        }
        (closing || self_closing, name, attrs)
    }

    /// 递归解析一组兄弟节点，直到遇到 `stop` 指定的闭合标签或到达末尾
    ///
    /// `ordered` 表示当前是否处于 `<ol>` 上下文，用于给 `li` 生成 `1.` 序号（否则 `-`）。
    /// 文本段先解码 HTML 实体再追加到 `out`。
    ///
    /// # 参数
    /// * `out` - 输出缓冲
    /// * `stop` - 遇到同名闭合标签即结束（当前元素的结束边界）
    /// * `ordered` - 当前列表是否为有序列表
    fn parse_children(&mut self, out: &mut String, stop: Option<&str>, ordered: bool) {
        let mut li_index = 0usize;
        loop {
            if self.pos >= self.bytes.len() {
                break;
            }
            // 先把 '<' 之前的正文文本输出（解码实体）
            if let Some(lt) = self.find(b'<') {
                if lt > self.pos {
                    let text = std::str::from_utf8(&self.bytes[self.pos..lt]).unwrap_or("");
                    out.push_str(&decode_entities(text));
                }
                self.pos = lt;

                // 注释：跳过到 "-->"
                if self.peek("<!--") {
                    if let Some(e) = find_sub(&self.bytes[self.pos..], b"-->") {
                        self.pos += e + 3;
                    } else if let Some(g) = self.find(b'>') {
                        self.pos = g + 1;
                    } else {
                        self.pos = self.bytes.len();
                    }
                    continue;
                }
                // 声明 / 处理指令（<!doctype <?xml）：跳过到 '>'
                if self.peek("<!") || self.peek("<?") {
                    if let Some(g) = self.find(b'>') {
                        self.pos = g + 1;
                    } else {
                        self.pos = self.bytes.len();
                    }
                    continue;
                }

                let (closing, name, attrs) = self.parse_tag();
                // 自闭合或空元素（br / hr / img 等）：直接输出，不递归
                if closing && !is_void_tag(&name) {
                    // 普通闭合标签：匹配 stop 则结束本层，否则忽略（容错）
                    if let Some(stop) = stop {
                        if name == stop {
                            return;
                        }
                    }
                    continue;
                }
                if is_void_tag(&name) {
                    emit_void(&name, &attrs, out);
                    continue;
                }

                // 容器元素：按标签类型输出前后缀并递归子节点
                match name.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        let level = name[1..].parse::<usize>().unwrap_or(1);
                        out.push_str("\n\n");
                        out.push_str(&"#".repeat(level));
                        out.push(' ');
                        let mut inner = String::new();
                        self.parse_children(&mut inner, Some(&name), false);
                        out.push_str(inner.trim());
                        out.push_str("\n\n");
                    }
                    "p" | "div" => {
                        out.push_str("\n\n");
                        self.parse_children(out, Some(&name), false);
                    }
                    "blockquote" => {
                        let mut inner = String::new();
                        self.parse_children(&mut inner, Some(&name), false);
                        out.push('\n');
                        for line in inner.lines() {
                            out.push_str(&format!("> {}\n", line));
                        }
                    }
                    "ul" => {
                        out.push('\n');
                        self.parse_children(out, Some(&name), false);
                        out.push('\n');
                    }
                    "ol" => {
                        out.push('\n');
                        self.parse_children(out, Some(&name), true);
                        out.push('\n');
                    }
                    "li" => {
                        if ordered {
                            li_index += 1;
                            out.push_str(&format!("{}. ", li_index));
                        } else {
                            out.push_str("- ");
                        }
                        self.parse_children(out, Some(&name), false);
                        out.push('\n');
                    }
                    "pre" => {
                        let mut inner = String::new();
                        self.parse_children(&mut inner, Some(&name), false);
                        out.push_str(&format!("\n\n```\n{}\n```\n\n", inner.trim()));
                    }
                    "a" => {
                        let href = attrs.get("href").cloned().unwrap_or_default();
                        let mut inner = String::new();
                        self.parse_children(&mut inner, Some(&name), false);
                        out.push_str(&format!("[{}]({})", inner.trim(), href));
                    }
                    "strong" | "b" => {
                        out.push_str("**");
                        self.parse_children(out, Some(&name), false);
                        out.push_str("**");
                    }
                    "em" | "i" => {
                        out.push('*');
                        self.parse_children(out, Some(&name), false);
                        out.push('*');
                    }
                    "code" => {
                        out.push('`');
                        self.parse_children(out, Some(&name), false);
                        out.push('`');
                    }
                    "del" | "s" => {
                        out.push_str("~~");
                        self.parse_children(out, Some(&name), false);
                        out.push_str("~~");
                    }
                    // 其它标签（span / table / figure 等）：递归子节点，丢弃标签保留文本
                    _ => {
                        self.parse_children(out, Some(&name), false);
                    }
                }
            } else {
                // 已无标签：输出剩余文本并结束
                let text = std::str::from_utf8(&self.bytes[self.pos..]).unwrap_or("");
                out.push_str(&decode_entities(text));
                self.pos = self.bytes.len();
                break;
            }
        }
    }
}

/// 判断字符是否为合法标签名字符（字母数字 / 连字符）
fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b':'
}

/// 在字节切片中查找子串的相对下标（找不到返回 None）
fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// 常见空/void 元素：无闭合标签或自闭合，出现即就地输出
fn is_void_tag(name: &str) -> bool {
    matches!(
        name,
        "br" | "hr" | "img" | "input" | "meta" | "link" | "source" | "area" | "base" | "col"
            | "embed" | "param" | "track" | "wbr"
    )
}

/// 输出空元素的 Markdown 对应物（仅关心的几个，其余忽略）
fn emit_void(name: &str, attrs: &HashMap<String, String>, out: &mut String) {
    match name {
        "br" => out.push('\n'),
        "hr" => out.push_str("\n\n---\n\n"),
        "img" => out.push_str(&format!(
            "![{}]({})",
            attrs.get("alt").cloned().unwrap_or_default(),
            attrs.get("src").cloned().unwrap_or_default()
        )),
        _ => {}
    }
}

/// 解码 HTML 实体（命名 + 十进制/十六进制数值）
///
/// 覆盖导出场景常见的 `&amp; &lt; &gt; &quot; &apos; &nbsp;` 以及 `&#NN;` / `&#xHH;` 数值实体；
/// 无法识别的实体原样保留，避免把畸形转义吞掉。
///
/// # 参数
/// * `s` - 含实体的文本段
///
/// # 返回值
/// 解码后的文本
fn decode_entities(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            if let Some(semi) = chars[i..].iter().position(|&c| c == ';') {
                let ent: String = chars[i + 1..i + semi].iter().collect();
                if let Some(rep) = map_entity(&ent) {
                    out.push_str(&rep);
                    i = i + semi + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 把单个实体名/数值映射为替换字符（见 [`decode_entities`]）
fn map_entity(ent: &str) -> Option<String> {
    match ent {
        "amp" => Some("&".to_string()),
        "lt" => Some("<".to_string()),
        "gt" => Some(">".to_string()),
        "quot" => Some("\"".to_string()),
        "apos" => Some("'".to_string()),
        "nbsp" => Some(" ".to_string()),
        other => {
            // 数值实体：&#NN（十进制）或 &#xHH（十六进制）
            if let Some(num) = other.strip_prefix('#') {
                let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    num.parse::<u32>().ok()
                };
                if let Some(c) = code.and_then(char::from_u32) {
                    return Some(c.to_string());
                }
            }
            None
        }
    }
}

/// 压缩 Markdown 中过多的连续空行（3 行及以上压成 2 行），并去除行尾空白
///
/// # 参数
/// * `text` - 待清理文本（原地修改）
fn clean_blank_lines(text: &mut String) {
    let mut result = String::with_capacity(text.len());
    let mut blank = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank += 1;
            // 最多保留一个空行（即连续空行压成一行）
            if blank <= 1 {
                result.push('\n');
            }
        } else {
            blank = 0;
            result.push_str(line.trim_end());
            result.push('\n');
        }
    }
    *text = result;
}
