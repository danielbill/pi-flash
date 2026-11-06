//! 问卷卡渲染件（自 ask_card.rs 外移）：选项行列表 / 自定义答案行 /
//! 预览侧栏 / 选中标记。

use gpui::{FontWeight, SharedString, div, px, rgb};

use super::render::accent_tint;
use super::{AskQ, Chat};
use crate::*;

/// 选项行列表：序号徽标 + 标签/描述 + 选中标记，选中行 tint。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
pub(super) fn ask_option_rows(
    q: &AskQ,
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
) -> gpui::Div {
    let mut opts = div().flex().flex_col().gap(px(6.));
    for (j, opt) in q.options.iter().enumerate() {
        let sel = q.sel[j];
        let weak_opt = weak.clone();
        let mark = option_mark(q.multi, sel, t);
        let mut row = div()
            .id(SharedString::from(format!("ask-opt-{j}")))
            .flex()
            .items_start()
            .gap(px(10.))
            .px(px(11.))
            .py(px(9.))
            .rounded(px(9.))
            .border_1()
            .cursor_pointer()
            .child(
                div()
                    .flex_none()
                    .min_w(px(18.))
                    .h(px(18.))
                    .mt(px(1.))
                    .rounded(px(5.))
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(SharedString::from(format!("{}", j + 1)))
                    .map(|d| {
                        if sel {
                            d.bg(rgb(t.accent)).text_color(rgb(t.accent_contrast))
                        } else {
                            d.border_1()
                                .border_color(rgb(t.bg_selected))
                                .bg(rgb(t.bg))
                                .text_color(rgb(t.text_faint))
                        }
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(SharedString::from(opt.label.clone())),
                    )
                    // 空描述不渲染：占位空行会把无描述选项撑到 1.5 倍高，
                    // 与自定义答案行不等高（用户定稿：视觉丑陋）
                    .when(!opt.desc.trim().is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(crate::appearance::ui_size(12.))
                                .text_color(rgb(t.text_muted))
                                .mt(px(2.))
                                .child(SharedString::from(opt.desc.clone())),
                        )
                    }),
            )
            .child(mark)
            .map(|d| {
                if sel {
                    d.bg(accent_tint(0.08)).border_color(accent_tint(0.35))
                } else {
                    d.border_color(gpui::hsla(0., 0., 0., 0.))
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_opt.update(cx, |c, cx| c.ask_toggle_option(j, cx));
            });
        if !q.multi && q.other_sel {
            // 单选自定义态下的选项行降透明度提示互斥
            row = row.opacity(0.72);
        }
        opts = opts.child(row);
    }
    opts
}

/// 自定义答案行（AskCard 常驻模式）：flat 输入框**常驻**（placeholder 走
/// 输入框自身，行高恒定 = 单行选项行高，选中态不再跳变）。行体点选 =
/// 确保选中并聚焦；圆标点选 = 切换选中（取消选中入口，用户定稿）。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
pub(super) fn ask_other_row(
    chat: &Chat,
    q: &AskQ,
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
) -> gpui::Stateful<gpui::Div> {
    let weak_other = weak.clone();
    let weak_mark = weak.clone();
    let mut row = div()
        .id("ask-other")
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(11.))
        // py(4)+flat 输入槽 30px = 38px ≈ 单行选项行（py9+文本行 ~37.5），
        // 两类行同高（用户定稿：高度差视觉丑陋）
        .py(px(4.))
        .rounded(px(9.))
        .border_1()
        .cursor_pointer()
        .child(
            // 圆标 = 切换开关：stop_propagation 防止行体 ensure 分支盖掉
            div()
                .id("ask-other-mark")
                .flex()
                .cursor_pointer()
                .child(option_mark(q.multi, q.other_sel, t))
                .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                    let _ = ev;
                    let _ = weak_mark.update(cx, |c, cx| c.ask_other_toggle(window, cx));
                    cx.stop_propagation();
                }),
        )
        .map(|d| {
            if q.other_sel {
                d.bg(accent_tint(0.08)).border_color(accent_tint(0.35))
            } else {
                d.border_color(rgb(t.bg_selected))
                    .bg(rgb(t.bg))
                    .hover(|s| s.bg(rgb(t.bg_hover)))
            }
        })
        .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            let _ = ev;
            // 输入区点击从 vendor 编辑器冒泡上来（其不 stop_propagation，
            // 已核）：与行体同走 ensure，编辑意图不会误触取消
            let _ = weak_other.update(cx, |c, cx| c.ask_other_select(window, cx));
        });
    row = row.child(chat.aux.ask_input.clone().into_any_element());
    row
}

/// 预览侧栏：当前选中（或首个带 preview 的）选项的 PREVIEW 头 + mono 正文框。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
pub(super) fn ask_preview_pane(
    scroll: &gpui::ScrollHandle,
    q: &AskQ,
    t: &crate::appearance::theme::Theme,
) -> gpui::Div {
    let pv_idx = q
        .sel
        .iter()
        .position(|s| *s)
        .or_else(|| q.options.iter().position(|o| o.preview.is_some()))
        .unwrap_or(0);
    let pv_text = q.options[pv_idx]
        .preview
        .clone()
        .unwrap_or_else(|| tr("ask.no_preview").to_string());
    let pv_name = q.options[pv_idx].label.clone();
    // 绝对定位脱流（top0→bottom0，左缘 = 左列 with_pv 定宽 400）：行高只由
    // 左列（题面+选项+自定义行）决定，预览区全高、底边与自定义行齐平
    //（用户定稿）；超长正文滚而不撑卡（max_h 裁切已废）
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(400.))
        .right_0()
        .pl(px(16.))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pb(px(8.))
                .border_b_1()
                .border_color(rgb(t.bg_selected))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(t.text_soft))
                .child(div().size(px(6.)).rounded_full().bg(rgb(t.accent)))
                .child(SharedString::from(format!("PREVIEW · {pv_name}"))),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .mt(px(10.))
                .child(
                    div()
                        .id("ask-preview-body")
                        .w_full()
                        .h_full()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .bg(rgb(t.tool_bg))
                        .border_1()
                        .border_color(rgb(t.border))
                        .rounded(px(9.))
                        .px(px(14.))
                        .py(px(13.))
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(11.5))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(pv_text)),
                )
                .child(crate::ui::psp_scrollbar::menu_scrollbar(scroll)),
        )
}

/// 选中标记：单选 = 圆（选中实心+白点），多选 = 方（选中 accent+白勾）
fn option_mark(multi: bool, sel: bool, t: &crate::appearance::theme::Theme) -> gpui::AnyElement {
    let base = div()
        .flex_none()
        .size(px(16.))
        .mt(px(2.))
        .flex()
        .items_center()
        .justify_center()
        .map(|d| {
            if multi {
                d.rounded(px(5.))
            } else {
                d.rounded_full()
            }
        });
    if sel {
        base.bg(rgb(t.accent))
            .border_1()
            .border_color(rgb(t.accent))
            .map(|d| {
                if multi {
                    // 白勾（无 45° 旋转原语，用字形）
                    d.text_color(rgb(t.accent_contrast))
                        .text_size(crate::appearance::ui_size(9.5))
                        .font_weight(FontWeight::BOLD)
                        .child(tr("action.confirm_mark"))
                } else {
                    d.child(div().size(px(6.)).rounded_full().bg(rgb(t.accent_contrast)))
                }
            })
            .into_any_element()
    } else {
        base.border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg))
            .into_any_element()
    }
}
