//! Subagents tab: profiles, global settings, live runs.

use super::*;

impl Chat {
    // -----------------------------------------------------------------------
    // subagents (pi-web subagents.ts / AgentSessionPanel parity)
    // -----------------------------------------------------------------------

    pub(crate) fn sa_selected(&self, section: &str) -> Option<&pi_link::subagents::SubagentProfile> {
        self.sa_profiles.iter().find(|p| &p.name == section)
    }

    /// Persist the agents global settings (builtInEnabled / maxConcurrent).
    pub(crate) fn sa_save_settings(&mut self, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let max = self
            .settings
            .as_ref()
            .map(|st| st.read(cx).sa_input.clone())
            .and_then(|input| input.read(cx).value().parse::<u32>().ok())
            .unwrap_or(self.sa_settings.max_concurrent)
            .clamp(1, 32);
        self.sa_settings.max_concurrent = max;
        if let Err(e) = pi_link::subagents::write_settings(
            &pi_link::config::agent_dir(),
            &self.sa_settings,
        ) {
            self.mc_set_error(&crate::i18n::tf("写入 agents/settings.json 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// Enable/disable a profile: built-ins go into disabledBuiltIns, file
    /// profiles flip the `enabled` frontmatter key.
    pub(crate) fn sa_toggle_profile(&mut self, name: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(profile) = self.sa_profiles.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        if profile.scope == pi_link::subagents::SubagentScope::Builtin {
            if profile.enabled {
                self.sa_settings.disabled_built_ins.push(profile.name.clone());
            } else {
                self.sa_settings
                    .disabled_built_ins
                    .retain(|n| n != &profile.name);
            }
            if let Err(e) = pi_link::subagents::write_settings(
                &pi_link::config::agent_dir(),
                &self.sa_settings,
            ) {
                self.mc_set_error(&e, cx);
                return;
            }
        } else if let Some(path) = &profile.file_path {
            let mut next = profile.clone();
            next.enabled = !profile.enabled;
            if let Err(e) = pi_link::subagents::write_profile_file(path, &next) {
                self.mc_set_error(&crate::i18n::tf("写入 profile 失败: {e}", &[("e", e)]), cx);
                return;
            }
        }
        self.reload_settings_panel();
        cx.notify();
    }

    pub(crate) fn sa_delete_profile(&mut self, name: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(profile) = self.sa_profiles.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        if let Some(path) = &profile.file_path {
            if let Err(e) = std::fs::remove_file(path) {
                self.mc_set_error(&crate::i18n::tf("删除失败: {e}", &[("e", e.to_string())]), cx);
                return;
            }
        }
        self.reload_settings_panel();
        if let Some(st) = self.settings.clone() {
            let first = self.sa_profiles.first().map(|p| p.name.clone()).unwrap_or_default();
            st.update(cx, |s, _| s.section = first);
        }
        cx.notify();
    }

    /// Run a profile: spawn a child vendored-pi RPC session with the
    /// profile's system prompt / tool allowlist / model / thinking level
    /// (pi-web runs subagents as full child sessions too; the model-facing
    /// Agent tool belongs to pi-web's server layer and is not in the RPC
    /// surface, so pi-flash runs them explicitly from the panel).
    pub(crate) fn sa_run(&mut self, name: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(profile) = self.sa_profiles.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        let mut args: Vec<String> = Vec::new();
        if !profile.system_prompt.trim().is_empty() {
            args.push("--system-prompt".into());
            args.push(profile.system_prompt.trim().to_string());
        }
        if !profile.tools.is_empty() {
            args.push("--tools".into());
            args.push(profile.tools.join(","));
        }
        if let Some(model) = &profile.model {
            if !model.is_empty() {
                args.push("--model".into());
                args.push(model.clone());
            }
        }
        if let Some(thinking) = &profile.thinking {
            if !thinking.is_empty() {
                args.push("--thinking".into());
                args.push(thinking.clone());
            }
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let (session, events) = match pi_link::client::spawn(&self.cwd, &arg_refs) {
            Ok(pair) => pair,
            Err(e) => {
                self.mc_set_error(&crate::i18n::tf("子代理启动失败: {e}", &[("e", e)]), cx);
                return;
            }
        };
        self.sa_run_seq += 1;
        let id = self.sa_run_seq;
        self.sa_runs.push(SubagentRun {
            id,
            profile: profile.name.clone(),
            status: 0,
            last_text: String::new(),
            session: Some(session),
        });
        cx.notify();
        // pump the child session's events until it settles
        let run_id = id;
        cx.spawn(async move |this, cx| {
            let mut events = events;
            while let Some(ev) = events.next().await {
                let settled = matches!(ev, pi_link::protocol::Event::AgentSettled);
                let alive = this
                    .update(cx, |c, cx| c.on_subagent_event(run_id, ev, cx))
                    .is_ok();
                if !alive || settled {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn on_subagent_event(
        &mut self,
        run_id: usize,
        event: pi_link::protocol::Event,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.sa_runs.iter_mut().find(|r| r.id == run_id) else { return };
        match event {
            pi_link::protocol::Event::AgentEnd { .. } => {
                if run.status == 0 {
                    run.status = 1;
                    if let Some(session) = &run.session {
                        let _ = session.send(&pi_link::protocol::Command::GetLastAssistantText);
                    }
                    cx.notify();
                }
            }
            pi_link::protocol::Event::Response { command, success, data, .. }
                if command == "get_last_assistant_text" =>
            {
                if success {
                    run.last_text = data
                        .as_ref()
                        .and_then(|d| d["text"].as_str())
                        .unwrap_or("")
                        .to_string();
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    pub(crate) fn sa_abort_run(&mut self, run_id: usize, cx: &mut Context<Self>) {
        if let Some(run) = self.sa_runs.iter_mut().find(|r| r.id == run_id) {
            if run.status == 0 {
                if let Some(session) = &run.session {
                    let _ = session.send(&pi_link::protocol::Command::Abort);
                }
                run.status = 3;
                cx.notify();
            }
        }
    }
}

/// Subagents tab: runs list + profile sidebar (scope groups) + detail
/// (profile fields, enable switch, run/abort/delete, agents global settings).
pub(crate) fn mc_subagents_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    sa_input: &gpui::Entity<TextInput>,
) -> (gpui::AnyElement, gpui::AnyElement) {
    use pi_link::subagents::SubagentScope;
    let t = T();

    // ---- sidebar: runs first, then profiles by scope ---------------------
    let mut sb = div()
        .id("mc-sidebar")
        .w(px(240.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(t.bg_panel))
        .border_r_1()
        .border_color(rgb(t.border))
        .p(px(6.))
        .pt(px(8.))
        .overflow_y_scroll();
    if !chat.sa_runs.is_empty() {
        sb = sb.child(
            div()
                .px(px(8.))
                .pt(px(6.))
                .pb(px(2.))
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(tr("运行")),
        );
        for run in &chat.sa_runs {
            let active = section == format!("run-{}", run.id);
            let (dot, status_text) = match run.status {
                0 => (t.accent, tr("运行中")),
                1 => (0x4ade80, tr("已完成")),
                2 => (0xf87171, tr("失败")),
                _ => (0xfacc15, tr("已中止")),
            };
            let weak_item = weak.clone();
            let sel = format!("run-{}", run.id);
            sb = sb.child(
                div()
                    .id(SharedString::from(format!("sa-run-{}", run.id)))
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                    .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_item.update(cx, |c, cx| {
                            if let Some(st) = c.settings.clone() {
                                                st.update(cx, |s, cx| {
                                                    s.section = sel.clone();
                                                    s.error = None;
                                                    cx.notify();
                                                });
                                            }
                        });
                    })
                    .child(div().size(px(6.)).rounded_full().flex_shrink_0().bg(rgb(dot)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(run.profile.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(t.text_dim))
                            .child(status_text),
                    ),
            );
        }
    }
    for (label, scope) in [
        (tr("内置"), SubagentScope::Builtin),
        (tr("全局"), SubagentScope::Global),
        (tr("工作区"), SubagentScope::Workspace),
        (tr("项目"), SubagentScope::Project),
    ] {
        let items: Vec<&pi_link::subagents::SubagentProfile> =
            chat.sa_profiles.iter().filter(|p| p.scope == scope).collect();
        if items.is_empty() {
            continue;
        }
        sb = sb.child(
            div()
                .px(px(8.))
                .pt(px(6.))
                .pb(px(2.))
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        );
        for p in items {
            let active = p.name == section;
            let weak_item = weak.clone();
            let name = p.name.clone();
            sb = sb.child(
                div()
                    .id(SharedString::from(format!("sa-prof-{}", p.name)))
                    .h(px(30.))
                    .px(px(8.))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.))
                    .cursor_pointer()
                    .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                    .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                    .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_item.update(cx, |c, cx| {
                            if let Some(st) = c.settings.clone() {
                                                st.update(cx, |s, cx| {
                                                    s.section = name.clone();
                                                    s.error = None;
                                                    cx.notify();
                                                });
                                            }
                        });
                    })
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .flex_shrink_0()
                            .bg(if p.enabled { rgb(0x4ade80) } else { rgb(t.border) }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(p.display_name.clone())),
                    )
                    .child(if p.overridden {
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(t.text_dim))
                            .child(tr("覆盖"))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            );
        }
    }

    // ---- detail ----------------------------------------------------------
    let detail = if let Some(run_str) = section.strip_prefix("run-") {
        // run detail
        let run = run_str.parse::<usize>().ok().and_then(|id| chat.sa_runs.iter().find(|r| r.id == id));
        match run {
            None => div()
                .flex_1()
                .p(px(20.))
                .text_size(px(12.))
                .text_color(rgb(t.text_dim))
                .child(tr("运行已结束"))
                .into_any_element(),
            Some(run) => {
                let weak_abort = weak.clone();
                let abort_id = run.id;
                let (status_text, status_color) = match run.status {
                    0 => (tr("运行中"), t.accent),
                    1 => (tr("已完成"), 0x4ade80),
                    2 => (tr("失败"), 0xf87171),
                    _ => (tr("已中止"), 0xfacc15),
                };
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
                                    .child(SharedString::from(run.profile.clone())),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(status_color))
                                    .child(status_text),
                            ),
                    );
                if run.status == 0 {
                    detail = detail.child(
                        div()
                            .id("sa-abort")
                            .w(px(64.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(0xef4444))
                            .bg(gpui::hsla(0., 0.84, 0.6, 0.06))
                            .text_size(px(11.))
                            .text_color(rgb(0xef4444))
                            .cursor_pointer()
                            .hover(|s| s.bg(gpui::hsla(0., 0.84, 0.6, 0.12)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_abort.update(cx, |c, cx| c.sa_abort_run(abort_id, cx));
                            })
                            .child(tr("中止")),
                    );
                }
                if !run.last_text.is_empty() {
                    detail = detail.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(t.text_muted))
                                    .child(tr("输出")),
                            )
                            .child(
                                div()
                                    .p(px(9.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_panel))
                                    .font_family("Consolas")
                                    .text_size(px(11.))
                                    .text_color(rgb(t.text))
                                    .flex()
                                    .flex_col()
                                    .children(run.last_text.lines().map(|l| {
                                        div().child(SharedString::from(l.to_string()))
                                    })),
                            ),
                    );
                }
                detail.into_any_element()
            }
        }
    } else if let Some(p) = chat.sa_selected(section).cloned() {
        // profile detail
        let builtin = p.scope == SubagentScope::Builtin;
        let weak_sw = weak.clone();
        let weak_del = weak.clone();
        let weak_run = weak.clone();
        let (sw_name, del_name, run_name) = (p.name.clone(), p.name.clone(), p.name.clone());
        let tools_text: SharedString = if p.tools.is_empty() {
            tr("（无）").into()
        } else {
            p.tools.join(", ").into()
        };
        div()
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
                            .child(SharedString::from(p.display_name.clone())),
                    )
                    .child(
                        div()
                            .px(px(5.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(if builtin { gpui::hsla(0., 0., 0.5, 0.12) } else { gpui::hsla(0.63, 0.86, 0.62, 0.12) })
                            .text_size(px(10.))
                            .text_color(rgb(t.text_dim))
                            .child(p.scope.label()),
                    )
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_size(px(10.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(p.name.clone())),
                    ),
            )
            .child(if let Some(path) = &p.file_path {
                div()
                    .font_family("Consolas")
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(path.to_string_lossy().to_string()))
                    .into_any_element()
            } else {
                div().into_any_element()
            })
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(t.text_muted))
                    .child(SharedString::from(p.description.clone())),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(t.text_muted))
                            .child(tr("工具")),
                    )
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_size(px(11.))
                            .text_color(rgb(t.text))
                            .child(tools_text),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(crate::i18n::tf(
                        "模型: {v}",
                        &[("v", p.model.clone().unwrap_or_else(|| tr("继承").into()))],
                    )))
                    .child(SharedString::from(crate::i18n::tf(
                        "思考: {v}",
                        &[("v", p.thinking.clone().unwrap_or_else(|| tr("继承").into()))],
                    )))
                    .child(SharedString::from(crate::i18n::tf(
                        "最大轮数: {v}",
                        &[(
                            "v",
                            p.max_turns.map(|x| x.to_string()).unwrap_or_else(|| "∞".into()),
                        )],
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(36.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(t.text_muted))
                            .child(if p.enabled { tr("已启用") } else { tr("已停用") }),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("sa-switch")
                            .w(px(32.))
                            .h(px(18.))
                            .rounded(px(9.))
                            .border_1()
                            .border_color(if p.enabled { rgb(t.accent) } else { rgb(t.border) })
                            .bg(if p.enabled { rgb(t.accent) } else { rgb(t.bg_selected) })
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .child(
                                div()
                                    .ml(if p.enabled { px(14.) } else { px(2.) })
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(if p.enabled { rgb(t.bg) } else { rgb(t.text_muted) }),
                            )
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_sw.update(cx, |c, cx| {
                                    c.sa_toggle_profile(sw_name.clone(), cx)
                                });
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .id("sa-run")
                            .h(px(28.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(t.accent))
                            .bg(rgb(t.accent))
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.accent_contrast))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.accent_hover)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_run.update(cx, |c, cx| c.sa_run(run_name.clone(), cx));
                            })
                            .child(tr("运行")),
                    )
                    .children((!builtin).then(|| {
                        div()
                            .id("sa-delete")
                            .h(px(28.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(0xef4444))
                            .bg(gpui::hsla(0., 0.84, 0.6, 0.06))
                            .text_size(px(11.))
                            .text_color(rgb(0xef4444))
                            .cursor_pointer()
                            .hover(|s| s.bg(gpui::hsla(0., 0.84, 0.6, 0.12)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_del.update(cx, |c, cx| {
                                    c.sa_delete_profile(del_name.clone(), cx)
                                });
                            })
                            .child(tr("删除"))
                            .into_any_element()
                    })),
            )
            .into_any_element()
    } else {
        // agents global settings (builtInEnabled + maxConcurrent)
        let weak_fea = weak.clone();
        let weak_save = weak.clone();
        let fea_on = chat.sa_settings.builtin_enabled;
        div()
            .id("mc-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .p(px(20.))
            .text_size(px(12.))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(tr("子代理")),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("选择一个子代理查看详情并运行；内置子代理由 agents/settings.json 控制")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(36.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(t.text_muted))
                            .child(tr("内置子代理")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("sa-fea-switch")
                            .w(px(32.))
                            .h(px(18.))
                            .rounded(px(9.))
                            .border_1()
                            .border_color(if fea_on { rgb(t.accent) } else { rgb(t.border) })
                            .bg(if fea_on { rgb(t.accent) } else { rgb(t.bg_selected) })
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .child(
                                div()
                                    .ml(if fea_on { px(14.) } else { px(2.) })
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(if fea_on { rgb(t.bg) } else { rgb(t.text_muted) }),
                            )
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_fea.update(cx, |c, cx| {
                                    c.sa_settings.builtin_enabled = !c.sa_settings.builtin_enabled;
                                    if let Err(e) = pi_link::subagents::write_settings(
                                        &pi_link::config::agent_dir(),
                                        &c.sa_settings,
                                    ) {
                                        c.mc_set_error(&e, cx);
                                        return;
                                    }
                                    c.reload_settings_panel();
                                    cx.notify();
                                });
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(px(36.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(t.text_muted))
                            .child(tr("最大并发 (1-32)")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .w(px(80.))
                            .child(sa_input.clone()),
                    )
                    .child(
                        div()
                            .id("sa-max-save")
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(t.accent))
                            .bg(rgb(t.accent))
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.accent_contrast))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.accent_hover)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_save.update(cx, |c, cx| c.sa_save_settings(cx));
                            })
                            .child(tr("保存")),
                    ),
            )
            .into_any_element()
    };
    (sb.into_any_element(), detail)
}

