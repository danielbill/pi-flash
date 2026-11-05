//! 文件树右键菜单层（自 psp_overlays.rs 切出，check_arch 文件 1500 行上限；
//! 纯结构搬移 + 插件菜单项接线，beads 360 ③）。卡形制（menu_card/menu_item/
//! menu_sep）仍住在 psp_overlays。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::services::workspace::same_path;
use crate::ui::icon;
use crate::ui::popup::PopupEdges;

use super::psp_overlays::{menu_card, menu_item, menu_sep};
use super::tree_menu_ext::ext_menu_entry;

/// 文件树右键菜单：条目按设计稿 021——目录=新建/资源管理器/终端/复制/
/// @/改名/删除，文件=新建(落点=所在目录)/资源管理器/终端/浏览器(html)/
/// 复制/@/改名/删除。
/// 复用 psp 菜单的 menu_card/menu_item（同一 theme 样式族），锚点 =
/// 右键事件坐标，弹出方向走统一算法。
pub(crate) fn tree_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = match chat.dlg.tree_menu.clone().expect("checked by caller") {
        crate::function_panel::TreeMenu::Dir { x, y, .. }
        | crate::function_panel::TreeMenu::File { x, y, .. } => (x, y),
    };
    // 与 psp 菜单同参：向下弹让 12px，左右象限决定横向方向
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.tree_menu = None;
    });
    let mut card = menu_card(edges, t);
    match chat.dlg.tree_menu.clone().expect("re-checked") {
        crate::function_panel::TreeMenu::Dir { path, .. } => {
            card = card
                .child(tree_item(
                    &weak,
                    "ftm-new-file",
                    "file-plus",
                    tr("file_view.new_file"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.open_file_new(path.clone(), false, cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-new-dir",
                    "folder-plus",
                    tr("file_tree.new_folder"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.open_file_new(path.clone(), true, cx)
                    },
                ))
                .child(tree_pin_item(&weak, "ftm-dir-pin", &path, chat))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-dir-explorer",
                    "folder-input",
                    tr("file_tree.reveal_in_explorer"),
                    false,
                    {
                        let path = path.clone();
                        move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
                    },
                ))
                .child(tree_terminal_item(&weak, "ftm-dir-term", &path))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-dir-copy",
                    "copy",
                    tr("file_tree.copy_path"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_copy_path(path.clone(), cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-dir-at",
                    "at-sign",
                    tr("file_tree.insert_at_mention"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_at_insert(path.clone(), true, cx)
                    },
                ));
            // 根目录（操作栏名字区右键）不出现 改名/删除——项目根本身不是
            // 可操作对象（改名会拖走整个工作区、删除等于删项目）
            let is_root = chat
                .ws
                .cwd
                .as_deref()
                .is_some_and(|cur| same_path(&path, cur));
            if !is_root {
                card = card
                    .child(menu_sep())
                    .child(tree_item(
                        &weak,
                        "ftm-dir-rename",
                        "pencil",
                        tr("file_tree.rename"),
                        false,
                        {
                            let path = path.clone();
                            move |c, cx| c.open_file_rename(path.clone(), true, false, cx)
                        },
                    ))
                    .child(tree_item(
                        &weak,
                        "ftm-dir-del",
                        "trash",
                        tr("action.delete"),
                        true,
                        {
                            let path = path.clone();
                            move |c, cx| c.confirm_tree_delete(path.clone(), cx)
                        },
                    ));
            }
        }
        crate::function_panel::TreeMenu::File { path, .. } => {
            // 新建对齐目录菜单语义：落点 = 文件所在目录（parent 取不到时
            // 兜底项目根）
            card = card
                .child(tree_item(
                    &weak,
                    "ftm-file-new-file",
                    "file-plus",
                    tr("file_view.new_file"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| {
                            let dir = path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .or_else(|| c.ws.cwd.clone());
                            let Some(dir) = dir else { return };
                            c.open_file_new(dir, false, cx)
                        }
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-file-new-dir",
                    "folder-plus",
                    tr("file_tree.new_folder"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| {
                            let dir = path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .or_else(|| c.ws.cwd.clone());
                            let Some(dir) = dir else { return };
                            c.open_file_new(dir, true, cx)
                        }
                    },
                ))
                .child(tree_pin_item(&weak, "ftm-file-pin", &path, chat))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-file-explorer",
                    "folder-input",
                    tr("file_tree.reveal_in_explorer"),
                    false,
                    {
                        let path = path.clone();
                        move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
                    },
                ))
                .child(tree_terminal_item(&weak, "ftm-file-term", &path));
            // 「在浏览器中打开」只对 html 页文件出现（设计稿 021）——判据
            // 与编辑器右键菜单同一处（editor/kind.rs `browser_openable`）
            if crate::editor::kind::browser_openable(&path) {
                card = card.child(tree_item(
                    &weak,
                    "ftm-file-browser",
                    "globe",
                    tr("file_tree.open_in_browser"),
                    false,
                    {
                        let path = path.clone();
                        move |_c, _cx| crate::services::shell_open::open_in_browser(&path)
                    },
                ));
            }
            // 021 右键【执行】（bat/cmd/ps1/js/py/sh）：解析成功 → 新终端 tab
            // 跑 `cmd /k`；失败置灰带原因。不查系统关联（js→wscript 陷阱），
            // 配方表与探测链在 services::script_exec
            if crate::services::script_exec::exec_kind(&path).is_some() {
                match crate::services::script_exec::resolve(&path, chat.ws.cwd.as_deref()) {
                    Some(plan) => {
                        card = card.child(tree_exec_item(&weak, "ftm-file-exec", &path, plan));
                    }
                    None => {
                        let label = format!(
                            "{}（{}）",
                            tr("file_tree.execute"),
                            tr("file_tree.execute_no_interpreter")
                        );
                        card = card.child(menu_item_greyed("ftm-file-exec", "play", &label));
                    }
                }
            }
            // —— 扩展菜单（beads 360 反馈④）：两条分隔线之间，hover 右侧
            // 浮现插件贡献的菜单项（contributes.file_menu）——
            card = card.child(menu_sep());
            if chat
                .plugins
                .plugins
                .iter()
                .any(|e| !e.disabled && !e.manifest.file_menu.is_empty())
            {
                card = card
                    .child(ext_menu_entry(chat, &weak, &path))
                    .child(menu_sep());
            }
            card = card
                .child(tree_item(
                    &weak,
                    "ftm-file-copy",
                    "copy",
                    tr("file_tree.copy_path"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_copy_path(path.clone(), cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-file-at",
                    "at-sign",
                    tr("file_tree.insert_at_mention"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_at_insert(path.clone(), false, cx)
                    },
                ))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-file-rename",
                    "pencil",
                    tr("file_tree.rename"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.open_file_rename(path.clone(), false, false, cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-file-del",
                    "trash",
                    tr("action.delete"),
                    true,
                    {
                        let path = path.clone();
                        move |c, cx| c.confirm_tree_delete(path.clone(), cx)
                    },
                ));
        }
    }
    // 主菜单实测高度 → 扩展子菜单的高度上限（超出内部滚动）
    let slot = chat.dlg.tree_menu_h.clone();
    layer
        .child(crate::ui::measure_height("ftm-card-h", &slot, card))
        .into_any_element()
}

/// 菜单项公共接线：点击 = 应用动作 + 收菜单（同 psp 菜单项语义）。
fn tree_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    icon_name: &'static str,
    label: &str,
    danger: bool,
    apply: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    menu_item(
        SharedString::from(id.to_string()),
        icon_name,
        label,
        danger,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            apply(c, cx);
            c.dlg.tree_menu = None;
            cx.notify();
        });
    })
}

/// 右键【执行】同样要走 window（open_script_terminal 建 tab），单独接线；
/// plan 移入闭包，菜单文案在移入前先取好。
fn tree_exec_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    file: &std::path::PathBuf,
    plan: crate::services::script_exec::ExecPlan,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let file = file.clone();
    let label = if plan.label.is_empty() {
        tr("file_tree.execute").to_string()
    } else {
        format!("{} · {}", tr("file_tree.execute"), plan.label)
    };
    menu_item(
        SharedString::from(id.to_string()),
        "play",
        &label,
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            c.open_script_terminal(file.clone(), plan.clone(), window, cx);
            c.dlg.tree_menu = None;
            cx.notify();
        });
    })
}

/// 置灰菜单项（【执行】解析失败）：无 hover 不可点，文案带原因
fn menu_item_greyed<S: Into<SharedString>>(
    id: S,
    icon_name: &'static str,
    label: &str,
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
        .text_color(rgb(t.text_muted))
        .child(icon(icon_name, 14., t.text_muted))
        .child(div().flex_1().child(SharedString::from(label.to_string())))
}

/// 021 右键菜单「Pin/取消固定」项：已在收藏夹 = 取消固定，反之加入
/// （toggle 一条写路 tree_pin_toggle）。
fn tree_pin_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    path: &std::path::PathBuf,
    chat: &Chat,
) -> gpui::Stateful<gpui::Div> {
    let pinned = chat.ws.pinned.iter().any(|p| same_path(p, path));
    tree_item(
        weak,
        id,
        "pin",
        if pinned {
            tr("file_tree.unpin")
        } else {
            tr("file_tree.pin")
        },
        false,
        {
            let path = path.clone();
            move |c, cx| c.tree_pin_toggle(path.clone(), cx)
        },
    )
}

/// 「在终端中打开」要走 window（open_terminal_in 建终端 tab），单独接线。
fn tree_terminal_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    path: &std::path::PathBuf,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let path = path.clone();
    menu_item(
        SharedString::from(id.to_string()),
        "icon-terminal-solid",
        tr("file_tree.open_in_terminal"),
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            // 文件 = 其所在目录；目录 = 自身（psp 项目菜单同语义）
            let cwd = if path.is_dir() {
                path.clone()
            } else {
                path.parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| path.clone())
            };
            c.open_terminal_in(cwd, window, cx);
            c.dlg.tree_menu = None;
            cx.notify();
        });
    })
}

