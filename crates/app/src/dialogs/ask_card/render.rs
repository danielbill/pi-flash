//! 问卷卡渲染（评审稿 ask-user-redesign.html 的 gpui 直译；自 ask_card.rs
//! 外移）：外框 + tab 栏 + 卡脚（选项行/自定义行/预览栏在 widgets.rs）。

use gpui::{
    FontWeight, MouseButton, SharedString, div, hsla, prelude::FluentBuilder as _, px, rgb,
};

use super::AskCard;
use super::widgets::{ask_option_rows, ask_other_row, ask_preview_pane};
use crate::*;

// 渲染（评审稿 ask-user-redesign.html 的 gpui 直译）
// ---------------------------------------------------------------------------

/// MIST accent #1e6559 的 HSL（167°, 54%, 26%）派生透明色
const ACCENT_HUE: f32 = 167.0 / 360.0;
const ACCENT_SAT: f32 = 0.54;
const ACCENT_LIT: f32 = 0.26;

pub(super) fn accent_tint(a: f32) -> gpui::Hsla {
    hsla(ACCENT_HUE, ACCENT_SAT, ACCENT_LIT, a)
}

pub(crate) fn render_ask_card(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let Some(card) = chat.aux.ask.clone() else {
        return div().into_any_element();
    };
    let t = crate::appearance::theme::theme();
    let n = card.questions.len();
    let done = card.questions.iter().filter(|q| q.answered()).count();
    let all_answered = done == n;
    let q = &card.questions[card.active];

    // ---- dismiss（点外 / Esc = 取消整卷） ----
    let weak_cancel = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_cancel.update(cx, |c, cx| c.ask_cancel(cx));
    };

    // ---- 键盘：Enter 跳题·提交 / 数字直选（Tab 已砍：与 Enter 表观重叠，
    //      用户拍板删除；Root 层 tab→Tab 绑定吞键的陷阱见 gpui.md keymap 节） ----
    let weak_keys = weak.clone();
    let keys = move |ev: &gpui::KeyDownEvent, window: &mut gpui::Window, cx: &mut gpui::App| {
        // 输入框持股焦点时整卡键盘让位：gpui-component 的 InputState 不停
        // 传播 Enter/数字键（vendor 仅 otp_input 停），按键会冒泡到这里 ——
        // 与输入框自己的 on_submit→defer(ask_enter) 双发 → 一次 Enter 连
        // 跳两题（2026-11-09 用户实测 Q1→Q3）；数字键同理会边打字边选中
        // 选项。打字态下 Enter 归输入框（on_submit），数字只作正文。
        let typing = weak_keys.upgrade().is_some_and(|h| {
            h.read(cx)
                .aux
                .ask_input
                .read(cx)
                .focus_handle_in(cx)
                .is_focused(window)
        });
        if typing {
            return;
        }
        let key = ev.keystroke.key.as_str();
        match key {
            "enter" => {
                let _ = weak_keys.update(cx, |c, cx| c.ask_enter(cx));
            }
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => {
                let d = (k.as_bytes()[0] - b'0') as usize;
                if d >= 1 {
                    let _ = weak_keys.update(cx, |c, cx| c.ask_digit(d - 1, cx));
                }
            }
            _ => {}
        }
    };

    // ---- tab 栏（含右上角 ×；装配见 ask_tab_bar） ----
    let tabs = ask_tab_bar(&card, weak, t);

    // ---- 题面（qcol + 可选预览栏） ----
    let with_pv = q.has_preview();
    let mut qcol = div()
        .flex()
        .flex_col()
        .gap(px(11.))
        .map(|d| if with_pv { d.w(px(400.)) } else { d.w_full() });
    qcol = qcol.child(
        div()
            .text_size(crate::appearance::ui_size(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(t.text))
            .child(SharedString::from(q.question.clone())),
    );

    qcol = qcol.child(ask_option_rows(q, weak, t));

    // 自定义答案行（AskCard 常驻模式）：active 题内嵌输入框，其余处只读回显
    qcol = qcol.child(ask_other_row(chat, q, weak, t));

    // relative = 预览侧栏（absolute 脱流，见 ask_preview_pane）的定位基准；
    // 行高只由左列决定，右栏全高随行
    let mut pane = div().relative().flex().gap(px(0.)).child(qcol);

    // 预览侧栏（当前题任一选项带 preview 时出现；装配见 ask_preview_pane）
    if with_pv {
        pane = pane.child(ask_preview_pane(&card.pv_scroll, q, t));
    }

    // ---- 卡脚：提示 + 取消 + 确认 [n/N]（装配见 ask_footer） ----
    let foot = ask_footer(weak, t, done, n, all_answered);

    // ---- 装配（浮层由 session/mod.rs main_column 挂进 chat-wrap 中栏；
    //      窄窗口下卡片随中栏收缩，两侧留 24px 呼吸边距） ----
    let card_el = div()
        // 问卷卡与内容列同宽（设置-界面「会话内容宽度」），窄窗由 max_w 收
        .w(px(crate::services::workspace::chat_content_width()))
        .max_w(gpui::relative(1.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(12.))
        .p(px(18.))
        .flex()
        .flex_col()
        .gap(px(11.))
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(tabs)
        .child(pane)
        .child(foot);

    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss)
        .on_key_down(keys)
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .px(px(24.))
                .child(card_el),
        )
        .into_any_element()
}

/// tab 栏：题签（ answered 实心点 / active 高亮 / 挂起题呼吸边）+ 右上角 ×
/// 关闭钮。内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
fn ask_tab_bar(
    card: &AskCard,
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
) -> gpui::Div {
    let mut tabs = div()
        .flex()
        .gap(px(5.))
        .items_center()
        .pb(px(10.))
        .border_b_1()
        .border_color(rgb(t.bg_selected));
    for (i, tq) in card.questions.iter().enumerate() {
        let answered = tq.answered();
        let active = i == card.active;
        let label = if tq.header.trim().is_empty() {
            format!("{} {}", tr("ask.question"), i + 1)
        } else {
            tq.header.trim().to_string()
        };
        let weak_tab = weak.clone();
        let mut tab = div()
            .id(SharedString::from(format!("ask-tab-{i}")))
            .flex()
            .items_center()
            .gap(px(7.))
            .px(px(11.))
            .py(px(5.))
            .rounded(px(8.))
            .text_size(crate::appearance::ui_size(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .font_family(crate::editor::markdown::MONO_FAMILY)
            .cursor_pointer()
            .child(div().size(px(6.)).rounded_full().map(|d| {
                if answered {
                    if active {
                        d.bg(rgb(t.accent_contrast))
                    } else {
                        d.bg(rgb(t.accent))
                    }
                } else if active {
                    d.border_1().border_color(rgb(t.accent_contrast))
                } else {
                    d.border_1().border_color(rgb(t.text_faint))
                }
            }))
            .child(SharedString::from(label))
            .map(|d| {
                if active {
                    d.bg(rgb(t.accent))
                        .text_color(rgb(t.accent_contrast))
                        .shadow_sm()
                } else if answered {
                    d.bg(accent_tint(0.08))
                        .text_color(rgb(t.accent_hover))
                        .hover(|s| s.bg(accent_tint(0.14)))
                } else {
                    d.text_color(rgb(t.text_muted))
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_tab.update(cx, |c, cx| c.ask_switch_tab(i, cx));
            });
        // 挂起中的请求对应题给一个呼吸点提示（提交前正在等这题）
        if card.pending.as_ref().is_some_and(|(pi, _)| *pi == i) {
            tab = tab.border_1().border_color(accent_tint(0.35));
        }
        tabs = tabs.child(tab);
    }
    // 右上角 X 关闭钮（点 × = 取消整卷；快捷键说明全部沉底）
    let weak_close = weak.clone();
    tabs.child(
        div().flex_1().flex().justify_end().child(
            div()
                .id("ask-close")
                .size(px(34.))
                .mt(px(-2.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .text_size(crate::appearance::ui_size(20.))
                .text_color(rgb(t.text_faint))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .child(tr("action.close"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_close.update(cx, |c, cx| c.ask_cancel(cx));
                }),
        ),
    )
}

/// 卡脚：快捷键提示行 + 取消 + 确认 [n/N]（全答完才亮起可点）。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
fn ask_footer(
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
    done: usize,
    n: usize,
    all_answered: bool,
) -> gpui::Div {
    let weak_cancel2 = weak.clone();
    let weak_ok = weak.clone();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(hint_row(
            &[
                ("enter", tr("ask.advance_submit")),
                ("1-4", tr("ask.direct_pick")),
                ("esc", tr("ask.cancel_all")),
            ],
            t,
        ))
        .child(div().flex_1())
        .child(
            div()
                .id("ask-cancel")
                .px(px(16.))
                .py(px(7.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .child(SharedString::from(tr("action.cancel").to_string()))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_cancel2.update(cx, |c, cx| c.ask_cancel(cx));
                }),
        )
        .child({
            let mut b = div()
                .id("ask-confirm")
                .px(px(16.))
                .py(px(7.))
                .rounded(px(8.))
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .child(SharedString::from(format!(
                    "{} [{}/{}]",
                    tr("ask.confirm"),
                    done,
                    n
                )));
            if all_answered {
                b = b
                    .bg(rgb(t.accent))
                    .text_color(rgb(t.accent_contrast))
                    .shadow_sm()
                    .hover(|s| s.bg(rgb(t.accent_hover)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_ok.update(cx, |c, cx| c.ask_confirm(cx));
                    });
            } else {
                b = b.bg(rgb(t.bg_selected)).text_color(rgb(t.text_faint));
            }
            b
        })
}

/// kbd 提示组：小键帽 + 灰说明
fn hint_row(pairs: &[(&str, &str)], t: &crate::appearance::theme::Theme) -> gpui::AnyElement {
    let mut row = div().flex().items_center().gap(px(5.));
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            row = row.child(div().mx(px(2.)));
        }
        row = row
            .child(
                div()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(9.5))
                    .text_color(rgb(t.text_muted))
                    .border_1()
                    .border_color(rgb(t.border))
                    .border_b_2()
                    .rounded(px(4.))
                    .px(px(5.))
                    .py(px(1.))
                    .bg(rgb(t.bg))
                    .child(SharedString::from(key.to_string())),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_size(11.5))
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(label.to_string())),
            );
    }
    row.into_any_element()
}
