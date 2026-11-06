//! 自定义 Provider 保存与删除：mj_apply_provider（改名分流/表单校验
//! 落盘）、删除确认流、key 落盘分流（托管/手写）。

use super::*;

impl Chat {
    /// 把编辑器字段应用进缓冲并落盘（provider 名称改动走重命名）。
    pub(crate) fn mj_apply_provider(&mut self, old_name: String, cx: &mut Context<Self>) {
        self.mj_apply_provider_inner(old_name, true, cx);
    }

    /// 测速自动保存专用（P2-3）：`honor_form_name=false` 时表单改名**不生效**
    /// ——按盘上老名保存（改名走头行【保存】，测速只保存连通性字段）。
    pub(super) fn mj_apply_provider_inner(
        &mut self,
        old_name: String,
        honor_form_name: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let (name, base, key) = {
            let s = st.read(cx);
            (
                s.mj_name.read(cx).value().trim().to_string(),
                s.mj_base.read(cx).value().trim().to_string(),
                s.mj_key.read(cx).value().trim().to_string(),
            )
        };
        if name.is_empty() {
            return self.mc_set_error(tr("custom_models.name_required"), cx);
        }
        // 落盘名：表单名优先；测速路径（honor=false）对已存在的商按老名
        // 保存，表单改名不随测速生效（P2-3）
        let target = if honor_form_name || old_name.is_empty() {
            name.clone()
        } else {
            old_name.clone()
        };
        let mut doc = Self::mj_doc();
        // 新建（草稿）时 old_name 为空：同名已存在就报错，别把人家覆盖了
        if old_name.is_empty() && pi_link::models_json::provider_entry(&doc, &target).is_some() {
            let msg = crate::i18n::tf("custom_models.name_exists", &[("name", target.clone())]);
            return self.mc_set_error(&msg, cx);
        }
        let mut entry = pi_link::models_json::provider_entry(&doc, &old_name)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        // 托管判据（2026-10-31 定稿）：auth.json 有 PF 标记条目（pif-secret
        // 命令引用，人工不可能手写）= PF 创建的条目；草稿（old_name 空）=
        // 本次新建，天然归 PF 管。**落盘动作按托管分流**：
        // - 托管：api 随 baseUrl 推断刷新，key 走凭据分流（051）
        // - 手写：只写表单真正拥有的键（name/baseUrl），其余原样——显式
        //   api 不被推断覆盖（openai-responses 等推断不了的形状）；apiKey
        //   不转存凭据库、不写标记、遗留引用不清
        let managed = old_name.is_empty() || mj_managed(&old_name);
        if managed {
            // API 形状 PF 推断（042-1 改版：baseUrl 决定协议，用户零选择）。
            // pi 对缺 api 硬 throw，必须写具体值；空 base 先落 openai 形状，
            // 填好 base 再保存即随推断更新
            entry["api"] = serde_json::Value::String(pi_link::probe::infer_api(&base).to_string());
        }
        // name 字段随输入同步（顺带保证新建条目 sanitize 不丢）
        set_opt_str(&mut entry, "name", &target);
        // 尾斜杠规范化（pi 运行时直接拼 base+path，避免 /key//path）；
        // 探测端本就 trim，这里统一落盘形状
        set_opt_str(&mut entry, "baseUrl", base.trim_end_matches('/'));
        // apiKey 分流（051）：明文 → 凭据库 + $PF_KEY_* 引用；高级引用原样；
        // 库不可用降级明文。手写条目跳过——用户的明文 key 原样留在盘上
        if managed {
            self.mj_store_key(&mut entry, &target, &key, cx);
        }
        // 草稿（old_name 为空）= 新建，没有旧键可改名；测速路径不改名
        let renamed = !old_name.is_empty() && target != old_name;
        if renamed {
            if let Err(e) = pi_link::models_json::rename_provider(&mut doc, &old_name, &target) {
                return self.mc_set_error(&e, cx);
            }
            // 凭据跟名搬家：钥匙库条目 + auth.json 标记条目（无旧条目静默
            // 跳过）。手写条目跳过——它的 key 在 models.json 里随条目搬，
            // 不得把用户明文收进凭据库
            if managed {
                if let Some(secret) = pi_link::auth_json::resolve_secret(&old_name) {
                    if pi_link::auth_json::store_secret(&target, &secret).is_ok() {
                        let _ = pi_link::auth_json::delete_secret(&old_name);
                    }
                }
            }
        }
        pi_link::models_json::upsert_provider(&mut doc, &target, entry);
        self.mj_write(&doc, cx);
        // 改名同步内存目录（P2-4）：磁盘现读重建新名条目，再整体摘除老名
        //（顺序保证 models-store.json 里万一残留的老名缓存也被滤掉）——
        // 否则可用模型区按新名过滤为 0 条、模型选择器残留旧商名直到重启。
        // 凭据快照同步重读（侧栏成员资格含「有凭据」支）。
        if renamed {
            self.refresh_provider_catalog(&target);
            self.drop_provider_from_catalog(&old_name);
            // 探测展示缓存键跟名搬家（先写盘再 reload——reload 会从盘重读镜像）
            if let Some(probe) = self.cfg.mj_probe_cache.remove(&old_name) {
                self.cfg.mj_probe_cache.insert(target.clone(), probe);
                let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
            }
            self.reload_settings_panel();
        }
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = format!("mj:p:{target}");
                cx.notify();
            });
        }
        cx.notify();
    }

    /// 【删除】入口（042-1 UI）：只置确认状态，真正删除在
    /// [`Self::mj_delete_provider_close`]。删除 = 删自定义商条目 + 其下全部
    /// 模型 + 凭据条目（与内置商的【移除】不同，移除只让商家不显示）。
    pub(crate) fn mj_ask_delete_provider(&mut self, name: String, cx: &mut Context<Self>) {
        self.cfg.mc_mj_delete_confirm = Some(name);
        cx.notify();
    }

    pub(crate) fn mj_delete_provider_close(&mut self, ok: bool, cx: &mut Context<Self>) {
        let Some(name) = self.cfg.mc_mj_delete_confirm.take() else {
            return;
        };
        if !ok {
            cx.notify();
            return;
        }
        self.mj_delete_provider(name, cx);
    }

    pub(crate) fn mj_delete_provider(&mut self, name: String, cx: &mut Context<Self>) {
        let mut doc = Self::mj_doc();
        pi_link::models_json::remove_provider(&mut doc, &name);
        // 凭据一并清：api_key 条目（不论谁写的——用户刚确认删除该商）+ 钥匙库
        // 条目；oauth 归 pi 不动
        let _ = pi_link::auth_json::delete_api_key(&name);
        self.cfg.mj_speed = None;
        self.mj_write(&doc, cx);
        // 探测展示缓存一并清（商没了，缓存条目无意义）
        if self.cfg.mj_probe_cache.remove(&name).is_some() {
            let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
        }
        // enabledModels 里该商名下的 pattern 一并清（`ds/*`、`ds/x`、带 :pin）——
        // 此前漏了，删掉的商在 settings.json 里留一堆死条目
        self.mc_prune_patterns(&name, None, cx);
        // 内存目录同步摘掉（否则进程答案 / 启动 globals 里的模型会让列表项
        // 继续挂着）。列表成员资格 = models.json 里有没有条目，无隐藏清单。
        self.drop_provider_from_catalog(&name);
        // 凭据快照（cfg.mc_creds）也要重读：侧栏成员资格含「有凭据」这一支，
        // 不重读会让已删的商凭旧快照继续挂在列表上
        self.reload_settings_panel();
        if let Some(st) = self.dlg.settings.clone() {
            let next = pi_link::models_json::providers(&doc)
                .first()
                .map(|(n, _)| format!("mj:p:{n}"))
                .unwrap_or_default();
            st.update(cx, |s, cx| {
                s.section = next;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// apiKey 分流（042-1）：PF 保存的 key 只进钥匙库 + auth.json 标记条目，
    /// **`models.json` 里不写 `apiKey`**（自定义商与内置商走同一条凭据路径）。
    /// 只清理 PF 自己以前写下的 `$PF_KEY_*` 遗留引用；用户手写的明文 / `$VAR`
    /// / `!cmd` 一律原样保留（规则 1）。空串 = 本次不动 key。
    fn mj_store_key(
        &mut self,
        entry: &mut serde_json::Value,
        name: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        let key = key.trim();
        if key.is_empty() {
            return;
        }
        if let Err(e) = pi_link::auth_json::store_secret(name, key) {
            self.mc_set_error(&crate::i18n::tf("models.failed_to_save", &[("e", e)]), cx);
            return;
        }
        let legacy_pf_ref = entry
            .get("apiKey")
            .and_then(|v| v.as_str())
            .is_some_and(|v| pi_link::credentials::pf_ref(v).is_some());
        if legacy_pf_ref {
            if let Some(obj) = entry.as_object_mut() {
                obj.remove("apiKey");
            }
        }
    }
}
