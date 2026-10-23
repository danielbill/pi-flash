//! 插件面板渲染（beads 360 / 契约 v2 · 090-9）。
//!
//! **PF 是树解释器**：插件 `render(ctx)` 返回**已算好**的组件树（值内联、动作显式），
//! 本模块只按结构画——不认字段名、不做 filter、不折算动作。
//!
//! v1 的数据平面（`rows` / `$字段` 占位 / `filter{field,equals}` / `$auto` 折算）
//! 已按 090-9 整层删除：可执行→run、文档→edit 是**插件策略**，由插件自己判定。
//!
//! 结构分两层：
//! - [`resolve`]：组件树 JSON → [`ResolvedNode`]（纯逻辑，可单测，不碰 gpui）
//! - gpui 渲染层：ResolvedNode → 元素

use gpui::{MouseButton, SharedString, anchored, div, prelude::*, px, rgb};
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

/// 格子尺寸（图标落底板的整卡；图标 24 居中于 36 底板）。
const CELL: f32 = 76.;
/// 列数上限（再多就不像 dock 了）
const GRID_MAX_COLS: u16 = 6;

/// `grid.minItem` 档位 → 最小格宽（实现值，插件不可见：01 §2.4）
fn min_item_width(min_item: &str) -> f32 {
    match min_item {
        "LG" => 160.,
        "MD" => 120.,
        _ => 88., // SM 默认
    }
}

/// 列数全由宿主算：`clamp(floor(可用宽 / 档位宽), 1, 6)`（01 §2.4 布局裁决权）
fn grid_cols(panel_w: f32, min_item: &str) -> u16 {
    // 扣两侧内边距（SP5×2）+ 格子流 px(8)×2
    let usable = (panel_w - 2. * crate::ui::tokens::space::SP5 - 16.).max(1.);
    ((usable / min_item_width(min_item)).floor() as u16).clamp(1, GRID_MAX_COLS)
}
const CELL_ICON_PLATE: f32 = 36.;
const CELL_ICON: f32 = 36.;

// ---------------------------------------------------------------------------
// 解析层：组件树 JSON → ResolvedNode（纯逻辑，可单测）
// ---------------------------------------------------------------------------

/// 格子点击动作（01 §8 白名单）。v2 由插件显式给出，宿主不再折算。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CellAction {
    Run(String),
    Edit(String),
    Reveal(String),
    Toast(String),
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
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedNode {
    Column {
        pad: f32,
        gap: f32,
        align_center: bool,
        children: Vec<ResolvedNode>,
    },
    Row {
        gap: f32,
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
            })
        })
        .collect()
}

/// 组件树解析入口。未知组件 → [`ResolvedNode::Unknown`]（渲染占位并记日志）。
pub(crate) fn resolve(node: &Value) -> Option<ResolvedNode> {
    let ty = node.get("type")?.as_str()?;
    match ty {
        "column" => Some(ResolvedNode::Column {
            pad: sp(node, "pad", crate::ui::tokens::space::SP5),
            gap: sp(node, "gap", crate::ui::tokens::space::SP2),
            align_center: node.get("align").and_then(Value::as_str) == Some("center"),
            children: resolve_children(node),
        }),
        "row" => Some(ResolvedNode::Row {
            gap: sp(node, "gap", crate::ui::tokens::space::SP3),
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

/// `text.size`：`XS/SM/MD/LG` → 字号档（走 ui_size，插件写的是相对档）。
fn text_size(node: &Value) -> gpui::Pixels {
    let base = match node.get("size").and_then(Value::as_str) {
        Some("XS") => 9.5,
        Some("MD") => 13.0,
        Some("LG") => 15.0,
        _ => 11.0, // SM 默认
    };
    ui_size(base)
}

/// `icon.size`：`INDICATOR/XS/SM/MD/LG` → 图标边长。
fn icon_size(node: &Value) -> f32 {
    match node.get("size").and_then(Value::as_str) {
        Some("INDICATOR") => 6.0,
        Some("XS") => 12.0,
        Some("MD") => 20.0,
        Some("LG") => 24.0,
        _ => 16.0, // SM 默认
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

    // 宿主标准外框：顶层 column 的 pad 可覆盖默认内边距
    let top_pad = match &tree {
        ResolvedNode::Column { pad, .. } if *pad > 0. => *pad,
        _ => crate::ui::tokens::space::SP5,
    };
    let id = entry.manifest.id.clone();
    let cell_menu = entry.cell_menu.clone();
    let panel_w = chat.layout.slp_w;
    let mut root = div()
        .id("pf-panel")
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px(px(top_pad))
        .pt(px(top_pad))
        .pb(px(4.))
                .child(render_node(&tree, ix, weak, t, &id, panel_w));
    if let Some(menu) = cell_menu {
        let close_weak = weak.clone();
        root = root
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = close_weak.update(cx, |c, cx| {
                    if let Some(entry) = c.plugins.plugins.get_mut(ix) {
                        entry.cell_menu = None;
                        cx.notify();
                    }
                });
            })
            .child(render_cell_menu(&menu, ix, weak, t));
    }
    root.into_any_element()
}

/// 子节点渲染：**纯结构**，宿主不按组件组合猜布局（契约 layout_ownership）。
/// 想要格子就让插件显式写 `grid`。
fn render_children(
    children: &[ResolvedNode],
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    plugin_id: &str,
    panel_w: f32,
) -> Vec<gpui::AnyElement> {
    children
        .iter()
        .map(|c| render_node(c, ix, weak, t, plugin_id, panel_w))
        .collect()
}

fn render_node(
    node: &ResolvedNode,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    plugin_id: &str,
    panel_w: f32,
) -> gpui::AnyElement {
    match node {
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
            for c in render_children(children, ix, weak, t, plugin_id, panel_w) {
                col = col.child(c);
            }
            col.into_any_element()
        }
        ResolvedNode::Row {
            gap,
            justify_between,
            children,
        } => {
            let mut row = div()
                .flex()
                .flex_wrap()
                .gap(px(*gap))
                .pt(px(4.))
                .pb(px(6.))
                .justify_center();
            if *justify_between {
                row = row.justify_between();
            }
            for c in children {
                row = row.child(render_node(c, ix, weak, t, plugin_id, panel_w));
            }
            row.into_any_element()
        }
        ResolvedNode::Group { title, children } => {
            let mut col = div().flex().flex_col().min_h_0();
            if !title.is_empty() {
                col = col.child(group_header(title, t));
            }
            for c in render_children(children, ix, weak, t, plugin_id, panel_w) {
                col = col.child(c);
            }
            col.into_any_element()
        }
        ResolvedNode::Grid {
            min_item,
            children,
        } => {
            let cols = grid_cols(panel_w, min_item);
            let mut grid = div()
                .grid()
                .grid_cols(cols)
                .gap(px(crate::ui::tokens::space::SP2))
                .px(px(8.))
                .pt(px(6.))
                .pb(px(8.));
            for c in children {
                grid = grid.child(render_node(c, ix, weak, t, plugin_id, panel_w));
            }
            grid.into_any_element()
        }
        ResolvedNode::Divider => div()
            .mx(px(10.))
            .my(px(6.))
            .h(px(1.))
            .bg(hairline(t))
            .into_any_element(),
        ResolvedNode::Text { text, tone, size } => div()
            .px(px(10.))
            .pt(px(4.))
            .text_size(*size)
            .text_color(rgb(tone_color(tone, t)))
            .child(SharedString::from(text.clone()))
            .into_any_element(),
        ResolvedNode::Icon { name, size, tone } => gpui::svg()
            .path(SharedString::from(icon_asset(name, plugin_id)))
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
        } => render_button(label, icon, tip, arg, action, menu, ix, weak, t, plugin_id),
        // cols 只对格子流有意义，其余节点忽略
        ResolvedNode::Unknown(ty) => div()
            .px(px(10.))
            .child(placeholder_center(&format!("[{ty}]"), t))
            .into_any_element(),
    }
}

/// 按钮 = 格子卡（图标底板 + 短名）或纯文字按钮（无图标时）。点击执行动作；
/// 有 `menu` 时右键……（v2 暂只支持宿主动作项，见 [`dispatch_menu`]）
#[allow(clippy::too_many_arguments)]
fn render_button(
    label: &str,
    icon: &str,
    tip: &str,
    arg: &str,
    action: &Option<CellAction>,
    menu: &[MenuItem],
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    plugin_id: &str,
) -> gpui::AnyElement {
    // 无图标 = 文字按钮（列内整行）
    if icon.is_empty() {
        let disabled = matches!(action, None | Some(CellAction::Unsupported(_)));
        let mut el = div()
            .id(SharedString::from(format!("pf-btn-{ix}-{label}")))
            .flex()
            .items_center()
            .h(px(24.))
            .px(px(8.))
            .rounded(px(crate::ui::tokens::radius::CTRL))
            .text_size(ui_size(11.))
            .text_color(rgb(if disabled { t.text_dim } else { t.text }));
        if !disabled {
            el = el.cursor_pointer().hover(|s| s.bg(rgb(t.bg_hover)));
        }
        if !tip.is_empty() {
            el = el.with_tip_text(tip.to_string());
        }
        if let Some(CellAction::Run(_) | CellAction::Edit(_) | CellAction::Reveal(_)) = action {
            el = el.on_mouse_down(
                MouseButton::Left,
                click_action(weak.clone(), ix, action.clone()),
            );
        }
        // 右键：菜单项（宿主动作类直接可用；插件回调类走 onCommand）
        if !menu.is_empty() {
            el = el.on_mouse_down(
                MouseButton::Right,
                open_cell_menu(weak.clone(), ix, menu.to_vec(), arg.to_string()),
            );
        }
        return el
            .child(SharedString::from(label.to_string()))
            .into_any_element();
    }

    // 有图标 = 格子卡（图标上、短名下）
    let mut cell = div()
        .id(SharedString::from(format!("pf-cell-{ix}-{label}")))
        .w_full()
        .h(px(CELL))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(crate::ui::tokens::space::SP1))
        .rounded(px(crate::ui::tokens::radius::CTRL));
    if !matches!(action, None | Some(CellAction::Unsupported(_))) {
        cell = cell.cursor_pointer().hover(|s| s.bg(rgb(t.bg_hover)));
    }
    if !tip.is_empty() {
        cell = cell.with_tip_text(tip.to_string());
    }
    if let Some(CellAction::Run(_) | CellAction::Edit(_) | CellAction::Reveal(_)) = action {
        cell = cell.on_mouse_down(
            MouseButton::Left,
            click_action(weak.clone(), ix, action.clone()),
        );
    }
    if !menu.is_empty() {
        cell = cell.on_mouse_down(
            MouseButton::Right,
            open_cell_menu(weak.clone(), ix, menu.to_vec(), arg.to_string()),
        );
    }
    cell.child(
        div()
            .w(px(CELL_ICON_PLATE))
            .h(px(CELL_ICON_PLATE))
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::svg()
                    .path(SharedString::from(icon_asset(icon, plugin_id)))
                    .text_color(rgb(t.text))
                    .size(px(CELL_ICON)),
            ),
    )
    .child(
        div()
            .w_full()
            .px(px(2.))
            .text_size(ui_size(10.5))
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

/// 右键弹出层（窗口坐标锚定；点项即执行并收起）。
fn render_cell_menu(
    menu: &CellMenuState,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let mut card = div()
        .w(px(168.))
        .flex()
        .flex_col()
        .p(px(4.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(hairline(t))
        .rounded(px(crate::ui::tokens::radius::CARD))
        .shadow_sm();
    for (i, item) in menu.items.iter().enumerate() {
        let id = item.id.clone();
        let title = item.title.clone();
        let action = item.action.clone();
        let path = menu.path.clone();
        let weak = weak.clone();
        card = card.child(
            div()
                .id(SharedString::from(format!("pf-menu-{ix}-{i}")))
                .h(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(crate::ui::tokens::radius::XS))
                .text_size(ui_size(11.5))
                .text_color(rgb(t.text))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    let (id, action, path) = (id.clone(), action.clone(), path.clone());
                    let _ = weak.update(cx, |c, cx| {
                        if let Some(entry) = c.plugins.plugins.get_mut(ix) {
                            entry.cell_menu = None;
                        }
                        match action {
                            Some(a) => c.plugin_dispatch(ix, vec![to_js_action(a)], window, cx),
                            None => c.plugin_command_fire(ix, &id, &path, window, cx),
                        }
                        cx.notify();
                    });
                })
                .child(SharedString::from(title)),
        );
    }
    anchored()
        .position(gpui::point(px(menu.x), px(menu.y)))
        .child(card)
        .into_any_element()
}

fn to_js_action(a: CellAction) -> JsAction {
    match a {
        CellAction::Run(p) => JsAction::Run(p),
        CellAction::Edit(p) => JsAction::Edit(p),
        CellAction::Reveal(p) => JsAction::RevealInExplorer(p),
        CellAction::Toast(m) => JsAction::Toast(m),
        CellAction::Unsupported(name) => JsAction::Toast(format!("宿主不支持的动作：{name}")),
    }
}

fn group_header(label: &str, t: &'static Theme) -> gpui::AnyElement {
    div()
        .h(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_size(ui_size(9.5))
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
    fn resolve_inline_tree() {
        // v2.1 内联树：grid 显式声明格子流，没有 rows / filter / $auto
        let v = tree(
            r#"{
              "type":"column","pad":"SP4","gap":"SP2","children":[
                {"type":"group","title":{"zh":"可执行文件"},"children":[
                  {"type":"grid","minItem":"SM","children":[
                    {"type":"button","icon":"icons/file_icons/terminal.svg","label":"build.ps1",
                     "tip":"D:/p/build.ps1","action":"run","arg":"D:/p/build.ps1",
                     "menu":[{"id":"reveal","title":{"zh":"定位"},"action":"reveal_in_explorer","arg":"D:/p/build.ps1"},
                             {"id":"unpin","title":{"zh":"移除快捷"}}]} ]} ]},
                {"type":"divider"},
                {"type":"group","title":{"zh":"文档与代码"},"children":[
                  {"type":"grid","minItem":"SM","children":[
                    {"type":"button","icon":"icons/file_icons/book.svg","label":"README.md",
                     "action":"edit","arg":"D:/p/README.md"} ]} ]} ]}"#,
        );
        let node = resolve(&v).expect("树应解析");
        let ResolvedNode::Column { children, .. } = node else {
            panic!("根节点应为 column")
        };
        assert_eq!(children.len(), 3);
        let ResolvedNode::Group {
            title,
            children: group_children,
        } = &children[0]
        else {
            panic!("第一项应为 group")
        };
        assert_eq!(title, "可执行文件");
        let ResolvedNode::Grid {
            min_item,
            children: cells,
        } = &group_children[0]
        else {
            panic!("组内应为 grid")
        };
        assert_eq!(min_item, "SM");
        let ResolvedNode::Button {
            label, action, menu, ..
        } = &cells[0]
        else {
            panic!("格内应为 button")
        };
        assert_eq!(label, "build.ps1");
        assert_eq!(action, &Some(CellAction::Run("D:/p/build.ps1".into())));
        assert_eq!(menu.len(), 2);
        assert!(menu[0].action.is_some()); // 宿主执行
        assert!(menu[1].action.is_none()); // 回调插件
        let ResolvedNode::Divider = children[1] else {
            panic!("第二项应为 divider")
        };
    }

    #[test]
    fn grid_columns_follow_panel_width() {
        // 布局裁决权归宿主：面板越宽列数越多，但封顶 6
        assert_eq!(grid_cols(220., "SM"), 2);
        assert_eq!(grid_cols(340., "SM"), 3); // 默认 dock 宽
        assert_eq!(grid_cols(600., "SM"), 6);
        assert_eq!(grid_cols(900., "SM"), 6); // 封顶
        assert_eq!(grid_cols(340., "LG"), 1);
        assert_eq!(grid_cols(900., "LG"), 5);
    }

    #[test]
    fn resolve_rejects_v1_shapes() {
        // 数据平面已删：list 不再认识的组件 → 占位而非 panic
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
