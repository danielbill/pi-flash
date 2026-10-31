//! 消息卡渲染：工具卡（render_tool_card）、自定义消息卡、压缩卡、
//! diff 三视图（分文件/单元格/补丁文本）与工具路径提取。

use super::*;

use std::collections::HashMap;

use gpui::{
    Animation, AnimationExt, FontWeight, MouseButton, SharedString, TextAlign, div, prelude::*, px,
    relative, rgb, rgba,
};
use pi_link::protocol::Block;

use crate::Chat;
use crate::appearance::theme;
use crate::editor::markdown;
use crate::i18n::{tf, tr};
use crate::ui::icon;

// 两个 args 缓存的线程局部 LRU（同 MD_PARSE_CACHE 的理由，元素构建只在
// 主线程；放模块级是为了让探针 [`tool_args_cache_probe`] 够得着）。
// key = (源文哈希, 长度)，命中后全等校验防碰撞。
thread_local! {
    static PREVIEW_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize), (String, String))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
    static READ_PATH_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize), (String, Option<String>))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
/// 字节预算（args 源文各 16MB）：write/edit 的 args 内嵌整个文件内容
/// （几十 KB～MB 级），只按条数封顶时大文件会话稳态可到几十 MB
const ARGS_CACHE_BUDGET_BYTES: usize = 16 * 1024 * 1024;

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

/// 044：subagent 专属卡。数据源（扩展 `pf-subagents` 的 details，snake_case）：
/// `child_id` / `status`(running) / `stop_reason` / `session_file` /
/// `duration_ms` / `total_tokens` / `cost_total` / `error_code`；args 里取
/// `label` / `prompt`。布局：头部（子代理 · 标签 · 状态徽标 · 耗时 · tokens ·
/// 成本）+ 展开正文（prompt 与子代理回答）+ 「打开子会话」跳转。
#[allow(clippy::too_many_arguments)]
fn render_subagent_card(
    msg_ix: usize,
    content_index: usize,
    args: &str,
    result: &str,
    is_error: bool,
    details: Option<&serde_json::Value>,
    duration_s: Option<i64>,
    // 子代理卡不区分「参数是否流式部分到达」（args 只取 label/prompt），
    // 保留形参以对齐普通工具卡的调用签名
    _args_partial: bool,
    result_arrived: bool,
    weak: &gpui::WeakEntity<Chat>,
    collapsed: &HashMap<(usize, usize), bool>,
    t: &theme::Theme,
) -> gpui::Div {
    let key = (msg_ix, content_index);
    let expanded = collapsed.get(&key).copied().unwrap_or(false);
    let d = details.cloned().unwrap_or_default();
    // child_id 仅在 details 里（面板/日志用），卡面不展示
    let running = d.get("status").and_then(|v| v.as_str()) == Some("running");
    let stop_reason = d.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("");
    let session_file = d
        .get("session_file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let total_tokens = d.get("total_tokens").and_then(|v| v.as_u64());
    let cost_total = d.get("cost_total").and_then(|v| v.as_f64());
    let error_code = d.get("error_code").and_then(|v| v.as_str());

    let args_v = serde_json::from_str::<serde_json::Value>(args).ok();
    let label = args_v
        .as_ref()
        .and_then(|v| v.get("label"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty());
    let prompt = args_v
        .as_ref()
        .and_then(|v| v.get("prompt"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    // 状态徽标：运行中(黄) / 完成(绿) / 失败(红，分码优先)
    let (phase_text, phase_c) = if is_error || error_code.is_some() {
        (error_code.unwrap_or("error").to_string(), 0xf87171)
    } else if running || (!result_arrived && stop_reason.is_empty()) {
        ("运行中".to_string(), 0xeab308)
    } else {
        ("完成".to_string(), 0x22c55e)
    };

    // 度量行：耗时（事件 duration_s 优先，回退 details.duration_ms）+ tokens + 成本
    let mut metrics: Vec<String> = Vec::new();
    if let Some(sec) = duration_s {
        metrics.push(format!("{sec}s"));
    } else if let Some(ms) = d.get("duration_ms").and_then(|v| v.as_u64()) {
        if ms >= 1000 {
            metrics.push(format!("{:.1}s", ms as f64 / 1000.0));
        } else if ms > 0 {
            metrics.push(format!("{ms}ms"));
        }
    }
    if let Some(tok) = total_tokens {
        metrics.push(format!("{tok} tok"));
    }
    if let Some(cost) = cost_total {
        metrics.push(format!("¥{cost:.4}"));
    }

    // 标题：label 优先，退 prompt 截断
    let title = label
        .map(str::to_string)
        .unwrap_or_else(|| prompt.chars().take(40).collect());

    // 头部（点击=折叠切换，复用 weak_toggle 模式）
    let weak_toggle = weak.clone();
    let mut head = div()
        .id(SharedString::from(format!("sa-{msg_ix}-{content_index}")))
        .flex()
        .items_center()
        .gap(px(7.))
        .px(px(10.))
        .py(px(6.))
        .min_w_0()
        .flex_1()
        .text_size(crate::appearance::sess_size(-2.))
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
                .text_color(rgb(0x60a5fa))
                .child("子代理"),
        );
    if !title.is_empty() {
        head = head.child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(t.text))
                .child(SharedString::from(title)),
        );
    }
    head = head.child(
        div()
            .flex_shrink_0()
            .text_size(crate::appearance::sess_size(-3.))
            .text_color(rgb(phase_c))
            .child(SharedString::from(phase_text)),
    );
    for m in &metrics {
        head = head.child(
            div()
                .flex_shrink_0()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(m.clone())),
        );
    }

    // 展开正文：prompt + 子代理回答 + 错误分码
    let mut body = div().flex().flex_col().gap(px(6.)).px(px(10.)).pb(px(8.));
    if !prompt.is_empty() {
        body = body.child(
            div()
                .text_size(crate::appearance::sess_size(-2.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(format!("任务：{prompt}"))),
        );
    }
    if result_arrived && !result.trim().is_empty() {
        body = body.child(
            div()
                .text_size(crate::appearance::sess_size(-2.))
                .text_color(rgb(t.text))
                .child(SharedString::from(format!(
                    "回答：{}",
                    result.chars().take(600).collect::<String>()
                ))),
        );
    }

    // 「打开子会话」：子会话是普通 pi 会话，直接 open_session（只读检视+可续跑）
    if let Some(sf) = session_file {
        let weak_open = weak.clone();
        body = body.child(
            div()
                .id(SharedString::from(format!(
                    "sa-open-{msg_ix}-{content_index}"
                )))
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .text_size(crate::appearance::sess_size(-2.))
                .text_color(rgb(0x60a5fa))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_open.update(cx, |c, cx| {
                        c.open_session(sf.clone(), false, cx);
                    });
                })
                .child("↗ 打开子会话"),
        );
    }

    // 卡壳（状态色与通用卡一致）
    let (border_c, bg_c) = if is_error {
        (theme::rgba_a(0xf87171, 0.45), theme::rgba_a(0xf87171, 0.05))
    } else {
        (theme::rgba_a(0x22c55e, 0.25), theme::rgba_a(0x22c55e, 0.04))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .px(px(2.))
        .py(px(2.))
        .border_1()
        .border_color(gpui::rgba(border_c))
        .bg(gpui::rgba(bg_c))
        .rounded(px(6.))
        .overflow_hidden()
        .child(head)
        .when(expanded, |dd| dd.child(body))
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
        let close = if kind == 0 {
            "</read-files>"
        } else {
            "</modified-files>"
        };
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
        "compaction" => col = col.child(render_compaction_card(m, msg_ix, t, collapsed, weak)),
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
                    .child(SharedString::from(tr("branch.detour_note"))),
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
                tr("messages.extended_hidden").to_string()
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
                            m.ts.map(crate::services::format::fmt_msg_time)
                                .unwrap_or_default(),
                        )),
                ),
        );
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
                .child(SharedString::from(tr("compaction.done"))),
        )
        .child(
            div()
                .mt(px(3.))
                .mb(px(10.))
                .text_size(crate::appearance::ui_size(14.))
                .line_height(relative(1.5))
                .text_color(rgb(t.text))
                .child(SharedString::from(tr("compaction.summary_intro"))),
        )
        .child(if body.is_empty() {
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(tr("compaction.no_summary")))
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
            parts.push(tf(
                "compaction.reads",
                &[("n", read_files.len().to_string())],
            ));
        }
        if !modified_files.is_empty() {
            parts.push(tf(
                "compaction.writes",
                &[("n", modified_files.len().to_string())],
            ));
        }
        // 折叠位复用 runtime.collapsed；usize::MAX 作content_index，
        // 避开同一消息里工具调用用的 (msg_ix, content_index) 键
        let key = (msg_ix, usize::MAX);
        let open = collapsed.get(&key).copied().unwrap_or(false);
        let weak_toggle = weak.clone();
        let label = tf("compaction.file_context", &[("details", parts.join(", "))]);
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
                            if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
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
            for (title, files) in [
                (tr("compaction.modified_files"), &modified_files),
                (tr("compaction.read_files"), &read_files),
            ] {
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
    // 带线程局部 LRU（同 tool_preview 的理由）：turn_written_files 每帧
    // 扫整轮文件写入工具，write/edit 的 args 内嵌整个文件内容
    const CAP: usize = 256;
    let key = (crate::editor::markdown::hash_str(args), args.len());
    READ_PATH_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0.as_str() == args {
                let out = entry.1.1.clone();
                cache.push_back(entry);
                return out;
            }
        }
        let v: serde_json::Value = serde_json::from_str(args).ok()?;
        let out = v
            .get("file_path")
            .or_else(|| v.get("path"))
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
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

/// 探针（automation snapshot）：两个 args 缓存合计（条数，源文字节）
pub(crate) fn tool_args_cache_probe() -> (usize, usize) {
    let preview = PREVIEW_CACHE.with(|cell| {
        let cache = cell.borrow();
        (cache.len(), cache.iter().map(|(k, _)| k.1).sum::<usize>())
    });
    let read_path = READ_PATH_CACHE.with(|cell| {
        let cache = cell.borrow();
        (cache.len(), cache.iter().map(|(k, _)| k.1).sum::<usize>())
    });
    (preview.0 + read_path.0, preview.1 + read_path.1)
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
pub(super) fn turn_written_files(turn: &[&Msg]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let cwd = std::env::current_dir().ok();
    let push =
        |raw: String, seen: &mut std::collections::HashSet<String>, out: &mut Vec<String>| {
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
            let Block::ToolCall {
                name,
                args,
                is_error,
                result_arrived,
                details,
                ..
            } = b
            else {
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

/// pi-web SplitFilesView：双栏 split diff，maxHeight 560 滚动；多文件时
/// 显示文件头（pi-web sticky 头在 gpui 无对应，退化为普通行）。
fn split_files_view(
    files: &[crate::session::diff::DiffFile],
    t: &theme::Theme,
) -> gpui::AnyElement {
    use crate::session::diff::Row;
    let show_headers = files.len() > 1;
    let mut wrap = div()
        .id(SharedString::from(SharedString::from(format!(
            "sd-{}",
            files.len()
        ))))
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
                            .unwrap_or_else(|| tr("messages.before").to_string()),
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
        CellKind::Added => gpui::rgba(theme::rgba_a(0x22c55e, 0.12)),
        CellKind::Removed => gpui::rgba(theme::rgba_a(0xf87171, 0.13)),
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
    let line_no = cell.line_no.map(|n| n.to_string()).unwrap_or_default();
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
                .when(
                    matches!(cell.kind, CellKind::Added | CellKind::Removed),
                    |d| d.font_weight(FontWeight::BOLD),
                )
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
                0 => (
                    gpui::rgba(theme::rgba_a(0x60a5fa, 0.12)),
                    t.accent,
                    t.accent,
                ),
                1 => (
                    gpui::rgba(theme::rgba_a(0x22c55e, 0.12)),
                    0x22c55e,
                    0x22c55e,
                ),
                2 => (
                    gpui::rgba(theme::rgba_a(0xf87171, 0.13)),
                    0xf87171,
                    0xf87171,
                ),
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
                .child(div().px(px(10.)).min_w_0().text_color(rgb(color)).child(
                    SharedString::from(if line.is_empty() {
                        "\u{a0}".to_string()
                    } else {
                        line.to_string()
                    }),
                ))
        }))
        .into_any_element()
}
