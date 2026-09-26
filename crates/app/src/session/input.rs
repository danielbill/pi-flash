//! inputPanel (031): the dedicated chat composer — editor base + toolbar
//! (model / thinking / tools pills, images, send) + slash/@ completion
//! menu. pi-web ChatInput parity. Free function over Chat state; entity
//! split lands in phase E (ARCHITECTURE.md §2).

use gpui::{Context, Entity, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::EditorInputElement;
use crate::PillMenu;
use crate::MenuKind;
use crate::i18n::tr;
use crate::services::workspace::{play_notify_sound, save_sound_pref};
use crate::theme::theme as T;
use crate::ui::icon;

pub(crate) fn input_area(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
    caret_on: bool,
    this_input: SharedString,
    model_label: SharedString,
    thinking_menu_open: bool,
    tools_menu_open: bool,
    thinking_label: SharedString,
    tools_label: &str,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    let t = T();
    let tools_label: SharedString = tools_label.to_string().into();
    let input_ph: SharedString = if streaming {
        tr("立即引导 / 排队后续消息...").into()
    } else if chat.input.is_empty() {
        tr("消息...输入 / 使用命令，输入 @ 查找文件").into()
    } else {
        chat.input.clone().into()
    };
    let input_empty = chat.input.is_empty();
    let can_queue = !input_empty || !chat.pending_images.is_empty();
            div()
                .px_4()
                .pb_2()
                .children(if chat.pending_images.is_empty() {
                    None
                } else {
                    let rows: Vec<gpui::AnyElement> = chat
                        .pending_images
                        .iter()
                        .enumerate()
                        .map(|(i, img)| {
                            let weak_i = weak.clone();
                            let name: SharedString = img.name.clone().into();
                            div()
                                .id(SharedString::from(format!("img-{i}")))
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .bg(rgb(t.bg_panel))
                                .border_1()
                                .border_color(rgb(t.border))
                                .text_xs()
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_i.update(cx, |c, cx| {
                                        if i < c.pending_images.len() {
                                            c.pending_images.remove(i);
                                        }
                                        cx.notify();
                                    });
                                })
                                .child(SharedString::from(format!(
                                    "\u{1f5bc} {name} \u{00d7}"
                                )))
                                .into_any_element()
                        })
                        .collect();
                    Some(
                        div()
                            .w_full()
                            .mb_1p5()
                            .flex()
                            .gap_2()
                            .children(rows)
                            .into_any_element(),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .rounded(px(14.))
                        .border_1()
                        .border_color(if streaming {
                            gpui::rgba(0xeab30866) // amber, pi-web streaming
                        } else if input_focused {
                            rgb(t.accent)
                        } else {
                            rgb(t.border)
                        })
                        .bg(rgb(t.bg))
                        .pl_3p5()
                        .pr_2p5()
                        .py_2p5()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("input")
                                .track_focus(&chat.focus)
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .rounded_md()
                                .on_key_down(cx.listener(
                                    |this, ev: &KeyDownEvent, _w, cx| {
                                        let key = ev.keystroke.key.as_str();
                                        let shift = ev.keystroke.modifiers.shift;
                                        // while the IME is composing, the
                                        // keyboard belongs to the IME — text
                                        // arrives via the InputHandler
                                        if this.ime_marked.is_some() {
                                            return;
                                        }
                                        let menu_open =
                                            this.active_menu().is_some();
                                        let items = this.menu_items();
                                        let streaming = this
                                            .state
                                            .as_ref()
                                            .is_some_and(|s| s.is_streaming);
                                        let can_queue = !this.input.is_empty()
                                            || !this.pending_images.is_empty();
                                        match key {
                                            "enter" if shift && streaming => {
                                                if can_queue {
                                                    this.follow_up_input(cx);
                                                }
                                            }
                                            "enter" if shift => {
                                                this.input.push('\n');
                                                cx.notify();
                                            }
                                            "enter"
                                                if menu_open && !items.is_empty() => {
                                                let ix = this
                                                    .menu_ix
                                                    .min(items.len() - 1);
                                                let insert =
                                                    items[ix].insert.clone();
                                                this.accept_menu(insert, cx);
                                            }
                                            "enter" if streaming => {
                                                if can_queue {
                                                    this.steer_input(cx);
                                                }
                                            }
                                            "enter" => this.send_input(cx),
                                            "escape" if streaming && !menu_open => {
                                                this.abort_stream(cx);
                                            }
                                            "escape" if menu_open => {
                                                this.menu_ix = 0;
                                                if this.active_menu()
                                                    == Some(MenuKind::At)
                                                {
                                                    if let Some(at) =
                                                        this.input.rfind('@')
                                                    {
                                                        let q = this.input
                                                            [at + 1..]
                                                            .to_string();
                                                        this.input = format!(
                                                            "{}{} ",
                                                            &this.input[..at],
                                                            q
                                                        );
                                                    }
                                                } else if !this.input.is_empty() {
                                                    this.input =
                                                        format!("{} ", this.input);
                                                }
                                                cx.notify();
                                            }
                                            "escape" => this.abort(cx),
                                            "tab" if menu_open && !items.is_empty() => {
                                                let ix = this
                                                    .menu_ix
                                                    .min(items.len() - 1);
                                                let insert =
                                                    items[ix].insert.clone();
                                                this.accept_menu(insert, cx);
                                            }
                                            "up" if menu_open && !items.is_empty() => {
                                                this.menu_ix =
                                                    this.menu_ix.saturating_sub(1);
                                                cx.notify();
                                            }
                                            "down"
                                                if menu_open && !items.is_empty() =>
                                            {
                                                this.menu_ix = (this.menu_ix + 1)
                                                    .min(items.len() - 1);
                                                cx.notify();
                                            }
                                            "up" if !this.history.is_empty() => {
                                                let ix = match this.history_ix {
                                                    None => this.history.len() - 1,
                                                    Some(i) => i.saturating_sub(1),
                                                };
                                                this.history_ix = Some(ix);
                                                this.input =
                                                    this.history[ix].clone();
                                                cx.notify();
                                            }
                                            "down" => {
                                                if let Some(i) = this.history_ix {
                                                    if i + 1 < this.history.len() {
                                                        this.history_ix = Some(i + 1);
                                                        this.input =
                                                            this.history[i + 1]
                                                                .clone();
                                                    } else {
                                                        this.history_ix = None;
                                                        this.input.clear();
                                                    }
                                                    cx.notify();
                                                }
                                            }
                                            "backspace" => {
                                                if !ev.keystroke.modifiers.modified()
                                                {
                                                    this.input.pop();
                                                    this.menu_ix = 0;
                                                    cx.notify();
                                                }
                                            }
                                            _ => {}
                                        }
                                    },
                                ))
                                .text_sm()
                                .child(
                                    // text + blinking caret (gpui editor is
                                    // hand-rolled; the caret marks the end)
                                    div()
                                        .relative()
                                        .flex()
                                        .items_center()
                                        .min_w_0()
                                        // caret is an absolute overlay so its
                                        // blinking never shifts the text
                                        .when(
                                            input_focused && caret_on && input_empty,
                                            |d| {
                                                d.child(
                                                    div()
                                                        .absolute()
                                                        .left_0()
                                                        .top(px(3.))
                                                        .w(px(1.5))
                                                        .h(px(16.))
                                                        .bg(rgb(t.accent)),
                                                )
                                            },
                                        )
                                        .when(input_empty, |d| {
                                            d.child(
                                                div()
                                                    .min_w_0()
                                                    .whitespace_nowrap()
                                                    .overflow_hidden()
                                                    .text_color(rgb(t.text_dim))
                                                    .opacity(0.55)
                                                    .child(SharedString::from(
                                                        input_ph.clone(),
                                                    )),
                                            )
                                        })
                                        .when(!input_empty, |d| {
                                            d.child(SharedString::from(
                                                this_input.clone(),
                                            ))
                                        })
                                        .when(
                                            input_focused && caret_on && !input_empty,
                                            |d| {
                                                d.child(
                                                    div()
                                                        .w(px(1.5))
                                                        .h(px(16.))
                                                        .flex_shrink_0()
                                                        .bg(rgb(t.accent)),
                                                )
                                            },
                                        ),
                                )
                                // paint-phase input handler: routes the
                                // Windows IME into the hand-rolled editor
                                .child(
                                    EditorInputElement::new(entity.clone(), chat.focus.clone())
                                        .absolute()
                                        .inset_0(),
                                ),
                        )
                        .child(
                            div()
                                .children(if streaming {
                                    // steer / follow-up pair (pi-web
                                    // ChatInput streaming mode)
                                    let weak_b = weak.clone();
                                    Some(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .child(
                                                div()
                                                    .id("steer")
                                                    .px_3()
                                                    .py_1p5()
                                                    .rounded_lg()
                                                    .border_1()
                                                    .border_color(gpui::rgba(
                                                        0xeab30800 | 0x35,
                                                    ))
                                                    .bg(gpui::rgba(0xeab30800 | 0x12))
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(gpui::rgb(0xb48200))
                                                    .cursor_pointer()
                                                    .when(!can_queue, |d| d.opacity(0.5))
                                                    .on_mouse_down(MouseButton::Left, {
                                                        let weak = weak_b.clone();
                                                        move |_, _, cx| {
                                                            let _ = weak.update(
                                                                cx,
                                                                |c, cx| {
                                                                    if can_queue {
                                                                        c.steer_input(cx)
                                                                    }
                                                                },
                                                            );
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id("followup")
                                                    .px_3()
                                                    .py_1p5()
                                                    .rounded_lg()
                                                    .border_1()
                                                    .border_color(gpui::rgba(
                                                        0x818cf400 | 0x35,
                                                    ))
                                                    .bg(gpui::rgba(0x818cf400 | 0x12))
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(gpui::rgb(0x6366f1))
                                                    .cursor_pointer()
                                                    .when(!can_queue, |d| d.opacity(0.5))
                                                    .on_mouse_down(MouseButton::Left, {
                                                        let weak = weak_b.clone();
                                                        move |_, _, cx| {
                                                            let _ = weak.update(
                                                                cx,
                                                                |c, cx| {
                                                                    if can_queue {
                                                                        c.follow_up_input(cx)
                                                                    }
                                                                },
                                                            );
                                                        }
                                                    }),
                                            ),
                                    )
                                } else {
                                    None
                                })
                                .child(if streaming {
                                    div().into_any_element()
                                } else {
                                    div()
                                        .id("send")
                                        .flex_shrink_0()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .px_3()
                                        .py_1p5()
                                        .rounded_lg()
                                        .bg(if can_queue {
                                            rgb(t.accent)
                                        } else {
                                            rgb(t.bg_panel)
                                        })
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(if can_queue {
                                            rgb(t.accent_contrast)
                                        } else {
                                            rgb(t.text_dim)
                                        })
                                        .cursor_pointer()
                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                this.send_input(cx);
                                            },
                                        ))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(icon("send", 12., t.text))
                                                .child(SharedString::from(tr("发送"))),
                                        )
                                        .into_any_element()
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .pt_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .text_xs()
                                .text_color(rgb(t.text_muted))
                                .child(
                                    div()
                                        .id("attach-image")
                                        .flex()
                                        .items_center()
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(t.text)))
                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                this.attach_images(cx);
                                            },
                                        ))
                                        .child(icon("image", 12., t.text_muted)),
                                )
                                .child(
                                    div()
                                        .id("open-model-select")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(t.text)))
                                        .on_mouse_down(MouseButton::Left, {
                                            let weak = weak.clone();
                                            move |_, _, cx| {
                                                let _ = weak.update(cx, |c, cx| {
                                                    if c.available_models.is_empty() {
                                                        c.refresh_state();
                                                    }
                                                    c.dialog =
                                                        Some(Chat::model_select_dialog(cx));
                                                    cx.notify();
                                                });
                                            }
                                        })
                                        .child(icon("settings", 12., t.text_muted))
                                        .child(model_label),
                                ),
                        )
                        .child(
                            div()
                                .relative()
                                .flex()
                                .items_center()
                                .gap_3()
                                .text_xs()
                                .text_color(rgb(t.text_muted))
                                // thinking level pill -> popup menu
                                // (pi-web ChatInput thinking dropdown)
                                .child(
                                    div()
                                        .id("thinking-menu")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px(px(4.))
                                        .py(px(3.))
                                        .rounded(px(5.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                this.pill_menu =
                                                    match this.pill_menu {
                                                        Some(PillMenu::Thinking) => None,
                                                        _ => Some(PillMenu::Thinking),
                                                    };
                                                cx.notify();
                                            },
                                        ))
                                        .child(icon(
                                            "lightbulb",
                                            12.,
                                            if thinking_menu_open { t.accent } else { t.text_muted },
                                        ))
                                        .child(thinking_label),
                                )
                                // tools preset pill -> popup menu
                                .child(
                                    div()
                                        .id("tools-menu")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px(px(4.))
                                        .py(px(3.))
                                        .rounded(px(5.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                this.pill_menu =
                                                    match this.pill_menu {
                                                        Some(PillMenu::Tools) => None,
                                                        _ => Some(PillMenu::Tools),
                                                    };
                                                cx.notify();
                                            },
                                        ))
                                        .child(icon(
                                            "wrench",
                                            12.,
                                            if tools_menu_open { t.accent } else { t.text_muted },
                                        ))
                                        .child(SharedString::from(tools_label)),
                                )
                                // compact (rpc compact)
                                .child(
                                    div()
                                        .id("compact")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px(px(4.))
                                        .py(px(3.))
                                        .rounded(px(5.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                this.compact_session(cx);
                                            },
                                        ))
                                        .child(icon("scissors", 12., t.text_muted))
                                        .child(SharedString::from(tr("压缩"))),
                                )
                                // 停止 (pi-web chat.stop; abort the run)
                                .when(streaming, |d| {
                                    d.child(
                                        div()
                                            .id("stop")
                                            .flex()
                                            .items_center()
                                            .gap_1p5()
                                            .px_2()
                                            .py(px(3.))
                                            .rounded(px(5.))
                                            .border_1()
                                            .border_color(gpui::rgba(0xef44444d))
                                            .bg(gpui::rgba(0xef444414))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(gpui::rgb(0xef4444))
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(gpui::rgba(0xef444429))
                                            })
                                            .on_mouse_down(MouseButton::Left, cx.listener(
                                                |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                    this.abort_stream(cx);
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .size(px(7.))
                                                    .rounded(px(1.5))
                                                    .bg(gpui::rgb(0xef4444)),
                                            )
                                            .child(SharedString::from(tr("停止"))),
                                    )
                                })
                                // notification sound toggle
                                .child(
                                    div()
                                        .id("sound")
                                        .flex()
                                        .items_center()
                                        .px(px(4.))
                                        .py(px(3.))
                                        .rounded(px(5.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                                this.sound_on = !this.sound_on;
                                                save_sound_pref(this.sound_on);
                                                if this.sound_on {
                                                    play_notify_sound();
                                                }
                                                cx.notify();
                                            },
                                        ))
                                        .child(icon(
                                            "volume",
                                            12.,
                                            if chat.sound_on { t.accent } else { t.text_muted },
                                        )),
                                )
                        )
                )
}
