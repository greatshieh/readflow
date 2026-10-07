//! OPML 订阅列表的解析与生成
//!
//! # 职责
//! OPML（Outline Processor Markup Language）是 RSS 阅读器之间交换订阅列表的通用格式。
//! 本模块只负责两端转换，不涉及网络与数据库：
//! - [`parse_opml`]：把 OPML 文本解析为 `(标题, 订阅URL)` 列表，供上层批量入库；
//! - [`export_opml`]：把已订阅的 [`Feed`] 列表反向序列化为标准 OPML 文本。
//!
//! # 设计意图
//! - **格式健壮**：OPML 由各阅读器自由生成，常出现"分类文件夹也是 `<outline>`"的嵌套结构
//!   （如 Feedly、Inoreader）。本解析器遇到没有 `xmlUrl` 的 `<outline>` 视为文件夹并跳过，
//!   只收集真正带 `xmlUrl` 的叶子节点，因此能正确导入任意来源的导出文件。
//! - **容忍缺省**：部分 OPML 用 `text` 而非 `title` 承载名称，部分两者皆无。
//!   名称缺失时回退到 URL，保证导入后条目始终可读。
//! - **入库交给 db 层**：本模块不碰 SQL，解析结果只是普通的 `(String, String)`，
//!   由 [`crate::db::Feed::import_batch`] 负责去重与写入，符合"数据访问集中在 db 模块"的约定。
//!
//! # 安全
//! 解析来自用户文件的外部内容，但本模块只读取 `text` / `title` / `xmlUrl` 三个属性，
//! 不执行任何外部输入，因此无需额外消毒；导出时则对名称/URL 做 XML 转义，
//! 避免订阅源名称中的特殊字符破坏生成文件的合法性。

use crate::db::{Feed, Folder};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashMap;

/// 从 OPML 中解析出的一条订阅源
#[derive(Debug, Clone)]
pub struct OpmlFeed {
    /// 展示名称（来自 text 或 title 属性，缺失时回退为 URL）
    pub title: String,
    /// RSS/Atom 订阅地址（来自 xmlUrl 属性）
    pub xml_url: String,
}

/// 解析 OPML 文本，提取所有真实订阅源
///
/// 采用 quick-xml 的流式事件读取（而非一次性加载整棵 DOM），内存占用与文件大小无关，
/// 即便超大订阅列表也不会爆内存。遇到 XML 结构错误时返回 `Err`，由命令层转成用户提示。
///
/// # 参数
/// * `content` - OPML 原始文本（由前端读取本地文件后传入）
///
/// # 返回值
/// 所有带 `xmlUrl` 的订阅源列表（已跳过分类文件夹）
///
/// # 错误
/// XML 无法解析（如损坏或不完整）时返回错误信息
pub fn parse_opml(content: &str) -> Result<Vec<OpmlFeed>, String> {
    let mut reader = Reader::from_str(content);
    // 去掉标签间空白，避免把缩进误当成文本内容（OPML 的 <outline> 是无文本标签）
    reader.trim_text(true);

    let mut feeds = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            // 自闭合或带闭合标签的 <outline> 都会触发 Start 事件；
            // 叶子订阅源可能是 <outline .../>（Empty）也可能写成 <outline></outline>（Start+End）。
            // 只在 Start/Empty 时收集一次，End 事件忽略，避免重复。
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"outline" {
                    if let Some(feed) = extract_outline(&e) {
                        feeds.push(feed);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("OPML 解析失败: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(feeds)
}

/// 从一个 `<outline>` 元素中抽取订阅源（若它是带 xmlUrl 的叶子节点）
///
/// # 参数
/// * `e` - 当前 outline 元素的起始/空标签事件
///
/// # 返回值
/// 成功识别为订阅源时返回 `Some(OpmlFeed)`；文件夹（无 xmlUrl）或非订阅节点返回 `None`
fn extract_outline(e: &quick_xml::events::BytesStart) -> Option<OpmlFeed> {
    let mut xml_url = String::new();
    let mut title = String::new();

    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"xmlUrl" => xml_url = String::from_utf8_lossy(&attr.value).to_string(),
            // text 优先作为名称；title 仅作回退，避免覆盖已读到的 text
            b"text" => title = String::from_utf8_lossy(&attr.value).to_string(),
            b"title" if title.is_empty() => title = String::from_utf8_lossy(&attr.value).to_string(),
            _ => {}
        }
    }

    // 没有 xmlUrl 的 outline 视为分类文件夹，跳过
    if xml_url.is_empty() {
        return None;
    }
    // 名称缺失时用 URL 兜底，保证导入后条目可读
    if title.is_empty() {
        title = xml_url.clone();
    }

    Some(OpmlFeed { title, xml_url })
}

/// 将订阅源列表序列化为标准 OPML 2.0 文本
///
/// 输出结构固定为 `head/title` + `body/outline`，每个订阅源一行；
/// 名称与 URL 均经过 XML 转义，确保含 `&` `<` 等特殊字符的源也能被正确导回。
///
/// `category` 属性填该源所属**文件夹名**，未归入文件夹（`folder_id = 0`）时省略该属性：
/// 文件夹是本应用唯一的分组语义，导出的结构因此与用户在侧边栏看到的一致。
///
/// # 参数
/// * `feeds` - 当前所有订阅源
/// * `folders` - 全部文件夹（用于把 `folder_id` 还原为可读名称）
///
/// # 返回值
/// 完整的 OPML 文档字符串（UTF-8 编码声明）
pub fn export_opml(feeds: &[Feed], folders: &[Folder]) -> String {
    let folder_names: HashMap<i64, &str> =
        folders.iter().map(|f| (f.id, f.name.as_str())).collect();

    let mut body = String::new();
    for f in feeds {
        let name = escape_xml(&f.name);
        let url = escape_xml(&f.url);
        // 未归入文件夹时不写 category：空属性没有语义，省略比 category="" 更干净
        let category = match folder_names.get(&f.folder_id) {
            Some(folder) => format!(" category=\"{}\"", escape_xml(folder)),
            None => String::new(),
        };
        body.push_str(&format!(
            "    <outline text=\"{name}\" title=\"{name}\" type=\"rss\" xmlUrl=\"{url}\"{category}/>\n"
        ));
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <opml version=\"2.0\">\n\
           <head>\n\
             <title>ReadFlow Subscriptions</title>\n\
           </head>\n\
           <body>\n\
         {body}\
           </body>\n\
         </opml>\n"
    )
}

/// 对字符串做最小 XML 转义，防止属性值破坏生成文件的合法性
///
/// 只转义 XML 属性里会造成结构破坏的五个字符：`&` `<` `>` `"` `'`。
///
/// # 参数
/// * `s` - 原始文本
///
/// # 返回值
/// 转义后的安全文本
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
