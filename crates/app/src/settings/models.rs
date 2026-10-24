//! Models tab（042 定稿）：无顶部 banner、无页面 footer。左栏**单一列表**：
//! Provider 行下直接平铺该商已启用的模型（自动展开）；排序三档——默认模型
//! 所在 Provider → 有启用模型的 → 没模型的，档内按显示名。头行
//! 【+添加Provider】开清单弹窗，行 switch 整商启停；模型行与 inputpanel
//! 菜单共用组件（缩进 / 五角星设默认）。右栏详情 = API KEY 表单 + 通路
//! 探查行 + 可用模型区（内置商）/ 自定义编辑器（表单头保存即写盘）。

use super::custom_models::{mj_add_panel, mj_model_editor, mj_provider_editor};
use super::*;

impl Chat {
    pub(crate) fn open_settings(&mut self, tab: u8, cx: &mut Context<Self>) {
        self.reload_settings_panel();
        let section = super::SettingsPanel::prefill_section(self, tab);
        let panel = cx.new(|cx| {
            let mut panel = SettingsPanel::new(cx);
            panel.tab = tab;
            panel.section = section.clone();
            panel
        });
        // 043 MCP 页：打开即回填选中行的编辑表单（与点行同一条链路）
        if tab == super::TAB_MCP {
            let data = self.mcp_backfill_data(&section);
            panel.update(cx, |s, cx| super::mcp::mcp_apply_backfill(s, data, cx));
        }
        self.dlg.settings = Some(panel);
        cx.notify();
    }

    /// Provider ids in available-models display order.
    pub(crate) fn mc_provider_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in self.catalog_for(self.project()) {
            if !out.contains(&m.provider) {
                out.push(m.provider.clone());
            }
        }
        // auth.json 凭据 provider 也算一行（TypeSafe 奇偶校验：有 key、0 个
        // 可用模型也显示，pi-web activeApiKey parity）
        for (p, _) in &self.cfg.mc_creds {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        out
    }

    pub(crate) fn mc_provider_counts(
        &self,
        provider: &str,
        enabled_set: &std::collections::HashSet<String>,
    ) -> (usize, usize) {
        let catalog = self.catalog_for(self.project());
        let models: Vec<&pi_link::protocol::ModelInfo> =
            catalog.iter().filter(|m| m.provider == provider).collect();
        let enabled = models
            .iter()
            .filter(|m| {
                let r = format!("{}/{}", m.provider, m.id);
                enabled_set.contains(&r)
            })
            .count();
        (enabled, models.len())
    }

    /// 042 显示名排序键：`Provider显示名/model` 小写——菜单与「无默认回落
    /// 取第一」用同一把尺（设置页合并列表展示为三档分组，回落仍用本键）。
    pub(crate) fn mc_sort_key(m: &pi_link::protocol::ModelInfo) -> String {
        format!("{}/{}", crate::ui::provider_display_name(&m.provider), m.id).to_lowercase()
    }

    /// Display name of one catalog model ("name", falling back to
    /// `provider/id` for models the catalog hasn't listed).
    pub(crate) fn model_display_name(&self, provider: &str, id: &str) -> String {
        self.catalog_for(self.project())
            .iter()
            .find(|m| m.provider == provider && m.id == id)
            .map(|m| m.label())
            .unwrap_or_else(|| format!("{provider}/{id}"))
    }

    /// 启用模型按显示名序（下列表数据源）。
    pub(crate) fn mc_sorted_enabled(&self) -> Vec<pi_link::protocol::ModelInfo> {
        let all_enabled = self.cfg.mc_state.all_enabled;
        let enabled: std::collections::HashSet<&str> = self
            .cfg
            .mc_state
            .enabled
            .iter()
            .map(|s| s.as_str())
            .collect();
        let mut list: Vec<pi_link::protocol::ModelInfo> = self
            .catalog_for(self.project())
            .iter()
            .filter(|m| {
                all_enabled || {
                    let r = format!("{}/{}", m.provider, m.id);
                    enabled.contains(r.as_str())
                }
            })
            .cloned()
            .collect();
        list.sort_by_key(|m| Self::mc_sort_key(m));
        list
    }

    /// Re-read the model-related defaults a new session starts with. 042：
    /// 默认模型只认 PF 自己的 app-settings.json（五角星写入，不读 pi
    /// settings.json 的同名键——系统 pi / pi-web 的地盘）；思考档仍读 pi。
    /// Cheap (one settings.json read) — also called at startup.
    pub(crate) fn reload_model_defaults(&mut self) {
        let settings_path = pi_link::config::settings_path();
        self.cfg.mc_patterns =
            pi_link::config::read_enabled_models(&settings_path).unwrap_or_else(|_| None);
        let project = self
            .ws
            .cwd
            .as_deref()
            .map(pi_link::config::project_settings_path);
        self.cfg.mc_project_scope = project
            .and_then(|p| pi_link::config::read_enabled_models(&p).unwrap_or_else(|_| None))
            .is_some();
        self.cfg.mc_default_model = services::workspace::default_model_pref();
        self.cfg.mc_default_thinking = pi_link::config::read_default_thinking_level(&settings_path);
        self.cfg.mc_model_thinking = pi_link::config::read_model_thinking_levels(&settings_path);
        self.cfg.mc_state =
            models_config::compute_state(self.cfg.mc_patterns.as_ref(), &self.mc_refs());
    }

    pub(crate) fn new_session_default(&self) -> (Option<(String, String)>, Option<String>) {
        let catalog = self.catalog_for(self.project());
        let in_scope = |m: &pi_link::protocol::ModelInfo| {
            self.cfg.mc_state.all_enabled || {
                let r = format!("{}/{}", m.provider, m.id);
                self.cfg.mc_state.enabled.iter().any(|e| e == &r)
            }
        };
        // pi: patterns that resolve to nothing fall back to every model
        let scope: Vec<&pi_link::protocol::ModelInfo> = {
            let scoped: Vec<_> = catalog.iter().filter(|m| in_scope(m)).collect();
            if scoped.is_empty() {
                catalog.iter().collect()
            } else {
                scoped
            }
        };
        // PF 默认在 scope 内才用；无默认/默认失效 → 显示名序第一个（与
        // 下列表排序同一把尺子）
        let default = self
            .cfg
            .mc_default_model
            .as_ref()
            .and_then(|(p, id)| {
                scope
                    .iter()
                    .find(|m| m.provider == *p && m.id == *id)
                    .map(|m| (m.provider.clone(), m.id.clone()))
            })
            .or_else(|| {
                scope
                    .iter()
                    .min_by_key(|m| Self::mc_sort_key(m))
                    .map(|m| (m.provider.clone(), m.id.clone()))
            });
        let thinking = default
            .as_ref()
            .and_then(|(p, id)| {
                let r = format!("{p}/{id}");
                self.cfg
                    .mc_state
                    .pins
                    .iter()
                    .find(|(pr, _)| pr == &r)
                    .map(|(_, l)| l.clone())
                    .or_else(|| {
                        self.cfg
                            .mc_model_thinking
                            .iter()
                            .find(|(pr, _)| pr == &r)
                            .map(|(_, l)| l.clone())
                    })
            })
            .or_else(|| self.cfg.mc_default_thinking.clone());
        (default, thinking)
    }

    /// 把启动装载的全局态 + 当前项目的项目上下文**安装**进设置页字段
    /// （010-启动.md §1/§5/§7）。纯内存赋值：不再逐项扫盘——skills / packages /
    /// mcp 都来自启动时已装好的 `globals` 与 `project_ctx`；
    /// 切项目命中集合就直接切，未命中时由 `project_ctx_for` 现算一次并纳入。
    pub(crate) fn reload_settings_panel(&mut self) {
        self.reload_model_defaults();
        // 凭据（PF 自有账本 pf-auth.json；auth.json 归 pi，PF 不读）
        self.cfg.mc_creds = pi_link::pf_auth::kinds();
        // 会话预设 `configured` 对应的工具清单（settings.json defaultTools）
        self.cfg.mc_default_tools = self.pool.globals.default_tools.clone();
        self.cfg.mc_pkgs_global = self.pool.globals.packages.clone();
        let ctx = self.project_ctx_now();
        self.cfg.mc_pkgs_project = ctx.packages;
        self.cfg.mc_project_scope = ctx.project_scope;
        // 技能表盘上现读：project_ctx 是启动缓存，本页开关写盘后拿它回读
        // 只会得到旧列表（“点了没反应”）。同时回写缓存，让 composer 的
        // skill 命令跟手。
        self.cfg.mc_skills = self.skills_fresh();
        // 子代理页：盘上现读（~/.pi/agent/agents/*.md + 内置；自定义在前，
        // 同名覆盖内置）——同 skills/MCP，写盘后必须重读才跟手
        self.cfg.mc_agents = pi_link::agents::list_all_agents();
        // 043：MCP 盘上现读——project_ctx 是启动缓存，本页写盘后重读只会
        // 拷到旧列表（040 globals 内存副本同款教训）
        let (mcp_servers, mcp_errors) = pi_link::mcp::load(self.project());
        self.cfg.mcp_servers = mcp_servers;
        self.cfg.mcp_errors = mcp_errors;
        // 自定义 provider 编辑缓冲（PF 私有账本；面板打开/写盘后重建）
        match pi_link::pf_providers::read() {
            Ok(v) => {
                self.cfg.mc_models_json = v;
                self.cfg.mc_mj_error = None;
            }
            Err(e) => {
                self.cfg.mc_models_json = serde_json::json!({});
                self.cfg.mc_mj_error = Some(e);
            }
        }
    }

    pub(crate) fn mc_set_error(&mut self, msg: &str, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, _| s.error = Some(msg.to_string()));
        }
        cx.notify();
    }

    pub(crate) fn mc_clear_error(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
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

    pub(crate) fn mc_toggle_model(&mut self, r: String, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        match models_config::set_models_enabled(
            self.cfg.mc_patterns.as_ref(),
            &self.mc_refs(),
            &[r],
            enable,
        ) {
            Ok(edit) => self.apply_pattern_edit(edit, cx),
            Err(_) => self.mc_set_error(tr("model.keep_one"), cx),
        }
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

    pub(crate) fn mc_save_key(&mut self, provider: String, key: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if key.trim().is_empty() {
            self.mc_set_error(tr("models.api_key_required"), cx);
            return;
        }
        // 051：明文 → 凭据库 + pf-auth.json 引用；高级引用原样；库不可用降级明文
        match pi_link::pf_auth::store_catalog_key(
            &provider,
            key.trim(),
            &pi_link::credentials::KeyringVault,
        ) {
            Ok(pi_link::credentials::StoreMode::File) => {
                self.mc_set_error(tr("conflict.vault_unavailable"), cx);
            }
            Ok(_) => {}
            Err(e) => {
                self.mc_set_error(&crate::i18n::tf("models.failed_to_save", &[("e", e)]), cx);
                return;
            }
        }
        // 换 key 注入的是子进程环境，重启会话才生效（051 §8）
        // 保存 = （重新）注册：从【移除】清单除名，合并列表恢复显示
        let mut removed = crate::services::workspace::removed_providers();
        if removed.iter().any(|p| p == &provider) {
            removed.retain(|p| p != &provider);
            crate::services::workspace::set_removed_providers(&removed);
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
        let Some(target) = pi_link::probe::catalog_target(&provider) else {
            return;
        };
        self.cfg.mc_probe.insert(provider.clone(), McProbe::Running);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let provider_bg = provider.clone();
            let provider_probe = provider.clone();
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let Some(key) = pi_link::probe::catalog_probe_key(
                        &provider_probe,
                        &pi_link::credentials::KeyringVault,
                    ) else {
                        return pi_link::probe::ProbeOutcome::Unreachable {
                            error: crate::i18n::tr("model.key_not_in_vault").to_string(),
                        };
                    };
                    let (url, auth) = target;
                    pi_link::probe::probe(url, auth, &key)
                })
                .await;
            let _ = this.update(cx, |c, cx| {
                // 失败不再落 Done：Failed 态让空目录占位区有据可显（错误条
                // 在保存/获取后短暂可见，占位区常驻到下次探查）
                let state = match &outcome {
                    pi_link::probe::ProbeOutcome::Ok { catalog, .. } if !catalog.is_empty() => {
                        match pi_link::catalog::save_catalog(&provider_bg, catalog) {
                            Ok(()) => {
                                c.refresh_provider_catalog(&provider_bg);
                                McProbe::Done
                            }
                            Err(e) => {
                                c.mc_set_error(&e, cx);
                                McProbe::Failed(e)
                            }
                        }
                    }
                    // 200 但解析不出模型条目：此前完全静默，用户只见"没动静"
                    pi_link::probe::ProbeOutcome::Ok { .. } => {
                        let msg = tr("models.probe_empty_catalog").to_string();
                        c.mc_set_error(&msg, cx);
                        McProbe::Failed(msg)
                    }
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

    /// Whether the provider has an api_key credential（detail 头部状态点用；
    /// 侧栏已改 provider logo，不再用凭据状态点缀行）。
    pub(crate) fn mc_configured(&self, provider: &str) -> bool {
        self.cfg
            .mc_creds
            .iter()
            .any(|(p, k)| p == provider && *k == pi_link::config::CredentialKind::ApiKey)
    }

    /// 选中一个 catalog provider：已配置的把已存 key 解出预填进输入框
    /// （掩码态，眼睛展开）；未配置清空。筛选同步重置（换 provider 重置表单）。
    pub(crate) fn mc_select_provider(&mut self, id: String, cx: &mut Context<Self>) {
        let saved = if self.mc_configured(&id) {
            pi_link::probe::catalog_probe_key(&id, &pi_link::credentials::KeyringVault)
        } else {
            None
        };
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = id;
                s.error = None;
                s.key_input.update(cx, |ti, cx| {
                    ti.set_value(saved.unwrap_or_default(), cx);
                    ti.set_masked(true, cx);
                });
                s.model_filter
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
                cx.notify();
            });
        }
    }

    /// 042 详情头【移除】：确认后删 key + 合并列表除名。仅内置商走这里
    /// （自定义商的【移除】在表单头，删整条账本）。
    pub(crate) fn mc_ask_remove_provider(&mut self, provider: String, cx: &mut Context<Self>) {
        self.cfg.mc_remove_confirm = Some(provider);
        cx.notify();
    }

    pub(crate) fn mc_remove_provider_close(&mut self, ok: bool, cx: &mut Context<Self>) {
        let Some(provider) = self.cfg.mc_remove_confirm.take() else {
            return;
        };
        if !ok {
            cx.notify();
            return;
        }
        if let Err(e) =
            pi_link::pf_auth::delete_catalog_key(&provider, &pi_link::credentials::KeyringVault)
        {
            self.mc_set_error(&e, cx);
            return;
        }
        self.cfg.mc_probe.remove(&provider);
        let mut removed = crate::services::workspace::removed_providers();
        if !removed.contains(&provider) {
            removed.push(provider.clone());
            crate::services::workspace::set_removed_providers(&removed);
        }
        self.reload_settings_panel();
        let first = self.mc_provider_ids().first().cloned().unwrap_or_default();
        self.mc_select_provider(first, cx);
        cx.notify();
    }

    /// 042 详情【获取模型】：与保存后自动探查同一条链路——成功即把响应里
    /// best-effort 解出的目录落盘并刷新内存（详情计数随之更新）。
    pub(crate) fn mc_fetch_models(&mut self, provider: String, cx: &mut Context<Self>) {
        self.mc_probe_start(provider, cx);
    }
}

// ---------------------------------------------------------------------------
// view
// ---------------------------------------------------------------------------

/// The models tab: 左栏上下两等高列表 + 右栏详情（无 banner / footer）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mc_models_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    key_input: &gpui::Entity<crate::TextInput>,
    key_visible: bool,
    model_filter: &gpui::Entity<crate::TextInput>,
    model_filter_value: &str,
    enabled_filter: &gpui::Entity<crate::TextInput>,
    enabled_filter_value: &str,
    mj_name: &gpui::Entity<crate::TextInput>,
    mj_base: &gpui::Entity<crate::TextInput>,
    mj_key: &gpui::Entity<crate::TextInput>,
    mj_id: &gpui::Entity<crate::TextInput>,
    mj_mname: &gpui::Entity<crate::TextInput>,
    mj_ctx: &gpui::Entity<crate::TextInput>,
    mj_api: u8,
    mj_reasoning: bool,
    error: &Option<String>,
    free_only: bool,
) -> gpui::AnyElement {
    let t = T();
    // models.json 不可读：错误条常挂详情区顶部（无页面级 footer 了）
    let mut detail_error = error.clone();
    if let Some(e) = &chat.cfg.mc_mj_error {
        detail_error = Some(crate::i18n::tf(
            "models.json_read_failed",
            &[("e", e.clone())],
        ));
    }

    // ---- split view -------------------------------------------------------
    let selected = if section.is_empty() {
        chat.mc_provider_ids().first().cloned().unwrap_or_default()
    } else {
        section.to_string()
    };
    // 本帧的启用集合（O(1) 行查询；一次构建，上下列表/详情共用）
    let enabled_set: std::collections::HashSet<String> =
        chat.cfg.mc_state.enabled.iter().cloned().collect();
    let sb = mc_models_sidebar(
        chat,
        &weak.clone(),
        &enabled_set,
        enabled_filter,
        enabled_filter_value,
        t,
    );
    let detail = mc_models_detail(
        chat,
        &weak.clone(),
        &selected,
        key_input,
        key_visible,
        model_filter,
        model_filter_value,
        mj_name,
        mj_base,
        mj_key,
        mj_id,
        mj_mname,
        mj_ctx,
        mj_api,
        mj_reasoning,
        &detail_error,
        &enabled_set,
        free_only,
        t,
    );
    div()
        .flex()
        .w_full()
        .h_full()
        .min_h_0()
        .flex_1()
        .min_w_0()
        .child(sb)
        .child(detail)
        .into_any_element()
}

/// 左栏（合并后的单一列表）：Provider 行下直接平铺该商已启用的模型行
/// （常开，无折叠态）。排序三档——默认模型所在 Provider → 有启用模型的
/// → 没模型的，档内按显示名；组内模型按显示名。模型行与 inputpanel 菜单
/// 共用组件（缩进槽 / 五角星）；行 switch 整商启停（决策 4）。过滤框沿用
/// MODEL_FILTER_MIN 规则（不足不渲染、文本不生效），只筛模型行，
/// Provider 行常驻。行不画选中底色（选中反馈在右栏详情）。
fn mc_models_sidebar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    enabled_set: &std::collections::HashSet<String>,
    enabled_filter: &gpui::Entity<crate::TextInput>,
    enabled_filter_value: &str,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    // ---- Provider 全集（内置目录 + models.json 自定义）与计数 -------------
    let custom: Vec<(String, &serde_json::Value)> =
        pi_link::models_json::providers(&chat.cfg.mc_models_json);
    let mut provs: Vec<String> = chat
        .mc_provider_ids()
        .into_iter()
        .filter(|p| !custom.iter().any(|(n, _)| n == p))
        .collect();
    provs.extend(custom.iter().map(|(n, _)| n.clone()));
    let counts: Vec<(usize, usize)> = provs
        .iter()
        .map(|p| chat.mc_provider_counts(p, enabled_set))
        .collect();

    // 三档：0 = 默认模型所在商，1 = 有启用模型，2 = 无；档内显示名字典序
    let default_prov = chat.cfg.mc_default_model.as_ref().map(|(p, _)| p.clone());
    let mut order: Vec<usize> = (0..provs.len()).collect();
    order.sort_by_key(|&ix| {
        let tier = match default_prov.as_deref() {
            Some(dp) if dp == provs[ix] => 0u8,
            _ if counts[ix].0 > 0 => 1,
            _ => 2,
        };
        (
            tier,
            crate::ui::provider_display_name(&provs[ix]).to_lowercase(),
        )
    });

    // ---- 过滤（菜单同规则）：只筛模型行，Provider 行常驻 ------------------
    let total_enabled: usize = counts.iter().map(|(en, _)| *en).sum();
    let show_filter = total_enabled >= crate::session::model_picker::MODEL_FILTER_MIN;
    let q = if show_filter {
        enabled_filter_value.trim().to_lowercase()
    } else {
        String::new()
    };
    let label = |m: &pi_link::protocol::ModelInfo| {
        if m.name.is_empty() {
            m.id.clone()
        } else {
            m.name.clone()
        }
    };
    let hits = |m: &pi_link::protocol::ModelInfo, p: &str| {
        q.is_empty()
            || m.id.to_lowercase().contains(&q)
            || m.name.to_lowercase().contains(&q)
            || crate::ui::provider_display_name(p)
                .to_lowercase()
                .contains(&q)
    };

    // ---- 行序：Provider 行 + 其下启用模型行（组内显示名序）----------------
    enum SbRow {
        /// `prov_rows` 下标
        Prov(usize),
        /// `flat` 下标
        Model(usize),
    }
    struct ProvRow {
        id: String,
        is_custom: bool,
        enabled_n: usize,
        total: usize,
        on: bool,
        switchable: bool,
        show_count: bool,
    }
    let mut prov_rows: Vec<ProvRow> = Vec::new();
    let mut flat: Vec<pi_link::protocol::ModelInfo> = Vec::new();
    let mut rows: Vec<SbRow> = Vec::new();
    let enabled_models = chat.mc_sorted_enabled();
    for &ix in &order {
        let p = &provs[ix];
        let (en, total) = counts[ix];
        let is_custom = custom.iter().any(|(n, _)| n == p);
        let configured = is_custom || chat.mc_configured(p);
        rows.push(SbRow::Prov(prov_rows.len()));
        prov_rows.push(ProvRow {
            id: p.clone(),
            is_custom,
            enabled_n: en,
            total,
            on: chat.cfg.mc_state.all_enabled || en > 0,
            switchable: !chat.cfg.mc_project_scope && total > 0 && configured,
            show_count: !chat.cfg.mc_state.all_enabled,
        });
        let mut ms: Vec<pi_link::protocol::ModelInfo> = enabled_models
            .iter()
            .filter(|m| &m.provider == p && hits(m, p))
            .cloned()
            .collect();
        ms.sort_by_key(|m| label(m).to_lowercase());
        for m in ms {
            flat.push(m);
            rows.push(SbRow::Model(flat.len() - 1));
        }
    }

    // 行距统一：全表（Provider 行 + 模型行）同一行高，严格等距
    let row_h = (f32::from(crate::appearance::ui_size(11.)) + 19.).max(30.);
    let default_ref = chat
        .cfg
        .mc_default_model
        .as_ref()
        .map(|(p, id)| format!("{p}/{id}"));
    let pins = chat.cfg.mc_state.pins.clone();
    let weak_rows = weak.clone();

    let list = vlist(
        "mc-merged-list",
        rows.len(),
        row_h,
        VListHeight::Fill,
        false,
        true,
        tr("model.no_provider_match"),
        None,
        move |ix, _window, _cx| match &rows[ix] {
            SbRow::Prov(pr) => {
                let d = &prov_rows[*pr];
                let pid = d.id.clone();
                let pid_sw = d.id.clone();
                let (is_custom, on) = (d.is_custom, d.on);
                let row_weak = weak_rows.clone();
                div()
                    .id(SharedString::from(format!("mc-side-{}", d.id)))
                    .h(px(row_h))
                    // vlist 行 fit-content：不写 w_full switch 贴着文字
                    .w_full()
                    .px(px(15.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .text_size(crate::appearance::ui_size(12.))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = row_weak.update(cx, |c, cx| {
                            if is_custom {
                                c.mj_select(format!("mj:p:{pid}"), cx);
                            } else {
                                c.mc_select_provider(pid.clone(), cx);
                            }
                        });
                    })
                    .child(crate::ui::provider_icon(&d.id, 16., t.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(
                                crate::ui::provider_display_name(&d.id).to_string(),
                            )),
                    )
                    .children(d.show_count.then(|| {
                        div()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(10.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(format!("{}/{}", d.enabled_n, d.total)))
                            .into_any_element()
                    }))
                    // 整商 switch（决策 4：enabledModels 整商开关，凭据不动）；
                    // 未配置/零模型的内置商恒关禁用，点行进详情配 key
                    .child(config_switch(
                        format!("mc-prov-sw-{}", d.id),
                        &weak_rows,
                        on,
                        !d.switchable,
                        move |c, cx| c.mc_toggle_provider(&pid_sw, !on, cx),
                    ))
                    .into_any_element()
            }
            SbRow::Model(fx) => {
                let m = &flat[*fx];
                let r = format!("{}/{}", m.provider, m.id);
                let is_default = default_ref.as_deref() == Some(r.as_str());
                let pin = pins.iter().find(|(p, _)| p == &r).map(|(_, l)| l.clone());
                crate::session::model_picker::group_model_row(
                    &weak_rows, m, is_default, false, false, false, row_h, 15., pin, None, t,
                )
            }
        },
    );

    sidebar_shell("mc-sidebar")
        .w(px(LIST_W))
        .child(mc_add_action(weak))
        .when(show_filter, |d| {
            d.child(div().px(px(15.)).pb(px(4.)).child(enabled_filter.clone()))
        })
        .child(list)
        .into_any_element()
}

/// 上列表头行【+添加Provider】（原底部钮上移；开两组清单弹窗）。
fn mc_add_action(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let weak = weak.clone();
    div()
        .id("mc-add-provider")
        .flex_shrink_0()
        .px(px(6.))
        .pt(px(8.))
        .pb(px(6.))
        .child(
            div()
                .id("mc-add-provider-go")
                .h(px(30.))
                .px(px(8.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(crate::appearance::ui_size(12.))
                .cursor_pointer()
                .text_color(rgb(t.text_dim))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .child(crate::ui::icon_hover("plus", 13., t.text_dim))
                .child(tr("model.add_provider"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dlg.dialog = Some(c.provider_picker_dialog(cx));
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}

/// 详情页文字按钮：统一 [`ui::button`] 规范样式（BTN_LG 32 高，设置页表单
/// 主按钮档；Danger/Primary/Secondary 三变体），回调签名为 Chat 动作的糖。
#[allow(clippy::too_many_arguments)]
fn detail_button(
    id: &'static str,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    variant: crate::ui::button::BtnVariant,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::AnyElement {
    let weak = weak.clone();
    // 宽度钉死 84：详情页三钮（移除/保存/获取）同宽右对齐（用户定稿）
    let el = crate::ui::button::button_base(id, crate::ui::button::BtnSize::Lg, variant)
        .w(px(84.))
        .child(SharedString::from(label.to_string()));
    if disabled {
        return el.opacity(0.5).into_any_element();
    }
    el.on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
        let _ = weak.update(cx, |c, cx| on_click(c, cx));
    })
    .into_any_element()
}

/// Models detail by section kind (catalog provider / custom editor / add).
#[allow(clippy::too_many_arguments)]
fn mc_models_detail(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    selected: &str,
    key_input: &gpui::Entity<crate::TextInput>,
    key_visible: bool,
    model_filter: &gpui::Entity<crate::TextInput>,
    model_filter_value: &str,
    mj_name: &gpui::Entity<crate::TextInput>,
    mj_base: &gpui::Entity<crate::TextInput>,
    mj_key: &gpui::Entity<crate::TextInput>,
    mj_id: &gpui::Entity<crate::TextInput>,
    mj_mname: &gpui::Entity<crate::TextInput>,
    mj_ctx: &gpui::Entity<crate::TextInput>,
    mj_api: u8,
    mj_reasoning: bool,
    error: &Option<String>,
    enabled_set: &std::collections::HashSet<String>,
    free_only: bool,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    // custom provider / model editors
    if let Some(name) = selected.strip_prefix("mj:p:") {
        return mj_provider_editor(chat, weak, name, mj_name, mj_base, mj_key, mj_api, error, t);
    }
    if let Some(rest) = selected.strip_prefix("mj:m:") {
        if let Some((name, ix)) = rest.rsplit_once(':') {
            if let Ok(ix) = ix.parse::<usize>() {
                return mj_model_editor(
                    weak,
                    name,
                    ix,
                    mj_id,
                    mj_mname,
                    mj_ctx,
                    mj_reasoning,
                    error,
                    t,
                );
            }
        }
    }
    if selected == "__add_provider__" {
        return mj_add_panel(chat, weak, mj_name, mj_base, mj_key, mj_api, t);
    }

    // catalog provider detail（API Key；OAuth 归 pi，PF 只管自有账本）
    let provider = selected.to_string();
    let models: Vec<pi_link::protocol::ModelInfo> = chat
        .catalog_for(chat.project())
        .iter()
        .filter(|m| m.provider == provider)
        .cloned()
        .collect();
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let detail = detail_shell("mc-detail")
        .p(px(15.))
        .when(error.is_some(), |d| {
            d.child(error_note(error.as_deref().unwrap_or("")))
        });

    // header：Provider 名称 +【移除】（删 key + 从合并列表除名，确认后执行）
    let remove_provider = provider.clone();
    let detail = detail.child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_h(px(28.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(
                        crate::ui::provider_display_name(&provider).to_string(),
                    )),
            )
            .child(div().flex_1())
            .child(detail_button(
                "mc-remove",
                weak,
                &tr("action.remove"),
                crate::ui::button::BtnVariant::Danger,
                false,
                move |c, cx| c.mc_ask_remove_provider(remove_provider.clone(), cx),
            )),
    );

    // API Key 段：标题 + 输入（右缘内置眼睛，掩码显隐已存 key）+【保存】
    let weak_eye = weak.clone();
    let save_provider = provider.clone();
    let detail = detail
        .child(section_title("API Key"))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .child(key_input.clone())
                        .child(
                            div()
                                .id("mc-key-eye")
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right(px(8.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.7))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_eye.update(cx, |c, cx| {
                                        if let Some(st) = c.dlg.settings.clone() {
                                            let input = st.read(cx).key_input.clone();
                                            let visible = st.update(cx, |s, cx| {
                                                s.key_visible = !s.key_visible;
                                                cx.notify();
                                                s.key_visible
                                            });
                                            input.update(cx, |ti, cx| ti.set_masked(!visible, cx));
                                        }
                                    });
                                })
                                .child(crate::ui::icon(
                                    if key_visible { "eye-off" } else { "eye" },
                                    14.,
                                    t.text_muted,
                                )),
                        ),
                )
                .child(detail_button(
                    "mc-key-save",
                    weak,
                    &tr("action.save"),
                    crate::ui::button::BtnVariant::Primary,
                    false,
                    move |c, cx| {
                        let key = c
                            .dlg
                            .settings
                            .as_ref()
                            .map(|st| st.read(cx).key_input.clone())
                            .map(|input| input.read(cx).value().to_string())
                            .unwrap_or_default();
                        c.mc_save_key(save_provider.clone(), key, cx);
                    },
                )),
        )
        // API Key 与可用模型之间的分隔线
        .child(
            div()
                .h(px(1.))
                .w_full()
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
        );

    // 可用模型区 + 底部（detail_shell 收尾）
    mc_enabled_section(
        chat,
        weak,
        &provider,
        &models,
        enabled_set,
        model_filter,
        model_filter_value,
        free_only,
        detail,
    )
    .into_any_element()
}

/// 空目录占位：模型数为 0 时模型区不再整体消失（原来连同「获取模型」钮、
/// Running 指示一起被藏掉，保存 key 后"没动静"也无处重试）。表内商给状态
/// 行 + 常驻获取钮；表外商（无列模型端点）说明列表从何而来。
fn mc_empty_models_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    detail: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    if pi_link::probe::catalog_target(provider).is_none() {
        return detail.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(px(10.))
                .child(note(&tr("models.no_auto_fetch"))),
        );
    }
    let status: gpui::AnyElement = match chat.cfg.mc_probe.get(provider) {
        Some(McProbe::Running) => note(&tr("extensions.fetching")),
        Some(McProbe::Failed(e)) => div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(error_note(e))
            .child(note(&tr("models.proxy_hint")))
            .into_any_element(),
        Some(McProbe::Done) => note(&tr("models.probe_empty_catalog")).into_any_element(),
        None if chat.mc_configured(provider) => note(&tr("models.fetch_hint")).into_any_element(),
        None => note(&tr("models.fetch_hint_no_key")).into_any_element(),
    };
    let fetch_provider = provider.to_string();
    let detail = detail.child(
        div().flex().flex_col().gap(px(8.)).pt(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().child(status))
                .child(detail_button(
                    "mc-fetch-empty",
                    weak,
                    &tr("extensions.fetch_models"),
                    crate::ui::button::BtnVariant::Secondary,
                    matches!(chat.cfg.mc_probe.get(provider), Some(McProbe::Running)),
                    move |c, cx| c.mc_fetch_models(fetch_provider.clone(), cx),
                )),
        ),
    );
    detail
}

/// 可用模型区（EnabledModelsSection parity）：标题+计数+只看free开关+批量钮
/// +筛选+行开关。
///
/// 列表用 uniform_list 虚拟化——只构建可见的 ~12 行。overflow div 全量
/// 构建 400 行 × ~12 元素、每敲一字筛选全量重建，是设置页卡顿的来源。
/// 行高固定 36px；启用查询走调用方一次构建的 HashSet（原来是每行线性扫
/// enabled 全表，400×400 次字符串比较/帧）。
#[allow(clippy::too_many_arguments)]
fn mc_enabled_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    models: &[pi_link::protocol::ModelInfo],
    enabled_set: &std::collections::HashSet<String>,
    model_filter: &gpui::Entity<crate::TextInput>,
    model_filter_value: &str,
    free_only: bool,
    detail: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    if models.is_empty() {
        return mc_empty_models_section(chat, weak, provider, detail);
    }
    let t = T();
    let enabled_count = models
        .iter()
        .filter(|m| {
            let r = format!("{}/{}", m.provider, m.id);
            enabled_set.contains(&r)
        })
        .count();
    // 标题行：可用模型 N/总数（左） + 只看free 开关 + rotate-ccw 图标钮（右，
    // 实拉该商目录；开关紧贴刷新钮左侧）
    let running = matches!(chat.cfg.mc_probe.get(provider), Some(McProbe::Running));
    let fetch_provider = provider.to_string();
    let fetch_weak = weak.clone();
    let mut section = div().flex().flex_col().gap(px(8.)).pt(px(10.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(section_title(&crate::i18n::tf(
                "models.available_count",
                &[
                    ("enabled", enabled_count.to_string()),
                    ("total", models.len().to_string()),
                ],
            )))
            .child(div().flex_1())
            .child({
                // 只看free：开 = 列表只显示 free 模型（筛选叠加隐藏的 free
                // 关键字）；状态在 SettingsPanel，切换即整面板重渲染
                let weak_free = weak.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_dim))
                            .child(tr("models.free_only")),
                    )
                    .child(config_switch(
                        "mc-free-only",
                        &weak_free,
                        free_only,
                        false,
                        |c, cx| {
                            if let Some(st) = c.dlg.settings.clone() {
                                st.update(cx, |s, cx| {
                                    s.model_free_only = !s.model_free_only;
                                    cx.notify();
                                });
                            }
                            cx.notify();
                        },
                    ))
            })
            .child({
                // 纯 icon 获取钮：与移除/保存同宽 84 同高 32（右缘对齐）
                let el = crate::ui::button::button_base(
                    "mc-fetch",
                    crate::ui::button::BtnSize::Lg,
                    crate::ui::button::BtnVariant::Secondary,
                )
                .w(px(84.))
                .child(crate::ui::icon("rotate-ccw", 16., t.text));
                if running {
                    el.opacity(0.5).into_any_element()
                } else {
                    let weak = fetch_weak.clone();
                    let fp = fetch_provider.clone();
                    el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak.update(cx, |c, cx| c.mc_fetch_models(fp.clone(), cx));
                    })
                    .into_any_element()
                }
            }),
    );
    if chat.cfg.mc_project_scope {
        section = section.child(note(tr("models.project_override_dotpi")));
    }

    // 筛选（>8 个模型时出现；值经快照传入）。「只看free」开着时在用户
    // 关键字之外叠加隐藏的 free 关键字（AND 语义，不写进输入框）
    let q = model_filter_value.trim().to_lowercase();
    let shown: Vec<pi_link::protocol::ModelInfo> = models
        .iter()
        .filter(|m| {
            let hit = q.is_empty()
                || m.id.to_lowercase().contains(&q)
                || m.name.to_lowercase().contains(&q);
            hit && (!free_only
                || m.id.to_lowercase().contains("free")
                || m.name.to_lowercase().contains("free"))
        })
        .cloned()
        .collect();
    if models.len() > 8 {
        section = section.child(model_filter.clone());
    }
    // pi-web 行规格：min-height 36 + padding 6/9 + 浏览器默认行高（≈1.2，
    // 实际约 37px，开关右对齐、分隔线通栏）。行高不能写死——ui_size 随用户
    // 界面字号缩放，行高从两行缩放后的文本尺寸推导（上下各 6px 呼吸），
    // panel=12 时 ≈38px 与 pi-web 对齐，调大界面字号也不会再挤。
    let row_h = (f32::from(crate::appearance::ui_size(11.))
        + f32::from(crate::appearance::ui_size(10.)))
        * 1.25
        + 12.;
    let all_enabled = chat.cfg.mc_state.all_enabled;
    let project_scope = chat.cfg.mc_project_scope;
    let last_guard = enabled_count == 1;
    let enabled_rows = enabled_set.clone();
    let pins = chat.cfg.mc_state.pins.clone();
    let weak_rows = weak.clone();
    let list = vlist(
        "mc-model-list",
        shown.len(),
        row_h,
        VListHeight::Capped(360.),
        true,
        true,
        tr("models.no_matching_models"),
        None,
        move |ix, _window, _cx| {
            let m = &shown[ix];
            let r = format!("{}/{}", m.provider, m.id);
            let is_enabled = all_enabled || enabled_rows.contains(&r);
            let pin = pins.iter().find(|(p, _)| p == &r).map(|(_, l)| l.clone());
            let last_one = last_guard && is_enabled;
            let ref_click = r.clone();
            div()
                .id(SharedString::from(format!("mc-row-{ix}")))
                // uniform_list 的行是 fit-content（taffy 根节点不拉伸）：
                // 不写 w_full 分隔线只有内容宽、开关贴在文字右边
                .w_full()
                .h(px(row_h))
                // 左 12 内容留白；右 24 = 12 内容 + 12 滚动条让位（不压 switch）
                .pl(px(12.))
                .pr(px(24.))
                .flex()
                .items_center()
                .gap(px(8.))
                .when(ix > 0, |d| d.border_t_1().border_color(rgb(t.border)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(11.))
                                .line_height(gpui::relative(1.25))
                                .text_color(rgb(t.text))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(m.name.clone())),
                        )
                        .child(
                            div()
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(10.))
                                .line_height(gpui::relative(1.25))
                                .text_color(rgb(t.text_dim))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    // click the id to copy the ref
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        ref_click.clone(),
                                    ));
                                })
                                .child(SharedString::from(m.id.clone())),
                        ),
                )
                .children(pin.map(|p| {
                    div()
                        .px(px(4.))
                        .py(px(1.))
                        .rounded(px(3.))
                        .bg(widgets::indigo_bg())
                        .text_size(crate::appearance::ui_size(9.))
                        .text_color(widgets::indigo_fg())
                        .child(SharedString::from(p))
                }))
                .child(config_switch(
                    format!("mc-sw-{ix}"),
                    &weak_rows,
                    is_enabled,
                    last_one || project_scope,
                    {
                        let r = r.clone();
                        move |c, cx| c.mc_toggle_model(r.clone(), !is_enabled, cx)
                    },
                ))
                .into_any_element()
        },
    );
    let section = section.child(list);
    detail.child(section)
}

/// 042 详情头【移除】的确认浮层（与扩展卸载确认同款机制，叠在设置弹窗之上）。
pub(crate) fn remove_provider_confirm_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let provider = chat.cfg.mc_remove_confirm.clone()?;
    let name = crate::ui::provider_display_name(&provider).to_string();
    let weak = cx.entity().downgrade();
    let message = crate::i18n::tf("extensions.remove_key_confirm", &[("name", name)]);
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.mc_remove_provider_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.mc_remove_provider_close(true, cx));
        },
    ))
}

// ---------------------------------------------------------------------------
// 模型引用与 Provider 选择弹窗（P2 归并自 actions_terminal/actions_rename）
// ---------------------------------------------------------------------------

impl Chat {
    /// `provider/modelId` refs of every available model, display order.
    /// Reads the shared catalog of the current project (models_by_cwd).
    pub(crate) fn mc_refs(&self) -> Vec<String> {
        self.catalog_for(self.project())
            .iter()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .collect()
    }

    /// 042 ProviderPicker 弹窗构造：搜索输入每敲一字通知重渲染（清单按值
    /// 过滤在渲染期读取快照）。ESC 交给浮层公共基座，这里只兜输入框内路径。
    /// 打开即确保全量注册表在拉（缓存命中则同步可用）。
    pub(crate) fn provider_picker_dialog(&mut self, cx: &mut Context<Self>) -> Dialog {
        self.ensure_registry(cx);
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("model.search_provider")));
        input.update(cx, |ti, _| {
            ti.set_on_escape(Box::new(move |cx| {
                // same dispatch-lease hazard as submit: closing the dialog
                // drops the dispatching input — defer the dismissal
                let weak = weak_esc.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dlg.dialog = None;
                        cx.notify();
                    });
                });
            }));
        });
        Dialog::ProviderPicker {
            input,
            scroll: gpui::ScrollHandle::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// 目录与可用模型方法（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

impl Chat {
    /// 一个项目的模型目录（010-启动.md §7）：**进程答案优先**（`models_by_cwd`
    /// 有该 cwd 的非空条目 → 用它），否则回落启动装载的全局清单（磁盘 ∪ 自有缓存）——
    /// 于是 lazy draft / 冷启动也有清单，不再出现「选择模型」「no models match」。
    /// 042【移除】墓碑在此统一过滤（composer 模型菜单 / 设置列表 / 新会话
    /// 默认回落 / remote /模型 全走本口）——移除只删 PF 账本与凭据，pi 侧
    /// 目录缓存与 auth.json 不归 PF 管，数据面原样保留、展示层除名。
    pub(crate) fn catalog_for(
        &self,
        cwd: Option<&std::path::Path>,
    ) -> Vec<pi_link::protocol::ModelInfo> {
        let raw = cwd
            .and_then(|c| {
                self.pool
                    .models_by_cwd
                    .get(&c.to_string_lossy().to_string())
            })
            .map(|v| v.as_slice())
            .filter(|v| !v.is_empty())
            .unwrap_or(self.pool.globals.models.as_slice());
        let removed = crate::services::workspace::removed_providers();
        raw.iter()
            .filter(|m| !removed.iter().any(|p| p == &m.provider))
            .cloned()
            .collect()
    }

    /// 「获取模型」落盘后刷新内存目录：该 provider 的条目整体以磁盘现读为
    /// 准（实拉目录替换 models-store 陈旧缓存），其余 provider 不动；其他
    /// 商的进程答案/RPC 清单原样保留。
    pub(crate) fn refresh_provider_catalog(&mut self, provider: &str) {
        let fresh = pi_link::catalog::disk_models();
        let mut g: Vec<_> = self
            .pool
            .globals
            .models
            .iter()
            .filter(|m| m.provider != provider)
            .cloned()
            .collect();
        g.extend(fresh.iter().cloned());
        self.pool.globals.models = pi_link::catalog::dedupe(g);
        let cwd_key = self.project_key();
        let mut m: Vec<_> = self
            .pool
            .models_by_cwd
            .remove(&cwd_key)
            .unwrap_or_default()
            .into_iter()
            .filter(|x| x.provider != provider)
            .collect();
        m.extend(fresh);
        self.pool
            .models_by_cwd
            .insert(cwd_key, pi_link::catalog::dedupe(m));
    }

    /// Picker fallback: catalog entry for the active session's cwd is empty →
    /// ask once through a live runtime of the SAME cwd (the response flows
    /// back via its pump into models_by_cwd). Never spawns a process for
    /// this — model selection must not conjure session processes.
    pub(crate) fn ensure_models_requested(&mut self, cx: &mut Context<Self>) {
        let active = self.rt();
        let (cwd_key, has_session) = {
            let r = active.read(cx);
            (
                r.cwd.to_string_lossy().to_string(),
                r.agent.session.is_some(),
            )
        };
        if self
            .pool
            .models_by_cwd
            .get(&cwd_key)
            .is_some_and(|v| !v.is_empty())
        {
            return;
        }
        if has_session {
            active.update(cx, |r, _| {
                r.rpc(&pi_link::protocol::Command::GetAvailableModels);
            });
            return;
        }
        // active is a draft: any other live runtime of the same cwd answers too
        for (_, rt) in &self.pool.runtimes {
            let r = rt.read(cx);
            if r.cwd.to_string_lossy().to_string() == cwd_key && r.agent.session.is_some() {
                r.rpc(&pi_link::protocol::Command::GetAvailableModels);
                return;
            }
        }
    }
}

/// 042：provider 通路探查状态（保存 key 自动一次 + 详情页【重新探查】）。
/// Failed 带已格式化的失败文案（与详情错误条同文，空目录占位复用）。
#[derive(Debug, Clone)]
pub(crate) enum McProbe {
    Running,
    Done,
    Failed(String),
}
