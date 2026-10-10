//! v57-1: raw HTML 安全子集映射（pi-web rehype-raw + rehype-sanitize 语义的
//! GPUI 对应物）。
//!
//! 来源：pulldown-cmark 的 `Event::Html`（块级原文）/ `InlineHtml`（行内片段），
//! v56 前这两类事件被直接丢弃。本模块用 scraper（html5ever，HTML5 容错 +
//! 实体解码）解析，把安全子集映射到 markdown 的 Run/MdBlock 模型：
//! - 行内：b/strong/i/em/code/kbd/a/del/s/strike/br + 无样式透传（span/u/mark）
//! - 块级：p/pre/img/h1-h6/ul/ol/li/table/blockquote/hr + 容器递归
//!   （div/details/summary/section）
//! - 剥离：script/style/iframe/object/embed/form/head/svg（子树整体丢弃）
//!
//! 已知偏差：sub/sup 降级为普通文本；行内图片以 🖼 alt 占位（GPUI 文本流
//! 无法嵌图）；details 不折叠、summary 加粗平铺。

use ego_tree::NodeRef;
use scraper::Html;
use scraper::Node;

use crate::markdown::{MdBlock, Run, Style};

/// 行内片段 → Run 列表（markdown collect_inline 的 InlineHtml 分支用）。
pub(crate) fn inline_runs(html: &str) -> Vec<Run> {
    let frag = Html::parse_fragment(html);
    let mut runs = Vec::new();
    let mut styles = Vec::new();
    for child in frag.tree.root().children() {
        walk_inline(&child, &mut styles, &mut runs);
    }
    runs
}

/// 单个 InlineHtml 片段的效果（pulldown 把 `<b>x</b>` 拆成开标签/文本/
/// 闭标签三个事件——开闭标签需要跨事件维持样式栈）。
#[derive(Debug)]
pub(crate) enum InlineHtmlEffect {
    /// 开标签：把样式压入调用方的样式栈（`<a href>` 同时携带跳转目标）
    StylePush(Style, Option<String>),
    /// 闭标签：弹出样式栈
    StylePop,
    /// 自带内容（文本/br/img/完整片段）：直接产出 runs
    Runs(Vec<Run>),
}

fn tag_style(tag: &str) -> Option<Style> {
    let style = match tag {
        "b" | "strong" => Style::Bold,
        "i" | "em" | "cite" | "var" => Style::Italic,
        "code" | "kbd" | "samp" => Style::Code,
        "a" => Style::Link,
        "del" | "s" | "strike" => Style::Strike,
        _ => return None,
    };
    Some(style)
}

/// 判定一个 InlineHtml 片段的效果。html5ever 会丢弃无配对的闭标签——
/// 解析结果为空即视为闭标签（StylePop）；纯开标签（无文本内容）视为
/// StylePush；有内容则产出 runs。
pub(crate) fn fragment_effect(html: &str) -> InlineHtmlEffect {
    let frag = Html::parse_fragment(html);
    let mut style_tag: Option<Style> = None;
    let mut href: Option<String> = None;
    let mut has_content = false;
    for node in frag.tree.root().descendants() {
        match node.value() {
            scraper::node::Node::Text(t) => {
                if !t.text.trim().is_empty() {
                    has_content = true;
                }
            }
            scraper::node::Node::Element(el) => {
                let name = el.name().to_ascii_lowercase();
                match name.as_str() {
                    "br" | "img" => has_content = true,
                    _ => {
                        if let Some(st) = tag_style(&name) {
                            if style_tag.is_none() {
                                style_tag = Some(st);
                                // `<a href="…">`：目标随样式一并交给调用方样式栈
                                if name == "a" {
                                    href = el.attr("href").map(|h| h.to_string());
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if has_content {
        InlineHtmlEffect::Runs(inline_runs(html))
    } else if let Some(st) = style_tag {
        InlineHtmlEffect::StylePush(st, href)
    } else {
        InlineHtmlEffect::StylePop
    }
}

/// 块级原文 → MdBlock 列表（markdown 顶层 Event::Html 分支用）。
pub(crate) fn blocks(html: &str) -> Vec<MdBlock> {
    let frag = Html::parse_fragment(html);
    let mut out = Vec::new();
    // parse_fragment 包一层 <html><body>…</body></html>，直接下钻 body
    for child in frag.tree.root().children() {
        if is_element(&child, "body") {
            for bc in child.children() {
                walk_block(&bc, &mut out, 0);
            }
        } else {
            walk_block(&child, &mut out, 0);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 行内遍历
// ---------------------------------------------------------------------------

/// 样式栈元素：(样式, 链接目标)，与 markdown::StyleEntry 同构——透传标签
/// （span/u/mark 等）继承链接目标，保证 `<a href><span>x</span></a>` 可点。
fn push_style(styles: &mut Vec<(Style, Option<String>)>, tag: &str, href: Option<String>) {
    let (base, base_url) = styles
        .last()
        .map(|(s, u)| (*s, u.clone()))
        .unwrap_or((Style::Normal, None));
    let next = match tag {
        "b" | "strong" => match base {
            Style::Italic | Style::BoldItalic => Style::BoldItalic,
            _ => Style::Bold,
        },
        "i" | "em" | "cite" | "var" => match base {
            Style::Bold | Style::BoldItalic => Style::BoldItalic,
            _ => Style::Italic,
        },
        "code" | "kbd" | "samp" => Style::Code,
        "a" => Style::Link,
        "del" | "s" | "strike" => Style::Strike,
        _ => base,
    };
    let url = if tag == "a" { href } else { base_url };
    styles.push((next, url));
}

fn walk_inline(
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
                runs.push(Run { text: text.text.to_string(), style, url });
            }
        }
        Node::Element(el) => {
            let tag = el.name().to_ascii_lowercase();
            match tag.as_str() {
                "script" | "style" | "iframe" | "object" | "embed" | "form" | "head"
                | "svg" | "template" => return,
                "br" => {
                    let (style, url) = styles
                        .last()
                        .map(|(s, u)| (*s, u.clone()))
                        .unwrap_or((Style::Normal, None));
                    runs.push(Run { text: "\n".to_string(), style, url });
                }
                "img" => {
                    let alt = el.attr("alt").unwrap_or("");
                    let (style, url) = styles
                        .last()
                        .map(|(s, u)| (*s, u.clone()))
                        .unwrap_or((Style::Normal, None));
                    runs.push(Run {
                        text: format!("🖼 {alt}"),
                        style: if style == Style::Normal { Style::Italic } else { style },
                        url,
                    });
                }
                _ => {
                    let href =
                        if tag == "a" { el.attr("href").map(|h| h.to_string()) } else { None };
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

fn is_element(node: &NodeRef<'_, Node>, tag: &str) -> bool {
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
        None
            | Some("a") | Some("b") | Some("strong") | Some("i") | Some("em")
            | Some("code") | Some("span") | Some("del") | Some("s") | Some("strike")
            | Some("kbd") | Some("u") | Some("mark") | Some("small") | Some("sub")
            | Some("sup") | Some("cite") | Some("var") | Some("samp") | Some("br")
    )
}

fn walk_block(node: &NodeRef<'_, Node>, out: &mut Vec<MdBlock>, depth: usize) {
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
        | "template" | "col" | "colgroup" | "thead" | "tbody" | "tfoot" | "tr" | "th"
        | "td" | "dt" | "dd" | "option" => {}
        "p" => {
            let runs = collect_cell_runs(node);
            if !runs.is_empty() {
                out.push(MdBlock::Paragraph { runs });
            }
        }
        "pre" => {
            let code;
            let mut lang = String::new();
            let code_child = node.children().find(|c| tag_of(c).as_deref() == Some("code"));
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
            out.push(MdBlock::Heading { level, runs: collect_cell_runs(node) });
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
                let marker = if ordered { format!("{n}.") } else { "•".to_string() };
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
                out.push(MdBlock::ListItem { depth, marker, runs, task: None });
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
                    Some("p") | Some("div") | Some("pre") | Some("table") | Some("ul")
                        | Some("ol") | Some("h1") | Some("h2") | Some("h3") | Some("h4")
                        | Some("h5") | Some("h6") | Some("blockquote") | Some("img")
                        | Some("details") | Some("hr")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn styles_of(runs: &[Run]) -> Vec<Style> {
        runs.iter().map(|r| r.style).collect()
    }

    #[test]
    fn inline_bold_italic_code() {
        let runs = inline_runs("<b>bold</b> and <i>it</i> <code>c</code>");
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            vec!["bold", " and ", "it", " ", "c"]
        );
        assert_eq!(
            styles_of(&runs),
            vec![Style::Bold, Style::Normal, Style::Italic, Style::Normal, Style::Code]
        );
    }

    #[test]
    fn script_dropped_text_kept() {
        let runs = inline_runs("<script>alert(1)</script>ok");
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), vec!["ok"]);
    }

    #[test]
    fn entities_decoded() {
        let runs = inline_runs("a &amp; &lt;b&gt;");
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), vec!["a & <b>"]);
    }

    #[test]
    fn pre_code_block_with_language() {
        let blocks = blocks("<pre><code class=\"language-rust\">fn x() {}</code></pre>");
        match &blocks[0] {
            MdBlock::Code { lang, code } => {
                assert_eq!(lang, "rust");
                assert_eq!(code.trim(), "fn x() {}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn table_maps_head_and_rows() {
        let blocks = blocks(
            "<table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>",
        );
        match &blocks[0] {
            MdBlock::Table { head, rows } => {
                assert_eq!(head.len(), 2);
                assert_eq!(rows.len(), 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn img_and_list() {
        let blocks = blocks("<img src=\"x.png\" alt=\"y\"><ul><li>one</li><li>two</li></ul>");
        assert!(matches!(&blocks[0], MdBlock::Image { url, .. } if url == "x.png"));
        assert_eq!(blocks.len(), 3); // img + 2 list items
    }

    #[test]
    fn details_flattens_with_bold_summary() {
        let blocks = blocks("<details><summary>Title</summary><p>body</p></details>");
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            MdBlock::Paragraph { runs } => assert_eq!(runs[0].style, Style::Bold),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fragment_effects_for_paired_tags() {
        // pulldown 把 <b>x</b> 拆成三个事件：开标签/文本/闭标签
        use crate::markdown::Style;
        match fragment_effect("<b>") {
            InlineHtmlEffect::StylePush(st, href) => {
                assert_eq!(st, Style::Bold);
                assert_eq!(href, None);
            }
            other => panic!("{other:?}"),
        }
        match fragment_effect("</b>") {
            InlineHtmlEffect::StylePop => {}
            other => panic!("{other:?}"),
        }
        // br 有内容效果（换行 run）
        assert!(matches!(fragment_effect("<br>"), InlineHtmlEffect::Runs(_)));
        // 自带文本的完整片段走 runs
        assert!(matches!(fragment_effect("<b>x</b>"), InlineHtmlEffect::Runs(_)));
    }

    #[test]
    fn link_run() {
        let runs = inline_runs("<a href=\"https://x.example\">link</a>");
        assert_eq!(runs[0].style, Style::Link);
        assert_eq!(runs[0].text, "link");
        // href 随 run 保留（渲染层据此接点击）
        assert_eq!(runs[0].url.as_deref(), Some("https://x.example"));
    }

    #[test]
    fn link_effect_carries_href() {
        // 纯开标签 <a>：StylePush 携带 href，供跨事件样式栈保留目标
        match fragment_effect("<a href=\"https://y.example\">") {
            InlineHtmlEffect::StylePush(st, href) => {
                assert_eq!(st, Style::Link);
                assert_eq!(href.as_deref(), Some("https://y.example"));
            }
            other => panic!("{other:?}"),
        }
    }
}
