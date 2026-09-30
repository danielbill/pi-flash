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
        .border_color(gpui::rgba(0xafc4ba80))
        // 右缘竖线：与 slp|内容 分隔线贯通到窗口底
        .border_r_1()
        .border_color(gpui::rgba(0xafc4ba99))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .ml(px(16.))
                .children(tabs(active, t, cx)),
        );
    bar = bar.child(div().flex_1());
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
                    .border_color(gpui::rgba(0xafc4ba8c))
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
                                    this.active_panel_tab = Some(ix);
                                }
                            }
                        }
                    }
                    this.persist_ui();
                    cx.notify();
                },
            ))
            .child(crate::ui::icon(icon_name, 16., if on { t.accent } else { t.text_muted }))
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
