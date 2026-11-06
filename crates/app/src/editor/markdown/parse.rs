//! pulldown-cmark 解析层：inline 样式栈、链接切分、块事件折叠
//! （parse_blocks/parse_block）与表格单元格切分。产 MdBlock，纯函数。

use super::*;
use pulldown_cmark::Event;


mod block;
mod inline;

use inline::{split_links, style_pop, style_push};
pub(super) use block::parse_blocks;

/// Collect inline runs until `is_end` matches (**consuming** the end event) —
/// existing callers（段落 / 表格单元 / 图片 alt）的口径。
fn collect_inline(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
    html: bool,
) -> Vec<Run> {
    let runs = collect_inline_peek(events, i, is_end, html);
    if *i < events.len() && is_end(&events[*i]) {
        *i += 1;
    }
    runs
}

/// 同 [`collect_inline`]，但命中 `is_end` 时**不**消费终止事件（光标留在它上面）。
/// 列表项内容用这个：终止符 `End(TagEnd::Item)` 必须留给外层 while 判定，
/// 吃掉它会把下一项并进本项。
fn collect_inline_peek(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
    html: bool,
) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut styles: Vec<StyleEntry> = Vec::new();
    let mut text = String::new();
    let mut cur = Style::Normal;
    let mut cur_url: Option<String> = None;

    fn flush(text: &mut String, cur: Style, cur_url: &Option<String>, runs: &mut Vec<Run>) {
        if text.is_empty() {
            return;
        }
        let taken = std::mem::take(text);
        if cur == Style::Normal {
            // GFM 自动链接：普通文本里的裸 URL 提为 Link run（目标 = 文本本身）
            for (seg, is_link) in split_links(&taken) {
                if !seg.is_empty() {
                    let url = is_link.then(|| seg.clone());
                    runs.push(Run {
                        text: seg,
                        style: if is_link { Style::Link } else { Style::Normal },
                        url,
                    });
                }
            }
        } else {
            runs.push(Run {
                text: taken,
                style: cur,
                url: cur_url.clone(),
            });
        }
    }

    while *i < events.len() {
        match &events[*i] {
            e if is_end(e) => {
                flush(&mut text, cur, &cur_url, &mut runs);
                return runs;
            }
            Event::Text(t) => {
                let (s, u) = styles
                    .last()
                    .map(|(s, u)| (*s, u.clone()))
                    .unwrap_or((Style::Normal, None));
                if s != cur || u != cur_url {
                    flush(&mut text, cur, &cur_url, &mut runs);
                    cur = s;
                    cur_url = u;
                }
                text.push_str(t);
            }
            Event::InlineHtml(h) => {
                // v57-1: 行内 HTML；html=false（用户气泡）时标签原文可见。
                // 开/闭标签跨事件维持样式栈（配对标签样式不丢）
                flush(&mut text, cur, &cur_url, &mut runs);
                if html {
                    use crate::editor::markdown::render::html::InlineHtmlEffect as E;
                    match crate::editor::markdown::render::html::fragment_effect(h) {
                        E::StylePush(st, url) => styles.push((st, url)),
                        E::StylePop => {
                            styles.pop();
                        }
                        E::Runs(rs) => runs.extend(rs),
                    }
                    let (s, u) = styles
                        .last()
                        .map(|(s, u)| (*s, u.clone()))
                        .unwrap_or((Style::Normal, None));
                    cur = s;
                    cur_url = u;
                } else {
                    runs.extend(literal_runs(h));
                    cur = Style::Normal;
                    cur_url = None;
                    styles.clear();
                }
            }
            Event::InlineMath(tex) => {
                // v57-2: 行内公式标记 run（渲染期拆段成图）
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run {
                    text: tex.to_string(),
                    style: Style::Math,
                    url: None,
                });
            }
            Event::DisplayMath(tex) => {
                // 多行 $$…$$ 的 DisplayMath 事件发在段落流内（实测），同用标记
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run {
                    text: tex.to_string(),
                    style: Style::DisplayMath,
                    url: None,
                });
            }
            Event::Code(c) => {
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run {
                    text: c.to_string(),
                    style: Style::Code,
                    url: None,
                });
            }
            // html=false 即用户气泡路径（render_user 唯一调用方）：段内软
            // 换行保留 \n（pi-web parity：.markdown-user-message p 的
            // white-space:pre-wrap——用户分次回车的多行按原文折行，而非
            // CommonMark 默认折叠成空格）
            Event::SoftBreak => text.push(if html { ' ' } else { '\n' }),
            Event::HardBreak => text.push('\n'),
            Event::Start(tag) => style_push(&mut styles, tag),
            Event::End(end) => style_pop(&mut styles, end),
            _ => {}
        }
        *i += 1;
    }
    flush(&mut text, cur, &cur_url, &mut runs);
    runs
}
