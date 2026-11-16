//! sticky 祖先链 —— Zed project_panel sticky_scroll（crates/ui/src/components/
//! sticky_items.rs + project_panel.rs render_sticky_entries 同构）。自
//! file_tree.rs 内切拆出（check_arch 文件 1500 行上限），纯结构搬移。

use std::ops::Range;

use gpui::{
    Bounds, MouseButton, Pixels, Point, UniformListDecoration, div, hsla, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb,
};

use crate::Chat;
use crate::appearance::theme;
use crate::function_panel::file_tree_drag::{TreeDrag, drag_items, drag_move_target};
use crate::services::file_icons;
use crate::services::file_tree::TreeRow;
use crate::ui::icon_path;

use super::file_tree::{
    CONTENT_PAD, FILE_TREE_LIST_ID, INDENT, ROW_H, changed_dot, guide_lines, name_label,
    tree_right_click,
};

/// 钉住祖先链的装饰层：可见行里找锚点（祖先已滚出视口的行），把它的祖先
/// 目录链钉在列表顶部。挂在 vlist 的 `UniformListDecoration` 上，每次
/// paint 按当前可见范围与滚动位重算——滚动中实时跟手（行闭包只在视口内
/// 重建，装饰层同样只在 paint 期取数，render 不多跑）。`edit_ix` = 行内
/// 新建的虚拟行位置（不在 tree_rows 里，可见范围做一次索引平移）。
pub(crate) struct StickyAncestors {
    pub(crate) weak: gpui::WeakEntity<Chat>,
    pub(crate) edit_ix: Option<usize>,
}

impl UniformListDecoration for StickyAncestors {
    fn compute(
        &self,
        visible_range: Range<usize>,
        _bounds: Bounds<Pixels>,
        scroll_offset: Point<Pixels>,
        _item_height: Pixels,
        _item_count: usize,
        _window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> gpui::AnyElement {
        // paint 期 Chat 的 render 借用已结束，update 合法（Zed sticky_items
        // 同款）；weak 失联（理论上不发生）就整层不画
        if self.weak.upgrade().is_none() {
            return div().into_any_element();
        }
        self.weak
            .update(cx, |chat, _| {
                self.render_stack(chat, visible_range, scroll_offset)
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl StickyAncestors {
    fn render_stack(
        &self,
        chat: &Chat,
        visible_range: Range<usize>,
        scroll_offset: Point<Pixels>,
    ) -> gpui::AnyElement {
        let rows = &chat.ws.tree_rows;
        // Zed show_sticky_entries 门控：未滚动时不钉（否则「深度 < 视口
        // 序号」规则会在兄弟较多的树上于 scroll 0 误触发，把根行钉成重影）
        if scroll_offset.y >= px(0.) {
            return div().into_any_element();
        }
        let Some((anchor_ix, drifting)) = find_sticky_anchor(rows, visible_range, self.edit_ix)
        else {
            return div().into_any_element();
        };
        let chain = ancestor_chain(rows, anchor_ix);
        if chain.is_empty() {
            return div().into_any_element();
        }

        // 装饰层坐标系随内容滚动（prepaint 原点已含 scroll_offset），钉在
        // 屏幕上 = 每行 top 补偿 -scroll_offset.y（全按 f32 算，px() 只在
        // 落样式处取）
        let scroll_y = f32::from(scroll_offset.y);
        let compensation = -scroll_y;
        let stack_h = chain.len() as f32 * ROW_H;
        // Zed 漂移态：锚点行滚进钉住区时，链末行（锚点父目录）贴着锚点行
        // 底缘一起上滑，直到被钉住区吞没
        let drift = if drifting {
            let anchor_bottom = (anchor_ix as f32 + 1.) * ROW_H + scroll_y;
            (anchor_bottom - stack_h).min(0.)
        } else {
            0.
        };

        let last = chain.len() - 1;
        let mut stack = div().relative().w_full();
        for (k, &row_ix) in chain.iter().enumerate() {
            let y = px(k as f32 * ROW_H + if k == last { drift } else { 0. } + compensation);
            let prev_depth = if k == 0 {
                None
            } else {
                Some(rows[chain[k - 1]].depth)
            };
            // 末行引导线向锚点行续（不封底），与列表里的行无缝相接
            let next_depth = if k == last {
                Some(rows[anchor_ix].depth)
            } else {
                Some(rows[chain[k + 1]].depth)
            };
            stack = stack.child(sticky_dir_row(
                &rows[row_ix],
                prev_depth,
                next_depth,
                y,
                row_ix,
                k,
                self.weak.clone(),
            ));
        }
        // Zed 同款：钉住区底缘 1.5px 渐隐阴影（黑 0.12 → 0）
        stack = stack.child(
            div()
                .absolute()
                .left_0()
                .w_full()
                .top(px(stack_h + drift + compensation - 1.5))
                .h(px(1.5))
                .bg(linear_gradient(
                    0.,
                    linear_color_stop(hsla(0., 0., 0., 0.12), 1.),
                    linear_color_stop(hsla(0., 0., 0., 0.), 0.),
                )),
        );
        stack.into_any_element()
    }
}

/// 钉住的目录行：普通目录行的视觉（引导线按链上邻居接续）+ 点击滚回
/// 对应目录（Zed sticky 点击语义），并拦截冒泡——底下压着的列表行不得
/// 收到这次点击（否则会触发折叠）。
#[allow(clippy::too_many_arguments)]
fn sticky_dir_row(
    row: &TreeRow,
    prev_depth: Option<usize>,
    next_depth: Option<usize>,
    top: Pixels,
    row_ix: usize,
    slot: usize,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    let mut el = div()
        .absolute()
        .left_0()
        .w_full()
        .top(top)
        .h(px(ROW_H))
        .flex()
        .items_center()
        .overflow_hidden()
        .pr(px(8.))
        .rounded(px(4.))
        .bg(rgb(t.nav))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)));
    for g in guide_lines(row.depth, prev_depth, next_depth, t.border) {
        el = el.child(g);
    }
    let folder = file_icons::get_generic_folder_icon(row.expanded);
    let inner = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .ml(px(row.depth as f32 * INDENT))
        .px(px(CONTENT_PAD))
        .child(
            div()
                .flex_shrink_0()
                .child(icon_path(folder, 14., t.text_dim)),
        )
        .child(name_label(&row.name));
    el = el.child(inner);
    if row.changed_dot && crate::services::workspace::git_markers() {
        el = el.child(changed_dot());
    }
    // 点击滚到该目录：让目录行落在钉住区里它自己的槽位（Zed
    // scroll_to_item_strict_with_offset(index, Top, sticky_index)），再
    // +1px 防「刚好落在钉住位上重新参与锚点判定」的抖动
    let target_y = px((slot as f32 - row_ix as f32) * ROW_H + 1.);
    let weak_menu = weak.clone();
    // 拖拽落点：钉住的祖先目录同样可作目标（Zed 折叠目录条
    // FoldedDirectoryDragTarget 同语义）。钉住行画在列表之上，冒泡相
    // 先于底下压着的列表行拿走 drop（与上面右键拦截同一机制）
    let drag_row_path = row.path.clone();
    let drop_row_path = row.path.clone();
    let weak_drop = weak.clone();
    el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        let handle = crate::ui::vlist::scroll_handle(FILE_TREE_LIST_ID);
        handle
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), target_y));
        let _ = weak.update(cx, |_, cx| cx.notify());
        cx.stop_propagation();
    })
    .child(tree_right_click(&weak_menu, row))
    .drag_over::<TreeDrag>(move |style, drag, _, _| {
        // 多选拖拽任一条目有真实移动即亮（rows.rs 行侧同语义）
        let any = drag_items(&drag).iter().any(|(src, src_is_dir)| {
            drag_move_target(src, *src_is_dir, &drag_row_path, true).is_some()
        });
        if any {
            style.bg(rgb(theme::theme().bg_hover))
        } else {
            style
        }
    })
    .on_drop::<TreeDrag>(move |drag, _, cx| {
        let _ = weak_drop.update(cx, |c, cx| {
            // 标记集成员 = 整组逐条判定落点（rows.rs 行侧同语义）
            for (src, src_is_dir) in drag_items(&drag) {
                if let Some(dst) = drag_move_target(&src, src_is_dir, &drop_row_path, true) {
                    c.tree_move_commit(&src, src_is_dir, &dst, cx);
                }
            }
        });
    })
    .into_any_element()
}

/// Zed `ui::sticky_items::find_sticky_anchor` 同构：在可见行里找锚点行，
/// 返回（绝对行号，是否漂移）。
///
/// - ① `depth < 视口序号`：该行的祖先已滚出视口（序号 0..ix 里放不下
///   depth 个祖先）；
/// - ② `depth == 序号(+1)` 且下一行退出本子树（`next.depth + 1 == depth`）：
///   该行是子树内最后可见的行，其父链即将全部滚出；+1 的情况（深度刚好
///   比序号大 1）父链只差一行滚出，标记 drifting（链末行跟着锚点滑）。
fn find_sticky_anchor(
    rows: &[TreeRow],
    visible: Range<usize>,
    edit_ix: Option<usize>,
) -> Option<(usize, bool)> {
    // 行内新建的虚拟行夹在列表中间：输出序号 → tree_rows 序号平移一位
    // （虚拟行自身不参与锚点判定）
    let to_row = |abs: usize| -> Option<usize> {
        match edit_ix {
            Some(eix) if abs == eix => None,
            Some(eix) if abs > eix => Some(abs - 1),
            _ => Some(abs),
        }
    };
    for abs in visible.start..visible.end {
        let ix = abs - visible.start;
        let Some(row_ix) = to_row(abs) else { continue };
        let Some(row) = rows.get(row_ix) else { break };
        let depth = row.depth;
        if depth < ix {
            return Some((row_ix, false));
        }
        // 下一「输出行」的深度：虚拟行夹在中间时取虚拟行前一行（近似——
        // 虚拟行深度与被编辑目录子项同级，规则只做引导线裁剪，误差可忽略）
        let next_row_ix = row_ix + 1;
        if Some(abs + 1) == edit_ix {
            continue;
        }
        if let Some(next) = rows.get(next_row_ix)
            && next.depth + 1 == depth
            && (depth == ix || depth == ix + 1)
        {
            return Some((row_ix, depth == ix + 1));
        }
    }
    None
}

/// 锚点行的祖先行号（根在前）。DFS 展平里「前面最近的 depth-k 行」必是
/// depth-k 祖先（锚点行与各祖先之间只可能夹更深的行），折叠链行
/// （a/b/c 一行）不破坏该性质。
fn ancestor_chain(rows: &[TreeRow], anchor_ix: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    let mut cur = anchor_ix;
    while rows[cur].depth > 0 {
        let target = rows[cur].depth - 1;
        let mut found = None;
        let mut j = cur;
        while j > 0 {
            j -= 1;
            if rows[j].depth == target {
                found = Some(j);
                break;
            }
        }
        let Some(j) = found else {
            // 理论不可达：根行（depth 0）恒在 index 0
            return Vec::new();
        };
        chain.push(j);
        cur = j;
    }
    chain.reverse();
    chain
}

#[cfg(test)]
mod sticky_tests {
    use std::path::PathBuf;

    use super::{ancestor_chain, find_sticky_anchor};
    use crate::services::file_tree::TreeRow;

    fn row(name: &str, depth: usize) -> TreeRow {
        TreeRow {
            path: PathBuf::from(name),
            name: name.to_string(),
            depth,
            is_dir: true,
            expanded: true,
            git: None,
            changed_dot: false,
            ignored: false,
        }
    }

    /// root(0) / A(1) / a1 a2(2) / B(1) / b1(2) / bk(3) / C(1) / c(2)
    fn fixture() -> Vec<TreeRow> {
        vec![
            row("root", 0),
            row("A", 1),
            row("a1", 2),
            row("a2", 2),
            row("B", 1),
            row("b1", 2),
            row("bk", 3),
            row("C", 1),
            row("c", 2),
        ]
    }

    #[test]
    fn anchor_fires_on_first_row_shallower_than_viewport_index() {
        // 未滚动（门控由渲染层负责，此处测纯函数）：a2 深度 2 < 视口序号 3
        // → 首个命中行为锚点。滚动 1px 起即钉 [root, A]——钉住行恰好盖在
        // 这两行的真实位置上，视觉无缝（Zed 同款行为）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 0..9, None), Some((3, false)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
    }

    #[test]
    fn deep_subtree_pins_full_chain() {
        // 滚到 a1（视口 2..9）：a2 是子树内最后可见行且下一行退出子树、
        // 深度恰比视口序号大 1 → 锚点 a2、漂移态；链 = [root, A]（A 已滚出）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 2..9, None), Some((3, true)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
        assert_eq!(ancestor_chain(&rows, 6), vec![0, 4, 5]);
    }

    #[test]
    fn anchor_at_subtree_tail_with_visible_parent() {
        // 视口 [A, a1, a2, B, ...]（滚 1 行）：a2 下一行退出子树、深度恰
        // 等于视口序号 → 锚点 a2、不漂移，链 = [root, A]（A 在视口内，钉住
        // 行覆盖其上，Zed 同款）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 1..9, None), Some((3, false)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
    }

    #[test]
    fn drifting_when_anchor_one_row_past_its_slot() {
        // 视口 [a1, a2, B, ...]（滚 2 行）：a2 深度 2 = 视口序号 1 + 1 且
        // 下一行退出子树 → 漂移态
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 2..9, None), Some((3, true)));
    }

    #[test]
    fn anchor_walks_into_next_subtree_after_tail() {
        // 继续滚 1 行（视口 [a2, B, b1, bk, C, ...]）：a2 不再满足任何规则，
        // bk 深度 3 > 序号 1 也不满足，规则①在 C（深度 1 < 序号 2）命中。
        // C 与 B 是兄弟，链只有根
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 3..9, None), Some((7, false)));
        assert_eq!(ancestor_chain(&rows, 7), vec![0]);
        // bk 在 B 子树深处：它的链才是 [root, B, b1]（锚点判定另有规则，
        // 这里只测链回溯）
        assert_eq!(ancestor_chain(&rows, 6), vec![0, 4, 5]);
    }

    #[test]
    fn chain_walk_skips_folded_and_sibling_rows() {
        // B 的父链要跳过 A 的整棵子树；折叠链行（depth 不变的多级目录）
        // 同样只按深度回溯。锚点：B 深度 1 < 视口序号 4（规则①）
        let rows = vec![
            row("root", 0),
            row("A", 1),
            row("a/deep/chain", 2),
            row("a/deeper", 3),
            row("B", 1),
            row("b/x/y", 2),
        ];
        assert_eq!(find_sticky_anchor(&rows, 0..6, None), Some((4, false)));
        assert_eq!(ancestor_chain(&rows, 5), vec![0, 4]);
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1, 2]);
    }
}
