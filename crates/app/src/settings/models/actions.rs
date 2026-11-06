//! 模型动作：开关（模型/商/默认）、API KEY 保存与通路探查、
//! models_config 模式编辑与修剪、目录拉取入口。

use super::*;

impl Chat {
    /// Apply a pattern edit: persist to settings.json, refresh panel state.
    pub(crate) fn apply_pattern_edit(&mut self, edit: models_config::Edit, cx: &mut Context<Self>) {
        if self.cfg.mc_project_scope {
            self.mc_set_error(tr("models.project_override_dotpi"), cx);
            return;
        }
        if !edit.changed {
            return;
        }
        if let Err(e) = pi_link::config::write_enabled_models(
            &pi_link::config::settings_path(),
            edit.patterns.clone(),
        ) {
            self.mc_set_error(&crate::i18n::tf("settings.write_failed", &[("e", e)]), cx);
            return;
        }
        self.cfg.mc_patterns = edit.patterns;
        self.cfg.mc_state =
            models_config::compute_state(self.cfg.mc_patterns.as_ref(), &self.mc_refs());
        cx.notify();
    }

    /// 删商 / 删模型后清掉 `enabledModels` 里的残留 pattern（此前只清
    /// models.json/auth.json/钥匙库/清单仓，settings.json 里 `ds/*`、
    /// `ds/glm-5.3-flash` 留成死条目）。只删死条目，不经项目覆盖门；没命中
    /// 零写盘。`model_id = None` → 该商名下全部。**盘上现读**再改再写——内存
    /// 副本可能落后于 pi / 别的写者，拿它回写会把别人刚加的条目冲掉。
    pub(crate) fn mc_prune_patterns(
        &mut self,
        provider: &str,
        model_id: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let path = pi_link::config::settings_path();
        let Ok(Some(patterns)) = pi_link::config::read_enabled_models(&path) else {
            return;
        };
        let Some(kept) = models_config::prune_patterns(&patterns, provider, model_id) else {
            return;
        };
        // 只删条目不删键（清空后 pi/PF 同样按「全部可用」回落，语义不变）
        let next = Some(kept);
        if let Err(e) = pi_link::config::write_enabled_models(&path, next.clone()) {
            self.mc_set_error(&crate::i18n::tf("settings.write_failed", &[("e", e)]), cx);
            return;
        }
        self.cfg.mc_patterns = next;
        self.cfg.mc_state =
            models_config::compute_state(self.cfg.mc_patterns.as_ref(), &self.mc_refs());
    }
}
impl Chat {
    pub(crate) fn mc_toggle_model(&mut self, r: String, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        // 2026-11-01 定稿：勾选 = 双写——① enabledModels patterns（pi available
        // 门 + picker 过滤，现状机制）② 自定义商物化/移除 models.json（勾选的
        // 才保存；内置商 pi 静态注册表即持久，不物化）。② 不依赖 ① 的
        // changed（all_enabled 态下勾选 patterns 无变化，但账本仍要物化）
        if let Some((prov, id)) = r.split_once('/') {
            if self.mc_is_custom_provider(prov) {
                if let Err(e) = self.mj_materialize_selection(prov, id, enable) {
                    self.mc_set_error(&crate::i18n::tf("models.failed_to_save", &[("e", e)]), cx);
                    return;
                }
            }
        }
        match models_config::set_models_enabled(
            self.cfg.mc_patterns.as_ref(),
            &self.mc_refs(),
            &[r],
            enable,
        ) {
            Ok(edit) => self.apply_pattern_edit(edit, cx),
            Err(_) => self.mc_set_error(tr("model.keep_one"), cx),
        }
        // 账本变了 → 勾选态/计数重算（apply_pattern_edit 在 changed=false 时
        // 不重算，这里兜底）
        self.cfg.mc_state =
            models_config::compute_state(self.cfg.mc_patterns.as_ref(), &self.mc_refs());
        cx.notify();
    }

    pub(crate) fn mc_toggle_provider(
        &mut self,
        provider: &str,
        enable: bool,
        cx: &mut Context<Self>,
    ) {
        self.mc_clear_error(cx);
        match models_config::set_provider_enabled(
            self.cfg.mc_patterns.as_ref(),
            &self.mc_refs(),
            provider,
            enable,
        ) {
            Ok(edit) => self.apply_pattern_edit(edit, cx),
            Err(_) => self.mc_set_error(tr("model.keep_one"), cx),
        }
    }

    /// 042 五角星：设/取消 PF 默认模型（设置页下列表与 inputpanel 菜单
    /// 共用）。落 app-settings.json，并扇出到全部 runtime——全新 draft
    /// spawn 时以 `--model` 带上（重绑/恢复的会话不动）。
    pub(crate) fn mc_set_default(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        let next = if self.cfg.mc_default_model.as_ref() == Some(&(provider.clone(), id.clone())) {
            None
        } else {
            Some((provider, id))
        };
        services::workspace::set_default_model_pref(next.clone());
        self.cfg.mc_default_model = next;
        for (_, rt) in &self.pool.runtimes {
            rt.update(cx, |r, _| {
                r.default_model = self.cfg.mc_default_model.clone()
            });
        }
        cx.notify();
    }

    /// 内置商详情里的「获取模型 / 刷新目录」UI 开关（2026-11-01 定稿：内置
    /// 商列表来自 pi 自带注册表，无需 key 即罗列全部，刷新钮**隐藏而非删除**
    /// ——代码路径保留，需要时翻开）。
    pub(crate) const BUILTIN_FETCH_UI: bool = false;
}
impl Chat {
    pub(crate) fn mc_save_key(&mut self, provider: String, key: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if key.trim().is_empty() {
            self.mc_set_error(tr("models.api_key_required"), cx);
            return;
        }
        // 042-1：钥匙库写秘密 + auth.json 写「标记条目」（!<helper> get <provider>）。
        // pi 请求时自己执行 helper 取回，盘上永不落明文；换 key 需重开会话
        // 才生效（pi 的命令结果按进程缓存）。
        if let Err(e) = pi_link::auth_json::store_secret(&provider, key.trim()) {
            self.mc_set_error(&crate::i18n::tf("models.failed_to_save", &[("e", e)]), cx);
            return;
        }
        self.reload_settings_panel();
        // 输入框保留刚保存的 key（掩码态），眼睛可复核
        // 042 决策 3：保存后自动探查 + 实拉该商模型目录落盘
        self.mc_probe_start(provider, cx);
        cx.notify();
    }

    /// 042 决策 3：通路探查——后台线程 GET 该 provider 列模型端点，三态
    /// 结果落 `mc_probe`（只驱动 UI 状态点，不回滚保存）。
    pub(crate) fn mc_probe_start(&mut self, provider: String, cx: &mut Context<Self>) {
        let Some(target) = self.builtin_catalog_target(&provider) else {
            return;
        };
        self.cfg.mc_probe.insert(provider.clone(), McProbe::Running);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let provider_probe = provider.clone();
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let Some(key) = pi_link::auth_json::resolve_secret(&provider_probe) else {
                        return pi_link::probe::ProbeOutcome::Unreachable {
                            error: crate::i18n::tr("model.key_not_in_vault").to_string(),
                        };
                    };
                    let (url, auth) = target;
                    pi_link::probe::probe(&url, auth, &key)
                })
                .await;
            let _ = this.update(cx, |c, cx| {
                // 2026-11-01 定稿收缩：probe 只做**保存 key 后的连通检查**，
                // 结果只驱动状态点，不落任何目录（内置商列表 = pi 自带注册表，
                // models.json 零写入）。失败不落 Done：Failed 态有据可显
                let state = match &outcome {
                    pi_link::probe::ProbeOutcome::Ok { .. } => McProbe::Done,
                    pi_link::probe::ProbeOutcome::AuthRejected { status } => {
                        let msg = crate::i18n::tf(
                            "extensions.fetch_key_rejected",
                            &[("s", status.to_string())],
                        );
                        c.mc_set_error(&msg, cx);
                        McProbe::Failed(msg)
                    }
                    pi_link::probe::ProbeOutcome::Unreachable { error } => {
                        let msg =
                            crate::i18n::tf("extensions.fetch_failed", &[("e", error.clone())]);
                        c.mc_set_error(&msg, cx);
                        McProbe::Failed(msg)
                    }
                };
                c.cfg.mc_probe.insert(provider, state);
                cx.notify();
            });
        })
        .detach();
    }
}
impl Chat {
    /// 042 详情【获取模型】：与保存后自动探查同一条链路——成功即把响应里
    /// best-effort 解出的目录落盘并刷新内存（详情计数随之更新）。
    pub(crate) fn mc_fetch_models(&mut self, provider: String, cx: &mut Context<Self>) {
        self.mc_probe_start(provider, cx);
    }
}
