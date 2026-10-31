//! 全 app 统一菜单原语（090 重构自 function_panel/psp_overlays.rs 上移）：
//! 文件树右键、tab 菜单、编辑区菜单、市场菜单、插件面板菜单共用同一形制
//! （同一卡座 [`menu_card`] + 同一行样式 [`menu_item`] 族）。
//!
//! 定位统一走 `ui::popup::PopupEdges`；浮层开合收口在 [`menu_layer`]。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::appearance::theme::Theme;
use crate::ui::popup::PopupEdges;

use crate::appearance::theme::theme as T;

use crate::ui::icon;

// ---------------------------------------------------------------------------
// 卡座与菜单条
// ---------------------------------------------------------------------------

/// 统一菜单卡座：absolute 弹层基座，贴边由统一算法的 [`PopupEdges`] 决定。
pub(crate) fn menu_card(edges: PopupEdges, t: &'static Theme) -> gpui::Div {
    edges.apply(
        div()
            .absolute()
            .min_w(px(200.))
            .p(px(4.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
            .rounded(px(8.))
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            }),
    )
}

pub(crate) fn menu_item<S: Into<SharedString>>(
    id: S,
    icon_name: &'static str,
    label: &str,
    danger: bool,
    checked: bool,
) -> gpui::Stateful<gpui::Div> {
    menu_item_sized(id, icon_name, 14., label, danger, checked)
}

/// 带图标尺寸的菜单条（`menu_item` 的通用体）。
///
/// **图标档位**：菜单默认 14（`ICON_SM`）；市场菜单传 12（`ICON_XS`）——
/// 菜单项的图标与 topbar 同名标签的图标（040-2 定夺 = 12）必须同尺寸，
/// 否则同一个 plug 在标签里一个样、在菜单里一个样（2026-10-23 用户定夺）。
pub(crate) fn menu_item_sized<S: Into<SharedString>>(
    id: S,
    icon_name: &'static str,
    icon_size: f32,
    label: &str,
    danger: bool,
    checked: bool,
) -> gpui::Stateful<gpui::Div> {
    menu_item_asset(
        id,
        Some(format!("icons/{icon_name}.svg")),
        icon_size,
        label,
        danger,
        checked,
    )
}

/// 菜单条（**任意资产路径**）：内置图标走 `icons/x.svg`，插件图标走
/// `plugins/<id>/assets/x.svg`。样式与 `menu_item` 同源——插件面板的右键菜单
/// 必须和文件树右键菜单长得一样（同一卡座 `menu_card` + 同一行样式）。
pub(crate) fn menu_item_asset<S: Into<SharedString>>(
    id: S,
    asset: Option<String>,
    icon_size: f32,
    label: &str,
    danger: bool,
    checked: bool,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(7.))
        .rounded(px(6.))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(if danger { rgb(t.danger) } else { rgb(t.text) })
        .cursor_pointer()
        .hover(|s| {
            if danger {
                s.bg(gpui::rgba(crate::appearance::theme::danger_wash(t)))
            } else {
                s.bg(rgb(t.bg_hover))
            }
        })
        .children(asset.map(|a| {
            gpui::svg()
                .path(SharedString::from(a))
                .text_color(rgb(if danger { t.danger } else { t.text_muted }))
                .size(px(icon_size))
                .into_any_element()
        }))
        .child(div().flex_1().child(SharedString::from(label.to_string())))
        .children(checked.then(|| icon("check", 12., t.accent)))
}

/// 菜单分组分隔线（§5.6：1px t.border）。
pub(crate) fn menu_sep() -> gpui::Div {
    div()
        .h(px(1.))
        .my(px(4.))
        .bg(gpui::rgba(crate::appearance::theme::border_alpha(
            T(),
            0x66,
        )))
}

// ---------------------------------------------------------------------------
// 浮层开合收口
// ---------------------------------------------------------------------------

/// 菜单浮层基座：`overlay::layer(false, focus)` + 点外/ESC 收菜单槽位 + notify。
/// `clear` 只负责写「哪个槽位 = None」（或等价的取消动作）。
///
/// 「layer + WeakEntity + 收槽位」三件套样板（原散布 13 处）的唯一本体；
/// 返回 `Div`，调用方继续 `.child(menu_card(...))`。
pub(crate) fn menu_layer(
    focus: Option<&gpui::FocusHandle>,
    weak: &gpui::WeakEntity<crate::Chat>,
    clear: impl Fn(&mut crate::Chat, &mut gpui::Context<crate::Chat>) + 'static,
) -> gpui::Div {
    let weak = weak.clone();
    // Rc 包一层让外层闭包满足 overlay::layer 的 Clone 约束（调用方的
    // clear 闭包无需 Clone）
    let clear = std::rc::Rc::new(clear);
    crate::ui::overlay::layer(false, focus, move |_w, cx| {
        let _ = weak.update(cx, |c, cx| {
            clear(c, cx);
            cx.notify();
        });
    })
}
