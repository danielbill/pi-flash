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
            // 档位表 + 行渲染都收口在 session::menu_rows（设置·子代理页同源
            // 复用，2026-11-07）；这里只分派 on_pick 的写入目标
            let rows: Vec<crate::session::menu_rows::PillRow> = match menu {
                PillMenu::Thinking => {
                    let cur = thinking_override.as_deref().unwrap_or("auto");
                    crate::session::menu_rows::thinking_rows(cur)
                }
                PillMenu::Tools => crate::session::menu_rows::tools_preset_rows(&preset_key),
            };
            let is_thinking = menu == PillMenu::Thinking;
            let items = rows
                .into_iter()
                .map(|row| {
                    let weak_row = weak_menu.clone();
                    let key_for_rpc = row.key.to_string();
                    crate::session::menu_rows::pill_row(
                        &weak_row,
                        move |c, cx| {
                            c.dlg.pill_menu = None;
                            if is_thinking {
                                c.set_thinking_level(&key_for_rpc, cx);
                            } else {
                                c.mc_set_tools_preset(&key_for_rpc, cx);
                            }
                        },
                        row,
                    )
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
                // 统一定位（用户定稿）：与扩展/MCP/模型 picker 同一份
                // `pill_anchor_edges`——按钮水平居中 + 半屏定向（上半向下弹、
                // 下半向上弹），左右钳在视口内。InputPanel 弹出菜单行为完全
                // 一致；卡片自身 min_w 保持内容自适应（ picker 卡是定宽）。
                let vp = window.viewport_size();
                let menu_w = px(320.);
                let pos = crate::session::picker_shell::pill_anchor_edges(
                    self.dlg.pill_anchor,
                    vp,
                    menu_w,
                );
                div()
                    .absolute()
                    .left(pos.left)
                    .when_some(pos.top, |d, top| d.top(top))
                    .when_some(pos.bottom, |d, bottom| d.bottom(bottom))
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
