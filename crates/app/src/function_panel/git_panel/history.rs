//! git 面板 History 页（itlq.12 自 git_panel.rs 切出，纯结构搬移）：提交
//! 列表（标题 + ↑ 推送小钮；元信息：作者 · 时间 · hash）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::ui::icon;

/// History: 提交列表（标题 + ↑ 推送小钮；元信息：作者 · 时间 · hash）。
pub(crate) fn history_body(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let mut list = div()
        .id("git-history")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px(px(6.))
        .py(px(4.));
    if chat.ws.git_log.is_empty() {
        list = list.child(
            div()
                .px(px(8.))
                .py(px(6.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("git.no_commits")),
        );
    }
    for c in &chat.ws.git_log {
        let subject: SharedString = c.subject.clone().into();
        let meta: SharedString = format!("{} · {} · {}", c.author, c.date, c.hash).into();
        let weak_push = weak.clone();
        list = list.child(
            div()
                .id(SharedString::from(format!("gc-{}", c.hash)))
                .px(px(8.))
                .pt(px(8.))
                .pb(px(9.))
                .border_b_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x4d)))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(7.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(subject),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("gc-push-{}", c.hash)))
                                .size(px(17.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .text_color(rgb(t.text_dim))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = weak_push.update(cx, |c, cx| c.git_push_branch(cx));
                                })
                                .child(icon("arrow-up", 10., t.text_dim)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .mt(px(4.))
                        .pl(px(2.))
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_dim))
                        .child(
                            div()
                                .size(px(12.))
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(t.border))
                                .bg(rgb(t.bg)),
                        )
                        .child(meta),
                ),
        );
    }
    list.into_any_element()
}
