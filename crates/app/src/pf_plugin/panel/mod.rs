//! 插件面板渲染（契约 v2.2 · 090-9）。
//!
//! **PF 是树解释器**：插件 `render(ctx)` 返回**已算好**的组件树（值内联、动作显式），
//! 本模块只按结构画——不认字段名、不做 filter、不折算动作、**不按组件组合猜布局**。
//!
//! 外壳归宿主：`panel-frame` / `section` 的分区标题样式、各自滚动、可拖拽分隔线、
//! 比例状态全部由本模块实现（与 project panel 同款），插件只说“分几区、每区放什么”。
//!
//! 结构分两层：
//! - [`resolve`]：组件树 JSON → [`ResolvedNode`]（纯逻辑，可单测，不碰 gpui）
//! - gpui 渲染层：ResolvedNode → 元素

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};
use serde_json::Value;

use pf_plugin_host::js::JsAction;

use crate::Chat;
use crate::T;
use crate::appearance::theme::Theme;
use crate::appearance::ui_size;
use crate::ui::tooltip::WithTip;

/// 01 规范 §7 SP 间距 token → px（ui::tokens::space 同值表）。
fn sp_token(name: &str) -> Option<f32> {
    Some(match name {
        "SP1" => 2.,
        "SP2" => 4.,
        "SP3" => 6.,
        "SP4" => 8.,
        "SP5" => 10.,
        "SP6" => 12.,
        "SP7" => 16.,
        "SP8" => 20.,
        "SP9" => 24.,
        "SP10" => 32.,
        _ => return None,
    })
}

// padX / padY 取值由下方 sp(node, key, default) 统一承担（2026-10-29 清
// dead_code：旧 node_pad 与其重复，当前树无调用方）。

fn sp(node: &Value, key: &str, default: f32) -> f32 {
    // 数字 = 直接像素（弹窗要精确对齐老 UI 的 16/12 这类值）；字符串 = SP token
    match node.get(key) {
        Some(Value::Number(n)) => n.as_f64().map(|v| v as f32).unwrap_or(default),
        Some(Value::String(t)) => sp_token(t).unwrap_or(default),
        _ => default,
    }
}

/// 面板内边距（**唯一来源**）：外壳四周留白 + 列数公式共用同一份，
/// 子元素不再各加一层（曾 8px 叠 8px，视觉上与工具条贴死）
/// 外壳留白：与 topbar 工具条图标边距对齐（用户定夺 12）
const PANEL_PAD: f32 = 12.;
/// 分区标题缺省右缘间距（插件没给 `titlePad` 时用；给了就按 token 算）
const SECTION_TITLE_PR_DEFAULT: f32 = 0.;
/// 格子间距
const GRID_GAP: f32 = 4.;
/// 分区标题与分区顶边的距离（用户定夺 5 → 8：贴着工具条太挤）
const SECTION_HEAD_PT: f32 = 8.;
/// 列数上限（再多就不像 dock 了）
const GRID_MAX_COLS: u16 = 6;
/// 分区最小占比（拖到极限也不让某个分区消失）
const SPLIT_MIN_RATIO: f32 = 0.12;

/// `grid.minItem` 档位 → 最小格宽（实现值，插件不可见：01 §2.4）
fn min_item_width(min_item: &str) -> f32 {
    match min_item {
        "LG" => 160.,
        "MD" => 120.,
        _ => 88., // SM 默认
    }
}

/// 列数全由宿主算：`clamp(floor(可用宽 / 档位宽), 1, 6)`（01 §2.4）
fn grid_cols(panel_w: f32, min_item: &str) -> u16 {
    let usable = (panel_w - 2. * PANEL_PAD).max(1.);
    ((usable / min_item_width(min_item)).floor() as u16).clamp(1, GRID_MAX_COLS)
}

/// 格子尺寸/图标尺寸：`button.size` 档位（SM/MD/LG），格子与文字按钮共用
fn button_dims(size: &str) -> (f32, f32) {
    match size {
        "SM" => (64., 30.),
        "LG" => (92., 44.),
        _ => (76., 36.), // MD 默认
    }
}

// ---------------------------------------------------------------------------
// 解析层：组件树 JSON → ResolvedNode（纯逻辑，可单测）
// ---------------------------------------------------------------------------

/// 动作（01 §8 白名单）。由插件显式给出，宿主不再折算。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellAction {
    Run(String),
    Edit(String),
    Reveal(String),
    Toast(String),
    OpenUrl(String),
    OpenFolder(String),
    NewSession,
    /// 预填输入框（不代发）
    AskPi(String),
    /// 白名单内但宿主未接 → 渲染禁用 + 日志（01 §8）
    Unsupported(String),
}

/// 一个格子的右键菜单（待渲染的弹出层；窗口坐标由鼠标事件给出）。
#[derive(Debug, Clone)]
pub(crate) struct CellMenuState {
    pub x: f32,
    pub y: f32,
    /// 该格子对应的路径（回调插件时作为 `{ path }` 递回）
    pub path: String,
    pub items: Vec<MenuItem>,
}

/// 右键菜单项（01 §4.2）：带 action = 宿主执行；不带 = 回调插件 onCommand。
#[derive(Debug, Clone)]
pub(crate) struct MenuItem {
    pub id: String,
    pub title: String,
    pub action: Option<CellAction>,
    /// 图标：宿主内置名或 `assets:`（与 button.icon 同规则）
    pub icon: String,
    /// 危险项（红字 + 红 hover，对齐文件树「删除」）
    pub danger: bool,
}

/// 分区拖动中的瞬时状态（宿主 UI 状态，内存态，不落插件数据）。
#[derive(Debug, Clone)]
pub(crate) struct SplitDrag {
    /// 第 index 与 index+1 个分区之间的分隔线
    pub index: usize,
    pub start_y: f32,
    pub base: Vec<f32>,
}

/// 渲染上下文：外壳布局状态全部来自宿主。
pub(crate) struct RenderCtx<'a> {
    pub plugin_id: &'a str,
    pub panel_w: f32,
    /// 分区比例（宿主 UI 状态；插件不可见）
    pub ratios: Vec<f32>,
    /// 分区滚动句柄（每个 section 一个，跨帧存续）
    pub scrolls: Vec<gpui::ScrollHandle>,
    /// 悬停中的分隔线序号（§5.12 统一分隔线的高亮源）
    pub split_hover: Option<usize>,
    /// 拖拽中的分隔线序号（同上：hover 与拖拽同款 4px accent）
    pub split_drag: Option<usize>,
}

// ---------------------------------------------------------------------------
// gpui 渲染层
// ---------------------------------------------------------------------------

/// 插件预览页签内容（契约 v2.4 dock="tab"，`ContentView::Plugin` 分支）：
/// 归属 = 活跃标签携带的 pkg（重装/重载后 ix 漂移安全，经 by_pkg 现查），
/// 主体复用 dock 的 `plugin_panel_view`（同一解释器，同一渲染契约）。
pub(crate) fn plugin_tab_page(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let active = chat
        .ws
        .active_panel_tab
        .and_then(|ix| chat.ws.panel_tabs.get(ix));
    let Some(crate::PanelTab::Plugin { pkg, .. }) = active else {
        // 归属失配（幻影页防护，002 硬规则同源）：无主不渲染
        return placeholder_center("plugin", t);
    };
    let Some(ix) = chat.plugin_ix_by_pkg(pkg) else {
        // 插件已被卸载/停用：占位而非崩溃（页签残留由卸载链清理）
        return placeholder_center("plugin", t);
    };
    plugin_panel_view(chat, ix, weak)
}

/// 插件面板主体（dock 的 `DockPanel::Plugin` 分支）。
pub(crate) fn plugin_panel_view(
    chat: &Chat,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let Some(entry) = chat.plugins.plugins.get(ix) else {
        return placeholder_center("plugin", t);
    };
    // 停用兜底：面板只认 tree_loaded + 缓存树，不看 disabled 位（停用时
    // `plugin_evict` 已清缓存 + 退 dock，这里再招一道，防止其他路径漏撤）
    if entry.disabled {
        return div()
            .id("pf-panel")
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .child(placeholder_center("plugin", t))
            .into_any_element();
    }
    if !entry.tree_loaded {
        return div()
            .id("pf-panel")
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .child(placeholder_center("…", t))
            .into_any_element();
    }
    let Some(tree) = entry.body.as_ref().and_then(resolve) else {
        return div()
            .id("pf-panel")
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .child(placeholder_center("…", t))
            .into_any_element();
    };

    let id = entry.manifest.id.clone();
    let n = section_count(&tree);

    // 分区比例：宿主 UI 状态，分区数变了就重置等分（重启也回等分）
    {
        let mut ratios = entry.split_ratios.borrow_mut();
        if ratios.len() != n {
            *ratios = vec![1.0 / n as f32; n];
        }
    }
    let ratios = entry.split_ratios.borrow().clone();
    let scrolls = {
        let mut s = entry.section_scrolls.borrow_mut();
        while s.len() < n {
            s.push(gpui::ScrollHandle::new());
        }
        s[..n].to_vec()
    };
    let ctx = RenderCtx {
        plugin_id: &id,
        panel_w: chat.layout.slp_w,
        ratios,
        scrolls,
        split_hover: *entry.split_hover.borrow(),
        split_drag: entry.split_drag.borrow().as_ref().map(|d| d.index),
    };

    // 外层是"平整卡片"：与 project panel 同款留白，滚动在分区内部
    let dragging = entry.split_drag.borrow().clone();
    let mut root = div()
        .id("pf-panel")
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(render_root(&tree, ix, weak, t, &ctx));

    // 拖拽中：全幅透明覆层接管鼠标移动（分隔线只有几像素，靠它才不会丢事件）
    if let Some(drag) = dragging {
        let weak_move = weak.clone();
        let weak_up = weak.clone();
        root = root.child(
            div()
                .id("pf-split-drag")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .on_mouse_move(move |ev, window, cx| {
                    let dy = f32::from(ev.position.y) - drag.start_y;
                    let h = f32::from(window.viewport_size().height).max(1.);
                    let i = drag.index;
                    let mut next = drag.base.clone();
                    let d = dy / h;
                    if i + 1 < next.len() {
                        let total = next[i] + next[i + 1];
                        let a = (next[i] + d).clamp(SPLIT_MIN_RATIO, total - SPLIT_MIN_RATIO);
                        next[i] = a;
                        next[i + 1] = total - a;
                    }
                    let _ = weak_move.update(cx, |c, cx| {
                        if let Some(e) = c.plugins.plugins.get_mut(ix) {
                            *e.split_ratios.borrow_mut() = next;
                            cx.notify();
                        }
                    });
                })
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_up.update(cx, |c, cx| {
                        if let Some(e) = c.plugins.plugins.get_mut(ix) {
                            *e.split_drag.borrow_mut() = None;
                            cx.notify();
                        }
                    });
                }),
        );
    }

    root.into_any_element()
}

/// 子节点渲染：**纯结构**，宿主不按组件组合猜布局（契约 layout_ownership）。
mod popup;
mod render;
mod resolve;

pub(crate) use popup::{placeholder_center, plugin_popup_layer};
pub(crate) use render::{cell_menu_layer, render_root};
pub(crate) use resolve::{CanvasPaint, ResolvedNode, icon_asset, resolve, section_count};
