//! 块解析层（自 parse.rs 外移）：块边界判定、HTML 块三路分发、
//! parse_blocks 整卷折叠 / parse_block 单块递归 / 表格单元格切分。

use super::*;
use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};

/// 块级边界事件：列表项内容里遇到它就得停手（块级 `Start` 走递归解析、
/// 块级 `End` 只跳过标记）。**行内事件不在此列**——`Code`/强调/链接/
/// 软换行/行内公式都由 [`collect_inline`] 消费。
///
/// ⚠️ 含 `TagEnd::Item`：列表项内容用 [`collect_inline_peek`]（不消费终止
/// 事件），终止符原样留给外层 while 判定。
fn is_block_boundary(e: &Event) -> bool {
    matches!(
        e,
        Event::Start(
            Tag::Paragraph
                | Tag::List(_)
                | Tag::CodeBlock(_)
                | Tag::BlockQuote(_)
                | Tag::Table(_)
                | Tag::Image { .. }
                | Tag::HtmlBlock
        ) | Event::End(
            TagEnd::Item
                | TagEnd::Paragraph
                | TagEnd::List(_)
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_)
                | TagEnd::Table
                | TagEnd::Image
                | TagEnd::HtmlBlock
        )
    )
}

// ---------------------------------------------------------------------------
// block parsing
// ---------------------------------------------------------------------------

/// HTML 块内容三路分发：doc 模式 img 提取（html_block_doc）/ 安全子集映射
/// （html::blocks）/ 用户气泡标签原文。裸 `Event::Html` 与
/// `Start(Tag::HtmlBlock)` 包裹层共用。
fn html_block_dispatch(raw: &str, html: bool) -> Vec<MdBlock> {
    if html && doc_mode() {
        html_block_doc(raw)
    } else if html {
        crate::editor::markdown::render::html::blocks(raw)
    } else {
        vec![MdBlock::Paragraph {
            runs: literal_runs(raw),
        }]
    }
}

pub(in crate::editor::markdown) fn parse_blocks(events: &[Event], html: bool) -> Vec<MdBlock> {
    let mut out: Vec<MdBlock> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::HtmlBlock) => {
                // pulldown 0.10+ 把块级 HTML 包进 Start/End(HtmlBlock)。此前
                // 落进 parse_block 的兜底「跳到容器尾」，内部 Event::Html 整段
                // 被吞——README 头部 <p><img></p>+tagline 就这样消失（图片
                // 不显示的真根因，v57-1/doc 模式两条 HTML 路径都没被走到）。
                i += 1;
                let mut raw = String::new();
                while i < events.len() && !matches!(events[i], Event::End(TagEnd::HtmlBlock)) {
                    if let Event::Html(h) | Event::InlineHtml(h) = &events[i] {
                        raw.push_str(h);
                    }
                    i += 1;
                }
                i += 1; // End(HtmlBlock)
                out.extend(html_block_dispatch(&raw, html));
            }
            Event::Start(tag) => {
                let tag = tag.clone();
                i += 1;
                parse_block(events, &mut i, tag, &mut out, 0, html);
            }
            Event::Rule => {
                out.push(MdBlock::Rule);
                i += 1;
            }
            Event::Text(t) => {
                out.push(MdBlock::Paragraph {
                    runs: vec![Run {
                        text: t.to_string(),
                        style: Style::Normal,
                        url: None,
                    }],
                });
                i += 1;
            }
            Event::Html(h) => {
                out.extend(html_block_dispatch(h, html));
                i += 1;
            }
            Event::DisplayMath(tex) => {
                // v57-2: 块级公式（$$…$$）
                out.push(MdBlock::Math {
                    latex: tex.to_string(),
                });
                i += 1;
            }
            Event::InlineHtml(h) => {
                if html {
                    let runs = crate::editor::markdown::render::html::inline_runs(h);
                    if !runs.is_empty() {
                        out.push(MdBlock::Paragraph { runs });
                    }
                } else {
                    out.push(MdBlock::Paragraph {
                        runs: literal_runs(h),
                    });
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// True when the paragraph from `start` holds nothing but whitespace and a
/// single image — those render as a real image block (pi-web img: block).
fn solo_image(events: &[Event], start: usize) -> Option<String> {
    let mut url: Option<String> = None;
    let mut depth = 0usize;
    let mut i = start;
    while i < events.len() {
        match &events[i] {
            Event::End(TagEnd::Paragraph) => break,
            Event::Start(Tag::Image { dest_url, .. }) => {
                if depth == 0 {
                    if url.is_some() {
                        return None;
                    }
                    url = Some(dest_url.to_string());
                }
                depth += 1;
            }
            Event::End(TagEnd::Image) => depth -= 1,
            _ if depth == 0 => {
                if let Event::Text(t) = &events[i] {
                    if t.trim().is_empty() {
                        i += 1;
                        continue;
                    }
                }
                return None;
            }
            _ => {}
        }
        i += 1;
    }
    url
}

fn parse_block(
    events: &[Event],
    i: &mut usize,
    tag: Tag,
    out: &mut Vec<MdBlock>,
    depth: usize,
    html: bool,
) {
    match tag {
        Tag::Paragraph => {
            if let Some(url) = solo_image(events, *i) {
                // 独立图片段落：定位 Image，收 alt，越过 End(Paragraph)
                while !matches!(events[*i], Event::Start(Tag::Image { .. })) {
                    *i += 1;
                }
                if let Event::Start(Tag::Image { .. }) = &events[*i] {
                    *i += 1;
                    let alt = collect_inline(
                        events,
                        i,
                        &|e| matches!(e, Event::End(TagEnd::Image)),
                        html,
                    );
                    out.push(MdBlock::Image {
                        url,
                        alt,
                        width: None,
                    });
                }
                while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Paragraph)) {
                    *i += 1;
                }
                *i += 1;
            } else {
                let runs = collect_inline(
                    events,
                    i,
                    &|e| matches!(e, Event::End(TagEnd::Paragraph)),
                    html,
                );
                out.push(MdBlock::Paragraph { runs });
            }
        }
        Tag::Heading { level, .. } => {
            let runs = collect_inline(
                events,
                i,
                &|e| matches!(e, Event::End(TagEnd::Heading(_))),
                html,
            );
            out.push(MdBlock::Heading {
                level: level as u8,
                runs,
            });
        }
        Tag::BlockQuote(_) => {
            let mut inner = Vec::new();
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::BlockQuote(_))) {
                if let Event::Start(t) = &events[*i] {
                    let t = t.clone();
                    *i += 1;
                    parse_block(events, i, t, &mut inner, depth + 1, html);
                } else {
                    *i += 1;
                }
            }
            *i += 1; // End(BlockQuote)
            out.push(MdBlock::Quote { blocks: inner });
        }
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                CodeBlockKind::Fenced(l) => l.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            let mut code = String::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::Text(t) => code.push_str(t),
                    Event::End(TagEnd::CodeBlock) => {
                        *i += 1;
                        break;
                    }
                    _ => {}
                }
                *i += 1;
            }
            if lang.trim() == "mermaid" {
                // v57-3: mermaid 图（渲染期流式回退源码）
                out.push(MdBlock::Mermaid { source: code });
            } else {
                out.push(MdBlock::Code { lang, code });
            }
        }
        Tag::Table(_) => {
            let mut head: Vec<Vec<Run>> = Vec::new();
            let mut rows: Vec<Vec<Vec<Run>>> = Vec::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::End(TagEnd::Table) => {
                        *i += 1;
                        break;
                    }
                    Event::Start(Tag::TableHead) => {
                        *i += 1;
                        head = parse_table_cells(
                            events,
                            i,
                            &|e| matches!(e, Event::End(TagEnd::TableHead)),
                            html,
                        );
                    }
                    Event::Start(Tag::TableRow) => {
                        *i += 1;
                        let row = parse_table_cells(
                            events,
                            i,
                            &|e| matches!(e, Event::End(TagEnd::TableRow)),
                            html,
                        );
                        rows.push(row);
                    }
                    _ => *i += 1,
                }
            }
            out.push(MdBlock::Table { head, rows });
        }
        Tag::Image { dest_url, .. } => {
            let alt = collect_inline(events, i, &|e| matches!(e, Event::End(TagEnd::Image)), html);
            out.push(MdBlock::Image {
                url: dest_url.to_string(),
                alt,
                width: None,
            });
        }
        Tag::List(start) => {
            let ordered = start.is_some();
            let mut n = start.unwrap_or(1);
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::List(_))) {
                if let Event::Start(Tag::Item) = &events[*i] {
                    *i += 1;
                    let marker = if ordered {
                        format!("{n}.")
                    } else {
                        "•".to_string()
                    };
                    n += 1;
                    let mut runs: Vec<Run> = Vec::new();
                    let mut nested = Vec::new();
                    let mut task: Option<bool> = None;
                    while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Item)) {
                        match &events[*i] {
                            // c12: 任务列表复选框（pi-web 自绘 14px accent 对勾）
                            Event::TaskListMarker(checked) => {
                                task = Some(*checked);
                                *i += 1;
                            }
                            // 只接**块级** Start（段落 / 嵌套列表 / 代码块 / 引用 /
                            // 表格 / 图片 / HTML 块）。行内 Start（Link / Strong /
                            // Emphasis / Strikethrough）必须落到下面的
                            // collect_inline_peek —— 旧写法 `Event::Start(inner_tag)`
                            // 把行内开标签也抢走（内层 `_ => {}` 静默空转），
                            // 于是列表里的链接/强调丢了样式。
                            Event::Start(inner_tag) if is_block_boundary(&events[*i]) => {
                                let inner_tag = inner_tag.clone();
                                *i += 1;
                                match &inner_tag {
                                    Tag::Paragraph => {
                                        let r = collect_inline(
                                            events,
                                            i,
                                            &|e| matches!(e, Event::End(TagEnd::Paragraph)),
                                            html,
                                        );
                                        if !runs.is_empty() {
                                            if let Some(last) = runs.last_mut() {
                                                last.text.push(' ');
                                            }
                                        }
                                        runs.extend(r);
                                    }
                                    Tag::List(_) => {
                                        parse_block(
                                            events,
                                            i,
                                            inner_tag,
                                            &mut nested,
                                            depth + 1,
                                            html,
                                        );
                                    }
                                    Tag::CodeBlock(_) | Tag::BlockQuote(_) | Tag::Table(_) => {
                                        parse_block(
                                            events,
                                            i,
                                            inner_tag,
                                            &mut nested,
                                            depth + 1,
                                            html,
                                        );
                                    }
                                    Tag::Image { .. } => {
                                        parse_block(
                                            events,
                                            i,
                                            inner_tag,
                                            &mut nested,
                                            depth + 1,
                                            html,
                                        );
                                    }
                                    Tag::HtmlBlock => {
                                        // 列表项内包裹式 HTML 块：收拢 chunks 走
                                        // 统一分发（i 已过 Start，此处到 End 为止）
                                        let mut raw = String::new();
                                        while *i < events.len()
                                            && !matches!(events[*i], Event::End(TagEnd::HtmlBlock))
                                        {
                                            if let Event::Html(h) | Event::InlineHtml(h) =
                                                &events[*i]
                                            {
                                                raw.push_str(h);
                                            }
                                            *i += 1;
                                        }
                                        *i += 1; // End(HtmlBlock)
                                        nested.extend(html_block_dispatch(&raw, html));
                                    }
                                    _ => {}
                                }
                            }
                            // 块级收尾标记（End(Paragraph)/End(CodeBlock)…）：其内容
                            // 已由上面的 Start 臂消费，这里只跳过标记本身。
                            // End(Item) 不在此列——它是本项终止符，交给外层 while。
                            Event::End(_) if is_block_boundary(&events[*i]) => {
                                *i += 1;
                            }
                            // 其余全是**行内**事件（Text / Code / 强调 / 链接 /
                            // 软换行 / 行内公式…）：一律交给 collect_inline_peek，
                            // 与段落同源。⚠️ 此前这里只有 `Event::Text` 一个分支，
                            // 其余全被 `_ => *i += 1` **静默吞掉**——紧凑列表
                            // （tight list，CommonMark 默认不带 <p>）里的 `code`、
                            // **粗体**、链接全部不渲染（用户实测：整条列表只剩
                            // 「：、、」）。
                            _ => runs.extend(collect_inline_peek(
                                events,
                                i,
                                &|e| is_block_boundary(e),
                                html,
                            )),
                        }
                    }
                    *i += 1; // End(Item)
                    out.push(MdBlock::ListItem {
                        depth,
                        marker,
                        runs,
                        task,
                    });
                    out.extend(nested);
                } else {
                    *i += 1;
                }
            }
            *i += 1; // End(List)
        }
        _ => {
            // unknown/unsupported container: skip to its end (depth-naive but fine
            // for the subset markdown chat needs)
            let mut d = 1usize;
            while *i < events.len() && d > 0 {
                match &events[*i] {
                    Event::Start(_) => d += 1,
                    Event::End(_) => d -= 1,
                    _ => {}
                }
                *i += 1;
            }
        }
    }
}

/// Parse `Start(TableCell) … End(TableCell)` sequences until `is_end` fires
/// (the row/head End event), one `Vec<Run>` per cell.
fn parse_table_cells(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
    html: bool,
) -> Vec<Vec<Run>> {
    let mut cells: Vec<Vec<Run>> = Vec::new();
    while *i < events.len() {
        if is_end(&events[*i]) {
            *i += 1;
            break;
        }
        if matches!(events[*i], Event::Start(Tag::TableCell)) {
            *i += 1;
            let runs = collect_inline(
                events,
                i,
                &|e| matches!(e, Event::End(TagEnd::TableCell)),
                html,
            );
            cells.push(runs);
        } else {
            *i += 1;
        }
    }
    cells
}
