//! General tab: theme picker + runtime info.

use super::*;

/// General tab: theme picker (4 pi-web themes with swatch previews) +
/// runtime info. Theme choice persists in pi settings.json (shared with the
/// pi TUI).
pub(crate) fn mc_general_view(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
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
                    let _ = weak_lang.update(cx, |_c, cx| {
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
    for entry in crate::appearance::registry() {
        let (name, th) = (
            entry.id,
            theme::ALL
                .iter()
                .find(|(id, _)| *id == entry.id)
                .map(|(_, t)| *t)
                .expect("registry id missing from theme table"),
        );
        let active = name == current;
        let weak_row = weak.clone();
        let theme_name = name.to_string();
        let display: SharedString = if entry.name != entry.id {
            format!("{} ({})", entry.name, entry.id).into()
        } else {
            entry.id.into()
        };
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
                    let _ = weak_row.update(cx, |_c, cx| {
                        if crate::appearance::persist_theme(&theme_name) {
                            let _ = pi_link::config::write_theme(
                                &pi_link::config::settings_path(),
                                &theme_name,
                            );
                            // 006: re-map tokens so widget-library surfaces
                            // (inputs/modals) follow the switch
                            crate::appearance::sync_gpui_tokens(cx);
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
                        .child(display),
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

    detail = detail.child(appearance_rows(weak, t));
    // v54 界面页补充：提示音 + 跨项目预载会话数（set-row 版式）
    detail = detail.child(v54_rows(weak, t));
    let _ = chat;
    detail.into_any_element()
}

/// v54 界面页补充行（提示音 / 跨项目预载会话数）。
fn v54_rows(weak: &gpui::WeakEntity<Chat>, t: &crate::theme::Theme) -> gpui::AnyElement {
    let mut col = div().w_full().flex().flex_col().mt_px();
    // 提示音
    let sound_on = crate::services::workspace::load_sound_pref();
    col = col.child(
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .py(px(12.))
            .border_t_1()
            .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x40)))
            .child(
                div()
                    .flex_1()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(tr("提示音")),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(t.text_dim))
                            .child(tr("agent 运行结束播放系统提示音")),
                    ),
            )
            .child(
                div()
                    .id("ui-sound")
                    .w(px(34.))
                    .h(px(19.))
                    .rounded_full()
                    .bg(rgb(if sound_on { t.accent } else { t.border }))
                    .relative()
                    .cursor_pointer()
                    .child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .when(sound_on, |d| d.left(px(17.)))
                            .when(!sound_on, |d| d.left(px(2.)))
                            .size(px(15.))
                            .rounded_full()
                            .bg(rgb(0xffffff)),
                    )
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                c.sound_on = !c.sound_on;
                                crate::services::workspace::save_sound_pref(c.sound_on);
                                if c.sound_on {
                                    crate::services::workspace::play_notify_sound();
                                }
                                cx.notify();
                            });
                        }
                    }),
            ),
    );
    col.into_any_element()
}


/// 006 外观 rows (icon theme + three font slots), split out of
/// mc_general_view for the view-size budget.
fn appearance_rows(weak: &gpui::WeakEntity<Chat>, _t: &crate::theme::Theme) -> gpui::AnyElement {
    let t = T();
    let weak = weak.clone();
    let mut out = div().w_full().flex().flex_col().gap_4();
    // ---- icon theme (006): zed architecture, pi-web set is the built-in ----
    let icon_current = crate::appearance::icon_theme_id();
    for it in crate::appearance::ICON_THEMES {
        let active = it.id == icon_current;
        out = out.child(
            div()
                .min_h(px(36.))
                .py(px(6.))
                .px(px(9.))
                .rounded(px(6.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(rgb(t.bg_panel))
                .flex()
                .items_center()
                .gap_2()
                .child(icon("image", 12., t.text_muted))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                        .text_color(rgb(t.text))
                        .child(SharedString::from(format!("{} ({})", it.name, tr("图标主题")))),
                )
                .child(if active {
                    div().text_size(px(10.)).text_color(rgb(t.accent)).child(tr("当前")).into_any_element()
                } else {
                    div().into_any_element()
                }),
        );
    }

    // ---- font slots (006): session / panel / markdown, family+size ----
    for (slot, label) in [
        (crate::appearance::FontSlot::Session, tr("会话字体")),
        (crate::appearance::FontSlot::Panel, tr("面板字体")),
        (crate::appearance::FontSlot::Markdown, tr("Markdown 字体")),
    ] {
        let spec = match slot {
            crate::appearance::FontSlot::Session => crate::appearance::session_font(),
            crate::appearance::FontSlot::Panel => crate::appearance::panel_font(),
            crate::appearance::FontSlot::Markdown => crate::appearance::markdown_font(),
        };
        let choices = crate::appearance::FONT_CHOICES;
        let fam_ix = choices
            .iter()
            .position(|f| *f == spec.family)
            .unwrap_or(0);
        let weak_fam_prev = weak.clone();
        let weak_fam_next = weak_fam_prev.clone();
        let weak_dec = weak.clone();
        let weak_inc = weak_dec.clone();
        let fam_dec = spec.family.clone();
        let fam_inc = spec.family.clone();
        let size0 = spec.size;
        out = out.child(
            div()
                .min_h(px(40.))
                .py(px(6.))
                .px(px(9.))
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .w(px(110.))
                        .text_size(px(12.))
                        .text_color(rgb(t.text))
                        .child(label),
                )
                // family cycle: < family >
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .id(SharedString::from(format!("font-prev-{:?}", slot)))
                                .px_2()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_fam_prev.update(cx, |_c, cx| {
                                        crate::appearance::save_font(
                                            slot,
                                            crate::services::workspace::FontSpec {
                                                family: cycle_family(choices, fam_ix, -1).to_string(),
                                                size: size0,
                                            },
                                        );
                                        cx.notify();
                                    });
                                })
                                .text_size(px(12.))
                                .text_color(rgb(t.text_muted))
                                .child("‹"),
                        )
                        .child(
                            div()
                                .min_w(px(120.))
                                .text_size(px(12.))
                                .text_color(rgb(t.text))
                                .font_family(spec.family.clone())
                                .child(SharedString::from(spec.family.clone())),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("font-next-{:?}", slot)))
                                .px_2()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .text_size(px(12.))
                                .text_color(rgb(t.text_muted))
                                .child("›")
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_fam_next.update(cx, |_c, cx| {
                                        crate::appearance::save_font(
                                            slot,
                                            crate::services::workspace::FontSpec {
                                                family: cycle_family(choices, fam_ix, 1).to_string(),
                                                size: size0,
                                            },
                                        );
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                // size stepper: - n +
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .id(SharedString::from(format!("font-dec-{:?}", slot)))
                                .px_2()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .text_size(px(12.))
                                .child("−")
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_dec.update(cx, |_c, cx| {
                                        crate::appearance::save_font(
                                            slot,
                                            crate::services::workspace::FontSpec {
                                                family: fam_dec.clone(),
                                                size: (size0 - 1.).max(10.),
                                            },
                                        );
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            div()
                                .min_w(px(28.))
                                .text_align(gpui::TextAlign::Center)
                                .text_size(px(12.))
                                .font_family("Consolas")
                                .text_color(rgb(t.text))
                                .child(SharedString::from(format!("{}", spec.size as i32))),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("font-inc-{:?}", slot)))
                                .px_2()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .text_size(px(12.))
                                .child("+")
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_inc.update(cx, |_c, cx| {
                                        crate::appearance::save_font(
                                            slot,
                                            crate::services::workspace::FontSpec {
                                                family: fam_inc.clone(),
                                                size: (size0 + 1.).min(24.),
                                            },
                                        );
                                        cx.notify();
                                    });
                                }),
                        ),
                ),
        );
    }
    out = out.child(
        div()
            .text_size(px(11.))
            .text_color(rgb(t.text_dim))
            .child(tr("字体与字号即时生效并保存到 app_settings.json；会话字体作用于聊天气泡，面板字体为全局界面字体，Markdown 字体作用于正文渲染")),
    );
    out.into_any_element()
}

/// Cycle the curated family list (006 settings parity with zed's font
/// dropdown, trimmed to shipped-safe families).
fn cycle_family(choices: &'static [&'static str], ix: usize, dir: i32) -> &'static str {
    let n = choices.len() as i32;
    let next = ((ix as i32) + dir).rem_euclid(n);
    choices[next as usize]
}

