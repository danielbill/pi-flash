//! Message model + row rendering for the session message panel (032,
//! pi-web MessageView.tsx parity). Free functions over Chat state; the
//! entity split lands in phase E (ARCHITECTURE.md §2).

use std::collections::HashSet;

use gpui::{MouseButton, SharedString, div, prelude::*, px, relative, rgb};
use pi_link::protocol::Block;

use crate::Chat;
use crate::i18n::tr;
use crate::markdown;
use crate::services::format::{pretty_args, tps_color, usage_footer};
use crate::theme;
use crate::ui::icon;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct UsageLine {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cost: f64,
    pub(crate) time: String,
}

pub(crate) struct Msg {
    pub(crate) role: Role,
    pub(crate) blocks: Vec<Block>,
    pub(crate) usage: Option<UsageLine>,
    /// session entry id for user messages (fork anchor), filled from get_entries
    pub(crate) entry_id: Option<String>,
}

impl Msg {
    pub(crate) fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Text { text, .. } => text.as_str(),
                _ => "",
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

pub(crate) fn render_block(
    b: &Block,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashSet<(usize, usize)>,
    t: &theme::Theme,
) -> gpui::Div {
    match b {
        Block::Text { text, .. } if !text.trim().is_empty() => {
            div().w_full().child(markdown::render_themed(text))
        }
        Block::Thinking { text, content_index } if !text.trim().is_empty() => {
            let key = (msg_ix, *content_index);
            let expanded = !collapsed.contains(&key);
            let weak = weak.clone();
            // pi-web getThinkingPreview: first line, up to 240 chars, trimmed
            let preview: String = text
                .trim_start()
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(240)
                .collect::<String>()
                .trim_end()
                .to_string();
            // pi-web ThinkingBlock: flat var(--bg) strip with 1px border —
            // [lightbulb] [one-line preview] collapsed, [lightbulb] [pre-wrap
            // muted body] expanded; amber bulb while expanded, no chevron, no
            // "thinking" label
            let toggle = div()
                .id(SharedString::from(format!("th-{msg_ix}-{content_index}")))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .text_color(rgb(t.text_muted))
                .when(expanded, |d| d.flex_shrink_0())
                .when(!expanded, |d| d.flex_1())
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let k = key;
                    let _ = weak.update(cx, |c, cx| {
                        if !c.collapsed.remove(&k) {
                            c.collapsed.insert(k);
                        }
                        cx.notify();
                    });
                })
                .child(if expanded {
                    icon("lightbulb", 14., 0xd4a017)
                } else {
                    icon("lightbulb", 14., t.text_muted)
                })
                .children((!expanded).then(|| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(if preview.is_empty() {
                            SharedString::from("...").into_any_element()
                        } else {
                            SharedString::from(preview).into_any_element()
                        })
                }));
            let mut block = div()
                .w_full()
                .my_1()
                .flex()
                .items_start()
                .gap_1p5()
                .min_w_0()
                .px(px(10.))
                .py(px(6.))
                .rounded(px(7.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg))
                .font_family("Consolas")
                .text_size(px(11.))
                .line_height(relative(1.5))
                .child(toggle);
            if expanded {
                block = block.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(text.clone())),
                );
            }
            block
        }
        Block::ToolCall { name, args, result, .. } if !name.is_empty() => {
            let mut card = div()
                .w_full()
                .my_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.tool_bg))
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .font_family("Consolas")
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.accent))
                        .child(SharedString::from(name.clone())),
                )
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(pretty_args(args))),
                );
            if !result.is_empty() {
                card = card.child(
                    div()
                        .px_2()
                        .pb_1()
                        .mt_1()
                        .border_t_1()
                        .border_color(rgb(t.border))
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(rgb(t.text))
                        .child(SharedString::from(result.clone())),
                );
            }
            card
        }
        _ => div().w_full(),
    }
}

pub(crate) fn render_msg(
    m: &Msg,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashSet<(usize, usize)>,
    t: &theme::Theme,
    model_label: &str,
    // while this message is streaming: (estimated tokens, tok/s)
    stream_info: Option<(u64, Option<f32>)>,
) -> gpui::Div {
    let mut col = div().w_full().mb_4().flex().flex_col();
    if m.role == Role::User {
        // MessageView.tsx: right-aligned bubble, --user-bg, radius 12, pad 8/12.
        // UserMessageView hover toolbar: fork button (git-branch 11px) appears
        // on hover and forks the session before this user message.
        let text = m.plain_text();
        let entry = m.entry_id.clone();
        let weak_fork = weak.clone();
        let mut row = div()
            .id(SharedString::from(format!("msgrow-{msg_ix}")))
            .group("usermsg")
            .w_full()
            .flex()
            .flex_col()
            .items_end()
            .gap_0p5();
        if entry.is_some() {
            row = row.child(
                div()
                    .id(SharedString::from(format!("fork-{msg_ix}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.))
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .opacity(0.)
                    .group_hover("usermsg", |s| s.opacity(1.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.accent)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        if let Some(eid) = entry.clone() {
                            let _ = weak_fork.update(cx, |c, cx| {
                                c.fork_from_entry(eid, cx)
                            });
                        }
                    })
                    .child(icon("git-branch", 11., t.text_dim))
                    .child(SharedString::from(tr("新分支"))),
            );
        }
        row = row.child(
            div()
                .max_w(relative(0.85))
                .px_3()
                .py_2()
                .rounded(px(12.))
                .bg(rgb(t.user_bg))
                .border_1()
                .border_color(gpui::rgba(0x3b82f633))
                .text_color(rgb(t.text))
                .text_size(px(14.))
                .child(SharedString::from(text)),
        );
        col = col.child(row);
    } else {
        // MessageView: model label 11px --text-dim, margin-bottom 4; while
        // streaming add the estimated-token arrow + speed badge
        col = col.child(
            div()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .mb_1()
                .flex()
                .items_center()
                .gap_1p5()
                .child(SharedString::from(model_label.to_string()))
                .children(stream_info.and_then(|(est, tps)| {
                    (est > 0).then(|| {
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_color(rgb(t.text))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_0p5()
                                    .text_size(px(11.))
                                    .child("\u{2193}"),
                            )
                            .child(SharedString::from(est.to_string()))
                            .children(tps.map(|v| {
                                div()
                                    .ml(px(6.))
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded(px(4.))
                                    .bg(rgb(tps_color(v)))
                                    .text_size(px(11.))
                                    .text_color(gpui::rgb(0xffffff))
                                    .child(SharedString::from(format!("{:.1} t/s", v)))
                            }))
                    })
                })),
        );
        for b in &m.blocks {
            col = col.child(render_block(b, msg_ix, weak, collapsed, t));
        }
        if let Some(u) = &m.usage {
            col = col.child(
                div()
                    .flex()
                    .justify_between()
                    .mt_2()
                    .font_family("Consolas")
                    .text_xs()
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(usage_footer(u.input, u.output, u.cache_read, u.cost)))
                    .child(SharedString::from(u.time.clone())),
            );
        }
    }
    col
}
