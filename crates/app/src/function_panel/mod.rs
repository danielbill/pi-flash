//! psp / dock (v54): 项目+会话一体列表（ChatGPT 式，设计核心=隐藏复杂度）。
//! dock = statusbar 三面板容器（psp / 文件树 / git），nav 底色，右缘 6px
//! 拖拽调宽（250–500，双击复位 282）。psp 的悬浮层（路径 tooltip / 会话
//! 详情卡 / 两级菜单 / 删除确认）由 `psp_overlays` 在根渲染挂载。
//!
//! itlq.12 超规拆分后本文件只剩模块声明、dock 容器、files_view 委派与
//! overlay 状态类型：psp 会话子面板视图 = `psp_view`。

pub(crate) mod file_actions;
pub(crate) mod file_actions_delete;
pub(crate) mod file_actions_edit;
pub(crate) mod file_actions_fs;
pub(crate) mod file_tree;
pub(crate) mod file_tree_drag;
pub(crate) mod file_tree_edit;
pub(crate) mod file_tree_pins;
pub(crate) mod file_tree_sticky;
pub(crate) mod git_panel;
pub(crate) mod psp_actions;
pub(crate) mod psp_card_btns;
pub(crate) mod psp_del_confirm;
pub(crate) mod psp_drag;
pub(crate) mod psp_hover_card;
pub(crate) mod psp_menus;
pub(crate) mod psp_overlays;
pub(crate) mod psp_proj_tip;
pub(crate) mod psp_project_card;
pub(crate) mod psp_row_slot;
pub(crate) mod psp_row_views;
pub(crate) mod psp_term_menu;
pub(crate) mod psp_tree_proj_menu;
pub(crate) mod psp_view;
pub(crate) mod tab_menu;
pub(crate) mod tree_menu;
pub(crate) mod tree_menu_ext;

use gpui::{Context, Entity, MouseButton, SharedString, div, prelude::*, px, rgb};

use std::path::PathBuf;

use crate::Chat;
use crate::DockPanel;
use crate::ListMode;
use crate::SortMode;
use crate::appearance::theme::{Theme, theme as T};
use crate::services::workspace::{same_path, same_ws, same_ws_key};
use crate::ui::TextInput;
use crate::ui::tokens::align;
use crate::ui::{WithTip, icon};

mod project_row;
mod psp_rows;

// ---------------------------------------------------------------------------
// dock container (020-1 v54): nav 底色 + 上下子面板（会话/文件树）+ 分隔线
// ---------------------------------------------------------------------------

pub(crate) fn dock(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    // git 面板保留独立视图（022；状态栏 tab 已隐藏，automation panel.dock 可达）
    if chat.layout.dock_panel == DockPanel::Git {
        return div()
            .id("dock")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.nav))
            .child(git_panel::view(chat, weak, cx))
            .into_any_element();
    }
    // 插件面板（beads 360 反馈②）：左列格子视图，互斥于整合面板/git。
    // **停用/条目不在（已被摘掉）时直接漏到下面的默认 psp 列表**——
    // 左列是「停用后仍旧挂着旧面板」的最后一道口，不能只倚靠状态侧去撤 dock。
    if let DockPanel::Plugin(ix) = chat.layout.dock_panel
        && chat.plugins.plugins.get(ix).is_some_and(|e| !e.disabled)
    {
        return div()
            .id("dock")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.nav))
            .child(crate::pf_plugin::plugin_panel_view(chat, ix, weak))
            .into_any_element();
    }
    // 020-1 整合面板：会话列表(上) + 文件树(下)，默认 50/50；分隔线拖拽
    // 改占比（拖拽即退出两侧最小态、双击复位 50%），各自可收起到控制栏
    //（单最大化钮，语义见 LayoutUi::sess_min / files_min）
    let bounds_slot = chat.layout.dock_bounds.clone();
    // §5.12 统一分隔线：常驻 1px；**hover 与拖拽同为 4px accent**
    let split_drag = chat.layout.split_drag;
    let split_active = split_drag || chat.layout.split_hover;
    let d_hit = crate::ui::tokens::divider::HIT;
    div()
        .id("dock")
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .bg(rgb(t.nav))
        // dock 容器 bounds 每帧捕获（PillBtns::tracker 同款 canvas）：
        // app-body 层的拖拽跟踪要把窗口 y 换算成占比
        .child(
            gpui::canvas(
                move |bounds, _, _| *bounds_slot.borrow_mut() = Some(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .child(psp_view::psp_view(chat, entity, weak.clone(), window, cx))
        // 子面板分隔线：7px 命中带 + 常驻 1px 视觉线（左右缩进 10 = 列表
        // 右线），hover/拖拽按 §5.12 统一规范换 4px accent 覆盖线（absolute
        // 改 top/h 不占布局零推挤，带内居中 = 与常驻线同一中线）
        .child(
            div()
                .id("sp-divider")
                .with_tip("psp.split_tip")
                .h(px(d_hit))
                .flex_shrink_0()
                .relative()
                .cursor_row_resize()
                // hover 状态存进字段（与 content 侧 slp_hover 同一机制），
                // 不用 group_hover——后者受 has_active_drag 门控，粗细拿不到
                .on_hover({
                    let weak_hv = weak.clone();
                    move |h: &bool, _, cx| {
                        let _ = weak_hv.update(cx, |c, cx| {
                            if c.layout.split_hover != *h {
                                c.layout.split_hover = *h;
                                cx.notify();
                            }
                        });
                    }
                })
                .on_mouse_down(MouseButton::Left, {
                    let weak_dz = weak.clone();
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        let _ = weak_dz.update(cx, |c, cx| {
                            if ev.click_count == 2 {
                                c.layout.dock_split = 0.5;
                                c.layout.sess_min = false;
                                c.layout.files_min = false;
                                c.persist_ui();
                            } else {
                                c.layout.split_drag = true;
                            }
                            cx.notify();
                        });
                    }
                })
                // 线形交给统一实现（ui::divider）：1px 常驻 / 4px accent
                // 高亮，带内垂直居中 = 两种粗细同一中线
                .child(crate::ui::divider::line(
                    crate::ui::divider::LineSpec::horizontal(10., d_hit),
                    split_active,
                    t,
                )),
        )
        .child(files_view(chat, weak, cx))
        .into_any_element()
}

/// files view：树渲染在 `file_tree.rs`（vlist 虚拟化 + 展平缓存，render
/// 不碰磁盘；Zed 图标主题 + gitignore 过滤在 services::file_tree/。
/// file_icons）。
pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    file_tree::files_view(chat, weak, _cx)
}

// ---- psp/文件树 overlay 状态类型（自 shell/types.rs 下沉，本域自持）----

/// 会话右键详情卡（2026-10-15 定夺：悬停自动弹窗整体撤销，改右键召唤的
/// 上下文卡——定位右键点、上/下方弹出、点外即收；名称沿用 hover 时代）。
#[derive(Debug, Clone)]
pub(crate) struct HoverCard {
    pub path: PathBuf,
    /// 右键点视口坐标：卡左缘对齐 x、上/下方 12px 弹出（PopupEdges 四象限）
    pub x: f32,
    pub y: f32,
    /// 点「删除」后卡内换 取消/确认 两键
    pub confirming: bool,
    /// 标题点击后原地改名（卡内变输入框，Enter 提交 / Esc 取消）
    pub renaming: bool,
    pub rename_input: Option<gpui::Entity<crate::ui::TextInput>>,
}

/// 项目行右键详情卡（020：样式对齐会话详情卡）：标题加粗 + 路径/会话数
/// 信息行 + 分隔线下 标签栏项目行同款双图标动作栏（删除带卡内确认态）。
#[derive(Debug, Clone)]
pub(crate) struct ProjCard {
    pub path: PathBuf,
    /// 右键点视口坐标：卡左缘对齐 x、上/下方 12px 弹出（PopupEdges 四象限）
    pub x: f32,
    pub y: f32,
    /// 点「删除」后动作栏换 取消/确认 两键
    pub confirming: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum TreeMenu {
    Dir { path: PathBuf, x: f32, y: f32 },
    File { path: PathBuf, x: f32, y: f32 },
}

/// 文件树标题项目下拉（树根跨项目快切）：一览 psp.projects（会话列表里
/// 的全部项目），↑/↓ 高亮 + Enter 切换、点击直切。`sel` = 键盘高亮行
/// （psp.projects 下标，打开时默认落当前项目）。切换 = **switch_project
/// 完整切项目**：树/侧栏全套随新 cwd 重挂；会话标签去留见 switch_project
/// 的 2026-12-06 保留规则（有上次会话则恢复；没有则进行中的原会话原地
/// 保留，不落新草稿）。内容视图保持不动。
#[derive(Debug, Clone)]
pub(crate) struct TreeProjMenu {
    pub(crate) sel: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TreeEdit {
    pub(crate) kind: TreeEditKind,
    pub(crate) is_dir: bool,
    pub(crate) input: gpui::Entity<TextInput>,
    /// 实时校验态（键入即查，Zed populate_validation_error 移植）
    pub(crate) validation: TreeValidation,
}

#[derive(Debug, Clone)]
pub(crate) enum TreeEditKind {
    /// 在该目录下新建（Zed leaf_entry_id: None——is_new_entry 判据）
    New { dir: PathBuf },
    /// 同目录改名该条目
    Rename { path: PathBuf },
}

/// Zed ValidationState 移植：错误（重名/非法）拦 Enter 提交，警告放行。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum TreeValidation {
    #[default]
    None,
    Error(String),
    Warning(String),
}
