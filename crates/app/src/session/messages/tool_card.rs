//! 工具卡（032）：args 展示双 LRU 缓存（PREVIEW/READ_PATH）、
//! [`render_tool_card`] 与子代理卡 [`render_subagent_card`]、alert 提示盒。
//! 自 cards.rs 拆出（090 重构）。

use super::diff_view::split_files_view;
use super::*;

use std::collections::HashMap;

use gpui::{
    Animation, AnimationExt, FontWeight, MouseButton, SharedString, div, prelude::*, px, relative,
    rgb, rgba,
};

use crate::Chat;
use crate::appearance::theme;
use crate::i18n::tr;
use crate::ui::icon;

// 两个 args 缓存的线程局部 LRU（同 MD_PARSE_CACHE 的理由，元素构建只在
// 主线程；放模块级是为了让探针 [`tool_args_cache_probe`] 够得着）。
// key = (源文哈希, 长度)，命中后全等校验防碰撞。
thread_local! {
    pub(crate) static PREVIEW_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize), (String, String))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
    pub(crate) static READ_PATH_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize), (String, Option<String>))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
/// 字节预算（args 源文各 16MB）：write/edit 的 args 内嵌整个文件内容
/// （几十 KB～MB 级），只按条数封顶时大文件会话稳态可到几十 MB
pub(crate) const ARGS_CACHE_BUDGET_BYTES: usize = 16 * 1024 * 1024;

/// pi-web getToolPreview：command/path/file_path/pattern/query 键序取值
/// 截 120 字符，都没有取首个键的值。
///
/// 结果带线程局部 LRU 缓存（key = 源文哈希 + 长度，命中全等校验防碰撞）：
/// List 每帧重建元素树，折叠卡也会每帧重算预览，而 serde 解析整段 args
/// （write/edit 的 args 内嵌整个文件内容，几十 KB 起）是巨轮会话打字延迟
/// 的大头——缓存后折叠帧只剩一次哈希扫描。
fn tool_preview(args: &str) -> String {
    const CAP: usize = 256;
    let key = (crate::editor::markdown::hash_str(args), args.len());
    PREVIEW_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0.as_str() == args {
                let out = entry.1.1.clone();
                cache.push_back(entry); // 刷新到队尾（LRU）
                return out;
            }
        }
        let out = tool_preview_uncached(args);
        cache.push_back((key, (args.to_string(), out.clone())));
        while cache.len() > CAP
            || cache.iter().map(|(k, _)| k.1).sum::<usize>() > ARGS_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        out
    })
}

fn tool_preview_uncached(args: &str) -> String {
    const KEYS: &[&str] = &["command", "path", "file_path", "pattern", "query"];
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return String::new();
    };
    let Some(obj) = v.as_object() else {
        return String::new();
    };
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
    raw.map(|r| r.chars().take(120).collect())
        .unwrap_or_default()
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
pub(super) fn result_is_empty(result: &str) -> bool {
    let r = result.trim();
    r.is_empty() || r == "(no output)"
}

/// 044-0：subagent 专属卡——向 zcode 渲染对齐的**紧凑行**（设计原文
/// 「当前会话流中的渲染有些啰嗦」，旧版头体双段卡废弃）：
/// `[状态标记] 子代理 <名称(profile)> <任务名(label 退 prompt 截断)> … [分码]`
/// 数据源（扩展 pf-subagents 的 details，snake_case）：`child_id` / `status`
/// / `stop_reason` / `session_file` / `error_code`；args 取 `profile` /
/// `label` / `prompt`。整行点击 = 打开子会话（只读观察窗）；token/耗时等
/// 统计不再进会话流（面板行二专责展示）。
#[allow(clippy::too_many_arguments)]
fn render_subagent_card(
    msg_ix: usize,
    content_index: usize,
    args: &str,
    _result: &str,
    is_error: bool,
    details: Option<&serde_json::Value>,
    // 统计已迁面板（044-0），行内不再展示耗时
    _duration_s: Option<i64>,
    // 行不区分「参数是否流式部分到达」（args 只取 name/task），保留形参
    // 以对齐派发签名
    _args_partial: bool,
    result_arrived: bool,
    weak: &gpui::WeakEntity<Chat>,
    // 旧版展开态残留槽位：紧凑行无正文，保留形参对齐签名
    _collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
) -> gpui::Div {
    let d = details.cloned().unwrap_or_default();
    let stop_reason = d.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("");
    let session_file = d
        .get("session_file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let error_code = d.get("error_code").and_then(|v| v.as_str());

    let args_v = serde_json::from_str::<serde_json::Value>(args).ok();
    let profile = args_v
        .as_ref()
        .and_then(|v| v.get("profile"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("general-purpose")
        .to_string();
    let task = args_v
        .as_ref()
        .and_then(|v| {
            v.get("label")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            args_v
                .as_ref()
                .and_then(|v| v.get("prompt"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>()
        });

    // 状态（044-0 B5：同面板四色，settle 才定色）——
    //   失败 红点+分码（含被拒 onUpdate 分片，B3② 即时上卡）
    //   轮次用尽/中止 橙点（044-1 §1.3，与面板 C_ABORTED 同源）
    //   完成 绿点 = result 到了且 stop_reason=="completed"
    //   其余（派出未决/运行中）黄 spin——旧条件 (!result_arrived||…) 把
    //   「结果未到」也算 done，派出瞬间全体恒绿（B5 根因）
    let max_turns = stop_reason == "max_turns";
    let failed = (is_error || error_code.is_some()) && !max_turns;
    let done = result_arrived && stop_reason == "completed";
    let aborted = result_arrived && (stop_reason == "aborted" || max_turns);
    let (mark_c, mark) = if failed {
        (0xf87171, "dot")
    } else if aborted {
        (0xf97316, "dot")
    } else if done {
        (0x22c55e, "dot")
    } else {
        (0xeab308, "spin")
    };
    let mark_el = if mark == "spin" {
        crate::ui::spark_spinner(13., mark_c)
    } else {
        div()
            .size(px(6.))
            .rounded_full()
            .bg(gpui::rgb(mark_c))
            .into_any_element()
    };

    let mut row = div()
        .id(SharedString::from(format!("sa-{msg_ix}-{content_index}")))
        .flex()
        .items_center()
        .gap(px(7.))
        .px(px(10.))
        .py(px(5.))
        .min_w_0()
        .rounded(px(crate::ui::tokens::radius::CTRL))
        .text_size(crate::appearance::sess_size(-2.))
        .hover(|s| s.bg(gpui::rgba(theme::rgba_a(t.accent, 0.06))))
        .child(mark_el)
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x60a5fa))
                .child("子代理"),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text))
                .child(SharedString::from(profile)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(task)),
        );
    if let Some(code) = error_code {
        let code = code.to_string();
        row = row.child(
            div()
                .flex_shrink_0()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(0xf87171))
                .child(SharedString::from(code)),
        );
    }
    if let Some(sf) = session_file {
        let weak_open = weak.clone();
        row = row
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_open.update(cx, |c, cx| {
                    c.open_session(sf.clone(), false, cx);
                });
            });
    }
    div().flex().flex_col().px(px(2.)).py(px(2.)).child(row)
}

/// 「工作详情」组内的 pi-web ToolCallBlock 卡片（v56-2 c8/c9）。
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(super) fn render_tool_card(
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
    // 044：subagent 专属卡（扩展 pf-subagents 的 agent 工具）——数据源为
    // 扩展 details（snake_case），不走通用 diff/preview 主流程。
    if name == "agent" {
        return render_subagent_card(
            msg_ix,
            content_index,
            args,
            result,
            is_error,
            details,
            duration_s,
            args_partial,
            result_arrived,
            weak,
            collapsed,
            t,
        );
    }
    let key = (msg_ix, content_index);
    // pi-web isToolCallExpanded：模块级记忆，默认全部折叠
    let expanded = collapsed.get(&key).copied().unwrap_or(false);
    let is_patch = crate::session::diff::is_apply_patch_tool_name(name);
    let is_edit = crate::session::diff::is_edit_tool_name(name);

    // c10: diff 视图数据 —— apply_patch 优先解析输入 V4A 文档，失败退
    // details.preview；write/edit 取 details.patch/.diff（统一 diff）。
    // **只在展开时才解析**：折叠卡每帧重建，这些 serde/V4A 解析若无条件
    // 执行，巨轮里每帧都是成百次全量 parse——折叠帧界面上一个像素都不画，
    // 没人为它付费的理由
    let patch_files = if expanded && is_patch {
        let input_text = serde_json::from_str::<serde_json::Value>(args)
            .ok()
            .and_then(|v| v.get("input").and_then(|x| x.as_str()).map(str::to_string))
            .unwrap_or_else(|| args.to_string());
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
    let result_diff = if expanded && result_arrived && !is_error {
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
            theme::rgba_a(0xf87171, 0.45),
            theme::rgba_a(0xf87171, 0.05),
            0xf87171,
            theme::rgba_a(0xf87171, 0.25),
        )
    } else {
        (
            theme::rgba_a(0x22c55e, 0.25),
            theme::rgba_a(0x22c55e, 0.04),
            0x16a34a,
            theme::rgba_a(0x22c55e, 0.2),
        )
    };

    // 标题行：工具名 + 参数摘要（apply_patch 用路径摘要）+ 耗时 + 箭头
    let preview = if args_partial && result.is_empty() && !result_arrived {
        tr("messages.generating_args").to_string()
    } else if is_patch {
        let s = summarize_apply_patch(args);
        if s.is_empty() { tool_preview(args) } else { s }
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
                    el.with_transformation(gpui::Transformation::rotate(gpui::radians(
                        delta * std::f32::consts::PI,
                    )))
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
                    .border_color(gpui::rgba(theme::rgba_a(0x22c55e, 0.15)))
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
                    .border_color(gpui::rgba(theme::rgba_a(0x22c55e, 0.15)))
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
pub(super) fn alert_box(
    text: String,
    color: u32,
    border_rgb: u32,
    _t: &theme::Theme,
) -> gpui::AnyElement {
    div()
        .mt(px(8.))
        .px(px(10.))
        .py(px(7.))
        .border_1()
        .border_color(gpui::rgba(theme::rgba_a(border_rgb, 0.3)))
        .rounded(px(6.))
        .bg(gpui::rgba(theme::rgba_a(border_rgb, 0.07)))
        .text_color(rgb(color))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .line_height(relative(1.5))
        .child(SharedString::from(text))
        .into_any_element()
}
