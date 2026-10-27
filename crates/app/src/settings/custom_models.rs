//! models.json 自定义 provider / 模型编辑器（设置·模型页右栏的 mj_* 分支）。
//! 042 定稿：表单头【保存】/【移除】（043 同款），点保存即写盘——无页面级
//! footer；编辑缓冲在 Chat.mc_models_json，成功落盘后缓冲与磁盘一致。
//! 042 M2 对齐 pi-web ProviderDetail/ModelDetail：headers、导入模型
//!（discovery 多选）、模型测试、models.dev 填入模型信息、API 覆盖 /
//! 兼容（DeepSeek / developer role）/ 思考等级映射。

use super::*;
use pi_link::credentials::SecretVault;

impl Chat {
    pub(crate) fn mj_select(&mut self, section: String, cx: &mut Context<Self>) {
        let (name, ix) = mj_parse_section(&section);
        let entry = pi_link::models_json::provider_entry(&self.cfg.mc_models_json, &name)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let model = ix
            .and_then(|ix| entry.get("models").and_then(|m| m.get(ix)).cloned())
            .unwrap_or_else(|| serde_json::json!({}));
        let get_str = |v: &serde_json::Value, k: &str| {
            v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
        };
        // 能力勾选回填：input 数组含 image/video（文本恒选不落 UI 态）
        let in_input = |key: &str| {
            model
                .get("input")
                .and_then(|v| v.as_array())
                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(key)))
        };
        // 换选中项 = 换被测模型，旧测试结果作废；测速态按"已有商 = 已通过"
        // 种子（042-1：不强迫重测；新商无 baseUrl = 未测，保存/+模型禁用）
        self.cfg.mj_test = None;
        self.cfg.mj_speed = if entry
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
        {
            Some(MjSpeedState::Passed {
                latency_ms: 0,
                models: 0,
                seeded: true,
            })
        } else {
            None
        };
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = section.clone();
                s.error = None;
                s.mj_image = in_input("image");
                s.mj_video = in_input("video");
                s.mj_name
                    .update(cx, |ti, cx| ti.set_value(name.clone(), cx));
                s.mj_base
                    .update(cx, |ti, cx| ti.set_value(get_str(&entry, "baseUrl"), cx));
                s.mj_key_visible = false;
                // key 回填对齐内置商（mc_select_provider）：$PF_KEY_* 引用经
                // 凭据库解出明文预填（掩码态眼睛复核）；解不出（库缺条目/
                // 库错）回退引用原文；降级明文与用户高级引用（$VAR/!cmd）原样
                let key_raw = get_str(&entry, "apiKey");
                let key_display = match pi_link::credentials::classify(&key_raw) {
                    pi_link::credentials::StoreMode::Vault => {
                        pi_link::credentials::pf_ref(&key_raw)
                            .and_then(|v| {
                                pi_link::credentials::KeyringVault.get(v).ok().flatten()
                            })
                            .unwrap_or(key_raw)
                    }
                    _ => key_raw,
                };
                s.mj_key.update(cx, |ti, cx| {
                    ti.set_value(key_display, cx);
                    ti.set_masked(true, cx);
                });
                s.mj_id
                    .update(cx, |ti, cx| ti.set_value(get_str(&model, "id"), cx));
                let ctx = model
                    .get("contextWindow")
                    .and_then(|v| v.as_u64())
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                s.mj_ctx.update(cx, |ti, cx| ti.set_value(ctx, cx));
                let max_tokens = model
                    .get("maxTokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                s.mj_max_tokens
                    .update(cx, |ti, cx| ti.set_value(max_tokens, cx));
                cx.notify();
            });
        }
    }

    /// Skills toggle: write `disable-model-invocation` into SKILL.md
    /// (pi-web PATCH /api/skills parity).
    pub(crate) fn mc_cli_op(&mut self, args: Vec<String>, done: String, cx: &mut Context<Self>) {
        let Some(tx) = self.cfg.op_tx.clone() else {
            return;
        };

        // 无项目态：pi CLI 安装/卸载是全局操作，进程 cwd 落用户主目录
        //（只是一次性子进程的工作目录，不是项目语义，不入任何记忆）
        let cwd = self.ws.cwd.clone().unwrap_or_else(|| {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
        });
        std::thread::spawn(move || {
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = match pi_link::vendor::run_cli(&cwd, &arg_refs) {
                Ok(_) => done,
                Err(e) => crate::i18n::tf(
                    tr("extensions.pi_failed"),
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

    /// 弹窗「自定义」卡进来：直接在账本创建 new-provider（重名自动
    /// `-1`/`-2`，pi-web addCustomProvider 同规则），落盘并进详情表单。
    /// 条目带 `name` 字段保证 sanitize 不丢；测速通过前保存/+模型禁用
    ///（042-1），名字在头行输入框改。
    pub(crate) fn mj_open_add(&mut self, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        self.cfg.mj_test = None;
        let existing: Vec<String> = pi_link::models_json::providers(&self.cfg.mc_models_json)
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        let mut name = "new-provider".to_string();
        let mut n = 1;
        while existing.iter().any(|e| e == &name) {
            name = format!("new-provider-{n}");
            n += 1;
        }
        pi_link::models_json::upsert_provider(
            &mut self.cfg.mc_models_json,
            &name,
            serde_json::json!({ "name": name }),
        );
        self.mj_flush(cx);
        self.mj_select(format!("mj:p:{name}"), cx);
        cx.notify();
    }

    /// 缓冲落盘（042：表单头保存即写盘；失败挂详情错误条，缓冲保留重试）。
    fn mj_flush(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = pi_link::pf_providers::write(&self.cfg.mc_models_json) {
            self.mc_set_error(&e, cx);
        }
    }

    /// 测速/刷新落目录后，把目录模型合并进账本 provider.models[]（042-1
    /// 改版：Provider = baseUrl + apiKey 即含模型）。pi 注册的 provider
    /// 若零模型，会话无模型可选——展示缓存（provider-catalog）不进账本
    /// 正是「测速拿到模型但聊天没反应」的根因。按 id 去重：已有条目
    /// （手调过参数的）原样保留，缺的目录 id 追加 {id, name?, contextWindow?}。
    fn mj_merge_catalog_into_ledger(
        doc: &mut serde_json::Value,
        provider: &str,
        catalog: &[pi_link::protocol::ModelInfo],
    ) {
        let Some(mut entry) = pi_link::models_json::provider_entry(doc, provider).cloned() else {
            return;
        };
        let existing: Vec<String> = pi_link::models_json::provider_models(&entry)
            .into_iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
        let mut fresh: Vec<serde_json::Value> = Vec::new();
        for m in catalog {
            if existing.contains(&m.id) || fresh.iter().any(|e| e["id"] == m.id) {
                continue;
            }
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), serde_json::Value::String(m.id.clone()));
            if !m.name.is_empty() {
                obj.insert("name".into(), serde_json::Value::String(m.name.clone()));
            }
            if let Some(cw) = m.context_window {
                obj.insert("contextWindow".into(), serde_json::json!(cw));
            }
            // pi 对扩展注册模型要求完整形状（缺 input/reasoning/cost 聊天即崩）
            let mut model = serde_json::Value::Object(obj);
            pi_link::models_json::ensure_model_defaults(&mut model);
            fresh.push(model);
        }
        if fresh.is_empty() {
            return;
        }
        if !entry.get("models").is_some_and(|m| m.is_array()) {
            entry["models"] = serde_json::json!([]);
        }
        if let Some(arr) = entry.get_mut("models").and_then(|m| m.as_array_mut()) {
            arr.extend(fresh);
        }
        pi_link::models_json::upsert_provider(doc, provider, entry);
    }

    /// 把编辑器字段应用进缓冲并落盘（provider 名称改动走重命名）。
    pub(crate) fn mj_apply_provider(&mut self, old_name: String, cx: &mut Context<Self>) {
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
        let mut entry = pi_link::models_json::provider_entry(&self.cfg.mc_models_json, &old_name)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        // API 形状 PF 推断（042-1 改版：baseUrl 决定协议，用户零选择）。
        // pi 对缺 api 硬 throw，必须写具体值；空 base 先落 openai 形状，
        // 填好 base 再保存即随推断更新
        entry["api"] =
            serde_json::Value::String(pi_link::probe::infer_api(&base).to_string());
        // name 字段随输入同步（顺带保证新建条目 sanitize 不丢）
        set_opt_str(&mut entry, "name", &name);
        // 尾斜杠规范化（pi 运行时直接拼 base+path，避免 /key//path）；
        // 探测端本就 trim，这里统一落盘形状
        set_opt_str(&mut entry, "baseUrl", base.trim_end_matches('/'));
        // apiKey 分流（051）：明文 → 凭据库 + $PF_KEY_* 引用；高级引用原样；
        // 库不可用降级明文
        self.mj_store_key(&mut entry, &name, &key, cx);
        if name != old_name {
            if let Err(e) = pi_link::models_json::rename_provider(
                &mut self.cfg.mc_models_json,
                &old_name,
                &name,
            ) {
                return self.mc_set_error(&e, cx);
            }
            // 凭据库条目跟名搬家（无旧条目则静默跳过）
            pi_link::pf_providers::rename_provider_key(
                &old_name,
                &name,
                &pi_link::credentials::KeyringVault,
            );
        }
        pi_link::models_json::upsert_provider(&mut self.cfg.mc_models_json, &name, entry);
        self.mj_flush(cx);
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = format!("mj:p:{name}");
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(crate) fn mj_delete_provider(&mut self, name: String, cx: &mut Context<Self>) {
        pi_link::models_json::remove_provider(&mut self.cfg.mc_models_json, &name);
        // 凭据库条目一并清（幂等；条目不存在是 Ok）
        let _ =
            pi_link::credentials::KeyringVault.delete(&pi_link::credentials::env_var_name(&name));
        self.cfg.mj_speed = None;
        self.mj_flush(cx);
        if let Some(st) = self.dlg.settings.clone() {
            let next = pi_link::models_json::providers(&self.cfg.mc_models_json)
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

    /// apiKey 分流（051）：明文 → 凭据库 + `$PF_KEY_*` 引用；`$`/`!` 高级
    /// 引用原样；凭据库失败 → 降级明文并提示。空串清除字段。
    fn mj_store_key(
        &mut self,
        entry: &mut serde_json::Value,
        name: &str,
        key: &str,
        cx: &mut Context<Self>,
    ) {
        if let Ok(Some(pi_link::credentials::StoreMode::File)) =
            pi_link::pf_providers::set_provider_key(
                entry,
                name,
                key,
                &pi_link::credentials::KeyringVault,
            )
        {
            self.mc_set_error(tr("conflict.vault_unavailable"), cx);
        }
    }

    pub(crate) fn mj_add_model(&mut self, provider: String, cx: &mut Context<Self>) {
        let ix = pi_link::models_json::add_model(&mut self.cfg.mc_models_json, &provider);
        self.mj_flush(cx);
        self.mj_select(format!("mj:m:{provider}:{ix}"), cx);
    }

    /// 把模型编辑器字段写回缓冲里的模型条目（精简版：编辑器只拥有 id /
    /// 上下文窗口 / 最大输出 / 能力勾选四个键；名称、价格、reasoning、兼容
    /// 等交【自动填参】从 models.dev 落——既有值原样保留不碰）。
    pub(crate) fn mj_apply_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        if !self.mj_commit_model_form(&provider, ix, cx) {
            return;
        }
        self.mj_flush(cx);
        cx.notify();
    }

    /// 表单四键落进缓冲（保存与【自动填参】共用）。返回 false = 模型ID 为空
    /// （已置错）或面板不在。
    /// 【自动填参】必须先走这一步：回填以本条模型为查询键、且末尾会把表单
    /// 按缓冲重播——不先落表单值就用旧 id 查目录，还会把输入框改回旧值。
    fn mj_commit_model_form(
        &mut self,
        provider: &str,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(st) = self.dlg.settings.clone() else {
            return false;
        };
        let (id, ctx, image, video, max_tokens) = {
            let s = st.read(cx);
            (
                s.mj_id.read(cx).value().trim().to_string(),
                s.mj_ctx.read(cx).value().trim().to_string(),
                s.mj_image,
                s.mj_video,
                s.mj_max_tokens.read(cx).value().trim().to_string(),
            )
        };
        if id.is_empty() {
            self.mc_set_error(tr("custom_models.model_id_required"), cx);
            return false;
        }
        let Some(entry) =
            pi_link::models_json::provider_entry(&self.cfg.mc_models_json, provider).cloned()
        else {
            return false;
        };
        // 同商下不允许**新增**重复模型 ID：pi 的启用键是 provider/id，两条同 ID
        // 条目只能共用一个开关（列表两边一起跳），存了也是脏数据。
        // 仅拦“改成重复”：存量已是重复的条目（stored == id）照常可改可存。
        let stored_id = entry
            .get("models")
            .and_then(|m| m.get(ix))
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let dup = stored_id != id
            && entry
                .get("models")
                .and_then(|m| m.as_array())
                .is_some_and(|a| {
                    a.iter().enumerate().any(|(i, m)| {
                        i != ix && m.get("id").and_then(|v| v.as_str()) == Some(id.as_str())
                    })
                });
        if dup {
            let msg = crate::i18n::tf("custom_models.model_id_duplicate", &[("id", id.clone())]);
            self.mc_set_error(&msg, cx);
            return false;
        }
        let mut model = entry
            .get("models")
            .and_then(|m| m.get(ix))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        // 全部键改动走整块替换（不在 obj 借用上跨调用，避免双重可变借用）
        if !model.is_object() {
            model = serde_json::json!({});
        }
        let mut obj = model.as_object().cloned().unwrap_or_default();
        obj.insert("id".into(), serde_json::Value::String(id));
        if let Ok(n) = ctx.parse::<u64>() {
            obj.insert("contextWindow".into(), serde_json::json!(n));
        } else {
            obj.remove("contextWindow");
        }
        if let Ok(n) = max_tokens.parse::<u64>() {
            obj.insert("maxTokens".into(), serde_json::json!(n));
        } else {
            obj.remove("maxTokens");
        }
        // 能力：文本恒在；仅 text = 删键走 ensure_model_defaults 缺省
        let mut input = vec!["text"];
        if image {
            input.push("image");
        }
        if video {
            input.push("video");
        }
        if input.len() > 1 {
            obj.insert("input".into(), serde_json::json!(input));
        } else {
            obj.remove("input");
        }
        let mut model = serde_json::Value::Object(obj);
        // pi 对扩展注册模型要求完整形状（缺 input/reasoning/cost 聊天即崩）：
        // 编辑器删键后的缺省在此统一补齐
        pi_link::models_json::ensure_model_defaults(&mut model);
        pi_link::models_json::update_model(&mut self.cfg.mc_models_json, provider, ix, model);
        true
    }

    pub(crate) fn mj_delete_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        pi_link::models_json::remove_model(&mut self.cfg.mc_models_json, &provider, ix);
        self.mj_flush(cx);
        self.mj_select(format!("mj:p:{provider}"), cx);
    }

    /// 能力勾选切换（图片 / 视频；文本恒选无动作）。
    pub(crate) fn mj_toggle_image(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_image = !s.mj_image;
                cx.notify();
            });
        }
    }

    pub(crate) fn mj_toggle_video(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_video = !s.mj_video;
                cx.notify();
            });
        }
    }

    // -- 导入模型（discovery）------------------------------------------------

    /// 表单里的 key 按引用规则解出明文（$PF_KEY_* → 凭据库；明文原样；
    /// 其他 $VAR / !cmd 解不出 → Err）。
    fn mj_form_key(key_text: &str) -> Result<String, String> {
        match pi_link::credentials::classify(key_text) {
            pi_link::credentials::StoreMode::File => Ok(key_text.to_string()),
            pi_link::credentials::StoreMode::Vault => pi_link::credentials::pf_ref(key_text)
                .and_then(|v| {
                    pi_link::credentials::KeyringVault.get(&v).ok().flatten()
                })
                .ok_or_else(|| tr("model.key_not_in_vault").to_string()),
            pi_link::credentials::StoreMode::Ref => {
                Err(tr("models.adv_ref_unresolved").to_string())
            }
        }
    }

    /// 测速（042-1）：读**表单**值打一次列模型 GET。能通 = ① 自动保存
    /// 全表单（含改名）② 目录落 provider-catalog 并刷新内存，可用模型区
    /// 即满；响应无模型条目 = 已保存但提示手动 +模型；不通 = 红条错误，
    /// 保存/+模型保持禁用。auto（api 缺省）按 openai 形状猜端点。
    pub(crate) fn mj_speedtest_start(&mut self, provider: String, cx: &mut Context<Self>) {
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let base = {
            let s = st.read(cx);
            s.mj_base.read(cx).value().trim().to_string()
        };
        let key_text = st.read(cx).mj_key.read(cx).value().trim().to_string();
        // API 形状随 baseUrl 推断（042-1 改版）：anthropic 兼容端点自动获得
        // 目录优先 + messages POST 兜底的组合探查
        let api = pi_link::probe::infer_api(&base);
        self.mc_clear_error(cx);
        let Some((url, auth)) = pi_link::probe::custom_target(&base, &api) else {
            return self.mc_set_error(tr("models.import_base_url_required"), cx);
        };
        let key = match Self::mj_form_key(&key_text) {
            Ok(k) => k,
            Err(e) => return self.mc_set_error(&e, cx),
        };
        // base 指向 Anthropic 兼容端点：目录优先 GET /v1/models，messages
        // POST 只作连通兜底（DeepSeek /anthropic 等；直打 messages 永远空目录）
        let compat = pi_link::probe::anthropic_compat_urls(&base);
        self.cfg.mj_speed = Some(MjSpeedState::Running);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let provider_bg = provider;
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    match compat {
                        Some((models, messages)) => {
                            pi_link::probe::probe_anthropic_compat(&models, &messages, &key)
                        }
                        None => pi_link::probe::probe(&url, auth, &key),
                    }
                })
                .await;
            let _ = this.update(cx, |c, cx| {
                match outcome {
                    pi_link::probe::ProbeOutcome::Ok {
                        catalog, latency_ms, ..
                    } => {
                        // 1、自动保存（表单全字段 + 头行输入框可能的改名）
                        if let Some(st) = c.dlg.settings.clone() {
                            let old = st.read(cx).mj_name.read(cx).value().trim().to_string();
                            c.mj_apply_provider(old, cx);
                        }
                        // 2、自动拉取：目录直接合并进账本 models[]（自定义商
                        // 唯一真相源；provider-catalog 缓存只服务内置商，
                        // 这里顺带清键防旧 id 复显）+ 刷新内存（可用模型区即满）
                        if catalog.is_empty() {
                            c.cfg.mj_speed = Some(MjSpeedState::PassedEmpty);
                        } else {
                            Self::mj_merge_catalog_into_ledger(
                                &mut c.cfg.mc_models_json,
                                &provider_bg,
                                &catalog,
                            );
                            c.mj_flush(cx);
                            let _ = pi_link::catalog::save_catalog(&provider_bg, &[]);
                            c.refresh_provider_catalog(&provider_bg);
                            c.cfg.mj_speed = Some(MjSpeedState::Passed {
                                latency_ms,
                                models: catalog.len(),
                                seeded: false,
                            });
                        }
                    }
                    pi_link::probe::ProbeOutcome::AuthRejected { status } => {
                        c.cfg.mj_speed = Some(MjSpeedState::Failed(crate::i18n::tf(
                            "extensions.fetch_key_rejected",
                            &[("s", status.to_string())],
                        )));
                    }
                    pi_link::probe::ProbeOutcome::Unreachable { error } => {
                        c.cfg.mj_speed = Some(MjSpeedState::Failed(crate::i18n::tf(
                            "extensions.fetch_failed",
                            &[("e", error)],
                        )));
                    }
                }
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |_, cx| cx.notify());
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 可用模型区头行的刷新钮（内置详情同位）：按**已保存**条目重新拉取
    /// 该商模型目录（probe → 合并进账本 → 刷新内存）；失败挂错误条。
    pub(crate) fn mj_fetch_models(&mut self, provider: String, cx: &mut Context<Self>) {
        let Some(entry) =
            pi_link::models_json::provider_entry(&self.cfg.mc_models_json, &provider).cloned()
        else {
            return;
        };
        let base = entry
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // 存量显式 api 尊重保留，缺省按 baseUrl 推断（042-1 改版）
        let api = entry
            .get("api")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| pi_link::probe::infer_api(&base).to_string());
        let key_text = entry
            .get("apiKey")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.mc_clear_error(cx);
        let Some((url, auth)) = pi_link::probe::custom_target(&base, &api) else {
            return self.mc_set_error(tr("models.import_base_url_required"), cx);
        };
        let key = match Self::mj_form_key(&key_text) {
            Ok(k) => k,
            Err(e) => return self.mc_set_error(&e, cx),
        };
        // 与测速同款：Anthropic 兼容端点目录优先，messages POST 兜底
        let compat = pi_link::probe::anthropic_compat_urls(&base);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    match compat {
                        Some((models, messages)) => {
                            pi_link::probe::probe_anthropic_compat(&models, &messages, &key)
                        }
                        None => pi_link::probe::probe(&url, auth, &key),
                    }
                })
                .await;
            let _ = this.update(cx, |c, cx| match outcome {
                pi_link::probe::ProbeOutcome::Ok { catalog, .. } if !catalog.is_empty() => {
                    // 目录直接合并进账本（自定义商唯一真相源），缓存键清掉
                    Self::mj_merge_catalog_into_ledger(
                        &mut c.cfg.mc_models_json,
                        &provider,
                        &catalog,
                    );
                    c.mj_flush(cx);
                    let _ = pi_link::catalog::save_catalog(&provider, &[]);
                    c.refresh_provider_catalog(&provider);
                    cx.notify();
                }
                pi_link::probe::ProbeOutcome::Ok { .. } => {
                    let msg = tr("models.autofetch_failed_hint").to_string();
                    c.mc_set_error(&msg, cx);
                }
                pi_link::probe::ProbeOutcome::AuthRejected { status } => {
                    c.mc_set_error(
                        &crate::i18n::tf(
                            "extensions.fetch_key_rejected",
                            &[("s", status.to_string())],
                        ),
                        cx,
                    );
                }
                pi_link::probe::ProbeOutcome::Unreachable { error } => {
                    c.mc_set_error(
                        &crate::i18n::tf("extensions.fetch_failed", &[("e", error)]),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    // -- 模型「测试」----------------------------------------------------------

    /// 测试连接：读**表单**（provider 头 + 模型体）发一条最小补全。
    pub(crate) fn mj_test_start(&mut self, provider: String, _ix: usize, cx: &mut Context<Self>) {
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let (model_id, base, key_text) = {
            let s = st.read(cx);
            (
                s.mj_id.read(cx).value().trim().to_string(),
                s.mj_base.read(cx).value().trim().to_string(),
                s.mj_key.read(cx).value().trim().to_string(),
            )
        };
        // API 随表单 baseUrl 推断（042-1 改版，与测速同源）
        let api = pi_link::probe::infer_api(&base).to_string();
        // provider 级 headers 从缓冲合并（模型级 headers 随编辑器精简退场，
        // 既有条目的 model.headers 原样保留不碰）
        let headers: Vec<(String, String)> = pi_link::models_json::provider_entry(
            &self.cfg.mc_models_json,
            &provider,
        )
        .and_then(|e| e.get("headers").and_then(|h| h.as_object()))
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
        self.mc_clear_error(cx);
        if model_id.is_empty() {
            return self.mc_set_error(tr("custom_models.model_id_required"), cx);
        }
        let Some(req) = pi_link::model_test::build_request(&base, &api, &model_id) else {
            return self.mc_set_error(tr("models.import_base_url_required"), cx);
        };
        let key = match Self::mj_form_key(&key_text) {
            Ok(k) => k,
            Err(e) => return self.mc_set_error(&e, cx),
        };
        self.cfg.mj_test = Some(MjTestState::Running);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { pi_link::model_test::send(&req, &key, &headers) })
                .await;
            let _ = this.update(cx, |c, cx| {
                c.cfg.mj_test = Some(match outcome {
                    pi_link::model_test::TestOutcome::Ok { latency_ms, status, text } => {
                        MjTestState::Ok {
                            latency_ms,
                            status,
                            text,
                        }
                    }
                    pi_link::model_test::TestOutcome::Err { message, .. } => {
                        MjTestState::Err(message)
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    // -- 填入模型信息（models.dev）--------------------------------------------

    /// 【自动填参】：点一次就真拉一次 models.dev（强制刷新，不看缓存 TTL；
    /// 网络挂了回退旧缓存），再按当前表单里的模型 ID 取预设**覆盖**回填
    ///（上下文/最大输出/能力/名称/价格都跟当前 ID 走）并重选回显。
    pub(crate) fn mj_catalog_fill(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        // 先落表单值：回填以它为查询键，且回填末尾按缓冲重播表单——
        // 不先落就会拿旧 ID 查、还把输入框改回旧值。
        if !self.mj_commit_model_form(&provider, ix, cx) {
            return;
        }
        self.cfg.mj_models_dev_state = MjCatalogState::Running;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async { pi_link::models_dev::load_fresh() })
                .await;
            let _ = this.update(cx, |c, cx| match loaded {
                Ok(entries) => {
                    c.cfg.mj_models_dev_state = MjCatalogState::Idle;
                    // 等目录期间用户可能又改了表单：再落一次，回填不吃回刚敲的值
                    if c.mj_commit_model_form(&provider, ix, cx) {
                        c.mj_catalog_apply(&entries, &provider, ix, cx);
                    }
                }
                Err(e) => {
                    c.cfg.mj_models_dev_state = MjCatalogState::Err(e);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn mj_catalog_apply(
        &mut self,
        entries: &[pi_link::models_dev::CatalogEntry],
        provider: &str,
        ix: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) =
            pi_link::models_json::provider_entry(&self.cfg.mc_models_json, provider).cloned()
        else {
            return;
        };
        let Some(model) = entry.get("models").and_then(|m| m.get(ix)).cloned() else {
            return;
        };
        let query = model
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let base_url = entry
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let rec = pi_link::models_dev::recommend(entries, &query, provider, base_url);
        // 点填参就是“把这个 ID 的参数拉下来填上”：目录派生字段（名称/思考/
        // 能力/上下文/最大输出/价格）一律按预设覆盖，旧值属于上一个模型。
        let mut model = model;
        let applied = pi_link::models_dev::fill_from_preset(&mut model, &rec.preset);
        pi_link::models_json::update_model(
            &mut self.cfg.mc_models_json,
            provider,
            ix,
            model,
        );
        self.mj_flush(cx);
        let note = mj_price_note(&rec.price);
        self.cfg.mj_models_dev_state = MjCatalogState::Done { applied, note };
        // 表单重选回显（新字段进输入框）
        self.mj_select(format!("mj:m:{provider}:{ix}"), cx);
        cx.notify();
    }
}

// ---------------------------------------------------------------------------

/// 价格可靠度提示文案（pi-web catalogResultSummary 的 price 段对齐）。
fn mj_price_note(price: &Option<pi_link::models_dev::Price>) -> String {
    match price {
        Some(pi_link::models_dev::Price::Reliable { method, .. }) => match method {
            pi_link::models_dev::MatchMethod::Provider => tr("models.catalog_price_provider").into(),
            pi_link::models_dev::MatchMethod::BaseUrl => tr("models.catalog_price_baseurl").into(),
            pi_link::models_dev::MatchMethod::Consensus => tr("models.catalog_price_consensus").into(),
        },
        Some(pi_link::models_dev::Price::Unreliable { reason, .. }) => match *reason {
            "no-exact-match" => tr("models.catalog_no_exact_match").into(),
            "no-valid-price" => tr("models.catalog_price_unreliable").into(),
            _ => tr("models.catalog_price_unreliable").into(),
        },
        None => String::new(),
    }
}

// section key helpers
// ---------------------------------------------------------------------------

/// "mj:p:name" → (name, None)；"mj:m:name:3" → (name, Some(3))；其他 → ("", None)
fn mj_parse_section(section: &str) -> (String, Option<usize>) {
    if let Some(rest) = section.strip_prefix("mj:m:") {
        if let Some((name, ix)) = rest.rsplit_once(':') {
            return (name.to_string(), ix.parse().ok());
        }
        return (rest.to_string(), None);
    }
    if let Some(name) = section.strip_prefix("mj:p:") {
        return (name.to_string(), None);
    }
    (String::new(), None)
}

/// 空串删除可选字符串键，其余写入（保序）。
fn set_opt_str(obj: &mut serde_json::Value, key: &str, value: &str) {
    let map = obj.as_object_mut().expect("entry is an object");
    if value.is_empty() {
        map.remove(key);
    } else {
        map.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

/// 自定义 Provider 详情页（042-1 定稿：**复用内置 Provider 详情面板** +
/// 自有组件）：头行 = name input +【移除】→ API Key 段（掩码+眼睛，保存
/// 钮已挪操作栏）→ Base URL → API 下拉（默认 auto）→ 操作栏
/// [测速][保存][+模型] + 注释行 → 高级设置（Headers 折叠）→ 分隔线 →
/// 可用模型区（内置同规格，行可点进模型编辑器）。
pub(crate) fn mj_provider_editor(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    d: &super::SettingsFormData,
    error: &Option<String>,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    // 042-1 门禁：测速通过前【保存】【+模型】禁用
    let edit_ok = chat
        .cfg
        .mj_speed
        .as_ref()
        .is_some_and(|st| st.allows_edit());
    let testing = matches!(chat.cfg.mj_speed, Some(MjSpeedState::Running));

    // 两列表单行：左侧标题列（定宽 120），右侧控件列（042-1 蓝框定稿）
    let form_row = move |label: &str, control: gpui::AnyElement| -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .mb(px(10.))
            .child(
                div()
                    .w(px(120.))
                    .flex_shrink_0()
                    .child(section_title(label)),
            )
            .child(div().flex_1().min_w_0().child(control))
            .into_any_element()
    };

    // name 行：标题列 + 控件列（input 半宽；移除与 name input 同列、右对齐）
    let remove_provider = name.to_string();
    let detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(form_row(
            &tr("field.name"),
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .w_full()
                .child(
                    div()
                        .w(gpui::relative(0.5))
                        .min_w_0()
                        .child(d.mj_name.clone()),
                )
                .child(div().flex_1())
                .child(super::models::detail_button(
                    "mj-del",
                    weak,
                    &tr("action.remove"),
                    crate::ui::button::BtnVariant::Danger,
                    false,
                    move |c, cx| c.mj_delete_provider(remove_provider.clone(), cx),
                ))
                .into_any_element(),
        ))
        .children(error.as_ref().map(|e| error_note(e)));

    // API Key 行：控件全宽（右缘眼睛，掩码显隐已存 key）；保存钮在操作栏
    let weak_eye = weak.clone();
    let detail = detail
        .child(form_row(
            "API Key",
            div()
                .relative()
                .w_full()
                .child(d.mj_key.clone())
                .child(
                    div()
                        .id("mj-key-eye")
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
                                    let input = st.read(cx).mj_key.clone();
                                    let visible = st.update(cx, |s, cx| {
                                        s.mj_key_visible = !s.mj_key_visible;
                                        cx.notify();
                                        s.mj_key_visible
                                    });
                                    input.update(cx, |ti, cx| ti.set_masked(!visible, cx));
                                }
                            });
                        })
                        .child(crate::ui::icon(
                            if d.mj_key_visible { "eye-off" } else { "eye" },
                            14.,
                            t.text_muted,
                        )),
                )
                .into_any_element(),
        ));

    // Base URL 行（全宽）。API 类型已下线（042-1 改版）：形状由 baseUrl
    // 推断落盘，用户零选择；误判走模型编辑器的 API 覆盖逃生门
    let detail = detail.child(form_row(
        &tr("custom_models.base_url"),
        d.mj_base.clone().into_any_element(),
    ));

    // 操作栏 [测速][保存][+模型]（042-1）：测速 = 连通后自动保存 + 自动
    // 拉模型；通过前保存/+模型禁用。结果行/失败提示紧跟其下。
    let detail = detail
        .child(
            div()
                .mt(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(super::models::detail_button(
                    "mj-speedtest",
                    weak,
                    &if testing {
                        tr("models.speed_testing")
                    } else {
                        tr("models.speedtest")
                    },
                    crate::ui::button::BtnVariant::Secondary,
                    testing,
                    {
                        let n = name.to_string();
                        move |c, cx| c.mj_speedtest_start(n.clone(), cx)
                    },
                ))
                .child(super::models::detail_button(
                    "mj-apply",
                    weak,
                    &tr("action.save"),
                    crate::ui::button::BtnVariant::Secondary,
                    !edit_ok,
                    {
                        let old = name.to_string();
                        move |c, cx| c.mj_apply_provider(old.clone(), cx)
                    },
                ))
                .child(super::models::detail_button(
                    "mj-add-model",
                    weak,
                    &format!("+ {}", tr("settings.tab.models")),
                    crate::ui::button::BtnVariant::Secondary,
                    !edit_ok,
                    {
                        let n = name.to_string();
                        move |c, cx| c.mj_add_model(n.clone(), cx)
                    },
                )),
        )
        // 测速结果行（seeded 通过态不显示）
        .children(match chat.cfg.mj_speed.as_ref() {
            Some(MjSpeedState::Running) => vec![note(&tr("models.speed_testing"))],
            Some(MjSpeedState::Passed {
                latency_ms,
                models,
                seeded: false,
            }) => vec![note(&crate::i18n::tf(
                "models.speed_pass",
                &[
                    ("ms", latency_ms.to_string()),
                    ("count", models.to_string()),
                ],
            ))],
            Some(MjSpeedState::PassedEmpty { .. }) => {
                vec![error_note(&tr("models.autofetch_failed_hint"))]
            }
            Some(MjSpeedState::Failed(e)) => vec![error_note(e)],
            _ => Vec::new(),
        })
        // 注释行用 faint 弱化（比 note 再小一号 + 降透明），不抢操作栏视线
        .child(
            div()
                .opacity(0.7)
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(tr("models.speed_note").to_string())),
                ),
        );


    // API 段与可用模型区之间的分隔线（内置详情同款）
    let detail = detail.child(
        div()
            .h(px(1.))
            .w_full()
            .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
    );

    // 可用模型区（内置同规格）
    mj_enabled_section(chat, weak, name, d, t, detail).into_any_element()
}

/// 自定义商的可用模型区（视觉规格对齐内置详情的 [`super::models::mc_enabled_section`]：
/// 标题 N/total + 只看free + [+模型] 钮 + 筛选 + vlist 行开关）。数据源为
/// 统一目录里该自定义商的条目（providers.json 与测速落盘的 provider-catalog
/// 都经 disk_models 合并）；模型行可点进模型编辑器（pi-web 模型行行为）。
fn mj_enabled_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    provider: &str,
    d: &super::SettingsFormData,
    _t: &crate::appearance::theme::Theme,
    detail: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    // 闭包存进元素需要 'static：主题用静态句柄遮蔽参数（mc_enabled_section 同款）
    let t = T();
    let provider_owned = provider.to_string();
    let models: Vec<pi_link::protocol::ModelInfo> = chat
        .catalog_for(chat.project())
        .iter()
        .filter(|m| m.provider == provider)
        .cloned()
        .collect();
    // 同 ID 多条目在 pi 眼里就是一个模型（启用键 = provider/id）：列表按 ID
    // 去重展示——否则两行行共用同一个开关，点一个两个一起动（"联动"根源）
    let mut seen = std::collections::HashSet::new();
    let models: Vec<pi_link::protocol::ModelInfo> = models
        .into_iter()
        .filter(|m| seen.insert(format!("{}/{}", m.provider, m.id)))
        .collect();
    let enabled_set: std::collections::HashSet<String> =
        chat.cfg.mc_state.enabled.iter().cloned().collect();
    let enabled_count = models
        .iter()
        .filter(|m| {
            let r = format!("{}/{}", m.provider, m.id);
            enabled_set.contains(&r)
        })
        .count();
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
                // 只看free：与内置详情共用同一面板状态
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
                        "mj-free-only",
                        &weak_free,
                        d.model_free_only,
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
                // 刷新钮（内置详情同位同款 rotate-ccw 84×32）：按已保存条目
                // 重拉该商模型目录
                let weak_fetch = weak.clone();
                let fp = provider_owned.clone();
                let el = crate::ui::button::button_base(
                    "mj-fetch",
                    crate::ui::BtnSize::Lg,
                    crate::ui::button::BtnVariant::Secondary,
                )
                .w(px(84.))
                .child(crate::ui::icon("rotate-ccw", 16., t.text));
                el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_fetch.update(cx, |c, cx| c.mj_fetch_models(fp.clone(), cx));
                })
                .into_any_element()
            }),
    );

    // 空目录：只有标题行（无提示文字，042-1 定稿）
    if models.is_empty() {
        return detail.child(section);
    }

    // 筛选（>8 个模型时出现；与内置详情同规则）+「只看free」叠加
    let q = d.model_filter_value.trim().to_lowercase();
    let shown: Vec<pi_link::protocol::ModelInfo> = models
        .iter()
        .filter(|m| {
            let hit = q.is_empty()
                || m.id.to_lowercase().contains(&q)
                || m.name.to_lowercase().contains(&q);
            hit && (!d.model_free_only
                || m.id.to_lowercase().contains("free")
                || m.name.to_lowercase().contains("free"))
        })
        .cloned()
        .collect();
    if models.len() > 8 {
        section = section.child(d.model_filter.clone());
    }
    // 行高与内置详情同公式（ui_size 缩放自适应）
    let row_h = (f32::from(crate::appearance::ui_size(11.))
        + f32::from(crate::appearance::ui_size(10.)))
        * 1.25
        + 12.;
    let all_enabled = chat.cfg.mc_state.all_enabled;
    // 账本内已有的模型 id（名字可点进编辑器；不在账本的纯展示行不可点）
    let ledger_ids: std::collections::HashSet<String> =
        pi_link::models_json::provider_entry(&chat.cfg.mc_models_json, provider)
            .map(pi_link::models_json::provider_models)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|mm| mm.get("id").and_then(|v| v.as_str()).map(str::to_string))
            .collect();
    let project_scope = chat.cfg.mc_project_scope;
    let enabled_rows = enabled_set.clone();
    let pins = chat.cfg.mc_state.pins.clone();
    let weak_rows = weak.clone();
    let weak_rows_v = weak_rows.clone();
    let prov = provider.to_string();
    let list = vlist(
        "mj-model-list",
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
            // 点击名字 → 模型编辑器（仅账本内条目可点；开关区不误触详情页）
            let open_ref = (prov.clone(), m.id.clone());
            let weak_row = weak_rows_v.clone();
            let editable = ledger_ids.contains(&m.id);
            div()
                .id(SharedString::from(format!("mj-row-{ix}")))
                .w_full()
                .h(px(row_h))
                .pl(px(12.))
                .pr(px(24.))
                .flex()
                .items_center()
                .gap(px(8.))
                .when(ix > 0, |dd| dd.border_t_1().border_color(rgb(t.border)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .when(editable, |dd| {
                            dd.cursor_pointer().on_mouse_down(MouseButton::Left, {
                                let weak_row = weak_row.clone();
                                move |_, _, cx| {
                                    let _ = weak_row.update(cx, |c, cx| {
                                        let (prov, id) = &open_ref;
                                        let ix = pi_link::models_json::provider_entry(
                                            &c.cfg.mc_models_json,
                                            prov,
                                        )
                                        .map(pi_link::models_json::provider_models)
                                        .unwrap_or_default()
                                        .iter()
                                        .position(|mm| {
                                            mm.get("id").and_then(|v| v.as_str())
                                                == Some(id.as_str())
                                        });
                                        if let Some(ix) = ix {
                                            c.mj_select(format!("mj:m:{prov}:{ix}"), cx);
                                        }
                                    });
                                }
                            })
                        })
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(11.))
                                .line_height(gpui::relative(1.25))
                                .text_color(rgb(t.text))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(if m.name.is_empty() {
                                    m.id.clone()
                                } else {
                                    m.name.clone()
                                })),
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
                    format!("mj-sw-{ix}"),
                    &weak_rows_v,
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

/// 自定义模型详情/编辑小弹窗（500×500 居中，2026-10-27 用户定稿规格）：
/// 标题栏 = chrome 头条 + 下划线（big_card 标准），标题「模型详情」+ ✕；
/// 页脚右对齐 [测试][保存][移除] 三钮同宽 84（间隔 10）；
/// 内容区 = 模型ID（左）+【自动填参】（右）→ 上下文窗口 / 最大输出 →
/// 能力勾选（文本恒选不可改 / 图片 / 视频）→ **唯一一条**提示栏（表单错误 /
/// 测试结果 / 填参结果三选一）。名称、价格、reasoning、
/// 兼容等交【自动填参】从 models.dev 落（内部实现不暴露）。
/// 兼容等交【自动填参】从 models.dev 落（内部实现不暴露）。
/// `section = mj:m:{prov}:{ix}` 即打开态：本函数由 render.rs 顶层挂载，叠在
/// 设置弹窗之上，右栏底图保持 Provider 详情；关闭（✕/点外/ESC）= 退回
/// `mj:p:{prov}`（重播 provider 回填，模态随 section 撤下）。
pub(crate) fn mj_model_modal_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let panel = chat.dlg.settings.as_ref()?;
    let selected = panel.read(cx).section.clone();
    let rest = selected.strip_prefix("mj:m:")?;
    let (name, ix) = {
        let (n, i) = rest.rsplit_once(':')?;
        (n.to_string(), i.parse::<usize>().ok()?)
    };
    let d = super::SettingsFormData::snapshot(panel.read(cx), cx);
    let error = panel.read(cx).error.clone();
    let t = T();
    let weak = cx.entity().downgrade();
    let dismiss = {
        let weak = weak.clone();
        let n = name.clone();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            let _ = weak.update(cx, |c, cx| c.mj_select(format!("mj:p:{n}"), cx));
        }
    };

    // 测试钮即时态 + 状态行文案（表单错误 / 测试结果 / 自动填参结果）
    let test_state = chat.cfg.mj_test.clone();
    let testing = matches!(test_state, Some(MjTestState::Running));
    let test_note: Option<(String, bool)> = match &test_state {
        Some(MjTestState::Running) => Some((tr("models.test_running").to_string(), false)),
        Some(MjTestState::Ok { latency_ms, status, text }) => Some((
            crate::i18n::tf(
                "models.test_passed",
                &[
                    ("ms", latency_ms.to_string()),
                    ("status", status.to_string()),
                    ("text", text.clone()),
                ],
            ),
            true,
        )),
        Some(MjTestState::Err(e)) => Some((e.clone(), false)),
        None => None,
    };
    // 标题栏（chrome 头条 + 下划线，big_card 同规格）：标题「模型详情」+ ✕；
    // 操作按钮不在这里（用户定稿：[测试][保存][移除] 在页脚右对齐）
    let header = div()
        .flex_shrink_0()
        .h(px(40.))
        .pl(px(15.))
        .pr(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .bg(rgb(t.chrome))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x73)))
        .child(section_title(&tr("models.detail_title")))
        .child(div().flex_1())
        .child(crate::ui::overlay::close_btn(
            "mj-model-close",
            t,
            dismiss.clone(),
        ));

    // 页脚：[测试][保存][移除] 右对齐（标准 §5.1 按钮，经 detail_button 三钮
    // 同宽 84）；状态行（表单错误 / 测试 / 自动填参结果）+ 三个设置行在滚动体
    let footer = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(10.))
        .px(px(15.))
        .pb(px(15.))
        .child(super::models::detail_button(
            "mj-test",
            &weak,
            &tr("models.test"),
            crate::ui::button::BtnVariant::Secondary,
            testing,
            {
                let n = name.clone();
                move |c, cx| c.mj_test_start(n.clone(), ix, cx)
            },
        ))
        .child(super::models::detail_button(
            "mj-mapply",
            &weak,
            &tr("action.save"),
            crate::ui::button::BtnVariant::Primary,
            false,
            {
                let n = name.clone();
                move |c, cx| c.mj_apply_model(n.clone(), ix, cx)
            },
        ))
        .child(super::models::detail_button(
            "mj-mdel",
            &weak,
            &tr("action.remove"),
            crate::ui::button::BtnVariant::Danger,
            false,
            {
                let n = name.clone();
                move |c, cx| c.mj_delete_model(n.clone(), ix, cx)
            },
        ));

    // 提示栏**只有一个**（用户定稿）：同一时刻只显示一条，位置在能力行下方。
    // 优先级：表单错误 > 测试结果 > 自动填参结果；查询中提示贴按钮。
    let filling = matches!(chat.cfg.mj_models_dev_state, MjCatalogState::Running);
    let catalog_line: Option<(String, bool)> = match &chat.cfg.mj_models_dev_state {
        MjCatalogState::Done {
            applied,
            note: price_note,
        } => {
            let mut msg =
                crate::i18n::tf("models.catalog_filled", &[("count", applied.to_string())]);
            if !price_note.is_empty() {
                msg.push_str(&format!(" · {price_note}"));
            }
            Some((msg, true))
        }
        MjCatalogState::Err(e) => Some((e.clone(), false)),
        MjCatalogState::Running | MjCatalogState::Idle => None,
    };
    let status_line: Option<(String, bool)> = error
        .as_ref()
        .map(|e| (e.clone(), false))
        .or(test_note.clone())
        .or(catalog_line);

    let detail = detail_shell("mc-detail")
        .p(px(15.))
        // 模型ID（左半列）+ 自动填参（右半列，标准 §5.1 按钮与 input 底缘
        // 对齐）；两列与下行 上下文/最大输出 同宽——ID input 等长对齐
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(field(&tr("custom_models.field_id"), d.mj_id.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_end()
                        .justify_end()
                        .child({
                            let n = name.clone();
                            let el = crate::ui::button::button_base(
                                "mj-autofill",
                                crate::ui::button::BtnSize::Lg,
                                crate::ui::button::BtnVariant::Secondary,
                            )
                            .w(px(84.))
                            .child(SharedString::from(
                                if filling {
                                    tr("models.catalog_filling")
                                } else {
                                    tr("models.autofill")
                                }
                                .to_string(),
                            ));
                            if filling {
                                el.opacity(0.5).into_any_element()
                            } else {
                                let weak_fill = weak.clone();
                                el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_fill.update(cx, |c, cx| {
                                        c.mj_catalog_fill(n.clone(), ix, cx)
                                    });
                                })
                                .into_any_element()
                            }
                        }),
                ),
        )
        // 上下文窗口 / 最大输出
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(field(&tr("custom_models.field_context"), d.mj_ctx.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(field(&tr("models.max_output"), d.mj_max_tokens.clone())),
                ),
        )
        // 能力：文本恒选不可改；图片/视频勾选落 model.input
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(5.))
                .child(section_title(&tr("models.capabilities")))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .child(mj_cap_checkbox(
                            &tr("models.cap_text"),
                            true,
                            &weak,
                            None,
                        ))
                        .child(mj_cap_checkbox(
                            &tr("models.cap_image"),
                            d.mj_image,
                            &weak,
                            Some(|c: &mut Chat, cx: &mut Context<Chat>| c.mj_toggle_image(cx)),
                        ))
                        .child(mj_cap_checkbox(
                            &tr("models.cap_video"),
                            d.mj_video,
                            &weak,
                            Some(|c: &mut Chat, cx: &mut Context<Chat>| c.mj_toggle_video(cx)),
                        )),
                ),
        )
        // 提示栏（唯一一条）：表单错误 / 测试结果 / 填参结果三选一
        .children(status_line.as_ref().map(|(text, ok)| {
            if *ok {
                note(text)
            } else {
                error_note(text)
            }
        }));
    // 弹窗装配：遮罩 + 外点/ESC 关（都退回 Provider 详情）+ 居中 500×500
    // 卡片（窗框对齐 big_card：chrome 头条 + 圆角 + 阴影）；控制栏固定，表单区滚动
    let overlay = crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::stop_click(
            div()
                .flex_shrink_0()
                .w(px(500.))
                .h(px(500.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(12.))
                .shadow_lg()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(header)
                .child(div().flex_1().min_h_0().flex().child(detail))
                .child(footer),
        ));
    Some(overlay.into_any_element())
}

/// 能力勾选项（checkbox §5.4：14、r3、勾 10 = accent_contrast；视觉同
/// pi_extension_picker 的既核勾选框）。`toggle` 传 None = 恒选不可改
///（文本：模型输入天然含 text）。
fn mj_cap_checkbox(
    label: &str,
    checked: bool,
    weak: &gpui::WeakEntity<Chat>,
    toggle: Option<fn(&mut Chat, &mut gpui::Context<Chat>)>,
) -> gpui::AnyElement {
    let t = T();
    let mut el = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(crate::ui::tokens::checkbox::SIZE))
                .flex_shrink_0()
                .rounded(px(3.))
                .border_1()
                .border_color(rgb(if checked { t.accent } else { t.border }))
                .bg(rgb(if checked { t.accent } else { t.bg_panel }))
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |dd| {
                    dd.child(crate::ui::icon(
                        "check",
                        crate::ui::tokens::checkbox::CHECK,
                        t.accent_contrast,
                    ))
                }),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(if toggle.is_none() {
                    t.text_dim
                } else {
                    t.text
                }))
                .child(SharedString::from(label.to_string())),
        );
    if let Some(toggle) = toggle {
        el = el
            .cursor_pointer()
            .hover(|s| s.opacity(0.85))
            .on_mouse_down(MouseButton::Left, {
                let weak = weak.clone();
                move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| toggle(c, cx));
                }
            });
    }
    el.into_any_element()
}
