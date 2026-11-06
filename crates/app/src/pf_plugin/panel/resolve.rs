//! 组件树解析层·类型（契约 01）：插件 `render(ctx)` 的 JSON 解析目标
//! [`ResolvedNode`] 与 canvas paint [`CanvasPaint`]。解析遍历在
//! [`super::parse`]（纯逻辑，可单测，不碰 gpui；tests 随迁本文件尾）。

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

#[cfg(test)]
mod tests {
    use super::super::parse::CANVAS_PAINT_CAP;
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
