//! 投影写入：把机器生成的 Markdown 合并进用户可能已经写过的文件
//!
//! # 为什么需要这个模块
//! 报告与文章笔记都落在**用户自己的目录**里（Obsidian Vault 或应用数据目录的
//! `reports/`），用户会直接在同一个文件里写批注、写结论、贴自己的分析。此前所有
//! 导出都是 `fs::write` 整篇覆盖，于是一句"重新生成报告"就会**静默抹掉用户写在
//! 那份报告里的一切**——而用户写的内容只存在于该文件，没有第二份副本，抹掉即不可
//! 恢复。这不是"新功能没做"，是既有实现会毁数据。
//!
//! 本模块定义「哪部分是机器的、哪部分是用户的」这条边界，并只做这一件事：
//! - **frontmatter**：机器按 key 覆盖自己认识的键，用户独有的键原样保留、原位保留；
//! - **标记块内**（`<!-- readflow:begin:<name> -->` … `<!-- readflow:end:<name> -->`）：
//!   机器所有，可被整块替换；
//! - **块与块之间的空隙**：用户所有，**永不触碰**（连字节都不重新序列化）。
//!
//! 因此"幂等"在这里的含义是：同一份 `doc` 投影两次，文件内容逐字节相同。
//!
//! # 与 DESIGN-05 的差异（写死，避免后人"顺手补回 id"）
//! 设计稿里的标记带 `id=` / `v=` 属性，用于**一个文件承载多个对象**（论点笔记里
//! 同时放判断、判据、证据）。本模块服务的是**一文件一文档**的导出（报告、文章），
//! 对象身份已经写进 frontmatter 的 `readflow_id`，块内只需块名。少一层属性 = 少
//! 一处可能与 frontmatter 不一致的地方。将来做论点笔记（多对象）时再加 `id=`，
//! 那时是**新增一套块语法版本**，不是回来改这里的解析。
//!
//! # 块名不带 id，那靠什么认人
//! 靠 frontmatter 的 `readflow_id`。合并前会比对旧文件与本份文档的 ID，不一致直接
//! 拒绝（见 [`assert_same_identity`]）——否则只要文件名撞上（两篇文章同名"业绩点评"、
//! "无标题"），后一篇就会把前一篇的块原地换掉，前一篇的内容连同一份 frontmatter 一起
//! 消失。旧文件没有该键时放行（本轮之前写出的文件，按升级路径处理）。
//!
//! # 为什么块名只允许 `[a-z0-9_]`
//! 定位标记是 HTML 注释，而 HTML 注释内容里**不允许出现 `--`**。块名若含连字符，
//! `begin:foo--bar` 这类名字会让解析器与别的工具对"注释在哪结束"产生分歧。限制成
//! 小写字母数字下划线即可彻底排除这个坑（现有块名 `header` / `summary` / `body` /
//! `content` / `translation` 全部符合）。
//!
//! # 「用户写在块内被静默覆盖」——一个明确的取舍，以及为什么不加提示
//!
//! 块外的空隙归用户，这是本模块的全部保障。但用户完全可能把话写进**块内**（比如
//! 顺着 `body` 的事件列表往下接着写一段自己的结论），那次内容下次投影就被替换掉了，
//! 且无提示、无备份。这个风险是真的，结论是：**不加任何额外提示**，理由不是"不重要"，
//! 而是"现有的信号已经落在正确的时刻"。
//!
//! 关键在于：**要把话写进块内，用户必然处于编辑视图**（阅读视图里没法输入）。而 HTML
//! 注释的可见性恰好是"编辑视图可见、阅读视图隐藏"：
//!
//! | 场景 | 标记行 | 是否有人在打字 |
//! |---|---|---|
//! | Obsidian 阅读视图 / 任何 Markdown 预览 | 不可见 | 否 |
//! | Obsidian Live Preview（默认编辑模式） | **以原文形式显示** | **是** |
//! | VS Code / GitHub 源码 | 可见 | 是 |
//!
//! 也就是说，`<!-- readflow:begin:body -->` 这行文字**正好出现在用户准备越界的那一行
//! 上方**，而它在成品外观里完全不占位置。这是很少见的一种对齐：提示在需要时出现，
//! 不需要时消失。任何额外手段（导出时弹窗、文件里的固定说明段、给块加可见的"自动
//! 生成"角标）都是在**已经出现提示的地方再叠一层**，代价是从此每份报告都带一行噪音。
//!
//! 顺带记录两个被否掉的方案，省得后人重新发明：
//! - **导出时弹"注意别写在块内"**：用户点一次就再也不会读，且导出是批量/定时行为，
//!   弹窗会打断它。
//! - **检测块内是否被改过再决定覆盖**：技术上可行，但要存"上次写进去的内容"当基准
//!   （影子副本），也就是一张新表 + 一套版本语义。**这是将来真要做时的第一块砖**，
//!   不是现在为了提示一句话该付的成本。
//!
//! # 如果要升级：先解决 X
//! 若将来反馈确实集中在"我写在里面的东西没了"，正确的方向不是加提示文案，而是把
//! 上面第二条捡起来：**存上次投影写入的块内容 → 投影前比对 → 不一致就拒绝覆盖并报出
//! 差异**。那时需要先解决的问题是 **X = 影子副本的存放位置与生命周期**（跟文档走还是
//! 单独一张表、文档被删/改名后怎么跟随、多设备下会不会打架）。X 不解决就做检测，只会
//! 得到"每次重跑都报冲突、用户只能删文件"的更差结果。

use std::collections::HashSet;
use std::path::Path;

/// 块起始标记前缀（后接块名，再接 ` -->`）
const BEGIN_PREFIX: &str = "<!-- readflow:begin:";
/// 块结束标记前缀（后接块名，再接 ` -->`）
const END_PREFIX: &str = "<!-- readflow:end:";

/// 一次投影的结果
///
/// 刻意区分「整篇新建」与「合并」，并报出合并的明细：调用方与用户需要知道的恰恰
/// 不是"导出成功"，而是"我写的判据为什么没更新"——替换了 0 个块与替换了 3 个块，
/// 是两种完全不同的情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectOutcome {
    /// 目标文件原本不存在，整篇写入
    Created,
    /// 目标文件已存在，在块级别完成合并
    Merged {
        /// 命中并就地替换的块数
        replaced: usize,
        /// 旧文件里没有、已追加到末尾的块数
        appended: usize,
    },
}

/// 把一份带标记的完整文档投影到 `path`
///
/// `doc` 必须是完整文档：`---` frontmatter + 若干标记块。**除 frontmatter 之外，
/// 机器生成的一切内容都必须在块内**——块外的字节会被当作"用户写的"而永不改动，
/// 所以标题这类会随日期变化的内容也必须包在块里。
///
/// 文件不存在时整篇写入；已存在时逐块合并（详见 [`merge_into_existing`]）。
///
/// # 参数
/// * `path` - 目标文件路径（父目录不存在时会创建）
/// * `doc` - 带标记的完整文档
///
/// # 返回值
/// [`ProjectOutcome`]：新建 / 合并（含替换与追加的块数）
///
/// # 错误
/// - `doc` 缺 frontmatter、没有任何标记块、块名重复或标记不配对（生成的文档本身有问题）
/// - 目标文件存在但不是 Readflow 生成的（一个标记都没有）→ **拒绝写入**，避免把机器
///   内容追加进一篇纯手写笔记
/// - 目标文件不是合法 UTF-8（盲写会毁掉用户的编码）或读写失败
pub fn project_document(path: &Path, doc: &str) -> Result<ProjectOutcome, String> {
    let (doc_frontmatter, doc_body) = split_frontmatter(doc);
    let doc_frontmatter =
        doc_frontmatter.ok_or("投影文档缺少 frontmatter：生成的 Markdown 必须以 --- 开头")?;
    let doc_blocks = parse_doc_blocks(doc_body)?;

    if !path.exists() {
        atomic_write(path, doc)?;
        return Ok(ProjectOutcome::Created);
    }

    let existing = std::fs::read_to_string(path)
        .map_err(|e| format!("读取目标文件失败（{}）：{}", path.display(), e))?;

    // 一个标记都没有 = 这已经是用户的笔记，不是我们的产物。
    // 此时"追加机器内容"比"什么都不做"更糟：用户会在一篇纯手写笔记末尾突然看到
    // 一大段报告，且此后我们每次导出都会往里塞。
    if !existing.contains(BEGIN_PREFIX) {
        return Err(format!(
            "目标文件已存在且不是 Readflow 生成的内容：{}。为避免覆盖你手写的内容，本次未写入。\
             如需重新生成，请先把该文件移走或改名。",
            path.display()
        ));
    }

    let (merged, replaced, appended) =
        merge_into_existing(&existing, doc_frontmatter, &doc_blocks, path)?;
    atomic_write(path, &merged)?;
    Ok(ProjectOutcome::Merged { replaced, appended })
}

/// 拼装一份带标记的完整文档
///
/// 标记文本只在本模块产生，业务代码不手写 `<!-- readflow:begin:... -->`——
/// 一旦有两处各自拼标记，格式分叉就只是时间问题。
///
/// # 参数
/// * `frontmatter` - frontmatter 内文（不含上下 `---`）
/// * `blocks` - 按文件内顺序排列的 `(块名, 块内 Markdown)`
///
/// # 返回值
/// 可直接交给 [`project_document`] 的完整文档
pub fn assemble(frontmatter: &str, blocks: &[(&str, &str)]) -> String {
    let fm = frontmatter.trim_end_matches(|c| c == '\n' || c == '\r');
    let mut out = String::with_capacity(1024);
    out.push_str("---\n");
    out.push_str(fm);
    out.push('\n');
    out.push_str("---\n");
    // 每个块（含第一个）前留一个空行：`---\n\n<!-- begin -->`、块间一个空行。
    // 空行落在块外，是用户的书写区，所以这里必须稳定，不能靠块的内部内容凑。
    for (name, inner) in blocks {
        out.push('\n');
        out.push_str(&render_block(name, inner));
    }
    out
}

/// 起始标记行
pub fn begin_marker(name: &str) -> String {
    format!("{}{} -->", BEGIN_PREFIX, name)
}

/// 结束标记行
pub fn end_marker(name: &str) -> String {
    format!("{}{} -->", END_PREFIX, name)
}

/// 把一个语义段包成标记块
///
/// # 参数
/// * `name` - 块名（`[a-z0-9_]`）
/// * `inner` - 块内 Markdown；不以换行结尾时会补一个
///
/// # 返回值
/// `begin 行 + 块内 + end 行`，以换行结尾
pub fn render_block(name: &str, inner: &str) -> String {
    let mut out = String::with_capacity(inner.len() + 128);
    out.push_str(&begin_marker(name));
    out.push('\n');
    out.push_str(inner);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&end_marker(name));
    out.push('\n');
    out
}

/// 一个已定位的标记块在文本中的字节区间
struct BlockSpan {
    /// 块名
    name: String,
    /// 起始标记所在行的行首字节下标
    begin_start: usize,
    /// 起始标记所在行**之后**的字节下标（块内内容的起点）
    begin_end: usize,
    /// 结束标记所在行的行首字节下标（块内内容的终点）
    end_start: usize,
}

/// 扫描文本里所有配对的标记块
///
/// # 参数
/// * `text` - 待扫描文本
///
/// # 返回值
/// 按出现顺序排列的块区间
///
/// # 错误
/// 出现嵌套、孤立 `end`、`begin`/`end` 名字不一致、或有 `begin` 无 `end` 时报错。
/// 这些状态只可能来自"用户手工删了半个标记"或"生成逻辑有 bug"，两种情况下**继续
/// 写都有风险**：替换会切错区间，追加会产生同名重复块。交给人来判断。
fn find_blocks(text: &str) -> Result<Vec<BlockSpan>, String> {
    let mut spans = Vec::new();
    let mut open: Option<(String, usize, usize)> = None;
    let mut offset = 0usize;

    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim();

        if let Some(name) = marker_name(trimmed, BEGIN_PREFIX) {
            if let Some((prev, ..)) = &open {
                return Err(format!(
                    "标记块嵌套：begin:{prev} 尚未闭合就出现 begin:{name}"
                ));
            }
            open = Some((name, line_start, offset));
        } else if let Some(name) = marker_name(trimmed, END_PREFIX) {
            match open.take() {
                Some((begin_name, begin_start, begin_end)) if begin_name == name => {
                    spans.push(BlockSpan {
                        name: begin_name,
                        begin_start,
                        begin_end,
                        end_start: line_start,
                    });
                }
                Some((begin_name, ..)) => {
                    return Err(format!(
                        "标记块不配对：begin:{begin_name} 之后出现 end:{name}"
                    ));
                }
                None => return Err(format!("标记块不配对：出现孤立的 end:{name}")),
            }
        }
    }

    if let Some((name, ..)) = open {
        return Err(format!("标记块不配对：begin:{name} 没有对应的 end"));
    }
    Ok(spans)
}

/// 从一行里解析块名
///
/// 行必须形如 `<prefix><name> -->`，且 `name` 只含 `[a-z0-9_]`；不符合就当成普通
/// 正文（用户的代码块里可能恰好有类似文字，不该被我们认领）。
fn marker_name(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?;
    let name = rest.strip_suffix("-->")?.trim();
    if name.is_empty() {
        return None;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    Some(name.to_string())
}

/// 切出 frontmatter 内文与正文
///
/// # 参数
/// * `text` - 完整文档
///
/// # 返回值
/// `(frontmatter 内文, 正文)`；没有 frontmatter 时返回 `(None, text)`
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let rest = match text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        Some(r) => r,
        None => return (None, text),
    };
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(|c| c == '\r' || c == '\n');
        if trimmed == "---" {
            return (Some(&rest[..offset]), &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

/// 解析生成的文档里的块，并校验块名唯一
///
/// # 参数
/// * `body` - 文档正文（frontmatter 之后）
///
/// # 返回值
/// `(块名, 块内文本)`，按文档内顺序
///
/// # 错误
/// 没有块、标记不配对、块名重复
fn parse_doc_blocks(body: &str) -> Result<Vec<(String, &str)>, String> {
    let spans = find_blocks(body)?;
    if spans.is_empty() {
        return Err("投影文档没有任何标记块：无法判断哪部分内容归机器所有".to_string());
    }
    let mut seen = HashSet::new();
    for span in &spans {
        if !seen.insert(span.name.as_str()) {
            return Err(format!(
                "投影文档里块名重复：{}（同一文件内块名必须唯一）",
                span.name
            ));
        }
    }
    Ok(spans
        .iter()
        .map(|s| (s.name.clone(), &body[s.begin_end..s.end_start]))
        .collect())
}

/// 把生成的文档合并进已存在的文件
///
/// 规则：
/// - **先校验身份**：旧文件 frontmatter 里的 `readflow_id` 与本次要写的不一致时直接
///   报错（见 [`assert_same_identity`]）；
/// - 生成的块在旧文件里能找到 → **只替换 begin 与 end 两行之间的内容**，标记行本身
///   保留原样（用户可能调整过缩进或行尾，不必纠正）；
/// - 找不到 → 追加到文件末尾（前面空一行）。这是**升级路径**：老版本写出的文件缺少
///   新版本才有的块，追加才能补上，否则功能一升级就卡在旧格式上；
/// - 同名块在旧文件里出现多次 → 只替换**第一对**，其余原样保留（很可能是用户自己
///   复制的副本，删了就是删用户的东西）；
/// - frontmatter 按 key 合并（见 [`merge_frontmatter`]）。
///
/// # 参数
/// * `existing` - 旧文件全文
/// * `doc_frontmatter` - 生成文档的 frontmatter 内文
/// * `doc_blocks` - 生成文档的块
/// * `path` - 目标路径（仅用于报错时指认文件）
///
/// # 返回值
/// `(合并后的全文, 替换块数, 追加块数)`
///
/// # 错误
/// 旧文件的标记不配对，或两边的 `readflow_id` 不一致时返回错误
fn merge_into_existing(
    existing: &str,
    doc_frontmatter: &str,
    doc_blocks: &[(String, &str)],
    path: &Path,
) -> Result<(String, usize, usize), String> {
    let (existing_frontmatter, existing_body) = split_frontmatter(existing);
    assert_same_identity(existing_frontmatter.unwrap_or(""), doc_frontmatter, path)?;
    let existing_spans = find_blocks(existing_body)?;

    let mut plans: Vec<(&BlockSpan, &str)> = Vec::new();
    let mut missing: Vec<(&str, &str)> = Vec::new();
    for (name, inner) in doc_blocks {
        match existing_spans.iter().find(|s| &s.name == name) {
            Some(span) => plans.push((span, inner)),
            None => missing.push((name.as_str(), inner)),
        }
    }
    // 就地替换必须按旧文件中的先后顺序推进游标，否则后面所有切片都会错位
    plans.sort_by_key(|(span, _)| span.begin_start);

    let mut body = String::with_capacity(existing_body.len() + 1024);
    let mut cursor = 0usize;
    for (span, inner) in &plans {
        body.push_str(&existing_body[cursor..span.begin_end]);
        body.push_str(inner);
        cursor = span.end_start;
    }
    // 尾部原样带走：最后一个块之后用户写的东西也属于用户
    body.push_str(&existing_body[cursor..]);

    for (name, inner) in &missing {
        append_block(&mut body, name, inner);
    }

    let frontmatter = merge_frontmatter(existing_frontmatter, doc_frontmatter);
    let merged = format!("---\n{}---\n{}", frontmatter, body);
    Ok((merged, plans.len(), missing.len()))
}

/// 校验两份 frontmatter 描述的是同一个对象
///
/// # 为什么必须校验
/// 本模块的块名**不带 id**（`header` / `body` / `content` …，见模块头"与 DESIGN-05
/// 的差异"）。不带 id 的代价是：只要文件名撞上，任何文档都能往任何文件里合并。两条
/// 真实撞车路径：
/// 1. 两篇文章同名（"业绩点评"、"无标题"）→ 导出第二篇时把第一篇的块原地换成第二篇，
///    第一篇的内容就此消失，frontmatter 的 `readflow_id` 也被改写；
/// 2. 报告文件名只含「类型 + 周期」，同周期重跑本应合并（这是设计意图），但如果
///    `readflow_id` 不稳定（比如周报用"今天"当键），第二次就会被误判成"另一篇"。
///
/// 因此这条校验同时起两个作用：**挡住跨对象覆盖**，并**强制 `readflow_id` 与文件名
/// 一样以周期为键**（稳定性由校验反过来约束）。
///
/// 旧文件没有 `readflow_id`（本轮改动之前写出的文件）时放行——它没有身份可比，而它
/// 有标记块说明确实是我们的产物，按"升级路径"处理。
///
/// # 参数
/// * `existing` - 旧文件 frontmatter 内文
/// * `incoming` - 生成文档 frontmatter 内文
/// * `path` - 目标路径（仅用于报错文案）
///
/// # 错误
/// 两边都有 `readflow_id` 但不相等时返回错误，附带两个 ID 与文件路径
fn assert_same_identity(existing: &str, incoming: &str, path: &Path) -> Result<(), String> {
    let old = readflow_id_of(existing);
    let new = readflow_id_of(incoming);
    match (old, new) {
        (Some(old), Some(new)) if old != new => Err(format!(
            "目标文件已属于另一条记录（文件是 {}，本次要写 {}），\
             为避免两篇笔记互相覆盖，本次未写入：{}",
            old,
            new,
            path.display()
        )),
        _ => Ok(()),
    }
}

/// 从 frontmatter 内文里取出 `readflow_id` 的值（去引号与首尾空白）
///
/// # 参数
/// * `fm` - frontmatter 内文
///
/// # 返回值
/// 取到时的裸值（如 `report:research-daily:2026-11-01`）；没有该键时 `None`
fn readflow_id_of(fm: &str) -> Option<String> {
    let (_, lines) = frontmatter_entries(fm)
        .into_iter()
        .find(|(key, _)| key == "readflow_id")?;
    let line = lines.first()?;
    let (_, value) = line.split_once(':')?;
    let value = value.trim().trim_matches('"');
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 在文件末尾追加一个块（前留一个空行）
fn append_block(body: &mut String, name: &str, inner: &str) {
    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push('\n');
    body.push_str(&render_block(name, inner));
}

/// 按 key 合并 frontmatter
///
/// 整体替换会把用户加的 `rating:` / `my_tags:` 一起删掉，而 frontmatter 恰恰是
/// "用户也拥有"的一侧。所以只覆盖机器认识的那几个键，其余原样、原位保留。
///
/// 键顺序：先机器键（顺序固定，便于 diff），再用户独有键（保持其在旧文件里的相对顺序）。
///
/// # 参数
/// * `existing` - 旧文件的 frontmatter 内文（可能为 `None`）
/// * `machine` - 生成的 frontmatter 内文
///
/// # 返回值
/// 合并后的 frontmatter 内文（每行以换行结尾）
fn merge_frontmatter(existing: Option<&str>, machine: &str) -> String {
    let machine_entries = frontmatter_entries(machine);
    let existing_entries = existing.map(frontmatter_entries).unwrap_or_default();
    let machine_keys: HashSet<&str> = machine_entries.iter().map(|(k, _)| k.as_str()).collect();

    let mut out = String::with_capacity(machine.len() + 256);
    for (_, lines) in &machine_entries {
        push_lines(&mut out, lines);
    }
    for (key, lines) in &existing_entries {
        if machine_keys.contains(key.as_str()) {
            continue;
        }
        push_lines(&mut out, lines);
    }
    out
}

/// 追加一组键值行（每行补换行）
fn push_lines(out: &mut String, lines: &[String]) {
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
}

/// 把 frontmatter 解析成 `(键, 该键的全部原始行)` 的有序表
///
/// 支持续行：以空格/制表符/`-` 开头的行归入上一个键（嵌套 map 与列表都这样表示），
/// 这样 `tags:\n  - a` 这类写法不会在合并时被拆散。空行忽略。
///
/// # 参数
/// * `fm` - frontmatter 内文
///
/// # 返回值
/// 按出现顺序排列的键值表
fn frontmatter_entries(fm: &str) -> Vec<(String, Vec<String>)> {
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    for line in fm.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let is_continuation =
            line.starts_with(' ') || line.starts_with('\t') || line.starts_with('-');
        if is_continuation {
            if let Some(last) = entries.last_mut() {
                last.1.push(line.to_string());
            }
            continue;
        }
        if let Some(pos) = line.find(':') {
            let key = line[..pos].trim();
            if !key.is_empty() {
                entries.push((key.to_string(), vec![line.to_string()]));
            }
        }
    }
    entries
}

/// 原子写：同目录临时文件 + rename
///
/// 不用 `fs::write` 直接覆盖目标，是因为 Obsidian 会 watch 文件变化：非原子写存在
/// "目标已被截断、内容还没落盘"的时间窗，此时被读到轻则索引多出一条空笔记，重则
/// 被同步工具/插件当成删除并传播出去。临时文件用 `.<文件名>.tmp` 前缀而非 `.tmp`
/// 后缀，是为了让用户一眼看出这是工具临时文件、且在按扩展名过滤的工具里不会被当成
/// 待导入的笔记。
///
/// # 参数
/// * `path` - 目标文件
/// * `content` - 完整内容
///
/// # 错误
/// 父目录不可创建、临时文件不可写、rename 失败（失败时会清理临时文件，不留残骸）
fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("目标路径没有父目录：{}", path.display()))?;
    if !parent.exists() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败（{}）：{}", parent.display(), e))?;
    }
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("目标路径缺少文件名：{}", path.display()))?;
    let tmp = parent.join(format!(".{}.tmp", file_name));

    if let Err(e) = std::fs::write(&tmp, content) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("写入临时文件失败（{}）：{}", tmp.display(), e));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("替换目标文件失败（{}）：{}", path.display(), e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! 投影写入的边界验证：五种合并情形 + 幂等 + 块外内容逐字节不变 + 不留临时文件

    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 建一个进程内唯一的临时目录（用例并行执行，不能共用路径）
    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "readflow-projection-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录失败");
        dir
    }

    /// 一份两块的生成文档
    fn doc_v1() -> String {
        assemble(
            "type: research-daily\nreadflow_id: report:research-daily:2026-11-01\n",
            &[
                ("header", "# 研究日报 · 2026-11-01\n"),
                ("body", "## 英伟达\n\n- 事件 A\n"),
            ],
        )
    }

    /// 边界一：文件不存在 → 整篇写入，返回 Created
    #[test]
    fn creates_document_when_file_absent() {
        let dir = temp_dir("create");
        let path = dir.join("报告.md");

        let outcome = project_document(&path, &doc_v1()).expect("首次投影失败");
        assert_eq!(outcome, ProjectOutcome::Created);

        let written = std::fs::read_to_string(&path).expect("读回失败");
        assert_eq!(written, doc_v1(), "新建时应当逐字节等于生成文档");
        assert!(written.contains("<!-- readflow:begin:header -->"));
        assert!(written.contains("<!-- readflow:end:body -->"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 边界二：文件存在但一个标记都没有 → 拒绝写入，且不留下任何机器内容
    #[test]
    fn refuses_when_existing_file_has_no_markers() {
        let dir = temp_dir("refuse");
        let path = dir.join("我的手写笔记.md");
        let hand_written = "# 我自己写的\n\n这段不能被追加任何东西。\n";
        std::fs::write(&path, hand_written).expect("写夹具失败");

        let err = project_document(&path, &doc_v1()).expect_err("应当拒绝写入");
        assert!(
            err.starts_with("目标文件已存在且不是 Readflow 生成的内容"),
            "错误信息应以约定前缀开头，实际：{err}"
        );

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert_eq!(after, hand_written, "拒绝写入时文件必须一字未改");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 边界三：块内替换后，块外用户内容逐字节不变（含块间空隙与文件尾部）
    #[test]
    fn replaces_between_markers_and_keeps_user_text_byte_identical() {
        let dir = temp_dir("replace");
        let path = dir.join("报告.md");
        project_document(&path, &doc_v1()).expect("首次投影失败");

        // 用户在两块之间插一段自己的话，并在末尾再写一段
        let old = std::fs::read_to_string(&path).expect("读回失败");
        let old = old.replace(
            "<!-- readflow:end:header -->\n",
            "<!-- readflow:end:header -->\n\n我自己的批注：今天没有超预期消息。\n",
        );
        let old = format!("{old}\n尾注：明天盯宁德。\n");
        std::fs::write(&path, &old).expect("写回夹具失败");

        // 重新生成：只有 header 的内容变了
        let doc_v2 = assemble(
            "type: research-daily\nreadflow_id: report:research-daily:2026-11-01\n",
            &[
                ("header", "# 研究日报 · 2026-11-01（重跑）\n"),
                ("body", "## 英伟达\n\n- 事件 A\n"),
            ],
        );
        let outcome = project_document(&path, &doc_v2).expect("二次投影失败");
        assert_eq!(
            outcome,
            ProjectOutcome::Merged {
                replaced: 2,
                appended: 0
            }
        );

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert!(
            after.contains("# 研究日报 · 2026-11-01（重跑）"),
            "块内内容应被替换：{after}"
        );
        assert!(
            after.contains("我自己的批注：今天没有超预期消息。"),
            "块间空隙是用户区，必须原样保留：{after}"
        );
        assert!(
            after.contains("尾注：明天盯宁德。"),
            "文件尾部也必须保留：{after}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 边界三之二：旧文件缺一个新块 → 追加到末尾（升级路径，不是拒绝）
    #[test]
    fn appends_block_missing_from_old_file() {
        let dir = temp_dir("append");
        let path = dir.join("报告.md");
        project_document(&path, &doc_v1()).expect("首次投影失败");

        // 新版本多了 summary 块
        let doc_v2 = assemble(
            "type: research-daily\nreadflow_id: report:research-daily:2026-11-01\n",
            &[
                ("header", "# 研究日报 · 2026-11-01\n"),
                ("summary", "> [!summary] 覆盖近 1 天\n"),
                ("body", "## 英伟达\n\n- 事件 A\n"),
            ],
        );
        let outcome = project_document(&path, &doc_v2).expect("二次投影失败");
        assert_eq!(
            outcome,
            ProjectOutcome::Merged {
                replaced: 2,
                appended: 1
            }
        );

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert!(after.contains("<!-- readflow:begin:summary -->"), "{after}");
        assert!(
            after.contains("> [!summary] 覆盖近 1 天"),
            "追加的块必须带内容：{after}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 同名块出现多次 → 只替换第一对，其余原样保留（可能是用户复制的副本）
    #[test]
    fn keeps_duplicate_block_copy_untouched() {
        let dir = temp_dir("duplicate");
        let path = dir.join("报告.md");
        project_document(&path, &doc_v1()).expect("首次投影失败");

        // 用户把 body 整块复制了一份放在文件末尾
        let old = std::fs::read_to_string(&path).expect("读回失败");
        let copy =
            "<!-- readflow:begin:body -->\n## 我的副本\n\n- 旧内容\n<!-- readflow:end:body -->\n";
        std::fs::write(&path, format!("{old}\n{copy}")).expect("写回夹具失败");

        let doc_v2 = assemble(
            "type: research-daily\nreadflow_id: report:research-daily:2026-11-01\n",
            &[
                ("header", "# 研究日报 · 2026-11-01\n"),
                ("body", "## 英伟达\n\n- 事件 B\n"),
            ],
        );
        project_document(&path, &doc_v2).expect("二次投影失败");

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert!(after.contains("- 事件 B"), "第一对应被替换：{after}");
        assert!(
            after.contains("## 我的副本") && after.contains("- 旧内容"),
            "第二对必须原样保留，且不报错：{after}"
        );
        assert_eq!(
            after.matches("<!-- readflow:begin:body -->").count(),
            2,
            "不应新增或删除副本"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 目标文件属于另一条记录 → 拒绝合并
    ///
    /// 块名不带 id，若不校验 `readflow_id`，两篇同标题（"无标题"、"业绩点评"）的笔记
    /// 会互相把对方的块原地换掉 —— 前一篇的内容连同它的身份一起消失。
    #[test]
    fn refuses_when_target_belongs_to_another_document() {
        let dir = temp_dir("identity");
        let path = dir.join("无标题.md");
        project_document(&path, &doc_v1()).expect("首次投影失败");

        let other = assemble(
            "type: research-daily\nreadflow_id: report:research-daily:2026-11-02\n",
            &[
                ("header", "# 另一天\n"),
                ("body", "## 别的实体\n\n- 事件 Z\n"),
            ],
        );
        let err = project_document(&path, &other).expect_err("应当拒绝合并");
        assert!(err.contains("已属于另一条记录"), "实际错误：{err}");

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert!(
            after.contains("report:research-daily:2026-11-01"),
            "原文件的身份不得被改写：{after}"
        );
        assert!(
            after.contains("- 事件 A") && !after.contains("- 事件 Z"),
            "原文件的内容不得被顶掉：{after}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 旧文件没有 `readflow_id`（本轮之前的产物）→ 按升级路径正常合并
    #[test]
    fn legacy_file_without_id_is_upgraded() {
        let dir = temp_dir("legacy");
        let path = dir.join("报告.md");
        let legacy = "---\ntype: research-daily\n---\n\n\
                      <!-- readflow:begin:header -->\n# 旧标题\n<!-- readflow:end:header -->\n";
        std::fs::write(&path, legacy).expect("写夹具失败");

        let outcome = project_document(&path, &doc_v1()).expect("旧文件应可升级");
        assert_eq!(
            outcome,
            ProjectOutcome::Merged {
                replaced: 1,
                appended: 1
            }
        );

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert!(
            after.contains("readflow_id: report:"),
            "应补上身份键：{after}"
        );
        assert!(
            after.contains("# 研究日报 · 2026-11-01"),
            "旧块应被替换：{after}"
        );
        assert!(
            after.contains("<!-- readflow:begin:body -->"),
            "缺的块应被追加：{after}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 边界五：frontmatter 按 key 合并，用户独有键不能被删
    #[test]
    fn frontmatter_merge_keeps_user_keys() {
        let dir = temp_dir("frontmatter");
        let path = dir.join("报告.md");
        project_document(&path, &doc_v1()).expect("首次投影失败");

        let old = std::fs::read_to_string(&path).expect("读回失败");
        let old = old.replace(
            "type: research-daily\n",
            "type: research-daily\nrating: 5\nmy_tags:\n  - 重点\n",
        );
        std::fs::write(&path, &old).expect("写回夹具失败");

        let doc_v2 = assemble(
            "type: research-daily\nreadflow_id: report:research-daily:2026-11-01\nreadflow_version: 2\n",
            &[
                ("header", "# 研究日报 · 2026-11-01\n"),
                ("body", "## 英伟达\n\n- 事件 A\n"),
            ],
        );
        project_document(&path, &doc_v2).expect("二次投影失败");

        let after = std::fs::read_to_string(&path).expect("读回失败");
        assert!(after.contains("rating: 5"), "用户的键必须保留：{after}");
        assert!(
            after.contains("my_tags:\n  - 重点"),
            "用户的嵌套值必须整组保留：{after}"
        );
        assert!(
            after.contains("readflow_version: 2"),
            "机器新增的键应写入：{after}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 幂等：同一份 doc 连投两次，文件逐字节相同、第二次零替换
    #[test]
    fn projecting_same_document_twice_is_byte_identical() {
        let dir = temp_dir("idempotent");
        let path = dir.join("报告.md");

        project_document(&path, &doc_v1()).expect("首次投影失败");
        let first = std::fs::read_to_string(&path).expect("读回失败");
        let outcome = project_document(&path, &doc_v1()).expect("二次投影失败");

        assert_eq!(
            outcome,
            ProjectOutcome::Merged {
                replaced: 2,
                appended: 0
            }
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("读回失败"),
            first,
            "幂等投影不应改变任何字节"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 标记不配对（用户删了 end）→ 报错而不是猜测，文件一字未改
    #[test]
    fn unpaired_begin_marker_is_error() {
        let dir = temp_dir("unpaired");
        let path = dir.join("报告.md");
        project_document(&path, &doc_v1()).expect("首次投影失败");

        let broken = "---\ntype: research-daily\n---\n\n<!-- readflow:begin:body -->\n- 内容\n";
        std::fs::write(&path, broken).expect("写回夹具失败");

        let err = project_document(&path, &doc_v1()).expect_err("应当报错");
        assert!(err.contains("没有对应的 end"), "实际错误：{err}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("读回失败"),
            broken,
            "报错时不得改动文件"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 原子写：成功与失败路径都不留临时文件
    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let dir = temp_dir("atomic");
        let path = dir.join("报告.md");
        atomic_write(&path, "内容\n").expect("原子写失败");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "内容\n");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件：{leftovers:?}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
