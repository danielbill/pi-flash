//! 通用工具卡渲染（render_tool_card）与告警条构件。

use super::subagent::render_subagent_card;
use super::*;

/// 「工作详情」组内的 pi-web ToolCallBlock 卡片（v56-2 c8/c9）。
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_tool_card(
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
    // 031-1：present_plan 审批弹窗专用工具——调用与结果都对用户隐藏
    // （结果文案是给模型的反馈「用户否决/批准」，用户视角的裁决 UI 是
    // 弹窗本身；真机反馈：不应看到工具返回行）。空 Div：不占版面。
    if name == "present_plan" {
        return gpui::div();
    }
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
pub(crate) fn alert_box(
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
