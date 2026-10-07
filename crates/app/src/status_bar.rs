//! statusbar (v54): 36px，只在面板段（panel-col 内），三个面板 tab
//! （psp / 文件树 / Git）46px 宽 icon 即标签。激活 tab = nav 色连体卡
//! （顶无边、底圆角、accent 色），与 dock 背景连成一体。收起态整个隐藏
//! （panel-col 随 panes_hidden 不渲染）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::DockPanel;
use crate::services::workspace::{save_ui_state, ui_state};
use crate::theme::theme as T;

const HEIGHT: f32 = 36.;

pub(crate) fn control_bar(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();
    let active = chat.dock_panel;

    let mut bar = div()
        .id("status-bar")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(t.chrome))
        .border_t_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x80)))
        // 右缘不画线：slp|内容分隔线 = content-col 的 border_l 一条全高线，
        // statusbar 再画 border_r 会与其相邻成 2px 双线（设计稿 ::before 在
        // 浏览器亚像素下糊成一条，gpui 锐利像素下显形）
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .ml(px(16.))
                .children(tabs(active, t, cx)),
        );
    bar = bar.child(div().flex_1());

    // 060 远程控制：右侧手机图标 → 扫码弹窗（弹窗内容现读 remote.qr，
    // 扫码 worker 的事件由 200ms 泵 drain 后 notify，弹窗自动刷新）
    let wx_on = chat.remote.bound.is_some();
    let wx_color = if wx_on { t.accent } else { t.text_muted };
    bar = bar.child(
        div()
            .id("wx-qr-btn")
            .w(px(46.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, cx.listener(
                move |this, _: &gpui::MouseDownEvent, _w, cx| {
                    // 幂等：已在扫码/已出码时不重复发起
                    this.remote.begin_qr();
                    this.dialog = Some(crate::Dialog::WxQr);
                    cx.notify();
                },
            ))
            .child(crate::ui::icon_hover("smartphone", 16., wx_color)),
    );
    bar
}

fn tabs(
    active: DockPanel,
    t: &'static crate::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> Vec<impl gpui::IntoElement> {
    [
        (DockPanel::Sessions, "messages-square", "psp-tab"),
        (DockPanel::Files, "folder-tree", "files-tab"),
        (DockPanel::Git, "git-branch", "git-tab"),
    ]
    .iter()
    .map(|(panel, icon_name, id)| {
        let on = *panel == active;
        let panel = *panel;
        div()
            .id(SharedString::from(*id))
            .w(px(46.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(if on { rgb(t.accent) } else { rgb(t.text_muted) })
            .when(on, |d| {
                // 连体卡：nav 底色 + 边框（顶无边）+ 底圆角 + 顶穿 1px
                d.bg(rgb(t.nav))
                    .border_1()
                    .border_t_0()
                    .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x8c)))
                    .rounded_bl(px(9.))
                    .rounded_br(px(9.))
                    .mt(px(-1.))
            })
            .when(!on, |d| {
                d.hover(|s| s.bg(rgb(t.bg_hover)).rounded_bl(px(8.)).rounded_br(px(8.)))
            })
            .on_mouse_down(MouseButton::Left, cx.listener(
                move |this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.dock_panel = panel;
                    if panel == DockPanel::Git {
                        this.refresh_git();
                        this.refresh_git_log();
                    }
                    // 绑定关系：会话 tab = 会话内容区（chat）；
                    // files tab = 浏览操作区（终端/文件预览的最后状态）；
                    // git tab = 只切面板
                    if panel == DockPanel::Sessions {
                        this.content_view = crate::ContentView::Chat;
                    } else if panel == DockPanel::Files {
                        // 浏览操作区：有文件 tab 或终端任一存在才切
                        let any = this
                            .panel_tabs
                            .iter()
                            .any(|t| matches!(t, crate::PanelTab::File(_)))
                            || !this.panel_tabs.is_empty();
                        if any {
                            let v = this.browse_last;
                            this.set_content_view(v);
                            // File 视图需要定位到具体 tab
                            if this.content_view == crate::ContentView::File {
                                if let Some(ix) = this.panel_tabs.iter().position(
                                    |t| matches!(t, crate::PanelTab::File(_)),
                                ) {
                                    this.activate_panel_tab(ix, cx);
                                }
                            }
                        }
                    }
                    this.persist_ui();
                    cx.notify();
                },
            ))
            .child(crate::ui::icon_hover(icon_name, 16., if on { t.accent } else { t.text_muted }))
    })
    .collect()
}

/// Persist the current shell layout (panel/width/modes/collapse).
impl Chat {
    pub(crate) fn persist_ui(&mut self) {
        let mut st = ui_state();
        st.panel = self.dock_panel.as_str().to_string();
        st.slp_w = self.slp_w;
        st.panes_hidden = self.panes_hidden;
        st.list_mode = match self.list_mode {
            crate::ListMode::Flat => "flat".to_string(),
            crate::ListMode::Grouped => "grouped".to_string(),
        };
        st.sort_mode = match self.sort_mode {
            crate::SortMode::Manual => "manual".to_string(),
            crate::SortMode::Time => "time".to_string(),
        };
        st.collapsed = self.collapsed_keys.iter().cloned().collect();
        save_ui_state(&st);
    }

    pub(crate) fn toggle_panes(&mut self, cx: &mut gpui::Context<Self>) {
        self.panes_hidden = !self.panes_hidden;
        self.persist_ui();
        cx.notify();
    }
}
