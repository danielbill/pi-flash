//! psp 终端右键菜单层：复制 / 粘贴 / 全选，动作作用于右键那一个终端 tab
//! （不依赖 active_terminal），完成后焦点还给终端。自 psp_overlays.rs
//! 切出（itlq.12 超规拆分，纯结构搬移）。

use gpui::{MouseButton, SharedString, prelude::*};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::popup::PopupEdges;

use super::psp_overlays::{menu_card, menu_item};

/// 编辑区右键菜单（2026-10-11 用户定案）：编辑块（复制/粘贴/剪切/全选）
/// + 文件动作块（由 editor/kind.rs 的 FileTypeHandler 声明：浏览器/预览/
/// 资源管理器/终端 + 改名/删除，本渲染器零类型特判），
/// 样式与文件树菜单同套（menu_card/menu_item/menu_sep）。编辑四项
/// 先把焦点还给编辑器再派发 Input 动作（菜单打开不改变焦点归属，
/// 但点击可能让焦点漂移，保险起见每次归位）。
/// 终端右键菜单（复制 / 粘贴 / 全选）：动作作用于右键那一个终端 tab
/// （不依赖 active_terminal），完成后焦点还给终端——菜单是终端的附属
/// UI，收起后键盘要能接着用。
pub(crate) fn term_menu_layer(
    chat: &mut Chat,
    tab_id: usize,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let has_sel = chat
        .ws
        .terminals
        .iter()
        .find(|t| t.id == tab_id)
        .is_some_and(|t| t.selection.is_some());
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.term_menu = None;
    });
    let card = menu_card(edges, t)
        .child(term_menu_item(
            &weak,
            "tm-copy",
            "copy",
            tr("action.copy"),
            has_sel,
            tab_id,
            |c, tab, cx| {
                c.term_copy(tab, cx);
            },
        ))
        .child(term_menu_item(
            &weak,
            "tm-paste",
            "clipboard",
            tr("action.paste"),
            true,
            tab_id,
            |c, tab, cx| {
                c.term_paste(tab, cx);
            },
        ))
        .child(term_menu_item(
            &weak,
            "tm-select-all",
            "check",
            tr("action.select_all"),
            true,
            tab_id,
            |c, tab, cx| {
                c.term_select_all(tab, cx);
            },
        ));
    layer.child(card).into_any_element()
}

/// 终端菜单动作项：点击 = 应用动作（作用于该 tab）+ 收菜单 + 焦点回终端。
fn term_menu_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    icon_name: &'static str,
    label: &str,
    enabled: bool,
    tab_id: usize,
    apply: fn(&mut Chat, usize, &mut gpui::Context<Chat>),
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let item = menu_item(
        SharedString::from(id.to_string()),
        icon_name,
        label,
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            apply(c, tab_id, cx);
            c.dlg.term_menu = None;
            // 焦点回终端：菜单收起后键盘接着用（与打开菜单时的聚焦对称）
            if let Some(tab) = c.ws.terminals.iter().find(|t| t.id == tab_id) {
                let f = tab.focus.clone();
                window.focus(&f);
            }
            cx.notify();
        });
    });
    if enabled { item } else { item.opacity(0.45) }
}
