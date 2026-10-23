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

/// 格子尺寸（图标落底板的整卡；图标 24 居中于 36 底板）。
const CELL: f32 = 88.;
const CELL_ICON_PLATE: f32 = 36.;
const CELL_ICON: f32 = 24.;

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
    div()
        .id("pf-panel")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px(px(top_pad))
        .pt(px(top_pad))
        .pb(px(4.))
        .child(render_node(&tree, ix, weak, t, &id))
        .into_any_element()
}

fn render_node(
    node: &ResolvedNode,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    plugin_id: &str,
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
            for c in children {
                col = col.child(render_node(c, ix, weak, t, plugin_id));
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
                row = row.child(render_node(c, ix, weak, t, plugin_id));
            }
            row.into_any_element()
        }
        ResolvedNode::Group { title, children } => {
            let mut col = div().flex().flex_col().min_h_0();
            if !title.is_empty() {
                col = col.child(group_header(title, t));
            }
            for c in children {
                col = col.child(render_node(c, ix, weak, t, plugin_id));
            }
            col.into_any_element()
        }
        ResolvedNode::Divider => div()
            .mx(px(10.))
            .my(px(4.))
            .h(px(1.))
            .bg(rgb(t.border))
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
                right_click_menu(weak.clone(), ix, menu.to_vec(), arg.to_string()),
            );
        }
        return el
            .child(SharedString::from(label.to_string()))
            .into_any_element();
    }

    // 有图标 = 格子卡（图标上、短名下）
    let mut cell = div()
        .id(SharedString::from(format!("pf-cell-{ix}-{label}")))
        .group("pf-cell")
        .w(px(CELL))
        .flex()
        .flex_col()
        .items_center()
        .pt(px(crate::ui::tokens::space::SP2))
        .pb(px(crate::ui::tokens::space::SP1))
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
            right_click_menu(weak.clone(), ix, menu.to_vec(), arg.to_string()),
        );
    }
    cell.child(
        div()
            .w(px(CELL_ICON_PLATE))
            .h(px(CELL_ICON_PLATE))
            .rounded(px(crate::ui::tokens::radius::CTRL))
            .bg(rgb(t.bg_subtle))
            .flex()
            .items_center()
            .justify_center()
            .group_hover("pf-cell", |s| s.bg(rgb(t.bg_selected)))
            .child(
                gpui::svg()
                    .path(SharedString::from(icon_asset(icon, plugin_id)))
                    .text_color(rgb(t.text))
                    .size(px(CELL_ICON)),
            ),
    )
    .child(
        div()
            .max_w(px(CELL))
            .text_size(ui_size(10.5))
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

/// 右键菜单：v2 暂用「点哪项执行哪项」的降级形态——只执行宿主动作类项，
/// 插件回调类（无 action）在状态栏提示（完整弹出菜单见 beads 后续任务）。
fn right_click_menu(
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    menu: Vec<MenuItem>,
    cell_path: String,
) -> impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    move |_, window, cx| {
        let menu = menu.clone();
        let path = cell_path.clone();
        let _ = weak.update(cx, |c, cx| {
            // 纯插件回调菜单（如「移除快捷」）：唯一一项时直接执行，多项则提示
            let callbacks: Vec<&MenuItem> = menu.iter().filter(|m| m.action.is_none()).collect();
            let host_actions: Vec<&MenuItem> = menu.iter().filter(|m| m.action.is_some()).collect();
            if callbacks.len() == 1 && host_actions.is_empty() {
                let id = callbacks[0].id.clone();
                c.plugin_command_fire(ix, &id, &path, window, cx);
            } else if callbacks.len() == 1 && host_actions.len() == 1 {
                // 常见组合：定位文件 + 移除快捷 → 右键 = 定位，Shift/后续再补弹层
                let a = host_actions[0].action.clone().unwrap();
                c.plugin_dispatch(ix, vec![to_js_action(a)], window, cx);
            } else {
                c.set_status("插件菜单项见后续版本（v2 未接弹出层）".to_string(), cx);
            }
        });
    }
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
        // v2 内联树：没有 rows / filter / $auto
        let v = tree(
            r#"{
              "type":"column","pad":"SP4","gap":"SP2","children":[
                {"type":"group","title":{"zh":"可执行文件"},"children":[
                  {"type":"button","icon":"icons/file_icons/terminal.svg","label":"build.ps1",
                   "tip":"D:\\p\\build.ps1","action":"run","arg":"D:\\p\\build.ps1",
                   "menu":[{"id":"reveal","title":{"zh":"定位"},"action":"reveal_in_explorer","arg":"D:\\p\\build.ps1"},
                           {"id":"unpin","title":{"zh":"移除快捷"}}]} ]},
                {"type":"divider"},
                {"type":"group","title":{"zh":"文档与代码"},"children":[
                  {"type":"button","icon":"icons/file_icons/book.svg","label":"README.md",
                   "action":"edit","arg":"D:\\p\\README.md"} ]} ]}"#,
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
        let ResolvedNode::Button {
            label,
            action,
            menu,
            ..
        } = &group_children[0]
        else {
            panic!("组内应为 button")
        };
        assert_eq!(label, "build.ps1");
        assert_eq!(action, &Some(CellAction::Run(r"D:\p\build.ps1".into())));
        assert_eq!(menu.len(), 2);
        assert!(menu[0].action.is_some()); // 宿主执行
        assert!(menu[1].action.is_none()); // 回调插件
        // 根节点第三项是第二个 group（divider 在根上）
        let ResolvedNode::Divider = children[1] else {
            panic!("第二项应为 divider")
        };
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
