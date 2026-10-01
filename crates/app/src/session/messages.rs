//! Message model + row rendering for the session message panel (032,
//! pi-web MessageView.tsx parity). Free functions over Chat state; the
//! entity split lands in phase E (ARCHITECTURE.md §2).

use std::collections::HashSet;

use gpui::{MouseButton, SharedString, div, prelude::*, px, relative, rgb};
use pi_link::protocol::{content_blocks, Block, Usage};

use crate::Chat;
use crate::i18n::tr;
use crate::markdown;
use crate::services::format::pretty_args;
use crate::theme;
use crate::ui::{icon, icon_hover};

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
    /// AssistantMessage.stopReason (v56 error/length alert boxes)
    pub(crate) stop_reason: Option<String>,
    /// AssistantMessage.errorMessage — set when stop_reason == "error"
    pub(crate) error_message: Option<String>,
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
        Block::ToolCall { .. } => {
            // v54: 工具调用收进「工作详情」折叠行（render_msg 的工作折
            // 叠区），不再逐卡内联渲染
            div().w_full()
        }
        _ => div().w_full(),
    }
}

/// 「工作详情」折叠行 + 展开的工具调用列表（左细线缩进；tool · 对象）。
/// 按轮聚合：label 计整轮，tools 为整轮的 (工具名, 对象) 序列。
#[allow(clippy::too_many_arguments)]
fn work_fold(
    msg_ix: usize,
    n_msgs: usize,
    n_tools: usize,
    tools: &[(String, String)],
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashSet<(usize, usize)>,
    t: &theme::Theme,
) -> gpui::AnyElement {
    let key = (msg_ix, usize::MAX);
    let expanded = !collapsed.contains(&key);
    let label = crate::i18n::tf(
        "工作详情 · {m} 条消息 · {t} 次工具调用",
        &[
            ("m", n_msgs.to_string()),
            ("t", n_tools.to_string()),
        ],
    );
    let weak_fold = weak.clone();
    // 外层列容器：折叠行在上、展开体在下（此前 body 挂在 flex 行容器里
    // 被排到标签右侧——布局 bug）
    let mut wrap = div().flex().flex_col();
    let fold_row = div()
        .id(SharedString::from(format!("work-fold-{msg_ix}")))
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(4.))
        .ml(px(-8.))
        .text_size(px(12.))
        .text_color(rgb(t.text_dim))
        .cursor_pointer()
        .rounded(px(6.))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak_fold.update(cx, |c, cx| {
                let rt = c.rt();
                rt.update(cx, |r, _| {
                    if !r.collapsed.remove(&key) {
                        r.collapsed.insert(key);
                    }
                });
                cx.notify();
            });
        })
        .child(icon(
            if expanded { "chevron-down" } else { "chevron-right" },
            12.,
            t.text_dim,
        ))
        .child(SharedString::from(label));
    wrap = wrap.child(fold_row);
    if expanded {
        let mut body = div()
            .ml(px(14.))
            .pl(px(14.))
            .mb(px(10.))
            .border_l_2()
            .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
            .flex()
            .flex_col();
        for (tool, target) in tools {
            body = body.child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(4.))
                    .py(px(3.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(crate::i18n::tf(
                                "{tool} ·",
                                &[("tool", tool.to_string())],
                            ))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_color(rgb(t.text_faint))
                            .child(SharedString::from(target)),
                    ),
            );
        }
        wrap = wrap.child(body);
    }
    wrap.into_any_element()
}

/// 工具调用「对象」：主参数（file_path/command/pattern/url/query…），缺省
/// 用 pretty_args 首 40 字符。
fn tool_target(args: &str) -> String {
    const KEYS: &[&str] = &[
        "file_path", "path", "command", "pattern", "url", "query", "content", "text",
    ];
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        for k in KEYS {
            if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
                return s.chars().take(60).collect();
            }
        }
    }
    pretty_args(args).chars().take(40).collect()
}

pub(crate) fn render_msg(
    m: &Msg,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashSet<(usize, usize)>,
    t: &theme::Theme,
    // Some(est_tokens) 仅当此消息是流式中的最后一条（工作中回复）
    _stream_info: Option<u64>,
    _meta: MsgMeta,
    // copy flash for this row (032 复制 → 已复制, 1.5s)
    copied: bool,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(22.)).flex().flex_col();
    if m.role == Role::User {
        // v54 用户气泡：右对齐、62% 宽、radius 14、pad 9/15、无边框；
        // 操作行（复制/编辑/新分支+时间）hover 整行淡入，无框无底色。
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

        let action = |id: String, icon_name: &'static str, label: &'static str| {
            div()
                .id(SharedString::from(id))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(11.5))
                .text_color(rgb(t.text_dim))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(t.text)))
                .child(icon(icon_name, 12., t.text_dim))
                .child(SharedString::from(tr(label)))
        };

        let mut actions = div().flex().items_center().gap(px(12.));
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
            .gap(px(12.))
            .mt(px(6.))
            .pr(px(4.))
            .opacity(if copied { 1. } else { 0. })
            .group_hover("usermsg", |s| s.opacity(1.))
            .child(actions);
        if let Some(ts) = m.ts {
            bottom = bottom.child(
                div()
                    .ml(px(4.))
                    .text_size(px(11.5))
                    .text_color(rgb(t.text_faint))
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
            .gap(px(6.))
            .child(
                div()
                    .max_w(relative(0.62))
                    .px(px(15.))
                    .py(px(9.))
                    .rounded(px(14.))
                    .bg(rgb(t.user_bg))
                    .text_color(rgb(t.text))
                    .font_family(session_family.clone())
                    .text_size(px(session_size))
                    .child(SharedString::from(text)),
            )
            .child(bottom);
        col = col.child(row);
    } else {
        // 单条 assistant（仅当它不构成轮头时才会走到这里——session_list
        // 已把轮渲染收敛到 render_assistant_turn；此分支防御性保留）
        col = col.group("astat");
        for b in &m.blocks {
            col = col.child(render_block(b, msg_ix, weak, collapsed, t));
        }
    }
    col
}

/// 一轮 agent 回复（用户消息 → 下一用户消息之间的全部 assistant 消息）：
/// 工作详情折叠（整轮聚合）+ 模型名（一次）+ 依序正文 + hover 操作栏
/// （复制整轮文本 / 用时 / 时间）。pi-web AssistantMessageView 的轮级
/// 收敛 + v54 设计折叠行。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_assistant_turn(
    turn: &[&Msg],
    // 轮内各消息的全局索引（thinking/copy key 用）
    turn_ixs: &[usize],
    start_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashSet<(usize, usize)>,
    t: &theme::Theme,
    model_label: &str,
    // Some(est_tokens)：此轮正在流式（最后一个 assistant 消息）
    stream_info: Option<u64>,
    meta: MsgMeta,
    copied: bool,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(22.)).flex().flex_col().group("astat");

    // 整轮聚合：工具调用与正文
    let mut n_tools = 0usize;
    let mut tools: Vec<(String, String)> = Vec::new();
    let mut turn_text = String::new();
    for m in turn {
        for b in &m.blocks {
            match b {
                Block::ToolCall { name, args, .. } if !name.is_empty() => {
                    n_tools += 1;
                    tools.push((name.clone(), tool_target(args)));
                }
                Block::Text { text, .. } if !text.trim().is_empty() => {
                    if !turn_text.is_empty() {
                        turn_text.push_str("\n\n");
                    }
                    turn_text.push_str(text);
                }
                _ => {}
            }
        }
    }
    let n_msgs = turn.len();
    let is_working = stream_info.is_some();

    // 工作详情折叠行：仅已完成轮有（v54 §13）
    if n_tools > 0 && !is_working {
        col = col.child(work_fold(start_ix, n_msgs, n_tools, &tools, weak, collapsed, t));
    }
    // 模型名：每轮一次
    col = col.child(
        div()
            .text_size(px(11.))
            .text_color(rgb(t.text_dim))
            .mb(px(4.))
            .child(SharedString::from(model_label.to_string())),
    );
    // 正文：依序渲染各消息的 text/thinking 块（ToolCall 已收进折叠行）
    for (m, &gix) in turn.iter().zip(turn_ixs) {
        for b in &m.blocks {
            col = col.child(render_block(b, gix, weak, collapsed, t));
        }
    }
    // token 用量行（pi-web formatUsage parity：常显于轮尾，有值才显示）
    if let Some(u) = turn.iter().rev().find_map(|m| m.usage.as_ref()) {
        let mut parts: Vec<String> = Vec::new();
        if u.input > 0 {
            parts.push(format!("{} in", crate::services::format::fmt_thousand(u.input)));
        }
        if u.output > 0 {
            parts.push(format!("{} out", crate::services::format::fmt_thousand(u.output)));
        }
        if u.cache_read > 0 {
            parts.push(format!(
                "{} cache R",
                crate::services::format::fmt_thousand(u.cache_read)
            ));
        }
        if u.cost > 0. {
            parts.push(format!("${:.4}", u.cost));
        }
        if !parts.is_empty() {
            col = col.child(
                div()
                    .mt(px(2.))
                    .text_size(px(11.))
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(parts.join(" · "))),
            );
        }
    }
    // hover 操作栏：复制整轮文本 + 用时 + 时间（末条消息的时间戳）
    let weak_copy = weak.clone();
    let mut bar = div()
        .flex()
        .items_center()
        .gap(px(14.))
        .mt(px(6.))
        .text_size(px(11.5))
        .text_color(rgb(t.text_dim))
        .opacity(0.)
        .group_hover("astat", |s| s.opacity(1.));
    if !turn_text.trim().is_empty() {
        let mut pill = div()
            .id(SharedString::from(format!("acopy-{start_ix}")))
            .flex()
            .items_center()
            .gap(px(4.))
            .cursor_pointer()
            .hover(|s| s.text_color(rgb(t.text)));
        pill = if copied {
            pill.child(icon_hover("check", 12., t.accent)).child(SharedString::from(tr("已复制")))
        } else {
            pill.child(icon_hover("copy", 12., t.text_dim)).child(SharedString::from(tr("复制")))
        };
        pill = pill.on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let text = turn_text.clone();
            let _ = weak_copy.update(cx, |c, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                c.rt().update(cx, |r, cx| {
                    r.copy_flash = Some((start_ix, std::time::Instant::now()));
                    r.spawn_flash_clear(cx);
                });
            });
        });
        bar = bar.child(pill);
    }
    // 用时/时间取轮内末条消息
    if let Some(last) = turn.last() {
        let mut meta_text = String::new();
        if let (Some(end), Some(start)) = (last.end_ts, meta.turn_user_ts) {
            meta_text.push_str(&format!(
                "{}{}",
                tr("用时"),
                crate::services::format::fmt_duration_ms(end - start)
            ));
            meta_text.push_str("  ");
        }
        meta_text.push_str(&last.ts.map(crate::services::format::fmt_msg_time).unwrap_or_default());
        if !meta_text.trim().is_empty() {
            bar = bar.child(
                div().text_color(rgb(t.text_faint)).child(SharedString::from(meta_text)),
            );
        }
    }
    col = col.child(bar);
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
                stop_reason: None,
                error_message: None,
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
                stop_reason: m["stopReason"].as_str().map(str::to_string),
                error_message: m["errorMessage"].as_str().map(str::to_string),
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
