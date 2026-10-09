//! Models tab (pi-web ModelsConfig + EnabledModelsSection parity): 路径条
//! （enabledModels n/m + 清理无效条目/启用全部模型）、左栏 catalog provider
//! 与 models.json 自定义 provider（嵌套模型行 + 加模型）、右栏 API Key /
//! OAuth / 自定义编辑器（保存前缓冲在 mc_models_json）、可用模型区（筛选 +
//! 全部开启/关闭）、底部 models.json 保存。

use super::*;
use super::custom_models::{mj_add_panel, mj_model_editor, mj_provider_editor};


impl Chat {
    pub(crate) fn open_settings(&mut self, tab: u8, cx: &mut Context<Self>) {
        self.reload_settings_panel();
        let section = super::SettingsPanel::prefill_section(self, tab);
        let panel = cx.new(|cx| {
            let mut panel = SettingsPanel::new(cx);
            panel.tab = tab;
            panel.section = section;
            panel
        });
        self.settings = Some(panel);
        cx.notify();
    }

    /// Provider ids in available-models display order.
    pub(crate) fn mc_provider_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in self.catalog_for(&self.cwd) {
            if !out.contains(&m.provider) {
                out.push(m.provider.clone());
            }
        }
        // auth.json 凭据 provider 也算一行（TypeSafe 奇偶校验：有 key、0 个
        // 可用模型也显示，pi-web activeApiKey parity）
        for (p, _) in &self.mc_creds {
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
        let models: Vec<&pi_link::protocol::ModelInfo> = self
            .catalog_for(&self.cwd)
            .iter()
            .filter(|m| m.provider == provider)
            .collect();
        let enabled = models
            .iter()
            .filter(|m| {
                let r = format!("{}/{}", m.provider, m.id);
                enabled_set.contains(&r)
            })
            .count();
        (enabled, models.len())
    }

    /// Re-read the model-related settings.json defaults a new session starts
    /// with (pi-web /api/models `defaultModel`/`defaultThinkingLevel` inputs).
    /// Cheap (one settings.json read) — also called at startup, not just when
    /// the settings panel is open.
    pub(crate) fn reload_model_defaults(&mut self) {
        let settings_path = pi_link::config::settings_path();
        self.mc_patterns =
            pi_link::config::read_enabled_models(&settings_path).unwrap_or_else(|_| None);
        let project = pi_link::config::project_settings_path(&self.cwd);
        self.mc_project_scope =
            pi_link::config::read_enabled_models(&project).unwrap_or_else(|_| None).is_some();
        self.mc_default_model = pi_link::config::read_default_model(&settings_path);
        self.mc_default_thinking = pi_link::config::read_default_thinking_level(&settings_path);
        self.mc_model_thinking = pi_link::config::read_model_thinking_levels(&settings_path);
        self.mc_state =
            models_config::compute_state(self.mc_patterns.as_ref(), &self.mc_refs());
    }

    pub(crate) fn new_session_default(&self) -> (Option<(String, String)>, Option<String>) {
        let catalog = self.catalog_for(&self.cwd);
        let in_scope = |m: &pi_link::protocol::ModelInfo| {
            self.mc_state.all_enabled
                || {
                    let r = format!("{}/{}", m.provider, m.id);
                    self.mc_state.enabled.iter().any(|e| e == &r)
                }
        };
        // pi: patterns that resolve to nothing fall back to every model
        let scope: Vec<&pi_link::protocol::ModelInfo> = {
            let scoped: Vec<_> = catalog.iter().filter(|m| in_scope(m)).collect();
            if scoped.is_empty() { catalog.iter().collect() } else { scoped }
        };
        let default = self
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
                    .first()
                    .map(|m| (m.provider.clone(), m.id.clone()))
            });
        let thinking = default.as_ref().and_then(|(p, id)| {
            let r = format!("{p}/{id}");
            self.mc_state
                .pins
                .iter()
                .find(|(pr, _)| pr == &r)
                .map(|(_, l)| l.clone())
                .or_else(|| {
                    self.mc_model_thinking
                        .iter()
                        .find(|(pr, _)| pr == &r)
                        .map(|(_, l)| l.clone())
                })
        }).or_else(|| self.mc_default_thinking.clone());
        (default, thinking)
    }

    /// Display name of one catalog model ("name", falling back to
    /// `provider/id` for models the catalog hasn't listed).
    pub(crate) fn model_display_name(&self, provider: &str, id: &str) -> String {
        self.catalog_for(&self.cwd)
            .iter()
            .find(|m| m.provider == provider && m.id == id)
            .map(|m| m.label())
            .unwrap_or_else(|| format!("{provider}/{id}"))
    }

    /// 把启动装载的全局态 + 当前项目的项目上下文**安装**进设置页字段
    /// （010-启动.md §1/§5/§7）。纯内存赋值：不再逐项扫盘——skills / packages /
    /// mcp 都来自启动时已装好的 `globals` 与 `project_ctx`；
    /// 切项目命中集合就直接切，未命中时由 `project_ctx_for` 现算一次并纳入。
    pub(crate) fn reload_settings_panel(&mut self) {
        self.reload_model_defaults();
        // 凭据（PF 自有账本 pf-auth.json；auth.json 归 pi，PF 不读）
        self.mc_creds = pi_link::pf_auth::kinds();
        // 会话预设 `configured` 对应的工具清单（settings.json defaultTools）
        self.mc_default_tools = self.globals.default_tools.clone();
        self.mc_pkgs_global = self.globals.packages.clone();
        let ctx = self.project_ctx_now();
        self.mc_skills = ctx.skills;
        self.mc_pkgs_project = ctx.packages;
        self.mc_project_scope = ctx.project_scope;
        self.mcp_servers = ctx.mcp_servers;
        self.mcp_errors = ctx.mcp_errors;
        // models.json 编辑缓冲（面板打开/写盘后重建）
        match pi_link::models_json::read() {
            Ok(v) => {
                self.mc_models_json = v;
                self.mc_mj_error = None;
            }
            Err(e) => {
                self.mc_models_json = serde_json::json!({});
                self.mc_mj_error = Some(e);
            }
        }
        self.mc_mj_dirty = false;
        self.mc_mj_saved = false;
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
            Err(_) => self.mc_set_error(tr("至少需要保留一个启用的模型"), cx),
        }
    }

    pub(crate) fn mc_toggle_provider(&mut self, provider: &str, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        match models_config::set_provider_enabled(self.mc_patterns.as_ref(), &self.mc_refs(), provider, enable) {
            Ok(edit) => self.apply_pattern_edit(edit, cx),
            Err(_) => self.mc_set_error(tr("至少需要保留一个启用的模型"), cx),
        }
    }

    /// 启用全部模型（enabledModels 白名单一键清空 → 删除该键）。
    pub(crate) fn mc_clear_scope(&mut self, cx: &mut Context<Self>) {
        if self.mc_project_scope {
            self.mc_set_error(tr("项目级 .pi/settings.json 覆盖了 enabledModels，面板只读"), cx);
            return;
        }
        if let Err(e) = pi_link::config::write_enabled_models(&pi_link::config::settings_path(), None) {
            self.mc_set_error(&e, cx);
            return;
        }
        self.reload_model_defaults();
        cx.notify();
    }

    /// 清理无效条目（匹配不到任何可用模型的 pattern）。
    pub(crate) fn mc_prune_stale(&mut self, cx: &mut Context<Self>) {
        if self.mc_project_scope {
            self.mc_set_error(tr("项目级 .pi/settings.json 覆盖了 enabledModels，面板只读"), cx);
            return;
        }
        let Some(patterns) = self.mc_patterns.clone() else { return };
        let stale: Vec<String> = self.mc_state.stale.clone();
        let next: Vec<String> = patterns.into_iter().filter(|p| !stale.contains(p)).collect();
        let edit = models_config::Edit {
            changed: !stale.is_empty(),
            patterns: Some(next),
        };
        self.apply_pattern_edit(edit, cx);
    }

    pub(crate) fn mc_save_key(&mut self, provider: String, key: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if key.trim().is_empty() {
            self.mc_set_error(tr("API Key 不能为空"), cx);
            return;
        }
        // 051：明文 → 凭据库 + pf-auth.json 引用；高级引用原样；库不可用降级明文
        match pi_link::pf_auth::store_catalog_key(&provider, key.trim(), &pi_link::credentials::KeyringVault)
        {
            Ok(pi_link::credentials::StoreMode::File) => {
                self.mc_set_error(tr("系统凭据库不可用，已降级为文件存储"), cx);
            }
            Ok(_) => {}
            Err(e) => {
                self.mc_set_error(&crate::i18n::tf("保存失败: {e}", &[("e", e)]), cx);
                return;
            }
        }
        // 换 key 注入的是子进程环境，重启会话才生效（051 §8）
        self.reload_settings_panel();
        if let Some(st) = self.settings.clone() {
            let input = st.read(cx).key_input.clone();
            input.update(cx, |ti, cx| ti.set_value(String::new(), cx));
        }
        cx.notify();
    }

    pub(crate) fn mc_delete_key(&mut self, provider: String, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if let Err(e) =
            pi_link::pf_auth::delete_catalog_key(&provider, &pi_link::credentials::KeyringVault)
        {
            self.mc_set_error(&e, cx);
            return;
        }
        self.reload_settings_panel();
        cx.notify();
    }

    /// Whether the provider has an api_key credential（detail 头部状态点用；
    /// 侧栏已改 provider logo，不再用凭据状态点缀行）。
    pub(crate) fn mc_configured(&self, provider: &str) -> bool {
        self.mc_creds
            .iter()
            .any(|(p, k)| p == provider && *k == pi_link::config::CredentialKind::ApiKey)
    }

    /// 051：OAuth 归 pi（auth.json），PF 页面只管自有账本，一律按无 OAuth 处理。
    pub(crate) fn mc_oauth(&self, _provider: &str) -> bool {
        false
    }

    /// 「已配置」状态文案：自有账本的落点（凭据库 / 文件存储 / 高级引用）。
    pub(crate) fn mc_store_label(&self, provider: &str) -> Option<String> {
        match pi_link::pf_auth::store_mode(provider) {
            Some(pi_link::credentials::StoreMode::Vault) => {
                Some(tr("已配置 · 系统凭据库").to_string())
            }
            Some(pi_link::credentials::StoreMode::Ref) => Some(tr("已配置").to_string()),
            Some(pi_link::credentials::StoreMode::File) => {
                Some(tr("已配置 · 文件存储").to_string())
            }
            None => None,
        }
    }

    /// 选中一个 catalog provider：清 key 输入与筛选（pi-web 换 provider 重置表单）。
    pub(crate) fn mc_select_provider(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = id;
                s.error = None;
                s.key_input.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                s.model_filter.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                cx.notify();
            });
        }
    }

    // -- models.json 自定义 provider / 模型编辑器（缓冲，底部保存落盘） ------

}


// ---------------------------------------------------------------------------
/// `C:\Users\x\.pi\agent\...` → `~\.pi\agent\...`（banner 路径缩短）。
fn shorten_home(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().to_string();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    home.is_empty()
        .then_some(text.clone())
        .unwrap_or_else(|| text.replacen(&home, "~", 1))
}

// ---------------------------------------------------------------------------
// view
// ---------------------------------------------------------------------------

/// The models tab: banner + sidebar/detail + footer (pi-web ConfigPanelShell
/// 内的 EnabledModelsBanner → SplitView → Footer 结构)。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mc_models_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
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
) -> gpui::AnyElement {
    let t = T();
    let mut col = div().flex().flex_col().w_full().h_full().min_h_0();

    // ---- 路径条（EnabledModelsBanner：白名单收窄或有失配时出现） ----------
    let total_available = chat.catalog_for(&chat.cwd).len();
    let enabled_total = if chat.mc_state.all_enabled {
        total_available
    } else {
        chat.mc_state.enabled.len()
    };
    let scoped = !chat.mc_state.all_enabled;
    let stale = chat.mc_state.stale.len();
    if scoped || stale > 0 {
        // 顶条与扩展页安装栏同款：pl15/pr20 与列表/详情内容缘对齐，底部划线
        let mut banner = div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(15.))
            .pr(px(20.))
            .pt(px(6.))
            .pb(px(10.))
            .border_b_1()
            .border_color(rgb(t.border))
            .child(
                div()
                    .font_family(crate::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.5))
                    .text_color(rgb(t.text_dim))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .flex_shrink()
                    .child(SharedString::from(format!(
                        "{} · enabledModels {enabled_total}/{total_available}",
                        shorten_home(&pi_link::config::settings_path()),
                    ))),
            );
        if stale > 0 {
            banner = banner
                .child(
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.5))
                        .text_color(rgb(WARN))
                        .child(SharedString::from(format!("{stale} 条失配"))),
                )
                .child(config_button(
                    "mj-prune",
                    weak,
                    &tr("清理无效条目"),
                    Btn::Secondary,
                    true,
                    chat.mc_project_scope,
                    |c, cx| c.mc_prune_stale(cx),
                ));
        }
        if scoped {
            banner = banner.child(config_button(
                "mj-clear",
                weak,
                &tr("启用全部模型"),
                Btn::Secondary,
                true,
                chat.mc_project_scope,
                |c, cx| c.mc_clear_scope(cx),
            ));
        }
        col = col.child(banner);
    }

    // ---- split view -------------------------------------------------------
    let provider_ids = chat.mc_provider_ids();
    let selected = if section.is_empty() {
        provider_ids.first().cloned().unwrap_or_default()
    } else {
        section.to_string()
    };
    // 本帧的启用集合（O(1) 行查询；一次构建，sidebar/detail 共用）
    let enabled_set: std::collections::HashSet<String> =
        chat.mc_state.enabled.iter().cloned().collect();
    let sb = mc_models_sidebar(chat, &weak.clone(), &selected, &provider_ids, &enabled_set, t);
    let detail = mc_models_detail(
        chat, &weak.clone(), &selected, key_input, key_visible, model_filter,
        model_filter_value, mj_name, mj_base, mj_key, mj_id, mj_mname, mj_ctx,
        mj_api, mj_reasoning, error, &enabled_set, t,
    );
    col = col.child(two_pane_outer(sb, detail));

    // ---- footer（models.json 缓冲保存） ------------------------------------
    let mut status: Option<gpui::AnyElement> = None;
    if let Some(e) = &chat.mc_mj_error {
        status = Some(error_note(&crate::i18n::tf(
            "无法读取 models.json，为避免覆盖已禁用保存：{e}",
            &[("e", e.clone())],
        )));
    } else if chat.mc_mj_saved {
        status = Some(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(GREEN))
                .child(tr("已保存"))
                .into_any_element(),
        );
    } else if chat.mc_mj_dirty {
        status = Some(note("更改已缓冲，点「保存」写入 ~/.pi/agent/models.json"));
    }
    col = col.child(footer(
        status,
        vec![config_button("mj-save", weak, &tr("保存"), Btn::Primary, false, chat.mc_mj_error.is_some(), |c, cx| {
            c.mj_save(cx)
        })],
    ));
    col.into_any_element()
}

/// banner/底栏夹着的分栏（外层 body 不滚动，h_full 内部各自滚）。
fn two_pane_outer(sidebar: gpui::AnyElement, detail: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(sidebar)
        .child(detail)
        .into_any_element()
}

/// Models sidebar: catalog providers + custom providers with nested models.
fn mc_models_sidebar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    selected: &str,
    provider_ids: &[String],
    enabled_set: &std::collections::HashSet<String>,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    // px7 + 行内 px8 = 内容左右 15px（与右侧详情 p15 等距，040 扩展页定稿）
    let mut list = sidebar_list().px(px(7.));

    // catalog providers（pi-web 侧栏同款：provider logo，未命中走首字母方块）
    for p in provider_ids {
        let active = *p == selected;
        let (enabled, total) = chat.mc_provider_counts(p, enabled_set);
        let weak_item = weak.clone();
        let pid = p.clone();
        list = list.child(
            widgets::sidebar_item(format!("mc-side-{p}"), active)
                // 行背景压平（040：列表无底色，选中态只靠加粗+深字色，hover 保留）
                .bg(rgb(t.bg))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| c.mc_select_provider(pid.clone(), cx));
                })
                .child(crate::ui::provider_icon(p, 16., t.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(p.clone())),
                )
                .children((!chat.mc_state.all_enabled).then(|| {
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!("{enabled}/{total}")))
                        .into_any_element()
                })),
        );
    }

    // 分隔线（有 catalog provider 且有自定义 provider 时）
    let custom: Vec<(String, &serde_json::Value)> = pi_link::models_json::providers(&chat.mc_models_json);
    if !provider_ids.is_empty() && !custom.is_empty() {
        list = list.child(
            div()
                .mx(px(8.))
                .my(px(4.))
                .h(px(1.))
                .bg(rgb(t.border)),
        );
    }

    // 自定义 provider（嵌套模型行 + 加模型）
    for (name, entry) in &custom {
        let prov_key = format!("mj:p:{name}");
        let active = prov_key == selected;
        let weak_item = weak.clone();
        let key = prov_key.clone();
        list = list.child(
            widgets::sidebar_item(format!("mj-side-{name}"), active)
                .bg(rgb(t.bg))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| c.mj_select(key.clone(), cx));
                })
                .child(crate::ui::icon("cpu", 11., t.text_dim))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(name.clone())),
                ),
        );
        for (ix, m) in pi_link::models_json::provider_models(entry).into_iter().enumerate() {
            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let reasoning = m.get("reasoning").and_then(|v| v.as_bool()).unwrap_or(false);
            let model_key = format!("mj:m:{name}:{ix}");
            let active = model_key == selected;
            let weak_row = weak.clone();
            list = list.child(
                widgets::sidebar_item(format!("mj-m-{name}-{ix}"), active)
                    .bg(rgb(t.bg))
                    .pl(px(26.))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_row.update(cx, |c, cx| c.mj_select(model_key.clone(), cx));
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_color(rgb(if id.is_empty() { t.text_dim } else { t.text_muted }))
                            .child(SharedString::from(if id.is_empty() {
                                tr("新模型").to_string()
                            } else {
                                id.to_string()
                            })),
                    )
                    .children(reasoning.then(|| {
                        div()
                            .px(px(4.))
                            .py(px(1.))
                            .rounded(px(3.))
                            .bg(widgets::indigo_bg())
                            .text_size(crate::appearance::ui_size(9.))
                            .text_color(widgets::indigo_fg())
                            .child("T")
                            .into_any_element()
                    })),
            );
        }
        // + 模型
        let weak_add = weak.clone();
        let pname = name.clone();
        list = list.child(
            widgets::sidebar_item(format!("mj-addm-{name}"), false)
                .bg(rgb(t.bg))
                .pl(px(26.))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_add.update(cx, |c, cx| c.mj_add_model(pname.clone(), cx));
                })
                .child(
                    div()
                        .text_color(rgb(t.text_dim))
                        .hover(|s| s.text_color(rgb(t.accent)))
                        .child(SharedString::from(format!("+ {}", tr("模型")))),
                ),
        );
    }

    // 列表底色压平（040：与页面同色，选中态只靠字重字色）
    sidebar_shell("mc-sidebar")
        .bg(rgb(t.bg))
        .child(list)
        .child(list_action(
            "mj-add-provider",
            weak,
            &tr("添加 Provider"),
            selected == "__add_provider__",
            |c, cx| {
                if let Some(st) = c.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.section = "__add_provider__".into();
                        s.error = None;
                        cx.notify();
                    });
                }
            },
        ))
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
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    // custom provider / model editors
    if let Some(name) = selected.strip_prefix("mj:p:") {
        return mj_provider_editor(
            chat, weak, name, mj_name, mj_base, mj_key, mj_api, error, t,
        );
    }
    if let Some(rest) = selected.strip_prefix("mj:m:") {
        if let Some((name, ix)) = rest.rsplit_once(':') {
            if let Ok(ix) = ix.parse::<usize>() {
                return mj_model_editor(
                    weak, name, ix, mj_id, mj_mname, mj_ctx, mj_reasoning, error, t,
                );
            }
        }
    }
    if selected == "__add_provider__" {
        return mj_add_panel(chat, weak, mj_name, mj_base, mj_key, mj_api, t);
    }

    // catalog provider detail（API Key；OAuth 归 pi，PF 只管自有账本）
    let provider = selected.to_string();
    let dc_provider = provider.clone();
    let models: Vec<pi_link::protocol::ModelInfo> = chat
        .catalog_for(&chat.cwd)
        .iter()
        .filter(|m| m.provider == provider)
        .cloned()
        .collect();
    let configured = chat.mc_configured(&provider);
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let detail = detail_shell("mc-detail")
        .p(px(15.))
        .when(error.is_some(), |d| {
            d.child(error_note(error.as_deref().unwrap_or("")))
        });

    // header：API Key + 状态（落点）+ 断开连接
    let mut head = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(28.))
        .child(section_title("API Key"))
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(status_dot(if configured { GREEN } else { t.border }))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(if configured { GREEN } else { t.text_dim }))
                        .child(if configured {
                            SharedString::from(
                                chat.mc_store_label(&provider)
                                    .unwrap_or_else(|| tr("已配置").to_string()),
                            )
                        } else {
                            SharedString::from(tr("未配置").to_string())
                        }),
                ),
        );
    if configured {
        head = head.child(config_button(
            "mc-disconnect",
            weak,
            &tr("断开连接"),
            Btn::Danger,
            true,
            false,
            move |c, cx| c.mc_delete_key(dc_provider.clone(), cx),
        ));
    }
    let detail = detail.child(head);

    // 凭据输入区（API key：输入+眼睛+保存）
    let weak_eye = weak.clone();
    let save_provider = provider.clone();
    let detail = detail
        .child(
            div()
                .flex()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(key_input.clone()),
                )
                .child(
                    div()
                        .id("mc-key-eye")
                        .w(px(36.))
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
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
                        })
                        .child(if key_visible { tr("隐藏") } else { tr("显示") }),
                )
                .child(config_button("mc-key-save", weak, &tr("保存"), Btn::Primary, false, false, move |c, cx| {
                    let key = c
                        .settings
                        .as_ref()
                        .map(|st| st.read(cx).key_input.clone())
                        .map(|input| input.read(cx).value().to_string())
                        .unwrap_or_default();
                    c.mc_save_key(save_provider.clone(), key, cx);
                })),
        )
        .child(note(
            "密钥存入系统凭据库（Windows 凭据管理器 / macOS 钥匙串）；pf-auth.json 只留变量名，auth.json 归 pi",
        ));

    // 可用模型区 + 底部（detail_shell 收尾）
    mc_enabled_section(
        chat, weak, &provider, &models, enabled_set, model_filter, model_filter_value, detail,
    )
        .into_any_element()
}
/// 可用模型区（EnabledModelsSection parity）：标题+计数+批量钮+筛选+行开关。
///
/// 列表用 uniform_list 虚拟化——只构建可见的 ~12 行。overflow div 全量
/// 构建 400 行 × ~12 元素、每敲一字筛选全量重建，是设置页卡顿的来源。
/// 行高固定 36px；启用查询走调用方一次构建的 HashSet（原来是每行线性扫
/// enabled 全表，400×400 次字符串比较/帧）。
fn mc_enabled_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    models: &[pi_link::protocol::ModelInfo],
    enabled_set: &std::collections::HashSet<String>,
    model_filter: &gpui::Entity<crate::TextInput>,
    model_filter_value: &str,
    detail: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    if models.is_empty() {
        return detail;
    }
    let t = T();
    let enabled_count = models
        .iter()
        .filter(|m| {
            let r = format!("{}/{}", m.provider, m.id);
            enabled_set.contains(&r)
        })
        .count();
    let bulk_provider = provider.to_string();

    let mut section = div().flex().flex_col().gap(px(8.)).pt(px(10.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(section_title(&tr("可用模型")))
            .child(
                div()
                    .font_family(crate::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(crate::i18n::tf(
                        "已启用 {enabled}/{total}",
                        &[("enabled", enabled_count.to_string()), ("total", models.len().to_string())],
                    ))),
            )
            .child(div().flex_1())
            .child(config_button("mc-bulk-on", weak, &tr("全部开启"), Btn::Secondary, true, chat.mc_project_scope, {
                let bp = bulk_provider.clone();
                move |c, cx| c.mc_toggle_provider(&bp, true, cx)
            }))
            .child(config_button("mc-bulk-off", weak, &tr("全部关闭"), Btn::Secondary, true, false, {
                let bp = bulk_provider.clone();
                move |c, cx| c.mc_toggle_provider(&bp, false, cx)
            })),
    );
    if chat.mc_project_scope {
        section = section.child(note(
            "当前项目的 .pi/settings.json 设置了 enabledModels 并覆盖全局配置，此处只读",
        ));
    }

    // 筛选（>8 个模型时出现；值经快照传入）
    let q = model_filter_value.trim().to_lowercase();
    let shown: Vec<pi_link::protocol::ModelInfo> = models
        .iter()
        .filter(|m| {
            q.is_empty()
                || m.id.to_lowercase().contains(&q)
                || m.name.to_lowercase().contains(&q)
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
    let all_enabled = chat.mc_state.all_enabled;
    let project_scope = chat.mc_project_scope;
    let last_guard = enabled_count == 1;
    let enabled_rows = enabled_set.clone();
    let pins = chat.mc_state.pins.clone();
    let weak_rows = weak.clone();
    let list = vlist(
        "mc-model-list",
        shown.len(),
        row_h,
        VListHeight::Capped(360.),
        true,
        true,
        "没有匹配的模型",
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
                .px(px(9.))
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
                                .font_family(crate::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(10.))
                                .line_height(gpui::relative(1.25))
                                .text_color(rgb(t.text_dim))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    // click the id to copy the ref
                                    cx.write_to_clipboard(
                                        gpui::ClipboardItem::new_string(ref_click.clone()),
                                    );
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
