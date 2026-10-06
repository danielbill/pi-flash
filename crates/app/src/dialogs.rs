//! Modal dialogs: ModelSelect / GitDiff / SessionSearch / ImagePreview
//! (pi-web parity surfaces layered over the app root). Free function over
//! Chat state; entity split lands in phase E (ARCHITECTURE.md §2).

use std::path::PathBuf;

use gpui::{App, Div, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Dialog;
use crate::Chat;
use crate::TextInput;
use crate::{ComposerDown, ComposerUp, MODEL_PICKER_ROWS};
use crate::i18n::tr;
use crate::services::format::time_ago;
use crate::theme;
use crate::ui::icon_hover;

/// 弹窗公共外壳 = `ui::overlay::layer`（遮挡/外点关闭/ESC 关闭三条全局规则
/// 的唯一实现）+ 居中排布 + 卡片停传播。参数 `chat` 只用来取浮层焦点。
fn dialog_shell(chat: &Chat, weak: &gpui::WeakEntity<Chat>, panel: Div) -> Div {
    let weak_bg = weak.clone();
    crate::ui::overlay::layer(
        true,
        Some(&chat.dialog_focus),
        move |_w, cx| {
            let _ = weak_bg.update(cx, |c, cx| {
                c.dialog = None;
                cx.notify();
            });
        },
    )
    .flex()
    .items_center()
    .justify_center()
    .child(crate::ui::overlay::stop_click(panel))
}

pub(crate) fn render_dialogs(
    mut root: Div,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
    cx: &App,
) -> Div {
        // dialogs mount as a CHILD of the chat root — on top of the content,
        // never replacing it (replacing the root blanks the whole UI behind
        // the dialog; settings parity = content stays visible beneath)
            if let Some(Dialog::ModelSelect { input: filter_input, .. }) = chat.dialog.as_ref() {
                root = root.child(render_model_select(chat, weak, filter_input, t, cx));
            }
            if let Some(Dialog::GitDiff { path, patch }) = chat.dialog.as_ref() {
                root = root.child(render_git_diff(chat, weak, path, patch, t, cx));
            }
            if let Some(Dialog::SessionSearch { input }) = chat.dialog.as_ref() {
                root = root.child(render_session_search(chat, weak, input, t, cx));
            }
            if let Some(Dialog::ImagePreview { image }) = chat.dialog.as_ref() {
                root = root.child(render_image_preview(chat, weak, image, t));
            }
            if let Some(Dialog::SessionInfo { kind }) = chat.dialog.as_ref() {
                root = root.child(crate::top_panels::session_info_dialog(
                    *kind,
                    chat,
                    weak,
                    t,
                    cx,
                ));
            }
    root
}

/// ModelSelect dialog surface (extracted from render_dialogs).
fn render_model_select(chat: &Chat, weak: &gpui::WeakEntity<Chat>, filter_input: &gpui::Entity<TextInput>, t: &theme::Theme, cx: &App) -> Div {
                let sel = match &chat.dialog {
                    Some(Dialog::ModelSelect { sel, .. }) => *sel,
                    _ => 0,
                };
                let models = chat.filtered_models(cx);
                let rows: Vec<gpui::AnyElement> = models
                    .iter()
                    .enumerate()
                    .take(MODEL_PICKER_ROWS)
                    .map(|(ix, m)| {
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
                            .when(ix == sel, |d| d.bg(rgb(t.bg_selected)))
                            .hover(|s| s.bg(rgb(t.bg_selected)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let (p, mid) = (provider.clone(), id.clone());
                                let _ = weak_row.update(cx, |c, cx| {
                                    c.rt().update(cx, |r, cx| r.select_model(p, mid, cx));
                                    // picking is also the dismissal gesture
                                    c.dialog = None;
                                    cx.notify();
                                });
                            })
                            .flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(14.))
                                    .text_color(rgb(t.text))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(14.))
                                    .text_color(rgb(t.text_dim))
                                    .child(ctx),
                            )
                            .into_any_element()
                    })
                    .collect();
                let list_panel = if rows.is_empty() {
                    div()
                        .py_2()
                        .text_size(crate::appearance::ui_size(14.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("no models match"))
                        .into_any_element()
                } else {
                    div().flex().flex_col().gap_0p5().children(rows).into_any_element()
                };
                let panel = div()
                    .w(px(620.))
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
                                    .child(tr("选择模型")),
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
                    .child(list_panel);
                // Enter: caught at the overlay as a bubbled key event — the
                // same layer the ESC and ↑/↓ handling lives on (all three
                // proven paths; the filter input's PressEnter subscription
                // chain did not fire reliably). InputState::enter propagates
                // the keystroke in single-line mode, so the event reaches
                // this handler.
                dialog_shell(chat, weak, panel)
                    .on_key_down({
                        let weak = weak.clone();
                        move |ev: &KeyDownEvent, _w, cx| {
                            if ev.keystroke.key != "enter" {
                                return;
                            }
                            cx.stop_propagation();
                            // applying drops the dispatching entities
                            // (dialog = None) — defer out of the dispatch
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                let _ = weak.update(cx, |c, cx| c.apply_model_sel(cx));
                            });
                        }
                    })
                    .on_action({
                        let weak = weak.clone();
                        move |_: &ComposerUp, _w, cx| {
                            let _ = weak.update(cx, |c, cx| c.move_model_sel(-1, cx));
                        }
                    })
                    .on_action({
                        let weak = weak.clone();
                        move |_: &ComposerDown, _w, cx| {
                            let _ = weak.update(cx, |c, cx| c.move_model_sel(1, cx));
                        }
                    })
}


/// 013 sessionSearchDialog + sessionSearchResultView: query on top, results
/// grouped by session below; a row click switches sessions and reveals the
/// GitDiff dialog surface (extracted from render_dialogs).
fn render_git_diff(chat: &Chat, weak: &gpui::WeakEntity<Chat>, path: &PathBuf, patch: &String, t: &theme::Theme, _cx: &App) -> Div {
                let path_text: SharedString = path.to_string_lossy().to_string().into();
                let mut body = patch.clone();
                if body.chars().count() > 60000 {
                    body = body.chars().take(60000).collect();
                    body.push_str("\n\n\u{2026} (truncated)");
                }
                let panel = div()
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
                                                        .font_family(crate::markdown::MONO_FAMILY)
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
                                        .font_family(crate::markdown::MONO_FAMILY)
                                        .text_size(crate::appearance::ui_size(11.))
                                        .text_color(rgb(t.text))
                                        .child(SharedString::from(body)),
                                );
                dialog_shell(chat, weak, panel)
}


/// composer 缩略图点击大图预览（v58）：dialog_shell 金标准外壳（点外关闭
/// /ESC/遮挡），图片居中按 max 限宽高等比缩放（messages.rs 结果图同款，
/// img 尊重 max 约束）；无头部控件——ESC 或点击弹窗外任意处关闭。
fn render_image_preview(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    image: &std::sync::Arc<gpui::Image>,
    t: &theme::Theme,
) -> Div {
    let weak_close = weak.clone();
    // 图片预览也要有看得见的关闭按钮（浮层规则 5）：× 绝对定位在卡片右上角
    let close = crate::ui::overlay::close_btn("image-preview-close", t, move |_w, cx| {
        let _ = weak_close.update(cx, |c, cx| {
            c.dialog = None;
            cx.notify();
        });
    });
    let panel = div()
        .relative()
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded_lg()
        .p_2()
        .shadow_lg()
        .child(
            gpui::img(image.clone())
                .max_w(px(1040.))
                .max_h(px(680.))
                .rounded(px(6.)),
        )
        .child(div().absolute().top(px(6.)).right(px(6.)).child(close));
    dialog_shell(chat, weak, panel)
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
                            .text_size(crate::appearance::ui_size(10.))
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
    let panel = div()
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
        );
    dialog_shell(chat, weak, panel)
}
