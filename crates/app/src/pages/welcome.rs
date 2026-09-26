//! pages (005/011/012): full-page states over the shell body.
//! welcome — shown until the project/session list has loaded (011);
//! newSession — 012 (互动文字 + logo + input) is the empty-session hero
//! inside sessionView, refined separately.

use gpui::{div, prelude::*, px, rgb};

use crate::i18n::tr;
use crate::theme::theme as T;
use crate::ui::spinner;

pub(crate) fn welcome() -> gpui::Div {
    let t = T();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text))
        // logo (011 参考 ZCode welcome: logo + 版本 + 轻量加载提示)
        .child(
            div()
                .text_size(px(36.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(t.accent))
                .child("π"),
        )
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("pi-flash"),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .child(env!("CARGO_PKG_VERSION").to_string()),
        )
        .child(
            div()
                .mt_2()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(rgb(t.text_muted))
                .child(spinner(13., t.text_muted))
                .child(tr("正在加载项目与会话…")),
        )
        .child(
            div()
                .mt_6()
                .max_w(px(420.))
                .text_center()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .child(tr("极速的 Pi 桌面端 — 支持流式响应、会话管理、文件树与终端。")),
        )
}
