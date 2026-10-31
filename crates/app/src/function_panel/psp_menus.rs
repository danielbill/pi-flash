//! 编辑区 / 输入面板 / 消息内容区的右键菜单层与菜单项装配。自
//! psp_overlays.rs 内切拆出（check_arch 文件 1500 行上限），纯结构搬移。
//! 卡片形制（menu_card/menu_item/menu_sep）仍住在 psp_overlays。

use gpui::{MouseButton, SharedString, prelude::*};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::popup::PopupEdges;

use super::psp_overlays::{menu_card, menu_item, menu_sep};

/// 编辑区右键菜单：编辑四项（复制/粘贴/剪切/全选，先归位焦点再派发
/// Input 动作）+ 文件动作块（条目与顺序由 kind handler 声明，含分隔线）。
pub(crate) fn editor_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (path, x, y) = chat.dlg.editor_menu.clone().expect("checked by caller");
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.editor_menu = None;
    });
    let ed = chat.ws.file_cache.get(&path).and_then(|f| f.editor.clone());
    let mut card = menu_card(edges, t);
    card = card
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-copy",
            "copy",
            tr("action.copy"),
            || Box::new(gpui_component::input::Copy),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-paste",
            "clipboard",
            tr("action.paste"),
            || Box::new(gpui_component::input::Paste),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-cut",
            "scissors",
            tr("action.cut"),
            || Box::new(gpui_component::input::Cut),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-select-all",
            "check",
            tr("action.select_all"),
            || Box::new(gpui_component::input::SelectAll),
        ))
        .child(menu_sep());
    // 文件动作块：条目与顺序由 kind handler 声明（含分隔线）——默认骨架 =
    // 资源管理器/终端 + 改名/删除；md 源码态带预览项；「在浏览器中打开」
    // 是路径判据（html/htm，同文件树菜单），保留在动作块首位
    let source_mode = chat.ws.file_cache.get(&path).is_some_and(|f| f.md_source);
    if crate::editor::kind::browser_openable(&path) {
        card = card.child(editor_item(
            &weak,
            "edm-browser",
            "globe",
            tr("file_tree.open_in_browser"),
            false,
            {
                let path = path.clone();
                move |_c, _cx| crate::services::shell_open::open_in_browser(&path)
            },
        ));
    }
    for action in crate::editor::kind::kind_of(&path).menu_file_actions(source_mode) {
        card = match action {
            crate::editor::kind::MenuAction::PreviewToggle => card.child(editor_item(
                &weak,
                "edm-preview",
                "eye",
                tr("file_view.preview"),
                false,
                {
                    let path = path.clone();
                    move |c, cx| {
                        if let Some(ft) = c.ws.file_cache.get_mut(&path) {
                            ft.md_source = false;
                        }
                        cx.notify();
                    }
                },
            )),
            crate::editor::kind::MenuAction::Explorer => card.child(editor_item(
                &weak,
                "edm-explorer",
                "folder-input",
                tr("file_tree.reveal_in_explorer"),
                false,
                {
                    let path = path.clone();
                    move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
                },
            )),
            crate::editor::kind::MenuAction::Terminal => {
                card.child(editor_terminal_item(&weak, "edm-term", &path))
            }
            crate::editor::kind::MenuAction::Sep => card.child(menu_sep()),
            crate::editor::kind::MenuAction::Rename => card.child(editor_item(
                &weak,
                "edm-rename",
                "pencil",
                tr("file_tree.rename"),
                false,
                {
                    let path = path.clone();
                    move |c, cx| c.open_file_rename(path.clone(), false, true, cx)
                },
            )),
            crate::editor::kind::MenuAction::Delete => card.child(editor_item(
                &weak,
                "edm-del",
                "trash",
                tr("action.delete"),
                true,
                {
                    let path = path.clone();
                    move |c, cx| c.confirm_tree_delete(path.clone(), cx)
                },
            )),
        };
    }
    layer.child(card).into_any_element()
}

/// 输入面板右键菜单（031 用户定案）：纯编辑块——撤销/恢复/全选 + 剪切/
/// 复制/粘贴/删除，样式与编辑区菜单同套（menu_card/menu_item/menu_sep，
/// 不出快捷键列：Ctrl+Z/Y/A/X/C/V 系统本就支持，菜单只是等价入口）。
/// 动作先把焦点还给 composer 输入组件再派发 Input 动作（同编辑区菜单
/// 编辑四项的接线）；粘贴派发 app 的 ComposerPaste，与 Ctrl+V 完全同
/// 路径（剪贴板含图先附件化）。
pub(crate) fn input_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = chat.dlg.input_menu.expect("checked by caller");
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.input_menu = None;
    });
    let card = menu_card(edges, t)
        .child(composer_edit_item(
            &weak,
            "cim-undo",
            "undo-2",
            tr("action.undo"),
            || Box::new(gpui_component::input::Undo),
        ))
        .child(composer_edit_item(
            &weak,
            "cim-redo",
            "redo-2",
            tr("action.redo"),
            || Box::new(gpui_component::input::Redo),
        ))
        .child(composer_edit_item(
            &weak,
            "cim-select-all",
            "check",
            tr("action.select_all"),
            || Box::new(gpui_component::input::SelectAll),
        ))
        .child(menu_sep())
        .child(composer_edit_item(
            &weak,
            "cim-cut",
            "scissors",
            tr("action.cut"),
            || Box::new(gpui_component::input::Cut),
        ))
        .child(composer_edit_item(
            &weak,
            "cim-copy",
            "copy",
            tr("action.copy"),
            || Box::new(gpui_component::input::Copy),
        ))
        .child(composer_paste_item(&weak, "cim-paste"))
        .child(composer_edit_item(
            &weak,
            "cim-delete",
            "trash",
            tr("action.delete"),
            || Box::new(gpui_component::input::Delete),
        ));
    layer.child(card).into_any_element()
}

/// 消息内容区右键菜单：单项「复制」= 复制当前消息划选（样式同输入面板
/// 右键菜单 menu_card/menu_item 套件）。文案取打开菜单当帧的划选快照——
/// 菜单开着时没有会清选区的路径（TextView 已 vendor 补丁为仅左键起选/
/// 清除，点菜单项本身有 stop_propagation），快照即点击时的真实选区；
/// 无划选时快照为空，点击自然空转。
pub(crate) fn msg_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = chat.dlg.msg_menu.expect("checked by caller");
    let copied = gpui_component::text::take_last_selected_text(cx);
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.msg_menu = None;
    });
    let copied_click = copied.clone();
    let card = menu_card(edges, t).child(
        menu_item("msg-copy", "copy", tr("action.copy"), false, false).on_mouse_down(
            MouseButton::Left,
            move |_, _, cx| {
                cx.stop_propagation();
                if let Some(text) = copied_click.clone() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
                let _ = weak.update(cx, |c, cx| {
                    c.dlg.msg_menu = None;
                    cx.notify();
                });
            },
        ),
    );
    layer.child(card).into_any_element()
}

/// 输入菜单动作项接线：点击 = 焦点归位 composer 输入组件 + 派发 Input
/// 动作 + 收菜单（同 editor_edit_item，目标换成 composer 门面内的
/// InputState——菜单打开不改变焦点归属，但点击可能让焦点漂移，保险
/// 每次归位，收起后键盘接着在输入框里用）。
fn composer_edit_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    icon_name: &'static str,
    label: &str,
    make: fn() -> Box<dyn gpui::Action>,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    menu_item(
        SharedString::from(id.to_string()),
        icon_name,
        label,
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            if let Some(comp) = &c.composer.composer {
                let handle = comp.read(cx).focus_handle_in(cx);
                window.focus(&handle);
                window.dispatch_action(make(), cx);
            }
            c.dlg.input_menu = None;
            cx.notify();
        });
    })
}

/// 输入菜单「粘贴」项：派发 app 的 ComposerPaste 而非组件 Paste——
/// Ctrl+V 的完整路径（剪贴板含图先附件化，纯文本再走组件粘贴）。
fn composer_paste_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    menu_item(
        SharedString::from(id.to_string()),
        "clipboard",
        tr("action.paste"),
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            if let Some(comp) = &c.composer.composer {
                let handle = comp.read(cx).focus_handle_in(cx);
                window.focus(&handle);
                window.dispatch_action(Box::new(crate::ComposerPaste), cx);
            }
            c.dlg.input_menu = None;
            cx.notify();
        });
    })
}

/// 编辑菜单动作项接线：点击 = 应用动作 + 收**编辑菜单**（tree_item 写死
/// 收 tree_menu，编辑菜单复用它会导致菜单不消、挡住改名框/删除确认）。
fn editor_item(
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
            c.dlg.editor_menu = None;
            cx.notify();
        });
    })
}

/// 编辑动作项（复制/粘贴/剪切/全选）：先归位焦点再派发 Input 动作。
fn editor_edit_item(
    weak: &gpui::WeakEntity<Chat>,
    ed: &Option<gpui::Entity<gpui_component::input::InputState>>,
    id: &'static str,
    icon_name: &'static str,
    label: &str,
    make: fn() -> Box<dyn gpui::Action>,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let ed = ed.clone();
    menu_item(
        SharedString::from(id.to_string()),
        icon_name,
        label,
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            if let Some(ed) = &ed {
                ed.update(cx, |st, scx| st.focus(window, scx));
                window.dispatch_action(make(), cx);
            }
            c.dlg.editor_menu = None;
            cx.notify();
        });
    })
}

/// 「在终端中打开」：文件 = 其所在目录（同文件树菜单语义）。
fn editor_terminal_item(
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
            let cwd = path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| path.clone());
            c.open_terminal_in(cwd, window, cx);
            c.dlg.editor_menu = None;
            cx.notify();
        });
    })
}
