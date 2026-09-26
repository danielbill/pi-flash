//! statusControlBar (018): bottom bar, dual-purpose — status strip and the
//! functionPanel control row. Left-aligned mutually-exclusive icon buttons
//! switch the dock view (sessions / files / git / terminal); right-click on
//! a button flips the dock side (015). Right side carries branch + pi
//! status (zed status bar parity).

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::DockPanel;
use crate::theme::theme as T;

const HEIGHT: f32 = 28.;

/// The dock views a status-bar button can target (functionPanel views).
pub(crate) const VIEW_BUTTONS: &[(&str, &str)] = &[
    ("sessions", "panel-left"),
    ("files", "file"),
    ("git", "git-branch"),
    ("terminal", "terminal"),
];

pub(crate) fn control_bar(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();
    let active: &str = chat.dock_panel.as_str();

    let mut bar = div()
        .id("status-bar")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .bg(rgb(t.bg_panel))
        .border_t_1()
        .border_color(rgb(t.border));

    for (view, icon_name) in VIEW_BUTTONS {
        let on = *view == active;
        let view = *view;
        bar = bar.child(
            div()
                .id(SharedString::from(format!("sb-{view}")))
                .px_2()
                .py_1()
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_color(if on { rgb(t.accent) } else { rgb(t.text_muted) })
                .when(on, |d| d.bg(rgb(t.bg_hover)))
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                // left click: switch the dock view (mutually exclusive)
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.dock_panel = DockPanel::parse(view);
                    this.persist_dock();
                    cx.notify();
                }))
                // right click: flip the dock side (015)
                .on_mouse_down(MouseButton::Right, cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.dock_right = !this.dock_right;
                    this.persist_dock();
                    cx.notify();
                }))
                .child(crate::ui::icon(icon_name, 14., if on { t.accent } else { t.text_muted })),
        );
    }

    // right side: branch + status (pi session line)
    bar = bar.child(div().flex_1());
    bar = bar
        .child(
            div()
                .px_2()
                .text_xs()
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(format!("{} {}", "\u{2325}", chat.branch))),
        )
        .child(
            div()
                .px_2()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(chat.status.clone())),
        );
    bar
}
