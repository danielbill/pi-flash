//! 组件树解析层：插件 `render(ctx)` 的 JSON → [`ResolvedNode`]。
//! **纯逻辑，可单测，不碰 gpui**（头注释见 mod.rs）；tests 随迁本文件。

use super::*;

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
        pad_x: Option<f32>,
        pad_y: Option<f32>,
        gap: f32,
        align_center: bool,
        /// `justify: "center"`：主轴（纵向）居中——空态提示在面板正中
        justify_center: bool,
        /// 固定宽度（px，可选）：夹住内容列（弹窗内容对齐画布方框这类场景）
        w: Option<f32>,
        /// 卡片能力（可选）：底色 / 圆角 / 1px 描边（通用，非插件特例）
        bg: Option<String>,
        radius: Option<f32>,
        border: Option<String>,
        children: Vec<ResolvedNode>,
    },
    Row {
        gap: f32,
        pad: f32,
        align: String,
        justify_between: bool,
        /// `justify: "end"`：主轴（横向）尾对齐（v3；switch 行居右等）
        justify_end: bool,
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
        /// 契约 v3+：左键直接回环插件命令（与 switch.command 同口径）。
        /// 空 = 只有 action/menu 面（旧包）。
        command: String,
    },
    /// 通用画布（契约 v2.4）：paint list 逐条绝对定位绘制，坐标裁剪在节点框内。
    /// 宿主不解释图形含义（rect 实心方块、text 主题色文字）；未知 op 解析期丢弃。
    Canvas {
        width: f32,
        height: f32,
        paints: Vec<CanvasPaint>,
        /// 契约 v3+：左键点画布 = 回环插件命令（画布内嵌「刷新」这类
        /// 提示本身是按钮）。
        command: String,
    },
    /// 开/关开关（契约 v3）：点击翻转 → onCommand(command, {value: 新布尔})。
    /// 样式全归宿主（§10.4 开关）。
    Switch {
        value: bool,
        label: String,
        tip: String,
        command: String,
        disabled: bool,
    },
    /// 未知组件：占位 + 日志（01 §1）
    Unknown(String),
}

/// canvas paint 项（契约 v2.4）。fill 允许字面色（图形语义 ≠ 主题语义）。
#[derive(Debug, Clone)]
pub(crate) enum CanvasPaint {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: String,
    },
    Text {
        x: f32,
        y: f32,
        size: f32,
        tone: String,
        value: String,
    },
}

/// 单画布 paint 上限（防护：QR ≈600 条足够，防异常树把面板拖死）。
const CANVAS_PAINT_CAP: usize = 4096;

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

    // ---- canvas（契约 v2.4）----

    #[test]
    fn canvas_resolves_paint_list() {
        let v = tree(
            r##"{"type":"canvas","width":392,"height":392,"paint":[
                {"op":"rect","x":0,"y":0,"w":8,"h":8,"fill":"#111827"},
                {"op":"text","x":12,"y":24,"size":11,"tone":"text.dim","value":"hi"},
                {"op":"circle","x":1,"y":1}
            ]}"##,
        );
        match resolve(&v) {
            Some(ResolvedNode::Canvas {
                width,
                height,
                paints,
                command: cmd,
            }) => {
                assert_eq!((width, height), (392., 392.));
                assert_eq!(cmd, "");
                // 未知 op（circle）解析期丢弃，不占位不报错
                assert_eq!(paints.len(), 2);
                assert!(matches!(
                    paints[0],
                    CanvasPaint::Rect { ref fill, .. } if fill == "#111827"
                ));
                assert!(matches!(paints[1], CanvasPaint::Text { ref value, .. } if value == "hi"));
            }
            other => panic!("期望 Canvas，实际 {other:?}"),
        }
    }

    #[test]
    fn canvas_paint_cap_and_defaults() {
        // 超 CAP 截断：4096 + 冗余 → 只取前 4096
        let paints: Vec<String> = (0..CANVAS_PAINT_CAP + 100)
            .map(|_| r##"{"op":"rect","x":1,"y":1,"w":1,"h":1,"fill":"#000000"}"##.to_string())
            .collect();
        let v = tree(&format!(
            r##"{{"type":"canvas","paint":[{}]}}"##,
            paints.join(",")
        ));
        match resolve(&v) {
            Some(ResolvedNode::Canvas { paints, .. }) => {
                assert_eq!(paints.len(), CANVAS_PAINT_CAP)
            }
            other => panic!("期望 Canvas，实际 {other:?}"),
        }
        // 无 paint / 缺宽高 → 兜底空画布不炸
        let v = tree(r#"{"type":"canvas"}"#);
        assert!(matches!(resolve(&v), Some(ResolvedNode::Canvas { .. })));
    }
}
