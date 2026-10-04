//! Models tab logic: open/reload, error surface, enabledModels edits,
//! credentials (API key / OAuth).


impl Chat {
    pub(crate) fn open_settings(&mut self, tab: u8, cx: &mut Context<Self>) {
        self.reload_settings_panel();
        let section = match tab {
            0 => self.mc_provider_ids().first().cloned().unwrap_or_default(),
            1 => self
                .mc_skills
                .first()
                .map(|s| s.path.to_string_lossy().to_string())
                .unwrap_or_default(),
            2 => self
                .mc_pkgs_global
                .first()
                .or_else(|| self.mc_pkgs_project.first())
                .map(pi_link::skills::entry_source)
                .unwrap_or_else(|| "__add__".into()),
            4 => self.sa_profiles.first().map(|p| p.name.clone()).unwrap_or_default(),
            _ => String::new(),
        };
        // subagents tab: prefill the max-concurrent input from saved settings
        let max_prefill = self.sa_settings.max_concurrent.to_string();
        let panel = cx.new(|cx| {
            let mut panel = SettingsPanel::new(cx);
            panel.tab = tab;
            panel.section = section;
            panel.sa_input.update(cx, |ti, cx| ti.set_value(max_prefill, cx));
            panel
        });
        self.settings = Some(panel);
        cx.notify();
    }

    /// Provider ids in available-models display order.
    pub(crate) fn mc_provider_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in &self.available_models {
            if !out.contains(&m.provider) {
                out.push(m.provider.clone());
            }
        }
        out
    }

    /// Re-read pi config + resources (called on open and after writes).
    pub(crate) fn reload_settings_panel(&mut self) {
        let settings_path = pi_link::config::settings_path();
        self.mc_patterns =
            pi_link::config::read_enabled_models(&settings_path).unwrap_or_else(|_| None);
        let project = pi_link::config::project_settings_path(&self.cwd);
        self.mc_project_scope =
            pi_link::config::read_enabled_models(&project).unwrap_or_else(|_| None).is_some();
        self.mc_creds = pi_link::config::read_credential_kinds(&pi_link::config::auth_path())
            .unwrap_or_default();
        self.mc_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &self.mc_refs());
        // skills (DefaultResourceLoader dir subset)
        let settings_value =
            pi_link::config::read_json(&settings_path).unwrap_or_else(|_| serde_json::json!({}));
        let agent_dir = pi_link::config::agent_dir();
        let home_agents = agent_dir
            .parent()
            .map(|p| p.join("..").join(".agents").join("skills"))
            .map(|p| p.canonicalize().unwrap_or(p))
            .unwrap_or_else(|| agent_dir.clone());
        self.mc_skills =
            pi_link::skills::discover_skills(&self.cwd, &agent_dir, &home_agents, &settings_value);
        // packages (global + project scopes)
        self.mc_pkgs_global =
            pi_link::config::read_packages(&settings_path).unwrap_or_default();
        self.mc_pkgs_project = pi_link::config::read_packages(&project).unwrap_or_default();
        self.mc_default_tools =
            pi_link::config::read_default_tools(&settings_path).unwrap_or_else(|_| None);
        // subagent profiles + agents settings
        self.sa_settings = pi_link::subagents::read_settings(&agent_dir);
        self.sa_profiles = pi_link::subagents::list_profiles(&self.cwd, &agent_dir, &self.sa_settings);
    }

    pub(crate) fn mc_set_error(&mut self, msg: &str, cx: &mut Context<Self>) {
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, _| s.error = Some(msg.to_string()));
        }
        cx.notify();
    }

    pub(crate) fn mc_clear_error(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.settings.clone() {
            let had = st.read(cx).error.is_some();
            if had {
                st.update(cx, |s, cx| {
                    s.error = None;
                    cx.notify();
                });
            }
        }
    }

    /// Apply a pattern edit: persist to settings.json, refresh panel state.
    pub(crate) fn apply_pattern_edit(&mut self, edit: models_config::Edit, cx: &mut Context<Self>) {
        if self.mc_project_scope {
            self.mc_set_error(tr("项目级 .pi/settings.json 覆盖了 enabledModels，面板只读"), cx);
            return;
        }
        if !edit.changed {
            return;
        }
        if let Err(e) =
            pi_link::config::write_enabled_models(&pi_link::config::settings_path(), edit.patterns.clone())
        {
            self.mc_set_error(&crate::i18n::tf("写入 settings.json 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.mc_patterns = edit.patterns;
        self.mc_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &self.mc_refs());
        cx.notify();
    }

    pub(crate) fn mc_toggle_model(&mut self, r: String, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        match models_config::set_models_enabled(self.mc_patterns.as_ref(), &self.mc_refs(), &[r], enable) {
            Ok(edit) => self.apply_pattern_edit(edit, cx),
            Err(_) => self.mc_set_error(tr("不能停用最后一个启用的模型"), cx),
        }
    }

    pub(crate) fn mc_toggle_provider(&mut self, provider: &str, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        match models_config::set_provider_enabled(self.mc_patterns.as_ref(), &self.mc_refs(), provider, enable) {
            Ok(edit) => self.apply_pattern_edit(edit, cx),
            Err(_) => self.mc_set_error(tr("不能停用最后一个启用的模型"), cx),
        }
    }

    pub(crate) fn mc_save_key(&mut self, provider: String, key: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if key.trim().is_empty() {
            self.mc_set_error(tr("API Key 不能为空"), cx);
            return;
        }
        if let Err(e) = pi_link::config::set_api_key(&pi_link::config::auth_path(), &provider, key.trim()) {
            self.mc_set_error(&crate::i18n::tf("保存失败: {e}", &[("e", e)]), cx);
            return;
        }
        // pi resolves auth.json per request; only a brand-new provider's
        // catalog needs a process restart to appear in available models
        self.reload_settings_panel();
        if let Some(input) = self.settings.as_ref().map(|st| st.read(cx).key_input.clone()) {
            input.update(cx, |ti, cx| ti.set_value(String::new(), cx));
        }
        cx.notify();
    }

    pub(crate) fn mc_delete_key(&mut self, provider: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if let Err(e) = pi_link::config::remove_credential_if_api_key(&pi_link::config::auth_path(), &provider) {
            self.mc_set_error(&e, cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    pub(crate) fn mc_logout(&mut self, provider: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        // OAuth logout: dropping the credential entry (no revocation flow)
        let path = pi_link::config::auth_path();
        let mut value = match pi_link::config::read_json(&path) {
            Ok(v) => v,
            Err(e) => return self.mc_set_error(&e, cx),
        };
        if let Some(obj) = value.as_object_mut() {
            obj.remove(&provider);
        }
        if let Err(e) = pi_link::config::write_json(&path, &value) {
            self.mc_set_error(&e, cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// Whether the provider has an api_key credential (green dot parity).
    pub(crate) fn mc_configured(&self, provider: &str) -> bool {
        self.mc_creds
            .iter()
            .any(|(p, k)| p == provider && *k == pi_link::config::CredentialKind::ApiKey)
    }

    pub(crate) fn mc_oauth(&self, provider: &str) -> bool {
        self.mc_creds
            .iter()
            .any(|(p, k)| p == provider && *k == pi_link::config::CredentialKind::OAuth)
    }

    /// Skills toggle: write `disable-model-invocation` into SKILL.md
    /// (pi-web PATCH /api/skills parity).
    pub(crate) fn mc_cli_op(&mut self, args: Vec<String>, done: String, cx: &mut Context<Self>) {
        let Some(tx) = self.op_tx.clone() else { return };

        let cwd = self.cwd.clone();
        std::thread::spawn(move || {
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = match pi_link::vendor::run_cli(&cwd, &arg_refs) {
                Ok(_) => done,
                Err(e) => crate::i18n::tf(
                    tr("pi {} 失败: {}"),
                    &[
                        ("cmd", args.join(" ")),
                        ("err", e.lines().last().unwrap_or("").to_string()),
                    ],
                ),
            };
            let _ = tx.unbounded_send(msg);
        });
        cx.notify();
    }
}


// Models tab view (split from render_settings: provider groups sidebar +
// per-provider model rows).

use super::*;

/// The models tab (was the inline tab-0 branch of render_settings).
pub(crate) fn mc_models_view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    key_input: &gpui::Entity<crate::TextInput>,
    key_visible: bool,
    error: &Option<String>,
) -> (gpui::AnyElement, gpui::AnyElement) {
    let t = T();
    // provider groups in available-models order
    let provider_ids = chat.mc_provider_ids();
    let selected = if section.is_empty() {
        provider_ids.first().cloned().unwrap_or_default()
    } else {
        section.to_string()
    };

    // ---- sidebar ---------------------------------------------------------
    let sb = mc_models_sidebar(chat, &weak.clone(), selected.clone(), &provider_ids, t);
    let detail = mc_models_detail(chat, &weak.clone(), selected.clone(), &provider_ids, key_input, key_visible, error, t);
    (sb.into_any_element(), detail.into_any_element())
}

/// Models sidebar: provider groups with counts (split from the view).
fn mc_models_sidebar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    selected: String,
    provider_ids: &[String],
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    let sb = div()
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
        .overflow_y_scroll()
        .children(provider_ids.iter().map(|p| {
            let active = *p == selected;
            let models: Vec<&pi_link::protocol::ModelInfo> = chat
                .available_models
                .iter()
                .filter(|m| &m.provider == p)
                .collect();
            let total = models.len();
            let enabled = models
                .iter()
                .filter(|m| {
                    let r = format!("{}/{}", m.provider, m.id);
                    chat.mc_state.enabled.iter().any(|e| e == &r)
                })
                .count();
            let configured = chat.mc_configured(p);
            let weak_item = weak.clone();
            let pid = p.clone();
            div()
                .id(SharedString::from(format!("mc-side-{p}")))
                .h(px(30.))
                .px(px(8.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .cursor_pointer()
                .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| {
                        if let Some(st) = c.settings.clone() {
                            st.update(cx, |s, cx| {
                                s.section = pid.clone();
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
                        .bg(if configured { rgb(0x4ade80) } else { rgb(t.border) }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(p.clone())),
                )
                .child(if enabled < total {
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(px(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!("{enabled}/{total}")))
                        .into_any_element()
                } else {
                    div().into_any_element()
                })
                .into_any_element()
        }));

    sb.into_any_element()
}

/// Models detail: header/status/error + provider model rows (split).
fn mc_models_detail(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    selected: String,
    _provider_ids: &[String],
    key_input: &gpui::Entity<crate::TextInput>,
    key_visible: bool,
    error: &Option<String>,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    // ---- detail pane -----------------------------------------------------
    let models: Vec<pi_link::protocol::ModelInfo> = chat
        .available_models
        .iter()
        .filter(|m| m.provider == selected)
        .cloned()
        .collect();
    let prov_refs: Vec<String> = models
        .iter()
        .map(|m| format!("{}/{}", m.provider, m.id))
        .collect();
    let configured = chat.mc_configured(&selected);
    let oauth = chat.mc_oauth(&selected);
    let detail = div()
        .id("mc-detail")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(20.))
        .text_size(px(12.))
        .flex()
        .flex_col()
        .gap_4();

    // provider header: name + status
    let (status_text, status_color) = if oauth {
        (tr("OAuth 已登录"), 0x4ade80)
    } else if configured {
        (tr("API Key 已配置"), 0x4ade80)
    } else {
        (tr("未配置"), t.text_dim)
    };
    let detail = detail.child(
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
                    .child(SharedString::from(selected.clone())),
            )
            .child(div().size(px(7.)).rounded_full().bg(rgb(status_color)))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(status_text.to_string())),
            ),
    );

    // error box
    let detail = if let Some(err) = &error {
        detail.child(
            div()
                .py(px(7.))
                .px(px(9.))
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(0xef4444))
                .text_size(px(11.))
                .text_color(rgb(0xef4444))
                .child(SharedString::from(err.clone())),
        )
    } else {
        detail
    };

    let mut detail = detail;

    // ---- credential section ---------------------------------------------
    if oauth {
        let weak_logout = weak.clone();
        let logout_provider = selected.clone();
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
                        .child(tr("凭据")),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("登录凭据存储于 ~/.pi/agent/auth.json（与 pi 共用）")),
                )
                .child(
                    div()
                        .id("mc-logout")
                        .h(px(28.))
                        .px(px(10.))
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
                            let _ = weak_logout.update(cx, |c, cx| {
                                c.mc_logout(logout_provider.clone(), cx)
                            });
                        })
                        .child(tr("退出登录")),
                ),
        );
    } else {
        let weak_save = weak.clone();
        let weak_del = weak.clone();
        let save_provider = selected.clone();
        let del_provider = selected.clone();
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
                        .child("API Key"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(key_input.clone()),
                        )
                        .child(
                            div()
                                .id("mc-key-eye")
                                .w(px(30.))
                                .h(px(30.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .text_size(px(11.))
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak_eye = weak.clone();
                                    move |_, _, cx| {
                                        let _ = weak_eye.update(cx, |c, cx| {
                                            if let Some(st) = c.settings.clone() {
                                                let input = st.read(cx).key_input.clone();
                                                let visible = st.update(cx, |s, cx| {
                                                    s.key_visible = !s.key_visible;
                                                    cx.notify();
                                                    s.key_visible
                                                });
                                                input.update(cx, |ti, cx| ti.set_masked(!visible, cx));
                                            }
                                        });
                                    }
                                })
                                .child(if key_visible { tr("隐藏") } else { tr("显示") }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("mc-key-save")
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
                                    let _ = weak_save.update(cx, |c, cx| {
                                        let key = c
                                            .settings
                                            .as_ref()
                                            .map(|st| st.read(cx).key_input.clone())
                                            .and_then(|input| {
                                                Some(input.read(cx).value().to_string())
                                            })
                                            .unwrap_or_default();
                                        c.mc_save_key(save_provider.clone(), key, cx);
                                    });
                                })
                                .child(if configured { tr("更新") } else { tr("保存") }),
                        )
                        .child(if configured {
                            div()
                                .id("mc-key-del")
                                .h(px(28.))
                                .px(px(10.))
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
                                        c.mc_delete_key(del_provider.clone(), cx)
                                    });
                                })
                                .child(tr("删除"))
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("密钥写入 ~/.pi/agent/auth.json（与 pi 共用）；新 provider 的模型需重启 pi-flash 后出现在列表")),
                ),
        );
    }

    // ---- enabled models section ------------------------------------------
    mc_model_rows(chat, weak, selected.clone(), &models, &prov_refs, detail, t).into_any_element()
}
/// Enabled-model rows for the selected provider (split). Appends to and
/// returns the detail pane.
fn mc_model_rows(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    selected: String,
    models: &[pi_link::protocol::ModelInfo],
    prov_refs: &[String],
    detail: gpui::Stateful<gpui::Div>,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    let enabled_count = prov_refs
        .iter()
        .filter(|r| chat.mc_state.enabled.contains(r))
        .count();
    let mut detail = detail;
    if !models.is_empty() {
        let shown: Vec<&pi_link::protocol::ModelInfo> = models.iter().collect();
        let weak_bulk_on = weak.clone();
        let weak_bulk_off = weak.clone();
        let bulk_provider = selected.clone();

        let mut section_col = div().flex().flex_col().gap(px(8.)).pt(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(tr("已启用模型")),
                )
                .child(
                    div()
                        .flex_grow()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(px(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!(
                            "{}/{}",
                            enabled_count,
                            models.len()
                        ))),
                )
                .child(
                    div()
                        .id("mc-bulk-on")
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(px(11.))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, {
                            let bp = bulk_provider.clone();
                            move |_, _, cx| {
                                let _ = weak_bulk_on.update(cx, |c, cx| {
                                    c.mc_toggle_provider(&bp, true, cx)
                                });
                            }
                        })
                        .child(tr("全部启用")),
                )
                .child(
                    div()
                        .id("mc-bulk-off")
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(px(11.))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, {
                            let bp = bulk_provider.clone();
                            move |_, _, cx| {
                                let _ = weak_bulk_off.update(cx, |c, cx| {
                                    c.mc_toggle_provider(&bp, false, cx)
                                });
                            }
                        })
                        .child(tr("全部停用")),
                ),
        );
        if chat.mc_project_scope {
            section_col = section_col.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("项目级 settings.json 覆盖了 enabledModels，此面板只读")),
            );
        }
        // rows
        let weak_rows = weak.clone();
        let enabled_now: Vec<String> = chat.mc_state.enabled.clone();
        let pins: Vec<(String, String)> = chat.mc_state.pins.clone();
        let all_enabled = chat.mc_state.all_enabled;
        let last_one = enabled_count == 1;
        let mut list = div()
            .id("mc-model-list")
            .max_h(px(360.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg_panel))
            .overflow_y_scroll();
        if shown.is_empty() {
            list = list.child(
                div()
                    .p(px(12.))
                    .text_size(px(11.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("没有匹配的模型")),
            );
        }
        for (ix, m) in shown.iter().enumerate() {
            let r = format!("{}/{}", m.provider, m.id);
            let is_enabled = all_enabled || enabled_now.iter().any(|e| e == &r);
            let pin = pins.iter().find(|(p, _)| p == &r).map(|(_, l)| l.clone());
            let row_last = last_one && is_enabled;
            let weak_row = weak_rows.clone();
            let ref_str = r.clone();
            let ref_click = r.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("mc-row-{ix}")))
                    .min_h(px(36.))
                    .py(px(6.))
                    .px(px(9.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(ix > 0, |d| d.border_t_1().border_color(rgb(t.border)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(t.text))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(SharedString::from(m.name.clone())),
                            )
                            .child(
                                div()
                                    .font_family(crate::markdown::MONO_FAMILY)
                                    .text_size(px(10.))
                                    .text_color(rgb(t.text_dim))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .on_mouse_down(MouseButton::Left, {
                                        let r2 = ref_click.clone();
                                        move |_, _, cx| {
                                            // click the id to copy the ref
                                            cx.write_to_clipboard(
                                                gpui::ClipboardItem::new_string(r2.clone()),
                                            );
                                        }
                                    })
                                    .child(SharedString::from(m.id.clone())),
                            ),
                    )
                    .children(pin.map(|p| {
                        div()
                            .px(px(4.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(gpui::hsla(0.63, 0.86, 0.62, 0.12))
                            .text_size(px(9.))
                            .text_color(gpui::hsla(0.63, 0.86, 0.62, 0.8))
                            .child(SharedString::from(p))
                    }))
                    .child({
                        // ConfigSwitch 32×18 (pi-web .config-switch)
                        let knob_left = if is_enabled { px(14.) } else { px(2.) };
                        let on = is_enabled;
                        div()
                            .id(SharedString::from(format!("mc-sw-{ix}")))
                            .w(px(32.))
                            .h(px(18.))
                            .flex_shrink_0()
                            .rounded(px(9.))
                            .border_1()
                            .border_color(if on { rgb(t.accent) } else { rgb(t.border) })
                            .bg(if on { rgb(t.accent) } else { rgb(t.bg_selected) })
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .ml(knob_left)
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(if on { rgb(t.bg) } else { rgb(t.text_muted) }),
                            )
                            .when(!row_last, |sw| {
                                sw.cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        cx.stop_propagation();
                                        let _ = weak_row.update(cx, |c, cx| {
                                            c.mc_toggle_model(ref_str.clone(), !on, cx)
                                        });
                                    })
                            })
                            .when(row_last, |sw| sw.opacity(0.55))
                            .into_any_element()
                    }),
            );
        }
        let _ = weak_rows;
        section_col = section_col.child(list);
         detail = detail.child(section_col);
    }
    detail.into_any_element()
}
