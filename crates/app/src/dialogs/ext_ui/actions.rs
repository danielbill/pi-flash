//! 扩展 UI 协议动作（P2 归并自 ext_ui_actions.rs，自 ext_ui.rs 外移）：
//! on_ext_ui 分派 / 队列 / 应答 / 发送。

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
            // 未知 method（zi2s.2）：不入阻塞队列、不弹窗——立刻回 cancelled
            // 让扩展 handler 的 await 落地（否则 pi 升级新增 UI 方法即永久
            // 阻塞，#7 同型卡死且界面零痕迹），eprintln 留痕到 app.log。
            ExtUiMethod::Unknown { method } => {
                eprintln!(
                    "[ext-ui] unknown extension ui method={method:?} id={} —— 已回 cancelled",
                    req.id
                );
                self.ext_send(
                    &pi_link::protocol::ExtensionUiRequest {
                        id: req.id.clone(),
                        method: ExtUiMethod::Unknown { method },
                    },
                    None,
                    None,
                    true,
                    cx,
                );
            }
            // 031-1 计划模式广播（必须在通用 SetStatus 之前拦下）：不入
            // 状态带（用户裁决：唯一 UI 指示 = 工具菜单条目，状态栏零额外
            // 信息），只投影到 runtime record；菜单勾选/弹窗数据都读它。
            ExtUiMethod::SetStatus {
                ref status_key,
                ref status_text,
            } if status_key == crate::mode::STATUS_KEY => {
                if let Some(b) = status_text
                    .as_deref()
                    .and_then(crate::mode::parse_plan_broadcast)
                {
                    self.rt().update(cx, |r, _| {
                        // 031-1 同步窗口期（草稿勾选意图未获扩展确认前）：
                        // 与意图相反的广播忽略（session_start 的 false 不能
                        // 覆盖用户勾选），意图由 spawn_attached 的同步命令
                        // 兑现，其广播与意图一致时在此收口。
                        if r.plan_sync_pending && b.enabled != r.plan_enabled {
                            return;
                        }
                        r.plan_sync_pending = false;
                        r.plan_enabled = b.enabled;
                        r.plan_pending = b.pending.filter(|s| !s.is_empty());
                    });
                    cx.notify();
                }
            }
            ExtUiMethod::SetStatus {
                status_key,
                status_text,
            } => {
                // ANSI 剥离（zi2s.8）：rpc-mode 扩展拿真彩色主题，状态文本常
                // 带 SGR 序列，原样渲染是碎渣（pi-web PlainTextTheme parity）
                let status_text = status_text.map(|t| pi_link::protocol::strip_ansi(&t));
                // 034：状态项存**运行时**（per-session），漫染兜底走 retain
                // （空文本 = pi 的 setStatus(key, undefined) = 摘除）。
                match status_text {
                    Some(text) if !text.is_empty() => {
                        self.rt().update(cx, |r, _| {
                            if let Some(item) =
                                r.ext_status.iter_mut().find(|(k, _)| *k == status_key)
                            {
                                item.1 = text;
                            } else {
                                r.ext_status.push((status_key, text));
                            }
                        });
                    }
                    _ => self
                        .rt()
                        .update(cx, |r, _| r.ext_status.retain(|(k, _)| *k != status_key)),
                }
                cx.notify();
            }
            ExtUiMethod::SetWidget {
                widget_key,
                widget_lines,
                placement,
            } => {
                let above = placement.as_deref() != Some("belowEditor");
                // ANSI 剥离同 setStatus（zi2s.8）
                let widget_lines: Option<Vec<String>> = widget_lines.map(|ls| {
                    ls.iter()
                        .map(|l| pi_link::protocol::strip_ansi(l))
                        .collect()
                });
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
            ExtUiMethod::SetTitle { title } => {
                // 窗口标题映射（zi2s.9，pi-web document.title parity）：render
                // 帧 apply-diff 落 OS 标题（on_ext_ui 无 window 访问权）
                if self.aux.ext_title.as_deref() != Some(title.as_str()) {
                    self.aux.ext_title = Some(title);
                    cx.notify();
                }
            }
            ExtUiMethod::SetEditorText { text } => {
                self.composer.input = text;
                cx.notify();
            }
            blocking => {
                // Editor prefill / Input placeholder 由 `ext_show` 落进输入框
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
    }

    /// 队头上屏（含 Input 的输入框预填配置，pi-web 到达时同款）；晋升等待
    /// 项时复用。
    fn ext_show(&mut self, req: pi_link::protocol::ExtensionUiRequest, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        if let pi_link::protocol::ExtUiMethod::Input { placeholder, .. } = &req.method {
            let placeholder = placeholder.as_ref().map(|p| SharedString::from(p.clone()));
            let input = self.aux.ext_input.clone();
            let weak_ok = weak.clone();
            input.update(cx, |ti, cx| {
                ti.set_placeholder(placeholder);
                ti.set_value(String::new(), cx);
                // Enter 提交（zi2s.6，pi-web input 对话框同语义）
                ti.set_on_submit(Box::new(move |v, cx| {
                    let _ = weak_ok.update(cx, |c, cx| {
                        c.ext_respond(Some(v.to_string()), None, false, cx)
                    });
                }));
            });
        } else if let pi_link::protocol::ExtUiMethod::Editor { prefill, .. } = &req.method {
            // 多行编辑器（zi2s.6）：prefill 带真实换行——editor 语义本就多行，
            // 单行 TextInput 渲染遇 `\n` 直接 panic（GPUI text 契约），曾折行
            // 保命；换 multiline 组件后保留原样
            let prefill = prefill.clone().unwrap_or_default();
            let input = self.aux.ext_editor_input.clone();
            let weak_ok = weak.clone();
            input.update(cx, |ti, cx| {
                ti.set_value(prefill, cx);
                // Enter 提交：多行模式组件已自插 "\n" 再发 PressEnter
                //（composer_input 同款剥法）——剥尾部换行后按提交值应答
                ti.set_on_submit(Box::new(move |v, cx| {
                    let submitted = v.strip_suffix('\n').unwrap_or(v);
                    let _ = weak_ok.update(cx, |c, cx| {
                        c.ext_respond(Some(submitted.to_string()), None, false, cx)
                    });
                }));
            });
        }
        self.aux.ext_dialog = Some(req);
        // 键盘高亮从头开始（zi2s.7；晋升等待项时同款）
        self.aux.ext_sel_ix = 0;
        // 新请求从顶部开始看（旧滚动位置不属于这个面板）
        self.aux
            .ext_dialog_info_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        self.aux
            .ext_dialog_op_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
    }

    /// 停止/中止时取消全部在途扩展 UI（pi-web `rpc-manager abort` 的
    /// `extensionUiAbortController.abort()` parity）：阻塞请求逐个回
    /// `cancelled`，pi 侧 select/confirm/input 的 promise 立即落地，扩展
    /// handler 得以继续/退出。否则 handler 永久阻塞在弹窗上，prompt 的
    /// handled 回执永远不来——等待行挂到天荒地老（#7 的另一条尾巴）。
    pub(crate) fn ext_cancel_all(&mut self, cx: &mut Context<Self>) {
        let mut pending: Vec<pi_link::protocol::ExtensionUiRequest> = Vec::new();
        if let Some(req) = self.aux.ext_dialog.take() {
            pending.push(req);
        }
        pending.append(&mut self.aux.ext_dialog_waiting);
        // 问卷卡自身也持有阻塞请求（ask_user_question 整卷流）
        if let Some(card) = self.aux.ask.as_mut() {
            if let Some(req) = card.pending.take() {
                pending.push(req);
            }
        }
        self.aux.ask = None;
        if pending.is_empty() {
            return;
        }
        for req in &pending {
            self.ext_send(req, None, None, true, cx);
        }
        cx.notify();
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
