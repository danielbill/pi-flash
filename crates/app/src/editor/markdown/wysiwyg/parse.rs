//! pulldown-cmark → 行内 span + 语法标记 folds（doc 字节坐标）。
//!
//! 与渲染器（`super::super` 的 `markdown/mod.rs`）共用同一套 pulldown
//! Options——预览与 Live Preview 解析结果必须一致。
//!
//! **容错策略（024 §6.4）**：语法标记只在**逐字节验证相邻**后才折叠——
//! 范围存疑 → 不折叠（保守露源码），绝不猜。任何 fold 都不得含 `\n`
//! （折叠只在行内，行数必须与原文严格一致，否则行模型错乱）。
//!
//! P0 覆盖：strong（`**`/`__`）、emphasis（`*`/`_`）、行内 code（`` ` ``）、
//! ATX 标题（`# ` 前缀，带围栏感知）。链接/表格单元格样式等 P1 扩。
//!
//! 已知 P0 缺口（样式在、标记露源码，不崩溃不改文档）：
//! - 强调内嵌另一种定界符（`**_a_**`）时外层定界符可能漏折
//! - 引用块内的围栏/标题不做感知（保守漏折）

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::editor::markdown::Style;

/// 行内样式 span（doc 字节坐标，可能跨行——样式可跨行，fold 不可）。
pub struct Span {
    pub range: Range<usize>,
    pub style: Style,
}

/// 语法容器（024 P2 段级 reveal 单位）：一个语法结构（strong/em/行内
/// code/链接/标题前缀/围栏标记）的内容区间与其全部定界 folds——
/// reveal 时整容器同进退（`**粗体**` 光标触碰即整对标记同显，
/// 不会左显右不显）。
pub struct Container {
    /// 内容区间（定界符之外的正文；选区/光标相交判定用）
    pub content: Range<usize>,
    /// 该容器的全部定界 folds（标记字节天然互斥，跨容器不重叠）
    pub folds: Vec<Range<usize>>,
}

/// 平铺全部 folds（升序）——vendor 映射输入。
pub fn flat_folds(containers: &[Container]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = containers
        .iter()
        .flat_map(|c| c.folds.iter().cloned())
        .collect();
    out.sort_by_key(|f| f.start);
    out
}

/// 与渲染器完全一致的解析选项（复制此处以保同步；渲染器改选项时同改）。
fn parse_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_GFM);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_MATH);
    opts
}

/// 一个行内容器（strong/em）的跟踪状态：内容边界 = 定界符折叠的锚点。
struct Frame {
    style: Style,
    first_content: Option<usize>,
    last_content_end: usize,
    /// 本容器已收集的定界 folds（闭合时随 Container 一并产出）
    folds: Vec<Range<usize>>,
}

impl Frame {
    fn new(style: Style) -> Self {
        Self {
            style,
            first_content: None,
            last_content_end: 0,
            folds: Vec::new(),
        }
    }

    /// 定界符需求长度：strong 2、em 1。
    fn demand(&self) -> usize {
        match self.style {
            Style::Bold => 2,
            _ => 1,
        }
    }
}

/// 解析行内 span、语法容器（段级 reveal 单位）与行级字号倍数。
///
/// 返回 `(spans, containers, line_scale)`：容器带 folds（平铺用
/// [`flat_folds`]，整体升序不重叠）；`line_scale` 为 (doc 行号, 倍数)，
/// folds 不含换行 → 行号与 vis 一一对应。
pub fn parse(src: &str) -> (Vec<Span>, Vec<Container>, Vec<(usize, f32)>) {
    let mut spans = Vec::new();
    let mut containers: Vec<Container> = Vec::new();
    let mut line_scale: Vec<(usize, f32)> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // 链接起点（`[` 字节位）；活动期内 Text 产 Link span
    let mut link_open: Option<usize> = None;

    for (ev, range) in Parser::new_ext(src, parse_options()).into_offset_iter() {
        match ev {
            Event::Start(Tag::Strong) => stack.push(Frame::new(Style::Bold)),
            Event::Start(Tag::Emphasis) => stack.push(Frame::new(Style::Italic)),
            Event::Start(Tag::Link { .. }) => link_open = Some(range.start),
            Event::End(TagEnd::Strong | TagEnd::Emphasis) => {
                if let Some(mut frame) = stack.pop() {
                    // 全局已收定界（已闭容器 + 祖先活动 frames）——collect
                    // 跳过且不计入需求（嵌套 `***a***` 外层靠这条拿满）
                    let taken: Vec<Range<usize>> = containers
                        .iter()
                        .flat_map(|c| c.folds.iter().cloned())
                        .chain(stack.iter().flat_map(|f| f.folds.iter().cloned()))
                        .collect();
                    close_frame(&mut frame, src, &taken);
                    if let (Some(first), last) = (frame.first_content, frame.last_content_end) {
                        if last > first {
                            containers.push(Container {
                                content: first..last,
                                folds: std::mem::take(&mut frame.folds),
                            });
                        }
                    }
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(open) = link_open.take() {
                    if let Some(c) = link_container(src, open, &range) {
                        containers.push(c);
                    }
                }
            }
            Event::Text(_) => {
                // 链接文本：Link 样式（与嵌套 bold 等字段级合并）
                if link_open.is_some() {
                    spans.push(Span {
                        range: range.clone(),
                        style: Style::Link,
                    });
                }
                if !stack.is_empty() {
                    let style = combine(&stack);
                    spans.push(Span {
                        range: range.clone(),
                        style,
                    });
                    touch_frames(&mut stack, range.start, range.end);
                }
            }
            Event::Code(_) => {
                if let Some(c) = code_container(src, &range, &mut spans) {
                    containers.push(c);
                    touch_frames(&mut stack, range.start, range.end);
                }
            }
            _ => {}
        }
    }

    scan_headings(src, &mut containers, &mut spans, &mut line_scale);
    (spans, containers, line_scale)
}

/// 链接容器：`[text](url)` → 保 text（Link 样式），折 `[` 与 `](url)`。
/// 逐字节验证（`[` 开头 / `)` 结尾向前找 `]`）——存疑不折（None）。
/// autolink `<http://…>`（无 `[]`）验证失败自然不折，保守露源码。
fn link_container(src: &str, open: usize, end: &Range<usize>) -> Option<Container> {
    let b = src.as_bytes();
    if b.get(open) != Some(&b'[') || end.end > b.len() || end.end == 0 {
        return None;
    }
    let mut folds = vec![open..open + 1];
    let mut content_end = end.end;
    if b[end.end - 1] == b')' {
        if let Some(rb) = b[end.start..end.end].iter().rposition(|&c| c == b']') {
            let rb_abs = end.start + rb;
            folds.push(rb_abs..end.end);
            content_end = rb_abs;
        }
    }
    Some(Container {
        content: open + 1..content_end,
        folds,
    })
}

/// 叶子内容推进所有活动容器的内容边界。
fn touch_frames(stack: &mut [Frame], start: usize, end: usize) {
    for f in stack.iter_mut() {
        if f.first_content.is_none() {
            f.first_content = Some(start);
        }
        f.last_content_end = f.last_content_end.max(end);
    }
}

/// 容器闭合：从内容两缘向外各收集 `demand()` 个相邻定界符字符，
/// 收集结果落进 `frame.folds`（随 Container 产出）。
///
/// 只认 `*` / `_`，撞到其他字符（含 `\n`）即停——天然保证 fold 不跨行。
/// 已在其他 fold 中的位置跳过但**不计入**需求（嵌套 `***a***` 的
/// 外层定界符靠这条拿满）。
fn close_frame(frame: &mut Frame, src: &str, taken: &[Range<usize>]) {
    let (Some(first), last) = (frame.first_content, frame.last_content_end) else {
        return;
    };
    let demand = frame.demand();
    collect_edge(src, first, -1, demand, &mut frame.folds, taken);
    collect_edge(src, last, 1, demand, &mut frame.folds, taken);
}

fn collect_edge(
    src: &str,
    from: usize,
    dir: i64,
    demand: usize,
    folds: &mut Vec<Range<usize>>,
    taken: &[Range<usize>],
) {
    let b = src.as_bytes();
    // 左走从内容首字符的前一字节开始；右走从内容末（排他）开始。
    let mut pos = if dir < 0 {
        from as i64 - 1
    } else {
        from as i64
    };
    let mut got = 0usize;
    while got < demand {
        if pos < 0 || pos as usize >= b.len() {
            return;
        }
        let c = b[pos as usize];
        if c != b'*' && c != b'_' {
            return; // 撞到正文/空白/换行 → 收集结束
        }
        let p = pos as usize;
        if !folds.iter().any(|f| f.contains(&p)) && !taken.iter().any(|f| f.contains(&p)) {
            folds.push(p..p + 1);
            got += 1;
        }
        pos += dir;
    }
}

/// 行内 code：双假设验证 range 语义（含反引号 / 不含反引号）。
/// 验证失败 → 不折叠不加样式（保守）。返回是否成立。
fn code_container(src: &str, range: &Range<usize>, spans: &mut Vec<Span>) -> Option<Container> {
    let b = src.as_bytes();
    if range.len() >= 2 && b[range.start] == b'`' && b[range.end - 1] == b'`' {
        // range 含定界符
        spans.push(Span {
            range: range.start + 1..range.end - 1,
            style: Style::Code,
        });
        Some(Container {
            content: range.start + 1..range.end - 1,
            folds: vec![range.start..range.start + 1, range.end - 1..range.end],
        })
    } else if range.start > 0
        && range.end < b.len()
        && b[range.start - 1] == b'`'
        && b[range.end] == b'`'
    {
        // range 是内容本体
        spans.push(Span {
            range: range.clone(),
            style: Style::Code,
        });
        Some(Container {
            content: range.clone(),
            folds: vec![range.start - 1..range.start, range.end..range.end + 1],
        })
    } else {
        None
    }
}

/// 容器栈 → 单一样式（bold+italic → BoldItalic）。
fn combine(stack: &[Frame]) -> Style {
    let bold = stack.iter().any(|f| matches!(f.style, Style::Bold));
    let italic = stack.iter().any(|f| matches!(f.style, Style::Italic));
    match (bold, italic) {
        (true, true) => Style::BoldItalic,
        (true, false) => Style::Bold,
        (false, true) => Style::Italic,
        (false, false) => Style::Normal,
    }
}

/// 逐行扫描：ATX 标题（`#{1,6}` + 空格/行尾）→ 折叠前缀 + 整行样式
/// span + 行级字号倍数；围栏行（``` / ~~~ + 语言名）→ 整段折叠
/// （Obsidian 式：fence 标记不可见，块级 widget 留 P3）。
/// 感知 ``` / ~~~ 围栏（围栏内不认标题）；缩进 >3 空格视为代码不认。
/// 不做 setext 与引用块内标题——保守漏折。
fn scan_headings(
    src: &str,
    containers: &mut Vec<Container>,
    spans: &mut Vec<Span>,
    line_scale: &mut Vec<(usize, f32)>,
) {
    const HEADING_SCALE: [f32; 6] = [1.62, 1.42, 1.26, 1.12, 1.0, 1.0];
    let b = src.as_bytes();
    let mut in_fence: Option<u8> = None;
    let mut i = 0usize;
    let mut line_no = 0usize;
    while i < b.len() {
        let mut j = i;
        while j < b.len() && b[j] != b'\n' {
            j += 1;
        }
        let line = &b[i..j];
        let indent = line.iter().take_while(|&&c| c == b' ').count();

        if indent <= 3 && line.len() > indent {
            let c0 = line[indent];
            if c0 == b'`' || c0 == b'~' {
                let run = line[indent..].iter().take_while(|&&c| c == c0).count();
                if run >= 3 {
                    // 围栏标记行整段折叠（含语言名，保留空行）
                    containers.push(Container {
                        content: i..j,
                        folds: vec![i + indent..j],
                    });
                    match in_fence {
                        Some(fc) if fc == c0 => in_fence = None,
                        None => in_fence = Some(c0),
                        _ => {}
                    }
                    i = if j < b.len() { j + 1 } else { b.len() };
                    line_no += 1;
                    continue;
                }
            }
        }

        if in_fence.is_none() && indent <= 3 {
            let hashes = line.iter().skip(indent).take_while(|&&c| c == b'#').count();
            if (1..=6).contains(&hashes) {
                let after = indent + hashes;
                let (prefix, content_start) = if after == line.len() {
                    (i + indent..i + after, after)
                } else if line[after] == b' ' || line[after] == b'\t' {
                    (i + indent..i + after + 1, after + 1)
                } else {
                    (0..0, usize::MAX) // 非标题
                };
                if content_start != usize::MAX {
                    // 整行文本样式 + 行级字号上浮（element 按倍数 shaping）
                    if content_start < j {
                        spans.push(Span {
                            range: i + content_start..j,
                            style: Style::Heading(hashes as u8),
                        });
                    }
                    containers.push(Container {
                        content: i..j,
                        folds: vec![prefix],
                    });
                    line_scale.push((line_no, HEADING_SCALE[hashes - 1]));
                }
            }
        }

        i = if j < b.len() { j + 1 } else { b.len() };
        line_no += 1;
    }
}

#[cfg(test)]
mod tests;
