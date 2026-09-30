//! sessionView (v54): 聊天列（消息列表 + 空会话 hero + 悬浮 composer）+
//! 会话导航比例尺（右侧 26px gutter，垂直居中 65% 高，≤10 节点）。无工具
//! 栏、无内嵌状态行（v54 按设计删除）。

pub(crate) mod input;
pub(crate) mod messages;
pub(crate) mod runtime;

use gpui::{Animation, AnimationExt, MouseButton, SharedString, div, list, prelude::*, px, relative, rgb};
use pi_link::protocol::Block;

use self::messages::{Role, compute_meta, render_assistant_turn, render_msg};
use crate::ext_ui::render_ext_widget;

use crate::Chat;
use crate::i18n::tr;
use crate::services::format::*;
use crate::theme::theme as T;

pub(crate) fn main_column(
    chat: &mut Chat,
    entity: gpui::Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> gpui::Div {
    let t = T();
    // all conversation state comes from the ACTIVE session runtime
    let rt = chat.rt();
    let streaming = rt.read(cx).state.as_ref().is_some_and(|st| st.is_streaming);
    let input_focused = chat.focus.is_focused(window);
    chat.input_focused = input_focused;
    let caret_on = chat.caret_on;
    let this_input: SharedString = chat.input.clone().into();
    let chat_entity = entity.clone();
    let rt_entity = rt.clone();
    let rt_list = rt.read(cx).list.clone();

    let sf = crate::appearance::session_font();

    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .relative()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text))
        .font_family(sf.family.clone())
        .child(
            div()
                .id("chat-and-nav")
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .id("chat-wrap")
                        .flex_1()
                        .min_w_0()
                        .relative()
                        .flex()
                        .flex_col()
                        // empty new-session hero (pi-web isEmptyNew)
                        .children(session_hero(chat, t, cx))
                        // message list（底部 135px 让位悬浮 composer）
                        .child(session_list(chat, chat_entity, weak.clone(), rt_list, rt_entity.clone(), t))
                        // 悬浮 composer：0 高 wrapper（不吞点击/滚轮），
                        // 胶囊绝对定位上浮叠在聊天区上
                        .children(ext_widget_rows(chat, t, true))
                        .child(input::input_area(
                            chat,
                            entity.clone(),
                            weak,
                            streaming,
                            input_focused,
                            caret_on,
                            this_input,
                            cx,
                        ))
                        .children(ext_widget_rows(chat, t, false)),
                )
                .child(nav_gutter(chat, rt_entity.clone(), weak.clone(), t, cx)),
        )
}

/// Message list + phase row (v54: 920px 列，行距 22)。
fn session_list(
    _chat: &mut Chat,
    chat_entity: gpui::Entity<Chat>,
    weak_for_msg: gpui::WeakEntity<Chat>,
    rt_list: gpui::ListState,
    rt_entity: gpui::Entity<crate::session::runtime::SessionRuntime>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    list(rt_list.clone(), move |ix, _window, cx| {
        let rt_view = rt_entity.read(cx);
        let chat = chat_entity.read(cx);
        let weak = weak_for_msg.clone();
        match rt_view.messages.get(ix) {
            Some(m) if m.role == Role::User => div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(920.))
                        .child(render_msg(
                            m,
                            ix,
                            &weak,
                            &rt_view.collapsed,
                            t,
                            None,
                            compute_meta(&rt_view.messages, ix),
                            rt_view.copy_flash.is_some_and(|(cix, at)| {
                                cix == ix && at.elapsed().as_millis() < 1500
                            }),
                        )),
                )
                .into_any_element(),
            Some(_) => {
                // agent 回复按轮聚合：轮头（首条或前一条非 assistant）渲染
                // 整块；轮内后续消息已含在轮块中，跳过
                let is_turn_head = ix == 0
                    || !matches!(
                        rt_view.messages.get(ix - 1).map(|p| &p.role),
                        Some(Role::Assistant)
                    );
                if !is_turn_head {
                    return div().into_any_element();
                }
                let end = rt_view.messages[ix..]
                    .iter()
                    .position(|mm| mm.role == Role::User)
                    .map(|off| ix + off)
                    .unwrap_or(rt_view.messages.len());
                let turn: Vec<&pi_link::protocol::Block> = Vec::new();
                let _ = turn;
                let turn_msgs: Vec<&crate::session::messages::Msg> =
                    rt_view.messages[ix..end].iter().collect();
                let turn_ixs: Vec<usize> = (ix..end).collect();
                // 流式中且本轮包含最后一条消息 → 工作中（隐藏折叠行）
                let streaming = rt_view.state.as_ref().is_some_and(|s| s.is_streaming);
                let stream_est = if streaming && end == rt_view.messages.len() {
                    rt_view.messages.last().map(|last| {
                        let text: String = last
                            .blocks
                            .iter()
                            .map(|b| match b {
                                Block::Text { text, .. }
                                | Block::Thinking { text, .. } => text.as_str(),
                                _ => "",
                            })
                            .collect();
                        estimate_tokens(&text)
                    })
                } else {
                    None
                };
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(920.))
                            .child(render_assistant_turn(
                                &turn_msgs,
                                &turn_ixs,
                                ix,
                                &weak,
                                &rt_view.collapsed,
                                t,
                                &rt_view.model_label_text(),
                                stream_est,
                                compute_meta(&rt_view.messages, ix),
                                rt_view.copy_flash.is_some_and(|(cix, at)| {
                                    cix == ix && at.elapsed().as_millis() < 1500
                                }),
                            )),
                    )
                    .into_any_element()
            }
            None => {
                // v54 §13 等待动画：spark 旋转 + 正在思考 + shimmer 滑动条
                if chat.rt().read(cx).phase_row_visible() {
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .max_w(px(920.))
                                .pt(px(2.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(9.))
                                        .text_size(px(13.))
                                        .text_color(rgb(t.accent))
                                        .child(spark_spin(14., t.accent))
                                        .child(SharedString::from(tr("正在思考"))),
                                )
                                .child(shimmer_bar(t)),
                        )
                        .into_any_element()
                } else {
                    div().w_full().into_any_element()
                }
            }
        }
    })
    .flex_1()
    .min_h_0()
    .pt(px(22.))
    .pr(px(30.))
    .pb(px(135.))
    .pl(px(34.))
    .into_any_element()
}

/// Empty new-session hero (pi-web ChatWindow isEmptyNew; v54 极简版).
fn session_hero(
    chat: &Chat,
    t: &'static crate::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> Option<gpui::AnyElement> {
    (chat.rt().read(cx).messages.is_empty()
        && !chat
            .rt()
            .read(cx)
            .state
            .as_ref()
            .is_some_and(|s| s.is_streaming))
        .then(|| {
            div()
                .w_full()
                .px(px(34.))
                .pt(px(22.))
                .child(
                    div()
                        .max_w(px(920.))
                        .mx_auto()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .font_family("Consolas")
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2p5()
                                .min_w_0()
                                .child(
                                    div()
                                        .size(px(32.))
                                        .rounded(px(8.))
                                        .bg(rgb(t.accent))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(20.))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(t.accent_contrast))
                                        .child("\u{3c0}"),
                                )
                                .child(
                                    div()
                                        .text_size(px(22.))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(t.text))
                                        .child("pi-flash"),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_end()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(t.text_muted))
                                        .child(SharedString::from(format!(
                                            "app v{}",
                                            env!("CARGO_PKG_VERSION")
                                        ))),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(t.text_muted))
                                        .child(SharedString::from(format!(
                                            "pi v{}",
                                            pi_link::vendor::vendored_version()
                                                .unwrap_or_default()
                                        ))),
                                ),
                        ),
                )
                .into_any_element()
        })
}

/// 会话导航比例尺 (v54 §12): 右缘 26px gutter，垂直居中 65% 高；≤10 灰点
/// 灰线分段、当前位 accent；悬停展开 326px 发言列表覆盖层（不挤压布局）。
fn nav_gutter(
    chat: &mut Chat,
    rt_entity: gpui::Entity<crate::session::runtime::SessionRuntime>,
    _weak: gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    // turns = 用户消息锚点（ChatMinimap.tsx parity：每轮=用户消息 + 其后
    // 的全部 assistant 回复）
    let msgs_len = rt_entity.read(cx).messages.len();
    let scroll_top_ix = rt_entity.read(cx).list.logical_scroll_top().item_ix;
    let turns: Vec<usize> = rt_entity
        .read(cx)
        .messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::User)
        .map(|(i, _)| i)
        .collect();
    // 每轮的 assistant 消息索引（flyout 的 A 行，各自可点击定位）
    let turn_assistants: Vec<Vec<usize>> = turns
        .iter()
        .enumerate()
        .map(|(ti, &ux)| {
            let end = turns.get(ti + 1).copied().unwrap_or(msgs_len);
            (ux + 1..end)
                .filter(|&i| {
                    rt_entity.read(cx).messages.get(i).is_some_and(|m| {
                        m.role == Role::Assistant
                    })
                })
                .collect()
        })
        .collect();
    // 当前激活节点 = 焦点线（视口顶部附近）之上最近的轮
    let active_turn: Option<usize> = turns.iter().rposition(|&ux| ux <= scroll_top_ix + 1);
    let dots: Vec<usize> = if turns.len() > 10 {
        turns[turns.len() - 10..].to_vec()
    } else {
        turns.clone()
    };
    let dot_turn_ix = |msg_ix: usize| -> Option<usize> {
        turns.iter().position(|&ux| ux == msg_ix)
    };

    let mut gutter = div()
        .id("nav-gutter")
        .relative()
        .w(px(26.))
        .h_full()
        .flex_shrink_0()
        // gutter hover：进入即开，离开且鼠标不在 flyout 上才进入宽限
        .on_hover(cx.listener(|this, h: &bool, _w, cx| {
            if *h {
                this.nav_open = true;
                this.nav_hide_at = None;
            } else if !this.nav_flyout_hovered {
                this.nav_hide_at = Some(std::time::Instant::now());
            }
            cx.notify();
        }));

    // 悬停展开的发言列表覆盖层（pi-web preview box：自身接管 hover，
    // 点击行定位到对应消息）。选择框/比例尺亮点跟随鼠标：轮级 hover
    // 写 nav_hover_turn，无 hover 时回落滚动位置激活
    let sel = chat.nav_hover_turn.or(active_turn);
    if chat.nav_open && !dots.is_empty() {
        let mut flyout = div()
            .id("nav-flyout")
            .absolute()
            .right(px(26.))
            .top_0()
            .bottom_0()
            .w(px(326.))
            .bg(rgb(t.bg))
            .border_l_1()
            .border_color(gpui::rgba(0xafc4ba80))
            .shadow_lg()
            .overflow_y_scroll()
            .py(px(8.))
            .on_hover(cx.listener(|this, h: &bool, _w, cx| {
                if *h {
                    this.nav_open = true;
                    this.nav_hide_at = None;
                    this.nav_flyout_hovered = true;
                } else {
                    this.nav_flyout_hovered = false;
                    this.nav_hover_turn = None;
                    this.nav_hide_at = Some(std::time::Instant::now());
                }
                cx.notify();
            }));
        for (n, &ux) in dots.iter().enumerate() {
            let ti = dot_turn_ix(ux).unwrap_or(usize::MAX);
            let on = sel == Some(ti);
            let user_text = rt_entity
                .read(cx)
                .messages
                .get(ux)
                .map(|m| m.plain_text())
                .unwrap_or_default();
            let chars: String = user_text.chars().take(160).collect();
            let no = format!("{:02}", n + 1);
            let rt_scroll = rt_entity.clone();
            let assistants = turn_assistants
                .get(dot_turn_ix(ux).unwrap_or(usize::MAX))
                .cloned()
                .unwrap_or_default();
            let mut turn_el = div()
                .id(SharedString::from(format!("nv-turn-{ux}")))
                .mx(px(6.))
                .mt(px(2.))
                .px(px(8.))
                .py(px(7.))
                // 选择框四面完整：常驻 1px 边框（透明↔accent 切换，无
                // 布局跳动），bg 同步切换
                .rounded(px(8.))
                .border_1()
                .border_color(gpui::rgba(0x00000000))
                .when(on, |d| {
                    d.bg(gpui::rgba((t.accent as u32) << 8 | 0x0f))
                        .border_color(rgb(t.accent))
                })
                // 选择框跟随鼠标：轮级 hover 写 nav_hover_turn
                .on_hover(cx.listener(move |this, h: &bool, _w, cx| {
                    let v = if *h { Some(ti) } else { None };
                    if this.nav_hover_turn != v {
                        this.nav_hover_turn = v;
                        cx.notify();
                    }
                }))
                .child(
                    // 用户行：点击定位到该用户消息
                    div()
                        .id(SharedString::from(format!("nv-user-{ux}")))
                        .flex()
                        .gap(px(9.))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let list = rt_scroll.read(cx).list.clone();
                            list.scroll_to_reveal_item(ux);
                        })
                        .child(
                            div()
                                .w(px(18.))
                                .text_right()
                                .font_family("Consolas")
                                .text_size(px(10.))
                                .line_height(relative(1.7))
                                .text_color(rgb(t.text_dim))
                                .child(SharedString::from(no)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.5))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .line_height(relative(1.55))
                                .text_color(rgb(t.text))
                                .max_h(px(58.))
                                .overflow_hidden()
                                .child(SharedString::from(chars)),
                        ),
                );
            // 摘要行：每轮只取第一条有正文的回复的首句（定位器，不是
            // 会话副本——用户定案：agent 说多少话也只摘要一行）
            if let Some(aix) = assistants.into_iter().find(|&i| {
                rt_entity
                    .read(cx)
                    .messages
                    .get(i)
                    .is_some_and(|m| !m.plain_text().trim().is_empty())
            }) {
                let preview = rt_entity
                    .read(cx)
                    .messages
                    .get(aix)
                    .map(|m| {
                        let text = m.plain_text();
                        let first = text.trim_start().lines().next().unwrap_or("");
                        let mut line: String =
                            first.chars().take(80).collect();
                        if line.is_empty() {
                            line.push_str("…");
                        }
                        line
                    })
                    .unwrap_or_default();
                let rt_scroll_a = rt_entity.clone();
                turn_el = turn_el.child(
                    div()
                        .id(SharedString::from(format!("nv-agent-{aix}")))
                        .flex()
                        .gap(px(9.))
                        .mt(px(5.))
                        .ml(px(27.))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let list = rt_scroll_a.read(cx).list.clone();
                            list.scroll_to_reveal_item(aix);
                        })
                        .child(
                            div()
                                .w(px(18.))
                                .text_right()
                                .text_size(px(10.))
                                .line_height(relative(1.8))
                                .text_color(rgb(t.text_dim))
                                .child("A"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.))
                                .line_height(relative(1.55))
                                .text_color(rgb(t.text_dim))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(preview)),
                        ),
                );
            }
            flyout = flyout.child(turn_el);
        }
        gutter = gutter.child(flyout);
    }

    // 比例尺轨道（垂直居中 65% 高；≥1 轮即渲染，pi-web 布局 parity）
    if !dots.is_empty() {
        let n = dots.len();
        let step = if n > 1 { 1. / (n as f32 - 1.) } else { 0. };
        let mut rail = div().relative().h(relative(0.65)).w_full();
        // 分段线（不穿点）
        if n >= 2 {
            for i in 0..n - 1 {
                let top = i as f32 * step;
                rail = rail.child(
                    div()
                        .absolute()
                        .left_1_2()
                        .top(relative(top + step * 0.18))
                        .h(relative(step * 0.64))
                        .w(px(1.))
                        .bg(gpui::rgba(0xafc4ba99)),
                );
            }
        }
        for (i, &ux) in dots.iter().enumerate() {
            let on = sel == Some(dot_turn_ix(ux).unwrap_or(usize::MAX));
            let rt_scroll = rt_entity.clone();
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("nav-node-{ux}")))
                    .absolute()
                    .left_1_2()
                    .when(on, |d| d.ml(px(-4.)).size(px(8.)))
                    .when(!on, |d| d.ml(px(-3.5)).size(px(7.)))
                    .top(relative(if n > 1 {
                        i as f32 * step
                    } else {
                        0.5
                    }))
                    .mt(px(-4.))
                    .rounded_full()
                    .bg(rgb(if on { t.accent } else { 0xa9beb5 }))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.accent)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let list = rt_scroll.read(cx).list.clone();
                        list.scroll_to_reveal_item(ux);
                    }),
            );
        }
        gutter = gutter.child(
            div()
                .flex()
                .h_full()
                .flex_col()
                .justify_center()
                .child(rail),
        );
    }
    gutter.into_any_element()
}

/// Extension setWidget rows above/below the composer (ext protocol surface).
fn ext_widget_rows(chat: &Chat, t: &'static crate::theme::Theme, above: bool) -> Option<gpui::AnyElement> {
    let rows: Vec<gpui::AnyElement> = chat
        .ext_widgets
        .iter()
        .filter(|(_, _, a)| *a == above)
        .map(|(_, lines, _)| render_ext_widget(lines, t))
        .collect();
    (!rows.is_empty()).then(|| {
        div()
            .px(px(34.))
            .pb(if above { px(8.) } else { px(4.) })
            .flex()
            .flex_col()
            .gap_1()
            .children(rows)
            .into_any_element()
    })
}

/// 旋转的四角星 spark（设计 §13 phase-row 图标，1.4s/圈）。
fn spark_spin(size: f32, color: u32) -> gpui::AnyElement {
    gpui::svg()
        .path(SharedString::from("icons/spark.svg"))
        .text_color(gpui::rgb(color))
        .size(gpui::px(size))
        .with_animation(
            "phase-spark",
            Animation::new(std::time::Duration::from_millis(1400)).repeat(),
            |el, delta| {
                el.with_transformation(gpui::Transformation::rotate(gpui::radians(
                    delta * std::f32::consts::TAU,
                )))
            },
        )
        .into_any_element()
}

/// shimmer 滑动条（260×10 圆角，高亮块左→右循环 1.6s）。
fn shimmer_bar(t: &'static crate::theme::Theme) -> gpui::AnyElement {
    div()
        .mt(px(9.))
        .w(px(260.))
        .h(px(10.))
        .rounded(px(5.))
        .bg(rgb(t.bg_hover))
        .overflow_hidden()
        .child(
            div()
                .size_full()
                .with_animation(
                    "phase-shimmer",
                    Animation::new(std::time::Duration::from_millis(1600)).repeat(),
                    move |track, delta| {
                        // 高亮块自左滑出、右滑入
                        let x = (delta * 360. - 100.).clamp(-100., 260.);
                        track.child(
                            div()
                                .ml(px(x))
                                .mt(px(1.))
                                .w(px(100.))
                                .h(px(8.))
                                .rounded(px(4.))
                                .bg(rgb(t.bg_selected)),
                        )
                    },
                ),
        )
        .into_any_element()
}
