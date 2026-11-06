//! 工具定义面板（ToolDefinitionsPanel.tsx parity；自 top_panels.rs 外移）：
//! 左列表（启用中的工具名）+ 右详情（描述/参数表）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use super::{empty_state, selected_tool};
use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::{tf, tr};

// ---------------------------------------------------------------------------
// ToolDefinitionsPanel.tsx：左列表（启用中的工具名）+ 右详情（描述/参数）
// ---------------------------------------------------------------------------

/// One row of the detail pane's 参数 table.
struct ParamField {
    name: String,
    type_text: String,
    description: String,
    required: bool,
    allowed_values: String,
    default_value: Option<String>,
}

fn format_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// pi-web `formatSchemaType`: anyOf/oneOf 合并、enum 无 type 时取值的 JS 类型、
/// array 加 `[]`、$ref 取末段名、其余 unknown。
fn schema_type(schema: &serde_json::Value) -> String {
    let variants = schema["anyOf"]
        .as_array()
        .or_else(|| schema["oneOf"].as_array());
    if let Some(variants) = variants {
        let mut out: Vec<String> = Vec::new();
        for v in variants {
            let kind = schema_type(v);
            if !out.contains(&kind) {
                out.push(kind);
            }
        }
        return out.join(" | ");
    }
    if !schema["const"].is_null() {
        return format_value(&schema["const"]);
    }
    if let Some(values) = schema["enum"]
        .as_array()
        .filter(|_| schema["type"].is_null())
    {
        let mut out: Vec<String> = Vec::new();
        for v in values {
            let kind = match v {
                serde_json::Value::Null => "null".to_string(),
                other => json_type_name(other).to_string(),
            };
            if !out.contains(&kind) {
                out.push(kind);
            }
        }
        return out.join(" | ");
    }
    let type_text = match &schema["type"] {
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        serde_json::Value::String(s) => s.clone(),
        _ => match schema["$ref"].as_str() {
            Some(r) => r.rsplit('/').next().unwrap_or("object").to_string(),
            None => "unknown".to_string(),
        },
    };
    if type_text == "array" {
        let item = if schema["items"].is_object() {
            schema_type(&schema["items"])
        } else {
            "unknown".to_string()
        };
        return format!("{item}[]");
    }
    type_text
}

/// JS `typeof`（pi-web 在 enum 无 type 分支用它取名）。
fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "object",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        _ => "object",
    }
}

/// pi-web `getToolParameterFields`。
fn param_fields(parameters: &serde_json::Value) -> Vec<ParamField> {
    let Some(props) = parameters["properties"].as_object() else {
        return Vec::new();
    };
    let required: Vec<&str> = parameters["required"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    props
        .iter()
        .map(|(name, value)| {
            let schema = if value.is_object() {
                value.clone()
            } else {
                serde_json::Value::Null
            };
            ParamField {
                name: name.clone(),
                type_text: schema_type(&schema),
                description: schema["description"].as_str().unwrap_or("").to_string(),
                required: required.contains(&name.as_str()),
                allowed_values: schema["enum"]
                    .as_array()
                    .map(|a| a.iter().map(format_value).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default(),
                default_value: schema.get("default").map(format_value),
            }
        })
        .collect()
}

/// 左导航位的工具列表 —— 与设置弹窗左导航同款外观（200px、nav 底、左下倒角、
/// 行 = 11px 等宽名 + 选中 bg_selected + 左侧 2px accent 竖条），选中态存
/// `Chat.tool_sel`（pi-web 的 selectedToolName）。
pub(super) fn tool_list(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    tools: Option<&[pi_link::transcript::ToolDecl]>,
    t: &Theme,
) -> gpui::AnyElement {
    let selected = selected_tool(chat, tools);
    let mut list = div()
        .id("tool-defs-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll();
    match tools {
        Some(list_tools) if !list_tools.is_empty() => {
            let mut rows: Vec<gpui::AnyElement> = Vec::new();
            for decl in list_tools {
                let active = selected.is_some_and(|s| s.name == decl.name);
                rows.push(tool_row(decl, active, weak, t));
            }
            list = list.flex().flex_col().children(rows);
        }
        Some(_) => {
            list = list.child(empty_state(tr("tool_defs.no_active_tools"), t, true));
        }
        None => {
            list = list.child(empty_state(tr("tool_defs.not_loaded"), t, true));
        }
    }
    div()
        .w(px(200.))
        .flex_shrink_0()
        .bg(rgb(t.nav))
        // 左导航贴弹窗左下角，同理自己倒左下角
        .rounded_bl(px(10.))
        .border_r_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
        .flex()
        .flex_col()
        .p(px(8.))
        .pt(px(12.))
        .child(list)
        .into_any_element()
}

/// 左导航一行（设置弹窗 nav_items 同款）：选中 = bg_selected + semibold +
/// 左侧 2px accent 竖条。
fn tool_row(
    decl: &pi_link::transcript::ToolDecl,
    active: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &Theme,
) -> gpui::AnyElement {
    let name = decl.name.clone();
    let weak = weak.clone();
    div()
        .id(SharedString::from(format!("tool-def-{}", decl.name)))
        .flex()
        .items_center()
        .gap(px(9.))
        .px(px(10.))
        .mb(px(1.))
        .py(px(7.5))
        .border_l_2()
        .border_color(if active {
            rgb(t.accent)
        } else {
            gpui::rgba(0x00000000)
        })
        .rounded(px(8.))
        .text_size(crate::appearance::ui_size(12.))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_color(rgb(if active { t.text } else { t.text_muted }))
        .when(active, |d| {
            d.bg(rgb(t.bg_selected))
                .font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.aux.tool_sel = Some(name.clone());
                cx.notify();
            });
        })
        .child(SharedString::from(decl.name.clone()))
        .into_any_element()
}

/// 右列详情：描述 / 参数表。pi-web `.tool-definition-*`。
pub(super) fn tool_detail(
    selected: Option<&pi_link::transcript::ToolDecl>,
    tools: Option<&[pi_link::transcript::ToolDecl]>,
    t: &Theme,
) -> gpui::AnyElement {
    let Some(decl) = selected else {
        let text = if tools.is_some() {
            tr("tool_defs.no_active_tools")
        } else {
            tr("tool_defs.not_loaded")
        };
        return div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(empty_state(text, t, true))
            .into_any_element();
    };
    let fields = param_fields(&decl.parameters);
    let mut body = div().flex().flex_col();

    if !decl.description.is_empty() {
        body = body
            .child(section_label(tr("tool_defs.description"), "", t))
            .child(
                div()
                    .mb(px(18.))
                    .text_size(crate::appearance::ui_size(12.))
                    .line_height(relative(1.55))
                    .text_color(rgb(t.text_muted))
                    .child(SharedString::from(decl.description.clone())),
            );
    }
    body = body.child(section_label(
        tr("tool_defs.params"),
        &tf(
            "tool_defs.parameters",
            &[("count", fields.len().to_string())],
        ),
        t,
    ));
    if fields.is_empty() {
        body = body.child(
            div()
                .pb(px(10.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("tool_defs.no_parameters")),
        );
    } else {
        body = body.child(
            div()
                .border_t_1()
                .border_color(rgb(t.border))
                .children(fields.iter().map(|f| param_row(f, t)).collect::<Vec<_>>()),
        );
    }

    div()
        .id("tool-def-scroll")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        // 内边距对齐设置弹窗 body（mc-body：pt22 / pl30 / pr30 / pb30）
        .pt(px(22.))
        .pb(px(30.))
        .px(px(30.))
        .child(body)
        .into_any_element()
}

/// 小节标题：左标签 + 右计数（pi-web `.tool-definition-section-label`）。
fn section_label(label: &str, right: &str, t: &Theme) -> gpui::Div {
    let mut row = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .mb(px(7.))
        .text_size(crate::appearance::ui_size(11.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(label.to_string()));
    if !right.is_empty() {
        row = row.child(
            div()
                .font_weight(gpui::FontWeight::NORMAL)
                .whitespace_nowrap()
                .child(SharedString::from(right.to_string())),
        );
    }
    row
}

/// 参数行：左=名 + 必填/可选，右=类型 + 说明 + 可选值/默认值。
fn param_row(f: &ParamField, t: &Theme) -> gpui::AnyElement {
    let mut value = div()
        .min_w_0()
        .text_size(crate::appearance::ui_size(11.))
        .line_height(relative(1.45))
        .text_color(rgb(t.text_muted))
        .child(
            div()
                .mb(px(3.))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_color(rgb(t.text))
                .child(SharedString::from(f.type_text.clone())),
        );
    if !f.description.is_empty() {
        value = value.child(SharedString::from(f.description.clone()));
    }
    if !f.allowed_values.is_empty() {
        value = value.child(meta_line(tr("tool_defs.allowed"), &f.allowed_values, t));
    }
    if let Some(default) = &f.default_value {
        value = value.child(meta_line(tr("tool_defs.default"), default, t));
    }
    div()
        .flex()
        .gap(px(12.))
        .py(px(9.))
        .border_b_1()
        .border_color(rgb(t.border))
        .text_size(crate::appearance::ui_size(11.))
        .line_height(relative(1.45))
        .child(
            div()
                .w(px(150.))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .text_color(rgb(t.text))
                .child(
                    div()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .child(SharedString::from(f.name.clone())),
                )
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(if f.required { t.accent } else { t.text_dim }))
                        .child(tr(if f.required { "必填" } else { "可选" })),
                ),
        )
        .child(value)
        .into_any_element()
}

fn meta_line(label: &str, value: &str, t: &Theme) -> gpui::Div {
    div()
        .mt(px(4.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(format!("{label}: ")))
        .child(
            div()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(value.to_string())),
        )
}
