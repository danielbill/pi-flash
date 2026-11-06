//! 目录装载与列表数据（P3 自 main.rs impl Chat 拆入）：catalog_for /
//! model_listing / provider_models_for_list / 目录刷新与请求保证。

use super::*;

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
        self.catalog_slice_for(cwd).to_vec()
    }

    /// [`Self::catalog_for`] 的零克隆视图（2026-11-05 卡顿修复）：渲染热路径
    /// 上 model_listing / provider_models_for_list 每 Provider 各克隆一次整份
    /// 目录（实测真机目录 O(商) 次克隆 = 每帧 3-5ms），借这片迭代即可，
    /// 只有真正进结果集的条目才 clone。
    pub(crate) fn catalog_slice_for(
        &self,
        cwd: Option<&std::path::Path>,
    ) -> &[pi_link::protocol::ModelInfo] {
        cwd.and_then(|c| {
            self.pool
                .models_by_cwd
                .get(&c.to_string_lossy().to_string())
        })
        .map(|v| v.as_slice())
        .filter(|v| !v.is_empty())
        .unwrap_or(self.pool.globals.models.as_slice())
    }

    /// 设置页用的模型清单（042-1：单一来源——`models.json` 已并入磁盘目录，
    /// 不再有「内置取 pi / 自定义取 PF 账本」的两套口径）。
    pub(crate) fn catalog_with_ledger(&self) -> Vec<pi_link::protocol::ModelInfo> {
        self.model_listing(self.project())
    }

    /// **侧栏成员商清单**（统一接口①）：内置商 = 有凭据（移除凭据即出列；
    /// models.json 覆盖条目不构成成员）；自定义商 = models.json 有条目；
    /// 扩展注册的 provider 恒入列。设置页侧栏与 input panel 模型菜单共用。
    pub(crate) fn sidebar_providers(&self) -> Vec<String> {
        let doc = Self::mj_doc();
        let custom_entries: Vec<(String, &serde_json::Value)> =
            pi_link::models_json::providers(&doc);
        let in_models_json = |p: &str| custom_entries.iter().any(|(n, _)| n == p);
        let is_custom = |p: &str| -> bool {
            let src = match self.cfg.provider_registry.as_deref() {
                Some(dump) => dump.source(p, in_models_json(p)),
                None => pi_link::registry::source_without_registry(in_models_json(p)),
            };
            src == pi_link::registry::ProviderSource::Custom
        };
        let is_extension = |p: &str| -> bool {
            self.cfg
                .provider_registry
                .as_deref()
                .is_some_and(|dump| dump.is_extension_provider(p, in_models_json(p)))
        };
        let mut provs: Vec<String> = self
            .mc_provider_ids()
            .into_iter()
            .filter(|p| self.mc_configured(p) || is_custom(p) || is_extension(p))
            .collect();
        provs.extend(
            custom_entries
                .iter()
                .filter(|(n, _)| is_custom(n))
                .map(|(n, _)| n.clone()),
        );
        let mut seen = std::collections::HashSet::new();
        provs.retain(|p| seen.insert(p.clone()));
        provs
    }

    /// **统一模型清单**（统一接口②，input panel 菜单与设置页同源）：
    /// 目录（cwd 语义由调用方定）经账本并入 + 已配置内置商静态并入，
    /// 再按**侧栏成员**过滤——picker 里不再出现设置页没有的商（无凭据的
    /// OpenAI 等）。
    pub(crate) fn model_listing(
        &self,
        cwd: Option<&std::path::Path>,
    ) -> Vec<pi_link::protocol::ModelInfo> {
        // 2026-11-05 性能重构（原：ledger_merge 整份克隆 → 成员过滤再丢弃）：
        // 融合「账本并入 + 成员过滤」单趟完成——非成员商的目录行不 clone；
        // 账本商仍 pmf 原样（同 id 重复条目照样显示，用户要看真实磁盘内容）。
        // 顺序语义与原两步版等价：账本行落在其目录首行位置，无目录行的账本
        // 商补在尾部。
        let __t = std::time::Instant::now();
        let doc = Self::mj_doc();
        let ledger_provs = pi_link::models_json::providers(&doc);
        let members = self.sidebar_providers();
        crate::perf_span("frame.listing.members", __t);
        let __t = std::time::Instant::now();
        let slice = self.catalog_slice_for(cwd);
        let mut out: Vec<pi_link::protocol::ModelInfo> = Vec::new();
        let mut emitted: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let in_members = |p: &str| members.iter().any(|x| x == p);
        if ledger_provs.is_empty() {
            for m in slice {
                if in_members(&m.provider) {
                    out.push(m.clone());
                }
            }
        } else {
            for m in slice {
                if ledger_provs.iter().any(|(p, _)| p == &m.provider) {
                    // 该商的目录行让位给账本行（避免重复）
                    if emitted.insert(m.provider.as_str()) {
                        out.extend(self.provider_models_for_list(&m.provider));
                    }
                } else if in_members(&m.provider) {
                    out.push(m.clone());
                }
            }
            for (p, _) in &ledger_provs {
                if !emitted.contains(p.as_str()) {
                    out.extend(self.provider_models_for_list(p));
                }
            }
        }
        crate::perf_span("frame.listing.merge", __t);
        // 已配置凭据的内置商：静态注册表并入（否则其模型不在 mc_refs，
        // 勾选链 set_models_enabled 静默丢弃；只扩成员商，不灌 42 家静态）。
        // (provider,id) 集合去重（2026-11-05：原对 out 线性扫 O(n×m)）
        let __t = std::time::Instant::now();
        let mut seen: std::collections::HashSet<(String, String)> = out
            .iter()
            .map(|x| (x.provider.clone(), x.id.clone()))
            .collect();
        for (prov, _) in &self.cfg.mc_creds {
            if pi_link::models_json::provider_entry(&doc, prov).is_some() {
                continue; // 账本商已由上面 ledger 分支走过 provider_models_for_list
            }
            if !members.iter().any(|p| p == prov) {
                continue;
            }
            for m in self.provider_models_for_list(prov) {
                if seen.insert((m.provider.clone(), m.id.clone())) {
                    out.push(m);
                }
            }
        }
        crate::perf_span("frame.listing.creds", __t);
        out
    }

    /// **某商在列表里要显示的全部模型**（左栏合并列表 / 右栏可用模型区共用
    /// 这一处，左右不许再分叉）：
    /// 该商在 `models.json` 有条目 → 账本 `models[]` **原样**逐条（同 id 的重复
    /// 条目照样显示，用户要看真实磁盘内容）；账本里没有的（内置商）→ 目录该商
    /// 条目；账本条目里没有、目录里有的（扩展注册/pi 自带）补在后面。
    pub(crate) fn provider_models_for_list(
        &self,
        provider: &str,
    ) -> Vec<pi_link::protocol::ModelInfo> {
        let __t = std::time::Instant::now();
        let doc = Self::mj_doc();
        let ledger: Vec<pi_link::protocol::ModelInfo> =
            pi_link::models_json::provider_entry(&doc, provider)
                .map(pi_link::models_json::provider_models)
                .unwrap_or_default()
                .into_iter()
                .map(|m| pi_link::protocol::ModelInfo {
                    id: m
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: m
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    provider: provider.to_string(),
                    context_window: m.get("contextWindow").and_then(|v| v.as_u64()),
                    max_tokens: m.get("maxTokens").and_then(|v| v.as_u64()),
                    reasoning: m.get("reasoning").and_then(|v| v.as_bool()),
                    cost: m.get("cost").and_then(pi_link::protocol::CostInfo::parse),
                    input: m.get("input").and_then(|v| v.as_array()).map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    }),
                })
                .collect();
        let mut out = ledger;
        // id 去重集合（2026-11-05：原 push_missing 对 out 线性扫，O(n²)——
        // openrouter 静态目录 388 模型时每帧 ~75k 次比较；HashSet 后 O(n)，
        // 且命中才 clone，不再白克隆重复候选）
        let mut seen: std::collections::HashSet<String> =
            out.iter().map(|x| x.id.clone()).collect();
        // 借用迭代目录（2026-11-05：原 catalog_for 整份克隆，每帧 O(商) 次）
        for m in self.catalog_slice_for(self.project()) {
            if m.provider != provider {
                continue;
            }
            if seen.insert(m.id.clone()) {
                out.push(m.clone());
            }
        }
        // 2026-11-01 定稿：内置商罗列 = pi 自带注册表（静态目录，无需 key）
        // ——registry dump 的 models_for 并入补缺；静态只在设置页罗列路径
        // 并入，不进 catalog_for/Globals（picker 非虚拟化，性能铁律）
        if let Some(dump) = self.cfg.provider_registry.as_ref() {
            for m in dump.models_for(provider) {
                if seen.insert(m.id.clone()) {
                    out.push(m.clone());
                }
            }
        }
        // 自定义商清单仓：未勾选的定义也在罗列（勾选后进账本，走上面
        // ledger 分支，这里按 id 去重自然让位）
        for m in services::custom_models_store::listing(&self.cfg.mj_probe_cache, provider) {
            if seen.insert(m.id.clone()) {
                out.push(m);
            }
        }
        crate::perf_span("frame.listing.pmf", __t);
        out
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
