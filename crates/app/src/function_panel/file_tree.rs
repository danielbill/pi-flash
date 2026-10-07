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

use std::ops::Range;

use gpui::{
    Bounds, MouseButton, Pixels, Point, SharedString, UniformListDecoration, div, hsla,
    linear_color_stop, linear_gradient, point, prelude::*, px, rgb,
};

use crate::Chat;
use crate::services::file_icons;
use crate::services::file_tree::TreeRow;
use crate::theme;
use crate::ui::{VListHeight, icon_path, vlist};

/// 行高（pi-web FileExplorer 24px 行）。
const ROW_H: f32 = 24.;
/// 每级缩进（Zed project_panel `indent_size` 默认 20px）。
const INDENT: f32 = 20.;
/// 内容列左右内边距（Zed ListItem `px(DynamicSpacing::Base06)` = 6px）。
const CONTENT_PAD: f32 = 6.;
/// 引导线水平偏移：落在父级行图标列中央（图标 14px @ [6,20)，中心 13；
/// Zed 常量 15 是按其 16px 图标列取的）。
const GUIDE_X: f32 = 13.;
/// 文件树 vlist 的元素 id——sticky 点击「滚到目录」按同一 id 取句柄。
const FILE_TREE_LIST_ID: &str = "file-tree";

pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let rows = chat.tree_rows.clone();
    let weak = weak.clone();
    let t = theme::theme();
    let sticky = Box::new(StickyAncestors { weak: weak.clone() }) as Box<dyn UniformListDecoration>;
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(t.nav))
        .py(px(10.))
        .child(vlist(
            FILE_TREE_LIST_ID,
            rows.len(),
            ROW_H,
            VListHeight::Fill,
            false,
            true,
            "暂无文件",
            Some(sticky),
            move |ix, _window, _cx| match rows.get(ix) {
                Some(row) => {
                    // 邻行深度决定各段引导线的首尾裁剪（整段只在首行留上
                    // 4px、尾行留下 4px，段中连续成一条线）
                    let prev_depth = if ix == 0 { None } else { rows.get(ix - 1).map(|r| r.depth) };
                    let next_depth = rows.get(ix + 1).map(|r| r.depth);
                    tree_row(row.clone(), prev_depth, next_depth, weak.clone())
                }
                None => div().into_any_element(),
            },
        ))
        .into_any_element()
}

/// One flattened row: indent guides + chevron + type icon + name + git badge.
fn tree_row(
    row: TreeRow,
    prev_depth: Option<usize>,
    next_depth: Option<usize>,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    let mut el = div()
        .id(SharedString::from(format!("tree-{}", row.path.display())))
        .relative()
        .w_full()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .overflow_hidden()
        .pr(px(8.))
        .rounded(px(4.))
        // 目录树 = 面板设置值 -1（字体大小设置.md §1；text_xs 固定 12px
        // 不随设置走）
        .text_size(crate::appearance::ui_size(11.))
        // gitignored 行置灰（Zed parity：显示但 dimmed，仍可点开浏览）
        .text_color(rgb(if row.ignored { t.text_faint } else { t.text }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)));

    // 缩进引导线：Zed indent_guides 装饰层的行内画法（见 guide_lines）。
    for g in guide_lines(row.depth, prev_depth, next_depth, t.border) {
        el = el.child(g);
    }

    // 内容列：Zed ListItem 内层 ml(深度×缩进) + px(Base06)，引导线与悬停
    // 背景都在行全宽上（Zed 行底色也是全宽的）。
    let mut inner = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .ml(px(row.depth as f32 * INDENT))
        .px(px(CONTENT_PAD));

    if row.is_dir {
        // Zed project_panel 默认 folder_indicator = "icon"：纯文件夹图标，
        // 无 chevron，展开态由开/闭图标区分（面板后续若加该设置再接
        // get_folder_indicators 的 Both/Chevron 分支）
        let folder = file_icons::get_generic_folder_icon(row.expanded);
        inner = inner
            .child(
                div()
                    .flex_shrink_0()
                    .child(icon_path(folder, 14., t.text_dim)),
            )
            .child(name_label(&row.name));
        el = el.child(inner);
        let dir_path = row.path.clone();
        el = el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                if !c.expanded_dirs.remove(&dir_path) {
                    c.expanded_dirs.insert(dir_path.clone());
                }
                c.rebuild_tree();
                cx.notify();
            });
        });
        // 目录圆点：子树有变更（祖先链上浮，services 层已算好）
        if row.changed_dot && crate::services::workspace::git_markers() {
            el = el.child(changed_dot());
        }
    } else {
        let icon = file_icons::get_icon(&row.path);
        inner = inner
            .child(
                div()
                    .flex_shrink_0()
                    .child(icon_path(icon, 14., t.text_dim)),
            )
            .child(name_label(&row.name));
        el = el.child(inner);
        let fp = row.path.clone();
        // 023：文件点击一律进 fileView（编辑器区）。旧行为「带 git 变更
        // 徽标的文件点击弹 GitDiff」废弃——diff 入口收敛在 git 面板。
        el = el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.open_file_tab(fp.clone(), cx);
            });
        });
        // git badge on files（pi-web 徽标语义：M/A/D/R/U/C；023 默认关）
        if let Some(st) = row.git.filter(|_| crate::services::workspace::git_markers()) {
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
    el.into_any_element()
}

fn name_label(name: &str) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        // Zed 内容列 gap = DynamicSpacing::Base06（图标 → 名称 6px）
        .ml(px(6.))
        .child(SharedString::from(name.to_string()))
        .into_any_element()
}

/// 缩进引导线（Zed indent_guides 装饰层的行内画法）：第 L 级竖线在
/// L×INDENT+GUIDE_X（父级行图标列中央）；整段只在首行缩顶部 4px、尾行缩
/// 底部 4px（Zed PADDING_Y），段中行互相续上成一条线。`prev`/`next` 是
/// 渲染语境里上/下邻行的深度（普通行 = 展平表邻居，钉住行 = 链上邻居）。
fn guide_lines(
    depth: usize,
    prev: Option<usize>,
    next: Option<usize>,
    color: u32,
) -> impl Iterator<Item = gpui::AnyElement> {
    (0..depth).map(move |level| {
        let top = if prev.is_none_or(|d| d <= level) { 4. } else { 0. };
        let bottom = if next.is_none_or(|d| d <= level) { 4. } else { 0. };
        div()
            .absolute()
            .left(px(level as f32 * INDENT + GUIDE_X))
            .top(px(top))
            .h(px(ROW_H - top - bottom))
            .w(px(1.))
            .bg(rgb(color))
            .into_any_element()
    })
}

/// 目录圆点：子树有变更（amber，pi-web 同色）。
fn changed_dot() -> gpui::AnyElement {
    div()
        .size(px(6.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(0xd6a84b))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// sticky 祖先链 —— Zed project_panel sticky_scroll（crates/ui/src/components/
// sticky_items.rs + project_panel.rs render_sticky_entries 同构）
// ---------------------------------------------------------------------------

/// 钉住祖先链的装饰层：可见行里找锚点（祖先已滚出视口的行），把它的祖先
/// 目录链钉在列表顶部。挂在 vlist 的 `UniformListDecoration` 上，每次
/// paint 按当前可见范围与滚动位重算——滚动中实时跟手（行闭包只在视口内
/// 重建，装饰层同样只在 paint 期取数，render 不多跑）。
struct StickyAncestors {
    weak: gpui::WeakEntity<Chat>,
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
            .update(cx, |chat, _| self.render_stack(chat, visible_range, scroll_offset))
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
        let rows = &chat.tree_rows;
        // Zed show_sticky_entries 门控：未滚动时不钉（否则「深度 < 视口
        // 序号」规则会在兄弟较多的树上于 scroll 0 误触发，把根行钉成重影）
        if scroll_offset.y >= px(0.) {
            return div().into_any_element();
        }
        let Some((anchor_ix, drifting)) = find_sticky_anchor(rows, visible_range) else {
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
            let prev_depth = if k == 0 { None } else { Some(rows[chain[k - 1]].depth) };
            // 末行引导线向锚点行续（不封底），与列表里的行无缝相接
            let next_depth =
                if k == last { Some(rows[anchor_ix].depth) } else { Some(rows[chain[k + 1]].depth) };
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
        .child(div().flex_shrink_0().child(icon_path(folder, 14., t.text_dim)))
        .child(name_label(&row.name));
    el = el.child(inner);
    if row.changed_dot && crate::services::workspace::git_markers() {
        el = el.child(changed_dot());
    }
    // 点击滚到该目录：让目录行落在钉住区里它自己的槽位（Zed
    // scroll_to_item_strict_with_offset(index, Top, sticky_index)），再
    // +1px 防「刚好落在钉住位上重新参与锚点判定」的抖动
    let target_y = px((slot as f32 - row_ix as f32) * ROW_H + 1.);
    el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        let handle = crate::ui::vlist::scroll_handle(FILE_TREE_LIST_ID);
        handle.0.borrow().base_handle.set_offset(point(px(0.), target_y));
        let _ = weak.update(cx, |_, cx| cx.notify());
        cx.stop_propagation();
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
fn find_sticky_anchor(rows: &[TreeRow], visible: Range<usize>) -> Option<(usize, bool)> {
    for abs in visible.start..visible.end {
        let ix = abs - visible.start;
        let Some(row) = rows.get(abs) else { break };
        let depth = row.depth;
        if depth < ix {
            return Some((abs, false));
        }
        if let Some(next) = rows.get(abs + 1)
            && next.depth + 1 == depth
            && (depth == ix || depth == ix + 1)
        {
            return Some((abs, depth == ix + 1));
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
        assert_eq!(find_sticky_anchor(&rows, 0..9), Some((3, false)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
    }

    #[test]
    fn deep_subtree_pins_full_chain() {
        // 滚到 a1（视口 2..9）：a2 是子树内最后可见行且下一行退出子树、
        // 深度恰比视口序号大 1 → 锚点 a2、漂移态；链 = [root, A]（A 已滚出）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 2..9), Some((3, true)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
        assert_eq!(ancestor_chain(&rows, 6), vec![0, 4, 5]);
    }

    #[test]
    fn anchor_at_subtree_tail_with_visible_parent() {
        // 视口 [A, a1, a2, B, ...]（滚 1 行）：a2 下一行退出子树、深度恰
        // 等于视口序号 → 锚点 a2、不漂移，链 = [root, A]（A 在视口内，钉住
        // 行覆盖其上，Zed 同款）
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 1..9), Some((3, false)));
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1]);
    }

    #[test]
    fn drifting_when_anchor_one_row_past_its_slot() {
        // 视口 [a1, a2, B, ...]（滚 2 行）：a2 深度 2 = 视口序号 1 + 1 且
        // 下一行退出子树 → 漂移态
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 2..9), Some((3, true)));
    }

    #[test]
    fn anchor_walks_into_next_subtree_after_tail() {
        // 继续滚 1 行（视口 [a2, B, b1, bk, C, ...]）：a2 不再满足任何规则，
        // bk 深度 3 > 序号 1 也不满足，规则①在 C（深度 1 < 序号 2）命中。
        // C 与 B 是兄弟，链只有根
        let rows = fixture();
        assert_eq!(find_sticky_anchor(&rows, 3..9), Some((7, false)));
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
        assert_eq!(find_sticky_anchor(&rows, 0..6), Some((4, false)));
        assert_eq!(ancestor_chain(&rows, 5), vec![0, 4]);
        assert_eq!(ancestor_chain(&rows, 3), vec![0, 1, 2]);
    }
}
