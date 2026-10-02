//! composer (v57 输入组件化)：真输入框 = gpui-component InputState 多行
//! 模式（ui/composer_input.rs 门面）——点击定位光标、左右键移动、多行上下
//! 键、选区、IME、undo、超限纵向滚动条全为组件内置，不再手搓 String+绘制
//! 光标。单容器 16px 圆角 1px 边框，宽 75%/min 500/max 920（与消息列对齐）
//! 居中，悬浮胶囊上浮叠在聊天区上（0 高 wrapper 不吞点击/滚轮）。控件行：
//! 左 = 图片 + 工具预设「默认∨」；右 = 上下文用量环 + 模型∨ + 思考∨ +
//! 圆形发送 ↑ / 主题色停止块（运行中）。
//!
//! 按键路由（详见 composer_input.rs 头注）：
//! - Enter → 组件 PressEnter{secondary:false} → on_submit（菜单开=接受补全；
//!   运行中=引导；空闲=发送）
//! - Shift+Enter → wrapper on_key_down（运行中=排队 follow-up，空闲=换行）
//! - ↑/↓/Tab → "Input" 上下文覆盖绑定截获为 ComposerUp/Down/Tab（菜单导航
//!   /历史回溯/补全），非空多行重新派发组件 MoveUp/MoveDown
//! - Escape → wrapper on_key_down（菜单开=取消补全，运行中=中止）

use std::rc::Rc;

use gpui::{Context, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::ComposerInput;
use crate::PillMenu;
use crate::MenuKind;
use crate::{ComposerDown, ComposerTab, ComposerUp};
use crate::i18n::tr;
use crate::theme::theme as T;
use crate::ui::{icon, icon_hover};

/// 操作栏字体大小（工具预设/模型/思考统一；单点改这里）
const BAR_FONT: f32 = 15.;

pub(crate) fn input_area(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let can_queue = !chat.input.is_empty() || !chat.pending_images.is_empty();
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

    // 输入组件：惰性创建 + 每帧同步占位/值（set_value 同值跳过）
    let composer = ensure_composer(chat, weak, cx);
    let ph: SharedString = if streaming {
        tr("立即引导 / 排队后续消息...").into()
    } else {
        tr("/使用命令，shift回车换行").into()
    };
    composer.update(cx, |f, _| f.set_placeholder(Some(ph)));
    let cur = chat.input.clone();
    composer.update(cx, |f, fcx| f.set_value(cur, fcx));

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
        // 鼠标透传修复（psp_overlays 同款方案）：胶囊浮在聊天区上，指针处
        // 双层 hitbox 都命中，事件漏进底下的消息列表（悬停/滚轮/点击穿
        // 透）。occlude 让胶囊自身 bounds 遮挡先绘制的 hitbox；控件行子
        // 元素后绘制不受影响（occlusion 只作用于更早的 hitbox）。
        .occlude()
        .flex()
        .flex_col()
        .pt(px(10.))
        .pb(px(12.)); // 操作栏到下边框的距离
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
    // 编辑区：真输入组件（多行 AutoGrow，超出 max 出纵向滚动条）
    capsule = capsule.child(
        div()
            .w_full()
            .min_h(px(60.))
            .pt(px(2.))
            .pb(px(2.))
            .px(px(6.))
            .child(composer),
    );
    // 按键路由：组件处理的键（Enter/方向键/粘贴/undo…）到不了这里；
    // 这里只接组件无绑定的 Shift+Enter、已传播的 Escape，以及覆盖绑定
    // 截获的 ↑/↓/Tab 动作
    capsule = capsule
        .on_key_down(cx.listener(
            |this, ev: &KeyDownEvent, window, cx| {
                let key = ev.keystroke.key.as_str();                if key == "enter" && ev.keystroke.modifiers.shift {
                    // IME 组合期平台不派发按键（gpui windows events.rs），
                    // 无需组合守卫
                    let streaming = this.rt().read(cx).agent_running;
                    let can_queue =
                        !this.input.is_empty() || !this.pending_images.is_empty();
                    if streaming {
                        if can_queue {
                            this.follow_up_input(cx);
                        }
                    } else if let Some(c) = &this.composer {
                        c.update(cx, |f, fcx| f.insert_newline(window, fcx));
                    }
                    cx.stop_propagation();
                    return;
                }
                if key == "escape" {
                    // 走到这说明组件的 escape 已处理（解除 IME 标记且未清
                    // 草稿）并传播；这里做 app 层语义：菜单取消 / 中止
                    let menu = this.active_menu();
                    let streaming = this.rt().read(cx).agent_running;
                    if let Some(kind) = menu {
                        if kind == MenuKind::At {
                            if let Some(at) = this.input.rfind('@') {
                                let q = this.input[at + 1..].to_string();
                                let v = format!("{}{} ", &this.input[..at], q);
                                this.set_input(v, cx);
                            }
                        } else if !this.input.is_empty() {
                            let v = format!("{} ", this.input);
                            this.set_input(v, cx);
                        }
                        this.menu_ix = 0;
                        cx.notify();
                    } else if streaming {
                        this.abort_stream(cx);
                    }
                    cx.stop_propagation();
                }
            },
        ))
        .on_action(cx.listener(|this, _: &ComposerUp, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu().is_some() && !items.is_empty() {
                this.menu_ix = this.menu_ix.saturating_sub(1);
                cx.notify();
            } else if this.input.is_empty() && !this.history.is_empty() {
                let ix = match this.history_ix {
                    None => this.history.len() - 1,
                    Some(i) => i.saturating_sub(1),
                };
                this.history_ix = Some(ix);
                let v = this.history[ix].clone();
                this.set_input(v, cx);
            } else {
                // 非空多行：上移一行交还组件
                window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerDown, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu().is_some() && !items.is_empty() {
                this.menu_ix = (this.menu_ix + 1).min(items.len() - 1);
                cx.notify();
            } else if this.input.is_empty() {
                if let Some(i) = this.history_ix {
                    if i + 1 < this.history.len() {
                        this.history_ix = Some(i + 1);
                        let v = this.history[i + 1].clone();
                        this.set_input(v, cx);
                    } else {
                        this.history_ix = None;
                        this.set_input(String::new(), cx);
                    }
                }
            } else {
                window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerTab, _window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu().is_some() && !items.is_empty() {
                let ix = this.menu_ix.min(items.len() - 1);
                let insert = items[ix].insert.clone();
                this.accept_menu(insert, cx);
            }
        }));
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

/// 惰性创建 composer 输入组件（InputState 构造需要 &mut Window，挂在渲染
/// 期；回调经 weak 回写 Chat，不改持有结构）。
fn ensure_composer(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut Context<Chat>,
) -> gpui::Entity<ComposerInput> {
    if let Some(c) = &chat.composer {
        return c.clone();
    }
    let weak_change = weak.clone();
    let weak_submit = weak.clone();
    let c = cx.new(|icx| {
        let mut f = ComposerInput::new(icx);
        f.set_on_change(Rc::new(move |v, cx| {
            let _ = weak_change.update(cx, |chat, cx| {
                if chat.input != v {
                    chat.input = v.to_string();
                    chat.menu_ix = 0;
                    cx.notify();
                }
            });
        }));
        f.set_on_submit(Rc::new(move |v, cx| {
            let _ = weak_submit.update(cx, |chat, cx| {
                // 菜单开着：Enter=接受补全而非发送
                let items = chat.menu_items(cx);
                if chat.active_menu().is_some() && !items.is_empty() {
                    let ix = chat.menu_ix.min(items.len() - 1);
                    let insert = items[ix].insert.clone();
                    chat.accept_menu(insert, cx);
                    return;
                }
                chat.input = v.to_string();
                let can_queue = !chat.input.is_empty() || !chat.pending_images.is_empty();
                let streaming = chat.rt().read(cx).agent_running;
                if streaming {
                    if can_queue {
                        chat.steer_input(cx);
                    }
                } else if can_queue {
                    chat.send_input(cx);
                } else {
                    // 空输入回车：不发送，顺手清掉组件自插的 "\n"
                    chat.set_input(String::new(), cx);
                }
            });
        }));
        f
    });
    chat.composer = Some(c.clone());
    c
}

/// 控件行：左 = 图片 + 工具预设；右 = 上下文环 + 模型 + 思考 + 发送/停止。
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
                .pl(px(8.)) // 图片与正文首行左对齐：正文=编辑行6+组件12=18，图片=栏10+8=18
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
                .text_size(px(BAR_FONT))
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
            .text_size(px(BAR_FONT))
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
            .text_size(px(BAR_FONT))
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
    // 圆形发送 ↑（运行中变停止：主题色圆角方块+对比色停止块）；用户定位：
    // 左移 5px、上移 8px
    right = right.child(
        div()
            .id("send")
            .mr(px(2.)) // 右边距=栏padding10+2=12（与下边距对齐）
            .mt(px(-8.))
            .size(px(36.)) // 发送/停止共用直径（用户微调处）
            .rounded(if streaming { px(9.) } else { px(18.) })
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(rgb(if streaming {
                t.accent // 停止态外圈=主题色（用户定稿，非深色）
            } else if can_queue {
                t.accent
            } else {
                t.bg_selected
            }))
            .hover(|s| s.opacity(0.9))
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, _: &gpui::MouseDownEvent, _w, cx| {
                    // 与按钮渲染同源：agent_running（事件驱动），快照
                    // is_streaming 恒 false 会把"停止"点成"发送"
                    if this.rt().read(cx).agent_running {
                        this.abort_stream(cx);
                    } else {
                        this.send_input(cx);
                    }
                },
            ))
            .child(if streaming {
                // 停止方块
                div()
                    .size(px(12.))
                    .rounded(px(2.5))
                    .bg(rgb(t.accent_contrast))
                    .into_any_element()
            } else {
                icon("arrow-up", 15., t.accent_contrast)
            }),
    );
    bar = bar.child(right);
    bar
}
