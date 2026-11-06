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
/// 插件功能页入口钮**已撤**（2026-10-23 定夺：入口只留三横菜单，
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

mod tab_item;
mod tabs;

use self::tab_item::tab_body;
use self::tabs::tab_strip;
