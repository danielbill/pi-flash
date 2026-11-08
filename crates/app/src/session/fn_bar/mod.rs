//! 会话域功能栏（034）：session_fn_bar 装配与 fn_bar_btn 通用按钮。

use super::*;

/// 会话功能栏（2026-10-11：topbar ⋯ 菜单撤下拉改常驻条）：会话 tab 下方
/// 36px 操作条，版式对齐文件标签下方的 file_nav_bar（同下边框、同
/// pl12/pr8；2026-10-21 用户定案：栏高 36，不再与 file_nav_bar 32px 同高）。2026-10-16 系统提示词 / 会话工具集两钮迁入上下文用量面板
/// （session/input/ctx_tip.rs）；新增「会话目录」（资源管理器打开当前项目
/// 的 pi 会话组目录，动作同 psp 项目详情卡 explorer 钮）+「打开终端」，
/// 钮间淡色 `|` 区隔。终端视图开着时终端钮点亮 accent。
pub(crate) fn session_fn_bar(
    chat: &mut Chat,
    skin: &template::SessionPageSkin,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    // 栏首项目名（2026-10-14）：当前工作区基名（ws.cwd），粗体亮色，后随
    // 淡色 chevron（编辑器面包屑同款 ">"），再接原有三钮。（2026-10-20
    // 用户定案：改回一直展示，不再随功能面板收拢态切换）。无项目态显示占位名。
    let project: Option<SharedString> = Some(
        chat.ui_project()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| tr("statusbar.no_project").to_string())
            .into(),
    );
    // 会话组目录（sfbar-sessdir）：pi sessions 根下按 cwd 编码的组目录，
    // 还没建（无落盘会话）退回 sessions 根——与 psp 项目详情卡同口径；
    // 无项目态直接开 sessions 根
    let sess_dir_btn = {
        let cwd = chat.ui_project().map(|p| p.to_path_buf());
        move |_: &gpui::MouseDownEvent, _w: &mut gpui::Window, _cx: &mut gpui::App| {
            let dir = pi_link::sessions::sessions_root().map(|root| {
                match cwd.as_deref() {
                    Some(c) => {
                        let d =
                            root.join(pi_link::sessions::group_name_for_cwd(&c.to_string_lossy()));
                        if d.is_dir() { d } else { root }
                    }
                    // 无项目态：直接开 sessions 根
                    None => root,
                }
            });
            if let Some(d) = dir {
                crate::services::shell_open::reveal_in_explorer(&d);
            }
        }
    };
    div()
        .id("session-fn-bar")
        .h(px(36.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .pr(px(8.))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
        // 左距 23：栏首项目名与 topbar 会话 tab 的 icon 墨迹对齐
        //（tab = strip pl 10 + tab pl 12 = 22，23 为实测校准值）
        .pl(px(23.))
        .children(project.map(|project| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .mr(px(4.))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(11.5))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(t.text))
                        .child(project),
                )
                .child(crate::ui::icon("chevron-right", 10., t.text_faint))
        }))
        .child(fn_bar_btn(
            "sfbar-sessdir",
            "folder-input",
            tr("session.open_dir"),
            false,
            sess_dir_btn,
        ))
        .child(fn_bar_sep(t))
        .child(fn_bar_btn(
            "sfbar-term",
            "icon-terminal-solid",
            tr("session.open_terminal"),
            chat.layout.content_view == crate::ContentView::Term,
            cx.listener({
                // 终端起点跟界面当前项目
                let term_cwd = chat.ui_project().map(|p| p.to_path_buf());
                move |this, _: &gpui::MouseDownEvent, window, cx| {
                    this.open_terminal(term_cwd.clone(), window, cx);
                    cx.notify();
                }
            }),
        ))
        // 线的操作栏注入簇（模版注入点 ①，插件功能线 = 自有 icon 三色 +
        // 启停开关/说明文字 + 新建会话 + 重新绑定；002 操作栏定序）
        .children(skin.fn_bar.map(|f| f(chat, cx)))
        // 插件功能栏注册槽（契约 v3 fn_bar）：**功能栏右侧**（002，弹性占位
        // 把插件 icon 推到右端；注册序从右到左），点击回环 command（env 带会话键）
        .child(div().flex_1())
        .children(crate::pf_plugin::fn_bar_plugin_elements(chat, cx))
        .into_any_element()
}

/// 功能栏按钮：icon 在前 + 文案，段内对称 padding（内容居中）；`on` =
/// 对应面板/视图开着（accent 点亮）。
pub(crate) fn fn_bar_btn(
    id: &'static str,
    icon_name: &'static str,
    label: &'static str,
    on: bool,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let t = T();
    let hover_fg = if on { t.accent } else { t.text };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(4.))
        .rounded(px(6.))
        .text_size(crate::appearance::ui_size(11.5))
        .text_color(rgb(if on { t.accent } else { t.text_muted }))
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(hover_fg)))
        .on_mouse_down(MouseButton::Left, handler)
        .child(crate::ui::icon(
            icon_name,
            crate::ui::tokens::icon::MD,
            if on { t.accent } else { t.text_muted },
        ))
        .child(SharedString::from(label))
        .into_any_element()
}

/// 功能栏钮间淡化分隔（文字竖线，text_faint 档）。
fn fn_bar_sep(t: &'static crate::appearance::theme::Theme) -> gpui::AnyElement {
    div()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_faint))
        .child(SharedString::from(tr("session.separator")))
        .into_any_element()
}

/// 回到最新消息（pi-web chat-scroll-to-bottom parity）：36px 圆钮，由
/// composer 悬浮容器以 gap 20px 叠在输入面板顶部上方（随面板增高上移，
/// 永不叠进面板）；常态全亮、无投影；点击滚到末条并置位贴底。
pub(crate) fn scroll_to_bottom_button(
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let weak = weak.clone();
    div()
        .id("scroll-to-bottom")
        .flex_shrink_0()
        .size(px(36.))
        // 悬浮在消息列上（composer 悬浮簇成员），必须自挡穿透：不 occlude
        // 时点击会漏进身后的消息文本——TextView 起选监听判定遮挡用 hitbox，
        // 本钮自身无 BlockMouse hitbox 就挡不住（vendor text_view 修同期）
        .occlude()
        .rounded_full()
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.rt().update(cx, |r, _| r.scroll_to_bottom());
                cx.notify();
            });
        })
        .child(crate::ui::icon("arrow-down", 16., t.text_muted))
        .into_any_element()
}

/// 只读观察窗底部提示条（044-0）：composer 不渲染，换一条居中的浅色提示
/// ——子代理由主会话管理，此处只观察。高 40px（占住 PAD_BOTTOM 让位区的
/// 观感），无交互。
pub(crate) fn observer_notice(t: &crate::appearance::theme::Theme) -> gpui::AnyElement {
    div()
        .h(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(tr("subagent.observer"))
        .into_any_element()
}

/// Message list + phase row (v54: 内容列宽见 `workspace::CHAT_WIDTH_*`，行距 22)。
pub(crate) fn session_list(
    _chat: &mut Chat,
    chat_entity: gpui::Entity<Chat>,
    weak_for_msg: gpui::WeakEntity<Chat>,
    rt_list: gpui::ListState,
    rt_entity: gpui::Entity<crate::session::runtime::SessionRuntime>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    // 内容列宽（设置-界面「会话内容宽度」；composer 胶囊读同一值）
    let content_w = crate::services::workspace::chat_content_width();
    // 水平边距必须挂外层容器：gpui List 的 prepaint 只应用 padding.top/
    // bottom（item_origin = bounds.origin + (0, padding.top)），左右 padding
    // 对条目完全无效——此前 pl34/pr30「看起来不存在」即此因
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        // 两侧边距 15px（窄窗口下内容不贴边；列宽封顶后宽窗居中）
        // 必须与 composer 悬浮层 padding 一致（input.rs），否则胶囊与消息列错位
        .px(px(15.))
        // 内容区右键菜单（单项「复制」= 复制当前消息划选；渲染挂
        // psp_overlays 同层 msg_menu_layer）。TextView 的 mouse-down 已
        // vendor 补丁为仅左键起选/清除，右键不破坏既有划选
        .on_mouse_down(gpui::MouseButton::Right, {
            let weak_menu = weak_for_msg.clone();
            move |ev: &gpui::MouseDownEvent, _, cx| {
                let _ = weak_menu.update(cx, |c, cx| {
                    c.dlg.msg_menu = Some((f32::from(ev.position.x), f32::from(ev.position.y)));
                    cx.notify();
                });
            }
        })
        .child(
            list(rt_list.clone(), move |ix, _window, cx| {
                let rt_view = rt_entity.read(cx);
                let chat = chat_entity.read(cx);
                let weak = weak_for_msg.clone();
                // 槽位一律按**列表侧结构**分配（messages.len() 会因 notify_list
                // 33ms 节流门短暂超前于列表条目——现算槽位会让垫片槽位被当成
                // 等待行渲染，一整屏垫片塌掉 → 内容被拽到屏底再跳回，
                // 见 chat_list::row_slot 注释）
                let (l_msgs, l_phase, l_spacer) = rt_view.pager.listed();
                let slot = chat_list::row_slot(ix, l_msgs, l_phase, l_spacer);
                match rt_view.messages.get(ix).filter(|_| ix < l_msgs) {
                    Some(m) if m.role == Role::Custom => div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(div().w_full().max_w(px(content_w)).child(render_custom_msg(
                            m,
                            ix,
                            t,
                            &rt_view.collapsed,
                            &weak,
                        )))
                        .into_any_element(),
                    // `!` shell 命令卡（031）：独立消息行，不进轮分组
                    Some(m) if m.role == Role::Bash => div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(div().w_full().max_w(px(content_w)).child(render_bash_msg(
                            m,
                            ix,
                            t,
                            &rt_view.collapsed,
                            &weak,
                        )))
                        .into_any_element(),
                    Some(m) if m.role == Role::User => div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(div().w_full().max_w(px(content_w)).child(render_msg(
                            m,
                            ix,
                            &weak,
                            &chat.composer.expanded_skills,
                            &chat.composer.bubble_scrolls,
                            &rt_view.collapsed,
                            t,
                            None,
                            compute_meta(&rt_view.messages, ix),
                            chat.composer.bar_hover == Some(ix),
                            // 复制反馈（032 恢复）：copy_flash 点亮且未超 1.5s
                            rt_view.copy_flash.is_some_and(|(cix, at)| {
                                cix == ix && at.elapsed().as_millis() < 1500
                            }),
                            rt_view.observer,
                        )))
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
                            .position(|mm| {
                                matches!(mm.role, Role::User | Role::Custom | Role::Bash)
                            })
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
                                // 250ms 时间门（A4）：全文拼接+估算不再每帧做
                                stream_est_throttled(|| {
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
                            })
                        } else {
                            None
                        };
                        // v56-5 c23（pi-web tps badge parity）：启动 0.5s 后显示
                        let stream_tps =
                            stream_est
                                .zip(rt_view.stream_started)
                                .and_then(|(est, start)| {
                                    let secs = start.elapsed().as_secs_f32();
                                    (secs >= 0.5 && est > 0).then(|| est as f32 / secs)
                                });
                        // 「新分支」锚点：分支保留到本轮 agent 回复为止 → fork 到
                        // 「下一条用户消息之前」（该用户消息本身及之后的内容丢弃）。
                        // 本轮就是尾部时 pi 没有可 fork 的 before 目标 → 走 rpc
                        // clone（整段复制）。锚点还没回来就不给按钮。
                        // 观察窗恒无 fork（044-0：变异操作随输入面板一并隐藏）。
                        let fork = if rt_view.observer || rt_view.active_user_entry_ids.is_empty() {
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
                            .child(div().w_full().max_w(px(content_w)).child(
                                render_assistant_turn(
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
                                    chat.composer.bar_hover == Some(ix),
                                    // 复制反馈（032 恢复）：copy_flash 点亮且未超 1.5s
                                    // （键 = 轮头 start_ix，与用户栏的 msg_ix 互不重叠）
                                    rt_view.copy_flash.is_some_and(|(cix, at)| {
                                        cix == ix && at.elapsed().as_millis() < 1500
                                    }),
                                    fork,
                                    rt_view.observer,
                                ),
                            ))
                            .into_any_element()
                    }
                    None => {
                        // 条目序 [msgs | phase 行 | spacer]（chat_list::sync 的
                        // splice 编排与之对齐）。spacer = 翻页垫片（pi-web
                        // PromptAnchorSpacer）：把「跟随位」垫到用户消息顶。
                        // phase 行 = 等待脉冲（文本流式一经出现即让位）。
                        // 槽位按槽位表走，**不**按下标大小/现值现算（同前注）。
                        match slot {
                            chat_list::RowSlot::Spacer => div()
                                .w_full()
                                .h(px(rt_view.pager.spacer_px()))
                                .into_any_element(),
                            // 槽位在就渲等待行，**不**再加 `phase_row_visible()` 门：
                            // 结构未变前该条目的高度不能变（跟尾胶水期末尾条目一
                            // 变高矮就把整屏内容推上/推下）——等待行的增减一律等
                            // sync 改结构，最多滞后 33ms，肉眼看不出。
                            chat_list::RowSlot::Phase => {
                                // 计时起点 = stream_started（AgentStart 记录、
                                // settle/abort 清除）；None 仅防御性回落。
                                let started = rt_view
                                    .stream_started
                                    .unwrap_or_else(std::time::Instant::now);
                                div()
                                    .w_full()
                                    .flex()
                                    .justify_center()
                                    .child(
                                        div()
                                            .w_full()
                                            .max_w(px(content_w))
                                            .pt(px(2.))
                                            .child(phase_row(started, t)),
                                    )
                                    .into_any_element()
                            }
                            // 结构外下标 / 缩短间隙：0 高占位
                            chat_list::RowSlot::Empty | chat_list::RowSlot::Message(_) => {
                                div().w_full().into_any_element()
                            }
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

/// Extension setWidget rows above/below the composer (ext protocol surface).
pub(crate) fn ext_widget_rows(
    chat: &Chat,
    t: &'static crate::appearance::theme::Theme,
    above: bool,
) -> Option<gpui::AnyElement> {
    let rows: Vec<gpui::AnyElement> = chat
        .aux
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
