//! Models tab logic: open/reload, error surface, enabledModels edits,
//! credentials (API key / OAuth).

use super::*;

impl Chat {
    pub(crate) fn open_settings(&mut self, tab: u8, cx: &mut Context<Self>) {
        // BISECT A: reload only, no entity
        self.reload_settings_panel();
        let _ = tab;
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
        self.status = format!("pi {} …", args.join(" "));
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

