//! Message model + row rendering for the session message panel (032,
//! pi-web MessageView.tsx parity). Free functions over Chat state; the
//! entity split lands in phase E (ARCHITECTURE.md §2).

use std::collections::HashMap;

use gpui::{Animation, AnimationExt, FontWeight, MouseButton, SharedString, TextAlign, div, prelude::*, px, relative, rgb, rgba};
use pi_link::protocol::{content_blocks, Block, Usage};

use super::actions_bar::{self, user_action_bar};
use super::fork::ForkAnchor;
use crate::Chat;
use crate::i18n::{tf, tr};
use crate::editor::markdown;
use crate::theme;
use crate::ui::icon;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Role {
    User,
    Assistant,
    /// pi CustomMessage (compaction summary, extension messages,
    /// branch summaries) — v56-6 renders the card; skipped until then
    Custom,
    /// 用户 `!` / `!!` 发起的 shell 命令执行（pi `role:"bashExecution"`）。
    /// 独立角色而非 assistant 消息：不进轮分组、不计 nav 统计；pi 侧它
    /// 由 convertToLlm 在下一次 prompt 时折叠为 user 文本
    Bash,
}

/// 一条 shell 命令执行（Msg.bash；pi BashExecutionMessage 同名字段）。
/// 流式阶段 output 逐步追加（bash_execution_update），final 由 bash response
/// 回填（exit_code/cancelled/truncated/full_output_path）。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct BashInfo {
    pub(crate) command: String,
    pub(crate) output: String,
    pub(crate) exit_code: Option<i64>,
    pub(crate) cancelled: bool,
    pub(crate) truncated: bool,
    pub(crate) full_output_path: Option<String>,
    /// `!!` 前缀：会话有记录、模型看不到（pi excludeFromContext）
    pub(crate) excluded: bool,
    /// true = 乐观插入的执行中卡片（response 回填后翻 false）
    pub(crate) running: bool,
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
    /// Role::Bash 专属（其余角色恒 None）
    pub(crate) bash: Option<BashInfo>,
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

    /// 文本前缀（导航面板摘要专用）：拼够 max_chars 个字符即停，避免为
    /// 截断摘要把整条长回复全量拼接一遍（nav 每帧都取，长程任务会话里
    /// 全量 join 的成本随回复长度线性涨）
    pub(crate) fn plain_text_prefix(&self, max_chars: usize) -> String {
        let mut out = String::new();
        let mut taken = 0usize;
        for b in &self.blocks {
            if let Block::Text { text, .. } = b {
                for ch in text.chars() {
                    if taken >= max_chars {
                        return out;
                    }
                    out.push(ch);
                    taken += 1;
                }
            }
        }
        out
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
    // thinking 时长（pi-web ThinkingBlock 右侧 Ns；快照按消息首尾时间差）
    thinking_dur: Option<i64>,
) -> gpui::Div {
    match b {
        Block::Text { text, .. } if !text.trim().is_empty() => {
            div().w_full().child(markdown::render(text, t, streaming))
        }
        Block::Thinking { text, content_index } if !text.trim().is_empty() => {
            let key = (msg_ix, *content_index);
            // 「展示思考」开关 = 新思考块的默认展开态：开=默认展开全文，
            // 关（默认）=收成一行（灯泡+单行预览，点击可展开）；用户手动
            // 展开过的块以 collapsed 里的显式值为准
            let expanded = collapsed
                .get(&key)
                .copied()
                .unwrap_or(crate::services::workspace::show_thinking());
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
                // 块间距由上层 gap 8 容器统一（pi-web 块容器 gap:8），
                // 块自身不挂 margin（此前 my_1=4px 与工具卡 0px 不均）
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
                .font_family(crate::editor::markdown::MONO_FAMILY)
                // 思考块字号 = 会话字号 -2（字体大小设置.md §2；族保持等宽）
                .text_size(crate::appearance::sess_size(-2.))
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
            // pi-web {duration}s：右侧 text-dim
            if let Some(d) = thinking_dur {
                block = block.child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!("{d}s"))),
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

/// 0xRRGGBB + alpha → gpui::rgba 的 0xRRGGBBAA 布局（alpha 在低字节）。
pub(crate) fn rgba_a(rgb24: u32, alpha: f32) -> u32 {
    let a = (alpha * 255.0).round().clamp(0.0, 255.0) as u32;
    ((rgb24 & 0xffffff) << 8) | a
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
        // 工具执行内容字号 = 会话字号 -2（字体大小设置.md §2）
        .text_size(crate::appearance::sess_size(-2.))
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
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
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
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(preview)),
        );
    if let Some(d) = duration_s {
        head = head.child(
            div()
                .flex_shrink_0()
                .text_size(crate::appearance::sess_size(-3.))
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
        .text_size(crate::appearance::sess_size(-2.))
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
                .bg(rgba(t.bg_subtle))
                .px(px(10.))
                .py(px(8.))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-2.))
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

/// pi-web 告警框（providerError/truncated 共用结构）：mono 12px、
/// 1px 边框 + 0.07 底色、圆角 6、pre-wrap。
fn alert_box(text: String, color: u32, border_rgb: u32, _t: &theme::Theme) -> gpui::AnyElement {
    div()
        .mt(px(8.))
        .px(px(10.))
        .py(px(7.))
        .border_1()
        .border_color(gpui::rgba(rgba_a(border_rgb, 0.3)))
        .rounded(px(6.))
        .bg(gpui::rgba(rgba_a(border_rgb, 0.07)))
        .text_color(rgb(color))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .line_height(relative(1.5))
        .child(SharedString::from(text))
        .into_any_element()
}

/// pi-web parseCompactionSummary parity：剥离尾部的
/// <read-files>/<modified-files> 段，返回 (body, read, modified)。
fn parse_compaction_summary(summary: &str) -> (String, Vec<String>, Vec<String>) {
    let lines: Vec<&str> = summary.lines().collect();
    let mut spans: Vec<(usize, usize, usize)> = Vec::new(); // (kind, start, end)
    let mut k = 0usize;
    while k < lines.len() {
        let t = lines[k].trim();
        let kind = if t == "<read-files>" {
            0
        } else if t == "<modified-files>" {
            1
        } else {
            k += 1;
            continue;
        };
        let close = if kind == 0 { "</read-files>" } else { "</modified-files>" };
        let mut j = k + 1;
        while j < lines.len() && lines[j].trim() != close {
            j += 1;
        }
        if j < lines.len() {
            spans.push((kind, k, j));
            k = j + 1;
        } else {
            k += 1;
        }
    }
    // 尾段：从文档末尾反向取连续（只隔空行）的 section 段
    let mut read = Vec::new();
    let mut modified = Vec::new();
    let mut body_end = lines.len();
    let mut cursor = lines.len();
    for (kind, start, end) in spans.iter().rev() {
        if *end != cursor - 1 {
            break;
        }
        // 段与段之间（或首段之前）只允许空行
        if cursor < lines.len() {
            if !lines[*end + 1..cursor].iter().all(|l| l.trim().is_empty()) {
                break;
            }
        }
        if *start > 0 && !lines[*start - 1].trim().is_empty() {
            break;
        }
        let files: Vec<String> = lines[*start + 1..*end]
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if *kind == 0 {
            read.extend(files);
        } else {
            modified.extend(files);
        }
        body_end = *start;
        cursor = *start;
    }
    let body = lines[..body_end].join("\n").trim().to_string();
    (body, read, modified)
}

/// Role::Custom 渲染（v56-6 c28；mod.rs 分发入口）。
pub(crate) fn render_custom_msg(
    m: &Msg,
    msg_ix: usize,
    t: &theme::Theme,
    collapsed: &HashMap<(usize, usize), bool>,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    let custom_type = m.custom_type.as_deref().unwrap_or("");
    match custom_type {
        "compaction" => {
            col = col.child(render_compaction_card(m, msg_ix, t, collapsed, weak))
        }
        "branch_summary" => {
            // pi-web 将 branch_summary 渲为 user 气泡（斜体引言+摘要）；
            // 这里为保持 fork 锚点对齐保留 Custom 角色，渲染为斜体引言 +
            // 摘要 markdown（已知偏差）
            let summary = m.plain_text();
            col = col.child(
                div()
                    .italic()
                    .text_color(rgb(t.text_muted))
                    .text_size(crate::appearance::ui_size(13.))
                    .mb(px(4.))
                    .child(SharedString::from(tr(
                        "*此对话曾短暂探索另一分支后返回，摘要如下：*",
                    ))),
            );
            if !summary.trim().is_empty() {
                col = col.child(markdown::render(&summary, t, false));
            }
        }
        _ if !m.custom_display => {
            // pi-web display:false：折叠暗卡 + 140 字符预览
            let preview: String = m
                .plain_text()
                .replace('\n', " ")
                .chars()
                .take(140)
                .collect();
            let preview = if preview.is_empty() {
                tr("扩展消息（不显示）").to_string()
            } else {
                format!("{}...", preview)
            };
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg))
                    .opacity(0.82)
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_muted))
                    .child(
                        div()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(11.))
                            .child(SharedString::from(if custom_type.is_empty() {
                                "extension".to_string()
                            } else {
                                custom_type.to_string()
                            })),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(preview)),
                    ),
            );
        }
        _ => {
            // pi-web CustomMessageView 简化 parity：customType 头 + markdown 正文
            let body = m.plain_text();
            let card = div()
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .overflow_hidden()
                .bg(rgb(t.bg))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(7.))
                        .border_b_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg_panel))
                        .text_color(rgb(t.text_muted))
                        .child(
                            div()
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(11.))
                                .child(SharedString::from(if custom_type.is_empty() {
                                    "extension".to_string()
                                } else {
                                    custom_type.to_string()
                                })),
                        ),
                )
                .child(
                    div()
                        .px(px(13.))
                        .py(px(11.))
                        .child(markdown::render(&body, t, false)),
                );
            col = col.child(card);
        }
    }
    col
}

/// pi-web CompactionMessageView parity（v56-6 c28）。
fn render_compaction_card(
    m: &Msg,
    msg_ix: usize,
    t: &theme::Theme,
    collapsed: &HashMap<(usize, usize), bool>,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Div {
    let (body, read_files, modified_files) = parse_compaction_summary(&m.plain_text());
    let card = div()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(rgb(t.bg))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(7.))
                .border_b_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .text_color(rgb(t.text_muted))
                .child(
                    div()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(11.))
                        .child(SharedString::from("compaction")),
                )
                .child(
                    div()
                        .ml_auto()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(
                            m.ts.map(crate::services::format::fmt_msg_time).unwrap_or_default(),
                        )),
                ),
        )
        ;
    // 正文容器（pi-web <div style={{padding:"11px 13px 12px"}}>）：
    // 文件元数据必须挂在这层里面；挂到卡片根上会让分隔线和清单框
    // 顶掉左右 13px 内边距，直接贴住卡片边框
    let mut body = div()
        .px(px(13.))
        .pt(px(11.))
        .pb(px(12.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(15.))
                .font_weight(FontWeight::BOLD)
                        .line_height(relative(1.35))
                        .text_color(rgb(t.text))
                        .child(SharedString::from(tr("会话已压缩"))),
                )
                .child(
                    div()
                        .mt(px(3.))
                        .mb(px(10.))
                        .text_size(crate::appearance::ui_size(14.))
                        .line_height(relative(1.5))
                        .text_color(rgb(t.text))
                        .child(SharedString::from(tr(
                            "此处之前的会话历史已压缩为以下摘要：",
                        ))),
                )
                .child(if body.is_empty() {
                    div()
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(tr("（无摘要）")))
                        .into_any_element()
                } else {
                    markdown::render(&body, t, false)
                });

    // 文件元数据（pi-web CompactionFileMetadata parity）：
    // <details> 默认收起，summary 行 =「文件上下文：N 读取，M 修改」，
    // 顶线分隔；展开后按"修改/读取"分节，清单是带边框底色的等宽框
    let total = read_files.len() + modified_files.len();
    if total > 0 {
        let mut parts: Vec<String> = Vec::new();
        if !read_files.is_empty() {
            parts.push(tf("{n} 读取", &[("n", read_files.len().to_string())]));
        }
        if !modified_files.is_empty() {
            parts.push(tf("{n} 修改", &[("n", modified_files.len().to_string())]));
        }
        // 折叠位复用 runtime.collapsed；usize::MAX 作content_index，
        // 避开同一消息里工具调用用的 (msg_ix, content_index) 键
        let key = (msg_ix, usize::MAX);
        let open = collapsed.get(&key).copied().unwrap_or(false);
        let weak_toggle = weak.clone();
        let label = tf("文件上下文：{details}", &[("details", parts.join(", "))]);
        let mut meta = div()
            .mt(px(10.))
            .pt(px(8.))
            .border_t_1()
            .border_color(rgb(t.border))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
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
                        // pi-web 浏览器 <summary> 原生三角（summary 12px muted
                        // 同款尺寸）——原 10px text_faint 字形小到看不清
                        icon(
                            if open { "chevron-down" } else { "chevron-right" },
                            12.,
                            t.text_muted,
                        ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_size(crate::appearance::ui_size(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(1.4))
                            .text_color(rgb(t.text_muted))
                            .child(SharedString::from(label.clone())),
                    ),
            );

        if open {
            for (title, files) in [("修改文件", &modified_files), ("读取文件", &read_files)] {
                if files.is_empty() {
                    continue;
                }
                let list = div()
                    .id(SharedString::from(format!("cfiles-{msg_ix}-{title}")))
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .py(px(7.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg_panel))
                    .flex()
                    .flex_col()
                    .gap(px(3.)) // li + li { margin-top: 3px }
                    .children(files.iter().map(|f| {
                        div()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(11.))
                            .line_height(relative(1.45))
                            .text_color(rgb(t.text_muted))
                            .child(SharedString::from(f.clone()))
                    }));
                meta = meta.child(
                    div()
                        .mt(px(8.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mb(px(4.))
                                .text_size(crate::appearance::ui_size(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(SharedString::from(tr(title))),
                        )
                        .child(list),
                );
            }
        }
        body = body.child(meta);
    }
    card.child(body)
}

/// pi-web isWriteToolName（tool-names.ts）。
fn is_write_tool_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == "write" || n.starts_with("write_") || n.ends_with(".write") || n.ends_with("_write")
}

fn is_file_writing_tool(name: &str) -> bool {
    is_write_tool_name(name)
        || crate::session::diff::is_edit_tool_name(name)
        || crate::session::diff::is_apply_patch_tool_name(name)
}

fn read_tool_path(args: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args).ok()?;
    v.get("file_path")
        .or_else(|| v.get("path"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// pi-web readApplyPatchPaths parity：appliedFiles > preview > 输入解析，
/// 剔除 delete，failures 无 applied 时写零文件。
fn apply_patch_written_paths(args: &str, details: Option<&serde_json::Value>) -> Vec<String> {
    let delete_paths: Vec<String> = args
        .lines()
        .filter_map(|l| l.strip_prefix("*** Delete File: "))
        .map(|p| p.trim().to_string())
        .collect();
    let has_failures = details
        .and_then(|d| d.get("result"))
        .and_then(|r| r.get("failures"))
        .and_then(|f| f.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if let Some(applied) = details
        .and_then(|d| d.get("result"))
        .and_then(|r| r.get("appliedFiles"))
        .and_then(|a| a.as_array())
    {
        let applied: Vec<String> = applied
            .iter()
            .filter_map(|x| x.as_str())
            .filter(|p| !p.is_empty() && !delete_paths.contains(&p.to_string()))
            .map(str::to_string)
            .collect();
        return applied;
    }
    if has_failures {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if let Some(d) = details {
        if let Some(files) = crate::session::diff::apply_patch_preview_to_files(d) {
            for f in files {
                if let Some(p) = f.new_path {
                    paths.push(p);
                }
            }
        }
    }
    if paths.is_empty() {
        if let Some(files) = crate::session::diff::parse_apply_patch_input(args) {
            for f in files {
                if let Some(p) = f.new_path {
                    paths.push(p);
                }
            }
        }
    }
    paths
}

/// pi-web extractTurnWrittenFiles parity：本轮实际写过的文件（工具调用
/// 为准，去重保序，相对路径按 cwd 解析）。
fn turn_written_files(turn: &[&Msg]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let cwd = std::env::current_dir().ok();
    let push = |raw: String, seen: &mut std::collections::HashSet<String>, out: &mut Vec<String>| {
        let path = std::path::Path::new(&raw);
        let resolved = if path.is_absolute() {
            raw.clone()
        } else {
            match &cwd {
                Some(c) => c.join(raw).to_string_lossy().to_string(),
                None => return,
            }
        };
        if seen.insert(resolved.clone()) {
            out.push(resolved);
        }
    };
    for m in turn {
        for b in &m.blocks {
            let Block::ToolCall { name, args, is_error, result_arrived, details, .. } = b else {
                continue;
            };
            if *is_error || !*result_arrived || !is_file_writing_tool(name) {
                continue;
            }
            if crate::session::diff::is_apply_patch_tool_name(name) {
                for p in apply_patch_written_paths(args, details.as_ref()) {
                    push(p, &mut seen, &mut out);
                }
            } else if let Some(p) = read_tool_path(args) {
                push(p, &mut seen, &mut out);
            }
        }
    }
    out
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
            rgba(t.bg_subtle)
        })
        .px(px(10.))
        .py(px(8.))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::sess_size(-2.))
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
        let Ok(image) = decode_image_cached(&data.data, format) else {
            continue;
        };
        if image.bytes.len() > MAX_IMAGE_BYTES {
            continue;
        }
        wrap = wrap.child(
            gpui::img(image)
                .max_w(px(720.))
                .max_h(px(520.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border)),
        );
    }
    wrap.into_any_element()
}

pub(crate) fn mime_to_image_format(mime: &str) -> Option<gpui::ImageFormat> {
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

// 解码缓存：List 每帧重建元素树，同一张 base64 图每帧重走 base64 解码 +
// `Image::from_bytes` 的内容哈希（几百 KB 数据 = ms 级），滚动/流式时纯浪费。
// key = (哈希, 长度, 格式)，命中全等校验防碰撞；线程局部 VecDeque 当 LRU。
thread_local! {
    static IMAGE_DECODE_CACHE: std::cell::RefCell<
        std::collections::VecDeque<
            ((u64, usize, u8), (String, Result<std::sync::Arc<gpui::Image>, ()>)),
        >,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const IMAGE_DECODE_CACHE_CAP: usize = 64;

/// base64 → [`gpui::Image`]（带每进程缓存；Err 结果同样入缓存，坏数据不反复重试）
pub(crate) fn decode_image_cached(
    data: &str,
    format: gpui::ImageFormat,
) -> Result<std::sync::Arc<gpui::Image>, ()> {
    let key = (crate::editor::markdown::hash_str(data), data.len(), format as u8);
    IMAGE_DECODE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1 .0.as_str() == data {
                let image = entry.1 .1.clone();
                cache.push_back(entry); // 刷新到队尾（LRU）
                return image;
            }
        }
        let decoded = decode_image_data(data)
            .map(|bytes| std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)));
        cache.push_back((key, (data.to_string(), decoded.clone())));
        if cache.len() > IMAGE_DECODE_CACHE_CAP {
            cache.pop_front();
        }
        decoded
    })
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
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::sess_size(-2.))
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
        CellKind::Empty => rgba(t.bg_subtle),
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
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::sess_size(-2.))
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

/// pi-web lib/slash-display.ts skillExpansionToCommand 的 Rust 移植：识别
/// pi _expandSkillCommand 输出的信封，还原紧凑命令（仅显示用，存储文本
/// 不变）。贪婪正文（正文可能含示例 </skill>，取最后一个），可选双换行
/// 后缀为用户参数。
pub(crate) fn skill_expansion_to_command(text: &str) -> Option<String> {
    let rest = text.strip_prefix("<skill name=\"")?;
    let (name, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(" location=\"")?;
    let (_loc, rest) = rest.split_once('"')?;
    let rest = rest.strip_prefix(">\n")?;
    let rest = rest.strip_prefix("References are relative to ")?;
    let rest = rest.split_once('\n')?.1; // 跳过 base 目录行
    let rest = rest.strip_prefix('\n')?; // 空行
    let close = rest.rfind("\n</skill>")?;
    let tail = &rest[close + 9..];
    let args = match tail.strip_prefix("\n\n") {
        Some(a) => a.to_string(),
        None if tail.is_empty() => String::new(),
        None => return None,
    };
    Some(if args.is_empty() {
        format!("/skill:{}", name)
    } else {
        format!("/skill:{} {}", name, args)
    })
}

/// 操作栏悬停显影的**唯一**接线：用户消息行与 agent 轮块都走它，不许各写
/// 一段内联 `on_hover`（两套写法必然分叉，见下）。
///
/// 状态 = [`Chat::bar_hover`]：单个 `Option<usize>`，存该行在列表里的索引
/// （用户行 = 消息索引，agent 轮 = 轮首消息索引；两者角色不同，索引永不撞车）。
///
/// **为什么必须统一且小心后代遮挡**：gpui `Window::hit_test` 命中
/// `HitboxBehavior::BlockMouse`（即 `div().occlude()`）就 `break`，插入更早
/// 的 hitbox（**祖先全部**）连 `ids` 都进不去，`hitbox.is_hovered()` 恒
/// false。用户气泡曾挂 `occlude()`（本意「禁止鼠标透传到下层消息」），于是
/// 鼠标停在气泡上时行级 on_hover 永不触发、操作栏不显影；agent 轮块没有遮挡
/// 后代，一切正常——这就是「同一种行为两处表现」的根因。
///
/// 挂在**有 id 的行**上（用户行 `msgrow-*` / 轮块 `turn-*`），故要
/// `StatefulInteractiveElement`：`on_hover` 在它上面，不在裸 `InteractiveElement`。
/// 回归锁见本文件测试模块 `bar_hover_hit_test`（正反两侧）。
fn bar_hover_wired<E: StatefulInteractiveElement>(
    el: E,
    weak: &gpui::WeakEntity<Chat>,
    ix: usize,
) -> E {
    let weak = weak.clone();
    el.on_hover(move |hovered, _, cx| {
        let _ = weak.update(cx, |c, cx| {
            let next = if *hovered { Some(ix) } else { None };
            if c.bar_hover != next {
                c.bar_hover = next;
                cx.notify();
            }
        });
    })
}

pub(crate) fn render_msg(
    m: &Msg,
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    expanded_skills: &std::collections::HashSet<String>,
    bubble_scrolls: &std::rc::Rc<
        std::cell::RefCell<std::collections::HashMap<String, gpui::ScrollHandle>>,
    >,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    // Some(est_tokens) 仅当此消息是流式中的最后一条（工作中回复）
    _stream_info: Option<u64>,
    _meta: MsgMeta,
    // 操作栏悬停显影（Chat.bar_hover，pi-web hovered state parity）
    bar_revealed: bool,
    // 复制反馈（032 恢复）：runtime.copy_flash 点亮中 → 栏换 ✓ 已复制
    copied: bool,
) -> gpui::Div {
    // pi-web 消息间距 marginBottom 16（v 此前 22 偏大）
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    // 消息内文件路径/本地文件链接点击的目标（markdown 渲染构建期捕获进
    // 事件闭包；文件 md 预览走 content.rs 自设 base）
    markdown::set_link_target(weak.clone(), None);
    if m.role == Role::User {
        // v56-4 c18-c22（pi-web UserMessageView parity）：右对齐 85% 宽、
        // user_bg 底 + 1px 蓝边框 rgba(59,130,246,.2)、圆角 12、pad 8/12、
        // 内容走 markdown、内嵌图片 240 上限、超高 300px 内部滚动；
        // 操作行（复制/编辑/新分支）hover 淡入 = 自定义豁免项。
        let text = m.plain_text();

        // v58: 用户消息图片 = 气泡上方独立缩略图行（参考截图 parity，不再
        // 内嵌气泡）：67×67（composer 56 的 +20%）、间隔 5px、右对齐（随
        // 行 items_end）、点击开 ImagePreview 大图弹窗
        let image_row: Option<gpui::AnyElement> = {
            let mut thumbs: Vec<gpui::AnyElement> = Vec::new();
            for (bi, b) in m.blocks.iter().enumerate() {
                if let Block::Image { mime, data, .. } = b {
                    if let Some(format) = mime_to_image_format(mime) {
                        if let Ok(image) = decode_image_cached(data, format) {
                            let image_for_open = image.clone();
                            let weak_open = weak.clone();
                            thumbs.push(
                                div()
                                    .id(SharedString::from(format!("uimg-{msg_ix}-{bi}")))
                                    .size(px(67.))
                                    .flex_shrink_0()
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_panel))
                                    .overflow_hidden()
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        let image = image_for_open.clone();
                                        let _ = weak_open.update(cx, |c, cx| {
                                            c.dialog =
                                                Some(crate::Dialog::ImagePreview { image });
                                            cx.notify();
                                        });
                                    })
                                    .child(
                                        gpui::img(image)
                                            .size_full()
                                            .object_fit(gpui::ObjectFit::Cover),
                                    )
                                    .into_any_element(),
                            );
                        }
                    }
                }
            }
            (!thumbs.is_empty()).then(|| {
                div()
                    .w(relative(0.85))
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(5.))
                    .children(thumbs)
                    .into_any_element()
            })
        };
        // c19: 用户内容走 markdown；但 HTML 不渲染、标签原样显示
        // （render_user）——用户消息是发出内容的凭证，气泡吞标签会让
        // 用户无法核对 agent 实际收到的文本（v57 用户反馈）
        //
        // v57: 技能展开消息（CLI 把 /skill:xxx 展开成多行全文）默认折叠
        // 成 mono 命令行 + 展开箭头（pi-web parity），展开后内容区限高
        // 带滚动条；单行 /skill:xxx（未展开回显）按普通渲染
        // v57: pi-web UserMessageView parity——skill 展开消息（信封文本）
        // 默认折叠：mono 技能名 + 展开箭头 + 参数原文；展开显示全文
        // markdown；复制/编辑目标都是紧凑命令（copyTarget/editTarget parity）
        let command_text = skill_expansion_to_command(&text);
        let skill_key = m
            .entry_id
            .clone()
            .unwrap_or_else(|| format!("skill-{}", msg_ix));
        let skill_open = expanded_skills.contains(&skill_key);
        let md: gpui::AnyElement = if text.trim().is_empty() {
            div().into_any_element()
        } else if let Some(cmd) = &command_text {
            let (cmd_name, cmd_args) = match cmd.split_once(' ') {
                Some((n, a)) => (n.to_string(), a.to_string()),
                None => (cmd.clone(), String::new()),
            };
            let weak_skill = weak.clone();
            let chevron = if skill_open { "chevron-up" } else { "chevron-down" };
            let mut stack = div().flex().flex_col().w_full().gap(px(6.));
            stack = stack.child(
                div().flex().items_start().gap(px(8.)).flex_wrap()
                    .child(
                        div()
                            .id(SharedString::from(format!("skill-toggle-{}", msg_ix)))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .flex_shrink_0()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(13.))
                            .text_color(rgb(t.accent))
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.85))
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                let key = skill_key.clone();
                                let _ = weak_skill.update(cx, |c, cx| {
                                    if !c.expanded_skills.remove(&key) {
                                        c.expanded_skills.insert(key);
                                    }
                                    cx.notify();
                                });
                            })
                            .child(SharedString::from(cmd_name))
                            .child(icon(chevron, 11., t.accent)),
                    )
                    .when(!cmd_args.is_empty(), |d| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(crate::appearance::ui_size(14.))
                                .text_color(rgb(t.text))
                                .child(SharedString::from(cmd_args)),
                        )
                    }),
            );
            if skill_open {
                stack = stack.child(markdown::render_user(&text, t));
            }
            stack.into_any_element()
        } else {
            markdown::render_user(&text, t)
        };

        let scroll_key = m
            .entry_id
            .clone()
            .unwrap_or_else(|| format!("ububble-{}", msg_ix));
        let (scroll_handle, scroll_state) = {
            let mut map = bubble_scrolls.borrow_mut();
            let h = map
                .entry(scroll_key)
                .or_insert_with(gpui::ScrollHandle::new)
                .clone();
            let st = gpui_component::scroll::ScrollbarState::default();
            (h, st)
        };

        let bottom = user_action_bar(msg_ix, weak, &text, m.ts, bar_revealed, copied, t);
        // 纯图片消息（空文本）不渲染空泡；缩略图行在气泡上方
        let has_text = !text.trim().is_empty();
        let mut row = bar_hover_wired(
            div()
                .id(SharedString::from(format!("msgrow-{msg_ix}")))
                .w_full()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(3.)),
            weak,
            msg_ix,
        );
        if let Some(imgs) = image_row {
            row = row.child(imgs);
        }
        if has_text {
            row = row.child(
                div()
                    .max_w(relative(0.85))
                    .flex()
                    .min_w_0()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .id(SharedString::from(format!("ububble-{msg_ix}")))
                                    // c21: 超高气泡 300px 内部滚动（USER_BUBBLE_MAX_HEIGHT）
                                    .max_h(px(300.))
                                    .overflow_y_scroll()
                                    .track_scroll(&scroll_handle)
                                    .flex_1()
                                    .min_w_0()
                                    // 此处**不得** occlude()：gpui hit_test 命中
                                    // BlockMouse 即 break，会把祖先（本行的
                                    // bar_hover_wired）一起挡掉 → 鼠标停在气泡上
                                    // 操作栏永不显影（v62 修复）。滚轮也一并受益：
                                    // 气泡内滚到底后能继续滚会话列表。
                                    .px(px(12.))
                                    .pr(px(14.))
                                    .py(px(8.))
                                    .rounded(px(12.))
                                    .bg(rgb(t.user_bg))
                                    .border_1()
                                    .border_color(gpui::rgba(rgba_a(0x3b82f6, 0.2)))
                                    .text_color(rgb(t.text))
                                    .child(div().flex().flex_col().child(md)),
                            )
                            // 滚动条仅在实际溢出限高时渲染（max_offset>0
                            // = 内容超高；未溢出无条）
                            .children(
                                (scroll_handle.max_offset().height > px(0.)).then(|| {
                                    div()
                                        .absolute()
                                        .top(px(8.))
                                        .bottom(px(8.))
                                        .right(px(3.))
                                        .w(px(8.))
                                        .child(
                                            gpui_component::scroll::Scrollbar::vertical(
                                                &scroll_state,
                                                &scroll_handle,
                                            )
                                            .scroll_size(gpui::size(
                                                px(0.),
                                                px(300.)
                                                    + scroll_handle.max_offset().height,
                                            )),
                                        )
                                }),
                            ),
                    ),
            );
        }
        row = row.child(bottom);
        col = col.child(row);
    } else {
        // 单条 assistant（仅当它不构成轮头时才会走到这里——session_list
        // 已把轮渲染收敛到 render_assistant_turn；此分支防御性保留）
        // pi-web 块容器 gap 8（text/thinking/工具卡统一间距）
        let mut blocks_col = div().w_full().flex().flex_col().gap(px(8.));
        for b in &m.blocks {
            blocks_col = blocks_col.child(render_block(b, msg_ix, weak, collapsed, t, false, None));
        }
        col = col.child(blocks_col);
    }
    col
}

/// Role::Bash 渲染（mod.rs 分发入口；pi-web BashExecutionView parity）：
/// 合成 toolCall 卡片复用 render_tool_card（bash 卡 = 命令预览 + 结果体），
/// 执行中加一行「执行中」状态。独立消息行，不进轮分组。
pub(crate) fn render_bash_msg(
    m: &Msg,
    msg_ix: usize,
    t: &theme::Theme,
    collapsed: &HashMap<(usize, usize), bool>,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Div {
    let mut col = div().w_full().mb(px(16.)).flex().flex_col();
    let Some(info) = &m.bash else {
        return col;
    };
    // pi-web：excluded 的命令显示 "bash (local)"（输出仅本地，不进模型）
    let name = if info.excluded { "bash (local)" } else { "bash" };
    let args = serde_json::json!({ "command": info.command }).to_string();
    // is_error 对齐 agent 的 bash 工具卡：非零退出码 = 红（取消不算）
    let is_error = !info.cancelled && info.exit_code.is_some_and(|c| c != 0);
    let done = !info.running;
    // pi-web bashExecutionToText 的后缀行（展示即模型所见）
    let mut result = info.output.trim_end().to_string();
    if result.is_empty() {
        result = "(no output)".into();
    }
    if info.cancelled {
        result.push_str("\n(command cancelled)");
    } else if let Some(code) = info.exit_code {
        if code != 0 {
            result.push_str(&format!("\nCommand exited with code {code}"));
        }
    }
    if info.truncated {
        result.push_str(&format!(
            "\n[Output truncated. Full output: {}]",
            info.full_output_path.as_deref().unwrap_or("")
        ));
    }
    let duration_s = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
    col = col.child(render_tool_card(
        msg_ix,
        0,
        name,
        &args,
        &result,
        is_error,
        &[],
        None,
        duration_s,
        false,
        done,
        weak,
        collapsed,
        t,
    ));
    if info.running {
        col = col.child(
            div()
                .mt(px(4.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text_dim))
                .child(
                    gpui::svg()
                        .path(SharedString::from("icons/loader.svg"))
                        .text_color(rgb(t.accent))
                        .size(px(11.))
                        .with_animation(
                            SharedString::from(format!("bash-spin-{msg_ix}")),
                            Animation::new(std::time::Duration::from_millis(900)),
                            |el, delta| {
                                el.with_transformation(gpui::Transformation::rotate(
                                    gpui::radians(delta * 2. * std::f32::consts::PI),
                                ))
                            },
                        )
                        .into_any_element(),
                )
                .child(SharedString::from(tr("执行中 · Esc 中止"))),
        );
    }
    col
}

/// Block 渲染可见性（组内条目计数/过滤用；空 thinking/空 text 跳过）。
/// 思考块永远可见（收起=一行），不随「展示思考」开关消失。
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

/// 模型名行（pi-web MessageView model label）：模型名 + 流式期间的 ↓token
/// 估算 + t/s 徽章（四档配色同 pi-web：≥50 青 / ≥30 绿 / ≥15 黄 / 其余红）。
fn model_label_div(label: &str, est: Option<u64>, tps: Option<f32>, t: &theme::Theme) -> gpui::Div {
    // agent 名称 = 会话字号 -1（字体大小设置.md §2）
    let mut row = div()
        .text_size(crate::appearance::sess_size(-1.))
        .text_color(rgb(t.text_dim))
        .mb(px(4.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(SharedString::from(label.to_string()));
    if let Some(e) = est.filter(|e| *e > 0) {
        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .text_color(rgb(t.text))
                .child(SharedString::from(format!(
                    "\u{2193} {}",
                    crate::services::format::fmt_thousand(e)
                ))),
        );
        if let Some(v) = tps {
            let bg = if v >= 50. {
                0x53b3cb
            } else if v >= 30. {
                0x9bc53d
            } else if v >= 15. {
                0xf9c22e
            } else {
                0xe01a4f
            };
            row = row.child(
                div()
                    .ml(px(6.))
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(gpui::rgb(bg))
                    .text_size(crate::appearance::sess_size(-1.))
                    .text_color(rgb(0xffffff))
                    .child(SharedString::from(format!("{v:.1} t/s"))),
            );
        }
    }
    row
}

/// pi-web `isLiveTail` 的平铺渲染：运行中的这一轮**不分组、不折叠**——每条
/// assistant 消息一个模型名行 + 全部块（thinking/toolCall/text 就地展开），
/// 流式那条的模型名行带 ↓token 估算 + t/s 徽章。
///
/// 「工作详情」折叠行与最终回答分区只在轮末整形后生成；usage 行/复制栏也照
/// pi-web MessageView 的 `!isStreaming` 规则留给轮末（见 `render_assistant_turn`）。
#[allow(clippy::too_many_arguments)]
fn live_turn_body(
    turn: &[&Msg],
    turn_ixs: &[usize],
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
    model_label: &str,
    est: Option<u64>,
    tps: Option<f32>,
) -> gpui::Div {
    let mut body = div().flex().flex_col();
    for (i, (m, &gix)) in turn.iter().zip(turn_ixs).enumerate() {
        if m.role != Role::Assistant {
            continue;
        }
        let disp: Vec<&Block> = m.blocks.iter().filter(|b| block_displayable(b)).collect();
        if disp.is_empty() {
            continue;
        }
        // 轮内最后一条 = 正在流式输出/干活的那条（徽章与 streaming 态挂它）
        let streaming = i == turn.len() - 1;
        let mut item = div().mb(px(16.)).flex().flex_col().gap(px(8.));
        item = item.child(model_label_div(
            m.model.as_deref().unwrap_or(model_label),
            if streaming { est } else { None },
            if streaming { tps } else { None },
            t,
        ));
        // pi-web thinkingDurationFromFile：该消息的生成时长挂到 thinking 块
        let msg_think_dur = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
        for b in disp {
            item = item.child(render_block(b, gix, weak, collapsed, t, streaming, msg_think_dur));
        }
        body = body.child(item);
    }
    body
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
    // Some(tps)：流式速度（pi-web 300ms tick 估算，四档配色）
    stream_tps: Option<f32>,
    meta: MsgMeta,
    // 操作栏悬停显影（Chat.bar_hover，pi-web hovered state parity）
    bar_revealed: bool,
    // 复制反馈（032 恢复）：runtime.copy_flash 点亮中 → 栏换 ✓ 已复制
    copied: bool,
    // 「新分支」目标（None = 锚点还没回来，按钮不出）
    fork: Option<ForkAnchor>,
) -> gpui::AnyElement {
    // pi-web onMouseEnter/Leave parity：悬停整轮（无遮挡后代，见 bar_hover_wired）
    let mut col = bar_hover_wired(
        div()
            .id(SharedString::from(format!("turn-{start_ix}")))
            .w_full()
            .mb(px(16.))
            .flex()
            .flex_col(),
        weak,
        start_ix,
    );
    let is_working = stream_info.is_some();

    // pi-web findFinalAssistantIndex：有 answer 连续段的末条 assistant，
    // 兜底取末条 assistant
    let final_pos = turn
        .iter()
        .rposition(|m| m.role == Role::Assistant && msg_has_answer(m))
        .or_else(|| turn.iter().rposition(|m| m.role == Role::Assistant));
    let Some(final_pos) = final_pos else {
        return col.into_any_element();
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

    // ---- 运行中：pi-web `isLiveTail` 平铺渲染（不分组、不折叠）----
    // pi-web ChatWindow：`isLiveTail = (sessionBusy || isStreaming) &&
    // endIdx === messages.length && userIdx === lastAnchorIdx` 时，把这一轮直接
    // 平铺——每条 assistant 一个模型名行、thinking/toolCall/text 就地展开，流式
    // 那条的模型名行带 ↓token 估算 + t/s 徽章。usage 行 / 复制栏（pi-web
    // MessageView `!isStreaming` 才渲染）与「工作详情」折叠行、最终回答分区一律
    // 等到**轮末整形**（下面的 !is_working 分支）才出现。
    // 旧实现轮中就套上折叠组：思考/工具被折起来（streaming 时看不见思考框），
    // 内容高度忽大忽小，还把翻页钉顶搅乱。
    let est = stream_info.filter(|_| is_working);
    let tps = stream_tps.filter(|_| is_working);
    if is_working {
        return col
            .child(live_turn_body(turn, turn_ixs, weak, collapsed, t, model_label, est, tps))
            .into_any_element();
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
        // 块容器 gap 8（pi-web AssistantMessageView：label 下方 flexDirection
        // column + gap 8）；label 自带 mb 4 → label→首块 12px，与 pi-web 一致
        let mut item = div().mb(px(16.)).flex().flex_col().gap(px(8.));
        // 每条 assistant 消息自带模型名标签（pi-web AssistantMessageView）
        item = item.child(model_label_div(
            m.model.as_deref().unwrap_or(model_label),
            None,
            None,
            t,
        ));
        // pi-web thinkingDurationFromFile：该消息的生成时长挂到 thinking 块
        let msg_think_dur = m.end_ts.zip(m.ts).map(|(e, s)| (e - s).max(0) / 1000);
        for b in disp {
            if matches!(b, Block::ToolCall { .. }) {
                n_tools += 1;
            }
            let streaming = is_working && i == turn.len() - 1;
            item = item.child(render_block(b, gix, weak, collapsed, t, streaming, msg_think_dur));
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
            // 工作详情行 = 会话字号（字体大小设置.md §2）
            .text_size(crate::appearance::sess_size(0.))
            .text_color(rgb(t.text_muted))
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

    // ---- 轮末整形：最终回答分区（pi-web finalAnswerMessage）----
    // 走到这里必然 !is_working（运行中已在上面平铺返回），所以模型名行不带
    // ↓token/t-s 徽章——pi-web 的徽章只属于 isStreaming 的那条消息。
    if answer_len > 0 || final_error || final_truncated {
        col = col.child(model_label_div(
            final_msg.model.as_deref().unwrap_or(model_label),
            None,
            None,
            t,
        ));
    }
    if answer_len > 0 {
        // pi-web 最终回答块容器 gap 8
        let mut answer = div().w_full().flex().flex_col().gap(px(8.));
        for b in &final_msg.blocks[answer_start..] {
            answer = answer.child(render_block(b, final_gix, weak, collapsed, t, false, None));
        }
        col = col.child(answer);
    }
    // c26/c27（pi-web providerError / truncated parity）：错误红框、截断黄框
    if final_error {
        let msg = final_msg
            .error_message
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or("Unknown provider error");
        col = col.child(alert_box(
            format!("Error: {msg}"),
            0xef4444,
            0xef4444,
            t,
        ));
    }
    if final_truncated {
        col = col.child(alert_box(
            tr("回复因达到模型输出长度上限而被截断。发送一条后续消息以继续。").to_string(),
            0xca8a04,
            0xeab308,
            t,
        ));
    }
    // c24: 轮内写文件 chips（pi-web TurnWrittenFiles parity：由
    // 成功的 write/edit/apply_patch 工具调用推导，绝不扫描回复文本）
    let written = turn_written_files(turn);
    if !written.is_empty() {
        let mut chips = div().flex().flex_wrap().gap(px(6.)).mt(px(6.));
        for path in &written {
            let p = path.clone();
            let weak_open = weak.clone();
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            chips = chips.child(
                div()
                    .id(SharedString::from(format!("wf-{start_ix}-{name}")))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(6.))
                    .bg(rgba(t.bg_subtle))
                    .border_1()
                    .border_color(rgb(t.border))
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::sess_size(-2.))
                    .text_color(rgb(t.text))
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(t.text)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let p = p.clone();
                        let _ = weak_open.update(cx, |c, cx| {
                            c.open_file_tab(std::path::PathBuf::from(&p), cx)
                        });
                    })
                    .child(icon("file", 12., t.text_dim))
                    .child(SharedString::from(name)),
            );
        }
        col = col.child(chips);
    }
    col = col.child(actions_bar::assistant_action_bar(
        start_ix,
        weak,
        &turn_text,
        turn.last().and_then(|m| m.end_ts),
        meta.turn_user_ts,
        is_working,
        bar_revealed,
        copied,
        fork,
        t,
    ));
    col.into_any_element()
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
                bash: None,
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
                bash: None,
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
                bash: None,
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
            // `!` 执行记录（pi role:"bashExecution"；会话文件里的 message 条目。
            // get_messages 快照不带它——磁盘重读是唯一来源）
            "bashExecution" => out.push(Msg {
                role: Role::Bash,
                blocks: Vec::new(),
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
                bash: Some(BashInfo {
                    command: m["command"].as_str().unwrap_or("").to_string(),
                    output: m["output"].as_str().unwrap_or("").to_string(),
                    exit_code: m["exitCode"].as_i64(),
                    cancelled: m["cancelled"].as_bool().unwrap_or(false),
                    truncated: m["truncated"].as_bool().unwrap_or(false),
                    full_output_path: m["fullOutputPath"].as_str().map(str::to_string),
                    excluded: m["excludeFromContext"].as_bool().unwrap_or(false),
                    running: false,
                }),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// gpui::rgba 是 0xRRGGBBAA（alpha 低字节）——v56 曾写反成 AARRGGBB
    /// 导致通道错位（工具卡藏青/参数区纯黄，用户实测发现）。
    #[test]
    fn rgba_a_layout_is_rrggbbaa() {
        assert_eq!(rgba_a(0x22c55e, 0.25), 0x22c55e40);
        assert_eq!(rgba_a(0xf87171, 0.45), 0xf8717173);
        assert_eq!(rgba_a(0x3b82f6, 0.2), 0x3b82f633);
        // bg_subtle 是全仓库唯一 8 位 RRGGBBAA 主题色，必须走 rgba() 不能
        // 走 rgb()（rgb 跳首字节，0xffffff14 会读成 #ffff14 纯黄）
        // 深色主题 bg_subtle=0xffffff14 → 白 8% alpha（而非 rgb() 误读的 #ffff14）
        let subtle = gpui::rgba(0xffffff14);
        assert!((subtle.r - 1.0).abs() < 1e-3);
        assert!((subtle.g - 1.0).abs() < 1e-3);
        assert!((subtle.b - 1.0).abs() < 1e-3);
        assert!((subtle.a - 0.0784).abs() < 1e-3);
    }
}

#[cfg(test)]
mod skill_fold_tests {
    use super::skill_expansion_to_command;

    // pi-web lib/slash-display.test.mjs skillExpansion fixture 同款
    fn envelope(body: &str, args: Option<&str>) -> String {
        let args_s = args
            .map(|a| format!("\n\n{}", a))
            .unwrap_or_default();
        format!(
            "<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are relative to /path/to/review.\n\n{body}\n</skill>{args}"
            ,
            body = body,
            args = args_s,
        )
    }

    #[test]
    fn restores_with_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Review the supplied files.", Some("src/main.ts"))).as_deref(),
            Some("/skill:review src/main.ts")
        );
    }
    #[test]
    fn restores_without_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Review the supplied files.", None)).as_deref(),
            Some("/skill:review")
        );
    }
    #[test]
    fn multiline_args() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Body.", Some("first line\nsecond line"))).as_deref(),
            Some("/skill:review first line\nsecond line")
        );
    }
    #[test]
    fn final_closing_tag_wins() {
        assert_eq!(
            skill_expansion_to_command(&envelope("Example:\n</skill>\nContinue.", Some("src"))).as_deref(),
            Some("/skill:review src")
        );
    }

    #[test]
    fn lookalike_not_collapsed() {
        assert_eq!(skill_expansion_to_command("<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nordinary user text"), None);
        assert_eq!(skill_expansion_to_command("<skill name=\"review\" location=\"/path/to/review/SKILL.md\">\nReferences are elsewhere.\n\nbody\n</skill>"), None);
        assert_eq!(skill_expansion_to_command("ordinary user text"), None);
    }
}

#[cfg(test)]
mod bar_hover_hit_test {
    //! 用户消息行操作栏悬停（v62 修复）的 gpui 几何前提锁。
    //!
    //! `bar_hover_wired` 把 on_hover 挂在**整行**上，靠的是「行内后代不遮挡」
    //! 这条 gpui 语义：`Window::hit_test` 一旦命中 `HitboxBehavior::BlockMouse`
    //! （`div().occlude()`）就 `break`，被它挡住的祖先 hitbox 连 `ids` 都进不
    //! 去，`is_hovered()` 恒 false。用户气泡曾挂 `occlude()`，于是鼠标停在气泡
    //! 上时行级 on_hover 永不触发 → 操作栏不显影；agent 轮块无遮挡后代故一切
    //! 正常（同一个行为两处表现）。
    //!
    //! 两侧都锁：不遮挡 → 行悬停必须触发（线上行为）；遮挡 → 必须被挡掉（gpui
    //! 现语义；上游若改，这条先炸，提示回来复核用户行实现与上面那段注释）。

    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Context, IntoElement, Modifiers, ParentElement, Render, Styled, TestAppContext,
        VisualTestContext, Window, div, point, px, size, prelude::*,
    };

    /// 迷你复刻用户消息行：row = `bar_hover_wired` 的挂载点，bubble = 气泡本体
    /// （`bubble_occludes` 开关模拟 v62 前后两种写法）。
    struct Row {
        row_hovered: Rc<Cell<usize>>,
        bubble_hovered: Rc<Cell<usize>>,
        bubble_occludes: bool,
    }

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = self.row_hovered.clone();
            let bubble = self.bubble_hovered.clone();
            let bubble_el = div()
                .id("bubble")
                .when(self.bubble_occludes, |d| d.occlude())
                .w(px(100.))
                .h(px(40.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        bubble.set(bubble.get() + 1);
                    }
                });
            div()
                .id("row")
                .w(px(400.))
                .h(px(100.))
                .on_hover(move |hovered, _, _| {
                    if *hovered {
                        row.set(row.get() + 1);
                    }
                })
                .child(bubble_el)
        }
    }

    /// 画一帧后把指针移到气泡正中，返回 (行 on_hover 次数, 气泡 on_hover 次数)
    fn hover_bubble(cx: &mut TestAppContext, bubble_occludes: bool) -> (usize, usize) {
        let window: &mut VisualTestContext = cx.add_empty_window();
        let row_hovered: Rc<Cell<usize>> = Default::default();
        let bubble_hovered: Rc<Cell<usize>> = Default::default();
        let row = row_hovered.clone();
        let bubble = bubble_hovered.clone();
        let _ = window.draw(point(px(0.), px(0.)), size(px(400.), px(100.)), move |_, cx| {
            cx.new(|_| Row {
                row_hovered: row.clone(),
                bubble_hovered: bubble.clone(),
                bubble_occludes,
            })
        });
        window.simulate_mouse_move(point(px(50.), px(20.)), None, Modifiers::none());
        (row_hovered.get(), bubble_hovered.get())
    }

    /// 正例（v62 线上行为）：气泡不遮挡 → 行悬停触发 → `Chat.bar_hover` 置位
    /// → 操作栏显影，与 agent 轮块同构。
    #[gpui::test]
    fn bubble_without_occluder_lets_row_hover_fire(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, false);
        assert_eq!(bubble, 1, "指针在气泡内，气泡自身必须 hover");
        assert_eq!(row, 1, "行级 on_hover 必须触发——缺了它操作栏就不显影");
    }

    /// 反例（v62 之前的线上行为，勿再退化）：气泡挂 `occlude()` → 行 hitbox 被
    /// 截断在 hit-test 之外 → 行级 on_hover 永不触发 → 操作栏不显影。
    #[gpui::test]
    fn bubble_occluder_swallows_row_hover(cx: &mut TestAppContext) {
        let (row, bubble) = hover_bubble(cx, true);
        assert_eq!(bubble, 1);
        assert_eq!(row, 0, "occlude 会连祖先一起截断（gpui 语义若变，此断言先炸）");
    }
}
