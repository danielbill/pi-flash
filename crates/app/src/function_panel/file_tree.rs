//! 文件树渲染（FileExplorer.tsx TreeNodeView parity + Zed project_panel
//! 渲染结构）：消费 `services::file_tree` 的展平缓存 [`TreeRow`]，vlist
//! 虚拟化——只构建视口内的行，render 不再碰磁盘。
//!
//! 行结构对齐 Zed project_panel（list_item.rs + indent_guides.rs 的几何）：
//! 缩进引导线是行内绝对定位竖线（Zed 装饰层同位还原：x = 级×缩进 + 偏移，
//! 恰落在父级行图标列中央），内容列 = ml(深度×缩进) + px(6) → 目录/文件
//! 类型图标（Zed "Zed (Default)" 主题）→ 名称（ellipsis）→ git 徽标 /
//! 变更目录圆点。目录无 chevron（Zed 默认 folder_indicator = "icon"，
//! 展开态由开/闭文件夹图标区分）。
//!
//! sticky 祖先链（Zed project_panel `sticky_scroll`，ui::sticky_items +
//! render_sticky_entries 同构）挂在 vlist 的 `UniformListDecoration` 上：
//! 每次 paint 按当前可见范围/滚动位重算，滚进深层目录时把锚点行的祖先链
//! 钉在列表顶部，点击钉住行滚回对应目录。
//!
//! itlq.12 超规拆分后本文件只剩入口与常量：列表体（主树/变更 vlist）=
//! `lists`，行渲染（引导线/徽标/拖拽接线）= `rows`。下方 re-export 维持
//! 家族既有导入路径（sticky/pins/edit 三处消费 super::file_tree::*）。

use std::path::Path;

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::appearance::theme;
use crate::ui::WithTip;
use crate::*;
use crate::{Chat, i18n::tr, same_path};

use super::file_tree_pins::pin_list_view;

mod lists;
mod rows;
mod tree_action_bar;
mod tree_state;

pub(crate) use lists::tree_scrollbar;
pub(crate) use rows::{
    changed_dot, guide_lines, name_label, tree_drag_wiring, tree_right_click, tree_row,
};
pub(crate) use tree_action_bar::action_bar;

/// 行高（pi-web FileExplorer 24px 行）。
pub(crate) const ROW_H: f32 = 24.;
/// 每级缩进（Zed project_panel `indent_size` 默认 20px）。
pub(crate) const INDENT: f32 = 20.;
/// 内容列左右内边距（Zed ListItem `px(DynamicSpacing::Base06)` = 6px）。
pub(crate) const CONTENT_PAD: f32 = 6.;
/// 引导线水平偏移：落在父级行图标列中央（图标 14px @ [6,20)，中心 13；
/// Zed 常量 15 是按其 16px 图标列取的）。
const GUIDE_X: f32 = 13.;
/// 文件树 vlist 的元素 id——sticky 点击「滚到目录」按同一 id 取句柄。
pub(crate) const FILE_TREE_LIST_ID: &str = "file-tree";

pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let files_min = chat.layout.files_min;
    let rows = chat.ws.tree_rows.clone();
    let changes_only = chat.ws.changes_only;
    // 021 Pin 收藏夹视图（与变更视图互斥，toggle_pin_view 维护）
    let pin_view = chat.ws.pin_view;
    // 082 选中条目（点击/右键留存，行高亮 + F2 改名对象）在分支函数内取
    let weak = weak.clone();
    let t = theme::theme();
    // 021 操作栏：项目目录行固定在树顶，名字居左、操作钮右对齐（切换显示
    // 变更 / 搜索文件 / 资源管理器 / 终端 / 最大化），不随列表滚动
    let mut col = div()
        .id("sub-files")
        .w_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .bg(rgb(t.nav))
        // 020-1 文件树子面板伸缩：常态（含上会话面板最小）吃满 dock 剩余
        // 高度；本面板最小 → 只剩操作栏（auto 高）
        .when(files_min, |d| d.flex_shrink_0())
        .when(!files_min, |d| d.flex_1().min_h_0())
        // Zed EditState 收场主路：行内编辑激活时，面板内任何左键按下
        // （点别的行/空白/操作栏）= 提交。Zed 里这次点击先靠 uniform_list
        // `track_focus` 把焦点抢到面板（编辑行随之 blur → confirm_edit），
        // pi-flash 的面板不可聚焦（焦点保持见 `Chat::render` 级联末级，
        // 行内编辑存续期即焦点所有者），所以点击语义在面板本层直接兑现。
        // 编辑输入框内部点击在行内 stop_propagation 不受影响；点面板外的
        // 可聚焦目标（对话框/终端/文件编辑器/其它输入框）走真夺焦 → blur
        // 收场。
        .on_mouse_down(gpui::MouseButton::Left, {
            let weak_commit = weak.clone();
            move |_, _, cx| {
                let _ = weak_commit.update(cx, |c, cx| {
                    if c.dlg.tree_edit.is_some() {
                        c.tree_edit_confirm_from_click(cx);
                    }
                });
            }
        })
        .child(action_bar(chat, &weak, t));
    // 020-1：文件树最小态 = 只剩操作栏，列表体不渲染（vlist 句柄在
    // thread_local 表里，恢复时滚动位不丢）
    if !files_min && pin_view {
        col = col.child(pin_list_view(chat, rows, &weak, cx));
    } else if !files_min && changes_only {
        // 「只看变更」= 扁平列表（021 二改定稿：不要树状）。头行 =
        // N 个文件 + numstat（+绿 -红，色同 git 徽标盘）；行 = 文件名
        // 前置左对齐 + 目录段后置（目录段可截断、文件名恒可见）。
        let (add, del) = chat.ws.git_add_del;
        let n = rows.len();
        if n > 0 {
            let mut head = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tf(
                    "file_tree.files",
                    &[("n", n.to_string())],
                )));
            if add > 0 {
                head = head.child(
                    div()
                        .text_color(rgb(crate::services::git::GitStatus::Added.color()))
                        .child(SharedString::from(format!("+{add}"))),
                );
            }
            if del > 0 {
                head = head.child(
                    div()
                        .text_color(rgb(crate::services::git::GitStatus::Deleted.color()))
                        .child(SharedString::from(format!("-{del}"))),
                );
            }
            col = col.child(head);
        }
        col = col.child(lists::changes_list_view(chat, rows, &weak, cx));
    } else if !files_min {
        col = col.child(lists::tree_list_view(chat, rows, &weak, cx));
    }
    col.into_any_element()
}
