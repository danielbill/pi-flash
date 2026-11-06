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
use crate::ui::WithTip as _;
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

/// 044-0：subagent 专属卡——向 zcode 渲染对齐的**紧凑行**（验收⑤定稿）：
/// `[状态标记] 🤖子代理 <name(profile)> <时长> <任务名(25字截断)> <完成提示>`
/// 状态标记同会话列表（运行 spark_spinner(accent) / 完成 NOTICE 青蓝点 /
/// 异常 danger 红点）；完成提示 faint（运行 `.....` / 任务完成），异常态
/// faint+下划线 + hover tooltip（错误详情）。
/// 数据源（扩展 pf-subagents 的 details，snake_case）：`child_id` / `status`
/// / `stop_reason` / `session_file` / `error_code` / `duration_ms`；args 取
/// `profile` / `label` / `prompt`。整行点击 = 打开子会话（只读观察窗）。
#[allow(clippy::too_many_arguments)]
fn render_subagent_card(
    msg_ix: usize,
    content_index: usize,
    args: &str,
    result: &str,
    is_error: bool,
    details: Option<&serde_json::Value>,
    // 终态时长（result 到达 − 消息起始，秒）；运行中走 msg_ts 现算
    duration_s: Option<i64>,
    // 行不区分「参数是否流式部分到达」（args 只取 name/task），保留形参
    // 以对齐派发签名
    _args_partial: bool,
    result_arrived: bool,
    // 消息起始（epoch ms）：运行中 live 时长锚（验收⑤）
    msg_ts: Option<i64>,
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
    let duration_ms = d.get("duration_ms").and_then(|v| v.as_i64());

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
                .take(25)
                .collect::<String>()
        });

    // 完成提示分类（验收⑤注 2/3）：
    //   派发失败 = 子代理根本没跑起来——PF 容量闸（CAPACITY_REACHED）、
    //              provider 侧拒绝/并发（START_REJECTED 归一）、深度/资料
    //              校验（DEPTH_LIMIT / PROFILE_INVALID / PROVIDER_UNKNOWN /
    //              BACKEND_UNSUPPORTED / DUPLICATE_CHILD）
    //   执行异常 = 跑起来后的失败——轮次用尽（max_looped/MAX_TURNS）、
    //              意外中断（aborted）、provider 服务侧异常（PROVIDER_ERROR）
    //              及其他一切非派发分码
    //   任务完成 = result 到且 stopReason==completed 且无错
    //   运行中   = 其余（`.....`）
    const DISPATCH_CODES: [&str; 7] = [
        "CAPACITY_REACHED",
        "START_REJECTED",
        "DEPTH_LIMIT",
        "PROVIDER_UNKNOWN",
        "BACKEND_UNSUPPORTED",
        "PROFILE_INVALID",
        "DUPLICATE_CHILD",
    ];
    let errored = is_error || error_code.is_some();
    let dispatch_failed = error_code.is_some_and(|c| DISPATCH_CODES.contains(&c));
    let done = result_arrived && stop_reason == "completed" && !errored;
    let failed = errored && !done;

    // 状态标记（验收⑤注 4：同会话列表 psp_row_slot 统一样式）
    let mark_el: gpui::AnyElement = if done {
        div()
            .size(px(7.))
            .rounded_full()
            .bg(rgb(crate::appearance::theme::NOTICE))
            .into_any_element()
    } else if failed {
        div()
            .size(px(7.))
            .rounded_full()
            .bg(rgb(t.danger))
            .into_any_element()
    } else {
        crate::ui::spark_spinner(13., t.accent)
    };

    // 时长：终态 duration_ms（扩展回传）→ duration_s（pi 口径 result 到达
    // − 消息起始）兜底；运行中 = now − msg_ts（live，每帧重绘自然刷新）
    let secs = duration_ms.map(|ms| ms / 1000).or(duration_s).or_else(|| {
        (!result_arrived).then_some(()).and_then(|_| {
            msg_ts.map(|ts| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                ((now - ts) / 1000).max(0)
            })
        })
    });
    let duration_text = secs
        .filter(|s| *s >= 0)
        .map(crate::session::subagent_panel::duration_text);

    // tooltip 详情：result 首行（`${code}: ${message}`）→ 分码兜底
    let err_detail: SharedString = {
        let first_line = result.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            first_line.to_string()
        } else {
            error_code.unwrap_or("").to_string()
        }
    }
    .into();

    // 完成提示段（faint；异常态 faint+下划线+tooltip）
    let hint: gpui::AnyElement = if done {
        div()
            .flex_shrink_0()
            .text_color(rgb(t.text_muted))
            .child(tr("subagent.card.done"))
            .into_any_element()
    } else if failed {
        let (label, tip) = if dispatch_failed {
            (tr("subagent.card.dispatch_error"), err_detail)
        } else {
            (tr("subagent.card.run_error"), err_detail)
        };
        div()
            .id(SharedString::from(format!(
                "sa-hint-{msg_ix}-{content_index}"
            )))
            .flex_shrink_0()
            .text_color(rgb(t.text_muted))
            .underline()
            .with_tip_text(tip)
            .child(label)
            .into_any_element()
    } else {
        div()
            .flex_shrink_0()
            .text_color(rgb(t.text_muted))
            .child(".....")
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
                .flex()
                .items_center()
                .gap(px(4.))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x60a5fa))
                // subagent 统一 bot icon（044-0 验收③）
                .child(crate::ui::icon("bot", 11., 0x60a5fa))
                .child("子代理"),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text))
                .child(SharedString::from(profile)),
        );
    if let Some(dt) = duration_text {
        row = row.child(
            div()
                .flex_shrink_0()
                .text_size(crate::appearance::sess_size(-3.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(dt)),
        );
    }
    row = row.child(
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(task)),
    );
    row = row.child(hint);
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
    // 消息起始（epoch ms）：子代理卡运行中 live 时长锚（044-0 验收⑤）
    msg_ts: Option<i64>,
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
            msg_ts,
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
