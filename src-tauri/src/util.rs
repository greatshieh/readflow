//! 通用文本安全工具（HTML / URL 转义）
//!
//! 研究报告会生成自包含 HTML 并写入磁盘（Obsidian / 应用目录），
//! 事件的事实、证据、来源 URL 来自外部 RSS 与 AI 输出，必须经转义与
//! scheme 校验，防止 XSS 与 `javascript:` 之类的危险链接注入。

/// 转义用于 HTML 上下文的特殊字符
///
/// 覆盖 `< > & " '` 及控制字符（U+0000–U+001F，除合法空白外），
/// 防止证据/来源文本中的标签或脚本片段破坏文档结构或被浏览器执行。
///
/// # 参数
/// * `s` - 待转义的原始字符串
///
/// # 返回值
/// 可安全嵌入 HTML 的文本
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            // 控制字符（含 NUL）一律剔除，避免注入或截断
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// 校验并归一化来源 URL，仅放行 http / https
///
/// 来源链接来自 RSS 的 `link` 字段，可能是 `javascript:`、`data:` 等危险 scheme；
/// 仅当以 `http://` 或 `https://` 开头时才原样返回，否则回退到 `#`（noop）。
///
/// # 参数
/// * `url` - 原始 URL 字符串
///
/// # 返回值
/// 安全 URL；不安全时返回 `"#"`
pub fn sanitize_url(url: &str) -> String {
    let t = url.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        "#".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_neutralizes_angle_brackets_and_quotes() {
        let input = "<script>alert('x')</script> \"&\"";
        let got = html_escape(input);
        assert!(!got.contains('<'));
        assert!(!got.contains('>'));
        assert!(!got.contains('\''));
        assert!(got.contains("&lt;script&gt;"));
        assert!(got.contains("&quot;"));
        assert!(got.contains("&amp;"));
    }

    #[test]
    fn html_escape_strips_control_chars() {
        let input = "a\u{0000}b\u{001f}c";
        let got = html_escape(input);
        assert_eq!(got, "abc");
    }

    #[test]
    fn sanitize_url_allows_http_https() {
        assert_eq!(sanitize_url("https://example.com/a"), "https://example.com/a");
        assert_eq!(sanitize_url("http://example.com"), "http://example.com");
    }

    #[test]
    fn sanitize_url_blocks_dangerous_schemes() {
        assert_eq!(sanitize_url("javascript:alert(1)"), "#");
        assert_eq!(sanitize_url("data:text/html,<script>"), "#");
        assert_eq!(sanitize_url("  "), "#");
    }
}
