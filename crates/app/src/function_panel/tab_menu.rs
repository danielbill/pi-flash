//! topbar 标签右键菜单（zed tab_bar 菜单移植，pi-flash-56t）：Close /
//! Close Others / Close Left / Close Right / Close All。不实现 zed 的
//! Close Clean（用户定夺）；快捷键列不做——PF 未注册对应 keymap 动作，
//! 假键位违反 gpui.md 铁律。首标签（会话 tab）恒存不可关：菜单无 Close、
//! Close Left 恒灰。状态槽 `dlg.tab_menu`，右键捕获在 titlebar（会话/
//! 自由 tab 各一），渲染挂 psp_overlays 的浮层合集。批量关闭语义见
//! `editor/tabs.rs` 的 `close_tabs_batch`（脏文件收拢进一个确认弹窗）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::popup::PopupEdges;

/// 标签菜单层：条目启停按 panel_tabs 现状算，禁用 = 灰字不接点击。样式
/// 走 psp 同族 menu_card，条目为 zed 风格纯文本（无图标）。
pub(crate) fn layer(
    chat: &mut Chat,
    target: crate::TabMenuTarget,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.tab_menu = None;
    });
    let len = chat.ws.panel_tabs.len();
    let all_ix = |c: &Chat| (0..c.ws.panel_tabs.len()).collect::<Vec<_>>();
    let mut card = super::psp_overlays::menu_card(edges, t);
    match target {
        crate::TabMenuTarget::Session => {
            // 会话 tab 在 panel_tabs 之外：Others/Right/All 同义 = 清空自由区
            card = card
                .child(item(
                    &weak,
                    "tbm-others",
                    tr("tab.close_others"),
                    len > 0,
                    move |c, cx| {
                        let targets = all_ix(c);
                        c.close_tabs_batch(targets, None, cx);
                    },
                ))
                .child(super::psp_overlays::menu_sep())
                .child(item(
                    &weak,
                    "tbm-left",
                    tr("tab.close_left"),
                    false,
                    |_, _| {},
                ))
                .child(item(
                    &weak,
                    "tbm-right",
                    tr("tab.close_right"),
                    len > 0,
                    move |c, cx| {
                        let targets = all_ix(c);
                        c.close_tabs_batch(targets, None, cx);
                    },
                ))
                .child(super::psp_overlays::menu_sep())
                .child(item(
                    &weak,
                    "tbm-all",
                    tr("tab.close_all"),
                    len > 0,
                    move |c, cx| {
                        let targets = all_ix(c);
                        c.close_tabs_batch(targets, None, cx);
                    },
                ));
        }
        crate::TabMenuTarget::Panel(ix) => {
            card = card
                .child(item(
                    &weak,
                    "tbm-close",
                    tr("tab.close"),
                    true,
                    move |c, cx| c.close_tab_at(ix, cx),
                ))
                .child(item(
                    &weak,
                    "tbm-others",
                    tr("tab.close_others"),
                    len > 1,
                    move |c, cx| {
                        let targets = all_ix(c).into_iter().filter(|&j| j != ix).collect();
                        c.close_tabs_batch(targets, Some(ix), cx);
                    },
                ))
                .child(super::psp_overlays::menu_sep())
                .child(item(
                    &weak,
                    "tbm-left",
                    tr("tab.close_left"),
                    ix > 0,
                    move |c, cx| {
                        let targets = (0..ix).collect();
                        c.close_tabs_batch(targets, Some(ix), cx);
                    },
                ))
                .child(item(
                    &weak,
                    "tbm-right",
                    tr("tab.close_right"),
                    ix + 1 < len,
                    move |c, cx| {
                        let targets = (ix + 1..c.ws.panel_tabs.len()).collect();
                        c.close_tabs_batch(targets, Some(ix), cx);
                    },
                ))
                .child(super::psp_overlays::menu_sep())
                .child(item(
                    &weak,
                    "tbm-all",
                    tr("tab.close_all"),
                    len > 0,
                    move |c, cx| {
                        let targets = all_ix(c);
                        c.close_tabs_batch(targets, None, cx);
                    },
                ));
        }
    }
    layer.child(card).into_any_element()
}

/// 菜单条目：zed 风格纯文本（无图标）；禁用态灰字、不接点击。
/// 点击 = 应用动作 + 收菜单（tree_item 同语义，收的是 tab_menu）。
fn item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    label: &str,
    enabled: bool,
    apply: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    let weak = weak.clone();
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .px(px(10.))
        .py(px(7.))
        .rounded(px(6.))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(if enabled {
            rgb(t.text)
        } else {
            rgb(t.text_faint)
        })
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = weak.update(cx, |c, cx| {
                        apply(c, cx);
                        c.dlg.tab_menu = None;
                        cx.notify();
                    });
                })
        })
        .child(SharedString::from(label.to_string()))
}
