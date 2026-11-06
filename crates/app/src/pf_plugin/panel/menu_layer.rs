//! 组件树 gpui 渲染层·格子右键菜单层（01 §4.2）：挂载点是 `psp-overlays`
//! （与文件树右键菜单同一处），卡座与行样式复用 `menu_card` /
//! `menu_item_asset`——**样式只有一份**。

use super::*;

/// 插件格子右键菜单层（`psp-overlays` 消费）。
pub(crate) fn cell_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let Some(ix) = chat
        .plugins
        .plugins
        .iter()
        .position(|p| p.cell_menu.is_some())
    else {
        return div().into_any_element();
    };
    let Some(menu) = chat.plugins.plugins[ix].cell_menu.clone() else {
        return div().into_any_element();
    };
    let plugin_id = chat.plugins.plugins[ix].manifest.id.clone();
    let weak = cx.entity().downgrade();
    // 与 psp 菜单同参：向下弹让 12px，左右象限决定横向方向
    let edges = crate::ui::popup::PopupEdges::around((menu.x, menu.y), vp, 0., 12.);
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, move |c, cx| {
        if let Some(e) = c.plugins.plugins.get_mut(ix) {
            e.cell_menu = None;
            cx.notify();
        }
    });

    let mut card = crate::function_panel::psp_overlays::menu_card(edges, t);
    for (i, item) in menu.items.iter().enumerate() {
        let cmd_id = item.id.clone();
        let action = item.action.clone();
        let path = menu.path.clone();
        let label = item.title.clone();
        let danger = item.danger;
        let asset = (!item.icon.is_empty()).then(|| icon_asset(&item.icon, &plugin_id));
        let weak_row = weak.clone();
        let row = crate::function_panel::psp_overlays::menu_item_asset(
            SharedString::from(format!("pf-menu-{ix}-{i}")),
            asset,
            14.,
            &label,
            danger,
            false,
        )
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            let (cmd_id, action, path) = (cmd_id.clone(), action.clone(), path.clone());
            let _ = weak_row.update(cx, |c, cx| {
                if let Some(entry) = c.plugins.plugins.get_mut(ix) {
                    entry.cell_menu = None;
                }
                match action {
                    Some(a) => c.plugin_dispatch(ix, vec![to_js_action(a)], window, cx),
                    None => c.plugin_command_fire(ix, &cmd_id, &path, window, cx),
                }
                cx.notify();
            });
        });
        card = card.child(row);
    }
    layer.child(card).into_any_element()
}

/// 宿主动作白名单 → JS 动作（按钮/菜单项共用）。
pub(super) fn to_js_action(a: CellAction) -> JsAction {
    match a {
        CellAction::Run(p) => JsAction::Run(p),
        CellAction::Edit(p) => JsAction::Edit(p),
        CellAction::Reveal(p) => JsAction::RevealInExplorer(p),
        CellAction::Toast(m) => JsAction::Toast(m),
        CellAction::OpenUrl(u) => JsAction::OpenUrl(u),
        CellAction::OpenFolder(p) => JsAction::OpenFolder(p),
        CellAction::NewSession => JsAction::NewSession,
        CellAction::AskPi(t) => JsAction::AskPi(t),
        CellAction::Unsupported(name) => JsAction::Toast(format!("宿主不支持的动作：{name}")),
    }
}
