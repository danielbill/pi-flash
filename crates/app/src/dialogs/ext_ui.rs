//! Extension UI surface (pi-web ExtensionWidgets + blocking dialog
//! parity)：SetWidget 渲染 + 阻塞式 Select/Confirm/Input/Editor 弹窗，
//! 含协议动作 on_ext_ui / ext_respond（P2 自 ext_ui_actions.rs 并入）。
use crate::*;

use gpui::{MouseButton, SharedString, div, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;

/// One extension widget block (mono lines, pi-web widget rendering).
pub(crate) fn render_ext_widget(
    lines: &[String],
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let text: String = lines.join("\n");
    div()
        .w_full()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.tool_bg))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_muted))
        .child(SharedString::from(text))
        .into_any_element()
}

/// Blocking extension UI dialog (select/confirm/input/editor).
pub(crate) fn render_ext_dialog(
    chat: &mut Chat,
    req: pi_link::protocol::ExtensionUiRequest,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    use pi_link::protocol::ExtUiMethod;
    let t = T();
    let weak = weak.clone();
    let (title, body): (String, gpui::AnyElement) = match &req.method {
        ExtUiMethod::Select { title, options } => {
            let weak_opts = weak.clone();
            let opts: Vec<gpui::AnyElement> = options
                .iter()
                .enumerate()
                .map(|(ix, o)| {
                    let weak_o = weak_opts.clone();
                    let v = o.clone();
                    div()
                        .id(SharedString::from(format!("ext-opt-{ix}")))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_selected)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_o.update(cx, |c, cx| {
                                c.ext_respond(Some(v.clone()), None, false, cx)
                            });
                        })
                        .child(SharedString::from(o.clone()))
                        .into_any_element()
                })
                .collect();
            (
                title.clone(),
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(opts)
                    .into_any_element(),
            )
        }
        ExtUiMethod::Confirm { title, message } => {
            let msg: SharedString = message.clone().into();
            (
                title.clone(),
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_muted))
                    .child(msg)
                    .into_any_element(),
            )
        }
        ExtUiMethod::Input { title, .. } | ExtUiMethod::Editor { title, .. } => {
            (title.clone(), chat.aux.ext_input.clone().into_any_element())
        }
        _ => (String::new(), div().into_any_element()),
    };
    let title: SharedString = title.into();
    let is_confirm = matches!(req.method, ExtUiMethod::Confirm { .. });
    let is_select = matches!(req.method, ExtUiMethod::Select { .. });
    let weak_ok = weak.clone();
    // 浮层公共基座：遮挡不穿透 + 点外关闭 + ESC 关闭 —— 两者都等于「取消」
    //（扩展收到 cancelled 响应）
    let weak_cancel = weak.clone();
    let weak_dismiss = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| c.ext_respond(None, None, true, cx));
    };
    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss)
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(460.))
                .bg(rgb(t.bg_panel))
                .border_1()
                .border_color(rgb(t.border))
                .rounded_lg()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .shadow_lg()
                // 卡片内点击不冒泡到背板（否则点卡片任意处都会取消）
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        // §2 卡片标题档（原 text_sm 裸字号）
                        .text_size(crate::appearance::ui_size(14.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(title),
                )
                .children({
                    // 队列尾随数（pi-web waitingExtensionDialogCount parity）：
                    // 阻塞请求按到达序排队，队头之外的还在后面等
                    let n = chat.aux.ext_dialog_waiting.len();
                    (n > 0).then(|| {
                        div()
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_muted))
                            .child(crate::i18n::tf(
                                "ext.dialog_waiting",
                                &[("count", n.to_string())],
                            ))
                    })
                })
                .child(body)
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .children((!is_select).then(|| {
                            div()
                                .id("ext-cancel")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(t.border))
                                // §5.1 BTN_SM 字号（原 text_xs 裸字号）
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_cancel.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            c.ext_respond(None, None, true, cx)
                                        });
                                    }
                                })
                                .child(tr("action.cancel"))
                                .into_any_element()
                        }))
                        .children(is_confirm.then(|| {
                            div()
                                .id("ext-no")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(t.border))
                                // §5.1 BTN_SM 字号（原 text_xs 裸字号）
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_ok.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            c.ext_respond(None, Some(false), false, cx)
                                        });
                                    }
                                })
                                .child(tr("action.no"))
                                .into_any_element()
                        }))
                        .children((!is_select).then(|| {
                            let label = if is_confirm {
                                tr("action.yes")
                            } else {
                                tr("action.submit")
                            };
                            div()
                                .id("ext-ok")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(t.accent))
                                // §5.1 BTN_SM 字号（原 text_xs 裸字号）
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.accent_contrast))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.accent_hover)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_ok.clone();
                                    move |_, _, cx| {
                                        let _ = weak.update(cx, |c, cx| {
                                            if is_confirm {
                                                c.ext_respond(None, Some(true), false, cx);
                                            } else {
                                                let v =
                                                    c.aux.ext_input.read(cx).value().to_string();
                                                c.ext_respond(Some(v), None, false, cx);
                                            }
                                        });
                                    }
                                })
                                .child(label)
                                .into_any_element()
                        })),
                ),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 扩展 UI 协议动作（P2 归并自 ext_ui_actions.rs）
// ---------------------------------------------------------------------------

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
        // 问卷流接管：ask_user_question 的整卷卡（ask_card.rs）。
        // 只拦截阻塞原语；未命中（无进行中问卷 / 题目不匹配）才落到下面的原 match。
        if matches!(
            req.method,
            ExtUiMethod::Select { .. } | ExtUiMethod::Input { .. }
        ) && self.ask_route_request(req.clone(), cx)
        {
            cx.notify();
            return;
        }
        match req.method {
            ExtUiMethod::SetStatus {
                status_key,
                status_text,
            } => {
                match status_text {
                    Some(text) if !text.is_empty() => {
                        if let Some(item) = self
                            .aux
                            .ext_status
                            .iter_mut()
                            .find(|(k, _)| *k == status_key)
                        {
                            item.1 = text;
                        } else {
                            self.aux.ext_status.push((status_key, text));
                        }
                    }
                    _ => self
                        .aux
                        .ext_status
                        .retain(|(k, _): &(String, String)| *k != status_key),
                }
                cx.notify();
            }
            ExtUiMethod::SetWidget {
                widget_key,
                widget_lines,
                placement,
            } => {
                let above = placement.as_deref() != Some("belowEditor");
                match widget_lines {
                    Some(lines) if !lines.is_empty() => {
                        if let Some(w) = self
                            .aux
                            .ext_widgets
                            .iter_mut()
                            .find(|(k, _, _)| *k == widget_key)
                        {
                            w.1 = lines;
                            w.2 = above;
                        } else {
                            self.aux.ext_widgets.push((widget_key, lines, above));
                        }
                    }
                    _ => self.aux.ext_widgets.retain(|(k, _, _)| *k != widget_key),
                }
                cx.notify();
            }
            ExtUiMethod::Notify {
                message,
                notify_type,
            } => {
                let ty = match notify_type.as_deref() {
                    Some("warning") => 1,
                    Some("error") => 2,
                    _ => 0,
                };
                self.aux.ext_notice = Some((message, ty));
                cx.notify();
                // auto-dismiss (pi-web notice toast)
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(4))
                        .await;
                    let _ = this.update(cx, |c, cx| {
                        if c.aux.ext_notice.take().is_some() {
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
                self.composer.input = text;
                cx.notify();
            }
            blocking => {
                let prefill = match &blocking {
                    ExtUiMethod::Editor { prefill, .. } => prefill.clone().unwrap_or_default(),
                    _ => String::new(),
                };
                let placeholder = match &blocking {
                    ExtUiMethod::Input {
                        placeholder: Some(p),
                        ..
                    } => Some(SharedString::from(p.clone())),
                    _ => None,
                };
                self.ext_enqueue(
                    pi_link::protocol::ExtensionUiRequest {
                        id: req.id,
                        method: blocking,
                    },
                    cx,
                );
                cx.notify();
            }
        }
    }

    /// 阻塞弹窗在屏旗标回写 runtime（静默看门狗读，#7）。
    fn ext_sync_dialog_flag(&mut self, cx: &mut Context<Self>) {
        let on = self.aux.ext_dialog.is_some();
        self.rt().update(cx, |r, _| r.ext_dialog_on_screen = on);
    }

    /// 阻塞请求入队（pi-web extension-ui-queue parity）：队头在屏，其余按
    /// 到达序排队。同 id 重发 = 原位替换（SSE 重放语义，RPC 管道不该发生，
    /// 防御性保留）；否则新请求入队，队头空则直接上屏。
    fn ext_enqueue(&mut self, req: pi_link::protocol::ExtensionUiRequest, cx: &mut Context<Self>) {
        if self.aux.ext_dialog.as_ref().is_some_and(|h| h.id == req.id) {
            self.aux.ext_dialog = Some(req);
            return;
        }
        if let Some(w) = self
            .aux
            .ext_dialog_waiting
            .iter_mut()
            .find(|w| w.id == req.id)
        {
            *w = req;
            return;
        }
        if self.aux.ext_dialog.is_none() {
            self.ext_show(req, cx);
        } else {
            self.aux.ext_dialog_waiting.push(req);
        }
        self.ext_sync_dialog_flag(cx);
    }

    /// 队头上屏（含 Input 的输入框预填配置，pi-web 到达时同款）；晋升等待
    /// 项时复用。
    fn ext_show(&mut self, req: pi_link::protocol::ExtensionUiRequest, cx: &mut Context<Self>) {
        if let pi_link::protocol::ExtUiMethod::Input { placeholder, .. } = &req.method {
            let placeholder = placeholder.as_ref().map(|p| SharedString::from(p.clone()));
            let input = self.aux.ext_input.clone();
            input.update(cx, |ti, cx| {
                ti.set_placeholder(placeholder);
                ti.set_value(String::new(), cx);
            });
        } else if let pi_link::protocol::ExtUiMethod::Editor { prefill, .. } = &req.method {
            let prefill = prefill.clone().unwrap_or_default();
            let input = self.aux.ext_input.clone();
            input.update(cx, |ti, cx| {
                ti.set_value(prefill, cx);
            });
        }
        self.aux.ext_dialog = Some(req);
    }

    /// 应答队头并晋升下一个等待项（pi-web respondToExtensionUi：先摘队头
    /// 再发送；队列后续项依次上屏）。每个请求都最终被应答——handler 不会
    /// 因单槽位覆盖而永久阻塞，prompt 回执（相位收场）也就不会丢失（#7）。
    pub(crate) fn ext_respond(
        &mut self,
        value: Option<String>,
        confirmed: Option<bool>,
        cancelled: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(req) = self.aux.ext_dialog.take() {
            self.ext_send(&req, value, confirmed, cancelled, cx);
        }
        if self.aux.ext_dialog.is_none() {
            if let Some(next) = self.aux.ext_dialog_waiting.first().cloned() {
                self.aux.ext_dialog_waiting.remove(0);
                self.ext_show(next, cx);
            }
        }
        self.ext_sync_dialog_flag(cx);
        cx.notify();
    }

    /// 底层应答：桌面端先应答 → 远程端放弃同一请求（060 §8 档 2 不重复消费）
    /// → 发 extension_ui_response。ext_respond 与问卷卡（ask_card）共用。
    pub(crate) fn ext_send(
        &mut self,
        req: &pi_link::protocol::ExtensionUiRequest,
        value: Option<String>,
        confirmed: Option<bool>,
        cancelled: bool,
        cx: &mut Context<Self>,
    ) {
        self.rt()
            .read(cx)
            .rpc(&pi_link::protocol::Command::ExtensionUiResponse {
                id: req.id.clone(),
                value,
                confirmed,
                cancelled,
            });
        cx.notify();
    }
}
