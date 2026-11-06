//! toolbar pill 弹层：thinking/tools 菜单（composer 胶囊上方锚定）。

use crate::*;

impl Chat {
    /// popup menu overlay anchored above the composer controls row
    pub(super) fn pill_menu_element(
        &self,
        weak: &gpui::WeakEntity<Chat>,
        t: &'static crate::appearance::theme::Theme,
        window: &gpui::Window,
        cx: &gpui::Context<Chat>,
    ) -> Option<gpui::AnyElement> {
        let (thinking_override, preset_key) = {
            let rt = self.rt();
            let r = rt.read(cx);
            (r.thinking_override.clone(), r.tools_preset.clone())
        };
        self.dlg.pill_menu.map(|menu| {
            let weak_menu = weak.clone();
            let rows: Vec<(String, String, bool)> = match menu {
                PillMenu::Thinking => [
                    (
                        "auto",
                        tr("thinking.use_pi_default"),
                        thinking_override.is_none(),
                    ),
                    (
                        "off",
                        tr("thinking.off"),
                        thinking_override.as_deref() == Some("off"),
                    ),
                    (
                        "minimal",
                        tr("thinking.minimal_reasoning"),
                        thinking_override.as_deref() == Some("minimal"),
                    ),
                    (
                        "low",
                        tr("thinking.low_reasoning"),
                        thinking_override.as_deref() == Some("low"),
                    ),
                    (
                        "medium",
                        tr("thinking.medium_reasoning"),
                        thinking_override.as_deref() == Some("medium"),
                    ),
                    (
                        "high",
                        tr("thinking.high_reasoning"),
                        thinking_override.as_deref() == Some("high"),
                    ),
                    (
                        "xhigh",
                        tr("thinking.extra_high_reasoning"),
                        thinking_override.as_deref() == Some("xhigh"),
                    ),
                    (
                        "max",
                        tr("thinking.max_reasoning"),
                        thinking_override.as_deref() == Some("max"),
                    ),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
                PillMenu::Tools => [
                    (
                        "chat-only",
                        tr("thinking.chat_only"),
                        preset_key == "chat-only",
                    ),
                    (
                        "read-only",
                        tr("tools.preset_readonly_count"),
                        preset_key == "read-only",
                    ),
                    (
                        "default",
                        tr("tools.preset_full_count"),
                        preset_key == "default",
                    ),
                    // full：内置 7 件 + 精确集（-ne + 个人扩展/内置扩展，包零注入）
                    (
                        "full",
                        tr("extensions.builtin_all_desc"),
                        preset_key == "full",
                    ),
                    // full+ = full + 本会话选中扩展（默认档；清单按会话保存）
                    (
                        "custom",
                        tr("tools.preset_full_desc"),
                        preset_key == "custom",
                    ),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
            };
            let is_thinking = menu == PillMenu::Thinking;
            let items = rows
                .into_iter()
                .map(|(key, desc, active)| {
                    let weak_row = weak_menu.clone();
                    let key_for_rpc = key.clone();
                    div()
                        .id(SharedString::from(format!(
                            "pm-{}-{}",
                            if is_thinking { "t" } else { "w" },
                            key
                        )))
                        // 行内只有水平 PAD；上下不塞 padding（行高钉
                        // ROW_H，与扩展菜单同款节奏，条目间不松散）
                        .h(px(crate::session::picker_shell::ROW_H))
                        .px(px(crate::session::picker_shell::PAD))
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak_row.update(cx, |c, cx| {
                                c.dlg.pill_menu = None;
                                if is_thinking {
                                    c.set_thinking_level(&key_for_rpc, cx);
                                } else {
                                    c.mc_set_tools_preset(&key_for_rpc, cx);
                                }
                            });
                        })
                        .child(
                            div()
                                .w(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(active.then(|| icon("check", 13., t.accent))),
                        )
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(14.))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(rgb(t.text))
                                // full+ 档显示自定义名（其余档沿用内部键）
                                .child(SharedString::from(if key == "custom" {
                                    tr("extensions.full").to_string()
                                } else {
                                    key
                                })),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text_dim))
                                .child(desc),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            // 浮层公共基座：遮挡不穿透 + 点外关闭（ESC 走 composer 的输入框焦点，
            // 这里不抢焦）
            {
                crate::ui::menu::menu_layer(None, &weak_menu, |c, cx| {
                    if c.dlg.pill_menu.is_some() {
                        c.dlg.pill_menu = None;
                        cx.notify();
                    }
                })
            }
            .child({
                // 统一定位（用户定稿）：与扩展/MCP/模型 picker 同款
                // `pill_anchor_edges`——按钮水平居中、底缘距按钮顶缘
                // 5px（不遮按钮）、左右钳在视口内。tools 不再有右对齐
                // 特例，InputPanel 弹出菜单行为完全一致。
                let vp = window.viewport_size();
                let menu_w = px(320.);
                let (anchor_bottom, anchor_left) = crate::session::picker_shell::pill_anchor_edges(
                    self.dlg.pill_anchor,
                    vp,
                    menu_w,
                );
                div()
                    .absolute()
                    .bottom(anchor_bottom)
                    .left(anchor_left)
                    .min_w(menu_w)
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg))
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    // 内容区外围上下 PAD（同扩展菜单的头/列表外围留白节奏；
                    // 条目之间不塞 padding）
                    .pt(px(crate::session::picker_shell::PAD))
                    .pb(px(crate::session::picker_shell::PAD))
                    // 浮层规则 4：点卡片本身不关（菜单项自己关）
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(items)
            })
            .into_any_element()
        })
    }
}
