//! 单个自由 tab 的外观与交互（自 titlebar.rs 外移）：content_tab 装配 +
//! tab_body 外观本体 + tab_shell 真实 tab（zed `Tab` parity）。

use super::tab_icon_size;
use super::{HEIGHT, TabDrag, TabDragGhost};
use crate::*;

/// 内容区 tab（Obsidian 式）：激活 = 凸起卡片（bg 色、顶圆角、下缘融入
/// 内容区、× 可见）；非激活 = 平铺文字。`ml` = 距分隔线/前一 tab 的间距。
pub(super) fn content_tab(
    id: &'static str,
    ix: usize,
    label: SharedString,
    active: bool,
    ml: f32,
    tab_icon: Option<&'static str>,
    badge: Option<gpui::AnyElement>,
    strike: bool,
    drag_key: TabKey,
    // 082 三期后视图由 activate_content_tab 从 panel_tabs[ix] 推导，调用
    // 方传入的 view 仅作签名兼容保留
    _view: ContentView,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    // zed `Tab::new(ix)` 同款：元素 id 必须**每个 tab 唯一**。所有文件
    // tab 共用同一个字符串 id 时，gpui 的 element_state 也是同一条
    //（pending_mouse_down / drag_listener 共用）——在任意一个 tab 上
    // 起手拖，拿到的永远是最后一个 tab 的载荷（幽灵黏在最后那个 tab 上）
    let tab_id = gpui::ElementId::from((id, ix));
    let close = cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
        // 文件 tab：脏缓冲走确认弹窗；干净直关（含内容区回退）。逻辑收拢
        // 在 editor/tabs.rs close_tab_at——与标签右键菜单 Close 同一方法
        this.close_tab_at(ix, cx);
        cx.notify();
    });
    // 入 tab 的激活走 on_click（mouse-up），zed `Tab::on_click` 同式：
    // 起手拖拽会清 clicked_state，拖一个未激活 tab 不会把它顶成激活态
    let switch = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
        // 切 tab 只改状态，不自动聚焦内容区（082 三期：终端不再抢键盘；
        // 键入目标由内容区点击决定）。与 Ctrl+←/→ 同一条路径：
        // activate_content_tab（tabs.rs）内含视图真变化时的 window.blur()
        // 截断旧焦点所有者（避免卸载元素句柄残留持焦、回切时「复活」）。
        // 终端 = 点终端本体才持焦（term_view 的 mouse_down）；文件 = 显式
        // 打开手势（树点开/eye 切源码/链接跳转）才走 pending_focus_file。
        this.activate_content_tab(ix, window, cx);
    });
    // 行尾 ×（仅激活态由 tab_shell 渲染）——脏 tab 也要能关（关时走
    // FileDirty 确认弹窗）；id 随 tab 下标唯一（同上）
    let close_x = div()
        .id(gpui::ElementId::from((tab_id.clone(), "x")))
        .flex_shrink_0()
        .size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(rgb(t.text_dim))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |ev, w, cx| {
            cx.stop_propagation();
            close(ev, w, cx);
        })
        .child(crate::ui::icon_hover("x", 12., t.text_dim))
        .into_any_element();
    // 右键 = 标签菜单（zed 移植）：Close/Others/Left/Right/All，锚点 =
    // 事件坐标
    let ctx_menu = cx.listener(move |this, ev: &gpui::MouseDownEvent, _w, cx| {
        // 禁透传：右键已消费，不再喂给下层元素/raw 监听
        cx.stop_propagation();
        this.dlg.tab_menu = Some((
            crate::TabMenuTarget::Panel(ix),
            f32::from(ev.position.x),
            f32::from(ev.position.y),
        ));
        cx.notify();
    });
    // 中键关闭（浏览器/zed 同款；归队后全 tab 统一）
    let close_mid = cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
        // 禁透传：中键已消费
        cx.stop_propagation();
        this.close_tab_at(ix, cx);
        cx.notify();
    });
    // zed pane tab 拖拽换位：on_drag 发起（ghost = tab 本体自克隆）+
    // drag_over 插入侧指示 + on_drop 落位（按稳定 key 现查 from，防拖拽
    // 中列表漂移）。会话 tab 不接线（固定段禁拖）。
    let drag = TabDrag {
        ix,
        key: drag_key.clone(),
        label: label.clone(),
        icon: tab_icon,
        // 文件 tab 的幽灵图标走文件类型图标主题（同 TreeDragGhost 两态口径）
        path: match &drag_key {
            TabKey::File(p) => Some(p.clone()),
            _ => None,
        },
        active,
        strike,
    };
    let ghost = drag.clone();
    tab_shell(
        tab_id,
        // 040-2：topbar 标签图标统一 ICON_XS(12)（2026-10-22 用户定夺；
        // 原值 15 是非法档，规范 §3 五档之外）。
        // 例外：`plug` 图形本身细长（墨迹只占 ~13/24 宽），12 档下与方形图标
        // 并排显得小一圈，光学上补到 ICON_SM(14)（2026-10-23 用户定夺，
        // 与 cmd 菜单里的 plug 同档）。
        tab_icon.map(|n| crate::ui::icon(n, tab_icon_size(n), t.text_muted).into_any_element()),
        label,
        active,
        ml,
        t,
        switch,
        Some(close_x),
        badge,
        strike,
    )
    // 抓取手型（020 项目行同款）：可拖拽 = OpenHand，不用一指禅的
    // pointer；拖拽中由 tabs_host 的 on_drag_move 覆写成 ClosedHand
    .cursor(gpui::CursorStyle::OpenHand)
    .on_mouse_down(MouseButton::Right, ctx_menu)
    .on_mouse_down(MouseButton::Middle, close_mid)
    .on_drag(drag, move |_, _, _, cx| {
        cx.new(|_| TabDragGhost {
            drag: ghost.clone(),
        })
    })
    .drag_over::<TabDrag>(move |tab, dragged_tab, _, _| {
        // zed pane.rs drag_over 逐字：高亮底 + 边去掉，源在目标左 → 目标
        // 左缘 2px（源插到目标前）；源在目标右 → 目标右缘 2px；原地只
        // 留底，不画边
        let mut styled_tab = tab.bg(rgb(t.bg_hover)).border_0();
        if ix < dragged_tab.ix {
            styled_tab = styled_tab.border_l_2();
        } else if ix > dragged_tab.ix {
            styled_tab = styled_tab.border_r_2();
        }
        styled_tab.border_color(rgb(t.accent))
    })
    .on_drop::<TabDrag>(cx.listener(move |this, drag: &TabDrag, _, cx| {
        this.move_panel_tab(&drag.key, ix, cx);
    }))
    // 禁透传（2026-11-07 用户令）：命中链止于 tab 本体——hover/点击不再
    // 漏给下方重叠的兄弟 hitbox（drop 目标/内容区）。滚轮放行保横向滚动。
    .block_mouse_except_scroll()
}

#[allow(clippy::too_many_arguments)]
/// tab 外观本体（zed `Tab` 的 render）：前置 icon + 标题 + 行尾标记/钮。
/// 交互（激活/拖拽/右键）由调用点挂：真实 tab 走 `tab_shell`，拖拽幽灵
/// 复用同一份外观（zed `DraggedTab::render` 就是重渲染那个 Tab）。
pub(super) fn tab_body(
    icon: Option<gpui::AnyElement>,
    label: SharedString,
    active: bool,
    ml: f32,
    t: &'static crate::appearance::theme::Theme,
    trailing: Option<gpui::AnyElement>,
    badge: Option<gpui::AnyElement>,
    strike: bool,
) -> gpui::Div {
    let mut tab = div()
        .ml(px(ml))
        .flex()
        .items_center()
        .gap(px(9.))
        .text_size(crate::appearance::ui_size(12.))
        .cursor_pointer()
        .when(active, |d| {
            // 连体态：bg 填充、下缘融入下方内容区（host 已 items_end 贴
            // 底）；023① 激活/背景等高；023④ 激活 tab 限宽 300px（超长
            // 省略号截断）
            d.max_w(px(300.))
                .h(px(HEIGHT - 4.))
                .mb(px(-1.))
                .bg(rgb(t.bg))
                // 上沿不画线（2026-10-10 定夺）：只留左右两道与内容区分隔
                .border_l_1()
                .border_r_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
                .rounded_tl(px(9.))
                .rounded_tr(px(9.))
                .pl(px(12.))
                // 行尾有交互钮（文件/终端 tab 的 ×，自带 20px 命中区）让位
                // 5px；无行尾钮（会话 tab，⋯ 已撤 2026-10-11）右距对齐左距
                .pr(px(if trailing.is_some() { 5. } else { 12. }))
                .text_color(rgb(t.text))
        })
        .when(!active, |d| {
            // 023④ 未激活 tab 默认缩至 120px
            d.max_w(px(120.))
                .h(px(HEIGHT - 4.))
                .mb(px(-1.))
                .pl(px(12.))
                .pr(px(12.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.text_color(rgb(t.text)))
        });
    tab = tab
        // 前置 icon（会话 tab = bot-message-square；终端/更新日志 tab = 自身
        // 图标；文件 tab 按 023 不画）。flex_shrink_0 必须显式：非激活 tab
        // 限宽 100px 溢出时默认 shrink 会把 svg 连带宽一起压扁（icon 缩成
        // 团的 bug）
        .children(icon.map(|n| div().flex_shrink_0().child(n)))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                // zed tab parity：文件已从磁盘消失（DiskState::Deleted）标删除线
                .when(strike, |d| d.line_through())
                .child(label),
        )
        // 行尾标记（023 文件 tab：脏点/冲突!）；行尾交互钮（文件/终端 tab
        // = × 关闭，会话 tab = ⋯ 菜单）仅激活态显示
        .children(badge)
        .when(active, |d| d.children(trailing));
    tab
}

/// 真实 tab = 外观本体 + 元素 id（zed `Tab::new(ix)`：**每 tab 唯一**）+
/// 点击激活（mouse-up，zed 同式）。
#[allow(clippy::too_many_arguments)]
pub(super) fn tab_shell(
    id: gpui::ElementId,
    icon: Option<gpui::AnyElement>,
    label: SharedString,
    active: bool,
    ml: f32,
    t: &'static crate::appearance::theme::Theme,
    switch: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    trailing: Option<gpui::AnyElement>,
    badge: Option<gpui::AnyElement>,
    strike: bool,
) -> gpui::Stateful<gpui::Div> {
    tab_body(icon, label, active, ml, t, trailing, badge, strike)
        .id(id)
        .on_click(switch)
}
