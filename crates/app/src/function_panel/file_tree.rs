//! 文件树渲染（FileExplorer.tsx TreeNodeView parity + Zed project_panel
//! 渲染结构）：消费 `services::file_tree` 的展平缓存 [`TreeRow`]，vlist
//! 虚拟化——只构建视口内的行，render 不再碰磁盘。
//!
//! 行结构对齐 Zed project_panel：缩进引导线（每级一格）→ chevron → 目录/
//! 文件类型图标（Zed "Zed (Default)" 主题）→ 名称（ellipsis）→ git 徽标
//! / 变更目录圆点。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::services::file_icons;
use crate::services::file_tree::TreeRow;
use crate::theme;
use crate::ui::{VListHeight, icon_path, vlist};

/// 行高（pi-web FileExplorer 24px 行）。
const ROW_H: f32 = 24.;
/// 每级缩进（pi-web 14px；引导线占同宽格子，后续可接点击折叠）。
const INDENT: f32 = 14.;
/// 基础缩进（第一级之前的留白）。
const BASE_INDENT: f32 = 8.;

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
        .px(px(6.))
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
                Some(row) => tree_row(row.clone(), weak.clone()),
                None => div().into_any_element(),
            },
        ))
        .into_any_element()
}

/// One flattened row: indent guides + chevron + type icon + name + git badge.
fn tree_row(row: TreeRow, weak: gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = theme::theme();
    let mut el = div()
        .id(SharedString::from(format!("tree-{}", row.path.display())))
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

    // 缩进引导线：每级一个 INDENT 宽格子，中央 1px 竖线（Zed
    // indent_guides 的行内画法；带 hitbox 的点击折叠后续接）
    for _ in 0..row.depth {
        el = el.child(
            div()
                .flex_shrink_0()
                .w(px(INDENT))
                .h_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w(px(1.))
                        .h_full()
                        .my(px(2.))
                        .bg(rgb(t.border)),
                ),
        );
    }
    el = el.child(div().w(px(BASE_INDENT)).flex_shrink_0());

    if row.is_dir {
        // Zed project_panel 同款：folder_indicator 统一解析 chevron+icon
        //（面板暂无该设置，用 Zed 默认 Both）
        let ind = file_icons::get_folder_indicators(
            file_icons::FolderIndicator::default(),
            row.expanded,
        );
        let (chevron, folder) = (
            ind.chevron.unwrap_or(file_icons::get_chevron_icon(row.expanded)),
            ind.icon
                .unwrap_or(file_icons::get_generic_folder_icon(row.expanded)),
        );
        el = el
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(12.))
                    .flex()
                    .justify_center()
                    .child(icon_path(chevron, 10., t.text_dim)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .child(icon_path(folder, 14., t.text_dim)),
            )
            .child(name_label(&row.name));
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
                    .ml_auto()
                    .flex_shrink_0()
                    .bg(rgb(0xd6a84b)),
            );
        }
    } else {
        let icon = file_icons::get_icon(&row.path);
        el = el
            .child(div().w(px(12.)).flex_shrink_0())
            .child(div().flex_shrink_0().child(icon_path(icon, 14., t.text_dim)))
            .child(name_label(&row.name));
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
                    .ml_auto()
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
        .ml(px(4.))
        .child(SharedString::from(name.to_string()))
        .into_any_element()
}
