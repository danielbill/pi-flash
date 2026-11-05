//! psp 悬浮层 (v54): 项目路径 tooltip（深色，跟随鼠标）、会话/项目右键
//! 详情卡（锚右键点、点外即收；会话卡改名/删除流，项目卡删除确认/会话
//! 目录/新建会话）、⋯ 两级菜单（列表方式/排序方式带子菜单勾选态）。
//! 全部挂在根渲染，事件坐标定位。
//!
//! 定位统一走 `ui::popup::PopupEdges`（2026-10-11 用户算法：锚点在视口
//! 上半 → 向下弹，反之向上；左半 → 向右弹，反之向左）——列表拉到底时
//! 菜单/详情卡不再溢出下缘，窄窗时不再溢出右缘。
//!
//! itlq.12 超规拆分后本文件只剩根分发器：tooltip=psp_proj_tip、详情卡=
//! psp_hover_card/psp_project_card（共用原语=psp_card_btns）、树根项目
//! 下拉=psp_tree_proj_menu、终端菜单=psp_term_menu、删除确认/错误=
//! psp_del_confirm。下方 menu_* re-export 维持既有导入路径（tree_menu/
//! psp_menus/tab_menu/market/pf_plugin 五处消费）。

use gpui::{div, prelude::*};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::services::workspace::same_path;

use super::psp_del_confirm::{tree_del_error_layer, tree_delete_confirm};
use super::psp_hover_card::hover_card;
use super::psp_project_card::project_card;
use super::psp_proj_tip::proj_tip;
use super::psp_term_menu::term_menu_layer;
use super::psp_tree_proj_menu::tree_proj_menu_layer;

// psp 菜单原语（menu_card/menu_item 族/menu_sep）已于 090 重构上移
// ui::menu（全 app 统一菜单形制）；此处 re-export 维持既有导入路径。
pub(crate) use crate::ui::menu::{
    menu_card, menu_item, menu_item_asset, menu_item_sized, menu_sep,
};

/// 根渲染挂载的 psp 悬浮层合集（tooltip / 详情卡 / 菜单 / 确认）。
pub(crate) fn psp_overlays(
    chat: &mut Chat,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let vp = window.viewport_size();
    let vp = (f32::from(vp.width), f32::from(vp.height));
    // 全窗口定位层：absolute inset_0 铺满 root，悬浮子元素相对它定位，
    // 且不参与流布局（0 高容器会把 absolute 子元素裁掉——菜单曾因此不可见）。
    // **父层不得 occlude**：inset_0 的层一挡全窗口 hit-test 都断（行悬停
    // 背景闪烁的根因）；各浮层在自己的 bounds 内各自 occlude。
    let mut el = div().id("psp-overlays").absolute().inset_0();
    if let Some((path, x, y)) = &chat.dlg.proj_tip {
        el = el.child(proj_tip(path, *x, *y, vp, t));
    }
    if chat.dlg.editor_menu.is_some() {
        el = el.child(super::psp_menus::editor_menu_layer(chat, vp, t, cx));
    }
    // 输入面板右键菜单（031：编辑块 撤销/恢复/全选 + 剪切/复制/粘贴/删除）
    if chat.dlg.input_menu.is_some() {
        el = el.child(super::psp_menus::input_menu_layer(chat, vp, t, cx));
    }
    // 消息内容区右键菜单（单项「复制」= 复制当前消息划选）
    if chat.dlg.msg_menu.is_some() {
        el = el.child(super::psp_menus::msg_menu_layer(chat, vp, t, cx));
    }
    // 终端右键菜单（复制/粘贴/全选）；tab 已关则不再挂（防挂到空气里）
    if let Some((tab_id, x, y)) = chat.dlg.term_menu
        && chat.ws.terminals.iter().any(|t| t.id == tab_id)
    {
        el = el.child(term_menu_layer(chat, tab_id, x, y, vp, t, cx));
    }
    if chat.dlg.hover_card.is_some() {
        // 右键详情卡：打开即渲染（无悬停延迟），点卡外/再右键别行收换
        el = el.child(hover_card(chat, vp, t, cx));
    }
    if chat.dlg.proj_card.is_some() {
        // 项目行右键详情卡：同 hover_card 语义（点卡外/再右键别行收换）
        el = el.child(project_card(chat, vp, t, cx));
    }
    // 插件格子右键菜单：同一浮层挂载点，样式与文件树菜单同源
    if chat.plugins.plugins.iter().any(|p| p.cell_menu.is_some()) {
        el = el.child(crate::pf_plugin::cell_menu_layer(chat, vp, t, cx));
    }
    // 插件弹窗（契约 v3 popups）：已迁 session/mod.rs 会话域居中浮层槽
    //（chat-wrap 中栏，InputPanel 居中基准），不再挂全窗 psp-overlays
    if chat.dlg.tree_menu.is_some() {
        el = el.child(super::tree_menu::tree_menu_layer(chat, vp, t, cx));
    }
    // 文件树标题项目下拉（树根跨项目快切）：标题左键 / files.project_menu
    if chat.dlg.tree_proj_menu.is_some() {
        el = el.child(tree_proj_menu_layer(chat, vp, t, cx));
    }
    // topbar 标签右键菜单（zed tab_bar 菜单移植；tab 已关不再挂——防挂到
    // 空气里，顺带收槽位）
    if let Some((target, x, y)) = chat.dlg.tab_menu {
        let alive = match target {
            crate::TabMenuTarget::Session => true,
            crate::TabMenuTarget::Panel(ix) => ix < chat.ws.panel_tabs.len(),
        };
        if alive {
            el = el.child(super::tab_menu::layer(chat, target, x, y, vp, t, cx));
        } else {
            chat.dlg.tab_menu = None;
        }
    }
    if let Some(path) = chat.dlg.tree_confirm_del.clone() {
        // zed build_removal_prompt 同款：删除打开中的文件时未保存修改并入
        // 删除确认（一次确认 = 删文件 + 弃缓冲）；目录删除统计其子树内
        // 全部打开中的脏 tab（zed 对整个待删集合数 dirty_buffers）
        let dirty_count = chat
            .ws
            .file_cache
            .iter()
            .filter(|(p, f)| f.dirty && (same_path(p, &path) || p.starts_with(&path)))
            .count();
        el = el.child(tree_delete_confirm(
            &path,
            dirty_count,
            &chat.dlg.dialog_focus,
            cx,
        ));
    }
    if chat.dlg.tree_del_error.is_some() {
        el = el.child(tree_del_error_layer(chat, t, cx));
    }
    el.into_any_element()
}
