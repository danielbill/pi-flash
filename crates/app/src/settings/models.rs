//! Models tab（042 定稿）：无顶部 banner、无页面 footer。左栏**单一列表**：
//! Provider 行下直接平铺该商已启用的模型（自动展开）；排序三档——默认模型
//! 所在 Provider → 有启用模型的 → 没模型的，档内按显示名。头行
//! 【+添加Provider】开清单弹窗，行 switch 整商启停；模型行与 inputpanel
//! 菜单共用组件（缩进 / 五角星设默认）。右栏详情 = API KEY 表单 + 通路
//! 探查行 + 可用模型区（内置商）/ 自定义编辑器（表单头保存即写盘）。

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
        // 模型页：归属判定（内置 vs 自定义）依赖全量注册表——缓存命中同步
        // 可用，否则后台一次性 dump（幂等；与 provider 弹窗同款）
        if tab == super::TAB_MODELS {
            self.ensure_registry(cx);
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
        let catalog = self.catalog_with_ledger();
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
            .catalog_with_ledger()
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
        // 042-1：凭据清单来自 pi 的 auth.json（PF 标记条目 + 用户明文 + oauth），
        // 不再有 PF 私有账本
        self.cfg.mc_creds = pi_link::auth_json::kinds().unwrap_or_default();
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
        // 自定义 provider 账本**不留副本**：各读点现读 providers.json，
        // 这里只把「读失败」记成错误条（文件缺失是正常空账本）
        self.cfg.mc_mj_error = pi_link::models_json::read().err();
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
                    let Some(key) = pi_link::auth_json::resolve_secret(&provider_probe) else {
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
                        // 不再落 provider-catalog 副本：模型一律以 pi 目录为准
                        c.refresh_provider_catalog(&provider_bg);
                        McProbe::Done
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
        // api_key 与 oauth 都算「该商在用」（列表成员资格 = 有凭据）
        self.cfg.mc_creds.iter().any(|(p, _)| p == provider)
    }

    /// 选中一个 catalog provider：已配置的把已存 key 解出预填进输入框
    /// （掩码态，眼睛展开）；未配置清空。筛选同步重置（换 provider 重置表单）。
    pub(crate) fn mc_select_provider(&mut self, id: String, cx: &mut Context<Self>) {
        let saved = pi_link::auth_json::resolve_secret(&id);
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
        // 042-1【移除】= 断开连接：删掉该商的 api_key 凭据（用户已确认）。
        // 列表成员资格靠真实状态维持（内置商 = 有凭据；自定义商 = 有条目），
        // 凭据没了商家自然从列表消失——不需要任何隐藏清单。oauth 归 pi，不动。
        if let Err(e) = pi_link::auth_json::delete_api_key(&provider) {
            self.mc_set_error(&e, cx);
            return;
        }
        self.cfg.mc_probe.remove(&provider);
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
pub(crate) fn mc_models_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &super::SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let section = d.section.as_str();
    let error = &d.error;
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
        &d.enabled_filter,
        &d.enabled_filter_value,
        t,
    );
    let detail = mc_models_detail(
        chat,
        &weak.clone(),
        &selected,
        d,
        &detail_error,
        &enabled_set,
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
    // ---- Provider 全集 + 归属（042-1 UI：内置在上、自定义在下）-------------
    // 归属判据 = **在不在 pi 的 SDK 注册表**，不是「谁写的文件」：models.json
    // 里出现内置商条目（走代理只改 baseUrl / upsert 模型）是合法覆盖，不能
    // 因此判成自定义商。注册表未就绪时按旧判据过渡（models.json 有条目）。
    let models_doc = pi_link::models_json::read().unwrap_or_else(|_| serde_json::json!({}));
    let custom_entries: Vec<(String, &serde_json::Value)> =
        pi_link::models_json::providers(&models_doc);
    let in_models_json = |p: &str| custom_entries.iter().any(|(n, _)| n == p);
    let is_custom = |p: &str| -> bool {
        let src = match chat.cfg.provider_registry.as_deref() {
            Some(dump) => dump.source(p),
            None => pi_link::registry::source_without_registry(in_models_json(p)),
        };
        src == pi_link::registry::ProviderSource::Custom
    };
    // models.json 里用户手写的 apiKey 也算「已配置」（规则 1：只读不改）
    let key_in_models_json = |p: &str| {
        custom_entries.iter().any(|(n, e)| {
            n == p
                && e.get("apiKey")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.trim().is_empty())
        })
    };
    // 列表 = 用户在用的那一份：内置商按「有凭据」入列（移除凭据即出列），
    // 自定义商按「models.json 有条目」入列（删除条目即出列）。没有隐藏清单。
    let mut provs: Vec<String> = chat
        .mc_provider_ids()
        .into_iter()
        .filter(|p| chat.mc_configured(p) || is_custom(p))
        .collect();
    provs.extend(custom_entries.iter().map(|(n, _)| n.clone()));
    // 去重保序（models.json 的商可能已从目录进过一次）
    let mut seen = std::collections::HashSet::new();
    provs.retain(|p| seen.insert(p.clone()));
    let counts: Vec<(usize, usize)> = provs
        .iter()
        .map(|p| chat.mc_provider_counts(p, enabled_set))
        .collect();

    // 三档：0 = 默认模型所在商，1 = 有启用模型，2 = 无；档内显示名字典序
    let default_prov = chat.cfg.mc_default_model.as_ref().map(|(p, _)| p.clone());
    let tier_name = |ix: usize| {
        let tier = match default_prov.as_deref() {
            Some(dp) if dp == provs[ix] => 0u8,
            _ if counts[ix].0 > 0 => 1,
            _ => 2,
        };
        (
            tier,
            crate::ui::provider_display_name(&provs[ix]).to_lowercase(),
        )
    };
    let mut builtin_order: Vec<usize> = (0..provs.len())
        .filter(|&ix| !is_custom(&provs[ix]))
        .collect();
    builtin_order.sort_by_key(|&ix| tier_name(ix));
    let mut custom_order: Vec<usize> = (0..provs.len())
        .filter(|&ix| is_custom(&provs[ix]))
        .collect();
    custom_order.sort_by_key(|&ix| tier_name(ix));

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
        /// 内置组 / 自定义组之间的分隔线
        Divider,
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
    // 两组之间插一条分隔线（两组都非空才画）
    let show_divider = !builtin_order.is_empty() && !custom_order.is_empty();
    let ordered: Vec<usize> = builtin_order
        .iter()
        .copied()
        .chain(show_divider.then_some(usize::MAX))
        .chain(custom_order.iter().copied())
        .collect();
    for &ix in &ordered {
        if ix == usize::MAX {
            rows.push(SbRow::Divider);
            continue;
        }
        let p = &provs[ix];
        let (en, total) = counts[ix];
        let is_custom = is_custom(p);
        let configured = chat.mc_configured(p) || key_in_models_json(p);
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
            SbRow::Divider => div()
                .h(px(row_h))
                .w_full()
                .px(px(15.))
                .flex()
                .items_center()
                .child(div().h(px(1.)).w_full().bg(rgb(t.border)))
                .into_any_element(),
            SbRow::Model(fx) => {
                let m = &flat[*fx];
                let r = format!("{}/{}", m.provider, m.id);
                let is_default = default_ref.as_deref() == Some(r.as_str());
                let pin = pins.iter().find(|(p, _)| p == &r).map(|(_, l)| l.clone());
                let row = crate::session::model_picker::group_model_row(
                    &weak_rows, m, is_default, false, false, false, row_h, 15., pin, None, t,
                );
                // 042-1：点模型行打开模型详情弹窗（内置模型走 modelOverrides 覆盖；
                // 行内开关/五角星自己 stop_propagation，不会误触）
                let open_weak = weak_rows.clone();
                let (op, oi) = (m.provider.clone(), m.id.clone());
                div()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = open_weak
                            .update(cx, |c, cx| c.mj_open_model(op.clone(), oi.clone(), cx));
                    })
                    .child(row)
                    .into_any_element()
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
pub(crate) fn detail_button(
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
    d: &super::SettingsFormData,
    error: &Option<String>,
    enabled_set: &std::collections::HashSet<String>,
) -> gpui::AnyElement {
    let t = T();
    // 新建 Provider 草稿（042-1）：纯 UI，未落盘；name = 空 → 编辑器无删除钮
    if selected == "mj:new" {
        return super::custom_models::mj_provider_editor(chat, weak, "", d, error, t);
    }
    // custom provider / model editors
    if let Some(name) = selected.strip_prefix("mj:p:") {
        return super::custom_models::mj_provider_editor(chat, weak, name, d, error, t);
    }
    if let Some(rest) = selected.strip_prefix("mj:o:") {
        // 内置模型的覆盖式详情：右栏底图同样是 Provider 详情（section 原文
        // 不能当 provider 名，否则标题会显示成 mj:o:xxx:yyy）
        if let Some((name, _)) = rest.split_once(':') {
            return super::custom_models::mj_provider_editor(chat, weak, name, d, &None, t);
        }
    }
    if let Some(rest) = selected.strip_prefix("mj:m:") {
        // 模型编辑器已改居中 500×500 小弹窗（render.rs 顶层挂
        // mj_model_modal_overlay，section = mj:m:{prov}:{ix} 即打开态）；
        // 右栏底图保持 Provider 详情，错误归弹窗显示
        if let Some((name, _)) = rest.rsplit_once(':') {
            return super::custom_models::mj_provider_editor(chat, weak, name, d, &None, t);
        }
    }
    // catalog provider detail（API Key；OAuth 归 pi，PF 只管自有账本）
    let provider = selected.to_string();
    // 042-1：可用模型 = 唯一目录（models.json + models-store + 进程答案），
    // 不再有 registry 静态目录保底
    let models: Vec<pi_link::protocol::ModelInfo> = chat
        .catalog_for(chat.project())
        .into_iter()
        .filter(|m| m.provider == provider)
        .collect();
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let detail = detail_shell("mc-detail").p(px(15.));

    // header：Provider 名称（左）+ 操作钮（右，同一行）：[移除][保存]，间隔 10px。
    // 保存钮原本贴在 API Key 输入框右缘——输入框改全宽后统一收到这里。
    let remove_provider = provider.clone();
    let save_provider = provider.clone();
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(detail_button(
                        "mc-remove",
                        weak,
                        &tr("action.remove"),
                        crate::ui::button::BtnVariant::Danger,
                        false,
                        move |c, cx| c.mc_ask_remove_provider(remove_provider.clone(), cx),
                    ))
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
            ),
    );
    // 详情页唯一信息栏（用户定稿）：按钮下方、右对齐，同一时刻只有一条
    let detail = detail.when(error.is_some(), |d| {
        d.child(error_note_right(error.as_deref().unwrap_or("")))
    });

    // API Key 段：标题 + 输入**全宽**（右缘内置眼睛，掩码显隐已存 key）；
    // 保存钮在 headr 行（见上）
    let weak_eye = weak.clone();
    let detail = detail
        .child(section_title("API Key"))
        .child(
            div().flex().items_center().child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .child(d.key_input.clone())
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
                                if d.key_visible { "eye-off" } else { "eye" },
                                14.,
                                t.text_muted,
                            )),
                    ),
            ),
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
        &d.model_filter,
        &d.model_filter_value,
        d.model_free_only,
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
            // 右侧聚簇：只看free（开关在左、文字在右）+ 刷新钮；文字与刷新钮 10px
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child({
                        // 只看free：开 = 列表只显示 free 模型（筛选叠加隐藏的 free
                        // 关键字）；状态在 SettingsPanel，切换即整面板重渲染
                        let weak_free = weak.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
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
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(11.))
                                    .text_color(rgb(t.text_dim))
                                    .child(tr("models.free_only")),
                            )
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
            ),
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
                    project_scope,
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
        self.catalog_with_ledger()
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
        raw.to_vec()
    }

    /// 设置页用的模型清单（042-1：单一来源——`models.json` 已并入磁盘目录，
    /// 不再有「内置取 pi / 自定义取 PF 账本」的两套口径）。
    pub(crate) fn catalog_with_ledger(&self) -> Vec<pi_link::protocol::ModelInfo> {
        self.catalog_for(self.project())
    }

    /// 删掉某商在**内存目录**里的条目（042-1：删除自定义商后列表项立刻消失）。
    /// 光删 models.json 不够——`models_by_cwd` 里的进程答案与启动时装的
    /// globals 都还带着该商的模型，侧栏/菜单会继续列出它。
    pub(crate) fn drop_provider_from_catalog(&mut self, provider: &str) {
        self.pool.globals.models.retain(|m| m.provider != provider);
        for models in self.pool.models_by_cwd.values_mut() {
            models.retain(|m| m.provider != provider);
        }
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

/// 042-1 自定义商「测速」状态（单槽：详情页同屏只有一个）。测速通过 =
/// 自动保存表单 + 自动拉模型落目录；通过前【保存】【+模型】禁用。
/// `seeded` = 打开已有商时的初始通过态（非实测，不显示结果行）。
#[derive(Debug, Clone)]
pub(crate) enum MjSpeedState {
    Running,
    Passed {
        latency_ms: u64,
        models: usize,
        seeded: bool,
    },
    /// 通了但没解出模型条目：已保存，提示手动 +模型
    PassedEmpty,
    Failed(String),
}

impl MjSpeedState {
    /// 保存 / +模型 的可用判据（042-1：测速未通过前禁用）。
    pub(crate) fn allows_edit(&self) -> bool {
        matches!(
            self,
            MjSpeedState::Passed { .. } | MjSpeedState::PassedEmpty { .. }
        )
    }
}

/// 042 M2 自定义模型「测试」状态（pi-web ModelDetail test 同款：进行中 /
/// 通过（延迟 + HTTP 状态 + 回复文本）/ 失败文案）。
#[derive(Debug, Clone)]
pub(crate) enum MjTestState {
    Running,
    Ok {
        latency_ms: u64,
        status: u16,
        text: String,
    },
    Err(String),
}

/// 042 M2 models.dev 目录拉取状态（「填入模型信息」按钮与结果行）。
#[derive(Debug, Clone)]
pub(crate) enum MjCatalogState {
    Idle,
    Running,
    /// Done：回填成功（成功不占提示栏，用户定稿；只有失败才提示）
    Done,
    Err(String),
}
