//! 行内解析助手（自 parse.rs 外移）：pulldown 行内样式栈进出与
//! 「文本 → 文本/链接对」切分。

use super::*;
use pulldown_cmark::{Tag, TagEnd};

pub(super) fn style_push(styles: &mut Vec<StyleEntry>, tag: &Tag) {
    let (base, base_url) = styles
        .last()
        .map(|(s, u)| (*s, u.clone()))
        .unwrap_or((Style::Normal, None));
    let next = match tag {
        Tag::Strong => (
            match base {
                Style::Italic | Style::BoldItalic => Style::BoldItalic,
                _ => Style::Bold,
            },
            base_url,
        ),
        Tag::Emphasis => (
            match base {
                Style::Bold | Style::BoldItalic => Style::BoldItalic,
                _ => Style::Italic,
            },
            base_url,
        ),
        Tag::Link { dest_url, .. } => (Style::Link, Some(dest_url.to_string())),
        Tag::Strikethrough => (Style::Strike, base_url),
        _ => (base, base_url),
    };
    styles.push(next);
}

pub(super) fn style_pop(styles: &mut Vec<StyleEntry>, end: &TagEnd) {
    match end {
        TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link | TagEnd::Strikethrough => {
            styles.pop();
        }
        _ => {}
    }
}

/// GFM autolink-literal parity (pi-web remark-gfm；pulldown 0.13 无此扩展)：
/// 把普通文本按裸 http(s) URL 切成 Link run。尾部标点剥到 URL 外。
pub(super) fn split_links(text: &str) -> Vec<(String, bool)> {
    const HTTPS: &str = "https://";
    const HTTP: &str = "http://";
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut rest = text;
    loop {
        let start = [HTTPS, HTTP].iter().filter_map(|sc| rest.find(sc)).min();
        let Some(start) = start else { break };
        // 前缀须是词边界（避免 "foohttps://" 误切）
        let boundary_ok = start == 0
            || !rest[..start]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);
        if !boundary_ok {
            // 跳过这个伪起点，从下一个字符继续找
            let (head, tail) = rest.split_at(start + 1);
            out.push((head.to_string(), false));
            rest = tail;
            continue;
        }
        // URL 止于空白或尖括号；尾部标点剥出
        let candidate = &rest[start..];
        let end = candidate
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
            .unwrap_or(candidate.len());
        let mut url = &candidate[..end];
        while url
            .chars()
            .next_back()
            .map(|c| "!.,;:?\")'".contains(c))
            .unwrap_or(false)
        {
            url = &url[..url.len() - 1];
        }
        if url.len() > HTTPS.len() {
            if start > 0 {
                out.push((rest[..start].to_string(), false));
            }
            out.push((url.to_string(), true));
            rest = &rest[start + url.len()..];
        } else {
            // 裸 scheme 无主体——原样保留
            let (head, tail) = rest.split_at(start + 1);
            out.push((head.to_string(), false));
            rest = tail;
        }
    }
    if out.is_empty() {
        vec![(text.to_string(), false)]
    } else {
        out.push((rest.to_string(), false));
        out
    }
}
