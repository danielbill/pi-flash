//! market 域入口菜单 —— topbar「菜单」三横钮的浮层（040-1 / 040-2 共用）。
//!
//! 与 psp 的各种菜单同层：渲染挂 `shell/render.rs` 根，点卡片外 / ESC 关。
//! **菜单是多入口的壳**：哪一个市场页开着，对应项才出现（各自 dev flag 管），
//! 壳本身在 `market_menu_enabled()`（任一开）为真时由 titlebar 渲染按钮。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;

#[allow(unused_imports)]
use gpui::MouseButton as _MouseButtonKeepImport;

/// 菜单浮层。
pub(crate) fn menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = chat.dlg.market_menu.expect("checked by caller");
    let edges = crate::ui::popup::PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.market_menu = None;
            cx.notify();
        });
    });
    // 040-1「Pi扩展市场」已退役（2026-10-22 用户定夺）：菜单只留原生市场。
    // 旧页代码保留（open_pi_ext_market 仍可从自动化口进入），不再上菜单。
    // 图标档：`plug` 是细长形，12 档下如图标并与方形图标并排会显小→给 14
    // （与 topbar 同名标签同档，见 titlebar::tab_icon_size）；puzzle 保持 12。
    // 微信通信（060；dev-flag `wx` 默认关 = 发版不可见）：开态才渲染首项。
    // 2026-10-23 定夺：入口只留三横菜单（topbar/statusbar 图标全撤）。
    let mut card = crate::function_panel::psp_overlays::menu_card(edges, t);
    if crate::services::workspace::wx_enabled() {
        let weak_wx = weak.clone();
        card = card.child(
            crate::function_panel::psp_overlays::menu_item_sized(
                "menu-wx",
                "smartphone",
                14.,
                tr("remote.panel_title"),
                false,
                false,
            )
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_wx.update(cx, |c, cx| c.open_wx(cx));
            }),
        );
    }
    let weak_item = weak.clone();
    card = card.child(
        crate::function_panel::psp_overlays::menu_item_sized(
            "market-menu-pi-native",
            "plug",
            14.,
            tr("market.native_tab"),
            false,
            false,
        )
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak_item.update(cx, |c, cx| c.open_pi_native_market(cx));
        }),
    );
    // 090-4 PF 插件市场（主线功能，常驻菜单）
    let weak_pf = weak.clone();
    card = card.child(
        crate::function_panel::psp_overlays::menu_item_sized(
            "market-menu-pf",
            "puzzle",
            12.,
            tr("market.pf.tab"),
            false,
            false,
        )
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak_pf.update(cx, |c, cx| c.open_pf_market(cx));
        }),
    );
    // 插件 topbar_menu 条目（契约 v2.5，eb6）：读各插件清单自动生成，
    // 零插件名（宿主不认识订阅者，红线）。点击语义与 status_bar tab 同构
    // （按 panels[].dock 解释形态）；禁用插件不出条目。
    let items: Vec<(usize, String, gpui::SharedString)> = chat
        .plugins
        .plugins
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.disabled && p.manifest.topbar_menu.is_some())
        .filter_map(|(pix, p)| {
            let icon = crate::pf_plugin::topbar_menu_icon_path(p)?;
            let title = p.manifest.topbar_menu.as_ref()?.title_zh.clone();
            Some((pix, title, icon))
        })
        .collect();
    for (pix, title, icon) in items {
        let weak_item = weak.clone();
        card = card.child(
            crate::function_panel::psp_overlays::menu_item_asset(
                format!("plugin-topbar-{}", pix),
                Some(icon.to_string()),
                14.,
                &title,
                false,
                false,
            )
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_item.update(cx, |c, cx| {
                    // 清单 dock 归宿主解释（契约 v2.4）：tab 形态开预览页签，
                    // 否则开左列 dock 面板（插件不感知形态）
                    if c.plugin_dock_is_tab(pix) {
                        c.open_plugin_tab(pix, cx);
                    } else {
                        c.open_plugin_dock(pix, cx);
                    }
                });
            }),
        );
    }
    layer.child(card).into_any_element()
}
