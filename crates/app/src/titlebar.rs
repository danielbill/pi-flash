//! Title bar (005 上段): logo + drag region left, settings + window
//! control buttons right. The bar is a client-side decoration — the whole
//! surface is a `WindowControlArea::Drag` hitbox so the platform handles
//! dragging and double-click-zoom; the three caption buttons register
//! Min/Max/Close hitboxes the same way (zed platform_title_bar parity:
//! on Windows the platform layer owns the click behavior).

use gpui::{MouseButton, SharedString, Window, div, prelude::*, px, rgb};
use gpui::WindowControlArea;

use crate::Chat;
use crate::i18n::tr;
use crate::theme::theme as T;

/// Windows title bar height (zed ui constants parity).
const HEIGHT: f32 = 32.;

/// Caption-button glyph font (Win11; MDL2 covers Win10).
pub const CAPTION_FONT: &str = "Segoe Fluent Icons";

pub(crate) fn title_bar(
    _chat: &mut Chat,
    window: &mut Window,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let maximized = window.is_maximized();
    // Glyphs: 0xE921 min, 0xE922 max, 0xE923 restore, 0xE8BB close.
    let max_glyph = if maximized { "\u{E923}" } else { "\u{E922}" };
    let max_tip = if maximized { tr("向下还原") } else { tr("最大化") };

    let mut bar = div()
        .id("titlebar")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .window_control_area(WindowControlArea::Drag)
        // logo (left)
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .px_3()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(t.accent))
                        .child(SharedString::from("π")),
                )
                .child(SharedString::from("pi-flash")),
        )
        // settings button (005 右侧操作区)
        .child(
            div()
                .id("titlebar-settings")
                .mx_2()
                .px_2()
                .py_0p5()
                .rounded(px(5.))
                .cursor_pointer()
                .text_xs()
                .text_color(rgb(t.text_muted))
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        this.open_settings(0, cx);
                    },
                ))
                .child(SharedString::from(tr("设置"))),
        )
        // drag filler to the caption buttons
        .child(div().flex_1());

    for (area, glyph, tip) in [
        (WindowControlArea::Min, "\u{E921}", tr("最小化")),
        (WindowControlArea::Max, max_glyph, max_tip),
    ] {
        bar = bar.child(caption_button(area, glyph, tip, false, t));
    }
    bar = bar.child(caption_button(
        WindowControlArea::Close,
        "\u{E8BB}",
        tr("关闭"),
        true,
        t,
    ));
    bar
}

fn caption_button(
    area: WindowControlArea,
    glyph: &'static str,
    tip: &'static str,
    danger: bool,
    t: &crate::theme::Theme,
) -> impl gpui::IntoElement {
    let hover_bg = if danger {
        rgb(0xe81123) // Windows close-button red (platform_windows.rs parity)
    } else {
        rgb(t.bg_hover)
    };
    let hover_fg = if danger { rgb(0xffffff) } else { rgb(t.text) };
    let _ = tip; // tooltips land with the zed_ui vendoring (Tooltip text)
    div()
        .id(SharedString::from(format!("wb-{}", area as u8)))
        .w(px(46.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(CAPTION_FONT)
        .text_sm()
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .window_control_area(area)
        .child(
            div()
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(glyph)),
        )
}
