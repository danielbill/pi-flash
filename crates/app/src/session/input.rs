//! composer (v54 一体式): 单容器 16px 圆角 1px 边框，宽 75%/min 500/max 920
//! （与消息列对齐）居中，悬浮胶囊上浮叠在聊天区上（0 高 wrapper 不吞点击/
//! 滚轮）。控件行：左 = 图片 + 工具预设「默认∨」；右 = 上下文用量环 + 模型∨
//! + 思考∨ + 圆形发送 ↑（运行中变停止）。无压缩/铃声/AI 按钮。

use gpui::{Context, Entity, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::EditorInputElement;
use crate::PillMenu;
use crate::MenuKind;
use crate::i18n::tr;
use crate::theme::theme as T;
use crate::ui::{icon, icon_hover};

pub(crate) fn input_area(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
    caret_on: bool,
    this_input: SharedString,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let input_ph: SharedString = if streaming {
        tr("立即引导 / 排队后续消息...").into()
    } else if chat.input.is_empty() {
        tr("/使用命令，shift回车换行").into()
    } else {
        chat.input.clone().into()
    };
    let input_empty = chat.input.is_empty();
    let can_queue = !input_empty || !chat.pending_images.is_empty();
    let (model_label, thinking_label, tools_label, ctx_pct) = {
        let r = chat.rt().read(cx);
        (
            r.state
                .as_ref()
                .and_then(|s| s.model_label())
                .unwrap_or_else(|| tr("选择模型").to_string()),
            r.state
                .as_ref()
                .and_then(|s| s.thinking_level.clone())
                .unwrap_or_else(|| "medium".to_string()),
            r.tool_preset_label(),
            r.stats.as_ref().and_then(|s| s.context_percent),
        )
    };

    // ---- 胶囊 ----
    let mut capsule = div()
        .id("composer")
        .w(gpui::relative(0.75))
        .max_w(px(920.)) // 与消息列同宽对齐（pi-web 单一内容列宽）
        .min_w(px(500.))
        .rounded(px(16.))
        .border_1()
        .border_color(if streaming {
            gpui::rgba(0xeab30866) // amber while streaming (pi-web parity)
        } else if input_focused {
            rgb(t.accent).into()
        } else {
            rgb(t.border).into()
        })
        .bg(rgb(t.bg))
        .shadow_lg()
        .flex()
        .flex_col()
        .pt(px(5.))
        .pb(px(3.));
    // 附加图片 chips
    if !chat.pending_images.is_empty() {
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
                    .child(SharedString::from(format!("\u{1f5bc} {name} \u{00d7}")))
                    .into_any_element()
            })
            .collect();
        capsule = capsule.child(
            div()
                .w_full()
                .pt(px(4.))
                .px(px(12.))
                .flex()
                .flex_wrap()
                .gap_2()
                .children(rows),
        );
    }
    // 编辑区（键处理 + IME 完整保留）
    capsule = capsule.child(
        div()
            .w_full()
            .min_h(px(59.))
            .px(px(12.))
            .pt(px(10.))
            .pb(px(2.))
            .text_size(px(13.5))
            .child(input_editor(
                chat,
                entity,
                streaming,
                input_focused,
                caret_on,
                this_input,
                input_empty,
                input_ph,
                t,
                cx,
            )),
    );
    // 控件行
    capsule = capsule.child(composer_bar(
        chat,
        streaming,
        can_queue,
        &model_label,
        &thinking_label,
        &tools_label,
        ctx_pct,
        t,
        cx,
    ));

    // 0 高 wrapper：胶囊绝对定位悬浮（聊天消息从胶囊后滚过）
    div()
        .id("composer-wrap")
        .relative()
        .h(px(0.))
        .flex_shrink_0()
        .child(
            div()
                .absolute()
                .bottom(px(20.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(capsule),
        )
        .into_any_element()
}

/// Editor surface (hand-rolled editor + caret; keys/IME unchanged).
fn input_editor(
    chat: &mut Chat,
    entity: gpui::Entity<Chat>,
    _streaming: bool,
    input_focused: bool,
    caret_on: bool,
    this_input: SharedString,
    input_empty: bool,
    input_ph: SharedString,
    t: &'static crate::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
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
                // while the IME is composing, the keyboard belongs to the IME
                if this.ime_marked.is_some() {
                    return;
                }
                let menu_open = this.active_menu().is_some();
                let items = this.menu_items(cx);
                let streaming = this
                    .rt()
                    .read(cx)
                    .state
                    .as_ref()
                    .is_some_and(|s| s.is_streaming);
                let can_queue = !this.input.is_empty() || !this.pending_images.is_empty();
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
                    "enter" if menu_open && !items.is_empty() => {
                        let ix = this.menu_ix.min(items.len() - 1);
                        let insert = items[ix].insert.clone();
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
                        if this.active_menu() == Some(MenuKind::At) {
                            if let Some(at) = this.input.rfind('@') {
                                let q = this.input[at + 1..].to_string();
                                this.input = format!("{}{} ", &this.input[..at], q);
                            }
                        } else if !this.input.is_empty() {
                            this.input = format!("{} ", this.input);
                        }
                        cx.notify();
                    }
                    "escape" => this.abort_stream(cx),
                    "tab" if menu_open && !items.is_empty() => {
                        let ix = this.menu_ix.min(items.len() - 1);
                        let insert = items[ix].insert.clone();
                        this.accept_menu(insert, cx);
                    }
                    "up" if menu_open && !items.is_empty() => {
                        this.menu_ix = this.menu_ix.saturating_sub(1);
                        cx.notify();
                    }
                    "down" if menu_open && !items.is_empty() => {
                        this.menu_ix = (this.menu_ix + 1).min(items.len() - 1);
                        cx.notify();
                    }
                    "up" if !this.history.is_empty() => {
                        let ix = match this.history_ix {
                            None => this.history.len() - 1,
                            Some(i) => i.saturating_sub(1),
                        };
                        this.history_ix = Some(ix);
                        this.input = this.history[ix].clone();
                        cx.notify();
                    }
                    "down" => {
                        if let Some(i) = this.history_ix {
                            if i + 1 < this.history.len() {
                                this.history_ix = Some(i + 1);
                                this.input = this.history[i + 1].clone();
                            } else {
                                this.history_ix = None;
                                this.input.clear();
                            }
                            cx.notify();
                        }
                    }
                    "backspace" => {
                        if !ev.keystroke.modifiers.modified() {
                            this.input.pop();
                            this.menu_ix = 0;
                            cx.notify();
                        }
                    }
                    // Ctrl/Cmd+V：追加剪贴板文本（无选区模型，光标恒在末尾；
                    // 修复输入框从未实现粘贴的功能缺失）
                    "v" if ev.keystroke.modifiers.control
                        || ev.keystroke.modifiers.platform =>
                    {
                        if let Some(text) = cx
                            .read_from_clipboard()
                            .and_then(|item| item.text())
                        {
                                                        // Windows clipboard newlines are CRLF;
                                                        // cosmic-text treats bare CR as undefined
                                                        // (fail-fast) -- normalize to LF
                                                        let text = text
                                                            .replace("\r\n", "\n")
                                                            .replace('\r', "\n");
                            if !text.is_empty() {
                                this.input.push_str(&text);
                                this.menu_ix = 0;
                                cx.notify();
                            }
                        }
                    }
                    _ => {}
                }
            },
        ))
        .child(
            // text + blinking caret (caret marks the end; absolute overlay so
            // blinking never shifts the text)
            div()
                .relative()
                .flex()
                .items_center()
                .min_w_0()
                .when(input_focused && caret_on && input_empty, |d| {
                    d.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(3.))
                            .w(px(1.5))
                            .h(px(16.))
                            .bg(rgb(t.accent)),
                    )
                })
                .when(input_empty, |d| {
                    d.child(
                        div()
                            .min_w_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(rgb(t.text_faint))
                            .child(SharedString::from(input_ph.clone())),
                    )
                })
                .when(!input_empty, |d| {
                    // 块级全宽容器：文本按宽自动折行、换行符生效（此前在
                    // flex 行里不折行，长文本/粘贴多行横向溢出被裁）；caret
                    // 用追加着色字符实现——随文本折行，永远紧跟末尾
                    let mut text = this_input.to_string();
                    let caret_on_now = input_focused && caret_on;
                    let caret_char = "\u{258f}";
                    // 高亮 range 以 UTF-8 字节计且必须落在字符边界上——
                    // range 只罩光标字符的首字节会把 str 切进多字节序列
                    // 中间，gpui 按高亮边界切片直接 panic（0xc0000409）
                    let caret_ix = text.len();
                    if caret_on_now {
                        text.push_str(caret_char);
                    }
                    let caret_end = text.len();
                    let base = gpui::TextStyle {
                        color: rgb(t.text).into(),
                        font_size: px(14.).into(),
                        ..Default::default()
                    };
                    let highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> =
                        if caret_on_now {
                            vec![(caret_ix..caret_end,
                                gpui::HighlightStyle {
                                    color: Some(rgb(t.accent).into()),
                                    ..Default::default()
                                },
                            )]
                        } else {
                            Vec::new()
                        };
                    d.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .child(
                                gpui::StyledText::new(text)
                                    .with_default_highlights(&base, highlights),
                            ),
                    )
                })
        )
        // paint-phase input handler: routes the Windows IME
        .child(
            EditorInputElement::new(entity.clone(), chat.focus.clone())
                .absolute()
                .inset_0(),
        )
        .into_any_element()
}

/// 控件行：左 = 图片 + 工具预设；右 = 上下文环 + 模型 + 思考 + 发送。
#[allow(clippy::too_many_arguments)]
fn composer_bar(
    chat: &mut Chat,
    streaming: bool,
    can_queue: bool,
    model_label: &str,
    thinking_label: &str,
    tools_label: &str,
    ctx_pct: Option<u64>,
    t: &'static crate::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    let thinking_open = chat.pill_menu == Some(PillMenu::Thinking);
    let tools_open = chat.pill_menu == Some(PillMenu::Tools);
    let mut bar = div()
        .flex()
        .items_center()
        .px(px(10.))
        .child(
            // 左侧：图片 + 工具预设（内容裸宽，组内间距 10px，与右侧一致）
            div().flex().items_center().gap(px(10.)).child(
            // 图片
            div()
                .id("attach-image")
                .flex()
                .items_center()
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        this.attach_images(cx);
                    },
                ))
                .child(icon_hover("image", 15., t.text_muted)),
            )
            .child(
            // 工具预设「默认∨」
            div()
                .id("tools-menu")
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(5.))
                .rounded(px(8.))
                .text_size(px(12.))
                .text_color(rgb(if tools_open { t.accent } else { t.text_muted }))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, event: &gpui::MouseDownEvent, _w, cx| {
                        this.pill_anchor = Some(event.position);
                        this.pill_menu = match this.pill_menu {
                            Some(PillMenu::Tools) => None,
                            _ => Some(PillMenu::Tools),
                        };
                        cx.notify();
                    },
                ))
                .child(icon_hover("wrench", 13., if tools_open { t.accent } else { t.text_muted }))
                .child(SharedString::from(tools_label.to_string()))
                .child(icon("chevron-down", 10., t.text_dim)),
            ),
        );
    // 右侧：环 + 模型 + 思考 + 发送，按钮间距统一 10px；操作栏左右
    // padding 10px = 发送钮距胶囊边框 10px
    let mut right = div().ml_auto().flex().items_center().gap(px(10.)).child(
        // 上下文用量环（25% 分桶）
        div()
            .id("ctx-ring")
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .child(
                // 同心双环：track 在下、进度弧在上（svg 是 flex 行内子元素
                // 会并排——必须各自绝对定位铺满后居中才叠成同心）
                div()
                    .relative()
                    .size(px(18.))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::ui::icon("ring-track", 18., t.bg_selected)),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::ui::icon(
                                match ctx_pct.unwrap_or(0) {
                                    0..=12 => "ring-track",
                                    13..=37 => "ring-25",
                                    38..=62 => "ring-50",
                                    63..=87 => "ring-75",
                                    _ => "ring-100",
                                },
                                18.,
                                t.accent,
                            )),
                    ),
            ),
    );
    // 模型 ∨
    right = right.child(
        div()
            .id("open-model-select")
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(8.))
            .text_size(px(12.))
            .text_color(rgb(t.text_muted))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, _: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).available_models.is_empty() {
                        this.refresh_state(cx);
                    }
                    this.dialog = Some(Chat::model_select_dialog(cx));
                    cx.notify();
                },
            ))
            .child(SharedString::from(model_label.to_string()))
            .child(icon("chevron-down", 10., t.text_dim)),
    );
    // 思考强度 ∨
    right = right.child(
        div()
            .id("thinking-menu")
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(8.))
            .text_size(px(12.))
            .text_color(rgb(if thinking_open { t.accent } else { t.text_muted }))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, event: &gpui::MouseDownEvent, _w, cx| {
                    this.pill_anchor = Some(event.position);
                    this.pill_menu = match this.pill_menu {
                        Some(PillMenu::Thinking) => None,
                        _ => Some(PillMenu::Thinking),
                    };
                    cx.notify();
                },
            ))
            .child(icon_hover("lightbulb", 13., if thinking_open { t.accent } else { t.text_muted }))
            .child(SharedString::from(thinking_label.to_string()))
            .child(icon("chevron-down", 10., t.text_dim)),
    );
    // 圆形发送 ↑（运行中变停止）；用户定位：左移 5px、上移 8px
    right = right.child(
        div()
            .id("send")
            .mr(px(5.))
            .mt(px(-8.))
            .size(px(28.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(rgb(if streaming {
                t.danger
            } else if can_queue {
                t.accent
            } else {
                t.bg_selected
            }))
            .hover(|s| s.opacity(0.9))
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, _: &gpui::MouseDownEvent, _w, cx| {
                    let streaming = this
                        .rt()
                        .read(cx)
                        .state
                        .as_ref()
                        .is_some_and(|s| s.is_streaming);
                    if streaming {
                        this.abort_stream(cx);
                    } else {
                        this.send_input(cx);
                    }
                },
            ))
            .child(if streaming {
                // 停止方块
                div()
                    .size(px(10.))
                    .rounded(px(1.5))
                    .bg(rgb(0xffffff))
                    .into_any_element()
            } else {
                icon("arrow-up", 15., t.accent_contrast)
            }),
    );
    bar = bar.child(right);
    bar
}
