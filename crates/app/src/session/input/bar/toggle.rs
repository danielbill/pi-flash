//! 状态带开关（status_bar_toggle）：插件勾选常驻/隐藏管理菜单。

use super::*;

/// 034 会话状态带显示开关：MCP 胶囊右侧（badge-info ICON_SM + SW_SM 24×14）。
/// 关 = 状态带整条不渲染（数据面照旧维护）；状态直切无动画。
pub(crate) fn status_bar_toggle(
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let on = crate::services::workspace::session_status_bar_on();
    let color = if on { t.text_muted } else { t.text_faint };
    let weak = cx.entity().downgrade();
    div()
        .id("ext-status-toggle")
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(6.)) // SP3：图标↔控件
        .px(px(6.))
        .with_tip("footerbar.tip.toggle")
        // 图标 ICON_SM 14（§3 行内/按钮内默认档；与同排胶囊的 13–14 视觉一致）
        .child(icon("badge-info", 14., color))
        // 紧凑档开关（§5.3 SW_SM）：28px 行高里的行内件，MD 会几乎撑满整行
        .child(crate::ui::kit::switch_base_sm(
            "ext-status-switch",
            on,
            false,
            move |_on, cx| {
                let _ = weak.update(cx, |c, cx| c.toggle_session_status_bar(cx));
            },
        ))
        .into_any_element()
}

/// 控件行：左 = 图片 + 工具预设；右 = 上下文环 + 模型 + 思考 + 发送/停止。
#[allow(clippy::too_many_arguments)]
pub(crate) fn composer_bar(
    chat: &mut Chat,
    streaming: bool,
    can_queue: bool,
    // 压缩锁：整行控件禁用（图片/工具预设/模型/思考/发送）
    locked: bool,
    model_label: &str,
    thinking_label: &str,
    tools_label: &str,
    ctx_pct: Option<f64>,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    // `!` bash 执行中与模型流共用停止按钮形态（031）
    let bash_running = chat.rt().read(cx).bash_running;
    let running = streaming || bash_running;
    // `!` shell 模式（与 input_area 边框同判定）：左组控件整组换成提示
    let bash_mode = chat.composer.pending_images.is_empty()
        && chat.composer.input.trim_start().starts_with('!');
    let bash_excluded = chat.composer.input.trim_start().starts_with("!!");
    let thinking_open = chat.dlg.pill_menu == Some(PillMenu::Thinking);
    let tools_open = chat.dlg.pill_menu == Some(PillMenu::Tools);
    // 工具预设 = spawn 参数（--tools/--no-tools），换它要重绑会话进程 +
    // 整表重读：运行中换会把当前这一轮掐掉、并让「等待模型响应」掉回屏底。
    // 所以 agent 跑动期间置灰禁止（思考强度/模型都是实时 RPC，不动消息表，照旧可用）。
    let tools_locked = locked || streaming;
    let mut bar = div()
        .flex()
        .items_center()
        .px(px(10.))
        .child(bar_left_group(
            chat,
            locked,
            tools_locked,
            tools_open,
            tools_label,
            bash_mode,
            bash_excluded,
            t,
            cx,
        ));
    // 右侧：环 + 模型 + 思考 + 发送，按钮间距统一 15px（2026-10-13 用户
    // 定夺，与左组一致）；操作栏左右 padding 10px = 发送钮距胶囊边框 10px
    let ctx_tip = ctx_tip_element(chat, t, cx);
    let mut right = div().ml_auto().flex().items_center().gap(px(15.)).child(
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
            .text_size(crate::appearance::ui_size(BAR_FONT))
            .text_color(rgb(if locked { t.text_faint } else { t.text_muted }))
            .when(!locked, |d| d.cursor_pointer())
            .hover(move |s| {
                if locked {
                    s
                } else {
                    s.bg(rgb(t.bg_hover)).text_color(rgb(t.text))
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    // 042：模型菜单锚定本按钮（原居中 ModelSelect 弹窗退役）；
                    // 目录缺失时 ensure_models_requested 借同 cwd 活进程补拉，
                    // 草稿无进程也照常弹出（列表来自 Chat 共享目录）
                    this.toggle_model_menu_at(event.position, cx);
                }),
            )
            .child(SharedString::from(model_label.to_string()))
            .child(icon(
                "chevron-down",
                10.,
                if locked { t.text_faint } else { t.text_dim },
            ))
            // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
            .child(crate::PillBtns::tracker(&chat.dlg.pill_btn.model)),
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
            .text_size(crate::appearance::ui_size(BAR_FONT))
            .text_color(rgb(if locked {
                t.text_faint
            } else if thinking_open {
                t.accent
            } else {
                t.text_muted
            }))
            .when(!locked, |d| d.cursor_pointer())
            .hover(move |s| {
                if locked {
                    s
                } else {
                    s.bg(rgb(t.bg_hover)).text_color(rgb(t.text))
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    this.dlg.pill_anchor = Some(crate::PillBtns::anchor(
                        &this.dlg.pill_btn.thinking,
                        event.position,
                    ));
                    this.dlg.pill_menu = match this.dlg.pill_menu {
                        Some(PillMenu::Thinking) => None,
                        _ => Some(PillMenu::Thinking),
                    };
                    cx.notify();
                }),
            )
            .child(icon_hover(
                "lightbulb",
                13.,
                if locked {
                    t.text_faint
                } else if thinking_open {
                    t.accent
                } else {
                    t.text_muted
                },
            ))
            .child(SharedString::from(thinking_label.to_string()))
            .child(icon(
                "chevron-down",
                10.,
                if locked { t.text_faint } else { t.text_dim },
            ))
            // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
            .child(crate::PillBtns::tracker(&chat.dlg.pill_btn.thinking)),
    );
    // 圆形发送 ↑（运行中变停止：主题色圆角方块+对比色停止块）；用户定位：
    // 左移 5px、上移 8px
    right = right.child(
        div()
            .id("send")
            .mr(px(2.)) // 右边距=栏padding10+2=12（与下边距对齐）
            .mt(px(-8.))
            .size(px(36.)) // 发送/停止共用直径（用户微调处）
            .rounded(if running { px(9.) } else { px(18.) })
            .flex()
            .items_center()
            .justify_center()
            .when(!locked, |d| d.cursor_pointer())
            .bg(rgb(if locked {
                // 压缩锁：恒灰，不给"可点"的暗示
                t.bg_selected
            } else if running {
                t.accent // 停止态外圈=主题色（用户定稿，非深色）
            } else if can_queue {
                t.accent
            } else {
                t.bg_selected
            }))
            .hover(move |s| if locked { s } else { s.opacity(0.9) })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    // 与按钮渲染同源：agent_running（事件驱动），快照
                    // is_streaming 恒 false 会把"停止"点成"发送"；
                    // `!` bash 执行中点它 = abort_bash
                    let bash_running = this.rt().read(cx).bash_running;
                    if bash_running {
                        this.abort_bash(cx);
                    } else if this.rt().read(cx).agent_running {
                        this.abort_stream(cx);
                    } else {
                        this.send_input(cx);
                    }
                }),
            )
            .child(if running {
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

