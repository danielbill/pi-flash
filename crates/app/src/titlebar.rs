//! topbar 三段 (016): 左段在 panel-col 内（收放钮；手机遥控钮已隐藏——
//! 2026-10-10 定，060 管线保留），右段在 content-col 内（中段标签栏 +
//! 窗口控制钮）。中段标签栏 =
//! 置顶会话 tab（第一位，无 ×；行尾 ⋯ 菜单 2026-10-11 撤销，三入口改
//! session tab 下方功能栏 session::session_fn_bar）+ 自由标签区（终端/文件，
//! 023 规则），
//! 激活 tab Obsidian 卡片融底、条上不画横线。设置钮下放 018 状态栏、+ 已删
//! （016 定案）。整条是客户区自绘标题栏——空段挂 `WindowControlArea::Drag`
//! 命中盒，三个窗口钮注册 Min/Max/Close 命中盒（zed platform_title_bar
//! parity）。收起态面板全隐，收放钮跳到右段起点（竖线镜像位）。

use gpui::WindowControlArea;
use gpui::{MouseButton, SharedString, Window, div, prelude::*, px, rgb};
use std::path::PathBuf;

use crate::Chat;
use crate::ContentView;
use crate::TabKey;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::ui::tokens::{align, space};

/// tab 拖拽载荷（zed `DraggedTab` 单 pane 版）：`ix` 供悬停插入侧判定
/// （zed drag_over 的 `ix < dragged_tab.ix` 同式）；`key` 供 drop 落位
/// （稳定键，防中途列表漂移错位，zed 用 item id 同语义）；其余 = 幽灵
/// 渲染所需的 tab 快照（zed 的 DraggedTab 持 item 句柄，幽灵现场重渲染
/// 同一个 Tab）。TypeId 与树拖拽 TreeDrag 互不串扰。会话 tab 不接
/// on_drag（固定段禁拖，016 定案）。
#[derive(Clone)]
pub(crate) struct TabDrag {
    pub(crate) ix: usize,
    pub(crate) key: TabKey,
    pub(crate) label: SharedString,
    /// 终端/更新日志 tab 的命名图标（`ui::icon` 口径）
    pub(crate) icon: Option<&'static str>,
    /// 文件 tab 的路径：幽灵按**文件类型图标主题**取图（同 `TreeDragGhost`
    /// 的 dir/file 两态口径），tab 本体则按 023 不画 icon
    pub(crate) path: Option<PathBuf>,
    pub(crate) active: bool,
    pub(crate) strike: bool,
}

/// tab 拖拽幽灵：zed `DraggedTab::render` 同款——把被拖的 tab 本体
/// （`Tab::new("").toggle_state(is_active).children(icon).child(label)`）
/// 原样画在光标处，不另造小签。 icon 取值同文件树拖拽幽灵（`TreeDragGhost`）：
/// 文件 = `file_icons` 类型图标，终端/更新日志 = tab 自身的命名图标。
pub(crate) struct TabDragGhost {
    pub(crate) drag: TabDrag,
}

impl gpui::Render for TabDragGhost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let t = T();
        let active = self.drag.active;
        tab_body(
            drag_icon(&self.drag, 12., t.text_muted),
            self.drag.label.clone(),
            active,
            0.,
            t,
            None,
            None,
            self.drag.strike,
        )
        // 幽灵浮在内容之上：非激活 tab 在条上是平铺文字（底 = 条色），
        // 悬浮态补条底色，与 zed tab_inactive_background 同理
        .when(!active, |d| d.bg(rgb(t.chrome)))
        .shadow_md()
    }
}

/// topbar 标签图标档位：默认 ICON_XS(12)；`plug` 参照 ICON_SM(14)（光学尺寸
/// 补偿：该图形墨迹只占 ~13/24 宽，12 档下和方形图标并排显小）。
/// 菜单项同名图标走同一档（`market::menu`），两处必须一致。
fn tab_icon_size(name: &str) -> f32 {
    match name {
        "plug" => 14.,
        _ => 12.,
    }
}

/// 拖拽幽灵的图标元素：文件 tab → 文件类型图标（`ui::icon_path` 口径）；
/// 终端/更新日志 → tab 自身的命名图标（`ui::icon`）。
fn drag_icon(drag: &TabDrag, size: f32, color: u32) -> Option<gpui::AnyElement> {
    if let Some(path) = drag.path.as_deref() {
        return Some(crate::ui::icon_path(
            crate::services::file_icons::get_icon(path),
            size,
            color,
        ));
    }
    drag.icon
        .map(|n| crate::ui::icon(n, size, color).into_any_element())
}

/// topbar 高（两段同高；窗口控制钮高度跟随；2026-10-10 定夺 36→38，
/// 2026-10-13 定夺 38→40 与 statusbar 等高）。
pub(crate) const HEIGHT: f32 = 40.;

/// Caption-button glyph font (Win11; MDL2 covers Win10).
pub const CAPTION_FONT: &str = "Segoe Fluent Icons";

/// 左段（015 功能面板侧）：收放钮贴左 = `tokens::align::TOGGLE_ML`（图标
/// 视觉线 ICON_LINE 与 psp 列表图标对齐，2026-10-13 定稿）+ 拖拽填充。
/// 微信手机钮**已撤**（2026-10-23 定夺：入口只留三横菜单【微信通信】，
/// 内容区全屏页），旧钮走 git 历史找回。
pub(crate) fn topbar_l(_chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();
    let mut bar = div()
        .id("topbar-l")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(t.chrome))
        .child(div().ml(px(align::TOGGLE_ML)).child(icon_btn(
            "panes-toggle",
            "panel-left",
            tr("titlebar.toggle_sidebar"),
            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                this.toggle_panes(cx);
            }),
        )));
    // 040-1 「菜单」三横钮（紧贴收放钮右侧）：目前只有「Pi扩展市场」
    // 一项，浮层渲染挂 render.rs 根（dlg.market_menu = 按钮锚点）。
    // **本版隐藏**：市场页还没收拾干净，入口走 dev-flags
    // `pi_ext_market` 开关（默认关 = 发版不可见），开才渲染这个钮。
    if crate::services::workspace::market_menu_enabled() {
        bar = bar.child(div().ml(px(space::SP2)).child(icon_btn_sized(
            "market-menu",
            "menu",
            tr("titlebar.menu"),
            20.,
            cx.listener(|this, ev: &gpui::MouseDownEvent, _w, cx| {
                this.dlg.market_menu = Some((f32::from(ev.position.x), f32::from(ev.position.y)));
                cx.notify();
            }),
        )));
    }
    bar
        // drag filler: the empty middle is the drag region (HTCAPTION —
        // platform handles move + double-click-zoom)
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
}

/// 右段：中段标签栏 + 窗口控制。
pub(crate) fn topbar_r(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let maximized = window.is_maximized();

    let mut bar = div()
        .id("topbar-r")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .relative()
        .flex()
        .items_center()
        .bg(rgb(t.chrome));

    // 收起态：收放钮跳到右段起点（4px 等距，竖线镜像位）
    if chat.layout.panes_hidden {
        bar = bar
            .child(div().ml(px(4.)).mr(px(2.)).child(icon_btn(
                "panes-toggle-r",
                "panel-left",
                tr("titlebar.expand_sidebar"),
                cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.toggle_panes(cx);
                }),
            )))
            .child(
                div()
                    .w(px(1.))
                    .h(px(18.))
                    .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
                    .mx(px(4.)),
            );
    }

    // 016 中段标签栏：置顶会话 tab + 自由标签区（终端/文件），激活 tab
    // 卡片融底（Obsidian 式，条上不画横线）
    bar = bar.child(tab_strip(chat, cx));

    // 标签区右缘到最小化钮左缘固定 20px（016 2026-10-14 定案），仍是
    // 窗口拖拽命中区
    bar = bar.child(
        div()
            .w(px(20.))
            .flex_shrink_0()
            .h_full()
            .window_control_area(WindowControlArea::Drag),
    );

    // 016 右段：仅窗口控制钮（设置下放 018 状态栏、+ 删除）
    // v55: min/max switch to SVG icons via HoverIcon; close keeps red-bg exemption
    for (area, icon_name) in [
        (WindowControlArea::Min, "minus"),
        (
            WindowControlArea::Max,
            if maximized { "restore" } else { "square" },
        ),
    ] {
        bar = bar.child(caption_button(area, icon_name, t));
    }
    bar.child(caption_button(WindowControlArea::Close, "\u{E8BB}", t))
}

/// 30×30 图标钮（topbar 通用；hover chrome-hover）。钮盒/图标走 align 令牌。
fn icon_btn(
    id: &'static str,
    icon_name: &'static str,
    _tip: &'static str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    icon_btn_sized(id, icon_name, _tip, align::TOGGLE_ICON, handler)
}

/// 同 [`icon_btn`]，图标尺寸可指定（040-2：菜单三横是细线图形，18 档视觉
/// 质量比收放钮的 18×18 实心盒小一圈——用户定夺升 20 光学对齐）。
#[allow(clippy::too_many_arguments)]
fn icon_btn_sized(
    id: &'static str,
    icon_name: &'static str,
    _tip: &'static str,
    icon_size: f32,
    handler: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    let t = T();
    div()
        .id(id)
        .size(px(align::TOGGLE_BTN))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, handler)
        .child(crate::ui::icon_hover(icon_name, icon_size, t.text_muted))
}

/// 内容区 tab（Obsidian 式）：激活 = 凸起卡片（bg 色、顶圆角、下缘融入
/// 内容区、× 可见）；非激活 = 平铺文字。`ml` = 距分隔线/前一 tab 的间距。

/// 中段标签栏（016）：置顶会话 tab（固定段，第一位，距条左缘 10px）+
/// 自由标签区（终端/文件，023 规则：脏点/冲突标记在 tab 行尾）。
///
/// 标签区伸展到最小化钮左侧 20px（flex_1 + min_w_0；016 2026-10-14 定案
/// 撤 75% 限宽，回 023 的 Zed parity）；自由标签区独立横向滑动，会话 tab
/// 不参与滚动（2026-10-14 bug 修复：超多标签时旧实现把会话 tab 一起滚
/// 走——它必须恒定可见）；右走流内 20px spacer（滚到底也有留白）。
fn tab_strip(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let t = T();
    let mut strip = div()
        .id("topbar-tabs")
        .flex_1()
        .min_w_0()
        .pl(px(10.))
        .h_full()
        .flex()
        .items_end();
    // 固定段：置顶会话 tab（滚动区外，永不滚走）
    strip = strip.child(session_tab(chat, cx));
    // 固定段：微信线 tab（060 单开 tab，2026-10-24 定夺）——微信线会话
    // 自占一格，不占用第一格会话 tab
    if chat.aux.remote.wx_session.is_some() {
        strip = strip.child(wx_tab(chat, cx));
    }
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
            // 040-1 Pi 扩展市场标准页
            crate::PanelTab::PiExtMarket => (
                tr("market.tab").into(),
                None,
                false,
                Some("menu"),
                ContentView::PiExtMarket,
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
        let active = chat.layout.content_view == tab_view && chat.ws.active_panel_tab == Some(ix);
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
fn session_tab(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let t = T();
    // 微信线是单开 tab（2026-10-24 定夺）：查看微信时第一格不被占用——
    // 继续显示桌面会话，点击回到它
    let viewing_wx = chat.aux.remote.wx_session.as_deref() == Some(chat.pool.active_key.as_str());
    let desktop_key = if viewing_wx {
        chat.aux.remote.desktop_key.clone()
    } else {
        None
    };
    let active = chat.layout.content_view == ContentView::Chat && !viewing_wx;
    let title: SharedString = match &desktop_key {
        Some(k) => chat.session_title_of(k, cx).into(),
        None => chat.session_title(cx).into(),
    };
    let switch_desktop = desktop_key.clone();
    let switch = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
        // 查看微信时点击第一格 = 回桌面会话（而非空操作地切视图）
        if let Some(k) = &switch_desktop {
            if this.pool.active_key != *k {
                if let Some(rt) = this.pool.runtimes.get(k).cloned() {
                    this.switch_to(rt, cx);
                    return;
                }
            }
        }
        this.set_content_view(crate::ContentView::Chat);
        cx.notify();
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

/// 微信线 tab（060 单开 tab）：紧随会话 tab 的固定段，label = 微信通信，
/// 无 × 无右键菜单（线常驻，关停走操作栏开关）。点击 = `open_wx`——激活
/// 微信线会话；桌面活跃会话由 `switch_to` 记入 `desktop_key`，不被顶走。
fn wx_tab(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let t = T();
    let active = chat.layout.content_view == ContentView::Chat
        && chat.aux.remote.wx_session.as_deref() == Some(chat.pool.active_key.as_str());
    let title: SharedString = tr("remote.panel_title").to_string().into();
    let switch = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
        this.open_wx(cx);
    });
    tab_shell(
        "ctab-wx".into(),
        Some(crate::ui::icon("smartphone", 12., t.text_muted).into_any_element()),
        title,
        active,
        0.,
        t,
        switch,
        None,
        None,
        false,
    )
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn content_tab(
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
        this.dlg.tab_menu = Some((
            crate::TabMenuTarget::Panel(ix),
            f32::from(ev.position.x),
            f32::from(ev.position.y),
        ));
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
}

#[allow(clippy::too_many_arguments)]
/// tab 外观本体（zed `Tab` 的 render）：前置 icon + 标题 + 行尾标记/钮。
/// 交互（激活/拖拽/右键）由调用点挂：真实 tab 走 `tab_shell`，拖拽幽灵
/// 复用同一份外观（zed `DraggedTab::render` 就是重渲染那个 Tab）。
fn tab_body(
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
fn tab_shell(
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

fn caption_button(
    area: WindowControlArea,
    icon_name: &'static str,
    t: &crate::appearance::theme::Theme,
) -> impl gpui::IntoElement {
    // 关闭钮豁免：红底白字（v54 语义，glyph 渲染）；min/max 走 HoverIcon
    // 统一动效（无底色，图标自身上抬+放大，v55 全局规格）
    if area == WindowControlArea::Close {
        return div()
            .id(SharedString::from(format!("wb-{}", area as u8)))
            .w(px(42.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .font_family(CAPTION_FONT)
            // §2：随界面字号缩放（原 text_sm 裸字号；默认档下与旧 14px 等值）
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_muted))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(t.danger_hover)).text_color(rgb(0xffffff)))
            .window_control_area(area)
            .child(SharedString::from("\u{E8BB}"))
            .into_any_element();
    }
    div()
        .id(SharedString::from(format!("wb-{}", area as u8)))
        .w(px(42.))
        .h(px(HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(t.text)))
        .window_control_area(area)
        .child(crate::ui::icon_hover(icon_name, 13., t.text_muted))
        .into_any_element()
}
