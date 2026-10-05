//! sessionView (v54): 聊天列（消息列表 + 空会话 hero + 悬浮 composer）+
//! 会话导航面板（033：右缘 12px 刻度条，75% 高，悬停左弹 400px 轮次摘要
//! 卡片列表）。无工具栏、无内嵌状态行（v54 按设计删除）。

pub(crate) mod chat_list;
pub(crate) mod diff;
pub(crate) mod input;
pub(crate) mod messages;
pub(crate) mod runtime;

use gpui::{Animation, AnimationExt, MouseButton, SharedString, div, list, prelude::*, px, relative, rgb};
use pi_link::protocol::Block;

use self::messages::{Role, compute_meta, render_assistant_turn, render_custom_msg, render_msg};
use crate::ext_ui::render_ext_widget;

use crate::Chat;
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
    // 滚屏补账（chat_list.rs）：滚动事件回调与列表布局期严禁触碰
    // ListState（RefCell 冲突即崩），垫片的延迟卸载/结算借这里的下一帧
    // 渲染补 sync——page_turn 后消息测完即 settle 精确高度交还胶水；滚轮
    // 退役锚点后就地卸垫片
    let pager_pending = rt.read(cx).pager.take_frame_sync();
    if pager_pending {
        rt.update(cx, |r, cx| r.notify_list(cx));
    }
    // composer 的流式态用事件驱动的 agent_running（AgentStart/End 实时更新），
    // 不用 get_state 快照的 is_streaming——发消息后无人重拉快照，它恒 false
    // 导致 stop 按钮永远不出现
    let streaming = rt.read(cx).agent_running;
    // composer 焦点态来自输入组件（chat.focus 仅是组件创建前的回退）
    let input_focused = chat
        .composer
        .as_ref()
        .map(|c| c.read(cx).focus_handle_in(cx).is_focused(window))
        .unwrap_or(false);
    chat.input_focused = input_focused;
    let chat_entity = entity.clone();
    let rt_entity = rt.clone();
    let rt_list = rt.read(cx).pager.state();

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
                            weak,
                            streaming,
                            input_focused,
                            cx,
                        ))
                        .children(ext_widget_rows(chat, t, false)),
                )
                .child(nav_gutter(chat, rt_entity.clone(), weak.clone(), t, cx)),
        )
}

/// 回到最新消息（pi-web chat-scroll-to-bottom parity）：32px 圆钮，由
/// composer 悬浮容器以 gap 20px 叠在输入面板顶部上方（随面板增高上移，
/// 永不叠进面板）；常显 0.28 透明、hover 全亮；点击滚到末条并置位贴底。
pub(crate) fn scroll_to_bottom_button(
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    let weak = weak.clone();
    div()
        .id("scroll-to-bottom")
        .flex_shrink_0()
        .size(px(32.))
        .rounded_full()
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        // pi-web: 常态 opacity .28，hover/focus 全亮
        .opacity(0.28)
        .hover(|s| s.opacity(1.))
        // pi-web box-shadow: 0 2px 8px text 16%
        .shadow(vec![gpui::BoxShadow {
            color: gpui::rgba((t.text << 8) | 0x29).into(),
            offset: gpui::point(px(0.), px(2.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
        }])
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.rt().update(cx, |r, _| r.scroll_to_bottom());
                cx.notify();
            });
        })
        .child(crate::ui::icon("arrow-down", 14., t.text_muted))
        .into_any_element()
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
    // 水平边距必须挂外层容器：gpui List 的 prepaint 只应用 padding.top/
    // bottom（item_origin = bounds.origin + (0, padding.top)），左右 padding
    // 对条目完全无效——此前 pl34/pr30「看起来不存在」即此因
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        // 默认两侧边距 10px（窄窗口下内容不贴边；920 列宽不动，宽窗居中）
        .px(px(10.))
        .child(
            list(rt_list.clone(), move |ix, _window, cx| {
        let rt_view = rt_entity.read(cx);
        let chat = chat_entity.read(cx);
        let weak = weak_for_msg.clone();
        match rt_view.messages.get(ix) {
            Some(m) if m.role == Role::Custom => div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(920.))
                        .child(render_custom_msg(m, ix, t, &rt_view.collapsed, &weak)),
                )
                .into_any_element(),
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
                            &chat.expanded_skills,
                            &chat.bubble_scrolls,
                            &rt_view.collapsed,
                            t,
                            None,
                            compute_meta(&rt_view.messages, ix),
                            chat.bar_hover == Some(ix),
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
                    .position(|mm| matches!(mm.role, Role::User | Role::Custom))
                    .map(|off| ix + off)
                    .unwrap_or(rt_view.messages.len());
                let turn: Vec<&pi_link::protocol::Block> = Vec::new();
                let _ = turn;
                let turn_msgs: Vec<&crate::session::messages::Msg> =
                    rt_view.messages[ix..end].iter().collect();
                let turn_ixs: Vec<usize> = (ix..end).collect();
                // 流式中且本轮包含最后一条消息 → 工作中（思考/工具组默认展开，
                // 模型行显示 ↓token 估算 + t/s 徽章）。
                // 必须用事件驱动的 agent_running：get_state 快照的 is_streaming
                // 一轮内没人重拉 → 恒 false，会让「工作详情」组按「已有最终回答」
                // 默认折叠（思考框看不见）、↓token / t/s 徽章永不出现——composer
                // 早为此改用 agent_running（见 main_column 顶部注释），消息区漏改。
                let streaming = rt_view.agent_running;
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
                // v56-5 c23（pi-web tps badge parity）：启动 0.5s 后显示
                let stream_tps = stream_est.zip(rt_view.stream_started).and_then(|(est, start)| {
                    let secs = start.elapsed().as_secs_f32();
                    (secs >= 0.5 && est > 0).then(|| est as f32 / secs)
                });
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
                                stream_tps,
                                compute_meta(&rt_view.messages, ix),
                                chat.bar_hover == Some(ix),
                            )),
                    )
                    .into_any_element()
            }
            None => {
                // 条目序 [msgs | phase 行 | spacer]（chat_list::sync 的
                // splice 编排与之对齐）。spacer = 翻页垫片（pi-web
                // PromptAnchorSpacer）：把「跟随位」垫到用户消息顶。
                // phase 行 = 等待脉冲（文本流式一经出现即让位）
                let content = rt_view.messages.len()
                    + usize::from(rt_view.phase_row_visible());
                if rt_view.pager.anchor_active() && ix == content {
                    div()
                        .w_full()
                        .h(px(rt_view.pager.spacer_px()))
                        .into_any_element()
                } else if chat.rt().read(cx).phase_row_visible() {
                    let label = chat.rt().read(cx).phase_label();
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .max_w(px(920.))
                                .pt(px(2.))
                                .child(phase_pulse(label, t)),
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
    // 上下内边距必须与 chat_list 的钉顶/垫片几何一致（PAD_TOP/PAD_BOTTOM），
    // 改这里等于改滚屏数学
    .pt(px(chat_list::PAD_TOP))
    .pb(px(chat_list::PAD_BOTTOM)),
        )
        .into_any_element()
}

/// Empty new-session hero (pi-web ChatWindow isEmptyNew; v54 极简版).
fn session_hero(
    chat: &Chat,
    t: &'static crate::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> Option<gpui::AnyElement> {
    (chat.rt().read(cx).messages.is_empty()
        && !chat.rt().read(cx).agent_running)
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
                        .font_family(crate::markdown::MONO_FAMILY)
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
                                        .text_size(crate::appearance::ui_size(20.))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(rgb(t.accent_contrast))
                                        .child("\u{3c0}"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::appearance::ui_size(22.))
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
                                        .text_size(crate::appearance::ui_size(11.))
                                        .text_color(rgb(t.text_muted))
                                        .child(SharedString::from(format!(
                                            "app v{}",
                                            env!("CARGO_PKG_VERSION")
                                        ))),
                                )
                                .child(
                                    div()
                                        .text_size(crate::appearance::ui_size(11.))
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

/// 会话导航面板 (033)：右缘 12px 刻度条，屏高 75%，按「屏高 − composer
/// 高度×50%」居中；每轮一枚 2px 高 × 10px 宽刻度（左右 padding 4、间隔
/// 4px，新增刻度整体重新居中），当前定位轮 accent 12px 全宽——选中跟随
/// 真实滚动位置（贴底 = 末刻度），卡片悬浮框不回写。悬停向左弹出 400px
/// 轮次摘要卡片列表（用户前 50 字 + agent 首条回复前 50 字，编号 01-99），
/// 点击卡片/刻度定位会话；内容溢出时右缘显示滚动条。
fn nav_gutter(
    chat: &mut Chat,
    rt_entity: gpui::Entity<crate::session::runtime::SessionRuntime>,
    _weak: gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    // 摘要数据每帧在这里一次性算完并落地成 owned 值（旧实现散落在
    // 各 listener 之间反复 read），借用在块尾即结束，后面纯拼元素
    let (turns, turn_user, turn_agent, scroll_top_ix): (
        Vec<usize>,
        Vec<String>,
        Vec<Option<(usize, String)>>,
        usize,
    ) = {
        let rt = rt_entity.read(cx);
        // turns = 用户消息锚点（ChatMinimap.tsx parity：每轮=用户消息 +
        // 其后的全部 assistant 回复）
        let turns: Vec<usize> = rt
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == Role::User)
            .map(|(i, _)| i)
            .collect();
        // 每轮摘要（033：用户发言前 50 字 + agent 首条有正文回复前 50 字
        // 的首行；定位器不是会话副本——agent 说多少话也只摘要一行）
        let turn_user: Vec<String> = turns
            .iter()
            .map(|&ux| {
                rt.messages
                    .get(ux)
                    .map(|m| m.plain_text_prefix(50))
                    .unwrap_or_default()
            })
            .collect();
        let turn_agent: Vec<Option<(usize, String)>> = turns
            .iter()
            .enumerate()
            .map(|(ti, &ux)| {
                let end = turns.get(ti + 1).copied().unwrap_or(rt.messages.len());
                (ux + 1..end).find_map(|i| {
                    let m = rt.messages.get(i)?;
                    if m.role != Role::Assistant {
                        return None;
                    }
                    let head = m.plain_text_prefix(80);
                    let line: String = head
                        .trim_start()
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(50)
                        .collect();
                    (!line.is_empty()).then_some((i, line))
                })
            })
            .collect();
        let scroll_top_ix = rt.pager.scroll_top_ix();
        (turns, turn_user, turn_agent, scroll_top_ix)
    };
    // 刻度选中 = 真实定位轮次（033）：视口顶之上最近的轮。贴底（含未
    // 滚动的短会话）时逻辑位是 None，scroll_top_ix 回退成总条目数 →
    // 自然选中末轮（对话常态在末轮）。**不能**用 pager.is_at_bottom：
    // 那是滚轮回调维护的 Cell，程序化 reveal（scroll_to_reveal_item）
    // 不经过它——导航点击后它仍是 true，刻度会错停在末轮（v63-1 踩坑）
    let active_turn: Option<usize> = turns.iter().rposition(|&ux| ux <= scroll_top_ix + 1);
    // 卡片选中框 = 纯悬浮光标，随意滑动不联动刻度
    let card_sel = chat.nav_hover_turn;

    let mut gutter = div()
        .id("nav-gutter")
        .relative()
        .w(px(12.))
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

    // 悬停展开的轮次摘要列表（033：topbar 之下 100% 高、350px 宽、向左
    // 弹出、内容溢出显示滚动条）。滚动条挂在滚动容器平级的 absolute
    // 兄弟上（滚动容器内的绝对定位子元素会随内容滚走）
    if chat.nav_open && !turns.is_empty() {
        // absolute 定位本身即绝对定位子孙（滚动条）的包含块，无需再挂
        // relative——且 position 是单字段，relative 会覆盖 absolute
        let mut flyout_wrap = div()
            .absolute()
            .right(px(12.))
            .top_0()
            .bottom_0()
            .w(px(400.));
        let mut flyout = div()
            .id("nav-flyout")
            .w_full()
            .h_full()
            .occlude()
            .overflow_y_scroll()
            .track_scroll(&chat.nav_flyout_scroll)
            .bg(rgb(t.bg))
            .border_l_1()
            .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x80)))
            .shadow_lg()
            // pr 多留 14px 给右缘滚动条（滚动条占 right 3..11）
            .pl(px(6.))
            .pr(px(14.))
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
        for (ti, &ux) in turns.iter().enumerate() {
            let on = card_sel == Some(ti);
            let no = format!("{:02}", ti + 1);
            let chars = turn_user[ti].clone();
            let rt_scroll = rt_entity.clone();
            let mut turn_el = div()
                .id(SharedString::from(format!("nv-turn-{ux}")))
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
                // 选择框跟随鼠标：只在进入时置位，离开不清。gpui 鼠标
                // 事件 bubble 阶段按绘制逆序派发，向下移动时后画的卡的
                // leave 在下一张卡的 enter 之后触发，把 Some 清回 None
                // ——这正是「向上流畅、向下几乎不显示」的根因；最终清
                // 理由 flyout 的 on_hover(false) 统一做
                .on_hover(cx.listener(move |this, h: &bool, _w, cx| {
                    if *h && this.nav_hover_turn != Some(ti) {
                        this.nav_hover_turn = Some(ti);
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
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            rt_scroll.read(cx).pager.nav_goto(ux);
                            // 程序化定位不触发任何 notify，点刻度/卡片后
                            // 当帧重渲染，选中刻度才跟得上定位
                            window.refresh();
                        })
                        .child(
                            div()
                                .w(px(18.))
                                .text_right()
                                .font_family(crate::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(10.))
                                .line_height(relative(1.7))
                                .text_color(rgb(t.text_dim))
                                .child(SharedString::from(no)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                // 会话导航面板：用户发言 = 面板字号 -1
                                // （字体大小设置.md §1）
                                .text_size(crate::appearance::ui_size(11.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .line_height(relative(1.55))
                                .text_color(rgb(t.text))
                                .max_h(px(58.))
                                .overflow_hidden()
                                .child(SharedString::from(chars)),
                        ),
                );
            // 摘要行：A 与轮次编号对齐、正文与用户发言对齐（033）——
            // 与用户行同构（18px 右对齐编号列 + gap 9），此前多出的
            // ml(27) 缩进即「agent 回复没左对齐」的根因
            if let Some((aix, preview)) = turn_agent[ti].as_ref() {
                let (aix, preview) = (*aix, preview.clone());
                let rt_scroll_a = rt_entity.clone();
                turn_el = turn_el.child(
                    div()
                        .id(SharedString::from(format!("nv-agent-{aix}")))
                        .flex()
                        .gap(px(9.))
                        .mt(px(5.))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            rt_scroll_a.read(cx).pager.nav_goto(aix);
                            window.refresh();
                        })
                        .child(
                            div()
                                .w(px(18.))
                                .text_right()
                                .text_size(crate::appearance::ui_size(10.))
                                .line_height(relative(1.7))
                                .text_color(rgb(t.text_dim))
                                .child("A"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                // 会话导航面板：agent 发言 = 面板字号 -2
                                .text_size(crate::appearance::ui_size(10.))
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
        flyout_wrap = flyout_wrap.child(flyout);
        // 滚动条仅在实际溢出时渲染（max_offset>0 = 内容超高）
        if chat.nav_flyout_scroll.max_offset().height > px(0.) {
            let sb_state = gpui_component::scroll::ScrollbarState::default();
            flyout_wrap = flyout_wrap.child(
                div()
                    .absolute()
                    .top(px(8.))
                    .bottom(px(8.))
                    .right(px(3.))
                    .w(px(8.))
                    .child(
                        gpui_component::scroll::Scrollbar::vertical(
                            &sb_state,
                            &chat.nav_flyout_scroll,
                        ),
                    ),
            );
        }
        gutter = gutter.child(flyout_wrap);
    }

    // 导航刻度条（033）：屏高 75%；居中规则 = 屏高 − inputpanel 高度×50%
    // 后居中——rail 挂 mb(0.5×composer高)，flex 连 margin 盒一起居中，
    // 视觉中心正好上移 composer/4。行 = 10px 宽刻度 + 左右 padding 4
    // （18×6 命中区），间隔 4px；新增刻度 justify_center 每帧重新居中。
    // 极多轮次超出 75% 高时居中裁剪（>100 轮的加载/分页策略 033 待讨论）
    if !turns.is_empty() {
        let composer_h = chat.composer_h.get();
        let mut rail = div()
            .h(relative(0.75))
            .mb(px(composer_h * 0.5))
            .w_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .justify_center();
        for (ti, &ux) in turns.iter().enumerate() {
            let on = active_turn == Some(ti);
            let rt_scroll = rt_entity.clone();
            let tick_group = SharedString::from(format!("tick-{ux}"));
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("nav-node-{ux}")))
                    .group(tick_group.clone())
                    .w(px(18.))
                    .h(px(6.))
                    .flex()
                    .items_start()
                    .justify_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        rt_scroll.read(cx).pager.nav_goto(ux);
                        window.refresh();
                    })
                    .child(
                        div()
                            .h(px(2.))
                            .rounded(px(1.))
                            .when(on, |d| d.w(px(12.)).bg(rgb(t.accent)))
                            .when(!on, |d| d.w(px(10.)).bg(rgb(0xa9beb5)))
                            // 行 hover：刻度增亮（刻度本体仅 2px 高，
                            // group_hover 让整行命中区即可触发）
                            .group_hover(tick_group, |s| s.bg(rgb(t.accent))),
                    ),
            );
        }
        gutter = gutter.child(
            div()
                .flex()
                .h_full()
                .w_full()
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

/// 相位行文本（pi-web：13px text_muted，1.5s 透明度脉冲）。
fn phase_pulse(label: String, t: &crate::theme::Theme) -> gpui::AnyElement {
    div()
        .text_size(crate::appearance::ui_size(13.))
        .text_color(rgb(t.text_muted))
        .with_animation(
            "phase-pulse",
            Animation::new(std::time::Duration::from_millis(1500)).repeat(),
            move |el, delta| {
                let wave = (delta * std::f32::consts::PI).sin().abs() as f32;
                el.opacity(0.45 + 0.55 * wave)
                    .child(SharedString::from(label.clone()))
            },
        )
        .into_any_element()
}

