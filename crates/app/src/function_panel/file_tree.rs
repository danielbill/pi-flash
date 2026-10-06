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

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

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

pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let rows = chat.tree_rows.clone();
    let weak = weak.clone();
    let t = theme::theme();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(t.nav))
        .py(px(10.))
        .child(vlist(
            "file-tree",
            rows.len(),
            ROW_H,
            VListHeight::Fill,
            false,
            true,
            "暂无文件",
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
        .text_color(rgb(t.text))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)));

    // 缩进引导线：Zed indent_guides 装饰层的行内画法——第 L 级竖线在
    // L×INDENT+GUIDE_X（父级 chevron 列中央），每段只在首行缩进顶部 4px、
    // 尾行缩进底部 4px（Zed PADDING_Y），段中行互相续上成一条线。带
    // hitbox 的点击折叠后续接。
    for level in 0..row.depth {
        let top = if prev_depth.is_none_or(|d| d <= level) { 4. } else { 0. };
        let bottom = if next_depth.is_none_or(|d| d <= level) { 4. } else { 0. };
        el = el.child(
            div()
                .absolute()
                .left(px(level as f32 * INDENT + GUIDE_X))
                .top(px(top))
                .h(px(ROW_H - top - bottom))
                .w(px(1.))
                .bg(rgb(t.border)),
        );
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
        if row.changed_dot {
            el = el.child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .flex_shrink_0()
                    .bg(rgb(0xd6a84b)),
            );
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
        let is_changed = row.git.is_some();
        el = el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                if is_changed {
                    c.open_git_diff(fp.clone(), cx);
                } else {
                    c.open_file_tab(fp.clone(), cx);
                }
            });
        });
        // git badge on files（pi-web 徽标语义：M/A/D/R/U/C）
        if let Some(st) = row.git {
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
