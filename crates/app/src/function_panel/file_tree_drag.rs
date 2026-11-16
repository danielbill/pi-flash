//! 文件树拖拽移动（Zed project_panel drag_and_drop 移植，021）：gpui 的
//! 窗口内 DnD 三件套（`on_drag` 发起 + `drag_over` 落点高亮 + `on_drop`
//! 提交）挂到树行上，落点语义与实际移动在 [`drag_move_target`] /
//! `Chat::tree_move_commit`（file_actions）。
//!
//! vendored gpui 0.2.2 没有 `external_drag_payload` 一族 API——只能做窗口
//! 内拖拽 + 接收 OS 拖入，拖不出去到资源管理器（Zed 有，暂缺不算陷阱，
//! 见 docs/agents/gpui.md）。

use std::path::Path;

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::appearance::theme;
use crate::same_path;
use crate::services::{file_icons, workspace};
use crate::ui::icon_path;

/// 悬停折叠目录自动展开的等待时长（Zed hover_expand_task 同款 500ms）。
pub(crate) const DRAG_EXPAND_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// 树条目拖拽载荷（`on_drag` 的 value；drop 监听按 TypeId 匹配，与其他
/// 拖拽源互不串扰）。021 多选：`marked` = 拖拽起手时标记集的渲染期快照
/// （Zed `DraggedSelection { active_selection, marked_selections }` 同构
/// ——拖的是标记集成员 → 整组移动，否则单条目即全量，判定在
/// [`drag_items`]）。
#[derive(Debug, Clone)]
pub(crate) struct TreeDrag {
    pub(crate) path: std::path::PathBuf,
    pub(crate) is_dir: bool,
    pub(crate) marked: std::sync::Arc<Vec<(std::path::PathBuf, bool)>>,
}

/// Zed `DraggedSelection::items` 挪植：拖的是标记集成员 → 整组（起手期
/// 快照，drop 期不再读实体）；否则只拖自己。返回 (路径, is_dir) 逐条
/// 过落点判定。
pub(crate) fn drag_items(drag: &TreeDrag) -> Vec<(std::path::PathBuf, bool)> {
    if drag.marked.iter().any(|(p, _)| same_path(p, &drag.path)) {
        drag.marked.as_ref().clone()
    } else {
        vec![(drag.path.clone(), drag.is_dir)]
    }
}

/// 拖放落点判定（Zed move_worktree_entry 语义）：悬停目录行 = 移入该目录；
/// 悬停文件行 = 移到其父目录。返回 `Some(目标目录)` 表示这是一次**真实
/// 移动**——原地（拖到自己在的目录）、进自身子树、拖到自己身上都返回
/// None：不高亮、drop 静默 no-op（Zed 对高亮的抑制规则同源）。
pub(crate) fn drag_move_target(
    src: &Path,
    src_is_dir: bool,
    row: &Path,
    row_is_dir: bool,
) -> Option<std::path::PathBuf> {
    // 源行在拖拽中照常渲染、照常可悬停：拖回自己 = 不动
    if same_path(row, src) {
        return None;
    }
    let dst_dir = if row_is_dir { row } else { row.parent()? };
    // 目录进自身/子孙子树：fs 上不可能，语义上拒绝（大小写折叠走
    // workspace 的字符串规范化，Windows 路径逐字节比会对不上）
    if src_is_dir && workspace::is_under(dst_dir, src) {
        return None;
    }
    // 已在目标目录（文件行父目录 == 源父目录，含拖到兄弟文件行上）：原地
    if src.parent().is_some_and(|p| same_path(p, dst_dir)) {
        return None;
    }
    Some(dst_dir.to_path_buf())
}

/// 拖拽幽灵（gpui 把 `cx.active_drag.view` 画在光标处）：图标 + 名字的
/// 圆角小签，Zed DraggedProjectEntryView 的极简版。
pub(crate) struct TreeDragGhost {
    pub(crate) path: std::path::PathBuf,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
}

impl gpui::Render for TreeDragGhost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let t = theme::theme();
        let icon = if self.is_dir {
            file_icons::get_generic_folder_icon(false)
        } else {
            file_icons::get_icon(&self.path)
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(4.))
            .max_w(px(260.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.nav))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text))
            .child(icon_path(icon, 14., t.text_dim))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(self.name.clone())),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{TreeDrag, drag_items, drag_move_target};
    use std::sync::Arc;

    fn p(s: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(s)
    }

    fn drag(path: &str, is_dir: bool, marked: Vec<(&str, bool)>) -> TreeDrag {
        TreeDrag {
            path: p(path),
            is_dir,
            marked: Arc::new(marked.into_iter().map(|(s, d)| (p(s), d)).collect()),
        }
    }

    #[test]
    fn drag_items_group_when_marked_else_single() {
        // 拖的是标记集成员 → 整组（Zed DraggedSelection::items 同语义）
        let d = drag(
            "r/b",
            false,
            vec![("r/a", false), ("r/b", false), ("r/sub", true)],
        );
        assert_eq!(
            drag_items(&d),
            vec![(p("r/a"), false), (p("r/b"), false), (p("r/sub"), true)]
        );
        // 拖的不是标记集成员（起手后 ctrl+点清掉了）→ 单条目即全量
        let d = drag("r/x", false, vec![("r/a", false)]);
        assert_eq!(drag_items(&d), vec![(p("r/x"), false)]);
        // 空标记集（未多选直接拖）→ 单条目
        let d = drag("r/y", true, vec![]);
        assert_eq!(drag_items(&d), vec![(p("r/y"), true)]);
    }

    #[test]
    fn dir_row_targets_itself_file_row_targets_parent() {
        // 悬停目录 = 移入；悬停文件 = 移到其父目录（Zed 语义）
        assert_eq!(
            drag_move_target(&p("r/a.txt"), false, &p("r/sub"), true),
            Some(p("r/sub"))
        );
        assert_eq!(
            drag_move_target(&p("r/other/b.txt"), false, &p("r/sub/inner.txt"), false),
            Some(p("r/sub"))
        );
    }

    #[test]
    fn no_op_moves_are_none() {
        // 拖到自己身上；拖回所在目录（目录行与兄弟文件行两条路）；拖到
        // 自己子孙目录里——全部 None
        assert_eq!(drag_move_target(&p("r/a"), true, &p("r/a"), true), None);
        assert_eq!(
            drag_move_target(&p("r/sub/x.txt"), false, &p("r/sub"), true),
            None
        );
        assert_eq!(
            drag_move_target(&p("r/sub/x.txt"), false, &p("r/sub/y.txt"), false),
            None
        );
        assert_eq!(
            drag_move_target(&p("r/dir"), true, &p("r/dir/inner"), true),
            None
        );
        assert_eq!(
            drag_move_target(&p("r/dir"), true, &p("r/dir/inner/f.txt"), false),
            None
        );
    }

    #[test]
    fn real_moves_survive() {
        // 跨目录真移动：文件、目录、根下互移都是 Some
        assert_eq!(
            drag_move_target(&p("r/a/b.txt"), false, &p("r/c"), true),
            Some(p("r/c"))
        );
        assert_eq!(
            drag_move_target(&p("r/a/dir"), true, &p("r/b"), true),
            Some(p("r/b"))
        );
    }
}
