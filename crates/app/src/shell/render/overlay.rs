//! overlay 叠放：settings → dialogs → 各确认浮层 → mj 模型弹窗 → 市场/状态
//! 菜单 → psp 悬浮 → pill/勾选/模型 picker → status toast / ext notice。

use crate::*;

impl Chat {
    pub(super) fn overlay_stack(
        &mut self,
        mut root: gpui::Div,
        weak_for_dialog: gpui::WeakEntity<Chat>,
        pill_menu_el: Option<gpui::AnyElement>,
        t: &'static crate::appearance::theme::Theme,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Chat>,
    ) -> gpui::Div {
        if let Some(panel) = self.dlg.settings.clone() {
            let __t = std::time::Instant::now();
            let data = settings::SettingsFormData::snapshot(panel.read(cx), cx);
            root = root.child(settings::render_settings(
                self,
                &weak_for_dialog,
                &data,
                window,
            ));
            crate::perf_span("frame.settings", __t);
            // 子代理页五菜单（2026-11-07 弹层对齐 inputpanel）：同 composer
            // picker 走统一定位 pill_anchor_edges（按钮居中 + 半屏定向），
            // 挂 settings 之后 = 画在模态之上；menu_layer 遮挡不穿透，点外
            // 收菜单不关模态
            if let Some(el) =
                settings::agents::menus::ag_menus_overlay(self, &weak_for_dialog, &data, window)
            {
                root = root.child(el);
            }
        }
        // 弹窗必须挂在 settings **之后**：后渲染者在上——原顺序（dialogs →
        // settings）让设置面板盖住从设置页打开的 ProviderPicker，遮挡层吃掉
        // 全部点击（"看得见点不着"）。弹窗永远是顶层模态，统一放 settings 上。
        root = dialogs::render_dialogs(root, self, &weak_for_dialog, t, cx);
        // 040 卸载扩展确认浮层（叠在设置弹窗之上）
        if let Some(el) = settings::pi_extensions::remove_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // 090-4 PF 插件市场【已安装】卸载确认浮层（同款机制）
        if let Some(el) = market::pf_installed::remove_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // 042 移除内置商确认浮层（同款机制，叠在设置弹窗之上）
        if let Some(el) = settings::models::remove_provider_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // 042-1 自定义商【删除】确认浮层（删除自定义商 + 其下全部模型）
        if let Some(el) = settings::custom_models::mj_delete_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // 044 子代理【删除】确认浮层（进回收站前确认；2026-11-10 加，同款机制）
        if let Some(el) = settings::agents::agent_delete_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // 042-1 模型编辑器小弹窗（自定义商模型行 /【+模型】打开；叠在设置弹窗
        // 之上，section = mj:m:{prov}:{ix} 即打开态，关闭退回 Provider 详情）
        {
            let __t = std::time::Instant::now();
            let mj_modal = settings::custom_models::mj_model_modal_overlay(self, cx);
            crate::perf_span("frame.mj_modal", __t);
            if let Some(el) = mj_modal {
                root = root.child(el);
            }
        }
        // 040-1 topbar 三横菜单浮层（入口：Pi扩展市场）
        if self.dlg.market_menu.is_some() {
            let vp = window.viewport_size();
            let vp = (f32::from(vp.width), f32::from(vp.height));
            root = root.child(market::menu::menu_layer(self, vp, t, cx));
        }
        // 034 状态带 [...] 管理菜单（插件勾选 = 常驻/隐藏）
        if self.dlg.status_mgr_menu.is_some() {
            let vp = window.viewport_size();
            let vp = (f32::from(vp.width), f32::from(vp.height));
            root = root.child(session::footer_bar::mgr_menu_layer(self, vp, t, cx));
        }
        // psp 悬浮层（tooltip / 详情卡 / 菜单 / 确认）
        root = root.child(function_panel::psp_overlays::psp_overlays(self, window, cx));
        // toolbar pill popup menus
        if let Some(el) = pill_menu_el {
            root = root.child(el);
        }
        // 插件勾选面板：**独立于工具菜单**。千万别再塞回上面那个 if-let ——
        // 插件按钮点击时 pill_menu 是 None，面板会整块不渲染（"点不开"的根因）。
        if let Some(el) = session::pi_extension_picker::view(self, &weak_for_dialog, window) {
            root = root.child(el);
        }
        // MCP 勾选面板（043）：同上，独立挂载，锚点 = 扩展按钮右边的 MCP 按钮
        if let Some(el) = session::mcp_picker::view(self, &weak_for_dialog, window) {
            root = root.child(el);
        }
        // 模型菜单（042）：同上，独立挂载，锚点 = 输入面板的模型按钮
        if let Some(el) = session::model_picker::view(self, &weak_for_dialog, window, cx) {
            root = root.child(el);
        }
        // status toast（v54: statusbar 无状态文本，改瞬时提示）。2026-10-10
        // 定夺：配色随主题（bg_panel+border+text）、不加阴影（2026-11 用户拍
        // 掉 halo，见上方注释），尺寸自适应 max 600×200、超高滚动、自动换行
        if let Some((msg, _)) = &self.dlg.status_toast {
            let text: SharedString = msg.clone().into();
            // 统一弹出定位（2026-10-11）：纵边按锚点象限取自 ui::popup——
            // 顶槽锚点 (y=0) 恒判「上半」向下弹；水平居中是 2026-10-10 定夺
            //（left_0+right_0+justify_center），不走横向象限规则
            let ve = crate::ui::popup::PopupEdges::vertical(
                0.,
                f32::from(window.viewport_size().height),
                44.,
            );
            let mut slot = div().absolute().left_0().right_0().flex().justify_center();
            slot = match (ve.top, ve.bottom) {
                (Some(tp), _) => slot.top(tp),
                (_, Some(bt)) => slot.bottom(bt),
                _ => slot,
            };
            root = root.child(
                slot.child(
                    div().relative().child(
                        div()
                            .id("status-toast")
                            .max_w(px(600.))
                            .max_h(px(200.))
                            .overflow_y_scroll()
                            .px(px(20.))
                            .py(px(7.))
                            .rounded(px(8.))
                            .border_2()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg_panel))
                            .text_size(crate::appearance::ui_size(12.))
                            .text_color(rgb(t.text))
                            // 非 flex 块级容器：文本按容器宽度自动换行
                            //（flex 会让文本 item 按最大内容宽撑开不换行）
                            .child(text),
                    ),
                ),
            );
        }
        // extension notify toast (top-right)。上限与 status toast 对齐
        // （max 600×200、超高滚动、自动换行；2026-10-10 定夺）；不加阴影
        if let Some((message, ty)) = &self.aux.ext_notice {
            let color = match ty {
                1 => 0xfacc15,
                2 => 0xf87171,
                _ => 0x4ade80,
            };
            let text: SharedString = message.clone().into();
            // 统一弹出定位（2026-10-11）：右上槽锚点 (w, 0) → 上半向下弹、
            // 右半向左弹（right 贴边），输出与原 .top(12).right(12) 等价，
            // 但方向由同一算法给出（槽位挪到下方时自动向上弹）
            let vp = window.viewport_size();
            let edge = crate::ui::popup::PopupEdges::around(
                (f32::from(vp.width), 0.),
                (f32::from(vp.width), f32::from(vp.height)),
                12.,
                12.,
            );
            root = root.child(
                edge.apply(div().absolute()).child(
                    div().relative().child(
                        div()
                            .id("ext-notice")
                            .max_w(px(600.))
                            .max_h(px(200.))
                            .overflow_y_scroll()
                            .px(px(20.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(color))
                            .bg(rgb(t.bg_panel))
                            .flex()
                            .items_start()
                            .gap_2()
                            .child(div().size(px(7.)).rounded_full().mt(px(4.)).bg(rgb(color)))
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(12.))
                                    .text_color(rgb(t.text))
                                    .child(text),
                            ),
                    ),
                ),
            );
        }
        root
    }
}
