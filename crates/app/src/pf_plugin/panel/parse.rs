//! 组件树解析层·遍历（契约 01）：JSON → [`ResolvedNode`] 的逐节点解析——
//! 动作白名单、右键菜单、canvas paint list、switch 校验、尺寸档位换算、
//! 图标三来源映射。类型与 tests 见 [`super::resolve`]。

use super::*;

/// 单画布 paint 上限（防护：QR ≈600 条足够，防异常树把面板拖死）。
pub(super) const CANVAS_PAINT_CAP: usize = 4096;

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
            pad_x: node.get("padX").and_then(Value::as_f64).map(|v| v as f32),
            pad_y: node.get("padY").and_then(Value::as_f64).map(|v| v as f32),
            w: node.get("w").and_then(Value::as_f64).map(|v| v as f32),
            bg: node.get("bg").and_then(Value::as_str).map(str::to_string),
            radius: node.get("radius").and_then(Value::as_f64).map(|v| v as f32),
            border: node
                .get("border")
                .and_then(Value::as_str)
                .map(str::to_string),
            gap: sp(node, "gap", crate::ui::tokens::space::SP2),
            align_center: node.get("align").and_then(Value::as_str) == Some("center"),
            justify_center: node.get("justify").and_then(Value::as_str) == Some("center"),
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
            justify_end: node.get("justify").and_then(Value::as_str) == Some("end"),
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
            command: node
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        "canvas" => {
            let paints = node
                .get("paint")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|p| {
                            let f = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.) as f32;
                            let s = |k: &str| {
                                p.get(k).and_then(Value::as_str).unwrap_or("").to_string()
                            };
                            match p.get("op").and_then(Value::as_str) {
                                Some("rect") => Some(CanvasPaint::Rect {
                                    x: f("x"),
                                    y: f("y"),
                                    w: f("w"),
                                    h: f("h"),
                                    fill: s("fill"),
                                }),
                                Some("text") => Some(CanvasPaint::Text {
                                    x: f("x"),
                                    y: f("y"),
                                    size: f("size"),
                                    tone: s("tone"),
                                    value: s("value"),
                                }),
                                // 未知 op：静默丢弃（画布是纯数据面，不占位不报错）
                                _ => None,
                            }
                        })
                        .take(CANVAS_PAINT_CAP)
                        .collect()
                })
                .unwrap_or_default();
            Some(ResolvedNode::Canvas {
                width: node.get("width").and_then(Value::as_f64).unwrap_or(0.) as f32,
                height: node.get("height").and_then(Value::as_f64).unwrap_or(0.) as f32,
                paints,
                command: node
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        }
        "switch" => Some(resolve_switch(node)),
        other => {
            eprintln!("[pf-plugin] 未知组件已占位: {other}");
            Some(ResolvedNode::Unknown(other.to_string()))
        }
    }
}

/// switch 节点解析（契约 v3）：value 必须布尔、command 必填 kebab。
fn resolve_switch(node: &Value) -> ResolvedNode {
    let bad = |msg: String| {
        eprintln!("[pf-plugin] switch 节点非法，已占位: {msg}");
        ResolvedNode::Unknown("switch".to_string())
    };
    let Some(value) = node.get("value").and_then(Value::as_bool) else {
        return bad("value 必须是布尔".into());
    };
    let Some(command) = node.get("command").and_then(Value::as_str) else {
        return bad("缺 command".into());
    };
    if !is_kebab_cmd(command) {
        return bad(format!("command「{command}」非 kebab-case"));
    }
    ResolvedNode::Switch {
        value,
        label: i18n(node.get("label")),
        tip: node
            .get("tip")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        command: command.to_string(),
        disabled: node
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// 命令 id kebab-case（与清单 is_kebab 同口径；switch.command / fn_bar.command 共用）。
fn is_kebab_cmd(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars().last().is_some_and(|c| c.is_ascii_alphanumeric())
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
pub(crate) fn icon_asset(name: &str, plugin_id: &str) -> String {
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
pub(crate) fn section_count(tree: &ResolvedNode) -> usize {
    match tree {
        ResolvedNode::Frame { sections } => sections.len().max(1),
        _ => 1,
    }
}
