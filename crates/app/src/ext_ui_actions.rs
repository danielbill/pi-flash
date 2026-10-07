//! Extension UI protocol actions (split out of the former settings
//! tools.rs view file): on_ext_ui / ext_respond on Chat.

use crate::*;

impl Chat {

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
            // 桌面端先应答 → 微信端放弃同一请求（060 §8 档 2 不重复消费）
            self.remote.clear_pending(&req.id);
            if let Some(session) = &self.rt().read(cx).agent.session {
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
