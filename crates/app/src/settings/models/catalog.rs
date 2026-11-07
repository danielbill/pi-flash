//! 模型目录供给：共享目录读取、显示名、排序键、内置/自定义判别、
//! 内置目录 sweep。

use super::*;

impl Chat {
    /// Provider ids in available-models display order.
    pub(crate) fn mc_provider_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        // 借用迭代（2026-11-05：原 catalog_for 整份克隆只为取 provider 去重）
        for m in self.catalog_slice_for(self.project()) {
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
        // pi 自己的默认模型：仅草稿 pill 无星标时的显示兜底（见 CfgUi 注释）
        self.cfg.mc_pi_default_model = pi_link::config::read_default_model(&settings_path);
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
        // 兜底链（2026-11-02 定稿，镜像 pi findInitialModel：PF 不传 --models，
        // 其第 1 步恒空）：① PF 星标在 scope 内 → ② pi settings.json 默认——在
        // scope 内 ∧ 该商有凭据（auth.json 口径，对应 pi hasConfiguredAuth）
        // → ③ 显示名序第一个（与下列表同一把尺子）。无星标时 spawn 不传
        // --model，pi 真会选 ②，草稿 pill 先显示它才不会在进程起来后跳一下。
        // 残差（已知不镜像）：pi 第 3 步的内置 defaultModelPerProvider 表；
        // models.json 手写 apiKey 这种凭据 PF 不算——这两种情形仍可能跳
        let find_in_scope = |p: &str, id: &str| {
            scope
                .iter()
                .find(|m| m.provider == p && m.id == id)
                .map(|m| (m.provider.clone(), m.id.clone()))
        };
        let default = self
            .cfg
            .mc_default_model
            .as_ref()
            .and_then(|(p, id)| find_in_scope(p, id))
            .or_else(|| {
                let (p, id) = self.cfg.mc_pi_default_model.as_ref()?;
                let has_cred = self.cfg.mc_creds.iter().any(|(prov, _)| prov == p);
                has_cred.then(|| find_in_scope(p, id)).flatten()
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
}
impl Chat {
    /// 归属判定：该商是否「自定义商」（models.json 条目 + 注册表不认 = 用户
    /// 自建）。勾选物化 / 详情页可编辑 / 存量对账三处同一把尺子。
    pub(crate) fn mc_is_custom_provider(&self, prov: &str) -> bool {
        let in_ledger = pi_link::models_json::provider_entry(&Self::mj_doc(), prov).is_some();
        self.mc_source_is_custom(prov, in_ledger)
    }

    /// 同上，但账本成员资格由调用方给（已持有 doc 时免再读盘——启动路径用）。
    pub(crate) fn mc_source_is_custom(&self, prov: &str, in_ledger: bool) -> bool {
        match self.cfg.provider_registry.as_deref() {
            Some(dump) => dump.source(prov, in_ledger) == pi_link::registry::ProviderSource::Custom,
            None => {
                pi_link::registry::source_without_registry(in_ledger)
                    == pi_link::registry::ProviderSource::Custom
            }
        }
    }
}
impl Chat {
    /// 内置商「获取模型」端点（注册表 SDK 派生，[`pi_link::registry::models_list_target`]）。
    /// 拿不到 = SDK 没给 baseUrl 或注册表未装载。仅供隐藏中的拉取 UI
    /// （[`Self::BUILTIN_FETCH_UI`]）与保存 key 后的连通检查使用。
    pub(super) fn builtin_catalog_target(
        &self,
        provider: &str,
    ) -> Option<(String, pi_link::probe::Auth)> {
        let dump = self.cfg.provider_registry.as_ref()?;
        pi_link::registry::models_list_target(dump, provider)
    }

    /// 启动装载全量注册表（2026-11-01 定稿）：揭幕后后台一次 dump（缓存按
    /// vendor VERSION 失效），装进 `cfg.provider_registry`——内置商罗列
    /// （[`Self::provider_models_for_list`] 并入静态目录）与归属判定从第一帧
    /// 起就有注册表答案。**不再探测/拉取任何目录**：内置商列表 = pi 自带，
    /// models.json 零写入。
    pub(crate) fn spawn_builtin_catalog_sweep(&mut self, cx: &mut Context<Self>) {
        if self.cfg.provider_registry.is_some() {
            return;
        }
        if let Some(d) = pi_link::registry::read_cache() {
            self.cfg.provider_registry = Some(std::sync::Arc::new(d));
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let dumped = cx
                .background_executor()
                .spawn(async move { pi_link::registry::dump_and_cache() })
                .await;
            if let Ok(d) = dumped {
                let _ = this.update(cx, |c, cx| {
                    c.cfg.provider_registry = Some(std::sync::Arc::new(d));
                    c.cfg.mc_state = services::models_config::compute_state(
                        c.cfg.mc_patterns.as_ref(),
                        &c.mc_refs(),
                    );
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
impl Chat {
    /// Whether the provider has an api_key credential（detail 头部状态点用；
    /// 侧栏已改 provider logo，不再用凭据状态点缀行）。
    pub(crate) fn mc_configured(&self, provider: &str) -> bool {
        // api_key 与 oauth 都算「该商在用」（列表成员资格 = 有凭据）
        self.cfg.mc_creds.iter().any(|(p, _)| p == provider)
    }
}
