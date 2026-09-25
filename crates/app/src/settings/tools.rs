//! Tool-presets tab (defaultTools in settings.json).

use super::*;

    /// Tool presets (pi-web tool-presets.ts) via settings.json `defaultTools`;
    /// picked up by new sessions exactly like the CLI.
impl Chat {
    pub(crate) fn mc_set_tools_preset(&mut self, preset: &str, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let tools: Option<Vec<String>> = match preset {
            // configured sends no override: pi resolves settings defaultTools
            "configured" => None,
            "chat-only" => Some(Vec::new()),
            "read-only" => Some(["read", "grep", "find", "ls"].iter().map(|s| s.to_string()).collect()),
            "default" => Some(["read", "bash", "edit", "write"].iter().map(|s| s.to_string()).collect()),
            "full" => Some(
                ["bash", "read", "edit", "write", "grep", "find", "ls"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            _ => return,
        };
        if let Err(e) = pi_link::config::write_default_tools(&pi_link::config::settings_path(), tools) {
            self.mc_set_error(&crate::i18n::tf("写入 settings.json 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // extension UI protocol (rpc-mode extension_ui_request surface)
    // -----------------------------------------------------------------------

    pub(crate) fn on_ext_ui(
        &mut self,
        req: pi_link::protocol::ExtensionUiRequest,
        cx: &mut Context<Self>,
    ) {
        use pi_link::protocol::ExtUiMethod;
        match req.method {
            ExtUiMethod::SetStatus { status_key, status_text } => {
                match status_text {
                    Some(text) if !text.is_empty() => {
                        if let Some(item) = self.ext_status.iter_mut().find(|(k, _)| *k == status_key) {
                            item.1 = text;
                        } else {
                            self.ext_status.push((status_key, text));
                        }
                    }
                    _ => self.ext_status.retain(|(k, _): &(String, String)| *k != status_key),
                }
                cx.notify();
            }
            ExtUiMethod::SetWidget { widget_key, widget_lines, placement } => {
                let above = placement.as_deref() != Some("belowEditor");
                match widget_lines {
                    Some(lines) if !lines.is_empty() => {
                        if let Some(w) = self.ext_widgets.iter_mut().find(|(k, _, _)| *k == widget_key) {
                            w.1 = lines;
                            w.2 = above;
                        } else {
                            self.ext_widgets.push((widget_key, lines, above));
                        }
                    }
                    _ => self.ext_widgets.retain(|(k, _, _)| *k != widget_key),
                }
                cx.notify();
            }
            ExtUiMethod::Notify { message, notify_type } => {
                let ty = match notify_type.as_deref() {
                    Some("warning") => 1,
                    Some("error") => 2,
                    _ => 0,
                };
                self.ext_notice = Some((message, ty));
                cx.notify();
                // auto-dismiss (pi-web notice toast)
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(4))
                        .await;
                    let _ = this.update(cx, |c, cx| {
                        if c.ext_notice.take().is_some() {
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            ExtUiMethod::SetTitle { .. } => {
                // window title is fixed in this shell (pi-web sets document.title)
            }
            ExtUiMethod::SetEditorText { text } => {
                self.input = text;
                cx.notify();
            }
            blocking => {
                let prefill = match &blocking {
                    ExtUiMethod::Editor { prefill, .. } => prefill.clone().unwrap_or_default(),
                    _ => String::new(),
                };
                let placeholder = match &blocking {
                    ExtUiMethod::Input { placeholder: Some(p), .. } => {
                        Some(SharedString::from(p.clone()))
                    }
                    _ => None,
                };
                let input = self.ext_input.clone();
                input.update(cx, |ti, cx| {
                    ti.set_placeholder(placeholder);
                    ti.set_value(prefill, cx);
                });
                self.ext_dialog = Some(pi_link::protocol::ExtensionUiRequest {
                    id: req.id,
                    method: blocking,
                });
                cx.notify();
            }
        }
    }

    /// Answer the pending blocking extension UI request.
    pub(crate) fn ext_respond(
        &mut self,
        value: Option<String>,
        confirmed: Option<bool>,
        cancelled: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(req) = self.ext_dialog.take() {
            if let Some(session) = &self.session {
                let _ = session.send(&pi_link::protocol::Command::ExtensionUiResponse {
                    id: req.id,
                    value,
                    confirmed,
                    cancelled,
                });
            }
            cx.notify();
        }
    }
}

/// Tools tab: tool presets persisted to settings.json `defaultTools`
/// (pi-web tool-presets.ts; new sessions pick it up like the CLI).
pub(crate) fn mc_tools_view(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> (gpui::AnyElement, gpui::AnyElement) {
    let t = T();
    let current: SharedString = match &chat.mc_default_tools {
        None => tr("未设置（pi 默认解析全部工具）").into(),
        Some(list) if list.is_empty() => tr("[]（无工具）").into(),
        Some(list) => list.join(", ").into(),
    };
    let presets: [(&str, &str, &str); 4] = [
        (tr("全部"), "full", "read, bash, edit, write, grep, find, ls"),
        (tr("默认"), "default", "read, bash, edit, write"),
        (tr("只读"), "read-only", "read, grep, find, ls"),
        (tr("仅聊天"), "chat-only", tr("禁用所有工具")),
    ];
    let active_preset = |list: &Option<Vec<String>>| -> &str {
        match list {
            None => "configured",
            Some(l) if l.is_empty() => "chat-only",
            Some(l) if l == &vec!["read".to_string(), "bash".to_string(), "edit".to_string(), "write".to_string()] => "default",
            Some(l) if l == &vec!["read".to_string(), "grep".to_string(), "find".to_string(), "ls".to_string()] => "read-only",
            Some(l) if l == &vec!["bash".to_string(), "read".to_string(), "edit".to_string(), "write".to_string(), "grep".to_string(), "find".to_string(), "ls".to_string()] => "full",
            _ => "",
        }
    };
    let active = active_preset(&chat.mc_default_tools);
    let mut detail = div()
        .id("mc-detail")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(20.))
        .text_size(px(12.))
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_h(px(28.))
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(tr("工具选择")),
                ),
        )
        .child(
            div()
                .text_size(px(11.))
                .font_family("Consolas")
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(format!("defaultTools: {current}"))),
        );
    for (label, key, tools_text) in presets {
        let weak_row = weak.clone();
        let key = key.to_string();
        let is_active = active == key;
        detail = detail.child(
            div()
                .id(SharedString::from(format!("tool-preset-{key}")))
                .min_h(px(36.))
                .py(px(6.))
                .px(px(9.))
                .rounded(px(6.))
                .border_1()
                .border_color(if is_active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if is_active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| c.mc_set_tools_preset(&key, cx));
                })
                .child(
                    div()
                        .w(px(48.))
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if is_active { rgb(t.text) } else { rgb(t.text_muted) })
                        .child(label),
                )
                .child(
                    div()
                        .font_family("Consolas")
                        .text_size(px(11.))
                        .text_color(rgb(t.text_dim))
                        .child(tools_text),
                ),
        );
    }
    detail = detail.child(
        div()
            .text_size(px(11.))
            .text_color(rgb(t.text_dim))
            .child(tr("写入 ~/.pi/agent/settings.json 的 defaultTools；新会话生效（与 pi CLI --tools 一致）")),
    );
    (div().into_any_element(), detail.into_any_element())
}

