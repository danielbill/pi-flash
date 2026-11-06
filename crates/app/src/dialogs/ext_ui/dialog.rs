//! 扩展阻塞弹窗（pi-web blocking dialog parity；自 ext_ui.rs 外移）：
//! Select/Confirm/Input/Editor 四类 + 底部按钮行。

use gpui::{MouseButton, SharedString, div, px, relative, rgb};

use super::{Chat, bar_title_of};
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::*;

pub(crate) fn render_ext_dialog(
    chat: &mut Chat,
    req: pi_link::protocol::ExtensionUiRequest,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    use pi_link::protocol::ExtUiMethod;
    let t = T();
    let weak = weak.clone();
    // 三块：标题栏 / 信息面板 / 操作面板
    let (bar_title, info, ops): (String, gpui::AnyElement, gpui::AnyElement) = match &req.method {
        ExtUiMethod::Select { title, options } => {
            let weak_opts = weak.clone();
            // 键盘导航高亮行（zi2s.7）：↑↓ 移动、Enter 选中；首行预选
            let sel_ix = chat.aux.ext_sel_ix;
            let opts: Vec<gpui::AnyElement> = options
                .iter()
                .enumerate()
                .map(|(ix, o)| {
                    let weak_o = weak_opts.clone();
                    let v = o.clone();
                    div()
                        .id(SharedString::from(format!("ext-opt-{ix}")))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_selected)))
                        .when(sel_ix == ix, |d| {
                            d.bg(rgb(t.bg_selected)).border_color(rgb(t.accent))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_o.update(cx, |c, cx| {
                                c.ext_respond(Some(v.clone()), None, false, cx)
                            });
                        })
                        .child(SharedString::from(o.clone()))
                        .into_any_element()
                })
                .collect();
            (
                bar_title_of(title),
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text))
                    .child(SharedString::from(title.clone()))
                    .into_any_element(),
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(opts)
                    .into_any_element(),
            )
        }
        ExtUiMethod::Confirm { title, message } => (
            title.clone(),
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(message.clone()))
                .into_any_element(),
            div().into_any_element(),
        ),
        ExtUiMethod::Input { title, .. } => (
            title.clone(),
            div().into_any_element(),
            chat.aux.ext_input.clone().into_any_element(),
        ),
        // editor 用专用多行输入（zi2s.6）：Enter 提交，prefill 带真实换行
        ExtUiMethod::Editor { title, .. } => (
            title.clone(),
            div().into_any_element(),
            chat.aux.ext_editor_input.clone().into_any_element(),
        ),
        _ => (
            String::new(),
            div().into_any_element(),
            div().into_any_element(),
        ),
    };
    let bar_title: SharedString = bar_title.into();
    let is_confirm = matches!(req.method, ExtUiMethod::Confirm { .. });
    let is_select = matches!(req.method, ExtUiMethod::Select { .. });
    // editor 弹窗的提交值在专用多行输入里（zi2s.6）
    let is_editor = matches!(req.method, ExtUiMethod::Editor { .. });
    let weak_close = weak.clone();
    let weak_dismiss = weak.clone();
    // 浮层公共基座：遮挡不穿透 + 点外关闭 + ESC 关闭 —— 两者都等于「取消」
    //（扩展收到 cancelled 响应）
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| c.ext_respond(None, None, true, cx));
    };
    // select 键盘导航（zi2s.7，pi-web/TUI 同语义）：↑↓ 移动高亮（循环）、
    // Enter 选中高亮行，Esc 取消由 overlay 管。焦点在浮层上（overlay::layer
    // track_focus）才派发；非 select 弹窗（input/editor 输入框在打字）一律
    // 不拦不吞，按键照常进输入组件。
    let weak_nav = weak.clone();
    let nav = move |ev: &gpui::KeyDownEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        let key = ev.keystroke.key.as_str();
        let mut handled = false;
        match key {
            "up" | "down" => {
                let delta: i64 = if key == "down" { 1 } else { -1 };
                let _ = weak_nav.update(cx, |c, cx| {
                    if let Some(pi_link::protocol::ExtensionUiRequest {
                        method: ExtUiMethod::Select { options, .. },
                        ..
                    }) = &c.aux.ext_dialog
                    {
                        let n = options.len();
                        if n > 0 {
                            let cur = c.aux.ext_sel_ix as i64;
                            c.aux.ext_sel_ix = (cur + delta).rem_euclid(n as i64) as usize;
                            cx.notify();
                            handled = true;
                        }
                    }
                });
            }
            "enter" => {
                let _ = weak_nav.update(cx, |c, cx| {
                    let head = c.aux.ext_dialog.clone();
                    if let Some(pi_link::protocol::ExtensionUiRequest {
                        method: ExtUiMethod::Select { options, .. },
                        ..
                    }) = &head
                    {
                        if let Some(v) = options.get(c.aux.ext_sel_ix).cloned() {
                            handled = true;
                            c.ext_respond(Some(v), None, false, cx);
                        }
                    }
                });
            }
            _ => {}
        }
        if handled {
            cx.stop_propagation();
        }
    };
    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss)
        .on_key_down(nav)
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(600.))
                .max_w(relative(0.96))
                .h(px(600.))
                .max_h(relative(0.92))
                .bg(rgb(t.bg_panel))
                .border_1()
                .border_color(rgb(t.border))
                .rounded_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                .shadow_lg()
                // 卡片内点击不冒泡到背板（否则点卡片任意处都会取消）
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                // ── 通用标题栏（首行标题 + 排队数 + 关闭按钮常驻） ──
                .child(
                    div()
                        .h(px(40.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_4()
                        .border_b_1()
                        .border_color(rgb(t.border))
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(14.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(bar_title),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .children({
                                    // 队列尾随数（pi-web waitingExtensionDialogCount parity）
                                    let n = chat.aux.ext_dialog_waiting.len();
                                    (n > 0).then(|| {
                                        div()
                                            .text_size(crate::appearance::ui_size(11.))
                                            .text_color(rgb(t.text_muted))
                                            .child(crate::i18n::tf(
                                                "ext.dialog_waiting",
                                                &[("count", n.to_string())],
                                            ))
                                    })
                                })
                                // 关闭钮走 overlay::close_btn 唯一实现（§5.2 IB 表）
                                .child(crate::ui::overlay::close_btn(
                                    "ext-close",
                                    t,
                                    move |_, cx| {
                                        let _ = weak_close.update(cx, |c, cx| {
                                            c.ext_respond(None, None, true, cx)
                                        });
                                    },
                                )),
                        ),
                )
                // ── 上：信息面板（占剩余高度一半，独立滚动） ──
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .border_b_1()
                        .border_color(rgb(t.border))
                        .child(
                            div()
                                .id("ext-dlg-info")
                                .absolute()
                                .top(px(0.))
                                .left(px(0.))
                                .w_full()
                                .h_full()
                                .px_4()
                                .py_3()
                                .overflow_y_scroll()
                                .track_scroll(&chat.aux.ext_dialog_info_scroll)
                                .child(info),
                        )
                        .child(crate::ui::psp_scrollbar::menu_scrollbar(
                            &chat.aux.ext_dialog_info_scroll,
                        )),
                )
                // ── 下：操作面板（占另一半，独立滚动） ──
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(
                            div()
                                .id("ext-dlg-ops")
                                .absolute()
                                .top(px(0.))
                                .left(px(0.))
                                .w_full()
                                .h_full()
                                .px_4()
                                .py_3()
                                .overflow_y_scroll()
                                .track_scroll(&chat.aux.ext_dialog_op_scroll)
                                .child(ops),
                        )
                        .child(crate::ui::psp_scrollbar::menu_scrollbar(
                            &chat.aux.ext_dialog_op_scroll,
                        )),
                )
                // ── 底部按钮行（select 的选项就是操作，不出现底部按钮） ──
                .children((!is_select).then(|| {
                    ext_dialog_footer(is_confirm, is_editor, weak.clone(), weak.clone(), t)
                })),
        )
        .into_any_element()
}

/// 底部按钮行：取消 + confirm 的「否/是」或 input/editor 的「提交」。
/// select 弹窗的选项本身就是操作，不渲染此行。
fn ext_dialog_footer(
    is_confirm: bool,
    is_editor: bool,
    weak_ok: gpui::WeakEntity<Chat>,
    weak_cancel: gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex_shrink_0()
        .flex()
        .justify_end()
        .gap_2()
        .px_4()
        .py_3()
        .border_t_1()
        .border_color(rgb(t.border))
        .child(
            div()
                .id("ext-cancel")
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(t.border))
                // §5.1 BTN_SM 字号（原 text_xs 裸字号）
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(t.text)))
                .on_mouse_down(MouseButton::Left, {
                    let weak = weak_cancel.clone();
                    move |_, _, cx| {
                        let _ = weak.update(cx, |c, cx| c.ext_respond(None, None, true, cx));
                    }
                })
                .child(tr("action.cancel")),
        )
        .children(is_confirm.then(|| {
            div()
                .id("ext-no")
                .px_3()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(t.border))
                // §5.1 BTN_SM 字号（原 text_xs 裸字号）
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, {
                    let weak = weak_ok.clone();
                    move |_, _, cx| {
                        let _ =
                            weak.update(cx, |c, cx| c.ext_respond(None, Some(false), false, cx));
                    }
                })
                .child(tr("action.no"))
        }))
        .child({
            let label = if is_confirm {
                tr("action.yes")
            } else {
                tr("action.submit")
            };
            let weak = weak_ok.clone();
            div()
                .id("ext-ok")
                .px_3()
                .py_1()
                .rounded_md()
                .bg(rgb(t.accent))
                // §5.1 BTN_SM 字号（原 text_xs 裸字号）
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.accent_contrast))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.accent_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        if is_confirm {
                            c.ext_respond(None, Some(true), false, cx);
                        } else {
                            // editor = 专用多行输入（zi2s.6），
                            // input = 单行 ext_input
                            let v = if is_editor {
                                c.aux.ext_editor_input.read(cx).value().to_string()
                            } else {
                                c.aux.ext_input.read(cx).value().to_string()
                            };
                            c.ext_respond(Some(v), None, false, cx);
                        }
                    });
                })
                .child(label)
        })
        .into_any_element()
}
