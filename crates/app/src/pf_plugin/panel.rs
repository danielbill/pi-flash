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

fn sp(node: &Value, key: &str, default: f32) -> f32 {
    node.get(key)
        .and_then(Value::as_str)
        .and_then(sp_token)
        .unwrap_or(default)
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
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedNode {
    /// 面板外壳（宿主画：分区标题/各自滚动/可拖拽分隔线/内边距）
    Frame {
        sections: Vec<ResolvedNode>,
    },
    /// 一个分区（只能出现在 Frame 内）
    Section {
        title: Option<String>,
        /// 分区标题图标（内置名或 `assets:`；与 button.icon 同规则）
        icon: Option<String>,
        /// 标题对齐：`start` / `end`（插件给意图，宿主负责落像素）
        title_align: String,
        /// 标题与内容右/左缘的距离（插件给 SP token，此处已换算成 px）
        title_pad: f32,
        children: Vec<ResolvedNode>,
    },
    Column {
        pad: f32,
        gap: f32,
        align_center: bool,
        children: Vec<ResolvedNode>,
    },
    Row {
        gap: f32,
        pad: f32,
        align: String,
        justify_between: bool,
        children: Vec<ResolvedNode>,
    },
    Group {
        title: String,
        children: Vec<ResolvedNode>,
    },
    /// 等宽格子流（通用原语）：列数/格宽归宿主，插件只给档位
    Grid {
        min_item: String,
        children: Vec<ResolvedNode>,
    },
    Divider,
    Text {
        text: String,
        tone: String,
        size: gpui::Pixels,
        weight: String,
        ellipsis: bool,
    },
    Icon {
        name: String,
        size: f32,
        tone: String,
    },
    Button {
        label: String,
        icon: String,
        tip: String,
        /// 该格子对应的路径（菜单回调时作为 `{ path }` 递回插件）
        arg: String,
        action: Option<CellAction>,
        menu: Vec<MenuItem>,
        variant: String,
        size: String,
        disabled: bool,
    },
    /// 未知组件：占位 + 日志（01 §1）
    Unknown(String),
}

/// 文案取值：`{"zh","en"}` 走宿主语言；纯字符串原样返回（数据文案）。
fn i18n(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => {
            let zh = o.get("zh").and_then(Value::as_str).unwrap_or("");
            let en = o.get("en").and_then(Value::as_str).map(str::to_string);
            crate::pf_plugin::pick_lang(zh, &en)
        }
        _ => String::new(),
    }
}

fn tone(node: &Value) -> String {
    node.get("tone")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .to_string()
}

/// 动作名 + arg → CellAction（01 §8 白名单；`$auto` 已删）。
fn parse_action(node: &Value) -> Option<CellAction> {
    let name = node.get("action").and_then(Value::as_str)?;
    let arg = node
        .get("arg")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(match name {
        "run" => CellAction::Run(arg),
        "edit" => CellAction::Edit(arg),
        "reveal_in_explorer" => CellAction::Reveal(arg),
        "toast" => CellAction::Toast(arg),
        "open_url" => CellAction::OpenUrl(arg),
        "open_folder" => CellAction::OpenFolder(arg),
        "new_session" => CellAction::NewSession,
        "ask_pi" => CellAction::AskPi(arg),
        other => CellAction::Unsupported(other.to_string()),
    })
}

fn parse_menu(node: &Value) -> Vec<MenuItem> {
    let Some(items) = node.get("menu").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?.to_string();
            Some(MenuItem {
                id,
                title: i18n(m.get("title")),
                action: parse_action(m),
                icon: m
                    .get("icon")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                danger: m.get("danger").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// 组件树解析入口。未知组件 → [`ResolvedNode::Unknown`]（渲染占位并记日志）。
pub(crate) fn resolve(node: &Value) -> Option<ResolvedNode> {
    let ty = node.get("type")?.as_str()?;
    match ty {
        "panel-frame" => Some(ResolvedNode::Frame {
            sections: resolve_children(node),
        }),
        "section" => Some(ResolvedNode::Section {
            title: node
                .get("title")
                .map(|t| i18n(Some(t)))
                .filter(|s| !s.is_empty()),
            icon: node
                .get("icon")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            title_align: node
                .get("titleAlign")
                .and_then(Value::as_str)
                .unwrap_or("start")
                .to_string(),
            title_pad: sp(node, "titlePad", SECTION_TITLE_PR_DEFAULT),
            children: resolve_children(node),
        }),
        "column" => Some(ResolvedNode::Column {
            pad: sp(node, "pad", crate::ui::tokens::space::SP5),
            gap: sp(node, "gap", crate::ui::tokens::space::SP2),
            align_center: node.get("align").and_then(Value::as_str) == Some("center"),
            children: resolve_children(node),
        }),
        "row" => Some(ResolvedNode::Row {
            gap: sp(node, "gap", crate::ui::tokens::space::SP3),
            pad: sp(node, "pad", 0.),
            align: node
                .get("align")
                .and_then(Value::as_str)
                .unwrap_or("stretch")
                .to_string(),
            justify_between: node.get("justify").and_then(Value::as_str) == Some("between"),
            children: resolve_children(node),
        }),
        "group" => Some(ResolvedNode::Group {
            title: i18n(node.get("title")),
            children: resolve_children(node),
        }),
        "grid" => Some(ResolvedNode::Grid {
            min_item: node
                .get("minItem")
                .and_then(Value::as_str)
                .unwrap_or("SM")
                .to_string(),
            children: resolve_children(node),
        }),
        "divider" => Some(ResolvedNode::Divider),
        "text" => Some(ResolvedNode::Text {
            text: i18n(node.get("value")),
            tone: tone(node),
            size: text_size(node),
            weight: node
                .get("weight")
                .and_then(Value::as_str)
                .unwrap_or("regular")
                .to_string(),
            ellipsis: node
                .get("ellipsis")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        }),
        "icon" => Some(ResolvedNode::Icon {
            name: node
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("file")
                .to_string(),
            size: icon_size(node),
            tone: tone(node),
        }),
        "button" => Some(ResolvedNode::Button {
            label: i18n(node.get("label")),
            icon: node
                .get("icon")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            tip: node
                .get("tip")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            arg: node
                .get("arg")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            action: parse_action(node),
            menu: parse_menu(node),
            variant: node
                .get("variant")
                .and_then(Value::as_str)
                .unwrap_or("text")
                .to_string(),
            size: node
                .get("size")
                .and_then(Value::as_str)
                .unwrap_or("MD")
                .to_string(),
            disabled: node
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        other => {
            eprintln!("[pf-plugin] 未知组件已占位: {other}");
            Some(ResolvedNode::Unknown(other.to_string()))
        }
    }
}

fn resolve_children(node: &Value) -> Vec<ResolvedNode> {
    let mut out = Vec::new();
    if let Some(list) = node.get("children").and_then(Value::as_array) {
        for child in list {
            match resolve(child) {
                Some(n) => out.push(n),
                None => eprintln!("[pf-plugin] 组件节点无效已跳过: {child}"),
            }
        }
    }
    out
}

/// `text.size`：`XS/SM/MD/LG` → 面板轨字号（UI 规范 §2：
/// caption 10 / secondary 11 / **body 12** / title 14）。
fn text_size(node: &Value) -> gpui::Pixels {
    let base = match node.get("size").and_then(Value::as_str) {
        Some("XS") => 10.0,
        Some("MD") => 12.0,
        Some("LG") => 14.0,
        _ => 11.0, // SM 默认 = secondary
    };
    ui_size(base)
}

/// `icon.size`：`INDICATOR/XS/SM/MD/LG` → 图标五档（UI 规范 §3：10/12/14/16/18）。
fn icon_size(node: &Value) -> f32 {
    match node.get("size").and_then(Value::as_str) {
        Some("INDICATOR") => crate::ui::tokens::icon::INDICATOR,
        Some("XS") => crate::ui::tokens::icon::XS,
        Some("MD") => crate::ui::tokens::icon::MD,
        Some("LG") => crate::ui::tokens::icon::LG,
        _ => crate::ui::tokens::icon::SM,
    }
}

/// 图标引用 → 可渲染资产路径（01 §3.3 三来源）：
/// - 含 `/`：宿主预解析好的完整路径（`ctx.fileIcon()` 的返回值）**原样用**
/// - `assets:<名>`：插件包自带（`plugins/<id>/assets/<名>`）
/// - 其余：宿主内置名
fn icon_asset(name: &str, plugin_id: &str) -> String {
    if name.is_empty() {
        return "icons/file.svg".into();
    }
    if name.contains('/') {
        name.to_string()
    } else if let Some(n) = name.strip_prefix("assets:") {
        format!("plugins/{plugin_id}/assets/{n}")
    } else {
        format!("icons/{name}.svg")
    }
}

/// 分区数量（外壳几块）；非 frame 视为 1 块
fn section_count(tree: &ResolvedNode) -> usize {
    match tree {
        ResolvedNode::Frame { sections } => sections.len().max(1),
        _ => 1,
    }
}

// ---------------------------------------------------------------------------
// gpui 渲染层
// ---------------------------------------------------------------------------

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
        .child(render_node(&tree, ix, weak, t, &ctx));

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
fn render_children(
    children: &[ResolvedNode],
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> Vec<gpui::AnyElement> {
    children
        .iter()
        .map(|c| render_node(c, ix, weak, t, ctx))
        .collect()
}

fn render_node(
    node: &ResolvedNode,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    match node {
        // ---- 外壳：分区 + 各自滚动 + 可拖拽分隔线（宿主画，插件不参与）----
        ResolvedNode::Frame { sections } => {
            let n = sections.len().max(1);
            let mut col = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .h_full()
                .px(px(PANEL_PAD))
                .pt(px(2.))
                .pb(px(PANEL_PAD));
            for (i, sec) in sections.iter().enumerate() {
                if i > 0 {
                    col = col.child(render_splitter(ix, i - 1, weak, t));
                }
                let ratio = ctx.ratios.get(i).copied().unwrap_or(1. / n as f32);
                col = col.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_basis(gpui::relative(ratio))
                        .min_h_0()
                        .child(render_section(sec, i, ix, weak, t, ctx)),
                );
            }
            col.into_any_element()
        }
        ResolvedNode::Section { .. } => render_section(node, 0, ix, weak, t, ctx),
        ResolvedNode::Column {
            pad,
            gap,
            align_center,
            children,
        } => {
            let mut col = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .gap(px(*gap))
                .p(px(*pad));
            if *align_center {
                col = col.items_center();
            }
            for c in render_children(children, ix, weak, t, ctx) {
                col = col.child(c);
            }
            col.into_any_element()
        }
        ResolvedNode::Row {
            gap,
            pad,
            align,
            justify_between,
            children,
        } => {
            let mut row = div().flex().flex_wrap().gap(px(*gap)).p(px(*pad));
            row = match align.as_str() {
                "start" => row.items_start(),
                "center" => row.items_center(),
                "end" => row.items_end(),
                _ => row, // stretch = 默认
            };
            if *justify_between {
                row = row.justify_between();
            } else {
                row = row.justify_center();
            }
            for c in render_children(children, ix, weak, t, ctx) {
                row = row.child(c);
            }
            row.into_any_element()
        }
        ResolvedNode::Group { title, children } => {
            let mut col = div().flex().flex_col().min_h_0();
            if !title.is_empty() {
                col = col.child(group_header(title, t));
            }
            for c in render_children(children, ix, weak, t, ctx) {
                col = col.child(c);
            }
            col.into_any_element()
        }
        ResolvedNode::Grid { min_item, children } => {
            let cols = grid_cols(ctx.panel_w, min_item);
            let mut grid = div()
                .grid()
                .grid_cols(cols)
                .gap(px(GRID_GAP))
                .pt(px(4.))
                .pb(px(8.));
            for c in render_children(children, ix, weak, t, ctx) {
                grid = grid.child(c);
            }
            grid.into_any_element()
        }
        ResolvedNode::Divider => div()
            .mx(px(10.))
            .my(px(6.))
            .h(px(1.))
            .bg(hairline(t))
            .into_any_element(),
        ResolvedNode::Text {
            text,
            tone,
            size,
            weight,
            ellipsis,
        } => {
            let mut el = div()
                .pt(px(4.))
                .text_size(*size)
                .font_weight(match weight.as_str() {
                    "medium" => gpui::FontWeight::MEDIUM,
                    "semibold" => gpui::FontWeight::SEMIBOLD,
                    _ => gpui::FontWeight::NORMAL,
                })
                .text_color(rgb(tone_color(tone, t)));
            if *ellipsis {
                el = el.text_ellipsis();
            }
            el.child(SharedString::from(text.clone()))
                .into_any_element()
        }
        ResolvedNode::Icon { name, size, tone } => gpui::svg()
            .path(SharedString::from(icon_asset(name, ctx.plugin_id)))
            .text_color(rgb(tone_color(tone, t)))
            .size(px(*size))
            .into_any_element(),
        ResolvedNode::Button {
            label,
            icon,
            tip,
            arg,
            action,
            menu,
            variant,
            size,
            disabled,
        } => render_button(
            label, icon, tip, arg, action, menu, variant, size, *disabled, ix, weak, t, ctx,
        ),
        ResolvedNode::Unknown(ty) => div()
            .px(px(10.))
            .child(placeholder_center(&format!("[{ty}]"), t))
            .into_any_element(),
    }
}

/// 一个分区：标题（宿主样式）+ 自己的滚动区（每区一个 ScrollHandle）。
fn render_section(
    node: &ResolvedNode,
    seq: usize,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let (title, icon, title_align, title_pad, children): (
        Option<String>,
        Option<String>,
        &str,
        f32,
        &[ResolvedNode],
    ) = match node {
        ResolvedNode::Section {
            title,
            icon,
            title_align,
            title_pad,
            children,
        } => (
            title.clone(),
            icon.clone(),
            title_align.as_str(),
            *title_pad,
            children.as_slice(),
        ),
        other => (None, None, "start", 0., std::slice::from_ref(other)),
    };
    let mut col = div().flex().flex_col().flex_1().min_h_0();
    if title.is_some() || icon.is_some() {
        col = col.child(section_header(
            icon.as_deref(),
            title.as_deref().unwrap_or(""),
            title_align,
            title_pad,
            t,
            ctx.plugin_id,
        ));
    }
    let handle = ctx
        .scrolls
        .get(seq)
        .cloned()
        .unwrap_or_else(gpui::ScrollHandle::new);
    let mut body = div()
        .id(SharedString::from(format!("pf-sec-{ix}-{seq}")))
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&handle);
    for c in render_children(children, ix, weak, t, ctx) {
        body = body.child(c);
    }
    col.child(body).into_any_element()
}

/// 分区之间的分隔线：6px 命中区，按下后进入拖拽（比例归宿主）。
fn render_splitter(
    ix: usize,
    index: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let weak = weak.clone();
    div()
        .id(SharedString::from(format!("pf-split-{ix}-{index}")))
        .h(px(7.))
        .flex_none()
        .flex()
        .items_center()
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
            cx.stop_propagation();
            let y = f32::from(ev.position.y);
            let _ = weak.update(cx, |c, cx| {
                if let Some(e) = c.plugins.plugins.get_mut(ix) {
                    let base = e.split_ratios.borrow().clone();
                    *e.split_drag.borrow_mut() = Some(SplitDrag {
                        index,
                        start_y: y,
                        base,
                    });
                    cx.notify();
                }
            });
        })
        .child(div().flex_1().h(px(1.)).bg(hairline(t)))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn render_button(
    label: &str,
    icon: &str,
    tip: &str,
    arg: &str,
    action: &Option<CellAction>,
    menu: &[MenuItem],
    variant: &str,
    size: &str,
    disabled: bool,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    ctx: &RenderCtx,
) -> gpui::AnyElement {
    let dead = disabled || matches!(action, None | Some(CellAction::Unsupported(_)));
    let (cell_h, icon_px) = button_dims(size);
    let icon_only = variant == "icon";

    // 点/右键 都只在"活"按钮上挂
    let with_clicks = |mut el: gpui::Stateful<gpui::Div>| -> gpui::Stateful<gpui::Div> {
        if !dead {
            el = el.cursor_pointer().hover(|s| s.bg(rgb(t.bg_hover)));
            if !matches!(action, None | Some(CellAction::Unsupported(_))) {
                el = el.on_mouse_down(
                    MouseButton::Left,
                    click_action(weak.clone(), ix, action.clone()),
                );
            }
        }
        if !menu.is_empty() {
            el = el.on_mouse_down(
                MouseButton::Right,
                open_cell_menu(weak.clone(), ix, menu.to_vec(), arg.to_string()),
            );
        }
        el
    };

    // 纯图标按钮：方形，label 退成 tooltip
    if icon_only {
        let side = match size {
            "SM" => 28.,
            "LG" => 40.,
            _ => 34.,
        };
        let mut el = div()
            .id(SharedString::from(format!("pf-icobtn-{ix}-{label}")))
            .w(px(side))
            .h(px(side))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(crate::ui::tokens::radius::CTRL));
        el = with_clicks(el);
        let tip_text = if tip.is_empty() { label } else { tip };
        if !tip_text.is_empty() {
            el = el.with_tip_text(tip_text.to_string());
        }
        return el
            .child(
                gpui::svg()
                    .path(SharedString::from(icon_asset(icon, ctx.plugin_id)))
                    .text_color(rgb(if dead { t.text_dim } else { t.text }))
                    .size(px(icon_px.min(side - 8.))),
            )
            .into_any_element();
    }

    // 无图标 = 文字按钮（列内整行）
    if icon.is_empty() {
        let mut el = div()
            .id(SharedString::from(format!("pf-btn-{ix}-{label}")))
            .flex()
            .items_center()
            .h(px(cell_h * 0.34))
            .px(px(8.))
            .rounded(px(crate::ui::tokens::radius::CTRL))
            .text_size(ui_size(11.))
            .text_ellipsis()
            .text_color(rgb(if dead { t.text_dim } else { t.text }));
        el = with_clicks(el);
        if !tip.is_empty() {
            el = el.with_tip_text(tip.to_string());
        }
        return el
            .child(SharedString::from(label.to_string()))
            .into_any_element();
    }

    // 有图标 = 格子（图标上、短名下；尺寸随 button.size）
    let mut cell = div()
        .id(SharedString::from(format!("pf-cell-{ix}-{label}")))
        .w_full()
        .h(px(cell_h))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(crate::ui::tokens::space::SP1))
        .rounded(px(crate::ui::tokens::radius::CTRL));
    cell = with_clicks(cell);
    if !tip.is_empty() {
        cell = cell.with_tip_text(tip.to_string());
    }
    cell.child(
        div()
            .w(px(icon_px))
            .h(px(icon_px))
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::svg()
                    .path(SharedString::from(icon_asset(icon, ctx.plugin_id)))
                    .text_color(rgb(if dead { t.text_dim } else { t.text }))
                    .size(px(icon_px)),
            ),
    )
    .child(
        div()
            .w_full()
            .px(px(2.))
            .text_size(ui_size(11.))
            .text_center()
            .text_color(rgb(t.text_muted))
            .text_ellipsis()
            .child(SharedString::from(label.to_string())),
    )
    .into_any_element()
}

fn click_action(
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    action: Option<CellAction>,
) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    move |_, window, cx| {
        let action = action.clone();
        let _ = weak.update(cx, |c, cx| {
            if let Some(a) = action {
                c.plugin_dispatch(ix, vec![to_js_action(a)], window, cx);
            }
        });
    }
}

/// 右键：把菜单项连同鼠标位置记进 `PluginEntry::cell_menu`，由面板渲染弹出层。
fn open_cell_menu(
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    menu: Vec<MenuItem>,
    cell_path: String,
) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    move |ev: &gpui::MouseDownEvent, _window, cx| {
        cx.stop_propagation();
        let menu = menu.clone();
        let path = cell_path.clone();
        let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
        let _ = weak.update(cx, |c, cx| {
            if let Some(entry) = c.plugins.plugins.get_mut(ix) {
                entry.cell_menu = Some(CellMenuState {
                    x,
                    y,
                    path,
                    items: menu,
                });
                cx.notify();
            }
        });
    }
}

/// 插件格子右键菜单层：挂载点是 `psp-overlays`（与文件树右键菜单同一处），
/// 卡座与行样式复用 `menu_card` / `menu_item_asset`——**样式只有一份**。
pub(crate) fn cell_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let Some(ix) = chat
        .plugins
        .plugins
        .iter()
        .position(|p| p.cell_menu.is_some())
    else {
        return div().into_any_element();
    };
    let Some(menu) = chat.plugins.plugins[ix].cell_menu.clone() else {
        return div().into_any_element();
    };
    let plugin_id = chat.plugins.plugins[ix].manifest.id.clone();
    let weak = cx.entity().downgrade();
    // 与 psp 菜单同参：向下弹让 12px，左右象限决定横向方向
    let edges = crate::ui::popup::PopupEdges::around((menu.x, menu.y), vp, 0., 12.);
    let weak_out = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_out.update(cx, |c, cx| {
            if let Some(e) = c.plugins.plugins.get_mut(ix) {
                e.cell_menu = None;
                cx.notify();
            }
        });
    });

    let mut card = crate::function_panel::psp_overlays::menu_card(edges, t);
    for (i, item) in menu.items.iter().enumerate() {
        let cmd_id = item.id.clone();
        let action = item.action.clone();
        let path = menu.path.clone();
        let label = item.title.clone();
        let danger = item.danger;
        let asset = (!item.icon.is_empty()).then(|| icon_asset(&item.icon, &plugin_id));
        let weak_row = weak.clone();
        let row = crate::function_panel::psp_overlays::menu_item_asset(
            SharedString::from(format!("pf-menu-{ix}-{i}")),
            asset,
            14.,
            &label,
            danger,
            false,
        )
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            let (cmd_id, action, path) = (cmd_id.clone(), action.clone(), path.clone());
            let _ = weak_row.update(cx, |c, cx| {
                if let Some(entry) = c.plugins.plugins.get_mut(ix) {
                    entry.cell_menu = None;
                }
                match action {
                    Some(a) => c.plugin_dispatch(ix, vec![to_js_action(a)], window, cx),
                    None => c.plugin_command_fire(ix, &cmd_id, &path, window, cx),
                }
                cx.notify();
            });
        });
        card = card.child(row);
    }
    layer.child(card).into_any_element()
}

fn to_js_action(a: CellAction) -> JsAction {
    match a {
        CellAction::Run(p) => JsAction::Run(p),
        CellAction::Edit(p) => JsAction::Edit(p),
        CellAction::Reveal(p) => JsAction::RevealInExplorer(p),
        CellAction::Toast(m) => JsAction::Toast(m),
        CellAction::OpenUrl(u) => JsAction::OpenUrl(u),
        CellAction::OpenFolder(p) => JsAction::OpenFolder(p),
        CellAction::NewSession => JsAction::NewSession,
        CellAction::AskPi(t) => JsAction::AskPi(t),
        CellAction::Unsupported(name) => JsAction::Toast(format!("宿主不支持的动作：{name}")),
    }
}

/// 分区标题：图标 + 文字，**左对齐到内容左边**（外壳 `PANEL_PAD` 由
/// `panel-frame` 统一给，这里不再叠一层），上方留 `SECTION_HEAD_PT`。
fn section_header(
    icon_name: Option<&str>,
    label: &str,
    align: &str,
    pad: f32,
    t: &'static Theme,
    plugin_id: &str,
) -> gpui::AnyElement {
    let mut row = div()
        .flex_none()
        .flex()
        .items_center()
        // 对齐与留白由插件给意图（titleAlign / titlePad），像素在这里算：
        // 外壳已经有 PANEL_PAD，插件要的距离比它大就补差额，小则不再外扩
        .gap(px(6.))
        .pt(px(SECTION_HEAD_PT))
        .pb(px(2.));
    let inset = (pad - PANEL_PAD).max(0.);
    row = match align {
        "end" => row.justify_end().pr(px(inset)),
        "center" => row.justify_center(),
        _ => row.justify_start(),
    };
    if let Some(name) = icon_name {
        row = row.child(
            gpui::svg()
                .path(SharedString::from(icon_asset(name, plugin_id)))
                .text_color(rgb(t.text_faint))
                .size(px(crate::ui::tokens::icon::MD)),
        );
    }
    row.child(
        div()
            // 标题字号 = 面板正文（UI 规范 §2 body = ui(12)），不再用 9.5 小字
            .text_size(ui_size(12.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(t.text_faint))
            .child(SharedString::from(label.to_string())),
    )
    .into_any_element()
}

fn group_header(label: &str, t: &'static Theme) -> gpui::AnyElement {
    div()
        .h(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .into_any_element()
}

/// 发丝线：主题 border 在暗色下与底色太近（看不清），统一用 text 的低透明度。
fn hairline(t: &'static Theme) -> gpui::Rgba {
    gpui::rgba(crate::appearance::theme::rgba_a(t.text, 0.18))
}

fn tone_color(tone: &str, t: &'static Theme) -> u32 {
    match tone {
        "text" => t.text,
        "text.muted" => t.text_muted,
        "text.faint" => t.text_faint,
        "accent" => t.accent,
        "danger" => t.danger,
        _ => t.text_dim, // text.dim 默认
    }
}

fn placeholder_center(text: &str, t: &'static Theme) -> gpui::AnyElement {
    div()
        .text_size(ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn resolve_frame_sections_and_grid() {
        let v = tree(
            r#"{
              "type":"panel-frame","children":[
                {"type":"section","title":{"zh":"程序"},"icon":"play","titleAlign":"start","titlePad":"SP7","children":[
                  {"type":"grid","minItem":"SM","children":[
                    {"type":"button","icon":"icons/file_icons/terminal.svg","label":"build.ps1",
                     "tip":"D:/p/build.ps1","action":"run","arg":"D:/p/build.ps1",
                     "menu":[{"id":"reveal","title":{"zh":"定位"},"action":"reveal_in_explorer","arg":"D:/p/build.ps1"},
                             {"id":"unpin","title":{"zh":"移除快捷"}}]} ]} ]},
                {"type":"section","title":{"zh":"文档"},"icon":"assets:doc.svg","titleAlign":"start","titlePad":"SP8","children":[
                  {"type":"grid","minItem":"SM","children":[
                    {"type":"button","icon":"icons/file_icons/book.svg","label":"README.md",
                     "action":"edit","arg":"D:/p/README.md"} ]} ]} ]}"#,
        );
        let node = resolve(&v).expect("树应解析");
        let ResolvedNode::Frame { sections } = node else {
            panic!("根节点应为 panel-frame")
        };
        assert_eq!(sections.len(), 2);
        let ResolvedNode::Section {
            title,
            icon,
            title_align,
            title_pad,
            children,
        } = &sections[0]
        else {
            panic!("分区应为 section")
        };
        assert_eq!(title.as_deref(), Some("程序"));
        assert_eq!(icon.as_deref(), Some("play"));
        assert_eq!(title_align, "start");
        assert_eq!(*title_pad, 16.); // SP7
        let ResolvedNode::Grid {
            min_item,
            children: cells,
        } = &children[0]
        else {
            panic!("分区内容应为 grid")
        };
        assert_eq!(min_item, "SM");
        let ResolvedNode::Button {
            label,
            action,
            menu,
            ..
        } = &cells[0]
        else {
            panic!("格内应为 button")
        };
        assert_eq!(label, "build.ps1");
        assert_eq!(action, &Some(CellAction::Run("D:/p/build.ps1".into())));
        assert!(menu[0].action.is_some());
        assert!(menu[1].action.is_none());
    }

    #[test]
    fn grid_columns_follow_panel_width() {
        // 布局裁决权归宿主：面板越宽列数越多，但封顶 6
        assert_eq!(grid_cols(220., "SM"), 2);
        assert_eq!(grid_cols(340., "SM"), 3);
        assert_eq!(grid_cols(600., "SM"), 6);
        assert_eq!(grid_cols(900., "SM"), 6);
        assert_eq!(grid_cols(200., "LG"), 1);
        assert_eq!(grid_cols(340., "LG"), 1);
        assert_eq!(grid_cols(360., "LG"), 2);
        assert_eq!(grid_cols(900., "LG"), 5);
    }

    #[test]
    fn resolve_rejects_v1_shapes() {
        let v = tree(r#"{"type":"list","source":"quick_pins"}"#);
        assert!(matches!(resolve(&v), Some(ResolvedNode::Unknown(_))));
    }

    #[test]
    fn icon_paths() {
        assert_eq!(
            icon_asset("icons/file_icons/ps1.svg", "p"),
            "icons/file_icons/ps1.svg"
        );
        assert_eq!(icon_asset("terminal", "p"), "icons/terminal.svg");
        assert_eq!(
            icon_asset("assets:bolt.svg", "quick-panel"),
            "plugins/quick-panel/assets/bolt.svg"
        );
        assert_eq!(icon_asset("", "p"), "icons/file.svg");
    }
}
