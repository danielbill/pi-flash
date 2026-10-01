//! Message model + row rendering for the session message panel (032,
//! pi-web MessageView.tsx parity). Free functions over Chat state; the
//! entity split lands in phase E (ARCHITECTURE.md §2).

use std::collections::HashMap;

use gpui::{Animation, AnimationExt, FontWeight, MouseButton, SharedString, TextAlign, div, prelude::*, px, relative, rgb};
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
    /// pi CustomMessage (compaction summary, extension messages,
    /// branch summaries) — v56-6 renders the card; skipped until then
    Custom,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct UsageLine {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
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
    /// CustomMessage.customType (None for user/assistant)
    pub(crate) custom_type: Option<String>,
    /// CustomMessage.display — false = hidden extension payload
    pub(crate) custom_display: bool,
    /// CustomMessage.details (compaction: tokensBefore/firstKeptEntryId)
    pub(crate) details: Option<serde_json::Value>,
    /// AssistantMessage.model — per-message label (pi-web getModelDisplayName
    /// source); None on live-streamed messages until MessageEnd
    pub(crate) model: Option<String>,
}

/// Per-message display metadata computed by the list owner (needs whole-list
/// lookarounds): pi-web showTimestamp rule + the turn's user stamp for
/// 回复用时.
#[derive(Clone, Copy, Default)]
pub(crate) struct MsgMeta {
    pub(crate) show_ts: bool,
    pub(crate) turn_user_ts: Option<i64>,
}

/// Decoded image payload from a toolResult content array (v56-0 c3).
pub(crate) fn result_payload(blocks: &[Block]) -> (String, Vec<pi_link::protocol::ImageData>) {
    use pi_link::protocol::ImageData;
    let mut text = String::new();
    let mut images = Vec::new();
    for b in blocks {
        match b {
            Block::Text { text: t, .. } => text.push_str(t),
            Block::Image { mime, data, .. } => images.push(ImageData {
                mime: mime.clone(),
                data: data.clone(),
            }),
            _ => {}
        }
    }
    (text.trim_end().to_string(), images)
}

/// Merge a toolResult payload into the paired ToolCall of `msg` (must be the
/// last assistant message). Correlates by toolCallId; blind-fallback to the
/// last call keeps old snapshots without ids working.
pub(crate) fn merge_tool_result(
    msg: &mut Msg,
    tool_call_id: Option<&str>,
    is_error: bool,
    text: &str,
    images: Vec<pi_link::protocol::ImageData>,
    details: Option<serde_json::Value>,
    result_ts: Option<i64>,
) {
    if msg.role != Role::Assistant {
        return;
    }
    // two-pass by index: rposition's immutable borrow ends before get_mut
    let ix = msg
        .blocks
        .iter()
        .rposition(|b| matches!(b, Block::ToolCall { id, .. } if tool_call_id == Some(id.as_str())))
        .or_else(|| msg.blocks.iter().rposition(|b| matches!(b, Block::ToolCall { .. })));
    // pi-web {n}s duration = result arrival − message start (rounded s)
    let duration_s = match (result_ts, msg.ts) {
        (Some(r), Some(start)) => Some(((r - start).max(0)) / 1000),
        _ => None,
    };
    if let Some(ix) = ix {
        if let Some(Block::ToolCall { result, is_error: err, images: imgs, duration_s: dur, details: det, .. }) =
            msg.blocks.get_mut(ix)
        {
            result.push_str(text);
            *err = is_error;
            imgs.extend(images);
            *det = details;
            *dur = duration_s;
        }
    }
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
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    // 流式中的消息：markdown 代码块跳过 syntect 高亮（v56-3 c15）
    streaming: bool,
) -> gpui::Div {
    match b {
        Block::Text { text, .. } if !text.trim().is_empty() => {
            div().w_full().child(markdown::render(text, t, streaming))
        }
        Block::Thinking { text, content_index } if !text.trim().is_empty() => {
            let key = (msg_ix, *content_index);
            let expanded = collapsed.get(&key).copied().unwrap_or(true);
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
                            let next = !r.collapsed.get(&k).copied().unwrap_or(true);
                            r.collapsed.insert(k, next);
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
        Block::ToolCall {
            content_index,
            name,
            args,
            result,
            is_error,
            images,
            duration_s,
            details,
            args_partial,
            result_arrived,
            ..
        } => {
            // v56-2 c8/c9: pi-web ToolCallBlock parity —— 状态色卡片 +
            // 参数摘要 + 耗时 + 旋转箭头 + 展开体（参数 pre / 结果 pre）；
            // diff（c10）与图片（c11）见后续提交。收进「工作详情」组内。
            render_tool_card(
                msg_ix,
                *content_index,
                name,
                args,
                result,
                *is_error,
                images,
                details.as_ref(),
                *duration_s,
                *args_partial,
                *result_arrived,
                weak,
                collapsed,
                t,
            )
        }
        _ => div().w_full(),
    }
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

fn rgba_a(rgb24: u32, alpha: f32) -> u32 {
    let a = (alpha * 255.0).round().clamp(0.0, 255.0) as u32;
    ((rgb24 & 0xffffff) | (a << 24)) as u32
}

/// pi-web getToolPreview：command/path/file_path/pattern/query 键序取值
/// 截 120 字符，都没有取首个键的值。
fn tool_preview(args: &str) -> String {
    const KEYS: &[&str] = &["command", "path", "file_path", "pattern", "query"];
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return String::new();
    };
    let Some(obj) = v.as_object() else { return String::new() };
    if obj.is_empty() {
        return String::new();
    }
    let value_of = |k: &str| -> Option<String> {
        obj.get(k).map(|x| match x {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    };
    let raw = KEYS.iter().find_map(|k| value_of(k)).or_else(|| {
        obj.iter().next().and_then(|(_, v)| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        })
    });
    raw.map(|r| r.chars().take(120).collect()).unwrap_or_default()
}

/// pi-web summarizeApplyPatchInput：apply_patch 目标文件路径 join 截 120。
fn summarize_apply_patch(args: &str) -> String {
    let text = serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get("input").and_then(|x| x.as_str()).map(str::to_string))
        .unwrap_or_else(|| args.to_string());
    let paths = crate::session::diff::extract_apply_patch_paths(&text);
    let joined = paths.join(", ");
    joined.chars().take(120).collect()
}

/// 参数展开体文本：流式原始串；完整参数 pretty JSON（pi-web
/// JSON.stringify(input, null, 2) parity）。
fn tool_args_display(args: &str, partial: bool) -> String {
    if partial {
        return args.to_string();
    }
    serde_json::from_str::<serde_json::Value>(args)
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or_else(|_| args.to_string()))
        .unwrap_or_else(|_| args.to_string())
}

/// 结果空文案判定：空串或 pi 的 "(no output)"。
fn result_is_empty(result: &str) -> bool {
    let r = result.trim();
    r.is_empty() || r == "(no output)"
}

/// 「工作详情」组内的 pi-web ToolCallBlock 卡片（v56-2 c8/c9）。
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn render_tool_card(
    msg_ix: usize,
    content_index: usize,
    name: &str,
    args: &str,
    result: &str,
    is_error: bool,
    images: &[pi_link::protocol::ImageData],
    details: Option<&serde_json::Value>,
    duration_s: Option<i64>,
    args_partial: bool,
    result_arrived: bool,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
) -> gpui::Div {
    let _ = images; // c11 渲染
    let key = (msg_ix, content_index);
    // pi-web isToolCallExpanded：模块级记忆，默认全部折叠
    let expanded = collapsed.get(&key).copied().unwrap_or(false);
    let is_patch = crate::session::diff::is_apply_patch_tool_name(name);
    let is_edit = crate::session::diff::is_edit_tool_name(name);

    // c10: diff 视图数据 —— apply_patch 优先解析输入 V4A 文档，失败退
    // details.preview；write/edit 取 details.patch/.diff（统一 diff）
    let input_text = serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get("input").and_then(|x| x.as_str()).map(str::to_string))
        .unwrap_or_else(|| args.to_string());
    let patch_files = if is_patch {
        crate::session::diff::parse_apply_patch_input(&input_text)
            .or_else(|| {
                if is_error {
                    None
                } else {
                    details
                        .as_ref()
                        .and_then(|d| crate::session::diff::apply_patch_preview_to_files(d))
                }
            })
            .map(|f| (f, false))
    } else {
        None
    };
    let result_diff = if result_arrived && !is_error {
        details.as_ref().and_then(|d| {
            d.get("patch")
                .or_else(|| d.get("diff"))
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
    } else {
        None
    };

    // 状态色（pi-web 成功绿/失败红 边框+底色+工具名）
    let (border_c, bg_c, name_c, top_c) = if is_error {
        (
            rgba_a(0xf87171, 0.45),
            rgba_a(0xf87171, 0.05),
            0xf87171,
            rgba_a(0xf87171, 0.25),
        )
    } else {
        (
            rgba_a(0x22c55e, 0.25),
            rgba_a(0x22c55e, 0.04),
            0x16a34a,
            rgba_a(0x22c55e, 0.2),
        )
    };

    // 标题行：工具名 + 参数摘要（apply_patch 用路径摘要）+ 耗时 + 箭头
    let preview = if args_partial && result.is_empty() && !result_arrived {
        tr("正在生成参数...").to_string()
    } else if is_patch {
        let s = summarize_apply_patch(args);
        if s.is_empty() {
            tool_preview(args)
        } else {
            s
        }
    } else {
        tool_preview(args)
    };

    let weak_toggle = weak.clone();
    let mut head = div()
        .id(SharedString::from(format!("tc-{msg_ix}-{content_index}")))
        .flex()
        .items_center()
        .gap(px(7.))
        .px(px(10.))
        .py(px(6.))
        .min_w_0()
        .flex_1()
        .text_size(px(12.))
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak_toggle.update(cx, |c, cx| {
                let rt = c.rt();
                rt.update(cx, |r, _| {
                    let next = !r.collapsed.get(&key).copied().unwrap_or(false);
                    r.collapsed.insert(key, next);
                });
                cx.notify();
            });
        })
        .child(
            div()
                .flex_shrink_0()
                .font_family("Consolas")
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(name_c))
                .child(SharedString::from(name.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family("Consolas")
                .text_size(px(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(preview)),
        );
    if let Some(d) = duration_s {
        head = head.child(
            div()
                .flex_shrink_0()
                .text_size(px(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(format!("{d}s"))),
        );
    }
    // 箭头：pi-web 收起=下、展开=上（150ms 旋转过渡）
    head = head.child(if expanded {
        gpui::svg()
            .path(SharedString::from("icons/chevron-down.svg"))
            .text_color(rgb(t.text_dim))
            .size(px(10.))
            .flex_shrink_0()
            .with_animation(
                SharedString::from(format!("tchev-{msg_ix}-{content_index}")),
                Animation::new(std::time::Duration::from_millis(150)),
                |el, delta| {
                    el.with_transformation(gpui::Transformation::rotate(
                        gpui::radians(delta * std::f32::consts::PI),
                    ))
                },
            )
            .into_any_element()
    } else {
        icon("chevron-down", 10., t.text_dim)
    });

    let mut card = div()
        .w_full()
        .rounded(px(7.))
        .overflow_hidden()
        .text_size(px(12.))
        .border_1()
        .border_color(gpui::rgba(border_c))
        .bg(gpui::rgba(bg_c))
        .child(head);

    // 展开体：参数 pre（有 diff 视图时让位——pi-web !patchFiles 门）
    if expanded && (args_partial || !is_edit) && !is_patch && patch_files.is_none() {
        card = card.child(
            div()
                .border_t_1()
                .border_color(gpui::rgba(top_c))
                .bg(rgb(t.bg_subtle))
                .px(px(10.))
                .py(px(8.))
                .font_family("Consolas")
                .text_size(px(12.))
                .line_height(relative(1.5))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(tool_args_display(args, args_partial))),
        );
    }
    // 结果图片（c11）：始终显示，与折叠无关（pi-web ResultImages）
    if !images.is_empty() {
        card = card.child(result_images(images, is_error, t));
    }
    // 展开体（pi-web 决策树）：patchFiles > resultDiff(SplitPatch) >
    // PairedResult；patch 视图存在且失败时结果文本仍然显示
    if expanded {
        if let Some((files, _)) = &patch_files {
            card = card.child(
                div()
                    .border_t_1()
                    .border_color(gpui::rgba(rgba_a(0x22c55e, 0.15)))
                    .bg(rgb(t.bg))
                    .child(split_files_view(files, t)),
            );
            if is_error && result_arrived && !result_is_empty(result) {
                card = card.child(paired_result(result, is_error, t, msg_ix, content_index));
            }
        } else if let Some(diff_text) = &result_diff {
            card = card.child(
                div()
                    .border_t_1()
                    .border_color(gpui::rgba(rgba_a(0x22c55e, 0.15)))
                    .bg(rgb(t.bg))
                    .child(match crate::session::diff::parse_unified_patch(diff_text) {
                        Some(files) => split_files_view(&files, t),
                        None => patch_text_view(diff_text, t),
                    }),
            );
        } else if result_arrived && (is_error || !result_is_empty(result)) {
            card = card.child(paired_result(result, is_error, t, msg_ix, content_index));
        }
    }
    card
}

/// pi-web PairedResult：maxHeight 400 滚动、空结果斜体 0.6、错误红字。
fn paired_result(
    result: &str,
    is_error: bool,
    t: &theme::Theme,
    msg_ix: usize,
    content_index: usize,
) -> gpui::AnyElement {
    let empty = result_is_empty(result);
    let text = if empty {
        tr("（无输出）").to_string()
    } else {
        result.to_string()
    };
    div()
        .id(SharedString::from(format!("tres-{msg_ix}-{content_index}")))
        .max_h(px(400.))
        .overflow_y_scroll()
        .border_t_1()
        .border_color(gpui::rgba(if is_error {
            rgba_a(0xf87171, 0.3)
        } else {
            rgba_a(0x22c55e, 0.15)
        }))
        .bg(if is_error {
            gpui::rgba(rgba_a(0xf87171, 0.04))
        } else {
            rgb(t.bg_subtle)
        })
        .px(px(10.))
        .py(px(8.))
        .font_family("Consolas")
        .text_size(px(12.))
        .line_height(relative(1.5))
        .when(empty, |d| d.italic().text_color(rgb(t.text_dim)).opacity(0.6))
        .when(!empty && is_error, |d| d.text_color(rgb(0xf87171)))
        .when(!empty && !is_error, |d| d.text_color(rgb(t.text_muted)))
        .child(SharedString::from(text))
        .into_any_element()
}

/// pi-web ResultImages parity（v56-2 c11）：flex wrap、图片
/// maxWidth 720/maxHeight 520、圆角 6、1px 边框；10MB 上限、
/// png/jpeg/webp/gif/bmp 白名单（pi-web 另含 avif，gpui 无解码器跳过）。
fn result_images(
    images: &[pi_link::protocol::ImageData],
    is_error: bool,
    t: &theme::Theme,
) -> gpui::AnyElement {
    const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
    let mut wrap = div()
        .flex()
        .flex_wrap()
        .gap(px(8.))
        .p(px(10.))
        .bg(rgb(t.bg))
        .border_t_1()
        .border_color(gpui::rgba(if is_error {
            rgba_a(0xf87171, 0.3)
        } else {
            rgba_a(0x22c55e, 0.15)
        }));
    for data in images {
        let Some(format) = mime_to_image_format(&data.mime) else {
            continue;
        };
        let Ok(bytes) = decode_image_data(&data.data) else {
            continue;
        };
        if bytes.len() > MAX_IMAGE_BYTES {
            continue;
        }
        wrap = wrap.child(
            gpui::img(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
                .max_w(px(720.))
                .max_h(px(520.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border)),
        );
    }
    wrap.into_any_element()
}

fn mime_to_image_format(mime: &str) -> Option<gpui::ImageFormat> {
    match mime {
        "image/png" => Some(gpui::ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(gpui::ImageFormat::Jpeg),
        "image/gif" => Some(gpui::ImageFormat::Gif),
        "image/bmp" => Some(gpui::ImageFormat::Bmp),
        "image/webp" => Some(gpui::ImageFormat::Webp),
        "image/svg+xml" => Some(gpui::ImageFormat::Svg),
        _ => None,
    }
}

fn decode_image_data(data: &str) -> Result<Vec<u8>, ()> {
    use base64::Engine as _;
    let trimmed = data.trim();
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed))
        .map_err(|_| ())
}

/// pi-web SplitFilesView：双栏 split diff，maxHeight 560 滚动；多文件时
/// 显示文件头（pi-web sticky 头在 gpui 无对应，退化为普通行）。
fn split_files_view(files: &[crate::session::diff::DiffFile], t: &theme::Theme) -> gpui::AnyElement {
    use crate::session::diff::Row;
    let show_headers = files.len() > 1;
    let mut wrap = div()
        .id(SharedString::from(SharedString::from(format!("sd-{}", files.len()))))
        .max_h(px(560.))
        .overflow_y_scroll()
        .min_w_0()
        .font_family("Consolas")
        .text_size(px(12.))
        .line_height(relative(1.55));
    for (fix, file) in files.iter().enumerate() {
        let mut fcol = div().min_w_0();
        if fix > 0 {
            fcol = fcol.border_t_1().border_color(rgb(t.border));
        }
        if show_headers {
            let header = |title: Option<&String>, left: bool| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(10.))
                    .py(px(5.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(rgb(t.text_dim))
                    .when(left, |d| d.border_r_1().border_color(rgb(t.border)))
                    .child(SharedString::from(
                        title
                            .map(|s| s.clone())
                            .unwrap_or_else(|| tr("之前").to_string()),
                    ))
                    .into_any_element()
            };
            // 右栏标题：newPath 缺省显示「之后」
            let right_title = match &file.new_path {
                Some(p) => Some(p.clone()),
                None => None,
            };
            fcol = fcol.child(
                div()
                    .flex()
                    .min_w_0()
                    .bg(rgb(t.bg_panel))
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .child(header(file.old_path.as_ref(), true))
                    .child(header(right_title.as_ref(), false)),
            );
        }
        for row in &file.rows {
            match row {
                Row::Hunk(_) => {}
                Row::Line { left, right } => {
                    fcol = fcol
                        .child(diff_cell(left, true, t))
                        .child(diff_cell(right, false, t));
                }
            }
        }
        wrap = wrap.child(fcol);
    }
    wrap.into_any_element()
}

/// pi-web SplitDiffCellView：行号槽 42px + 标记列 18px + 文本。
fn diff_cell(
    cell: &crate::session::diff::Cell,
    left_side: bool,
    t: &theme::Theme,
) -> gpui::AnyElement {
    use crate::session::diff::CellKind;
    let bg = match cell.kind {
        CellKind::Added => gpui::rgba(rgba_a(0x22c55e, 0.12)),
        CellKind::Removed => gpui::rgba(rgba_a(0xf87171, 0.13)),
        CellKind::Empty => rgb(t.bg_subtle),
        CellKind::Context => gpui::rgba(0),
    };
    let marker = match cell.kind {
        CellKind::Added => "+",
        CellKind::Removed => "-",
        _ => " ",
    };
    let marker_color = match cell.kind {
        CellKind::Added => 0x22c55e,
        CellKind::Removed => 0xf87171,
        _ => t.text_dim,
    };
    let line_no = cell
        .line_no
        .map(|n| n.to_string())
        .unwrap_or_default();
    div()
        .flex()
        .min_w_0()
        .bg(bg)
        .when(left_side, |d| d.border_r_1().border_color(rgb(t.border)))
        .child(
            div()
                .w(px(42.))
                .px(px(6.))
                .flex_shrink_0()
                .text_align(TextAlign::Right)
                .text_color(rgb(t.text_dim))
                .bg(rgb(t.bg_panel))
                .border_r_1()
                .border_color(rgb(t.border))
                .child(SharedString::from(line_no)),
        )
        .child(
            div()
                .w(px(18.))
                .px(px(5.))
                .flex_shrink_0()
                .text_color(rgb(marker_color))
                .when(matches!(cell.kind, CellKind::Added | CellKind::Removed), |d| {
                    d.font_weight(FontWeight::BOLD)
                })
                .child(SharedString::from(marker)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(10.))
                .text_color(rgb(if matches!(cell.kind, CellKind::Empty) {
                    t.text_dim
                } else {
                    t.text
                }))
                .child(SharedString::from(if cell.text.is_empty() {
                    "\u{a0}".to_string()
                } else {
                    cell.text.clone()
                })),
        )
        .into_any_element()
}

/// pi-web PatchTextView：单栏回退（unified 解析失败时），maxHeight 520。
fn patch_text_view(text: &str, t: &theme::Theme) -> gpui::AnyElement {
    div()
        .id("ptext")
        .max_h(px(520.))
        .overflow_y_scroll()
        .min_w_0()
        .font_family("Consolas")
        .text_size(px(12.))
        .line_height(relative(1.55))
        .children(text.lines().enumerate().map(|(i, line)| {
            let kind = if line.starts_with("@@") {
                0
            } else if line.starts_with('+') && !line.starts_with("+++") {
                1
            } else if line.starts_with('-') && !line.starts_with("---") {
                2
            } else {
                3
            };
            let (bg, color, bar) = match kind {
                0 => (gpui::rgba(rgba_a(0x60a5fa, 0.12)), t.accent, t.accent),
                1 => (gpui::rgba(rgba_a(0x22c55e, 0.12)), 0x22c55e, 0x22c55e),
                2 => (gpui::rgba(rgba_a(0xf87171, 0.13)), 0xf87171, 0xf87171),
                _ => (gpui::rgba(0), t.text, 0),
            };
            div()
                .flex()
                .min_w_0()
                .bg(bg)
                .when(kind != 3, |d| d.border_l_3().border_color(rgb(bar)))
                .when(kind == 3, |d| d.border_l_3().border_color(gpui::rgba(0)))
                .child(
                    div()
                        .w(px(48.))
                        .px(px(8.))
                        .flex_shrink_0()
                        .text_align(TextAlign::Right)
                        .text_color(rgb(t.text_dim))
                        .bg(rgb(t.bg_panel))
                        .border_r_1()
                        .border_color(rgb(t.border))
                        .child(SharedString::from((i + 1).to_string())),
                )
                .child(
                    div()
                        .px(px(10.))
                        .min_w_0()
                        .text_color(rgb(color))
                        .child(SharedString::from(if line.is_empty() {
                            "\u{a0}".to_string()
                        } else {
                            line.to_string()
                        })),
                )
        }))
        .into_any_element()
}

pub(crate) fn render_msg(
    m: &Msg,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    // Some(est_tokens) 仅当此消息是流式中的最后一条（工作中回复）
    _stream_info: Option<u64>,
    _meta: MsgMeta,
    // copy flash for this row (032 复制 → 已复制, 1.5s)
    copied: bool,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(22.)).flex().flex_col();
    if m.role == Role::User {
        // v56-4 c18-c22（pi-web UserMessageView parity）：右对齐 85% 宽、
        // user_bg 底 + 1px 蓝边框 rgba(59,130,246,.2)、圆角 12、pad 8/12、
        // 内容走 markdown、内嵌图片 240 上限、超高 300px 内部滚动；
        // 操作行（复制/编辑/新分支）hover 淡入 = 自定义豁免项。
        let text = m.plain_text();
        let entry = m.entry_id.clone();
        let weak_copy = weak.clone();
        let weak_edit = weak.clone();
        let weak_fork = weak.clone();
        let copy_text = text.clone();
        let edit_text = text.clone();

        // c20: 用户消息内嵌图片（flex wrap、240 上限、蓝边框）
        let image_block: gpui::AnyElement = {
            let imgs: Vec<&Block> =
                m.blocks.iter().filter(|b| matches!(b, Block::Image { .. })).collect();
            if imgs.is_empty() {
                div().into_any_element()
            } else {
                let mut wrap = div().flex().flex_wrap().gap(px(6.)).mb(px(8.));
                for b in imgs {
                    if let Block::Image { mime, data, .. } = b {
                        if let Some(format) = mime_to_image_format(mime) {
                            if let Ok(bytes) = decode_image_data(data) {
                                wrap = wrap.child(
                                    gpui::img(std::sync::Arc::new(gpui::Image::from_bytes(
                                        format, bytes,
                                    )))
                                    .max_w(px(240.))
                                    .max_h(px(240.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(gpui::rgba(rgba_a(0x3b82f6, 0.15))),
                                );
                            }
                        }
                    }
                }
                wrap.into_any_element()
            }
        };
        // c19: 用户内容走 markdown（pi-web SafeMarkdownBody parity）
        let md: gpui::AnyElement = if text.trim().is_empty() {
            div().into_any_element()
        } else {
            markdown::render(&text, t, false)
        };

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

        // pi-web UserMessageView footer parity（v56-1 c7）：操作按钮 hover
        // 淡入（自定义豁免），时间戳常显 10px 右对齐
        let mut actions_wrap = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .opacity(if copied { 1. } else { 0. })
            .group_hover("usermsg", |s| s.opacity(1.))
            .child(actions);
        if copied {
            actions_wrap = actions_wrap.opacity(1.);
        }
        let mut bottom = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(12.))
            .mt(px(6.))
            .pr(px(4.))
            .child(actions_wrap);
        if let Some(ts) = m.ts {
            bottom = bottom.child(
                div()
                    .text_size(px(10.))
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
            .gap(px(3.))
            .child(
                div()
                    .max_w(relative(0.85))
                    .flex()
                    .min_w_0()
                    .child(
                        div()
                            .id(SharedString::from(format!("ububble-{msg_ix}")))
                            // c21: 超高气泡 300px 内部滚动（USER_BUBBLE_MAX_HEIGHT）
                            .max_h(px(300.))
                            .overflow_y_scroll()
                            .flex_1()
                            .min_w_0()
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(12.))
                            .bg(rgb(t.user_bg))
                            .border_1()
                            .border_color(gpui::rgba(rgba_a(0x3b82f6, 0.2)))
                            .text_color(rgb(t.text))
                            .child(div().flex().flex_col().child(image_block).child(md)),
                    ),
            )
            .child(bottom);
        col = col.child(row);
    } else {
        // 单条 assistant（仅当它不构成轮头时才会走到这里——session_list
        // 已把轮渲染收敛到 render_assistant_turn；此分支防御性保留）
        col = col.group("astat");
        for b in &m.blocks {
            col = col.child(render_block(b, msg_ix, weak, collapsed, t, false));
        }
    }
    col
}

/// Block 渲染可见性（组内条目计数/过滤用；空 thinking/空 text 跳过）。
fn block_displayable(b: &Block) -> bool {
    match b {
        Block::Text { text, .. } | Block::Thinking { text, .. } => !text.trim().is_empty(),
        Block::ToolCall { name, .. } => !name.is_empty(),
        Block::Image { .. } => true,
    }
}

/// pi-web splitFinalAssistantBlocks：最终回答 = 尾部 text/image 连续段，
/// 返回首个 answer 块下标（无连续段时 == len）。
fn split_answer_start(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .rposition(|b| !matches!(b, Block::Text { .. } | Block::Image { .. }))
        .map_or(0, |i| i + 1)
}

/// pi-web hasFinalAssistantAnswer：存在非空 text 或 image 块。
fn msg_has_answer(m: &Msg) -> bool {
    m.blocks.iter().any(|b| match b {
        Block::Text { text, .. } => !text.trim().is_empty(),
        Block::Image { .. } => true,
        _ => false,
    })
}

fn usage_line(u: &UsageLine) -> Option<String> {
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
    if u.cache_write > 0 {
        parts.push(format!(
            "{} cache W",
            crate::services::format::fmt_thousand(u.cache_write)
        ));
    }
    if u.cost > 0. {
        parts.push(format!("${:.4}", u.cost));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn model_label_div(label: &str, t: &theme::Theme) -> gpui::Div {
    div()
        .text_size(px(11.))
        .text_color(rgb(t.text_dim))
        .mb(px(4.))
        .child(SharedString::from(label.to_string()))
}

/// 一轮 agent 回复（用户消息 → 下一用户消息之间的全部 assistant 消息）。
/// pi-web ChatWindow 轮分组 parity（v56-1）：思考+工具调用全部收进
/// 「工作详情」组——有最终回答时默认折叠、流式中/无最终回答时展开；
/// 最终回答 = 末条 assistant 的尾部 text/image 连续段
/// （splitFinalAssistantBlocks parity），其前置块同样入组；每条 assistant
/// 消息自带模型名标签；usage 只取最终消息（omitUsage parity）。hover
/// 操作栏（复制整轮/用时/时间）为自定义豁免项。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_assistant_turn(
    turn: &[&Msg],
    // 轮内各消息的全局索引（thinking/copy key 用）
    turn_ixs: &[usize],
    start_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    model_label: &str,
    // Some(est_tokens)：此轮正在流式（最后一个 assistant 消息）
    stream_info: Option<u64>,
    meta: MsgMeta,
    copied: bool,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(22.)).flex().flex_col().group("astat");
    let is_working = stream_info.is_some();

    // pi-web findFinalAssistantIndex：有 answer 连续段的末条 assistant，
    // 兜底取末条 assistant
    let final_pos = turn
        .iter()
        .rposition(|m| m.role == Role::Assistant && msg_has_answer(m))
        .or_else(|| turn.iter().rposition(|m| m.role == Role::Assistant));
    let Some(final_pos) = final_pos else {
        return col;
    };
    let final_msg = turn[final_pos];
    let final_gix = turn_ixs[final_pos];
    let answer_start = split_answer_start(&final_msg.blocks);
    let answer_len = final_msg.blocks.len() - answer_start;
    let final_error = final_msg.stop_reason.as_deref() == Some("error");
    let final_truncated = final_msg.stop_reason.as_deref() == Some("length");
    let has_final_answer = answer_len > 0 || final_error || final_truncated;

    // 复制整轮文本（hover 操作栏，自定义保留）
    let mut turn_text = String::new();
    for m in turn {
        for b in &m.blocks {
            if let Block::Text { text, .. } = b {
                if !text.trim().is_empty() {
                    if !turn_text.is_empty() {
                        turn_text.push_str("\n\n");
                    }
                    turn_text.push_str(text);
                }
            }
        }
    }

    // ---- 「工作详情」组：全部 assistant 消息的 thinking/toolCall，最终
    // 消息只贡献 answer 连续段之前的前置块（pi-web processViews parity）----
    let mut group_body = div().mt(px(8.)).flex().flex_col();
    let mut n_views = 0usize;
    let mut n_tools = 0usize;
    for (i, (m, &gix)) in turn.iter().zip(turn_ixs).enumerate().take(final_pos + 1) {
        if m.role != Role::Assistant {
            continue;
        }
        let end = if i == final_pos { answer_start } else { m.blocks.len() };
        let disp: Vec<&Block> = m.blocks[..end].iter().filter(|b| block_displayable(b)).collect();
        if disp.is_empty() {
            continue;
        }
        n_views += 1;
        let mut item = div().mb(px(16.)).flex().flex_col();
        // 每条 assistant 消息自带模型名标签（pi-web AssistantMessageView）
        item = item.child(model_label_div(
            m.model.as_deref().unwrap_or(model_label),
            t,
        ));
        for b in disp {
            if matches!(b, Block::ToolCall { .. }) {
                n_tools += 1;
            }
            let streaming = is_working && i == turn.len() - 1;
            item = item.child(render_block(b, gix, weak, collapsed, t, streaming));
        }
        group_body = group_body.child(item);
    }

    if n_views > 0 {
        // 折叠状态：显式覆盖 > 每态默认（流式中/无最终回答 = 展开，
        // 有最终回答 = 折叠；pi-web defaultExpanded={!finalAnswerMessage}）
        let key = (start_ix, usize::MAX);
        let default_open = is_working || !has_final_answer;
        let open = collapsed.get(&key).copied().unwrap_or(default_open);
        let label = crate::i18n::tf(
            "工作详情 · {m} 条消息 · {t} 次工具调用",
            &[("m", n_views.to_string()), ("t", n_tools.to_string())],
        );
        let weak_fold = weak.clone();
        let fold_row = div()
            .id(SharedString::from(format!("work-fold-{start_ix}")))
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
                        let next = !r.collapsed.get(&key).copied().unwrap_or(false);
                        r.collapsed.insert(key, next);
                    });
                    cx.notify();
                });
            })
            .child(icon(
                if open { "chevron-down" } else { "chevron-right" },
                12.,
                t.text_dim,
            ))
            .child(SharedString::from(label));
        col = col.child(fold_row);
        if open {
            col = col.child(group_body);
        }
    }

    // ---- 最终回答：末条消息的 answer 连续段（pi-web finalAnswerMessage）----
    if answer_len > 0 {
        col = col.child(model_label_div(
            final_msg.model.as_deref().unwrap_or(model_label),
            t,
        ));
        for b in &final_msg.blocks[answer_start..] {
            let streaming = is_working && final_pos == turn.len() - 1;
            col = col.child(render_block(b, final_gix, weak, collapsed, t, streaming));
        }
    }
    // token 用量行：只取最终消息（pi-web 中间消息 omitUsage parity）
    if let Some(line) = final_msg.usage.as_ref().and_then(usage_line) {
        col = col.child(
            div()
                .mt(px(2.))
                .text_size(px(11.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(line)),
        );
    }
    // hover 操作栏：复制整轮文本 + 用时 + 时间（自定义豁免项）
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
    // 用时取轮内末条消息（自定义 hover 豁免项）
    if let Some(last) = turn.last() {
        if let (Some(end), Some(start)) = (last.end_ts, meta.turn_user_ts) {
            bar = bar.child(
                div()
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(format!(
                        "{}{}",
                        tr("用时"),
                        crate::services::format::fmt_duration_ms(end - start)
                    ))),
            );
        }
    }
    col = col.child(bar);
    // pi-web parity（c7）：时间戳静态 10px 右下，仅轮尾显示、流式尾部隐藏
    if !is_working {
        if let Some(ts) = turn.last().and_then(|m| m.ts) {
            col = col.child(
                div()
                    .flex()
                    .justify_end()
                    .mt(px(2.))
                    .text_size(px(10.))
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(crate::services::format::fmt_msg_time(ts))),
            );
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
                stop_reason: None,
                error_message: None,
                custom_type: None,
                custom_display: true,
                details: None,
                model: None,
            }),
            "assistant" => out.push(Msg {
                role: Role::Assistant,
                blocks,
                usage: usage.map(|u| UsageLine {
                    input: u.input,
                    output: u.output,
                    cache_read: u.cache_read,
                    cache_write: u.cache_write,
                    cost: u.cost,
                }),
                entry_id: None,
                ts,
                end_ts: entry_ts.or(ts),
                stop_reason: m["stopReason"].as_str().map(str::to_string),
                error_message: m["errorMessage"].as_str().map(str::to_string),
                model: m["model"].as_str().map(str::to_string),
                custom_type: None,
                custom_display: true,
                details: None,
            }),
            "custom" => out.push(Msg {
                role: Role::Custom,
                blocks,
                usage: None,
                entry_id: None,
                ts,
                end_ts: None,
                stop_reason: None,
                error_message: None,
                custom_type: Some(m["customType"].as_str().unwrap_or("").to_string()),
                custom_display: m["display"].as_bool().unwrap_or(true),
                details: m["details"].as_object().map(|_| m["details"].clone()),
                model: None,
            }),
            "toolResult" => {
                let (text, images) = result_payload(&blocks);
                let is_error = m["isError"].as_bool().unwrap_or(false);
                let tcid = m["toolCallId"].as_str();
                let details = m["details"].as_object().map(|_| m["details"].clone());
                if let Some(last) = out.last_mut() {
                    merge_tool_result(last, tcid, is_error, &text, images, details, ts);
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
