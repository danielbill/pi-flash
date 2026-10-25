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

use std::path::Path;
use std::sync::Arc;

use gpui::{SharedString, UniformListDecoration, div, prelude::*, px, rgb};

use crate::appearance::theme;
use crate::function_panel::file_tree_drag::{
    DRAG_EXPAND_DELAY, TreeDrag, TreeDragGhost, drag_move_target,
};
use crate::services::file_icons;
use crate::services::file_tree::TreeRow;
use crate::ui::{VListHeight, WithTip, icon_path, vlist};
use crate::*;
use crate::{Chat, i18n::tr, same_path};

use super::file_tree_edit::{edit_chrome, edit_input_slot, tree_edit_row, validation_note};
use super::file_tree_pins::pin_list_view;
use super::file_tree_sticky::StickyAncestors;

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

impl Chat {
    /// 行内改名前置（Zed project_panel 语义）：沿途展开祖先目录 + 把改名
    /// 行滚进视口。树右键菜单入口行本就可见（幂等）；编辑区右键菜单入口
    /// 可能该行还没展开/滚出视口，不做这些就没有输入框可编辑。
    pub(crate) fn reveal_tree_row(&mut self, path: &Path) {
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let mut anc = path.parent().map(|p| p.to_path_buf());
        let mut opened = false;
        while let Some(p) = anc {
            if same_path(&p, &cwd) {
                break;
            }
            opened |= self.ws.expanded_dirs.insert(p.clone());
            anc = p.parent().map(|q| q.to_path_buf());
        }
        if opened {
            self.rebuild_tree();
        }
        // 「只看变更」/Pin 收藏夹视图可能不含目标（变更列表无未变更文件、
        // 收藏视图无未收藏行）：回退树模式
        if (self.ws.changes_only || self.ws.pin_view)
            && !self.ws.tree_rows.iter().any(|r| same_path(&r.path, path))
        {
            self.ws.changes_only = false;
            self.ws.pin_view = false;
            self.rebuild_tree();
            self.persist_ui();
        }
        // 滚动对齐 Zed project_panel 的 autoscroll（reveal 链的真实落点）：
        // scroll_to_item_with_offset 非 strict——目标已可见不动视口，出界才
        // 最小滚动；offset = 深度（sticky 祖先链行数，视口自顶收缩，目标居中
        // 于钉住链之下）。deferred 到下一帧 paint 按真实视口高夹紧执行——
        // gpui 偏移向下滚为负，此前手写正 y 的 set_offset 被夹回 0，长树
        // 永不跳转。
        let Some(ix) = self
            .ws
            .tree_rows
            .iter()
            .position(|r| same_path(&r.path, path))
        else {
            return;
        };
        let depth = self.ws.tree_rows[ix].depth;
        let handle = crate::ui::vlist::scroll_handle(FILE_TREE_LIST_ID);
        handle.scroll_to_item_with_offset(ix, gpui::ScrollStrategy::Center, depth);
    }

    /// 新建虚拟编辑行在 vlist 输出位里的落点（下标, 缩进深度）：Zed new_entry
    /// 虚拟行插在目标目录行下方、子项之前；目标 = 项目根时插列表顶（根下新
    /// 建目录按排序也落最顶）。目录行不在表里（理论不可达：open_file_new 已
    /// 沿途展开）兜底排表尾。渲染落点（tree_list_view edit_slot）与打开编辑
    /// 行时的自动滚动共用本公式，两处永不漂移。
    pub(crate) fn tree_new_edit_slot(&self, dir: &Path) -> (usize, usize) {
        if self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(dir, cur))
        {
            return (0, 0);
        }
        self.ws
            .tree_rows
            .iter()
            .position(|r| r.is_dir && same_path(&r.path, dir))
            .map_or((self.ws.tree_rows.len(), 1), |ix| {
                (ix + 1, self.ws.tree_rows[ix].depth + 1)
            })
    }

    /// 文件标签激活时的树同步（PF 与 Zed 的分歧点：Zed 多 worktree 面板挂
    /// 多根，PF 单根树跟随激活标签）：文件在当前项目内只定位选中；属于已知
    /// 其它项目（psp.projects，含当前 cwd 的 0 会话组）则走 **switch_project
    /// 完整切项目**——树要能定位文件在项目里的位置（快捷面板/搜索/消息路径
    /// 开文件都经这里）。会话标签的去向由 switch_project 定夺（2026-12-06
    /// 定案）：新项目有上次会话 → 恢复；没有 → **进行中的原会话原地保留**，
    /// 只有当前本身是空白草稿才落新草稿。内容视图保持不动：用户正看着这个
    /// 文件，不能被 switch_to 的回 chat 前台拽走。两处都不沾（附件类游离
    /// 文件）则树不动。定位复用 reveal_tree_row。
    pub(crate) fn sync_tree_to_file(&mut self, path: &Path, cx: &mut gpui::Context<Self>) {
        let under_current = self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| services::workspace::is_under(path, cur));
        if !under_current {
            let root = self
                .psp
                .projects
                .iter()
                .filter(|g| services::workspace::is_under(path, &g.path))
                .max_by_key(|g| g.path.components().count())
                .map(|g| g.path.clone());
            let Some(root) = root else {
                return;
            };
            let view = self.layout.content_view;
            self.switch_project(root, cx);
            if self.layout.content_view != view {
                self.set_content_view(view);
            }
        }
        self.ws.tree_selected = Some(path.to_path_buf());
        self.reveal_tree_row(path);
    }

    // -----------------------------------------------------------------------
    // 标题项目下拉（树根跨项目快切）
    // -----------------------------------------------------------------------

    /// 标题左键 / 自动化 files.project_menu：开/收项目下拉。打开时键盘
    /// 高亮默认落当前项目（Enter 空转、↓ 即下一个）。
    pub(crate) fn toggle_tree_proj_menu(&mut self, cx: &mut gpui::Context<Self>) {
        if self.dlg.tree_proj_menu.take().is_some() {
            cx.notify();
            return;
        }
        let cur = self.ws.cwd.clone();
        let sel = self
            .psp
            .projects
            .iter()
            .position(|g| {
                cur.as_deref()
                    .is_some_and(|c| same_ws(&g.path.to_string_lossy(), &c.to_string_lossy()))
            })
            .unwrap_or(0);
        self.dlg.tree_proj_menu = Some(crate::function_panel::TreeProjMenu { sel });
        cx.notify();
    }

    /// ↑/↓ 移动键盘高亮（钳位不回绕，model picker 同款）。
    pub(crate) fn tree_proj_menu_move(&mut self, delta: i32, cx: &mut gpui::Context<Self>) {
        let Some(m) = self.dlg.tree_proj_menu.as_mut() else {
            return;
        };
        let n = self.psp.projects.len();
        if n == 0 {
            return;
        }
        m.sel = ((m.sel as i32) + delta).clamp(0, n as i32 - 1) as usize;
        cx.notify();
    }

    /// Enter：应用当前高亮行。
    pub(crate) fn tree_proj_menu_apply(&mut self, cx: &mut gpui::Context<Self>) {
        let sel = match self.dlg.tree_proj_menu.as_ref() {
            Some(m) => m.sel,
            None => return,
        };
        self.tree_proj_menu_pick(sel, cx);
    }

    /// 点击行 / Enter 共用：切树的项目并收下拉。切换 = **switch_project 完整
    /// 切项目**（单一语义 2026-10-18：当前项目 ≡ 活跃会话的项目，该项目恢复
    /// 上次会话或落新草稿，注意力跟过去；树根/branch/git/fs watcher/侧栏
    /// 当前项目随之重挂）。「只改 cwd 不动活跃会话」的旧语义已废除——不然
    /// composer 还连着旧项目的草稿，发消息会落错项目（2026-10-18 实测事故）。
    /// 内容视图保持不动：用户可能停在终端/文件上，不被拽走。
    pub(crate) fn tree_proj_menu_pick(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        self.dlg.tree_proj_menu = None;
        let Some(g) = self.psp.projects.get(ix) else {
            return;
        };
        let path = g.path.clone();
        let cur_same = self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_ws(&path.to_string_lossy(), &cur.to_string_lossy()));
        if !cur_same {
            let view = self.layout.content_view;
            self.switch_project(path, cx);
            if self.layout.content_view != view {
                self.set_content_view(view);
            }
            // 旧项目的选中条目不再成立（F2/高亮都按行存在性复核，残留无害，
            // 这里清掉是为了下拉切走再切回来不闪旧高亮）
            self.ws.tree_selected = None;
        }
        cx.notify();
    }
}

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
        col = col.child(changes_list_view(chat, rows, &weak, cx));
    } else if !files_min {
        col = col.child(tree_list_view(chat, rows, &weak, cx));
    }
    col.into_any_element()
}

/// 「只看变更」列表体：容器 + vlist（行头由 files_view 构建，这里只管
/// 滚动列表）。会话列表同款滚动条挂法（psp-scroll-wrap 镜像）：vlist
/// 自带的 menu_scrollbar 关掉，改 relative 包裹 + 右缘 absolute 条；变更
/// 容器无水平内边距，条 right(0) 已贴 dock 右缘。
fn changes_list_view(
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

/// 主树列表体：行内新建虚拟行的落点计算 + sticky 祖先链装饰 + vlist。
/// 会话列表同款滚动条挂法：vlist 自带的 menu_scrollbar 关掉，改 relative
/// 包裹 + 右缘 absolute 条；容器 px(6) → 条外扩 6 抵消，右缘齐 dock 右缘
/// （同会话列表贴边）。
fn tree_list_view(
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
                        tree_row(
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
    el.child(tree_right_click(&weak, &row)).into_any_element()
}

/// 021 操作栏（020-1 起 38px、无下 border，与 psp 控制栏同规）：左侧
/// 项目目录名（选中项目 = 主题色粗体；左键 = 项目下拉、右键 = 根目录
/// 菜单）+ 右侧 5 颗图标钮。Pin / 只看变更两颗是**视图切换钮**（021 三改
/// 定稿）：树视图下显示各自功能 icon，进入对应视图后 icon 换成目录树
/// （accent 高亮）——「点击回目录树」语义。搜索文件 / 资源管理器 / 终端 /
/// 最大化恒为功能 icon。按钮命中区 26px（§5.2 槽位），图标统一小档 MD16。
///
/// **自动隐藏**：六颗功能 icon 常态不可见（opacity 0），鼠标进入操作栏
/// （group `ft-bar-reveal`）才显影——只用绘制期 opacity，钮盒位置/命中区
/// 不动，所以显隐不抖列对齐；最大化钮同参与（`reveal = true`）。
/// 受设置-界面「部分操作隐藏」总开关（`autohide_icons`，默认 on）控制。
fn action_bar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let project: SharedString = chat
        .project()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| tr("statusbar.no_project").to_string())
        .into();
    let changes_only = chat.ws.changes_only;
    let proj_open = chat.dlg.tree_proj_menu.is_some();
    let pin_open = chat.ws.pin_view;
    let title_bounds = chat.layout.ft_title_bounds.clone();
    let w_proj = weak.clone();
    let w_toggle = weak.clone();
    let w_pin = weak.clone();
    let w_search = weak.clone();
    let w_term = weak.clone();
    let w_menu = weak.clone();
    let w_max = weak.clone();
    let cwd_menu = chat.ws.cwd.clone();
    let cwd_explorer = chat.ws.cwd.clone();
    let cwd_term = chat.ws.cwd.clone();
    // 021 二改定稿：操作栏即根行——项目名居左（020-1：主题色粗体）、
    // 操作钮右对齐（钮盒 26/间距 2，与会话控制栏右二列对齐，各带 tooltip）；
    // 名字区右键 = 根目录菜单（树体不再有根行，根下的新建/改名等入口保留下）
    div()
        .id("ft-action-bar")
        // 自动隐藏的显影组：鼠标在本操作栏内 → 五颗功能 icon 显影
        // （group_hover 的父命中区覆盖子钮，见 bar_btn）
        .group("ft-bar-reveal")
        .w_full()
        .h(px(crate::ui::tokens::panel_ctrl::BAR_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .pl(px(crate::ui::tokens::align::ICON_LINE))
        .pr(px(crate::ui::tokens::align::LIST_PX))
        .gap(px(2.))
        .child(
            div()
                .id("ft-project")
                .relative()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                // gpui div 默认 block（Style::default 显式 Display::Block），
                // 名字与 chevron 会竖着堆——标题行显式横排
                .flex()
                .items_center()
                .text_size(crate::appearance::ui_size(12.))
                // 020-1：当前项目名 = 主题色 + 粗体（与项目列表选中行一致）
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(t.accent))
                .cursor_pointer()
                .with_tip(tr("file_tree.switch_project"))
                // 项目下拉（树根跨项目快切）：左键开/收，浮层贴标题下缘
                // （锚点 = 下方 canvas 捕获的 paint 期 bounds）
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    let _ = w_proj.update(cx, |c, cx| c.toggle_tree_proj_menu(cx));
                })
                // 名字自适应内容宽（flex-grow 0：chevron 紧贴名字，不吃剩
                // 余宽）；长名走 flex-shrink + min_w_0 收缩 ellipsis
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(project),
                )
                // 开合指示：下拉开着时翻向上
                .child(div().flex_shrink_0().ml(px(4.)).child(crate::ui::icon(
                    if proj_open {
                        "chevron-up"
                    } else {
                        "chevron-down"
                    },
                    12.,
                    t.accent,
                )))
                // 标题 paint 期 bounds 捕获（dock_bounds 同款 canvas，无交互
                // 不挡命中）：项目下拉的锚点
                .child(
                    gpui::canvas(
                        move |bounds, _, _| *title_bounds.borrow_mut() = Some(bounds),
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let Some(cwd_menu) = cwd_menu.clone() else {
                            return;
                        };
                        let _ = w_menu.update(cx, |c, cx| {
                            c.dlg.tree_menu = Some(crate::function_panel::TreeMenu::Dir {
                                path: cwd_menu,
                                x: f32::from(ev.position.x),
                                y: f32::from(ev.position.y),
                            });
                            cx.notify();
                        });
                    },
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                // 020-1 列对齐：钮盒 26、间距 2，与会话控制栏右二同指标——
                // 两排右缘图标逐列对齐（最大化列/次列同 x）
                .gap(px(2.))
                .child(bar_btn(
                    "ft-pins",
                    // 视图切换钮（021 三改）：树视图 = pin icon（进入收藏
                    // 视图）；Pin 视图 = 目录树 icon（点击回树）
                    if pin_open { "folder-tree" } else { "pin" },
                    if pin_open {
                        tr("file_tree.back_to_tree")
                    } else {
                        tr("file_tree.pin_favorites")
                    },
                    t,
                    pin_open,
                    true,
                    None,
                    move |_, _, cx| {
                        let _ = w_pin.update(cx, |c, cx| c.toggle_pin_view(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-toggle-changes",
                    // 同上：树视图 = 变更 icon；变更视图 = 目录树 icon
                    if changes_only {
                        "folder-tree"
                    } else {
                        "git-commit-horizontal"
                    },
                    if changes_only {
                        tr("file_tree.back_to_tree")
                    } else {
                        tr("file_tree.show_changes_only")
                    },
                    t,
                    changes_only,
                    true,
                    None,
                    move |_, _, cx| {
                        let _ = w_toggle.update(cx, |c, cx| c.toggle_changes_only(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-search",
                    "search",
                    tr("file_tree.search"),
                    t,
                    false,
                    true,
                    // tooltip 附键位 chip（OpenFileSearch = ctrl-l，082 同款）
                    Some(&crate::OpenFileSearch),
                    move |_, _, cx| {
                        let _ = w_search.update(cx, |c, cx| c.open_file_search(cx));
                    },
                ))
                .child(bar_btn(
                    "ft-explorer",
                    "folder-input",
                    tr("file_tree.reveal_in_explorer"),
                    t,
                    false,
                    true,
                    None,
                    move |_, _, _cx| {
                        // 无项目态：没有可展示的目录，按钮 no-op
                        if let Some(cwd) = &cwd_explorer {
                            crate::services::shell_open::open_in_explorer(cwd);
                        }
                    },
                ))
                .child(bar_btn(
                    "ft-term",
                    "icon-terminal-solid",
                    tr("file_tree.open_in_terminal"),
                    t,
                    false,
                    true,
                    None,
                    move |_, window, cx| {
                        // 无项目态：没有可打开终端的目录，按钮 no-op
                        let Some(cwd) = cwd_term.clone() else { return };
                        let _ = w_term.update(cx, |c, cx| c.open_terminal_in(cwd, window, cx));
                    },
                ))
                .child(bar_btn(
                    "ft-panel-max",
                    "maximize",
                    tr("file_tree.maximize"),
                    t,
                    false,
                    // 最大化与五颗低频钮同入显影组（自动隐藏，见 action_bar 头注）
                    true,
                    Some(&crate::MaximizeFileTree),
                    move |_, _, cx| {
                        let _ = w_max.update(cx, |c, cx| {
                            // 020-1 单最大化钮：本面板填满，会话列表收起到控制栏
                            c.layout.sess_min = true;
                            c.layout.files_min = false;
                            // 最大化态立即落盘（用户报障：最大化后关闭重启丢失）
                            c.persist_ui();
                            cx.notify();
                        });
                    },
                )),
        )
        .into_any_element()
}

/// 操作栏图标钮：26px 命中槽 + 16px 图标（icon_hover 悬停变亮；激活态
/// 用 accent 实色不参与 hover 变亮——开关点亮语义）。`tip` = hover
/// tooltip（ui::tooltip 统一入口 §5.10，显示期过 tr 随语言切换）；
/// `action` 非空时 tooltip 附键位 chip（键位反查 keymap，082 最大化钮）。
/// `reveal` = 自动隐藏（常态 opacity 0，操作栏 hover 显影；见 action_bar 头注）。
fn bar_btn(
    id: &'static str,
    icon_name: &'static str,
    tip: &'static str,
    t: &'static crate::appearance::theme::Theme,
    active: bool,
    reveal: bool,
    action: Option<&'static dyn gpui::Action>,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let btn = div()
        .id(id)
        .w(px(26.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)));
    // 自动隐藏：opacity 只管绘制，命中区/布局原样——显隐之间按钮不挪位，
    // 悬停灭掉一个 icon 不会让同排钮重排。tooltip 照旧（图标透明也能悬到）。
    // 设置-界面「部分操作隐藏」关掉后恒显（开关默认 on）
    let btn = if reveal && crate::services::workspace::autohide_icons() {
        btn.opacity(0.)
            .group_hover("ft-bar-reveal", |s| s.opacity(1.))
    } else {
        btn
    };
    let btn = match action {
        Some(a) => btn.with_tip_action(tip, a),
        None => btn.with_tip(tip),
    };
    btn.on_mouse_down(gpui::MouseButton::Left, on_click)
        .child(if active {
            crate::ui::icon(icon_name, 16., t.accent).into_any_element()
        } else {
            crate::ui::icon_hover(icon_name, 16., t.text_dim).into_any_element()
        })
        .into_any_element()
}

/// One flattened row: indent guides + chevron + type icon + name + git badge.
/// `selected` = 082 选中条目（常驻薄纱底，F2 改名对象；编辑态行不叠）。
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
            MouseButton::Right,
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

// ---------------------------------------------------------------------------
// 文件树与 fs 监听方法（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

impl Chat {
    /// 归档当前项目的文件树状态（展开集合 + Pin 视图）→ 内存表 + 落盘
    /// （`__tree`）。调用点：切项目（remount 开头）、保存 UI 状态（关窗/退出）、
    /// Pin 视图开关。展开/折叠本身不即时落盘（高频动作，靠这些时机兜住）。
    pub(crate) fn archive_tree_state(&mut self) {
        let Some((key, cwd)) = self.ws.tree_owner.clone() else {
            return;
        };
        let mem = crate::TreeMemory {
            expanded: self.ws.expanded_dirs.clone(),
            pin: self.ws.pin_view,
        };
        crate::services::workspace::save_tree_state(
            &cwd.to_string_lossy(),
            &crate::services::workspace::TreeState {
                expanded: crate::services::workspace::tree_rel_paths(&cwd, &mem.expanded),
                pin: mem.pin,
            },
        );
        self.ws.tree_memory.insert(key, mem);
    }

    /// 恢复一个项目的文件树状态：内存表（本次会话切回过）→ 磁盘 `__tree`
    /// → 默认「只展开根」（对齐 Zed：打开项目默认展开 cwd）。失效条目在
    /// `tree_abs_paths` 里 prune（目录已删 / 项目外），恒含根。
    pub(crate) fn restore_tree_state(&mut self, cwd: &Path) {
        use crate::services::workspace as ws_store;
        let key = ws_store::same_ws_key(&cwd.to_string_lossy());
        let (expanded, pin) = match self.ws.tree_memory.get(&key) {
            Some(mem) => (mem.expanded.clone(), mem.pin),
            None => match ws_store::tree_state_for(&cwd.to_string_lossy()) {
                Some(st) => (ws_store::tree_abs_paths(cwd, &st.expanded), st.pin),
                // 无记录 = 默认态：只展开根（flatten 从这里 walk 出第一层）
                None => (std::collections::HashSet::from([cwd.to_path_buf()]), false),
            },
        };
        self.ws.expanded_dirs = expanded;
        self.ws.expanded_dirs.insert(cwd.to_path_buf());
        self.ws.tree_owner = Some((key, cwd.to_path_buf()));
        // Pin 视图按项目恢复；收藏清单为空时无意义（行为与 021 一致：退回树）
        self.ws.pin_view = pin && !self.ws.pinned.is_empty();
        if self.ws.pin_view {
            self.ws.changes_only = false;
        }
    }

    /// 文件树展平缓存重建（services::file_tree::flatten：每展开目录读一次
    /// 盘 + gitignore 过滤 + Zed 语义排序 + git 徽标回填）。展开/折叠、
    /// git 刷新、fs 事件、切项目后都走这里。三种视图各吃各的展平：
    /// pin_view → flatten_pins（收藏森林）/ changes_only → flatten_changes
    /// （git status 直出）/ 树 → flatten。
    pub(crate) fn rebuild_tree(&mut self) {
        // 无项目态：树恒空（占位 runtime 不挂任何工作区）
        let Some(cwd) = self.ws.cwd.clone() else {
            self.ws.tree_rows = std::sync::Arc::new(Vec::new());
            return;
        };
        self.ws.tree_rows = std::sync::Arc::new(if self.ws.pin_view {
            services::file_tree::flatten_pins(
                &cwd,
                &self.ws.pinned,
                &self.ws.expanded_dirs,
                &self.ws.git_files,
            )
        } else if self.ws.changes_only {
            // 「只看变更」= 扁平表直接吃 git status 结果（不扫盘）
            services::file_tree::flatten_changes(&cwd, &self.ws.git_files)
        } else {
            services::file_tree::flatten(&cwd, &self.ws.expanded_dirs, &self.ws.git_files)
        });
    }

    /// 挂上 fs watcher（递归 watch cwd；旧句柄 drop 即解除）。切项目时
    /// 重挂。事件由 startup::spawn_fs_watch_pump 合批后驱动 refresh_git。
    pub(crate) fn attach_fs_watch(&mut self) {
        // 无项目态：无工作区监听
        let Some(cwd) = self.ws.cwd.clone() else {
            self.ws.fs_watch = None;
            return;
        };
        self.ws.fs_watch = services::watcher::watch(&cwd, self.ws.fs_watch_tx.clone()).ok();
    }

    /// 023 外部改动检测的监听面维护（对齐 Zed 的 worktree 监听面）：
    /// - cwd 内文件：cwd 的**递归** watch 已覆盖；
    /// - cwd 外文件（Obsidian 库 / pick_open_files / 远程 FILE_OPEN）：此前
    ///   完全没监听——磁盘怎么改都没有信号，编辑器不跟随。现按**所在目录**
    ///   挂非递归 watch（Zed single-file worktree 同款），事件与 cwd 事件
    ///   走同一条通道合批，`check_external_file_changes` 按 disk_sig 复查。
    ///
    /// 打开/关闭文件 tab、切项目后调用；不再被任何打开文件需要的目录卸掉
    /// （句柄 drop = 停止监听），避免长期持有无关目录的监听。
    pub(crate) fn retain_file_watches(&mut self) {
        let mut wanted: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for t in &self.ws.panel_tabs {
            if let PanelTab::File(p) = t
                && let Some(dir) = p.parent()
                && !self
                    .ws
                    .cwd
                    .as_deref()
                    .is_some_and(|cur| services::workspace::is_under(dir, cur))
            {
                wanted.insert(dir.to_path_buf());
            }
        }
        self.ws.file_watches.retain(|dir, _| wanted.contains(dir));
        for dir in wanted {
            if self.ws.file_watches.contains_key(&dir) {
                continue;
            }
            if let Ok(w) = services::watcher::watch_dir(&dir, self.ws.fs_watch_tx.clone()) {
                self.ws.file_watches.insert(dir, w);
            }
        }
    }

    // -----------------------------------------------------------------------
    // built-in terminal (pi-web TerminalPanel parity)
    // -----------------------------------------------------------------------

    pub(crate) fn load_project_files(&mut self) {
        // 无项目态：项目文件索引恒空
        let Some(cwd) = self.ws.cwd.clone() else {
            self.ws.project_files = Vec::new();
            return;
        };
        self.ws.project_files = walk_files(&cwd, 3, 400);
    }
}
