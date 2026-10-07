//! 系统提示词 / 工具定义 两个面板 —— **直接复用设置弹窗那套窗体**
//! （`ui::overlay::big_card`：0.7×0.98 大卡片 + chrome 顶条 + ×；外面那层
//! 遮挡/外点/ESC 由 `ui::overlay::layer` 保证）。
//!
//! 内容逐条对齐 pi-web 组件：
//! - [`system_prompt_panel`] ← components/SystemPromptPanel.tsx
//! - [`tool_definitions_panel`] ← components/ToolDefinitionsPanel.tsx
//!   （左侧工具列表走大卡片的 nav 位，与设置弹窗左导航同款）
//!
//! 数据同源：pi 0.86+ 把系统提示词与工具声明写进 transcript 的 system 消息，
//! runtime 在 get_messages 时用 `pi_link::transcript::transcript_system` replay
//! （pi-ai transcript.js 的读法，pi-web 的 `state.systemPrompt` 也是这份重放）。
//!
//! 历史上这里曾是一幅「topbar 下方半屏面板」（pi-web activeTopPanel 的位置），
//! 用户口径改为复用设置弹窗窗体后删除——不要再自造窗体。

use gpui::{App, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::TopPanel;
use crate::i18n::{tf, tr};
use crate::theme::Theme;

/// ⋯ 菜单入口：把两个面画进设置弹窗同款窗体（遮挡/外点/ESC = 浮层基座）。
pub(crate) fn session_info_dialog(
    kind: TopPanel,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &Theme,
    cx: &App,
) -> gpui::AnyElement {
    let sys = chat.rt().read(cx).sys.clone();
    let title = match kind {
        TopPanel::System => tr("系统提示词"),
        TopPanel::Tools => tr("工具定义"),
    };
    // 左导航位：工具面板放工具名列表（设置弹窗左导航同款），系统提示词没有
    let nav = match kind {
        TopPanel::System => None,
        TopPanel::Tools => Some(tool_list(
            chat,
            weak,
            sys.as_ref().map(|s| s.tools.as_slice()),
            t,
        )),
    };
    let body = match kind {
        TopPanel::System => {
            system_prompt_panel(sys.as_ref(), chat, weak, &chat.sysprompt_scroll, t)
        }
        TopPanel::Tools => tool_detail(
            selected_tool(
                chat,
                sys.as_ref().map(|s| s.tools.as_slice()),
            ),
            sys.as_ref().map(|s| s.tools.as_slice()),
            t,
        ),
    };

    let weak_dismiss = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| {
            c.dialog = None;
            cx.notify();
        });
    };
    crate::ui::overlay::layer(true, Some(&chat.dialog_focus), dismiss.clone())
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::big_card(&title, nav, body, t, dismiss))
        .into_any_element()
}

/// 当前选中的工具：`tool_sel` 命中就用它，否则回落列表首项（pi-web
/// ToolDefinitionsPanel 的 selectedToolName ?? activeTools[0] 同义）。
fn selected_tool<'a>(
    chat: &Chat,
    tools: Option<&'a [pi_link::transcript::ToolDecl]>,
) -> Option<&'a pi_link::transcript::ToolDecl> {
    chat.tool_sel
        .as_deref()
        .and_then(|name| tools.and_then(|list| list.iter().find(|t| t.name == name)))
        .or_else(|| tools.and_then(|list| list.first()))
}

/// 空态/未加载态文案（pi-web `.system-prompt-empty` / `.tool-definitions-empty`：
/// 12px、斜体、muted；`padded` = 列表里的 14px/12px 内边距）。
fn empty_state(text: &str, t: &Theme, padded: bool) -> gpui::AnyElement {
    let mut el = div()
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text_muted))
        .italic()
        .child(SharedString::from(text.to_string()));
    el = if padded {
        el.px(px(12.)).py(px(14.))
    } else {
        el.py(px(10.))
    };
    el.into_any_element()
}

/// SystemPromptPanel.tsx + token 分析块：顶部固定分析块（高 100px，不随内容
/// 滚动，用户定稿）——总计 + 7 大类「名称 token数 比例」8 个信息块一行排布 +
/// 100% 宽比例长条；下方滚动区等宽 12px、行高 1.6、muted、pre-wrap。psp 常显
/// 滚动条只包滚动区（不延展进分析块）；`scroll` 挂在 Chat 上（跨渲染持久）。
fn system_prompt_panel(
    sys: Option<&pi_link::transcript::TranscriptSystem>,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    scroll: &gpui::ScrollHandle,
    t: &Theme,
) -> gpui::AnyElement {
    // 正文按分类分段渲染：每段整块铺类别色淡背景（~15% alpha，用户定稿：
    // 不要竖条），段间 10px —— 与分析块/长条的类别色一一对应。有声明的桶
    // （系统工具/插件/MCP）在其文本下方追加折叠的「调用声明」块
    let body = match sys {
        Some(s) if !s.prompt.is_empty() => {
            let agent_dir = pi_link::paths::pi_agent_dir();
            let segs = pi_link::transcript::segments(s, agent_dir.as_deref());
            let decls = pi_link::transcript::declarations(s);
            let n = segs.len();
            div().flex().flex_col().children(
                segs.into_iter().enumerate().map(move |(i, (bucket, text))| {
                    let color = crate::theme::bucket_color(bucket);
                    let bucket_decl = decls
                        .iter()
                        .find(|(b, _)| *b == bucket)
                        .map(|(_, v)| v);
                    let idx =
                        pi_link::transcript::SYSTEM_BUCKETS.iter().position(|x| *x == bucket);
                    let mut seg = div()
                        .w_full()
                        .rounded(px(4.))
                        .bg(gpui::rgba((color << 8) | 0x26))
                        .px(px(8.))
                        .py(px(5.))
                        .mb(if i + 1 < n { px(10.) } else { px(0.) })
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(12.))
                        .line_height(relative(1.6))
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(text));
                    if let (Some(decl_list), Some(idx)) = (bucket_decl, idx) {
                        seg = seg.child(decl_block(
                            idx,
                            decl_list,
                            chat.decl_open[idx],
                            weak,
                            t,
                        ));
                    }
                    seg.into_any_element()
                }),
            ).into_any_element()
        }
        // prompt === "" → 空提示词（工具已禁用）；未加载 → 尚未加载
        Some(_) => empty_state(tr("系统提示词为空（工具已禁用）"), t, false),
        None => empty_state(tr("系统提示词加载中…"), t, false),
    };
    // 分析块：加载且有内容才画
    let mut col = div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col();
    if let Some(s) = sys.filter(|s| !s.prompt.is_empty() || !s.tools.is_empty()) {
        col = col.child(stats_block(s, t));
    }
    // 滚动条只属于滚动区：relative 包裹 + 滚动条绝对定位贴右缘
    // （actions_menu / vlist 同款结构），不得延展进上面的统计面板
    col.child(
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("sys-prompt-scroll")
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(scroll)
                    // 内边距对齐设置弹窗 body（mc-body：pl30 / pr30 / pb30）
                    .pt(px(14.))
                    .pb(px(30.))
                    .px(px(30.))
                    .child(body),
            )
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .bottom(px(2.))
                    .right(px(0.))
                    .w(px(10.))
                    .child(crate::ui::psp_scrollbar::menu_scrollbar(scroll)),
            ),
    )
    .into_any_element()
}

/// token 分析块（固定 100px，不参与滚动）：一行 8 个信息块（总计 + 7 类，
/// 每格 1/8 宽，块 = 色点+名称 / token 数+比例）+ 底部 100% 宽比例长条。
fn stats_block(sys: &pi_link::transcript::TranscriptSystem, t: &Theme) -> gpui::AnyElement {
    let agent_dir = pi_link::paths::pi_agent_dir();
    let tokens = pi_link::transcript::breakdown(sys, agent_dir.as_deref());
    let total: u64 = tokens.iter().sum();
    let mut row = vec![stat_cell(tr("总计"), total, total, t.accent, t, true)];
    for (i, bucket) in pi_link::transcript::SYSTEM_BUCKETS.iter().enumerate() {
        row.push(stat_cell(
            bucket_label(*bucket),
            tokens[i],
            total,
            crate::theme::bucket_color(*bucket),
            t,
            false,
        ));
    }

    div()
        .h(px(100.))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .justify_between()
        .pt(px(14.))
        .pb(px(12.))
        .px(px(30.))
        .child(div().flex().children(row))
        .child(ratio_bar(&tokens, total))
        .into_any_element()
}

/// 「调用声明」折叠块：挂在所属类别的文本段下方，颜色服从类别，整体字号
/// 比正文小 2px（12→10）。默认折叠只留标题行（▸ 调用声明 · N 条 · x tokens），
/// 点击展开逐条列 name + description（声明就是随 API `tools` 字段进上下文的
/// 那部分，展开后的体量 ≈ 统计里该桶的声明 token）。
fn decl_block(
    idx: usize,
    decls: &[pi_link::transcript::ToolDecl],
    open: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &Theme,
) -> gpui::AnyElement {
    let color = crate::theme::bucket_color(pi_link::transcript::SYSTEM_BUCKETS[idx]);
    let tokens = pi_link::estimate::estimate_tokens(&pi_link::transcript::tools_wire_text(decls));
    let title = format!(
        "{} {} · {} · {} tokens",
        if open { "▾" } else { "▸" },
        tr("调用声明"),
        tf("{n} 条", &[("n", decls.len().to_string())]),
        crate::services::format::fmt_thousand(tokens),
    );
    let weak_toggle = weak.clone();
    let mut block = div()
        .mt(px(8.))
        .pt(px(6.))
        .border_t_1()
        .border_color(gpui::rgba((color << 8) | 0x55))
        .child(
            div()
                .id(SharedString::from(format!("decl-toggle-{idx}")))
                .flex()
                .items_center()
                .rounded(px(3.))
                .px(px(4.))
                .py(px(2.))
                .cursor_pointer()
                .text_size(crate::appearance::ui_size(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(color))
                .hover(|s| s.bg(gpui::rgba((color << 8) | 0x1e)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    let _ = weak_toggle.update(cx, |c, cx| {
                        c.decl_open[idx] = !c.decl_open[idx];
                        cx.notify();
                    });
                })
                .child(SharedString::from(title)),
        );
    if open {
        block = block.child(
            div().mt(px(2.)).flex().flex_col().gap(px(6.)).children(
                decls.iter().map(|d| {
                    div()
                        .flex()
                        .flex_col()
                        .text_size(crate::appearance::ui_size(10.))
                        .font_family(crate::markdown::MONO_FAMILY)
                        .line_height(relative(1.5))
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(SharedString::from(d.name.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(t.text_muted))
                                .child(SharedString::from(d.description.clone())),
                        )
                        .into_any_element()
                }),
            ),
        );
    }
    block.into_any_element()
}

/// 一个信息块：色点 + 名称（上）/ token 数 · 比例（下）。一行 8 块，各占 1/8 宽。
fn stat_cell(    label: &str,
    value: u64,
    total: u64,
    color: u32,
    t: &Theme,
    is_total: bool,
) -> gpui::AnyElement {
    div()
        .w(relative(0.125))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(8.))
                .rounded(px(2.))
                .flex_shrink_0()
                .bg(rgb(color)),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(SharedString::from(label.to_string())),
                )
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(if is_total { t.text } else { t.text_muted }))
                                .child(SharedString::from(
                                    crate::services::format::fmt_thousand(value),
                                )),
                        )
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(10.))
                                .text_color(rgb(t.text_dim))
                                .whitespace_nowrap()
                                .child(SharedString::from(fmt_pct(value, total))),
                        ),
                ),
        )
        .into_any_element()
}

/// 100% 宽比例长条：段宽 = 占比，零桶跳过；h6 圆角，段间 1px 缝。
fn ratio_bar(tokens: &[u64; 7], total: u64) -> gpui::AnyElement {
    if total == 0 {
        return div().into_any_element();
    }
    div()
        .w_full()
        .h(px(6.))
        .rounded(px(3.))
        .overflow_hidden()
        .flex()
        .flex_row()
        .children(tokens.iter().enumerate().filter(|(_, v)| **v > 0).map(
            |(i, v)| {
                div()
                    .w(relative(*v as f32 / total as f32))
                    .h_full()
                    .bg(rgb(crate::theme::bucket_color(
                        pi_link::transcript::SYSTEM_BUCKETS[i],
                    )))
                    .when(i + 1 < tokens.len(), |d| d.mr(px(1.)))
                    .into_any_element()
            },
        ))
        .into_any_element()
}

/// 比例文案：>=10% 取整数，<10% 留一位小数。
fn fmt_pct(value: u64, total: u64) -> String {
    if total == 0 {
        return "0%".into();
    }
    let p = value as f64 / total as f64 * 100.0;
    if p >= 9.95 {
        format!("{:.0}%", p)
    } else {
        format!("{:.1}%", p)
    }
}

/// 分析块类别名（i18n）。
fn bucket_label(b: pi_link::transcript::SystemBucket) -> &'static str {
    use pi_link::transcript::SystemBucket::*;
    match b {
        GlobalPrompt => tr("全局提示词"),
        SystemTools => tr("系统工具"),
        Skills => tr("技能"),
        Plugins => tr("插件"),
        Mcp => tr("MCP"),
        ProjectPrompt => tr("项目提示词"),
        Other => tr("其他"),
    }
}

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
    if let Some(values) = schema["enum"].as_array().filter(|_| schema["type"].is_null()) {
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
fn tool_list(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    tools: Option<&[pi_link::transcript::ToolDecl]>,
    t: &Theme,
) -> gpui::AnyElement {
    let selected = selected_tool(chat, tools);
    let mut list = div().id("tool-defs-list").flex_1().min_h_0().overflow_y_scroll();
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
            list = list.child(empty_state(tr("没有启用的工具"), t, true));
        }
        None => {
            list = list.child(empty_state(tr("工具定义尚未加载"), t, true));
        }
    }
    div()
        .w(px(200.))
        .flex_shrink_0()
        .bg(rgb(t.nav))
        // 左导航贴弹窗左下角，同理自己倒左下角
        .rounded_bl(px(10.))
        .border_r_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
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
        .font_family(crate::markdown::MONO_FAMILY)
        .text_color(rgb(if active { t.text } else { t.text_muted }))
        .when(active, |d| {
            d.bg(rgb(t.bg_selected)).font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.tool_sel = Some(name.clone());
                cx.notify();
            });
        })
        .child(SharedString::from(decl.name.clone()))
        .into_any_element()
}

/// 右列详情：描述 / 参数表。pi-web `.tool-definition-*`。
fn tool_detail(
    selected: Option<&pi_link::transcript::ToolDecl>,
    tools: Option<&[pi_link::transcript::ToolDecl]>,
    t: &Theme,
) -> gpui::AnyElement {
    let Some(decl) = selected else {
        let text = if tools.is_some() {
            tr("没有启用的工具")
        } else {
            tr("工具定义尚未加载")
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
        body = body.child(section_label(tr("描述"), "", t)).child(
            div()
                .mb(px(18.))
                .text_size(crate::appearance::ui_size(12.))
                .line_height(relative(1.55))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(decl.description.clone())),
        );
    }
    body = body.child(section_label(
        tr("参数"),
        &tf("{count} 个参数", &[("count", fields.len().to_string())]),
        t,
    ));
    if fields.is_empty() {
        body = body.child(
            div()
                .pb(px(10.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("无参数")),
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
                .font_family(crate::markdown::MONO_FAMILY)
                .text_color(rgb(t.text))
                .child(SharedString::from(f.type_text.clone())),
        );
    if !f.description.is_empty() {
        value = value.child(SharedString::from(f.description.clone()));
    }
    if !f.allowed_values.is_empty() {
        value = value.child(meta_line(tr("可选值"), &f.allowed_values, t));
    }
    if let Some(default) = &f.default_value {
        value = value.child(meta_line(tr("默认值"), default, t));
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
                        .font_family(crate::markdown::MONO_FAMILY)
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
                .font_family(crate::markdown::MONO_FAMILY)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(value.to_string())),
        )
}
