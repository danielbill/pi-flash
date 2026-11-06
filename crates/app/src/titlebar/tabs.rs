//! 中段标签栏（016，自 titlebar.rs 外移）：置顶会话 tab + 自由标签区
//! （终端/文件，023 规则），滚动/拖拽/右键语义见各函数。

use super::TabDrag;
use super::tab_item::{content_tab, tab_shell};
use crate::*;

/// 中段标签栏（016）：置顶会话 tab（固定段，第一位，距条左缘 10px）+
/// 自由标签区（终端/文件，023 规则：脏点/冲突标记在 tab 行尾）。
///
/// 标签区伸展到最小化钮左侧 20px（flex_1 + min_w_0；016 2026-10-14 定案
/// 撤 75% 限宽，回 023 的 Zed parity）；自由标签区独立横向滑动，会话 tab
/// 不参与滚动（2026-10-14 bug 修复：超多标签时旧实现把会话 tab 一起滚
/// 走——它必须恒定可见）；右走流内 20px spacer（滚到底也有留白）。
pub(super) fn tab_strip(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let t = T();
    let mut strip = div()
        .id("topbar-tabs")
        .flex_1()
        .min_w_0()
        .pl(px(10.))
        .h_full()
        .flex()
        .items_end();
    // 固定段：置顶会话 tab 条（滚动区外，永不滚走）
    strip = strip.child(session_tabs(chat, cx));
    // 插件功能页 tab 已归队 PanelTab（跟自由区一起渲染/拖拽/右键）——
    // 它曾作为固定段特例活在会话 tab 旁，代价是右键菜单/拖拽/批量关闭
    // 全套缺失（同规则见 shell/types.rs::PanelTab::Wx 注释）。
    // 自由标签区：独立横向滑动，起点紧贴会话 tab（tab 自带 ml 4px）。
    // `track_scroll` 挂上句柄（zed tab bar 同款）后 gpui 每帧记 child
    // bounds → `ScrollHandle::scroll_to_item(ix)` 才能把激活 tab 滚进视口
    //（activate_panel_tab 调）；子元素顺序 = panel_tabs 下标，末尾 drop
    // 目标不占 tab 下标，故不需 zed 的 `- pinned_tab_count` 偏移。
    let mut tabs_host = div()
        .id("topbar-tabs-scroll")
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .items_end()
        .overflow_x_scroll()
        .track_scroll(&chat.layout.tabs_scroll)
        // 拖拽全程握紧手（020 项目行同款）：AnyDrag.cursor_style 取源行样式
        //（OpenHand），这里覆写成 ClosedHand
        .on_drag_move::<TabDrag>(move |_, window, cx| {
            cx.set_active_drag_cursor_style(gpui::CursorStyle::ClosedHand, window);
        });
    for (ix, tab) in chat.ws.panel_tabs.iter().enumerate() {
        let (label, path, is_file, tab_icon, tab_view): (
            SharedString,
            Option<PathBuf>,
            bool,
            Option<&'static str>,
            ContentView,
        ) = match tab {
            crate::PanelTab::Term(id) => {
                let title = chat
                    .ws
                    .terminals
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| {
                        t.cwd
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                (
                    title.into(),
                    None,
                    false,
                    Some("icon-terminal-solid"),
                    ContentView::Term,
                )
            }
            crate::PanelTab::File(p) => (
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into())
                    .into(),
                Some(p.clone()),
                true,
                None,
                ContentView::File,
            ),
            // 081 更新日志 tab（新版本首启自动打开）
            crate::PanelTab::Changelog => (
                tr("about.changelog").into(),
                None,
                false,
                Some("history"),
                ContentView::Changelog,
            ),
            // 040-2 Pi 原生扩展市场（pi.dev 目录页移植）
            crate::PanelTab::PiNativeMarket => (
                tr("market.native_tab").into(),
                None,
                false,
                Some("plug"),
                ContentView::PiNativeMarket,
            ),
            // 090-4 PF 插件市场（内置索引 + 台账）
            crate::PanelTab::PfMarket => (
                tr("market.pf.tab").into(),
                None,
                false,
                Some("puzzle"),
                ContentView::PfMarket,
            ),
            // PF 插件面板（beads 360）：标题 = 清单 title（随语言），图标 puzzle
            // 083 项目搜索（Zed 同级 tab：ProjectSearchView 就是 pane item）
            crate::PanelTab::Search => (
                tr("search.tab").into(),
                None,
                false,
                Some("search"),
                ContentView::Search,
            ),
            // 会话查询（同版式，只查询）
            crate::PanelTab::SessionSearch => (
                tr("session_search.tab").into(),
                None,
                false,
                Some("search"),
                ContentView::SessionSearch,
            ),
            // 090 插件预览页签（契约 v2.4 dock="tab"）：标题 = 清单 title
            //（随语言，宿主解释清单；零插件名硬编码），图标通用 puzzle
            crate::PanelTab::Plugin { pkg, .. } => {
                let label = chat
                    .plugins
                    .plugins
                    .iter()
                    .find(|e| e.manifest.pkg == *pkg)
                    .map(|e| e.manifest.title_zh.clone())
                    .unwrap_or_else(|| pkg.clone());
                (
                    label.into(),
                    None,
                    false,
                    Some("puzzle"),
                    ContentView::Plugin,
                )
            }
            // 子代理观察窗（044-0 验收②）：标题 = descriptor 任务名快照
            //（open_session 写入；兜底「子代理」），复用 Chat 视图
            crate::PanelTab::Observer { key } => {
                let label = chat
                    .pool
                    .tab_labels
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| tr("subagent.tab").to_string());
                (
                    label.into(),
                    None,
                    false,
                    // subagent 统一 bot icon（044-0 验收③）
                    Some("bot"),
                    ContentView::Chat,
                )
            }
        };
        // 文件 tab 行尾标记（023）：冲突 ! > 脏点。zed parity（DiskState::
        // Deleted）：文件已从磁盘消失不标 !——删除线是唯一标记，脏点照常
        // （脏的已删缓冲仍显示为已修改）
        let deleted = path
            .as_ref()
            .and_then(|p| chat.ws.file_cache.get(p))
            .is_some_and(|f| f.conflict == Some(crate::FileConflict::Deleted));
        let badge: Option<gpui::AnyElement> = path.as_ref().and_then(|p| {
            let f = chat.ws.file_cache.get(p)?;
            Some(
                if f.conflict == Some(crate::FileConflict::Changed) {
                    div()
                        .flex_shrink_0()
                        .text_size(crate::appearance::ui_size(11.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(0xf87171))
                        .child(tr("status.alert"))
                } else if f.dirty {
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(rgb(0xe0a562))
                } else {
                    return None;
                }
                .into_any_element(),
            )
        });
        let active = chat
            .ws
            .panel_tabs
            .get(ix)
            .is_some_and(|t| t.owns_view(chat.layout.content_view))
            && chat.ws.active_panel_tab == Some(ix);
        tabs_host = tabs_host.child(content_tab(
            if is_file { "ctab-file" } else { "ctab-term" },
            ix,
            label,
            active,
            4.,
            tab_icon,
            badge,
            deleted,
            tab.key(),
            tab_view,
            cx,
        ));
    }
    // zed `render_tab_bar_drop_target` 同款：标签右侧余量整块是 drop
    // 目标（落 = 移到末尾），拖过时底高亮；没有它拖到空白处毫无反馈。
    // 空 child 是 zed 的 HACK 注释同款：没有内容时 drop 目标不画出来
    tabs_host = tabs_host.child(
        div()
            .id("topbar-tabs-drop")
            .min_w(px(24.))
            .flex_grow()
            .h_full()
            .child("")
            .drag_over::<TabDrag>(move |bar, _, _, _| bar.bg(rgb(t.bg_hover)))
            .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
                // zed: handle_tab_drop(dragged_tab, items.len())——落末尾
                let last = this.ws.panel_tabs.len();
                this.move_panel_tab(&drag.key, last, cx);
            })),
    );
    strip.child(tabs_host).into_any_element()
}

/// 置顶会话 tab（016）：中段第一位固定段（距条左缘 10px，不随自由标签
/// 滚动——2026-10-14 定案），无 × 无 ⋯（2026-10-11
/// 定夺：⋯ 菜单三入口改到会话 tab 下方的功能栏，见 session::session_fn_bar），
/// label = 会话标题（≤30 字截断不变），前置 bot-message-square icon。激活 =
/// 会话视图（Obsidian 卡片融底）；点击回会话视图。
fn session_tabs(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let t = T();
    // 固定段 = **一枚**主会话 tab（044-0 验收②：新会话/主会话永远第一位，
    // 子会话观察窗进自由区 panel_tabs 不抢这里）。标题 = primary 快照
    // （switch_to 刷新；空闲退役后 runtime 不在池仍有快照），无 primary
    //（启动空档/被删）→「新会话」。高亮 = chat 前台且自由区无激活 tab。
    let primary = chat.pool.primary_key.clone();
    let title: SharedString = chat
        .pool
        .tab_labels
        .get(&primary)
        .cloned()
        .or_else(|| (!primary.is_empty()).then(|| chat.session_title_of(&primary, cx)))
        .unwrap_or_else(|| tr("session.new").to_string())
        .into();
    let active =
        chat.layout.content_view == ContentView::Chat && chat.ws.active_panel_tab.is_none();
    let switch = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
        this.activate_primary_tab(cx);
    });
    // 右键 = 标签菜单（zed 移植）：会话 tab 恒存不可关，菜单无 Close、
    // Close Left 恒灰（见 psp_overlays::tab_menu_layer）
    let ctx_menu = cx.listener(move |this, ev: &gpui::MouseDownEvent, _w, cx| {
        this.dlg.tab_menu = Some((
            crate::TabMenuTarget::Session,
            f32::from(ev.position.x),
            f32::from(ev.position.y),
        ));
        cx.notify();
    });
    tab_shell(
        "ctab-session".into(),
        Some(crate::ui::icon("bot-message-square", 12., t.text_muted).into_any_element()),
        title,
        active,
        0.,
        t,
        switch,
        None,
        None,
        false,
    )
    .on_mouse_down(MouseButton::Right, ctx_menu)
    .into_any_element()
}
