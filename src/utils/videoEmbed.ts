/**
 * 视频内嵌播放的 URL 工具（YouTube）
 *
 * # 职责
 * 从文章链接中识别 YouTube 视频，并构造**唯一被信任**的嵌入播放地址
 * （`https://www.youtube-nocookie.com/embed/{id}`），供正文栏渲染内嵌播放器。
 *
 * # 为什么从 link 提取，而不是信正文 content
 * 文章正文入库前要过 Rust 侧 ammonia 白名单（`src-tauri/src/rss.rs` 的
 * `sanitize_html`），`iframe` 不在白名单——RSSHub YouTube 源 description 里的
 * 嵌入代码在**入库时**就被剥掉，且数据库无原始 HTML 备份列，渲染层再也拿不回来。
 * 而 `article.link` 字段全程不经净化（rss.rs 解析原样透传 → db.rs batch_insert
 * 直存），RSSHub YouTube 源保留完整的 `watch?v={id}` 形态。因此视频识别的
 * 唯一可靠信息源是 link，不是 content。
 *
 * # 安全约束
 * - 只有通过本文件白名单正则的 URL 才会被提取 ID；构造出的
 *   `https://www.youtube-nocookie.com/embed/{id}` 是唯一允许进入 iframe 的地址，
 *   RSS 原文里的任何第三方 URL 都不得直接进 iframe；
 * - ID 字符类与长度双重收窄（恒 11 位 `[A-Za-z0-9_-]`）：既防
 *   `?v=xxxx&list=yyy` 之类的尾缀被贪婪吞进 ID，也让提取结果无法夹带
 *   路径、查询串或片段——ID 之外的 URL 成分一律不进入播放地址。
 */

/**
 * 受信任的 YouTube URL 形态白名单（命中即止，顺序即优先级）
 *
 * 每条都做了三重收窄：
 * 1. 协议仅 `http` / `https`；
 * 2. 主机仅 `youtube.com`（`www.` / `m.` / 裸域）与 `youtu.be`（裸域 / `www.`）；
 * 3. ID 严格 11 位 `[A-Za-z0-9_-]`，后随边界（`&` `#` `?` `/` 或结尾）——
 *    取满 11 位即止，第 12 位仍是 ID 字符则整体判负，防尾缀误匹配。
 *
 * 用 `RegExp` 构造器而非字面量：把 ID 模式提为常量，四条规则共享同一份约束。
 */
const YOUTUBE_PATTERNS: readonly RegExp[] = [
  // 观看页：youtube.com/watch?v=ID（v 不一定是首个参数，如 ?list=PL&v=ID；
  // 前置换 `(?:[^#]*&)?` 要求以 & 分隔，故 ?foo=v=ID 这类伪造不会误提取）
  new RegExp(
    String.raw`^https?://(?:www\.|m\.)?youtube\.com/watch\?(?:[^#]*&)?v=([A-Za-z0-9_-]{11})(?:[&#]|$)`,
    "i",
  ),
  // 短链：youtu.be/ID（分享链接的常见形态，含 query/fragment 时也要能提取）
  new RegExp(String.raw`^https?://(?:www\.)?youtu\.be/([A-Za-z0-9_-]{11})(?:[?#]|$)`, "i"),
  // 既有嵌入页：youtube.com/embed/ID（RSSHub 附件 / 已嵌入口的形态）
  new RegExp(
    String.raw`^https?://(?:www\.|m\.)?youtube\.com/embed/([A-Za-z0-9_-]{11})(?:[?#/]|$)`,
    "i",
  ),
  // Shorts：youtube.com/shorts/ID
  new RegExp(
    String.raw`^https?://(?:www\.|m\.)?youtube\.com/shorts/([A-Za-z0-9_-]{11})(?:[?#/]|$)`,
    "i",
  ),
]

/**
 * 从文章链接提取 YouTube 视频 ID
 *
 * @param url - 文章链接（RSS `link` 字段原样值，未经任何净化）
 * @returns 11 位视频 ID；不属受信形态时返回 `null`
 */
export function extractYouTubeId(url: string): string | null {
  const trimmed = url.trim()
  if (!trimmed) return null
  for (const pattern of YOUTUBE_PATTERNS) {
    const matched = pattern.exec(trimmed)
    if (matched) return matched[1]
  }
  return null
}

/**
 * 构造受信的 YouTube 内嵌播放地址
 *
 * 固定走 `youtube-nocookie.com`（隐私增强模式：播放前不落跟踪 cookie），
 * 不附加任何来自原始 URL 的参数——原始 URL 的 query 一律丢弃，
 * 播放地址的成分只有协议、主机与提取出的 ID。
 *
 * @param url - 文章链接（RSS `link` 字段原样值，未经任何净化）
 * @returns `https://www.youtube-nocookie.com/embed/{id}`；非 YouTube 链接返回 `null`
 */
export function youtubeEmbedUrl(url: string): string | null {
  const videoId = extractYouTubeId(url)
  return videoId ? `https://www.youtube-nocookie.com/embed/${videoId}` : null
}
