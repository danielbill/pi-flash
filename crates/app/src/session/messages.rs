//! Message model + row rendering for the session message panel (032,
//! pi-web MessageView.tsx parity). Free functions over Chat state; the
//! entity split lands in phase E (ARCHITECTURE.md §2).

use std::collections::HashSet;

use gpui::{MouseButton, SharedString, div, prelude::*, px, relative, rgb};
use pi_link::protocol::{content_blocks, Block, Usage};

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
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Msg {
    pub(crate) role: Role,
    pub(crate) blocks: Vec<Block>,
    pub(crate) usage: Option<UsageLine>,
    /// session entry id for user messages (fork anchor), filled from get_entries
    pub(crate) entry_id: Option<String>,
    /// wall-clock stamp, epoch ms (032 发送时间/回复时间): user = send time,
    /// assistant = generation end; from pi message payloads / session entries
    pub(crate) ts: Option<i64>,
    /// completion stamp (回复用时 anchor): pi's message-object timestamp is
    /// the START of generation (≈ the user's send time — real files show a
    /// ~300ms delta), so duration needs the entry write-time (tails) or the
    /// MessageEnd arrival (live). None in get_messages snapshots.
    pub(crate) end_ts: Option<i64>,
}

/// Per-message display metadata computed by the list owner (needs whole-list
/// lookarounds): pi-web showTimestamp rule + the turn's user stamp for
/// 回复用时.
#[derive(Clone, Copy, Default)]
pub(crate) struct MsgMeta {
    pub(crate) show_ts: bool,
    pub(crate) turn_user_ts: Option<i64>,
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

/// Per-message display metadata (pi-web ChatWindow renderMessage parity):
/// assistant rows show the reply-time meta only on the last one of an
/// exchange (no later assistant before the next user); every row's
/// 回复用时 anchor is the nearest prior user stamp.
pub(crate) fn compute_meta(messages: &[Msg], ix: usize) -> MsgMeta {
    let mut meta = MsgMeta::default();
    let Some(m) = messages.get(ix) else {
        return meta;
    };
    meta.show_ts = if m.role == Role::User {
        true
    } else {
        messages[ix + 1..]
            .iter()
            .all(|later| later.role != Role::Assistant)
    };
    for prev in messages[..ix].iter().rev() {
        if prev.role == Role::User {
            meta.turn_user_ts = prev.ts;
            break;
        }
    }
    meta
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
                        let rt = c.rt();
                        rt.update(cx, |r, _| {
                            if !r.collapsed.remove(&k) {
                                r.collapsed.insert(k);
                            }
                        });
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
    meta: MsgMeta,
    // copy flash for this row (032 复制 → 已复制, 1.5s)
    copied: bool,
) -> gpui::Div {
    let mut col = div().w_full().mb_4().flex().flex_col();
    if m.role == Role::User {
        // MessageView.tsx UserMessageView: right-aligned bubble, --user-bg,
        // radius 12, pad 8/12; bottom row under the bubble (right-aligned):
        // [copy] [edit-from-here] [new branch] on hover + send time always.
        let text = m.plain_text();
        let entry = m.entry_id.clone();
        let weak_copy = weak.clone();
        let weak_edit = weak.clone();
        let weak_fork = weak.clone();
        let copy_text = text.clone();
        let edit_text = text.clone();
        // 006 session font slot drives the chat bubble text
        let sf = crate::appearance::session_font();
        let session_family = sf.family;
        let session_size = sf.size;

        // action pill: 11px icon+label, dim → accent, hover-revealed.
        // variants: (icon, label) differ when the copy flash is lit.
        let action = |id: String, icon_name: &'static str, label: &'static str| {
            div()
                .id(SharedString::from(id))
                .flex()
                .items_center()
                .gap_1()
                .px_1p5()
                .py_0p5()
                .rounded(px(5.))
                .text_size(px(11.))
                .text_color(rgb(t.text_dim))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.accent)))
                .child(icon(icon_name, 11., t.text_dim))
                .child(SharedString::from(tr(label)))
        };

        let mut actions = div().flex().items_center().gap_0p5();
        // copy (copied state swaps the icon to a check + label, pi-web parity)
        let copy_pill = if copied {
            action(format!("copy-{msg_ix}"), "check", "已复制").text_color(rgb(t.accent))
        } else {
            action(format!("copy-{msg_ix}"), "copy", "复制")
        }
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let text = copy_text.clone();
            let _ = weak_copy.update(cx, |c, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                c.rt().update(cx, |r, cx| {
                    r.copy_flash = Some((msg_ix, std::time::Instant::now()));
                    r.spawn_flash_clear(cx);
                });
            });
        });
        actions = actions.child(copy_pill);
        // edit-from-here: prefill the composer with this message's text
        // (pi-web replaceMessage parity; RPC has no navigate_tree — recorded
        // deviation: no in-place tree move)
        actions = actions.child(
            action(format!("edit-{msg_ix}"), "pencil", "编辑")
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    let _ = weak_edit.update(cx, |c, cx| {
                        c.with_active_editor(cx, |r, _| r.input = edit_text.clone());
                        let focus = c.focus.clone();
                        window.focus(&focus);
                    });
                }),
        );
        // fork this exchange into a new session (existing fork anchor)
        if let Some(eid) = entry.clone() {
            actions = actions.child(
                action(format!("fork-{msg_ix}"), "git-branch", "新分支")
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let eid = eid.clone();
                        let _ = weak_fork.update(cx, |c, cx| {
                            c.rt().update(cx, |r, cx| r.fork_from_entry(eid, cx))
                        });
                    }),
            );
        }

        let mut bottom = div()
            .flex()
            .items_center()
            .mt(px(3.))
            .child(
                div()
                    .flex()
                    .gap_0p5()
                    .opacity(if copied { 1. } else { 0. })
                    .group_hover("usermsg", |s| s.opacity(1.))
                    .child(actions),
            );
        if let Some(ts) = m.ts {
            bottom = bottom.child(
                div()
                    .ml_1()
                    .text_size(px(10.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(crate::services::format::fmt_msg_time(ts))),
            );
        }
        let row = div()
            .id(SharedString::from(format!("msgrow-{msg_ix}")))
            .group("usermsg")
            .w_full()
            .flex()
            .flex_col()
            .items_end()
            .gap_0p5()
            .child(
                div()
                    .max_w(relative(0.85))
                    .px_3()
                    .py_2()
                    .rounded(px(12.))
                    .bg(rgb(t.user_bg))
                    .border_1()
                    .border_color(gpui::rgba(0x3b82f633))
                    .text_color(rgb(t.text))
                    .font_family(session_family.clone())
                    .text_size(px(session_size))
                    .child(SharedString::from(text)),
            )
            .child(bottom);
        col = col.child(row);
    } else {
        // reveal for the hover copy pill (group ancestor)
        col = col.group("astat");
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
            // stats bar (032): usage always; copy on hover; 用时+时间 on the
            // last assistant of the exchange (pi-web showTimestamp rule)
            let reply_text = m.plain_text();
            let weak_copy = weak.clone();
            let mut bar = div()
                .flex()
                .items_center()
                .gap_2()
                .mt_2()
                .font_family("Consolas")
                .text_xs()
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(usage_footer(u.input, u.output, u.cache_read, u.cost)));
            if !reply_text.trim().is_empty() {
                let mut pill = div()
                    .id(SharedString::from(format!("acopy-{msg_ix}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .py_0p5()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .opacity(0.)
                    .hover(|s| s.bg(rgb(t.bg_hover)).opacity(1.))
                    .group_hover("astat", |s| s.opacity(1.));
                pill = if copied {
                    pill.child(icon("check", 11., t.accent)).child(SharedString::from(tr("已复制")))
                } else {
                    pill.child(icon("copy", 11., t.text_dim)).child(SharedString::from(tr("复制")))
                };
                pill = pill.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let text = reply_text.clone();
                    let _ = weak_copy.update(cx, |c, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                        c.rt().update(cx, |r, cx| {
                            r.copy_flash = Some((msg_ix, std::time::Instant::now()));
                            r.spawn_flash_clear(cx);
                        });
                    });
                });
                bar = bar.child(pill);
            }
            if meta.show_ts {
                let mut meta_text = String::new();
                if let (Some(end), Some(start)) = (m.end_ts, meta.turn_user_ts) {
                    meta_text.push_str(&crate::services::format::fmt_duration_ms(end - start));
                    meta_text.push_str(" · ");
                }
                meta_text.push_str(&m.ts.map(crate::services::format::fmt_msg_time).unwrap_or_default());
                if !meta_text.is_empty() {
                    bar = bar.child(
                        div()
                            .ml_auto()
                            .text_size(px(10.))
                            .child(SharedString::from(meta_text)),
                    );
                }
            }
            col = col.child(bar);
        }
    }
    col
}

/// Convert raw session-file tail entries (`{"type":"message","message":{…}}`,
/// as produced by `pi_link::sessions::read_tail_messages`) into renderable
/// `Msg`s. Mirrors `Chat::ingest_message` semantics exactly: user/assistant
/// push, toolResult merges into the last assistant's tool call, other roles
/// skipped. Timestamps: the message object's epoch-ms `timestamp`, falling
/// back to the entry's ISO `timestamp` (032 发送时间).
pub(crate) fn msgs_from_tail(values: Vec<serde_json::Value>) -> Vec<Msg> {
    let mut out: Vec<Msg> = Vec::new();
    for v in values {
        let m = &v["message"];
        let role = m["role"].as_str().unwrap_or("");
        let blocks = content_blocks(&m["content"]);
        let usage = Usage::parse(&m["usage"]);
        let ts = m["timestamp"].as_i64().or_else(|| parse_iso_ms(&v["timestamp"]));
        let entry_ts = parse_iso_ms(&v["timestamp"]);
        match role {
            "user" => out.push(Msg {
                role: Role::User,
                blocks,
                usage: None,
                entry_id: None,
                ts,
                end_ts: None,
            }),
            "assistant" => out.push(Msg {
                role: Role::Assistant,
                blocks,
                usage: usage.map(|u| UsageLine {
                    input: u.input,
                    output: u.output,
                    cache_read: u.cache_read,
                    cost: u.cost,
                }),
                entry_id: None,
                ts,
                end_ts: entry_ts.or(ts),
            }),
            "toolResult" => {
                let text: String = blocks
                    .iter()
                    .map(|b| match b {
                        Block::Text { text, .. } => text.as_str(),
                        _ => "",
                    })
                    .collect::<Vec<_>>()
                    .join("")
                    .trim_end()
                    .to_string();
                if let Some(last) = out.last_mut() {
                    if last.role == Role::Assistant {
                        if let Some(Block::ToolCall { result, .. }) = last
                            .blocks
                            .iter_mut()
                            .rev()
                            .find(|b| matches!(b, Block::ToolCall { .. }))
                        {
                            result.push_str(&text);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// RFC3339 entry stamp -> epoch ms (session entries carry ISO timestamps).
fn parse_iso_ms(v: &serde_json::Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tail_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_user_assistant_and_merges_toolresult() {
        let vals = vec![
            json!({"type":"message","timestamp":"2026-09-27T01:02:03.456Z","message":{"role":"user","timestamp":1790470923456i64,"content":"查一下"}}),
            json!({"type":"message","message":{"role":"assistant","content":[
                    {"type":"thinking","text":"想"},
                    {"type":"toolCall","id":"t1","name":"web","arguments":{"q":"x"},"result":""}
                ],
                "usage":{"input":10,"output":5,"cache_read":0,"cost":0.01}}}),
            json!({"type":"message","message":{"role":"toolResult","content":[
                    {"type":"text","text":"结果内容"}]}}),
            json!({"type":"message","message":{"role":"system","content":"skip"}}),
        ];
        let msgs = msgs_from_tail(vals);
        assert_eq!(msgs.len(), 2, "toolResult merges; system skipped");
        assert_eq!(msgs[0].role, Role::User);
        // message-object timestamp wins over the entry ISO stamp
        assert_eq!(msgs[0].ts, Some(1790470923456));
        assert_eq!(msgs[1].role, Role::Assistant);
        let usage = msgs[1].usage.as_ref().expect("assistant usage kept");
        assert_eq!((usage.input, usage.output), (10, 5));
        let merged = msgs[1]
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::ToolCall { result, .. } => Some(result.as_str()),
                _ => None,
            })
            .expect("toolcall present");
        assert_eq!(merged, "结果内容");
    }

    #[test]
    fn iso_entry_timestamp_falls_back() {
        let vals = vec![json!({"type":"message","timestamp":"2026-09-27T01:02:03.456Z","message":{"role":"user","content":"hi"}})];
        let msgs = msgs_from_tail(vals);
        assert_eq!(msgs[0].ts, Some(1790470923456));
    }
}
