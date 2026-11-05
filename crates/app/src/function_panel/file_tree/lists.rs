//! 文件树/变更列表体（itlq.12 自 file_tree.rs 切出，纯结构搬移）：主树
//! vlist（行内新建虚拟行落点 + sticky 祖先链装饰）、「只看变更」扁平
//! vlist（文件名前置 + 目录段后置 dim）+ 共用滚动条条带。行渲染本体在
//! `super::rows`（tree_row / changes_row）。

use std::sync::Arc;

use gpui::{SharedString, UniformListDecoration, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme;
use crate::function_panel::file_tree_drag::TreeDrag;
use crate::function_panel::file_tree_edit::{edit_chrome, edit_input_slot, tree_edit_row, validation_note};
use crate::function_panel::file_tree_sticky::StickyAncestors;
use crate::services::file_tree::TreeRow;
use crate::ui::{VListHeight, vlist};
use crate::{i18n::tr, same_path};

use super::{FILE_TREE_LIST_ID, ROW_H};

/// 主树列表体：行内新建虚拟行的落点计算 + sticky 祖先链装饰 + vlist。
/// 会话列表同款滚动条挂法：vlist 自带的 menu_scrollbar 关掉，改 relative
/// 包裹 + 右缘 absolute 条；容器 px(6) → 条外扩 6 抵消，右缘齐 dock 右缘
/// （同会话列表贴边）。
pub(crate) fn tree_list_view(
    chat: &Chat,
    rows: Arc<Vec<TreeRow>>,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = weak.clone();
    let weak_bg = weak.clone();
    let weak_root_drop = weak.clone();
    let cwd_bg = chat.ws.cwd.clone();
    let edit = chat.dlg.tree_edit.clone();
    let selected = chat.ws.tree_selected.clone();
    // 新建行的落点（Zed new_entry：虚拟行插在目录行下方、子项之前；
    // 目标 = 项目根时插列表顶）。改名行不注入（原地替换该行）。公式收在
    // Chat::tree_new_edit_slot，open_file_new 的编辑行自动滚动同源。
    let edit_slot: Option<(usize, usize)> = edit.as_ref().and_then(|e| match &e.kind {
        crate::function_panel::TreeEditKind::New { dir } => Some(chat.tree_new_edit_slot(dir)),
        crate::function_panel::TreeEditKind::Rename { .. } => None,
    });
    let edit_ix = edit_slot.map(|(ix, _)| ix);
    let total = rows.len() + edit_slot.map_or(0, |_| 1);
    // 键入中的文件名（Zed：新建文件行的类型图标跟名字实时换）
    let live_name = edit
        .as_ref()
        .map(|e| e.input.read(cx).value().to_string())
        .unwrap_or_default();
    let sticky = Box::new(StickyAncestors {
        weak: weak.clone(),
        edit_ix,
    }) as Box<dyn UniformListDecoration>;
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        // 设计稿 .ft-list（020-1）：上 4 左右 6 下 6；根行图标随
        // 容器 pl6 + 行内 CONTENT_PAD 6 落到 ICON_LINE 12
        .pt(px(4.))
        .px(px(6.))
        .pb(px(6.))
        // 空白处右键 = 根目录菜单（同变更分支，根行入口兜底）
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
        // 列表空白处 drop = 移到项目根（Zed 背景落点语义）。行先于
        // 容器拿走 drop（后代监听冒泡相先派发），这里只兜行与钉住
        // 行之外的空隙；原地 drop 由 tree_move_commit 静默 no-op
        .on_drop::<TreeDrag>(move |drag, _, cx| {
            let _ = weak_root_drop.update(cx, |c, cx| {
                if let Some(cwd) = c.ws.cwd.clone() {
                    c.tree_move_commit(&drag.path, drag.is_dir, &cwd, cx);
                }
            });
        })
        .child(
            div()
                .id("tree-scroll-wrap")
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                // 同上：显式 flex 列，vlist 的 flex_1 才拿得到定高
                .flex()
                .flex_col()
                .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                    this.ws.tree_sb_state.set_parent_hovered(*hovered);
                    cx.notify();
                }))
                .child(vlist(
                    FILE_TREE_LIST_ID,
                    total,
                    ROW_H,
                    VListHeight::Fill,
                    false,
                    false,
                    tr("file_tree.no_files"),
                    Some(sticky),
                    move |ix, _window, _cx| {
                        // 行内新建：该输出位渲染编辑行，其余行做索引平移
                        if edit_slot.is_some_and(|(eix, _)| ix == eix) {
                            let Some((_, depth)) = edit_slot else {
                                return div().into_any_element();
                            };
                            return tree_edit_row(
                                edit.clone().expect("slot checked"),
                                depth,
                                &live_name,
                            );
                        }
                        let row_ix = match edit_ix {
                            Some(eix) if ix > eix => ix - 1,
                            _ => ix,
                        };
                        let Some(row) = rows.get(row_ix) else {
                            return div().into_any_element();
                        };
                        // 邻行深度决定各段引导线的首尾裁剪（整段只在首行留上
                        // 4px、尾行留下 4px，段中连续成一条线）；编辑行占位时
                        // 邻行深度取编辑行的深度
                        let depth_at = |out_ix: usize| -> Option<usize> {
                            if Some(out_ix) == edit_ix {
                                edit_slot.map(|(_, d)| d)
                            } else {
                                let rix = match edit_ix {
                                    Some(eix) if out_ix > eix => out_ix - 1,
                                    _ => out_ix,
                                };
                                rows.get(rix).map(|r| r.depth)
                            }
                        };
                        let prev_depth = if ix == 0 { None } else { depth_at(ix - 1) };
                        let next_depth = depth_at(ix + 1);
                        super::rows::tree_row(
                            row.clone(),
                            prev_depth,
                            next_depth,
                            edit.clone(),
                            selected.as_ref().is_some_and(|p| same_path(p, &row.path)),
                            weak.clone(),
                        )
                    },
                ))
                .child(tree_scrollbar(chat, FILE_TREE_LIST_ID, 6.)),
        )
        .into_any_element()
}

/// 「只看变更」列表体：容器 + vlist（行头由 files_view 构建，这里只管
/// 滚动列表）。会话列表同款滚动条挂法（psp-scroll-wrap 镜像）：vlist
/// 自带的 menu_scrollbar 关掉，改 relative 包裹 + 右缘 absolute 条；变更
/// 容器无水平内边距，条 right(0) 已贴 dock 右缘。
pub(crate) fn changes_list_view(
    chat: &Chat,
    rows: Arc<Vec<TreeRow>>,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = weak.clone();
    let weak_c = weak.clone();
    let weak_bg = weak.clone();
    let cwd_bg = chat.ws.cwd.clone();
    let editing = chat.dlg.tree_edit.clone();
    let selected = chat.ws.tree_selected.clone();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .py(px(6.))
        // 空白处右键 = 根目录菜单（021 四改：根行已移除，根下的
        // 新建/改名等入口由这里兜底；行内右键自己 stop_propagation
        // 不会漏到这层）
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
                .id("changes-scroll-wrap")
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
                    FILE_TREE_LIST_ID,
                    rows.len(),
                    CHANGES_ROW_H,
                    VListHeight::Fill,
                    false,
                    false,
                    tr("file_tree.no_changes"),
                    None,
                    move |ix, _window, _cx| match rows.get(ix) {
                        Some(row) => changes_row(
                            row.clone(),
                            editing.clone(),
                            selected.as_ref().is_some_and(|p| same_path(p, &row.path)),
                            weak_c.clone(),
                        ),
                        None => div().into_any_element(),
                    },
                ))
                .child(tree_scrollbar(chat, FILE_TREE_LIST_ID, 0.)),
        )
        .into_any_element()
}

/// 树/变更列表共用的滚动条条带（会话列表 psp-scroll 同款挂法）：absolute
/// 贴列表区右缘、宽 10 命中区，内容 = psp_scrollbar（hover 常显 + 3s 淡出
/// + thumb 悬停加宽，与会话列表一根代码）。`right_ext` = 条带相对列表
/// 包裹的外扩量，抵消所在容器的水平内边距让条带右缘齐 dock 右缘（树列表
/// 容器 px(6) → 6；变更容器无边距 → 0）；thumb 右缘最终 = dock 右 − 4，
/// 与会话列表逐像素同位。
pub(crate) fn tree_scrollbar(
    chat: &Chat,
    list_id: &'static str,
    right_ext: f32,
) -> gpui::AnyElement {
    div()
        .absolute()
        .top(px(0.))
        .bottom(px(0.))
        .right(px(-right_ext))
        .w(px(10.))
        .child(crate::ui::psp_scrollbar::psp_scrollbar(
            &chat.ws.tree_sb_state,
            &crate::ui::vlist::scroll_handle(list_id)
                .0
                .borrow()
                .base_handle,
        ))
        .into_any_element()
}

/// 变更列表行高（比树行稍宽舒，对齐 pi-web 截图的呼吸感）。
const CHANGES_ROW_H: f32 = 28.;

/// 「只看变更」的扁平行：文件名前置左对齐（恒可见，shrink 0）+
/// 目录段后置 dim（过长收缩截断），点击开文件 tab，右键沿用文件菜单。
/// 行处于行内改名时（Zed 同款）名字段整体换成紧凑输入框，点击开文件
/// 让位给输入框。`selected` = 082 选中条目（常驻薄纱底，F2 改名对象）。
fn changes_row(
    row: crate::services::file_tree::TreeRow,
    editing: Option<crate::function_panel::TreeEdit>,
    selected: bool,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    let rel = row.name.clone();
    let file_name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let dir_part = rel[..rel.len() - file_name.len()].to_string();
    let editing = editing.filter(|e| {
        matches!(&e.kind, crate::function_panel::TreeEditKind::Rename { path } if same_path(path, &row.path))
    });
    // 082 选中态：10% 前景薄纱常驻（与 psp 会话行选中同色系）；编辑态
    // 行让位给校验边框，不再叠选中底
    let selected = selected && editing.is_none();
    let sel_bg = gpui::rgba((t.text << 8) | 0x1a);
    // 021 三改：徽标与文件图标都去掉（信息冗余——变更态整表皆然，
    // 数量/增删在头行 numstat），行 = 纯路径
    let mut el = div()
        .id(SharedString::from(format!("chg-{}", row.path.display())))
        .w_full()
        .h(px(CHANGES_ROW_H))
        .flex()
        .items_center()
        .px(px(12.))
        .gap(px(7.))
        .overflow_hidden()
        .cursor_pointer()
        // 行框恒挂透明占位（同 tree_row：编辑态只换色不动布局，防 1px 抖动）
        .border_1()
        .border_color(gpui::transparent_black())
        .when(selected, |d| d.bg(sel_bg).hover(move |s| s.bg(sel_bg)))
        .when(!selected, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
        .text_size(crate::appearance::ui_size(11.5));
    if let Some(e) = editing {
        // 行内改名态：输入框吃满名字区（目录段一并隐去），边框与校验
        // 消息同 Zed 编辑行
        let (border, message) = edit_chrome(&e, t);
        el = el
            .border_color(rgb(border))
            .relative()
            .child(edit_input_slot(e.input.clone()))
            .children(message.map(|m| validation_note(m, border)));
    } else {
        // 021 五改：文件名前置（各行文件名同一起点左对齐、恒可见），目录段
        // 后置 dim、仅在过长时收缩截断（中段省略），尾部弹性空隙吃掉剩余宽
        el = el.child(
            div()
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(t.text))
                .child(SharedString::from(file_name)),
        );
        el = el.child(
            div()
                .flex_shrink()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(dir_part)),
        );
        el = el.child(div().flex_1());
        let fp = row.path.clone();
        let weak_click = weak.clone();
        // on_click 与树行同款（mouse-up 触发，面板内点击语义统一）；点击
        // 即选中（082，F2 改名对象）
        el = el.on_click(move |_, _, cx| {
            let _ = weak_click.update(cx, |c, cx| {
                c.ws.tree_selected = Some(fp.clone());
                c.open_file_tab(fp.clone(), cx);
            });
        });
    }
    el.child(super::rows::tree_right_click(&weak, &row)).into_any_element()
}
