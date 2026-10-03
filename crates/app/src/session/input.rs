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

use gpui::{AnimationExt, Context, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::ComposerInput;
use crate::PillMenu;
use crate::MenuKind;
use crate::{ComposerDown, ComposerTab, ComposerUp, ComposerPaste};
use crate::i18n::tr;
use crate::services::format::fmt_thousand;
use crate::theme::theme as T;
use crate::ui::{icon, icon_hover};

/// 操作栏字体大小（工具预设/模型/思考统一；单点改这里）
const BAR_FONT: f32 = 15.;

/// 悬停标志更新 + 淡出状态机（乱序免疫）：只按 (was_open, open) 转移，
/// 环→面板交接时两个 hover 事件无论先后都收敛到正确状态。双面全空才
/// 开始淡出（200ms 定时器到期清除，守卫时刻戳防旧定时器误杀新一轮）。
fn ctx_tip_hover(
    chat: &mut Chat,
    ring: bool,
    entered: bool,
    cx: &mut Context<Chat>,
) {
    let was_open = chat.ctx_tip_ring_hover || chat.ctx_tip_panel_hover;
    if ring {
        chat.ctx_tip_ring_hover = entered;
    } else {
        chat.ctx_tip_panel_hover = entered;
    }
    let open = chat.ctx_tip_ring_hover || chat.ctx_tip_panel_hover;
    match (was_open, open) {
        (false, true) => {
            chat.ctx_tip_closing = None;
            cx.notify();
        }
        (true, false) => {
            let t = std::time::Instant::now();
            chat.ctx_tip_closing = Some(t);
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let _ = this.update(cx, |c, cx| {
                    if c.ctx_tip_closing == Some(t) {
                        c.ctx_tip_closing = None;
                        cx.notify();
                    }
                });
            })
            .detach();
            cx.notify();
        }
        _ => {}
    }
}

/// 悬浮详情浮层（挂在 ctx-ring 容器内，absolute 越界浮在聊天区上）：
/// Open 常显；Closing 包一层 160ms 透明度淡出，定时器随后卸载。wrapper
/// 下方 12px 隐形尾迹压住环顶 2px，环→面板悬停交接无死区。
fn ctx_tip_element(
    chat: &mut Chat,
    t: &'static crate::theme::Theme,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let visible =
        chat.ctx_tip_ring_hover || chat.ctx_tip_panel_hover || chat.ctx_tip_closing.is_some();
    if !visible {
        return None;
    }
    let stats = chat.rt().read(cx).stats.clone();
    let closing = chat.ctx_tip_closing.is_some();
    let wrap = div()
        .id("ctx-tip")
        .absolute()
        .bottom(px(24.)) // 环命中区 26px，压顶 2px 防接缝死区
        .right(px(0.))
        .w(px(320.))
        .pb(px(12.))
        .flex()
        .flex_col()
        .on_hover(cx.listener(|this, h: &bool, _w, cx| ctx_tip_hover(this, false, *h, cx)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(ctx_usage_panel(stats, t));
    let el = if closing {
        wrap.with_animation(
            "ctx-tip-out",
            gpui::Animation::new(std::time::Duration::from_millis(160)),
            |el, delta| el.opacity(1. - delta),
        )
        .into_any_element()
    } else {
        wrap.into_any_element()
    };
    // vendored gpui 的 Style::paint 把边框画在子元素之后——面板挂在胶囊
    // 内部会被胶囊聚焦边框压线（实测横穿面板）；deferred 推迟到整帧末尾
    // 绘制（布局仍在原位，锚定/hover 关系不变，弹层同机制）
    Some(gpui::deferred(el).into_any_element())
}

/// 用量详情面板本体（pi-web session-info-popover 简化版——上下文比例 /
/// token 分项 / 费用 / 缓存命中率）。
fn ctx_usage_panel(
    stats: Option<pi_link::protocol::SessionStats>,
    t: &'static crate::theme::Theme,
) -> gpui::Div {
    let compact = |n: u64| -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.)
        } else if n >= 1000 {
            format!("{}k", n / 1000)
        } else {
            n.to_string()
        }
    };
    let row = |label: &str, value: String| -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(label.to_string())),
            )
            .child(
                div()
                    .ml_auto()
                    .text_size(px(14.))
                    .font_family("Consolas")
                    .text_color(rgb(t.text))
                    .child(SharedString::from(value)),
            )
            .into_any_element()
    };
    let section = |title: &str, rows: Vec<gpui::AnyElement>| -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(title.to_string())),
            )
            .children(rows)
            .into_any_element()
    };

    let body: Vec<gpui::AnyElement> = match stats {
        None => vec![
            div()
                .text_size(px(14.))
                .text_color(rgb(t.text_muted))
                .child(tr("会话尚未产生用量。"))
                .into_any_element(),
        ],
        Some(s) => {
            let ctx_rows = vec![
                row(
                    tr("使用比例"),
                    match (s.context_percent, s.context_window) {
                        (Some(p), Some(w)) => format!("{p:.1}% / {}", compact(w)),
                        (Some(p), None) => format!("{p:.1}%"),
                        (None, Some(w)) => format!("? / {}", compact(w)),
                        (None, None) => "\u{2014}".into(),
                    },
                ),
                row(
                    tr("上下文 token"),
                    s.context_tokens
                        .map(fmt_thousand)
                        .unwrap_or_else(|| "\u{2014}".into()),
                ),
            ];
            let token_rows = vec![
                row(tr("输入"), fmt_thousand(s.input)),
                row(tr("输出"), fmt_thousand(s.output)),
                row(tr("缓存读"), fmt_thousand(s.cache_read)),
                row(tr("缓存写"), fmt_thousand(s.cache_write)),
                row(tr("总计"), fmt_thousand(s.tokens_total)),
            ];
            let mut cost_rows = vec![row(tr("累计费用"), format!("${:.4}", s.cost))];
            if let Some(r) = s.cache_hit_rate() {
                cost_rows.push(row(tr("缓存命中率"), format!("{:.1}%", r * 100.)));
            }
            vec![
                section(tr("上下文"), ctx_rows),
                section(tr("Token 累计"), token_rows),
                section(tr("费用"), cost_rows),
            ]
        }
    };

    div()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .children(body)
}

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
    let names: Vec<String> =
        chat.rt().read(cx).commands.iter().map(|c| c.name.clone()).collect();
    composer.update(cx, |f, fcx| f.set_command_names(names, fcx));

    // ---- 胶囊 ----
    let mut capsule = div()
        .id("composer")
        .w_full() // 尺寸由外层锚点统一定（75%/min500/max920），/ 菜单与胶囊同宽
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
    // 附加图片缩略图（v58 图片粘贴）：56×56 圆角方块，panel 左上角并列、
    // 间隔 5px；右上角悬浮 16px 圆形 X 点击删除（pi-web ImagePreview 布局
    // parity：X 偏移 -4,-4 半嵌在缩略图角上）
    if !chat.pending_images.is_empty() {
        let thumbs: Vec<gpui::AnyElement> = chat
            .pending_images
            .iter()
            .enumerate()
            .map(|(i, img)| {
                let weak_i = weak.clone();
                let weak_open = weak.clone();
                let thumb = img.thumb.clone();
                let thumb_for_open = thumb.clone();
                div()
                    .id(SharedString::from(format!("img-thumb-{i}")))
                    .relative()
                    .size(px(56.))
                    .flex_shrink_0()
                    .child(
                        // 点击看大图（X 是 sibling 不在祖先链，两 handler 互不误触）
                        div()
                            .id(SharedString::from(format!("img-view-{i}")))
                            .size_full()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg_panel))
                            .overflow_hidden()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let Some(image) = thumb_for_open.clone() else { return; };
                                let _ = weak_open.update(cx, |c, cx| {
                                    c.dialog = Some(crate::Dialog::ImagePreview { image });
                                    cx.notify();
                                });
                            })
                            .child(match thumb {
                                Some(image) => gpui::img(image)
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Cover)
                                    .into_any_element(),
                                None => div().into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("img-del-{i}")))
                            .absolute()
                            .top(px(-4.))
                            .right(px(-4.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(rgb(t.bg_panel))
                            .border_1()
                            .border_color(rgb(t.border))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                // X 与图片盒的 hitbox 都含点击点（X 叠在其角
                                // 上），且二者非祖先——不拦截会继续派发到图
                                // 片盒的"开大图"handler（实测删除前弹预览）
                                cx.stop_propagation();
                                let _ = weak_i.update(cx, |c, cx| {
                                    if i < c.pending_images.len() {
                                        c.pending_images.remove(i);
                                    }
                                    cx.notify();
                                });
                            })
                            .child(icon("x", 8., t.text_muted)),
                    )
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
                .gap(px(5.))
                .children(thumbs),
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
                this.menu_scroll.scroll_to_item(this.menu_ix);
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
                this.menu_scroll.scroll_to_item(this.menu_ix);
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
        }))
        // Ctrl+V 截获（bind_keys 覆盖组件粘贴绑定）：剪贴板含图 → 附件化；
        // 否则重派发组件 Paste 走原文本粘贴
        .on_action(cx.listener(|this, _: &ComposerPaste, window, cx| {
            if !this.attach_clipboard_image(cx) {
                window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
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

    // 0 高 wrapper：胶囊绝对定位悬浮（聊天消息从胶囊后滚过）；/ 菜单
    // 挂在胶囊正上方（pi-web：bottom 100% + 8px 间隙）
    let slash_open = chat.active_menu() == Some(MenuKind::Slash);
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
                .child(
                    div()
                        .relative()
                        .w(gpui::relative(0.75))
                        .max_w(px(920.)) // 与消息列同宽对齐（pi-web 单一内容列宽）
                        .min_w(px(500.))
                        .when(slash_open, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom(gpui::relative(1.))
                                    .pb(px(8.))
                                    .child(crate::slash_menu_view(chat, weak, t, cx)),
                            )
                        })
                        .child(capsule),
                ),
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
    let weak_chip = weak.clone();
    let c = cx.new(|icx| {
        let mut f = ComposerInput::new(icx);
        f.set_on_change(Rc::new(move |v, cx| {
            let _ = weak_change.update(cx, |chat, cx| {
                if chat.input != v {
                    chat.input = v.to_string();
                    chat.menu_ix = 0;
                    // 过滤集变了回顶部，避免新列表停留在旧滚动深处
                    chat.menu_scroll
                        .set_offset(gpui::point(px(0.), px(0.)));
                    cx.notify();
                }
            });
        }));
        f.set_on_submit(Rc::new(move |v, cx| {
            let _ = weak_submit.update(cx, |chat, cx| {
                // 先落到剥离换行后的 v 再判菜单：组件 enter() 会先插一个
                // 换行并经 Change 污染 chat.input（含空白使菜单判定失败，
                // 回车被误当发送）
                chat.input = v.to_string();
                // chip 激活（命令已确认成胶囊）时 Enter=发送——裸 "/命令"
                // 的输入形态与菜单过滤态相同，必须以 chip 态区分，否则
                // Enter 会走"接受补全"永远发不出去
                let chip = chat
                    .composer
                    .as_ref()
                    .map(|c| c.read(cx).chip_active())
                    .unwrap_or(false);
                let items = chat.menu_items(cx);
                if !chip && chat.active_menu().is_some() && !items.is_empty() {
                    let ix = chat.menu_ix.min(items.len() - 1);
                    let insert = items[ix].insert.clone();
                    chat.accept_menu(insert, cx);
                    return;
                }
                let can_queue = !chat.input.is_empty() || !chat.pending_images.is_empty();
                let streaming = chat.rt().read(cx).agent_running;
                if streaming {
                    if can_queue {
                        chat.steer_input(cx);
                    }
                } else if can_queue {
                    chat.send_input(cx);
                } else {
                    // 空输入回车：不发送，顺手清掉组件自插的换行
                    chat.set_input(String::new(), cx);
                }
            });
        }));
        f.set_on_chip_backspace(Rc::new(move |cx| {
            let _ = weak_chip.update(cx, |chat, cx| {
                chat.set_input(String::new(), cx);
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
    ctx_pct: Option<f64>,
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
    let ctx_tip = ctx_tip_element(chat, t, cx);
    let mut right = div().ml_auto().flex().items_center().gap(px(10.)).child(
        // 上下文用量环：track 在下、实际比例的主题色弧在上（弧 SVG 按百分比
        // 运行时生成，icons/ring-p{1-99}；100% 走静态 ring-100）。悬浮显示
        // 用量详情，挪开淡出（ctx_tip_* 状态机）
        div()
            .id("ctx-ring")
            .relative()
            .p(px(4.))
            .m(px(-4.)) // 命中区扩到 26px，视觉位置不变
            .cursor_pointer()
            .rounded_full()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_hover(cx.listener(|this, h: &bool, _w, cx| ctx_tip_hover(this, true, *h, cx)))
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
                    .children(ctx_pct.map(|p| {
                        let path = if p >= 100. {
                            "icons/ring-100.svg".to_string()
                        } else {
                            format!("icons/ring-p{}.svg", p.round().clamp(1., 99.) as u8)
                        };
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui::svg()
                                    .path(SharedString::from(path))
                                    .text_color(rgb(t.accent))
                                    .size(px(18.)),
                            )
                    }))
                    // 悬浮详情浮层（absolute 越界，浮层随 hover/淡出态挂载）
                    .children(ctx_tip),
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
