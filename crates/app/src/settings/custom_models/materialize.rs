//! models.json 物化与修复：探查目录落库、选择物化、账本对账、修复
//! 残缺文档（mj_repair_models_json）。

use super::*;

impl Chat {
    /// 测速/刷新探测到的官方清单 → **清单仓**���custom-provider.json + 内存镜像）。
    /// 2026-11-01 定稿：探测只补缺不覆盖（手调参数保留）、不整批入账本；用户
    /// 勾选哪个才把哪个复制进 models.json（[`Self::mj_materialize_selection`]）。
    pub(super) fn mj_store_probe_catalog(
        &mut self,
        provider: &str,
        catalog: &[pi_link::protocol::ModelInfo],
    ) {
        crate::services::custom_models_store::merge_probed(
            &mut self.cfg.mj_probe_cache,
            provider,
            catalog,
        );
        let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
    }

    /// 勾选物化（2026-11-01 定稿，自定义商专用）：enable → 把该模型以完整
    /// definition 追加进 models.json `providers.<p>.models[]`（仓定义优先原样
    /// 复制；仓里没有才按展示条目 + 骨架最小构造）；disable → 从 models[]
    /// 移除；models[] 空时**骨架保留**。内置商不走这里（pi 静态注册表即持久）。
    /// 纯变换在 [`pi_link::models_json::materialize_model`]（0.3.2 教训：两条
    /// 分支各自 push、仓命中分支漏落盘——现在只有这一处 save）。
    /// 返回 Err 文案供调用方挂错误条。
    pub(crate) fn mj_materialize_selection(
        &mut self,
        provider: &str,
        id: &str,
        enable: bool,
    ) -> Result<(), String> {
        let mut doc = Self::mj_doc();
        if pi_link::models_json::provider_entry(&doc, provider).is_none() {
            return Ok(()); // 不是账本里的商（内置/扩展）：不物化
        }
        let store_def = crate::services::custom_models_store::definition(
            &self.cfg.mj_probe_cache,
            provider,
            id,
        )
        .cloned();
        // 展示条目（探测缓存 / 目录）只在仓里没有时才用来补 name/contextWindow
        let shown = if enable && store_def.is_none() {
            self.provider_models_for_list(provider)
                .into_iter()
                .find(|m| m.id == id)
        } else {
            None
        };
        let changed = pi_link::models_json::materialize_model(
            &mut doc,
            provider,
            id,
            enable,
            store_def.as_ref(),
            shown.as_ref().map(|m| m.name.as_str()),
            shown.as_ref().and_then(|m| m.context_window),
        );
        if !changed {
            return Ok(());
        }
        let r = pi_link::models_json::save(&doc);
        crate::services::mj_doc_cache::invalidate();
        r
    }

    /// 存量 models.json 形状自愈（启动 / 设置页装载）：老版本写出的残缺 cost
    ///（自动填参只给 input/output）让 pi **整份拒载**——自定义商全部消失、
    /// `--model` 直接退出、会话空转（2026-11-02 真机 my-glm）。normalize 一遍，
    /// 有改动才写盘；不可读/坏文件照旧拒写（save 内置闸）。
    pub(crate) fn mj_repair_models_json(&mut self) {
        let Ok(mut doc) = pi_link::models_json::read() else {
            return;
        };
        if !pi_link::models_json::repair(&mut doc) {
            return;
        }
        // 结果只进 debug stderr（release 零输出门禁）；写失败下一次保存仍会再试
        if cfg!(debug_assertions) {
            match pi_link::models_json::save(&doc) {
                Ok(()) => eprintln!("[models.json] 形状自愈已写盘（残缺 cost 等已补齐）"),
                Err(e) => eprintln!("[models.json] 形状自愈写盘失败: {e}"),
            }
        } else {
            let _ = pi_link::models_json::save(&doc);
        }
        crate::services::mj_doc_cache::invalidate();
    }

    /// 存量自愈（0.3.2 勾选漏写盘的善后）：enabledModels **显式**勾选、清单仓
    /// 有定义、models.json 却没有的自定义商模型 → 物化补齐。all_enabled
    ///（无 patterns / patterns 全失配）一律跳过——绝不整批写入账本（2026-11-01
    /// 定稿：勾选的才进 models.json）。
    ///
    /// 走 `reload_settings_panel` → **启动路径**（Chat::new）也会跑：无自定义
    /// 仓 / 无显式 patterns 时零开销直接返回；有则一次 models.json 读 +
    /// 一次 compute_state，无事可做零写盘。
    pub(crate) fn mj_reconcile_custom_selection(&mut self) {
        if self.cfg.mj_probe_cache.is_empty()
            || self.cfg.mc_patterns.as_ref().is_none_or(|p| p.is_empty())
        {
            return;
        }
        let state = crate::services::models_config::compute_state(
            self.cfg.mc_patterns.as_ref(),
            &self.mc_refs(),
        );
        if state.all_enabled || state.enabled.is_empty() {
            return;
        }
        let doc = Self::mj_doc();
        let mut todo: Vec<(String, String)> = Vec::new();
        for (provider, entry) in &self.cfg.mj_probe_cache {
            let Some(ledger) = pi_link::models_json::provider_entry(&doc, provider) else {
                continue;
            };
            if !self.mc_source_is_custom(provider, true) {
                continue;
            }
            let in_ledger: Vec<&str> = pi_link::models_json::provider_models(ledger)
                .iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
                .collect();
            for id in entry.models.keys() {
                let r = format!("{provider}/{id}");
                if state.enabled.iter().any(|e| e == &r) && !in_ledger.contains(&id.as_str()) {
                    todo.push((provider.clone(), id.clone()));
                }
            }
        }
        for (p, id) in todo {
            if self.mj_materialize_selection(&p, &id, true).is_ok() {
                self.refresh_provider_catalog(&p);
            }
        }
    }
}
