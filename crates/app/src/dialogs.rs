//! Modal dialogs: ModelSelect / GitDiff / SessionSearch
//! (pi-web parity surfaces layered over the app root). Free function over
//! Chat state; entity split lands in phase E (ARCHITECTURE.md §2).

use std::path::PathBuf;

use gpui::{App, Div, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Dialog;
use crate::Chat;
use crate::TextInput;
use crate::i18n::tr;
use crate::services::format::time_ago;
use crate::theme;
use crate::ui::icon_hover;

pub(crate) fn render_dialogs(
    mut root: Div,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
    cx: &App,
) -> Div {
        // dialogs (bodies verbatim from the former inline section)
            if let Some(Dialog::ModelSelect { input: filter_input }) = chat.dialog.as_ref() {
                return render_model_select(root, chat, weak, filter_input, t, cx);
            }
            if let Some(Dialog::GitDiff { path, patch }) = chat.dialog.as_ref() {
                return render_git_diff(root, chat, weak, path, patch, t, cx);
            }
            if let Some(Dialog::SessionSearch { input }) = chat.dialog.as_ref() {
                root = root.child(render_session_search(chat, weak, input, t, cx));
            }
    root
}

/// ModelSelect dialog surface (extracted from render_dialogs).
fn render_model_select(mut root: Div, chat: &Chat, weak: &gpui::WeakEntity<Chat>, filter_input: &gpui::Entity<TextInput>, t: &theme::Theme, cx: &App) -> Div {
                let flt = filter_input.read(cx).value().to_lowercase();
                // enabledModels whitelist narrows the picker (pi-web /api/models
                // resolveVisibleModels parity)
                let picker_enabled = !chat.mc_state.all_enabled;
                let models = chat.rt().read(cx).available_models.clone();
                let rows: Vec<gpui::AnyElement> = models
                    .iter()
                    .filter(|m| {
                        if picker_enabled {
                            let r = format!("{}/{}", m.provider, m.id);
                            if !chat.mc_state.enabled.iter().any(|e| e == &r) {
                                return false;
                            }
                        }
                        flt.is_empty()
                            || m.id.to_lowercase().contains(&flt)
                            || m.name.to_lowercase().contains(&flt)
                            || m.provider.to_lowercase().contains(&flt)
                    })
                    .take(12)
                    .map(|m| {
                        let provider = m.provider.clone();
                        let id = m.id.clone();
                        let weak_row = weak.clone();
                        let label: SharedString =
                            format!("{} / {}", m.provider, m.label()).into();
                        let ctx: SharedString = m
                            .context_window
                            .map(|c| format!("{}k", c / 1000))
                            .unwrap_or_default()
                            .into();
                        div()
                            .id(SharedString::from(format!("model-{provider}-{id}")))
                            .w_full()
                            .px_3()
                            .py_1p5()
                            .cursor_pointer()
                            .rounded_md()
                            .hover(|s| s.bg(rgb(t.bg_selected)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let (p, mid) = (provider.clone(), id.clone());
                                let _ = weak_row.update(cx, |c, cx| c.rt().update(cx, |r, cx| r.select_model(p, mid, cx)));
                            })
                            .flex()
                            .justify_between()
                            .child(div().text_xs().text_color(rgb(t.text)).child(label))
                            .child(div().text_xs().text_color(rgb(t.text_dim)).child(ctx))
                            .into_any_element()
                    })
                    .collect();
                let list_panel = if rows.is_empty() {
                    div()
                        .py_2()
                        .text_xs()
                        .text_color(rgb(t.text_dim))
                        .child("no models match")
                        .into_any_element()
                } else {
                    div().flex().flex_col().gap_0p5().children(rows).into_any_element()
                };
                root = root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .bg(gpui::hsla(0., 0., 0., 0.35))
                        .track_focus(&chat.dialog_focus)
                        .on_key_down({
                            let weak = weak.clone();
                            move |ev: &KeyDownEvent, _w, cx| {
                                if ev.keystroke.key == "escape" {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(520.))
                                .max_h(px(560.))
                                .bg(rgb(t.bg_panel))
                                .border_1()
                                .border_color(rgb(t.border))
                                .rounded_lg()
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .shadow_lg()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(rgb(t.text))
                                                .child("select model"),
                                        )
                                        .child(
                                            div()
                                                .id("model-close")
                                                .px_2()
                                                .cursor_pointer()
                                                .text_color(rgb(t.text_muted))
                                                .hover(|s| s.text_color(rgb(t.text)))
                                                .on_mouse_down(MouseButton::Left, {
                                                    let weak = weak.clone();
                                                    move |_, _, cx| {
                                                        let _ = weak.update(cx, |c, cx| {
                                                            c.dialog = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(icon_hover("x", 12., t.text_muted)),
                                        ),
                                )
                                .child(filter_input.clone())
                                .child(list_panel),
                        ),
                );
    root
}


/// 013 sessionSearchDialog + sessionSearchResultView: query on top, results
/// grouped by session below; a row click switches sessions and reveals the
/// GitDiff dialog surface (extracted from render_dialogs).
fn render_git_diff(mut root: Div, chat: &Chat, weak: &gpui::WeakEntity<Chat>, path: &PathBuf, patch: &String, t: &theme::Theme, _cx: &App) -> Div {
                let path_text: SharedString = path.to_string_lossy().to_string().into();
                let mut body = patch.clone();
                if body.chars().count() > 60000 {
                    body = body.chars().take(60000).collect();
                    body.push_str("\n\n\u{2026} (truncated)");
                }
                root = root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .bg(gpui::hsla(0., 0., 0., 0.35))
                        .track_focus(&chat.dialog_focus)
                        .on_key_down({
                            let weak = weak.clone();
                            move |ev: &KeyDownEvent, _w, cx| {
                                if ev.keystroke.key == "escape" {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(760.))
                                .max_h(px(640.))
                                .bg(rgb(t.bg_panel))
                                .border_1()
                                .border_color(rgb(t.border))
                                .rounded(px(8.))
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .shadow_lg()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(icon_hover("git-branch", 12., t.accent))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_family("Consolas")
                                                        .text_color(rgb(t.text_muted))
                                                        .child(path_text),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("diff-close")
                                                .px_2()
                                                .cursor_pointer()
                                                .text_color(rgb(t.text_muted))
                                                .hover(|s| s.text_color(rgb(t.text)))
                                                .on_mouse_down(MouseButton::Left, {
                                                    let weak = weak.clone();
                                                    move |_, _, cx| {
                                                        let _ = weak.update(cx, |c, cx| {
                                                            c.dialog = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(icon_hover("x", 12., t.text_muted)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .max_h(px(520.))
                                        .overflow_hidden()
                                        .p_2()
                                        .rounded(px(6.))
                                        .bg(rgb(t.bg))
                                        .font_family("Consolas")
                                        .text_size(px(11.))
                                        .text_color(rgb(t.text))
                                        .child(SharedString::from(body)),
                                ),
                        ),
                );
    root
}


/// matched message (pi-web SessionSearch, dialog-mounted per 013).
fn render_session_search(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    t: &theme::Theme,
    _cx: &App,
) -> Div {
    let status: SharedString = if chat.search_running {
        tr("搜索中…").into()
    } else if chat.search_needle.is_empty() {
        tr("输入关键词，搜索当前项目的会话内容").into()
    } else if chat.search_hits.is_empty() {
        tr("没有匹配结果").into()
    } else {
        format!("{} 条结果", chat.search_hits.len()).into()
    };
    let weak_close = weak.clone();
    let mut results = div().flex().flex_col();
    let mut last_session: Option<PathBuf> = None;
    for (hit_ix, hit) in chat.search_hits.iter().enumerate() {
        if last_session.as_ref() != Some(&hit.session_path) {
            last_session = Some(hit.session_path.clone());
            let age = time_ago(hit.modified);
            let label: SharedString = hit
                .session_name
                .clone()
                .unwrap_or_else(|| {
                    if hit.preview.is_empty() {
                        hit.session_path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    } else {
                        hit.preview.clone()
                    }
                })
                .into();
            results = results.child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(SharedString::from(label))
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(age)),
                    ),
            );
        }
        let path = hit.session_path.clone();
        let ts = hit.ts;
        let weak_row = weak.clone();
        results = results.child(
            div()
                .id(SharedString::from(format!("hit-{hit_ix}")))
                .px_3()
                .py_1p5()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let path = path.clone();
                    let _ = weak_row.update(cx, |c, cx| c.jump_to_hit(path, ts, cx));
                })
                .child(
                    div()
                        .text_xs()
                        .line_height(relative(1.5))
                        .text_color(rgb(t.text_muted))
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .gap_1()
                        .child(SharedString::from(hit.before.clone()))
                        .child(
                            div()
                                .px_0p5()
                                .rounded(px(3.))
                                .bg(gpui::hsla(
                                    0., 0., 0.5, 0.15,
                                ))
                                .text_color(rgb(t.accent))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(SharedString::from(hit.match_text.clone())),
                        )
                        .child(SharedString::from(hit.after.clone())),
                ),
        );
    }
    div()
        .absolute()
        .inset_0()
        .occlude()
        .bg(gpui::hsla(0., 0., 0., 0.35))
        .track_focus(&chat.dialog_focus)
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, {
            let weak = weak_close.clone();
                move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dialog = None;
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .w(px(620.))
                    .max_h(px(640.))
                    .bg(rgb(t.bg_panel))
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(px(8.))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(input.clone())
                    .child({
                        let label: SharedString = if chat.search_truncated {
                            format!("{status} · {partial}", partial = tr("部分结果")).into()
                        } else {
                            status
                        };
                        div()
                            .px_1()
                            .text_xs()
                            .text_color(rgb(t.text_dim))
                            .child(label)
                    })
        .child(
            div()
                .id("search-results")
                .max_h(px(520.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(results),
        ),
    )
}
