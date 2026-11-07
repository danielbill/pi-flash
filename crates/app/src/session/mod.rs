//! sessionView (v54): 聊天列（消息列表 + 悬浮 composer；空会话走 012 新会话页
//! = `new_session`）+ 会话导航面板（033：右缘 10px 刻度条，75% 高，悬停左弹
//! 400px 轮次摘要卡片列表）。顶部会话功能栏（2026-10-11，原 topbar ⋯ 菜单
//! 改条；仅会话态，新会话页隐藏）；无内嵌状态行（v54 按设计删除）。

pub(crate) mod actions_bar;
pub(crate) mod attachments;
pub(crate) mod chat_list;
pub(crate) mod diff;
pub(crate) mod footer_bar;
pub(crate) mod fork;
pub(crate) mod input;
pub(crate) mod mcp_picker;
pub(crate) mod menu;
pub(crate) mod menu_rows;
pub(crate) mod messages;
pub(crate) mod model_picker;
pub(crate) mod nav_panel;
pub(crate) mod new_session;
pub(crate) mod pi_extension_picker;
pub(crate) mod picker_shell;
pub(crate) mod pool;
pub(crate) mod runtime;
pub(crate) mod subagent_panel;
pub(crate) mod template;
pub(crate) mod tools_recipe;

use gpui::{
    Animation, AnimationExt, MouseButton, SharedString, div, list, prelude::*, px, relative, rgb,
};
use pi_link::protocol::Block;

use self::nav_panel::{nav_gutter, phase_row};
use crate::i18n::tr;

use self::messages::{
    Role, compute_meta, render_assistant_turn, render_bash_msg, render_custom_msg, render_msg,
};
use crate::ext_ui::{render_ext_dialog, render_ext_widget};

use crate::Chat;

// 流式 token 估算门（A4，2026-11-06 流式卡死修复）：stream_est 原为每帧
// 把末条消息全部 Text+Thinking 拼成一个 String 再逐字估算——O(已流式总
// 字数)/帧，快流模型（116 t/s）下与 markdown 重解析叠加把帧成本推爆。
// 250ms 时间门：门内复用上次估算值（t/s 徽章本来就按 250ms 量级波动，
// 无感知）；门过即重算。会话切换后 ≤250ms 内可能沿用上一会话的值，量级
// 不变，无感知。
thread_local! {
    static STREAM_EST_GATE: std::cell::RefCell<Option<(std::time::Instant, u64)>> =
        const { std::cell::RefCell::new(None) };
}
const STREAM_EST_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

fn stream_est_throttled(compute: impl FnOnce() -> u64) -> u64 {
    STREAM_EST_GATE.with(|cell| {
        let mut gate = cell.borrow_mut();
        if let Some((at, est)) = gate.as_ref() {
            if at.elapsed() < STREAM_EST_INTERVAL {
                return *est;
            }
        }
        let est = compute();
        *gate = Some((std::time::Instant::now(), est));
        est
    })
}
use crate::appearance::theme::theme as T;
use crate::services::format::*;

pub(crate) use subagent_panel::SubagentPanel;

/// 当前活跃会话所属「线」的皮肤 = 会话页模版注入面的**唯一取值处**。
/// 新增一条功能线 = 在它自己的模块里给一份 `SessionPageSkin`，这里加一个
/// 分支；页面骨架保持零判定。
pub(crate) fn skin(_chat: &Chat) -> &'static template::SessionPageSkin {
    // 060 宿主功能面板退役（2026-10-29）：会话页皮肤恒桌面版（插件弹窗接管 UI）
    &template::DESKTOP
}

/// 加载页主体（2026-11-07 用户定夺）：文件已绑定、权威内容未到第一份——
/// 空白页 + 居中载入提示，不用欢迎语冒充加载态。放行条件见
/// `SessionRuntime::messages_loaded`（投影落定/失败响应/tail 非空）。
fn loading_body(t: &crate::appearance::theme::Theme) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(crate::appearance::ui_size(13.))
                .text_color(rgb(t.text_dim))
                .child(tr("session.loading")),
        )
        .into_any_element()
}

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
    // 会话页模版（030）：桌面线与插件功能线共用本骨架，差异全部由皮肤注入
    // （操作栏右侧簇 / 空会话策略 / 新会话页皮肤）——骨架里不写任何线判定。
    let page_skin = skin(chat);
    // 012 新会话页判据 = pi-web isEmptyNew（空会话且 agent 未跑）：启动时
    // 项目|会话列表为空、或「新建会话」都落在这里。插件功能线除外（皮肤
    // `keep_on_empty`）：它空会话也停在会话页，设置页由功能页分段承担。
    // 2026-11-07 用户定夺：**文件已绑定但内容未到第一份**的打开窗口渲染
    // 空白页 + 「会话载入中…」（messages_loaded 未就位），不再拿欢迎页
    // 冒充加载态——草稿（file=None）才落新会话页；投影/失败响应落定即
    // 放行（真空会话落正常会话页空列表）。
    let (messages_empty, file_bound, messages_loaded) = {
        let r = rt.read(cx);
        (r.messages.is_empty(), r.file.is_some(), r.messages_loaded)
    };
    let new_session_page = messages_empty && !streaming && !page_skin.keep_on_empty && !file_bound;
    let loading_page =
        messages_empty && !streaming && file_bound && !messages_loaded && !page_skin.keep_on_empty;
    // composer 焦点态来自输入组件（chat.layout.focus 仅是组件创建前的回退）
    let input_focused = chat
        .composer
        .composer
        .as_ref()
        .map(|c| c.read(cx).focus_handle_in(cx).is_focused(window))
        .unwrap_or(false);
    chat.composer.input_focused = input_focused;
    let chat_entity = entity.clone();
    let rt_entity = rt.clone();
    let rt_list = rt.read(cx).pager.state();
    // 只读观察窗（044-0）：子代理子会话 tab —— composer/操作栏全隐，
    // 底部换只读提示条
    let observer = rt.read(cx).observer;

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
        .when(!new_session_page, |d| {
            d.child(session_fn_bar(chat, page_skin, cx))
        })
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
                        .flex_col()
                        // wrap bounds 每帧捕获（PillBtns::tracker 同款 canvas，
                        // dock 容器同模式）：子代理面板的窗口坐标 → wrap 坐标
                        // 换算需要 wrap 原点（044-0 与按钮居中对齐）
                        .child(crate::PillBtns::tracker(&chat.dlg.pill_btn.chat_wrap));
                    if new_session_page {
                        // 012 新会话页：标题 + 超大背景 logo + inputpanel +
                        // 额外操作栏；扩展行照旧挂页面上下。034 状态带**不挂**
                        // （draft 无扩展状态，且会破坏空态构图）。
                        wrap = wrap
                            .children(ext_widget_rows(chat, t, true))
                            .child(new_session::page(
                                chat,
                                weak,
                                page_skin,
                                streaming,
                                input_focused,
                                f32::from(window.viewport_size().height),
                                cx,
                            ))
                            .children(ext_widget_rows(chat, t, false));
                    } else if loading_page {
                        // 加载页（2026-11-07 用户定夺）：空白页 + 载入提示，
                        // composer 不渲染（pi 还在恢复会话，此时发送只会堆进
                        // 未挂线的队列）；内容落定/失败放行见 messages_loaded。
                        wrap = wrap.child(loading_body(t));
                    } else {
                        wrap = wrap
                            // message list（底部 135px 让位悬浮 composer；
                            // 观察窗无 composer，让位高度由提示条占住观感）
                            .child(session_list(
                                chat,
                                chat_entity,
                                weak.clone(),
                                rt_list,
                                rt_entity.clone(),
                                t,
                            ))
                            // 悬浮 composer：0 高 wrapper（不吞点击/滚轮），
                            // 胶囊绝对定位上浮叠在聊天区上。观察窗不渲染
                            // （044-0：用户输入会生成第二驱动分叉账本/双花
                            // token/抢占并发槽——子代理由主agent独占管理），
                            // 换只读提示条。
                            .children(ext_widget_rows(chat, t, true))
                            .child(if observer {
                                observer_notice(t)
                            } else {
                                input::input_area(chat, weak, streaming, input_focused, false, cx)
                                    .into_any_element()
                            })
                            .children(ext_widget_rows(chat, t, false))
                            // 034 会话区底部状态栏：**absolute** 盖在 chat-wrap
                            // 底缘（最后绘制，压在消息列表之上），不参与 flex 流
                            // —— 开闭都不推挤 Inputpanel（胶囊底距恒为条高）。
                            // 无白名单项或开关关闭时整条不建 element。
                            .children(footer_bar::bar(chat, cx));
                    }
                    // ── 会话域居中浮层槽（机制）──────────────────────────
                    // 所有「以 InputPanel 为居中基准」的会话弹窗统一挂
                    // chat-wrap 尾部（absolute inset_0 随中栏定界，
                    // items_center 自动对齐 composer 中轴）。挂全窗根会被
                    // 左右侧栏挤偏（ask_card 090 用户实测定案）。新增会话
                    // 弹窗 = 在此追加一个 if 挂载；z 序 = 子节点绘制序，
                    // 模态压非模态。切走 content tab 时随主栏同隐（状态
                    // 仍在，切回即见；ask_card 同语义）。
                    // 非模态在下（插件弹窗），模态在上（问卷卡 → 扩展阻塞弹窗）。
                    if chat.plugins.popup.is_some() {
                        wrap = wrap.child(crate::pf_plugin::plugin_popup_layer(chat, t, cx));
                    }
                    // ask_user_question 问卷卡浮层：挂进 chat-wrap（中栏）——
                    // 与 composer 同一居中基准；挂整窗根会被左右侧栏挤偏
                    //（用户实测不对齐）。窄窗口下卡片收缩不溢出。
                    if chat.aux.ask.as_ref().is_some_and(|a| a.open) {
                        wrap = wrap.child(crate::ask_card::render_ask_card(chat, weak));
                    }
                    // 扩展阻塞弹窗（select/confirm/input/editor）：与问卷卡
                    // 同基准，遮罩只盖会话区
                    if let Some(req) = chat.aux.ext_dialog.clone() {
                        wrap = wrap.child(render_ext_dialog(chat, req, weak));
                    }
                    wrap
                })
                .child(nav_gutter(chat, rt_entity.clone(), weak.clone(), t, cx)),
        )
}

/// 会话功能栏（2026-10-11：topbar ⋯ 菜单撤下拉改常驻条）：会话 tab 下方
/// 36px 操作条，版式对齐文件标签下方的 file_nav_bar（同下边框、同
/// pl12/pr8；2026-10-21 用户定案：栏高 36，不再与 file_nav_bar 32px 同高）。2026-10-16 系统提示词 / 会话工具集两钮迁入上下文用量面板
/// （session/input/ctx_tip.rs）；新增「会话目录」（资源管理器打开当前项目
/// 的 pi 会话组目录，动作同 psp 项目详情卡 explorer 钮）+「打开终端」，
/// 钮间淡色 `|` 区隔。终端视图开着时终端钮点亮 accent。
pub(crate) mod fn_bar;
pub(crate) use fn_bar::{
    ext_widget_rows, observer_notice, scroll_to_bottom_button, session_fn_bar, session_list,
};
