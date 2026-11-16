//! Pin 收藏夹视图 —— 列表体 + 收藏根行。自 file_tree.rs 内切拆出
//! （check_arch 文件 1500 行上限），纯结构搬移：收藏条目按名称字母序
//! 为根行，展开的目录下钻完整子树，行操作与文件树一致。

use std::path::Path;
use std::sync::Arc;

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme;
use crate::i18n::tr;
use crate::same_path;
use crate::services::file_icons;
use crate::services::file_tree::TreeRow;
use crate::ui::{VListHeight, icon_path, vlist};

use super::file_tree::{
    CONTENT_PAD, ROW_H, changed_dot, tree_drag_wiring, tree_right_click, tree_row, tree_scrollbar,
};
use super::file_tree_edit::{edit_chrome, edit_input_slot, validation_note};

/// Pin 收藏夹视图 vlist 的元素 id（独立句柄，滚动位不与主树共享）。
const PIN_LIST_ID: &str = "file-pins";

/// Pin 收藏夹列表体（021 三改：与「只看变更」同级的独立面板，本质是
/// 文件树过滤器）——收藏条目按名称字母序为根行（名称前置、路径淡化
/// 后置），展开的目录下钻完整子树，行操作与文件树一致。几何与树分支
/// 同款（pt4/px6/pb6 + scroll-wrap + 贴边滚动条），vlist 换独立 id
/// 不与树共享滚动位；无 sticky 祖先链（收藏根 depth 0 无祖先）。
pub(crate) fn pin_list_view(
    chat: &Chat,
    rows: Arc<Vec<TreeRow>>,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = weak.clone();
    let weak_c = weak.clone();
    let weak_bg = weak.clone();
    let cwd_bg = chat.ws.cwd.clone();
    let cwd_rows = chat.ws.cwd.clone();
    let editing = chat.dlg.tree_edit.clone();
    // 021 多选：行高亮对象 = 动作对象集（同主树）；拖拽载荷带标记集快照
    let sel_set = std::sync::Arc::new(chat.tree_effective_sel());
    let drag_marked = std::sync::Arc::new(
        chat.ws
            .tree_marked
            .iter()
            .map(|p| {
                let is_dir = chat
                    .ws
                    .tree_rows
                    .iter()
                    .find(|r| same_path(&r.path, p))
                    .map(|r| r.is_dir)
                    .unwrap_or(p.is_dir());
                (p.clone(), is_dir)
            })
            .collect::<Vec<_>>(),
    );
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .pt(px(4.))
        .px(px(6.))
        .pb(px(6.))
        // 空白处右键 = 根目录菜单（同树/变更分支兜底）
        .on_mouse_down(
            gpui::MouseButton::Right,
            move |ev: &gpui::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let Some(cwd_bg) = cwd_bg.clone() else { return };
                let _ = weak_bg.update(cx, |c, cx| {
                    c.dlg.tree_menu = Some(crate::function_panel::TreeMenu::Dir {
                        path: cwd_bg,
                        x: f32::from(ev.position.x),
                        y: f32::from(ev.position.y),
                    });
                    cx.notify();
                });
            },
        )
        .child(
            div()
                .id("pins-scroll-wrap")
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                // gpui div 默认 block：不显式 flex 列，内层 vlist 的
                // flex_1 失效、absolute 列表不撑高 → 高度塌 0 整树不可见
                .flex()
                .flex_col()
                .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                    this.ws.tree_sb_state.set_parent_hovered(*hovered);
                    cx.notify();
                }))
                .child(vlist(
                    PIN_LIST_ID,
                    rows.len(),
                    ROW_H,
                    VListHeight::Fill,
                    false,
                    false,
                    tr("file_tree.no_pins_view"),
                    None,
                    move |ix, _window, _cx| {
                        let Some(row) = rows.get(ix) else {
                            return div().into_any_element();
                        };
                        let prev_depth = if ix == 0 {
                            None
                        } else {
                            rows.get(ix - 1).map(|r| r.depth)
                        };
                        let next_depth = rows.get(ix + 1).map(|r| r.depth);
                        if row.depth == 0 {
                            // 收藏根行：rel 去掉尾部名称 = 淡化路径段
                            //（pin 项目根本身时 rel 短于名称，置空）
                            let rel = row
                                .path
                                .strip_prefix(cwd_rows.as_deref().unwrap_or(Path::new("")))
                                .unwrap_or(&row.path)
                                .to_string_lossy()
                                .replace('\\', "/");
                            let name_len = row.name.len();
                            let dim_dir = if rel.len() > name_len {
                                rel[..rel.len() - name_len].to_string()
                            } else {
                                String::new()
                            };
                            pin_root_row(
                                row.clone(),
                                dim_dir,
                                editing.clone(),
                                sel_set.iter().any(|p| same_path(p, &row.path)),
                                &drag_marked,
                                weak_c.clone(),
                            )
                        } else {
                            tree_row(
                                row.clone(),
                                prev_depth,
                                next_depth,
                                editing.clone(),
                                sel_set.iter().any(|p| same_path(p, &row.path)),
                                &drag_marked,
                                weak_c.clone(),
                            )
                        }
                    },
                ))
                .child(tree_scrollbar(chat, PIN_LIST_ID, 6.)),
        )
        .into_any_element()
}

/// Pin 收藏夹视图的收藏根行（021 三改）：类型图标 + 名称前置恒可见 + 路
/// 径段后置淡化；目录点击展开/折叠（expanded_dirs 与主树共享），文件点击
/// 打开 tab；右键 = 文件树同款菜单（取消固定走菜单「Pin/取消固定」项）、
/// 拖拽接线同树行——「操作和文件树一样」。行内改名态（F2/菜单）原地换
/// 输入框。`dim_dir` = 相对 cwd 的目录段（可空，淡化展示）。
fn pin_root_row(
    row: crate::services::file_tree::TreeRow,
    dim_dir: String,
    editing: Option<crate::function_panel::TreeEdit>,
    selected: bool,
    marked: &std::sync::Arc<Vec<(std::path::PathBuf, bool)>>,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    // 接线句柄先分身（点击闭包会把 weak 吃掉，同 tree_row）
    let weak_menu = weak.clone();
    let weak_drag = weak.clone();
    // 本行处于行内改名：内容整体换输入框（Zed EditState 命中该行）
    let editing = editing.filter(|e| {
        matches!(&e.kind, crate::function_panel::TreeEditKind::Rename { path } if same_path(path, &row.path))
    });
    let selected = selected && editing.is_none();
    let sel_bg = gpui::rgba((t.text << 8) | 0x1a);
    let mut el = div()
        .id(SharedString::from(format!(
            "pin-root-{}",
            row.path.display()
        )))
        .w_full()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .overflow_hidden()
        .pr(px(8.))
        .rounded(px(4.))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(if row.ignored { t.text_faint } else { t.text }))
        .cursor_pointer()
        // 行框几何同 tree_row（border_1 恒挂只换色，防 1px 抖动）
        .border_1()
        .border_color(gpui::transparent_black())
        .when(selected, |d| d.bg(sel_bg).hover(move |s| s.bg(sel_bg)))
        .when(!selected, |d| {
            d.hover(|s| s.bg(rgb(t.bg_hover)).border_color(rgb(t.bg_hover)))
        });

    // depth 0 无缩进无引导线；px6 内容列同 Zed ListItem
    let mut inner = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .px(px(CONTENT_PAD));

    if let Some(e) = editing {
        let (border, message) = edit_chrome(&e, t);
        el = el.border_color(rgb(border));
        let icon = if row.is_dir {
            file_icons::get_generic_folder_icon(false)
        } else {
            file_icons::get_icon(&row.path)
        };
        inner = inner
            .child(
                div()
                    .flex_shrink_0()
                    .child(icon_path(icon, 14., t.text_dim)),
            )
            .child(edit_input_slot(e.input.clone()));
        el = el
            .child(inner)
            .children(message.map(|m| validation_note(m, border)));
        return el.into_any_element();
    }

    let icon = if row.is_dir {
        file_icons::get_generic_folder_icon(row.expanded)
    } else {
        file_icons::get_icon(&row.path)
    };
    inner = inner.child(
        div()
            .flex_shrink_0()
            .child(icon_path(icon, 14., t.text_dim)),
    );
    // 名称前置恒可见 + 路径段后置淡化（changes_row 同款双段布局）
    inner = inner.child(
        div()
            .flex_shrink_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(SharedString::from(row.name.clone())),
    );
    inner = inner.child(
        div()
            .flex_shrink()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_color(rgb(t.text_faint))
            .child(SharedString::from(dim_dir)),
    );
    inner = inner.child(div().flex_1());
    el = el.child(inner);

    if row.is_dir {
        let dir_path = row.path.clone();
        // on_click 同树行（mouse-up，让位拖拽起手）；展开集合与主树共享。
        // Pin 视图无多选手势：裸点清标记
        el = el.on_click(move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.ws.tree_marked.clear();
                c.ws.tree_selected = Some(dir_path.clone());
                if !c.ws.expanded_dirs.remove(&dir_path) {
                    c.ws.expanded_dirs.insert(dir_path.clone());
                }
                c.rebuild_tree();
                cx.notify();
            });
        });
        if row.changed_dot && crate::services::workspace::git_markers() {
            el = el.child(changed_dot());
        }
    } else {
        let fp = row.path.clone();
        el = el.on_click(move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.ws.tree_marked.clear();
                c.ws.tree_selected = Some(fp.clone());
                c.open_file_tab(fp.clone(), cx);
            });
        });
        // git 徽标同树行（023 默认关）
        if let Some(st) = row
            .git
            .filter(|_| crate::services::workspace::git_markers())
        {
            el = el.child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::appearance::ui_size(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(st.color()))
                    .child(st.badge()),
            );
        }
    }
    el = el.child(tree_right_click(&weak_menu, &row));
    tree_drag_wiring(el, &row, &weak_drag, marked).into_any_element()
}
