//! html5ever 树遍历（自 html.rs 外移）：walk_inline 行内 run 收集 +
//! walk_block 块级递归映射（含表格/代码块/容器）+ 节点助手。

use super::*;

pub(super) fn walk_inline(
    node: &NodeRef<'_, Node>,
    styles: &mut Vec<(Style, Option<String>)>,
    runs: &mut Vec<Run>,
) {
    match node.value() {
        Node::Text(text) => {
            if !text.text.is_empty() {
                let (style, url) = styles
                    .last()
                    .map(|(s, u)| (*s, u.clone()))
                    .unwrap_or((Style::Normal, None));
                runs.push(Run {
                    text: text.text.to_string(),
                    style,
                    url,
                });
            }
        }
        Node::Element(el) => {
            let tag = el.name().to_ascii_lowercase();
            match tag.as_str() {
                "script" | "style" | "iframe" | "object" | "embed" | "form" | "head" | "svg"
                | "template" => return,
                "br" => {
                    let (style, url) = styles
                        .last()
                        .map(|(s, u)| (*s, u.clone()))
                        .unwrap_or((Style::Normal, None));
                    runs.push(Run {
                        text: "\n".to_string(),
                        style,
                        url,
                    });
                }
                "img" => {
                    let alt = el.attr("alt").unwrap_or("");
                    let (style, url) = styles
                        .last()
                        .map(|(s, u)| (*s, u.clone()))
                        .unwrap_or((Style::Normal, None));
                    runs.push(Run {
                        text: format!("🖼 {alt}"),
                        style: if style == Style::Normal {
                            Style::Italic
                        } else {
                            style
                        },
                        url,
                    });
                }
                _ => {
                    let href = if tag == "a" {
                        el.attr("href").map(|h| h.to_string())
                    } else {
                        None
                    };
                    push_style(styles, &tag, href);
                    for child in node.children() {
                        walk_inline(&child, styles, runs);
                    }
                    styles.pop();
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 块级遍历
// ---------------------------------------------------------------------------

pub(super) fn is_element(node: &NodeRef<'_, Node>, tag: &str) -> bool {
    matches!(node.value(), Node::Element(el) if el.name().eq_ignore_ascii_case(tag))
}

fn tag_of(node: &NodeRef<'_, Node>) -> Option<String> {
    match node.value() {
        Node::Element(el) => Some(el.name().to_ascii_lowercase()),
        _ => None,
    }
}

/// 节点子树的行内 runs（p/summary/td/li 的文本部分用）。
fn collect_cell_runs(node: &NodeRef<'_, Node>) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut styles = Vec::new();
    walk_inline(node, &mut styles, &mut runs);
    runs
}

/// 行内子元素判定（li/容器内：这些子元素走行内收集，其余走块级下钻）。
fn is_inline_tag(tag: Option<&str>) -> bool {
    matches!(
        tag,
        None | Some("a")
            | Some("b")
            | Some("strong")
            | Some("i")
            | Some("em")
            | Some("code")
            | Some("span")
            | Some("del")
            | Some("s")
            | Some("strike")
            | Some("kbd")
            | Some("u")
            | Some("mark")
            | Some("small")
            | Some("sub")
            | Some("sup")
            | Some("cite")
            | Some("var")
            | Some("samp")
            | Some("br")
    )
}

pub(super) fn walk_block(node: &NodeRef<'_, Node>, out: &mut Vec<MdBlock>, depth: usize) {
    let Some(tag) = tag_of(node) else {
        // 散文本 → 段落
        let runs = collect_cell_runs(node);
        if runs.iter().any(|r| !r.text.trim().is_empty()) {
            out.push(MdBlock::Paragraph { runs });
        }
        return;
    };
    match tag.as_str() {
        "script" | "style" | "iframe" | "object" | "embed" | "form" | "head" | "svg"
        | "template" | "col" | "colgroup" | "thead" | "tbody" | "tfoot" | "tr" | "th" | "td"
        | "dt" | "dd" | "option" => {}
        "p" => {
            let runs = collect_cell_runs(node);
            if !runs.is_empty() {
                out.push(MdBlock::Paragraph { runs });
            }
        }
        "pre" => {
            let code;
            let mut lang = String::new();
            let code_child = node
                .children()
                .find(|c| tag_of(c).as_deref() == Some("code"));
            match code_child {
                Some(c) => {
                    lang = el_class_lang(&c).unwrap_or_default();
                    code = node_text(&c);
                }
                None => code = node_text(node),
            }
            if lang.is_empty() {
                lang = node
                    .children()
                    .find_map(|c| el_class_lang(&c))
                    .unwrap_or_default();
            }
            out.push(MdBlock::Code { lang, code });
        }
        "img" => {
            if let Some(src) = node.value().as_element().and_then(|el| el.attr("src")) {
                out.push(MdBlock::Image {
                    url: src.to_string(),
                    alt: vec![Run {
                        text: node
                            .value()
                            .as_element()
                            .and_then(|el| el.attr("alt"))
                            .unwrap_or("")
                            .to_string(),
                        style: Style::Normal,
                        url: None,
                    }],
                    width: None,
                });
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = tag[1..].parse::<u8>().unwrap_or(6).min(6);
            out.push(MdBlock::Heading {
                level,
                runs: collect_cell_runs(node),
            });
        }
        "hr" => out.push(MdBlock::Rule),
        "br" => {}
        "blockquote" => {
            let mut inner = Vec::new();
            for child in node.children() {
                walk_block(&child, &mut inner, depth + 1);
            }
            out.push(MdBlock::Quote { blocks: inner });
        }
        "ul" | "ol" => {
            let ordered = tag == "ol";
            let mut n = 1u64;
            for child in node.children() {
                if tag_of(&child).as_deref() != Some("li") {
                    continue;
                }
                let marker = if ordered {
                    format!("{n}.")
                } else {
                    "•".to_string()
                };
                n += 1;
                let mut runs = Vec::new();
                let mut styles = Vec::new();
                let mut nested = Vec::new();
                for sub in child.children() {
                    if is_inline_tag(tag_of(&sub).as_deref()) {
                        walk_inline(&sub, &mut styles, &mut runs);
                        if let Some(last) = runs.last_mut() {
                            last.text.push(' ');
                        }
                    } else {
                        walk_block(&sub, &mut nested, depth + 1);
                    }
                }
                out.push(MdBlock::ListItem {
                    depth,
                    marker,
                    runs,
                    task: None,
                });
                out.extend(nested);
            }
        }
        "table" => {
            if let Some(tb) = html_table(node) {
                out.push(tb);
            }
        }
        "details" => {
            for child in node.children() {
                if tag_of(&child).as_deref() == Some("summary") {
                    let mut runs = collect_cell_runs(&child);
                    for r in runs.iter_mut() {
                        r.style = match r.style {
                            Style::Normal => Style::Bold,
                            Style::Italic => Style::BoldItalic,
                            other => other,
                        };
                    }
                    out.push(MdBlock::Paragraph { runs });
                } else {
                    walk_block(&child, out, depth);
                }
            }
        }
        // 容器（div/section/article/figure/center/summary 裸…）：有块级子元素
        // 则下钻，否则行内收成段落
        _ => {
            let has_block_child = node.children().any(|c| {
                matches!(
                    tag_of(&c).as_deref(),
                    Some("p")
                        | Some("div")
                        | Some("pre")
                        | Some("table")
                        | Some("ul")
                        | Some("ol")
                        | Some("h1")
                        | Some("h2")
                        | Some("h3")
                        | Some("h4")
                        | Some("h5")
                        | Some("h6")
                        | Some("blockquote")
                        | Some("img")
                        | Some("details")
                        | Some("hr")
                )
            });
            if has_block_child {
                for child in node.children() {
                    walk_block(&child, out, depth);
                }
            } else {
                let runs = collect_cell_runs(node);
                if runs.iter().any(|r| !r.text.trim().is_empty()) {
                    out.push(MdBlock::Paragraph { runs });
                }
            }
        }
    }
}

fn el_class_lang(node: &NodeRef<'_, Node>) -> Option<String> {
    node.value()
        .as_element()?
        .attr("class")?
        .split_whitespace()
        .find(|c| c.starts_with("language-"))
        .map(|c| c["language-".len()..].to_string())
}

/// `<table>` → MdBlock::Table（首个含 th 的行为表头，其余为数据行）。
fn html_table(node: &NodeRef<'_, Node>) -> Option<MdBlock> {
    fn trs<'a>(node: &NodeRef<'a, Node>, out: &mut Vec<NodeRef<'a, Node>>) {
        for child in node.children() {
            match tag_of(&child).as_deref() {
                Some("tr") => out.push(child),
                Some("thead") | Some("tbody") | Some("tfoot") => trs(&child, out),
                _ => {}
            }
        }
    }
    let mut all = Vec::new();
    trs(node, &mut all);
    let mut head: Vec<Vec<Run>> = Vec::new();
    let mut rows: Vec<Vec<Vec<Run>>> = Vec::new();
    let mut first = true;
    for tr in all {
        let cells: Vec<Vec<Run>> = tr
            .children()
            .filter(|c| matches!(tag_of(c).as_deref(), Some("td") | Some("th")))
            .map(|c| collect_cell_runs(&c))
            .collect();
        if cells.is_empty() {
            continue;
        }
        let is_head = tr.children().any(|c| tag_of(&c).as_deref() == Some("th"));
        if first && is_head {
            head = cells;
        } else {
            rows.push(cells);
        }
        first = false;
    }
    (!head.is_empty() || !rows.is_empty()).then(|| MdBlock::Table { head, rows })
}

fn node_text(node: &NodeRef<'_, Node>) -> String {
    let mut text = String::new();
    match node.value() {
        Node::Text(t) => text.push_str(&t.text),
        _ => {
            for child in node.children() {
                text.push_str(&node_text(&child));
            }
        }
    }
    text
}
