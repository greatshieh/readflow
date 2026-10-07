//! 系统字体枚举：扫描本机已安装字体，供「界面字体 / 内容字体」下拉选择使用。
//!
//! # 职责
//! 1. 按平台列出标准字体目录（Linux 的 fontconfig 目录、macOS 的系统与用户字体库、
//!    Windows 的 Fonts 目录）；
//! 2. 解析字体文件自身的 `name` 表，取出真实字体族名（含中文本地化名）；
//! 3. 去重、排序后以 [`FontFamily`] 返回，结果在进程内缓存。
//!
//! # 设计意图
//! - **为什么自己解析而不是靠文件名**：Windows 的字体文件名是 `msyh.ttc`（微软雅黑）、
//!   `simsun.ttc`（宋体）这类内部代号，从文件名不可能推断出族名；只有读 `name` 表才能
//!   拿到跨平台一致的族名，也才能被 CSS `font-family` 正确命中。
//! - **为什么不引第三方字体库**：本项目对依赖很克制，而所需能力只是"读出 name 表里的
//!   族名字段"。SFNT 容器格式自 1990 年代起就稳定，手写约两百行即可覆盖
//!   ttf / otf / ttc / otc 与可变字体，且解析部分是纯函数、可脱离真实字体文件单元测试。
//! - **为什么只读文件片段**：CJK 字体单个 10–20MB，若整文件读入，几百个字体文件会立刻
//!   吃掉数百 MB 内存。这里先读文件头（表目录所在位置，只需几百字节），再 `seek` 到
//!   `name` 表局部读取。
//!
//! # 覆盖范围说明
//! 不处理 WOFF / WOFF2（浏览器专用封装，系统字体目录不会出现）。可变字体无需特殊处理：
//! 同一族的各字重共享族名，去重后只留一条。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio::sync::OnceCell;

// ─── 容器格式常量 ─────────────────────────────────────────────────────────────

/// 字体集合（.ttc / .otc）的文件头标识
const TTC_TAG: &[u8] = b"ttcf";
/// `name` 表的表标签
const NAME_TABLE_TAG: &[u8] = b"name";
/// SFNT 版本：TrueType 轮廓（.ttf）
const SFNT_TRUETYPE: &[u8] = &[0x00, 0x01, 0x00, 0x00];
/// SFNT 版本：CFF 轮廓（.otf）
const SFNT_CFF: &[u8] = b"OTTO";
/// SFNT 版本：Apple 旧式 TrueType
const SFNT_APPLE_TRUE: &[u8] = b"true";

// ─── name 表语义常量 ─────────────────────────────────────────────────────────

/// 平台 ID：Unicode
const PLATFORM_UNICODE: u16 = 0;
/// 平台 ID：Macintosh
const PLATFORM_MAC: u16 = 1;
/// 平台 ID：Windows
const PLATFORM_WINDOWS: u16 = 3;
/// 名称 ID：传统字体族名
const NAME_ID_FAMILY: u16 = 1;
/// 名称 ID：排版族名（字重 / 斜体等子族共用同一族名，优先采用）
const NAME_ID_TYPOGRAPHIC_FAMILY: u16 = 16;
/// 语言 ID：英语（美国）
const LANG_EN_US: u16 = 0x0409;
/// 语言 ID：简体中文
const LANG_ZH_CN: u16 = 0x0804;
/// 语言 ID：繁体中文（中国台湾）
const LANG_ZH_TW: u16 = 0x0404;
/// 语言 ID：繁体中文（中国香港）
const LANG_ZH_HK: u16 = 0x0C04;

// ─── 解析边界（异常文件保护）──────────────────────────────────────────────────

/// 单张表的表目录最多扫描的表数量（正常字体远小于此值）
const MAX_TABLES: usize = 512;
/// 字体集合最多解析的成员数
const MAX_COLLECTION: usize = 1024;
/// 单次读取 `name` 表的上限
const MAX_NAME_TABLE: u32 = 16 * 1024 * 1024;
/// 文件头读取长度：需同时覆盖 ttc 头与各成员的表目录，64KB 远超实际所需
const HEADER_PROBE: usize = 64 * 1024;
/// 目录递归深度上限，避免符号链接成环或异常深层结构
const MAX_DIR_DEPTH: usize = 6;

/// 系统字体族
#[derive(Debug, Clone, Serialize)]
pub struct FontFamily {
    /// 字体族名：用于 CSS `font-family`（优先英文名，跨字体环境下命中率最高）
    pub name: String,
    /// 展示名：中文本地化名与英文名不同时形如「微软雅黑（Microsoft YaHei）」
    pub label: String,
}

/// 字体扫描结果缓存
///
/// 字体安装情况在应用运行期间视为不变，扫描一次即可；同时避免用户每次打开设置页
/// 都重新遍历磁盘上数百个字体文件。
static CACHE: OnceCell<Vec<FontFamily>> = OnceCell::const_new();

/// 获取系统字体列表（首次调用扫描磁盘，之后走缓存）
///
/// # 返回值
/// 已去重并排序的字体族列表；扫描全部失败时返回空列表（前端下拉退化为仅预设项）
pub async fn system_fonts() -> Vec<FontFamily> {
    CACHE
        .get_or_init(|| async {
            // 扫描是纯阻塞磁盘 IO（数百个文件），放进阻塞线程池，
            // 避免占住异步运行时的工作线程导致其它命令排队。
            tokio::task::spawn_blocking(scan_system_fonts)
                .await
                .unwrap_or_default()
        })
        .await
        .clone()
}

/// 扫描本机全部字体目录并汇总字体族
///
/// # 返回值
/// 按族名（小写）排序的字体族列表
pub fn scan_system_fonts() -> Vec<FontFamily> {
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in font_dirs() {
        collect_font_files(&dir, &mut files, 0);
    }

    // 以"小写族名"为键去重：同一族的不同字重、不同格式（ttf 与 otf 并存）都会命中同一条
    let mut unique: HashMap<String, FontFamily> = HashMap::new();
    for file in files {
        for raw in families_in_file(&file) {
            if let Some(family) = raw.into_family() {
                unique.entry(family.name.to_lowercase()).or_insert(family);
            }
        }
    }

    let mut families: Vec<FontFamily> = unique.into_values().collect();
    // 排序保证每次打开下拉的顺序稳定（ASCII 名在前，仅中文名的排在其后）
    families.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    families
}

/// 列出当前平台的字体目录（仅返回真实存在的目录）
///
/// # 返回值
/// 已过滤掉不存在路径的目录列表
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library/Fonts"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        // 用户级安装的字体（"为当前用户安装"选项会落在该目录）
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    }

    // 其余（Linux 及类 Unix）：fontconfig 的标准目录
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }

    dirs.retain(|dir| dir.is_dir());
    dirs
}

/// 递归收集字体文件
///
/// # 参数
/// * `dir` - 当前目录
/// * `out` - 结果累加容器
/// * `depth` - 当前递归深度（超过 [`MAX_DIR_DEPTH`] 即停止下探）
fn collect_font_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > MAX_DIR_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_font_files(&path, out, depth + 1);
        } else if file_type.is_file() && is_font_file(&path) {
            out.push(path);
        }
    }
}

/// 判断路径是否为受支持的字体文件
///
/// # 参数
/// * `path` - 待判断的路径
///
/// # 返回值
/// 扩展名为 ttf / otf / ttc / otc（大小写不敏感）时为 `true`
fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("ttf") | Some("otf") | Some("ttc") | Some("otc")
    )
}

/// 读取单个字体文件中包含的全部字体族
///
/// # 参数
/// * `path` - 字体文件路径
///
/// # 返回值
/// 每个字体（字体集合的每个成员）一条记录；文件不可读或格式不支持时返回空列表
fn families_in_file(path: &Path) -> Vec<RawFamily> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };

    // 读文件头：表目录一定位于文件前部，64KB 足以覆盖 ttc 头与各成员的表目录
    let mut header = vec![0u8; HEADER_PROBE];
    let Ok(read) = file.read(&mut header) else {
        return Vec::new();
    };
    header.truncate(read);

    let mut families = Vec::new();
    for font_start in font_offsets(&header) {
        let Some((offset, length)) = name_table_range(&header, font_start) else {
            continue;
        };
        let length = length.min(MAX_NAME_TABLE) as usize;
        let mut table = vec![0u8; length];
        if file.seek(SeekFrom::Start(offset as u64)).is_err() {
            continue;
        }
        if file.read_exact(&mut table).is_err() {
            continue;
        }
        if let Some(family) = parse_family_names(&table) {
            families.push(family);
        }
    }
    families
}

/// 列出文件内各字体的 SFNT 起始偏移
///
/// # 参数
/// * `header` - 文件头字节
///
/// # 返回值
/// 单字体文件返回 `[0]`；字体集合返回各成员偏移；非 SFNT 容器返回空列表
fn font_offsets(header: &[u8]) -> Vec<u32> {
    if header.len() < 12 {
        return Vec::new();
    }
    if &header[0..4] == TTC_TAG {
        // ttc 头：tag(4) version(4) numFonts(4) 之后紧跟 numFonts 个偏移
        let count = u32::from_be_bytes([header[8], header[9], header[10], header[11]]) as usize;
        let mut offsets = Vec::new();
        for index in 0..count.min(MAX_COLLECTION) {
            let at = 12 + index * 4;
            if at + 4 > header.len() {
                break;
            }
            offsets.push(u32::from_be_bytes([
                header[at],
                header[at + 1],
                header[at + 2],
                header[at + 3],
            ]));
        }
        offsets
    } else if is_sfnt_version(&header[0..4]) {
        vec![0]
    } else {
        Vec::new()
    }
}

/// 判断 4 字节标签是否为受支持的 SFNT 版本标识
///
/// # 参数
/// * `tag` - 文件头前 4 字节
///
/// # 返回值
/// TrueType / CFF / Apple 旧式 TrueType 之一时为 `true`
fn is_sfnt_version(tag: &[u8]) -> bool {
    tag == SFNT_TRUETYPE || tag == SFNT_CFF || tag == SFNT_APPLE_TRUE
}

/// 在表目录中定位 `name` 表
///
/// # 参数
/// * `header` - 文件头字节（须包含目标字体的表目录）
/// * `font_start` - 目标字体的 SFNT 起始偏移
///
/// # 返回值
/// `Some((表偏移, 表长度))`；未找到 `name` 表时为 `None`
fn name_table_range(header: &[u8], font_start: u32) -> Option<(u32, u32)> {
    let base = font_start as usize;
    if base + 12 > header.len() {
        return None;
    }
    // SFNT 偏移表：version(4) numTables(2) searchRange(2) entrySelector(2) rangeShift(2)
    let table_count = u16::from_be_bytes([header[base + 4], header[base + 5]]) as usize;
    for index in 0..table_count.min(MAX_TABLES) {
        // 表记录：tag(4) checkSum(4) offset(4) length(4)
        let record = base + 12 + index * 16;
        if record + 16 > header.len() {
            return None;
        }
        if &header[record..record + 4] == NAME_TABLE_TAG {
            let offset = u32::from_be_bytes([
                header[record + 8],
                header[record + 9],
                header[record + 10],
                header[record + 11],
            ]);
            let length = u32::from_be_bytes([
                header[record + 12],
                header[record + 13],
                header[record + 14],
                header[record + 15],
            ]);
            return Some((offset, length));
        }
    }
    None
}

/// 从 `name` 表中挑出英文族名与本地化族名各一条最优记录
///
/// # 参数
/// * `table` - `name` 表字节
///
/// # 返回值
/// `Some(RawFamily)`；表中没有任何可用族名时为 `None`
fn parse_family_names(table: &[u8]) -> Option<RawFamily> {
    if table.len() < 6 {
        return None;
    }
    // name 表头：format(2) count(2) stringOffset(2)；字符串区起点相对表首
    let record_count = u16::from_be_bytes([table[2], table[3]]) as usize;
    let string_base = u16::from_be_bytes([table[4], table[5]]) as usize;

    let mut family = RawFamily::default();
    for index in 0..record_count {
        // 名称记录：platformID(2) encodingID(2) languageID(2) nameID(2) length(2) offset(2)
        let record = 6 + index * 12;
        if record + 12 > table.len() {
            break;
        }
        let platform = u16::from_be_bytes([table[record], table[record + 1]]);
        let language = u16::from_be_bytes([table[record + 4], table[record + 5]]);
        let name_id = u16::from_be_bytes([table[record + 6], table[record + 7]]);

        // 只关心族名：16（排版族名，子族共用）优于 1（传统族名）
        let name_rank = match name_id {
            NAME_ID_TYPOGRAPHIC_FAMILY => 0u32,
            NAME_ID_FAMILY => 1u32,
            _ => continue,
        };

        let length = u16::from_be_bytes([table[record + 8], table[record + 9]]) as usize;
        let offset = u16::from_be_bytes([table[record + 10], table[record + 11]]) as usize;
        let start = string_base + offset;
        if length == 0 || start + length > table.len() {
            continue;
        }
        let Some(text) = decode_name(platform, &table[start..start + length]) else {
            continue;
        };
        let text = text.trim().to_string();
        // 过滤空名与 macOS 的隐藏字体（族名以 . 开头，如 ".SF NS"）
        if text.is_empty() || text.starts_with('.') || text.chars().count() > 128 {
            continue;
        }

        let Some((slot_rank, is_chinese)) = classify_name(platform, language, &text) else {
            continue;
        };
        let rank = name_rank * 100 + slot_rank;
        let slot = if is_chinese {
            &mut family.localized
        } else {
            &mut family.english
        };
        // 数字越小越优先，只保留当前最优
        if slot.as_ref().is_none_or(|(best, _)| rank < *best) {
            *slot = Some((rank, text));
        }
    }

    if family.english.is_none() && family.localized.is_none() {
        None
    } else {
        Some(family)
    }
}

/// 判断一条名称记录该归入哪个槽位，并给出优先级
///
/// 槽位判定优先依据平台与语言 ID；当语言 ID 不可靠（Unicode 平台）时，
/// 退化为"看文本是否含中日韩字符"。优先级用数字表示，越小越优先：
/// Windows > Unicode > Macintosh。
///
/// # 参数
/// * `platform` - 平台 ID（0 = Unicode，1 = Macintosh，3 = Windows）
/// * `language` - 语言 ID（含义随平台变化）
/// * `text` - 已解码的名称文本
///
/// # 返回值
/// `Some((优先级, 是否为本地化名))`；无法归类（如仅日文名）时为 `None`
fn classify_name(platform: u16, language: u16, text: &str) -> Option<(u32, bool)> {
    match platform {
        PLATFORM_WINDOWS => match language {
            // zh-CN 最优先，zh-TW / zh-HK 次之
            LANG_ZH_CN => Some((0, true)),
            LANG_ZH_TW | LANG_ZH_HK => Some((1, true)),
            LANG_EN_US => Some((1, false)),
            _ => None,
        },
        PLATFORM_UNICODE => {
            // Unicode 平台不区分语言，按文本内容归类
            let chinese = has_cjk(text);
            Some((if chinese { 12 } else { 11 }, chinese))
        }
        // Mac 平台：languageID 0 即英语；其余语言多为旧编码，直接跳过
        PLATFORM_MAC => {
            if language == 0 {
                Some((20, false))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 按平台解码名称文本
///
/// # 参数
/// * `platform` - 平台 ID
/// * `bytes` - 名称字符串的原始字节
///
/// # 返回值
/// `Some(文本)`；编码不受支持或字节非法时为 `None`
fn decode_name(platform: u16, bytes: &[u8]) -> Option<String> {
    match platform {
        // Windows 与 Unicode 平台均为 UTF-16BE
        PLATFORM_UNICODE | PLATFORM_WINDOWS => {
            if bytes.len() % 2 != 0 {
                return None;
            }
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units).ok()
        }
        // Mac 平台为单字节编码。Mac Roman 与 Latin-1 在 ASCII 区间完全一致，
        // 而字体族名绝大多数是纯 ASCII，故直接按码位映射；非 ASCII 极端情况忽略
        PLATFORM_MAC => Some(bytes.iter().map(|&byte| byte as char).collect()),
        _ => None,
    }
}

/// 判断文本是否含中日韩字符
///
/// # 参数
/// * `text` - 待判断文本
///
/// # 返回值
/// 含汉字、假名、谚文或全角字符时为 `true`
fn has_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3000..=0x9FFF   // 中日韩标点、假名、汉字
                | 0xAC00..=0xD7AF  // 谚文音节
                | 0xF900..=0xFAFF  // 兼容汉字
                | 0xFF00..=0xFFEF // 全角字符
        )
    })
}

/// 从字体文件 `name` 表中读出的原始族名（英文与本地化各一条最优）
#[derive(Default)]
struct RawFamily {
    /// 英文族名及其优先级
    english: Option<(u32, String)>,
    /// 本地化（中日韩）族名及其优先级
    localized: Option<(u32, String)>,
}

impl RawFamily {
    /// 合并为对外的字体族条目
    ///
    /// CSS 的 `font-family` 用英文名（跨环境命中率最高），展示名优先用中文名并附英文名，
    /// 便于用户在长列表中快速定位。
    ///
    /// # 返回值
    /// 至少有一种族名时返回 `Some(FontFamily)`，否则返回 `None`
    fn into_family(self) -> Option<FontFamily> {
        let english = self.english.map(|(_, text)| text);
        let localized = self.localized.map(|(_, text)| text);

        let name = match (&english, &localized) {
            (Some(en), _) => en.clone(),
            (None, Some(zh)) => zh.clone(),
            (None, None) => return None,
        };
        let label = match (&english, &localized) {
            (Some(en), Some(zh)) if !en.eq_ignore_ascii_case(zh) => format!("{zh}（{en}）"),
            _ => name.clone(),
        };
        Some(FontFamily { name, label })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按 `(platform, language, nameId, text)` 构造一张 name 表
    ///
    /// 用于脱离真实字体文件验证解析逻辑，覆盖"平台优先级 / 名称 ID 优先级 / 编码解码"。
    fn build_name_table(records: &[(u16, u16, u16, &str)]) -> Vec<u8> {
        let mut strings: Vec<u8> = Vec::new();
        // (platform, language, name_id, length, offset)
        let mut metas: Vec<(u16, u16, u16, u16, u16)> = Vec::new();
        for (platform, language, name_id, text) in records {
            let encoded = encode_for_platform(*platform, text);
            metas.push((
                *platform,
                *language,
                *name_id,
                encoded.len() as u16,
                strings.len() as u16,
            ));
            strings.extend_from_slice(&encoded);
        }

        let string_base = 6 + metas.len() * 12;
        let mut table = Vec::new();
        table.extend_from_slice(&0u16.to_be_bytes()); // format
        table.extend_from_slice(&(metas.len() as u16).to_be_bytes()); // count
        table.extend_from_slice(&(string_base as u16).to_be_bytes()); // stringOffset
        for (platform, language, name_id, length, offset) in metas {
            table.extend_from_slice(&platform.to_be_bytes());
            table.extend_from_slice(&1u16.to_be_bytes()); // encodingID
            table.extend_from_slice(&language.to_be_bytes());
            table.extend_from_slice(&name_id.to_be_bytes());
            table.extend_from_slice(&length.to_be_bytes());
            table.extend_from_slice(&offset.to_be_bytes());
        }
        table.extend_from_slice(&strings);
        table
    }

    /// 按平台编码字符串（Windows / Unicode 为 UTF-16BE，Mac 为单字节）
    fn encode_for_platform(platform: u16, text: &str) -> Vec<u8> {
        if platform == PLATFORM_MAC {
            text.as_bytes().to_vec()
        } else {
            text.encode_utf16()
                .flat_map(|unit| unit.to_be_bytes())
                .collect()
        }
    }

    /// 构造一个只含 `name` 表的合法 SFNT 文件字节流
    fn build_sfnt(name_table: &[u8]) -> Vec<u8> {
        let mut file = Vec::new();
        file.extend_from_slice(SFNT_TRUETYPE); // sfntVersion
        file.extend_from_slice(&1u16.to_be_bytes()); // numTables
        file.extend_from_slice(&0u16.to_be_bytes()); // searchRange
        file.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
        file.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        file.extend_from_slice(NAME_TABLE_TAG); // tag
        file.extend_from_slice(&0u32.to_be_bytes()); // checkSum
        file.extend_from_slice(&28u32.to_be_bytes()); // offset（12 字节偏移表 + 16 字节表记录）
        file.extend_from_slice(&(name_table.len() as u32).to_be_bytes()); // length
        file.extend_from_slice(name_table);
        file
    }

    #[test]
    fn 同时解析出英文名与中文本地化名() {
        let table = build_name_table(&[
            (PLATFORM_WINDOWS, LANG_EN_US, NAME_ID_FAMILY, "Microsoft YaHei"),
            (PLATFORM_WINDOWS, LANG_ZH_CN, NAME_ID_FAMILY, "微软雅黑"),
        ]);
        let family = parse_family_names(&table).expect("应解析出族名").into_family().unwrap();
        assert_eq!(family.name, "Microsoft YaHei", "CSS 用名应取英文");
        assert_eq!(family.label, "微软雅黑（Microsoft YaHei）", "展示名应中文优先并附英文");
    }

    #[test]
    fn 排版族名优先于传统族名() {
        // nameID 16 = "Ubuntu"；nameID 1 = "Ubuntu Light"（子族名）。
        // 若不优先 16，会把每个字重都列成一个"字体族"。
        let table = build_name_table(&[
            (PLATFORM_WINDOWS, LANG_EN_US, NAME_ID_FAMILY, "Ubuntu Light"),
            (PLATFORM_WINDOWS, LANG_EN_US, NAME_ID_TYPOGRAPHIC_FAMILY, "Ubuntu"),
        ]);
        let family = parse_family_names(&table).expect("应解析出族名").into_family().unwrap();
        assert_eq!(family.name, "Ubuntu");
    }

    #[test]
    fn windows_英文名优先于_unicode_同名记录() {
        let table = build_name_table(&[
            (PLATFORM_UNICODE, 0, NAME_ID_FAMILY, "Noto Sans"),
            (PLATFORM_WINDOWS, LANG_EN_US, NAME_ID_FAMILY, "Noto Sans"),
        ]);
        let family = parse_family_names(&table).expect("应解析出族名");
        // 两条文本相同，关键是没有把 Unicode 平台的记录误判为中文名
        assert!(family.localized.is_none(), "纯 ASCII 名不应进入本地化槽位");
        // 两条都是 nameID 1（传统族名），故基准分 100：
        // Windows/en-US 记 101，Unicode 记 111，取值小者胜出
        assert_eq!(family.english.unwrap().0, 101, "Windows 平台应胜出");
    }

    #[test]
    fn unicode_平台按文本内容区分中英文() {
        let table = build_name_table(&[
            (PLATFORM_UNICODE, 0, NAME_ID_FAMILY, "Source Han Sans SC"),
            (PLATFORM_UNICODE, 0, NAME_ID_FAMILY, "思源黑体"),
        ]);
        let family = parse_family_names(&table).expect("应解析出族名");
        assert_eq!(family.english.unwrap().1, "Source Han Sans SC");
        assert_eq!(family.localized.unwrap().1, "思源黑体");
    }

    #[test]
    fn 无可用族名时返回_none() {
        // 只有日文平台记录（无法归类）应被整体丢弃
        let table = build_name_table(&[(PLATFORM_WINDOWS, 0x0411, NAME_ID_FAMILY, "メイリオ")]);
        assert!(parse_family_names(&table).is_none());
        // 隐藏字体（macOS 族名以 . 开头）同样丢弃
        let hidden = build_name_table(&[(PLATFORM_MAC, 0, NAME_ID_FAMILY, ".SF NS")]);
        assert!(parse_family_names(&hidden).is_none());
    }

    #[test]
    fn 识别单字体与字体集合的偏移() {
        // 单字体：直接返回偏移 0
        let mut single = Vec::new();
        single.extend_from_slice(SFNT_TRUETYPE);
        single.extend_from_slice(&[0u8; 8]);
        assert_eq!(font_offsets(&single), vec![0]);

        // 字体集合：ttcf 头 + numFonts=2 + 两个成员偏移
        let mut collection = Vec::new();
        collection.extend_from_slice(TTC_TAG);
        collection.extend_from_slice(&0x00010000u32.to_be_bytes());
        collection.extend_from_slice(&2u32.to_be_bytes());
        collection.extend_from_slice(&100u32.to_be_bytes());
        collection.extend_from_slice(&900u32.to_be_bytes());
        assert_eq!(font_offsets(&collection), vec![100, 900]);

        // 非字体数据：返回空
        assert!(font_offsets(b"not a font file at all").is_empty());
    }

    #[test]
    fn 在表目录中定位_name_表() {
        let mut header = Vec::new();
        header.extend_from_slice(SFNT_TRUETYPE);
        header.extend_from_slice(&2u16.to_be_bytes()); // numTables = 2
        header.extend_from_slice(&[0u8; 6]);
        // 第一张表：glyf（应被跳过）
        header.extend_from_slice(b"glyf");
        header.extend_from_slice(&[0u8; 12]);
        // 第二张表：name，偏移 512、长度 64
        header.extend_from_slice(NAME_TABLE_TAG);
        header.extend_from_slice(&0u32.to_be_bytes());
        header.extend_from_slice(&512u32.to_be_bytes());
        header.extend_from_slice(&64u32.to_be_bytes());

        assert_eq!(name_table_range(&header, 0), Some((512, 64)));
        // 偏移越界（字体集合成员指向文件尾之后）时应放弃而不是 panic
        assert_eq!(name_table_range(&header, 4096), None);
    }

    #[test]
    fn 端到端读取真实文件字节流() {
        let table = build_name_table(&[
            (PLATFORM_WINDOWS, LANG_EN_US, NAME_ID_TYPOGRAPHIC_FAMILY, "Test Family"),
            (PLATFORM_WINDOWS, LANG_ZH_CN, NAME_ID_TYPOGRAPHIC_FAMILY, "测试字体"),
        ]);
        let file_bytes = build_sfnt(&table);

        // 写到临时文件后走真实的"读文件头 → 定位 name 表 → seek 读取"路径
        let path = std::env::temp_dir().join(format!("readflow-font-test-{}.ttf", std::process::id()));
        std::fs::write(&path, &file_bytes).expect("写入临时字体文件失败");

        let families = families_in_file(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(families.len(), 1, "单字体文件应只解析出一条记录");
        let family = families.into_iter().next().unwrap().into_family().unwrap();
        assert_eq!(family.name, "Test Family");
        assert_eq!(family.label, "测试字体（Test Family）");
    }

    #[test]
    fn 中日韩字符判定() {
        assert!(has_cjk("思源黑体"));
        assert!(has_cjk("Noto Sans 中文"));
        assert!(!has_cjk("Noto Sans CJK SC"));
    }

    /// 本机字体目录探测：仅用于人工排查（`cargo test -- --ignored`），
    /// 因依赖运行环境而不纳入常规测试
    #[test]
    #[ignore]
    fn 打印本机字体扫描结果() {
        let families = scan_system_fonts();
        println!("扫描到 {} 个字体族", families.len());
        for family in families.iter().take(40) {
            println!("  {}  |  CSS 用名 = {}", family.label, family.name);
        }
    }
}
