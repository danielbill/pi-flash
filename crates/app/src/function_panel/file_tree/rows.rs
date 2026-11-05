//! 文件树行渲染（itlq.12 自 file_tree.rs 切出，纯结构搬移）：Zed
//! project_panel 行结构 parity——缩进引导线（indent_guides 行内画法）、
//! 类型图标 + 名称 + git 徽标 / 变更目录圆点、拖拽接线三件套（on_drag/
//! drag_over/on_drop + 悬停自动展开）、行右键锚点。列表容器在
//! `super::lists`，行内编辑 chrome 在 file_tree_edit。

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme;
use crate::function_panel::file_tree_drag::{
    DRAG_EXPAND_DELAY, TreeDrag, TreeDragGhost, drag_move_target,
};
use crate::function_panel::file_tree_edit::{edit_chrome, edit_input_slot, validation_note};
use crate::services::file_icons;
use crate::services::file_tree::TreeRow;
use crate::ui::icon_path;
use crate::{same_path};

use super::{CONTENT_PAD, GUIDE_X, INDENT, ROW_H};

/// One flattened row: indent guides + chevron + type icon + name + git badge.
pub(crate) fn tree_row(
    row: TreeRow,
    prev_depth: Option<usize>,
    next_depth: Option<usize>,
    editing: Option<crate::function_panel::TreeEdit>,
    selected: bool,
    weak: gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = theme::theme();
    let weak_menu = weak.clone();
    // 拖拽接线用的句柄：分支里的点击闭包会把 weak 吃掉，先取一份
    let weak_drag = weak.clone();
    // 本行处于行内改名（Zed EditState 命中该行）：名字标签换输入框，点
    // 击展开/打开让位给输入框
    let editing = editing.filter(|e| {
        matches!(&e.kind, crate::function_panel::TreeEditKind::Rename { path } if same_path(path, &row.path))
    });
    // 082 选中态：10% 前景薄纱常驻（与 psp 会话行选中同色系）；编辑态
    // 行让位给校验边框，不再叠选中底
    let selected = selected && editing.is_none();
    let sel_bg = gpui::rgba((t.text << 8) | 0x1a);
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
        // Zed project_panel 行框几何（render_entry：border_1 恒挂、非聚焦
        // 行边框色 = 背景色隐形占位，hover 同步换边色）——边框临挂临摘会
        // 让整行内容抖 1px，改名框尤其明显；这里只换色不动布局
        .border_1()
        .border_color(gpui::transparent_black())
        .when(selected, |d| d.bg(sel_bg).hover(move |s| s.bg(sel_bg)))
        .when(!selected, |d| {
            d.hover(|s| s.bg(rgb(t.bg_hover)).border_color(rgb(t.bg_hover)))
        });

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

    if let Some(e) = &editing {
        // 编辑行 chrome（Zed show_focused_border + validation 同位）：行框
        // border_1 恒挂（见基样式），这里只把边框换成校验色，校验消息浮在
        // 该行下缘
        let (border, message) = edit_chrome(e, t);
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
        // 折叠/展开挂 on_click（mouse-up）而非 on_mouse_down：gpui 的 click
        // 与 drag 天然互斥（位移超阈值进 drag、未超阈值 mouse-up 才算
        // click）——按下即触发的旧写法会在拖拽起手时先把目录翻个面；点击
        // 即选中（082，F2 改名对象）
        el = el.on_click(move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.ws.tree_selected = Some(dir_path.clone());
                if !c.ws.expanded_dirs.remove(&dir_path) {
                    c.ws.expanded_dirs.insert(dir_path.clone());
                }
                c.rebuild_tree();
                cx.notify();
            });
        });
        el = el.child(tree_right_click(&weak_menu, &row));
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
        // on_click 同目录行：让位给拖拽起手（见上）；点击即选中（082）
        el = el.on_click(move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.ws.tree_selected = Some(fp.clone());
                c.open_file_tab(fp.clone(), cx);
            });
        });
        el = el.child(tree_right_click(&weak_menu, &row));
        // git badge on files（pi-web 徽标语义：M/A/D/R/U/C；023 默认关）
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
    tree_drag_wiring(el, &row, &weak_drag).into_any_element()
}

/// 树行的拖拽接线（Zed project_panel drag_and_drop 行侧三件套）：`on_drag`
/// 发起（payload = 条目路径，幽灵 = 图标 + 名字小签）、`drag_over` 落点
/// 高亮（只在「真实移动」上亮，原地/进自身子树不高亮——拖拽期 hover 底
/// 被 gpui 抑制，亮色即落点指示）、`on_drop` 提交移动。拖到文件行 = 移到
/// 其父目录（语义在 [`drag_move_target`]，drop 与高亮同源）。行内改名态
/// 不接线（tree_row 提前 return）。
pub(crate) fn tree_drag_wiring<E>(mut el: E, row: &TreeRow, weak: &gpui::WeakEntity<Chat>) -> E
where
    E: gpui::InteractiveElement + gpui::StatefulInteractiveElement,
{
    let t = theme::theme();
    let drag = TreeDrag {
        path: row.path.clone(),
        is_dir: row.is_dir,
    };
    let ghost_path = row.path.clone();
    let ghost_name = row.name.clone();
    let ghost_is_dir = row.is_dir;
    let hl_path = row.path.clone();
    let hl_is_dir = row.is_dir;
    let drop_path = row.path.clone();
    let drop_is_dir = row.is_dir;
    let weak_drop = weak.clone();
    el = el
        .on_drag(drag, move |_, _, _, cx| {
            cx.new(|_| TreeDragGhost {
                path: ghost_path.clone(),
                name: ghost_name.clone(),
                is_dir: ghost_is_dir,
            })
        })
        .drag_over::<TreeDrag>(move |style, drag, _, _| {
            if drag_move_target(&drag.path, drag.is_dir, &hl_path, hl_is_dir).is_some() {
                style.bg(rgb(t.bg_hover))
            } else {
                style
            }
        })
        .on_drop::<TreeDrag>(move |drag, _, cx| {
            let Some(dst) = drag_move_target(&drag.path, drag.is_dir, &drop_path, drop_is_dir)
            else {
                return;
            };
            let _ = weak_drop.update(cx, |c, cx| {
                c.tree_move_commit(&drag.path, drag.is_dir, &dst, cx);
            });
        });

    // 悬停自动展开（Zed hover_expand_task 500ms）：只挂折叠目录行。任何
    // 一次 mouse-move 会派发给**所有**在册行的 on_drag_move（capture 相、
    // 无悬停过滤），离开用本行的 inside=false 清目标——鼠标只要还在窗口
    // 里动就不会有陈旧目标；整体出窗后定时器仍触发的极端场景容忍一次
    // 误展开。目标去重靠 ws.tree_drag_expand：同目标不重复起定时器。
    if row.is_dir && !row.expanded {
        let exp_path = row.path.clone();
        let exp_is_dir = true;
        let weak_exp = weak.clone();
        el = el.on_drag_move::<TreeDrag>(move |ev, _, cx| {
            let inside = ev.bounds.contains(&ev.event.position);
            let drag = ev.drag(cx);
            let (src, src_is_dir) = (drag.path.clone(), drag.is_dir);
            let _ = weak_exp.update(cx, |c, cx| {
                let pending =
                    c.ws.tree_drag_expand
                        .as_ref()
                        .is_some_and(|p| same_path(p, &exp_path));
                if inside {
                    let valid = drag_move_target(&src, src_is_dir, &exp_path, exp_is_dir).is_some();
                    if valid && !pending {
                        c.ws.tree_drag_expand = Some(exp_path.clone());
                        let target = exp_path.clone();
                        cx.spawn(async move |weak, cx| {
                            cx.background_executor().timer(DRAG_EXPAND_DELAY).await;
                            let _ = weak.update(cx, |c, cx| {
                                // 目标未变（未被离开清除、未被别行覆盖）才展开
                                if c.ws
                                    .tree_drag_expand
                                    .as_ref()
                                    .is_some_and(|p| same_path(p, &target))
                                {
                                    c.ws.tree_drag_expand = None;
                                    c.ws.expanded_dirs.insert(target);
                                    c.rebuild_tree();
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    }
                } else if pending {
                    c.ws.tree_drag_expand = None;
                }
            });
        });
    }
    el
}

pub(crate) fn name_label(name: &str) -> gpui::AnyElement {
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
pub(crate) fn guide_lines(
    depth: usize,
    prev: Option<usize>,
    next: Option<usize>,
    color: u32,
) -> impl Iterator<Item = gpui::AnyElement> {
    (0..depth).map(move |level| {
        let top = if prev.is_none_or(|d| d <= level) {
            4.
        } else {
            0.
        };
        let bottom = if next.is_none_or(|d| d <= level) {
            4.
        } else {
            0.
        };
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
pub(crate) fn changed_dot() -> gpui::AnyElement {
    div()
        .size(px(6.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(0xd6a84b))
        .into_any_element()
}

/// 行右键：挂文件树右键菜单（目录/文件两套，锚点 = 事件坐标）。
/// 挂为行尾子元素占位（无视觉），事件在行容器上接。右键即选中该行
/// （Zed project_panel 同语义，082：右键后 F2 可直接改名）。
pub(crate) fn tree_right_click(weak: &gpui::WeakEntity<Chat>, row: &TreeRow) -> gpui::AnyElement {
    let weak = weak.clone();
    let path = row.path.clone();
    let is_dir = row.is_dir;
    div()
        .absolute()
        .size_full()
        .top_0()
        .left_0()
        .on_mouse_down(
            gpui::MouseButton::Right,
            move |ev: &gpui::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let path = path.clone();
                let _ = weak.update(cx, |c, cx| {
                    c.ws.tree_selected = Some(path.clone());
                    c.dlg.tree_menu = Some(if is_dir {
                        crate::function_panel::TreeMenu::Dir {
                            path,
                            x: f32::from(ev.position.x),
                            y: f32::from(ev.position.y),
                        }
                    } else {
                        crate::function_panel::TreeMenu::File {
                            path,
                            x: f32::from(ev.position.x),
                            y: f32::from(ev.position.y),
                        }
                    });
                    cx.notify();
                });
            },
        )
        .into_any_element()
}
