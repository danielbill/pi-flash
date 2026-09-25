//! General tab: theme picker + runtime info.

use super::*;

/// General tab: theme picker (4 pi-web themes with swatch previews) +
/// runtime info. Theme choice persists in pi settings.json (shared with the
/// pi TUI).
pub(crate) fn mc_general_view(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> (gpui::AnyElement, gpui::AnyElement) {
    let t = T();
    let mut detail = div()
        .id("mc-detail")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(20.))
        .text_size(px(12.))
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(tr("外观")),
                )
                .child(
                    div()
                        .font_family("Consolas")
                        .text_size(px(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!(
                            "pi-flash v{} · vendored pi {}",
                            env!("CARGO_PKG_VERSION"),
                            pi_link::vendor::vendored_version().unwrap_or_default()
                        ))),
                ),
        );
    // language row (pi-web i18n parity: 简体中文 / 繁體中文 / English)
    let lang_current = i18n::lang_ix();
    for (ix, label) in i18n::LANG_LABELS.iter().enumerate() {
        let active = lang_current == ix;
        let weak_lang = weak.clone();
        detail = detail.child(
            div()
                .id(SharedString::from(format!("lang-{ix}")))
                .min_h(px(36.))
                .py(px(6.))
                .px(px(9.))
                .rounded(px(6.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_lang.update(cx, |c, cx| {
                        i18n::set_lang(ix);
                        save_lang_pref(ix);
                        cx.notify();
                    });
                })
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                        .text_color(rgb(t.text))
                        .child(SharedString::from(label.to_string())),
                )
                .child(if active {
                    div().text_size(px(10.)).text_color(rgb(t.accent)).child(tr("当前")).into_any_element()
                } else {
                    div().into_any_element()
                }),
        );
    }
    let current = theme::theme_name();
    for (name, th) in theme::ALL {
        let active = *name == current;
        let weak_row = weak.clone();
        let theme_name = name.to_string();
        detail = detail.child(
            div()
                .id(SharedString::from(format!("theme-{name}")))
                .min_h(px(44.))
                .py(px(8.))
                .px(px(9.))
                .rounded(px(6.))
                .border_1()
                .border_color(if active { rgb(th.accent) } else { rgb(t.border) })
                .bg(rgb(t.bg_panel))
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| {
                        if theme::set_by_name(&theme_name) {
                            let _ = pi_link::config::write_theme(
                                &pi_link::config::settings_path(),
                                &theme_name,
                            );
                            cx.notify();
                        }
                    });
                })
                // swatch preview: bg / accent / border / text dots
                .child(div().w(px(28.)).h(px(20.)).rounded(px(4.)).border_1().border_color(rgb(th.border)).bg(rgb(th.bg)).flex().items_center().justify_center().gap_0p5()
                    .child(div().size(px(6.)).rounded_full().bg(rgb(th.accent)))
                    .child(div().size(px(6.)).rounded_full().bg(rgb(th.text_muted)))
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                        .text_color(rgb(t.text))
                        .child(SharedString::from(name.to_string())),
                )
                .child(if active {
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(th.accent))
                        .child(tr("当前"))
                        .into_any_element()
                } else {
                    div().into_any_element()
                }),
        );
    }
    detail = detail.child(
        div()
            .text_size(px(11.))
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(crate::i18n::tf(
                tr("主题写入 ~/.pi/agent/settings.json 的 theme 键（与 pi 共用）；vendored pi {}"),
                &[("v", pi_link::vendor::vendored_version().unwrap_or_default())],
            ))),
    );
    let _ = chat;
    (
        div().into_any_element(),
        detail.into_any_element(),
    )
}

