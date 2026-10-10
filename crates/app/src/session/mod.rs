//! sessionView (v54): 聊天列（消息列表 + 悬浮 composer；空会话走 012 新会话页
//! = `new_session`）+ 会话导航面板（033：右缘 10px 刻度条，75% 高，悬停左弹
//! 400px 轮次摘要卡片列表）。无工具栏、无内嵌状态行（v54 按设计删除）。

pub(crate) mod actions_bar;
pub(crate) mod chat_list;
pub(crate) mod diff;
pub(crate) mod fork;
pub(crate) mod input;
pub(crate) mod model_picker;
pub(crate) mod messages;
pub(crate) mod new_session;
pub(crate) mod runtime;
pub(crate) mod plugin_picker;
pub(crate) mod mcp_picker;
pub(crate) mod tools_recipe;

use gpui::{Animation, AnimationExt, MouseButton, SharedString, div, list, prelude::*, px, relative, rgb};
use pi_link::protocol::Block;

use self::messages::{
    Role, compute_meta, render_assistant_turn, render_bash_msg, render_custom_msg, render_msg,
};
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
    // 012 新会话页判据 = pi-web isEmptyNew（空会话且 agent 未跑）：启动时
    // 项目|会话列表为空、或「新建会话」都落在这里
    let new_session_page = rt.read(cx).messages.is_empty() && !streaming;
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
                .child({
                    let mut wrap = div()
                        .id("chat-wrap")
                        .flex_1()
                        .min_w_0()
                        .relative()
                        .flex()
                        .flex_col();
                    if new_session_page {
                        // 012 新会话页：标题 + 超大背景 logo + inputpanel +
                        // 额外操作栏；扩展行照旧挂页面上下
                        wrap = wrap
                            .children(ext_widget_rows(chat, t, true))
                            .child(new_session::page(
                                chat,
                                weak,
                                streaming,
                                input_focused,
                                f32::from(window.viewport_size().height),
                                cx,
                            ))
                            .children(ext_widget_rows(chat, t, false));
                    } else {
                        wrap = wrap
                            // message list（底部 135px 让位悬浮 composer）
                            .child(session_list(
                                chat,
                                chat_entity,
                                weak.clone(),
                                rt_list,
                                rt_entity.clone(),
                                t,
                            ))
                            // 悬浮 composer：0 高 wrapper（不吞点击/滚轮），
                            // 胶囊绝对定位上浮叠在聊天区上
                            .children(ext_widget_rows(chat, t, true))
                            .child(input::input_area(
                                chat,
                                weak,
                                streaming,
                                input_focused,
                                false,
                                cx,
                            ))
                            .children(ext_widget_rows(chat, t, false));
                    }
                    wrap
                })
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
        // 两侧边距 15px（窄窗口下内容不贴边；920 列宽不动，宽窗居中）
        // 必须与 composer 悬浮层 padding 一致（input.rs），否则胶囊与消息列错位
        .px(px(15.))
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
            // `!` shell 命令卡（031）：独立消息行，不进轮分组
            Some(m) if m.role == Role::Bash => div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(920.))
                        .child(render_bash_msg(m, ix, t, &rt_view.collapsed, &weak)),
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
                            // 复制反馈（032 恢复）：copy_flash 点亮且未超 1.5s
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
                    .position(|mm| matches!(mm.role, Role::User | Role::Custom | Role::Bash))
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
                // 「新分支」锚点：分支保留到本轮 agent 回复为止 → fork 到
                // 「下一条用户消息之前」（该用户消息本身及之后的内容丢弃）。
                // 本轮就是尾部时 pi 没有可 fork 的 before 目标 → 走 rpc
                // clone（整段复制）。锚点还没回来就不给按钮。
                let fork = if rt_view.active_user_entry_ids.is_empty() {
                    None
                } else {
                    let next_user = rt_view.messages[end..]
                        .iter()
                        .find(|m| m.role == Role::User)
                        .and_then(|m| m.entry_id.clone());
                    Some(self::fork::ForkAnchor {
                        tail: end >= rt_view.messages.len(),
                        next_user,
                        forking: rt_view.forking,
                    })
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
                                stream_tps,
                                compute_meta(&rt_view.messages, ix),
                                chat.bar_hover == Some(ix),
                                // 复制反馈（032 恢复）：copy_flash 点亮且未超 1.5s
                                // （键 = 轮头 start_ix，与用户栏的 msg_ix 互不重叠）
                                rt_view.copy_flash.is_some_and(|(cix, at)| {
                                    cix == ix && at.elapsed().as_millis() < 1500
                                }),
                                fork,
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


/// 会话导航面板 (033)：右缘 10px 竖条，屏高 75%，按「屏高 − composer
/// 高度×50%」居中；生成算法（v63-5，2026-10-10 上限 20→10）：轮次均分
/// 切割整条——1 轮整条弱主题色、N 轮切 N 段（flex_1 等分 + 2px 缝），10
/// 轮后段高不再缩小，>10 走百分比桶映射（轮 k → 段 k×10/N，点击回桶首
/// 轮）。当前定位段 accent
/// 实色——选中跟随真实滚动位置（贴底 = 末段），卡片悬浮框不回写。
/// 悬停向左弹出 400px 轮次摘要卡片列表（用户前 50 字 + agent 首条回复
/// 前 50 字，编号 01-99）+ 30px 总结栏，点击卡片/段定位会话；内容溢出
/// 时右缘显示滚动条。摘要数据走 runtime 缓存（v63-3）：notify_list 置
/// 脏惰性重建，渲染帧零重算。
fn nav_gutter(
    chat: &mut Chat,
    rt_entity: gpui::Entity<crate::session::runtime::SessionRuntime>,
    weak: gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    // 摘要读 runtime 缓存（v63-3：notify_list 置脏、惰性重建，渲染帧
    // 零重算——摘要「一轮一次」，不随渲染帧反复拼前缀）
    let (summary, scroll_top_ix, nav_pinned) = rt_entity.update(cx, |rt, _| {
        (rt.nav_summary(), rt.pager.scroll_top_ix(), rt.pager.nav_pinned())
    });
    // 刻度选中 = 真实定位轮次（033）：视口顶之上最近的轮。贴底（含未
    // 滚动的短会话）时逻辑位是 None，scroll_top_ix 回退成总条目数 →
    // 自然选中末轮（对话常态在末轮）。**不能**用 pager.is_at_bottom：
    // 那是滚轮回调维护的 Cell，程序化定位不经过它——导航点击后它仍是
    // true，刻度会错停在末轮（v63-1 踩坑）。
    // 导航点击优先：nav_pinned = 被点轮次（贴底钳制会改写视口顶，位置
    // 推导会错选上一段；物理滚轮清除交还推导，v63-7）
    let active_turn: Option<usize> = nav_pinned.or_else(|| {
        summary
            .turns
            .iter()
            .rposition(|s| s.user_ix <= scroll_top_ix + 1)
    });
    // 033 段上限 10（v63-5 生成算法；2026-10-10 由 20 收紧——20 段太碎）：
    // 轮次均分切割整条竖条——N 轮切
    // N 段（flex_1 等分 + 2px 缝），10 轮后段高不再缩小，>10 走既有的
    // 百分比桶映射（轮 k 命中段 k×10/N，点击段回桶首轮 i×N/10）。定位/
    // 选中算法与刻度时代完全一致：active 跟随真实滚动位置；飞出卡片仍
    // 逐轮全列（滚动查看）
    const TICK_MAX: usize = 10;
    let n_turns = summary.turns.len();
    let tick_count = n_turns.min(TICK_MAX);
    let turn_of_tick = move |i: usize| -> usize { i * n_turns / tick_count };
    // 逆映射用桶包含式 floor(((t+1)·k − 1)/N)：与正向边界 floor(i·N/k)
    // 自洽（i = max{i : floor(i·N/k) ≤ t}）。两个错例都踩过：朴素
    // floor(t·k/N) 对桶起始轮（i·N mod k ≠ 0）floor 回上一桶——点段 i
    // 高亮 i−1；不带 −1 的 floor((t+1)·k/N) 整体偏下一桶（恒等映射下
    // = t+1，点哪段亮下一段）——v63-7 两版都踩过，此为终版
    let tick_of_turn = move |t: usize| -> usize {
        (((t + 1) * tick_count - 1) / n_turns).min(tick_count - 1)
    };

    let mut gutter = div()
        .id("nav-gutter")
        .relative()
        // 14 = 段命中区宽（实体 10 + 右侧 4px 透明可点，不贴窗口边）
        .w(px(14.))
        .h_full()
        .flex_shrink_0();
    // 悬停展开的轮次摘要列表（033：topbar 之下 100% 高、400px 宽、向左
    // 弹出、内容溢出显示滚动条）。滚动条挂在滚动容器平级的 absolute
    // 兄弟上（滚动容器内的绝对定位子元素会随内容滚走）。整包结构：
    // wrap（flex 列）= 总结栏 30px 固定 + 卡片滚动区 flex_1
    if chat.nav_open && !summary.turns.is_empty() {
        // absolute 定位本身即绝对定位子孙（滚动条）的包含块，无需再挂
        // relative——且 position 是单字段，relative 会覆盖 absolute
        let mut flyout_wrap = div()
            .id("nav-flyout-wrap")
            .absolute()
            // 14 = 与段左缘齐平（gutter 14 = 段命中区 10 实体 + 右 4 透明）
            .right(px(14.))
            .top_0()
            .bottom_0()
            .w(px(400.))
            .flex()
            .flex_col()
            // 悬停保持打开：挂 wrap（含总结栏）而不是滚动区——鼠标挪到
            // 总结栏不算离开面板，否则 250ms 宽限一到面板就收了
            .occlude()
            .shadow_lg()
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
        // 总结栏（v63-4）：30px 固定高，滚动区之外。口径「按我们自己设
        // 计」：N = 用户 + 助手 + 工具调用（ToolCall 块数），非 pi-web
        // totalMessages
        let stats_line = crate::i18n::tf(
            "{n}条消息：用户{u}条 助手{a}条 工具调用{t}次",
            &[
                (
                    "n",
                    (summary.users + summary.assistants + summary.tool_calls)
                        .to_string(),
                ),
                ("u", summary.users.to_string()),
                ("a", summary.assistants.to_string()),
                ("t", summary.tool_calls.to_string()),
            ],
        );
        let header = div()
            .h(px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            // 左缘 14 = 面板 pl 6 + 卡片 px 8，与卡片文字对齐
            .px(px(14.))
            .border_b_1()
            // 分隔线与面板左边框同档（0x40 太淡看不出）
            .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x80)))
            .bg(rgb(t.bg))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(stats_line));
        // 卡片列表虚拟化（v63-6）：ListState 只建可视卡片（~20）而非全
        // 量（千轮会话原先是 1.2 万+ div/帧）。轮次数仅在用户新增一轮时
        // 变化（面板开着时几乎不可能），reset 重算计数
        if chat.nav_flyout_list.item_count() != summary.turns.len() {
            chat.nav_flyout_list.reset(summary.turns.len());
        }
        let weak_cards = weak.clone();
        let summary_cards = summary.clone();
        let rt_cards = rt_entity.clone();
        let flyout = div()
            .id("nav-flyout")
            .w_full()
            .flex_1()
            .min_h_0()
            // list 子元素靠 flex_1 撑高——父容器必须是 flex 列：block 容
            // 器里 flex_grow 无效，list 高度 auto 塌成 0，一张卡都不渲染
            //（v63-6 踩坑；session_list 的父容器同样是 flex().flex_col()）
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .border_l_1()
            .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x80)))
            // 水平 padding 必须挂外层容器：gpui List 的 prepaint 只应用
            // 条目的 padding.top/bottom，左右 padding 对条目完全无效
            //（session_list 同款结论）
            .pl(px(6.))
            .pr(px(14.))
            .child(
                gpui::list(chat.nav_flyout_list.clone(), move |ix, _window, cx| {
                    let Some(s) = summary_cards.turns.get(ix) else {
                        return div().into_any_element();
                    };
                    // 选中框 = 悬浮光标（enter 置位），状态在 Chat 上，
                    // 经 weak 升级读取（list 闭包只有 &mut App）
                    let on = weak_cards
                        .upgrade()
                        .is_some_and(|c| c.read(cx).nav_hover_turn == Some(ix));
                    nav_card(ix, s, on, &weak_cards, &rt_cards, t)
                })
                // 上下 padding 挂 list 本体（prepaint 会应用）
                .pt(px(8.))
                .pb(px(8.))
                .flex_1()
                .min_h_0(),
            );
        flyout_wrap = flyout_wrap.child(header).child(flyout);
        // 滚动条仅在实际溢出时渲染；ListStateHandle 把 ListState 适配成
        // gpui-component Scrollbar 的句柄（拖拽走 list.rs 的 scrollbar 协
        // 作接口），锚在滚动区（总结栏之下）
        if chat.nav_flyout_list.max_offset_for_scrollbar().height > px(0.) {
            let handle = crate::ui::list_handle::ListStateHandle(
                chat.nav_flyout_list.clone(),
            );
            let sb_state = gpui_component::scroll::ScrollbarState::default();
            flyout_wrap = flyout_wrap.child(
                div()
                    .absolute()
                    .top(px(38.))
                    .bottom(px(8.))
                    .right(px(3.))
                    .w(px(8.))
                    .child(
                        gpui_component::scroll::Scrollbar::vertical(&sb_state, &handle),
                    ),
            );
        }
        gutter = gutter.child(flyout_wrap);
    }

    // 导航刻度条（033）：屏高 75%；居中规则 = 屏高 − inputpanel 高度×50%
    // 后居中——rail 挂 mb(0.5×composer高)，flex 连 margin 盒一起居中，
    // 视觉中心正好上移 composer/4。生成算法（v63-5）：轮次均分切割整条
    // ——flex_1 等分 + 2px 缝，1 轮 = 整条弱主题色、2 轮 = 上下两段……
    // 10 轮后段高不再缩小（>10 百分比桶映射）。段色 = 弱主题色常驻，
    // 选中 = accent 边框（不整段覆盖），hover 增亮；段命中区 14px = 实体
    // 10px + 右侧 4px 透明可点（2026-10-10：命中不满宽会点穿下方内容
    // 误收面板）
    if !summary.turns.is_empty() {
        let composer_h = chat.composer_h.get();
        let active_tick = active_turn.map(tick_of_turn);
        let mut rail = div()
            .id("nav-rail")
            .h_full()
            .w(px(14.))
            .flex()
            .flex_col()
            // 实体段贴左，右侧留 4px 透明命中区
            .items_start()
            .gap(px(2.));
        for i in 0..tick_count {
            let on = active_tick == Some(i);
            let target_ix = summary.turns[turn_of_tick(i)].user_ix;
            let rt_scroll = rt_entity.clone();
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("nav-node-{i}")))
                    // 命中区吃满 rail 14px：左 10px 实体 + 右 4px 透明同权
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .group("navtick")
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            // 面板联动（v63-6）：面板滚到该段的区域起始卡
                            //（卡序号 = 轮序号）
                            this.nav_flyout_list
                                .scroll_to_reveal_item(turn_of_tick(i));
                            rt_scroll.read(cx).pager.nav_goto(turn_of_tick(i), target_ix);
                            window.refresh();
                        }),
                    )
                    // 可见段体只有左 10px；右 4px 透明但同属命中区
                    .child(
                        div()
                            .w(px(10.))
                            .h_full()
                            .rounded(px(2.))
                            // 常驻 2px 边框（透明↔accent）防选中时尺寸跳动；
                            // 选中 = 2px accent 实边
                            .border_2()
                            .bg(gpui::rgba((t.accent as u32) << 8 | 0x3d))
                            .when(on, |d| d.border_color(rgb(t.accent)))
                            .when(!on, |d| d.border_color(gpui::rgba(0x00000000)))
                            // 段 hover：整段增亮到实色（navtick 组联动——
                            // 4px 透明区悬停同样增亮）
                            .group_hover("navtick", |s| s.bg(rgb(t.accent))),
                    ),
            );
        }
        gutter = gutter.child(
            // 居中外层（与原 rail 定位一致）：h_full + justify_center 把
            // rail 垂直居中，连 margin 盒一起算（视觉中心上移 composer/4）
            div()
                .flex()
                .h_full()
                .w_full()
                .flex_col()
                .items_start()
                .justify_center()
                .child(
                    // hover 响应区 = rail 垂直带 × gutter 全宽（033「悬浮
                    // 竖条弹出」）：高度 0.75 屏 + composer 偏移与 rail 同
                    // 款，宽度 w_full 含右侧 4px 呼吸位——只挂 rail 本体
                    // 的话那 4px 是死区；挂 gutter 全高又会误触上下空白区
                    div()
                        .id("nav-rail-hit")
                        .h(relative(0.75))
                        .mb(px(composer_h * 0.5))
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_start()
                        .on_hover(cx.listener(|this, h: &bool, _w, cx| {
                            if *h {
                                this.nav_open = true;
                                this.nav_hide_at = None;
                            } else if !this.nav_flyout_hovered {
                                this.nav_hide_at =
                                    Some(std::time::Instant::now());
                            }
                            cx.notify();
                        }))
                        .child(rail),
                ),
        );
    }
    gutter.into_any_element()
}

/// 轮次摘要卡片（v63-6 从 nav_gutter 循环抽出供 list() 按需调用）：
/// 编号 + 用户前 50 字 + A 行 agent 摘要，A 对齐编号、正文对齐用户正文。
/// `on` = 悬浮选中框；hover 只在 enter 置位、leave 不清——gpui 鼠标事件
/// bubble 阶段按绘制逆序派发，向下移动时上一张卡的 leave 在下一张卡的
/// enter 之后触发会误清（v63 根因），统一由面板 on_hover(false) 清理。
fn nav_card(
    ti: usize,
    s: &crate::session::runtime::TurnSummary,
    on: bool,
    weak: &gpui::WeakEntity<Chat>,
    rt_entity: &gpui::Entity<crate::session::runtime::SessionRuntime>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    let no = format!("{:02}", ti + 1);
    let chars = s.user_prefix.clone();
    let user_ix = s.user_ix;
    let rt_scroll = rt_entity.clone();
    let weak_hover = weak.clone();
    let mut turn_el = div()
        .id(SharedString::from(format!("nv-turn-{}", s.user_ix)))
        .mt(px(2.))
        .px(px(8.))
        .py(px(7.))
        // 选择框四面完整：常驻 1px 边框（透明↔accent 切换，无布局跳动），
        // bg 同步切换
        .rounded(px(8.))
        .border_1()
        .border_color(gpui::rgba(0x00000000))
        .when(on, |d| {
            d.bg(gpui::rgba((t.accent as u32) << 8 | 0x0f))
                .border_color(rgb(t.accent))
        })
        .on_hover(move |h: &bool, _window, cx| {
            if *h {
                let _ = weak_hover.update(cx, |this, cx| {
                    if this.nav_hover_turn != Some(ti) {
                        this.nav_hover_turn = Some(ti);
                        cx.notify();
                    }
                });
            }
        })
        .child(
            // 用户行：点击定位到该用户消息
            div()
                .id(SharedString::from(format!("nv-user-{user_ix}")))
                .flex()
                .gap(px(9.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    rt_scroll.read(cx).pager.nav_goto(ti, user_ix);
                    // 程序化定位不触发任何 notify，点击后当帧重渲染，
                    // 选中段才跟得上定位
                    window.refresh();
                })
                .child(
                    div()
                        .w(px(18.))
                        .text_right()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
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
                        //（字体大小设置.md §1）；不加粗（2026-10-10 定夺）
                        .text_size(crate::appearance::ui_size(11.))
                        .line_height(relative(1.55))
                        .text_color(rgb(t.text))
                        .max_h(px(58.))
                        .overflow_hidden()
                        .child(SharedString::from(chars)),
                ),
        );
    // 摘要行：A 与轮次编号对齐、正文与用户发言对齐（033）——与用户行
    // 同构（18px 右对齐标号列 + gap 9）
    if let Some((aix, preview)) = s.agent.as_ref() {
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
                    rt_scroll_a.read(cx).pager.nav_goto(ti, aix);
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
    turn_el.into_any_element()
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

