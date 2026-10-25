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
        // 042-1：api 缺省 = auto（保存时省略 api 键，端点按 openai 形状猜）
        let api_opt = entry.get("api").and_then(|v| v.as_str()).map(str::to_string);
        let reasoning = model
            .get("reasoning")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let get_str = |v: &serde_json::Value, k: &str| {
            v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
        };
        // 兼容 effective 值（provider compat 合并 model compat，model 赢）
        let compat = |key: &str| -> Option<serde_json::Value> {
            model
                .get("compat")
                .and_then(|c| c.get(key))
                .cloned()
                .or_else(|| {
                    entry
                        .get("compat")
                        .and_then(|c| c.get(key))
                        .cloned()
                })
        };
        let deepseek = model
            .get("compat")
            .and_then(|c| c.get("thinkingFormat"))
            .and_then(|v| v.as_str())
            == Some("deepseek");
        let devrole = compat("supportsDeveloperRole")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let image = model
            .get("input")
            .and_then(|v| v.as_array())
            .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("image")));
        // headers 回填：provider 编辑器回 provider.headers，模型编辑器回
        // model.headers（共用槽，切换 section 时重建）
        let headers_src = if ix.is_some() {
            model.get("headers")
        } else {
            entry.get("headers")
        };
        let header_pairs: Vec<(String, String)> = headers_src
            .and_then(|h| h.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let tlm = model.get("thinkingLevelMap").cloned().unwrap_or(serde_json::json!({}));
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
                // openai-completions 与 auto 行为等价（pi 缺省即它）：旧数据
                // 里写死的该值一律显示为 auto，下次保存自动清键归一
                s.mj_api = match api_opt.as_deref() {
                    Some("openai-completions") | None => 4,
                    Some(a) => mj_api_ix(a),
                };
                s.mj_reasoning = reasoning;
                s.mj_headers = header_pairs
                    .iter()
                    .map(|(k, v)| MjHeaderRow {
                        name: cx.new(|tcx| {
                            let mut ti = TextInput::new(tcx);
                            ti.set_value(k.clone(), tcx);
                            ti
                        }),
                        value: cx.new(|tcx| {
                            let mut ti = TextInput::new(tcx);
                            ti.set_value(v.clone(), tcx);
                            ti
                        }),
                    })
                    .collect();
                s.mj_adv_open = false;
                s.mj_api_override = model
                    .get("api")
                    .and_then(|v| v.as_str())
                    .map(mj_api_ix)
                    .unwrap_or(4);
                s.mj_deepseek = deepseek;
                s.mj_devrole = devrole;
                s.mj_image = image;
                s.mj_cost_edit = false;
                let cost = model.get("cost").cloned().unwrap_or(serde_json::json!({}));
                let cost_str = |k: &str| {
                    cost.get(k)
                        .and_then(|v| v.as_f64())
                        .map(|v| {
                            let s = v.to_string();
                            s.trim_end_matches('0').trim_end_matches('.').to_string()
                        })
                        .unwrap_or_default()
                };
                s.mj_max_tokens.update(cx, |ti, cx| {
                    ti.set_value(
                        model
                            .get("maxTokens")
                            .and_then(|v| v.as_u64())
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                        cx,
                    )
                });
                s.mj_cost_in.update(cx, |ti, cx| ti.set_value(cost_str("input"), cx));
                s.mj_cost_out.update(cx, |ti, cx| ti.set_value(cost_str("output"), cx));
                s.mj_cost_cr.update(cx, |ti, cx| ti.set_value(cost_str("cacheRead"), cx));
                s.mj_cost_cw.update(cx, |ti, cx| ti.set_value(cost_str("cacheWrite"), cx));
                for (i, level) in TLM_LEVELS.iter().enumerate() {
                    let raw = tlm.get(*level);
                    let (mode, val) = match raw {
                        None => (MjTlmMode::Omit, String::new()),
                        Some(serde_json::Value::Null) => (MjTlmMode::Null, String::new()),
                        Some(v) => (MjTlmMode::Custom, v.as_str().unwrap_or("").to_string()),
                    };
                    s.mj_tlm[i].mode = mode;
                    s.mj_tlm[i].value.update(cx, |ti, cx| ti.set_value(val.clone(), cx));
                }
                s.mj_name
                    .update(cx, |ti, cx| ti.set_value(name.clone(), cx));
                s.mj_base
                    .update(cx, |ti, cx| ti.set_value(get_str(&entry, "baseUrl"), cx));
                s.mj_key_visible = false;
                s.mj_key.update(cx, |ti, cx| {
                    ti.set_value(get_str(&entry, "apiKey"), cx);
                    ti.set_masked(true, cx);
                });
                s.mj_id
                    .update(cx, |ti, cx| ti.set_value(get_str(&model, "id"), cx));
                s.mj_mname
                    .update(cx, |ti, cx| ti.set_value(get_str(&model, "name"), cx));
                let ctx = model
                    .get("contextWindow")
                    .and_then(|v| v.as_u64())
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                s.mj_ctx.update(cx, |ti, cx| ti.set_value(ctx, cx));
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

    /// 把编辑器字段应用进缓冲并落盘（provider 名称改动走重命名）。
    pub(crate) fn mj_apply_provider(&mut self, old_name: String, cx: &mut Context<Self>) {
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let (name, base, key, api_ix) = {
            let s = st.read(cx);
            (
                s.mj_name.read(cx).value().trim().to_string(),
                s.mj_base.read(cx).value().trim().to_string(),
                s.mj_key.read(cx).value().trim().to_string(),
                s.mj_api,
            )
        };
        let headers = st.read(cx).mj_headers_value(cx);
        if name.is_empty() {
            return self.mc_set_error(tr("custom_models.name_required"), cx);
        }
        let mut entry = pi_link::models_json::provider_entry(&self.cfg.mc_models_json, &old_name)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        // 042-1：auto（4）= 端点形状由 pi 侧定，条目省略 api 键
        if api_ix >= 4 {
            entry
                .as_object_mut()
                .expect("entry is an object")
                .remove("api");
        } else {
            entry["api"] = serde_json::Value::String(mj_api_name(api_ix));
        }
        // name 字段随输入同步（顺带保证新建条目 sanitize 不丢）
        set_opt_str(&mut entry, "name", &name);
        set_opt_str(&mut entry, "baseUrl", &base);
        set_opt_obj(
            &mut entry,
            "headers",
            headers.as_ref().and_then(|h| headers_to_json(h)),
        );
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

    /// 把模型编辑器全部字段写回缓冲里的模型条目（在既有条目上改键，保留
    /// 手写字段——对齐 pi-web 全量条目编辑语义）。
    pub(crate) fn mj_apply_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let (id, mname, ctx, reasoning) = {
            let s = st.read(cx);
            (
                s.mj_id.read(cx).value().trim().to_string(),
                s.mj_mname.read(cx).value().trim().to_string(),
                s.mj_ctx.read(cx).value().trim().to_string(),
                s.mj_reasoning,
            )
        };
        let form = st.read(cx).mj_model_form(cx);
        if id.is_empty() {
            return self.mc_set_error(tr("custom_models.model_id_required"), cx);
        }
        // cost 草稿 → JSON（pi-web parseCompleteModelCost 语义：空 = 0，
        // 任一项非数字 = 不完整 → 整个 cost 丢弃）
        let cost = cost_to_json(&form.cost);
        if cost.is_none() && form.cost_has_value {
            return self.mc_set_error(tr("models.cost_all_required"), cx);
        }
        let Some(entry) =
            pi_link::models_json::provider_entry(&self.cfg.mc_models_json, &provider).cloned()
        else {
            return;
        };
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
        if !mname.is_empty() {
            obj.insert("name".into(), serde_json::Value::String(mname));
        } else {
            obj.remove("name");
        }
        if let Ok(n) = ctx.parse::<u64>() {
            obj.insert("contextWindow".into(), serde_json::json!(n));
        } else {
            obj.remove("contextWindow");
        }
        if let Ok(n) = form.max_tokens.parse::<u64>() {
            obj.insert("maxTokens".into(), serde_json::json!(n));
        } else {
            obj.remove("maxTokens");
        }
        if reasoning {
            obj.insert("reasoning".into(), serde_json::json!(true));
        } else {
            obj.remove("reasoning");
        }
        if form.image {
            obj.insert("input".into(), serde_json::json!(["text", "image"]));
        } else {
            obj.remove("input");
        }
        // API 覆盖：4 = 沿用 Provider（删键），其余落显式 api
        if form.api_override >= 4 {
            obj.remove("api");
        } else {
            obj.insert("api".into(), serde_json::Value::String(mj_api_name(form.api_override)));
        }
        // compat：DeepSeek 兼容写 thinkingFormat + requiresReasoningContent…；
        // developer role 取消勾选才落显式 false（勾选 = 默认态，删键）
        let mut compat = model
            .get("compat")
            .and_then(|c| c.as_object())
            .cloned()
            .unwrap_or_default();
        if form.deepseek {
            compat.insert("thinkingFormat".into(), serde_json::json!("deepseek"));
            compat.insert(
                "requiresReasoningContentOnAssistantMessages".into(),
                serde_json::json!(true),
            );
        } else {
            compat.remove("thinkingFormat");
            compat.remove("requiresReasoningContentOnAssistantMessages");
        }
        if !form.devrole {
            compat.insert("supportsDeveloperRole".into(), serde_json::json!(false));
        } else {
            compat.remove("supportsDeveloperRole");
        }
        if compat.is_empty() {
            obj.remove("compat");
        } else {
            obj.insert("compat".into(), serde_json::Value::Object(compat));
        }
        if let Some(h) = form.headers.as_ref().and_then(|h| headers_to_json(h)) {
            obj.insert("headers".into(), h);
        } else {
            obj.remove("headers");
        }
        if let Some(t) = tlm_to_json(&form.tlm) {
            obj.insert("thinkingLevelMap".into(), t);
        } else {
            obj.remove("thinkingLevelMap");
        }
        match cost {
            Some(c) => obj.insert("cost".into(), c),
            None => obj.remove("cost"),
        };
        let model = serde_json::Value::Object(obj);
        pi_link::models_json::update_model(&mut self.cfg.mc_models_json, &provider, ix, model);
        self.mj_flush(cx);
        cx.notify();
    }

    pub(crate) fn mj_delete_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        pi_link::models_json::remove_model(&mut self.cfg.mc_models_json, &provider, ix);
        self.mj_flush(cx);
        self.mj_select(format!("mj:p:{provider}"), cx);
    }

    // -- headers / 高级设置 / 思考映射表单动作 -------------------------------

    pub(crate) fn mj_add_header_row(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_headers.push(MjHeaderRow {
                    name: cx.new(|tcx| TextInput::new(tcx)),
                    value: cx.new(|tcx| TextInput::new(tcx)),
                });
                cx.notify();
            });
        }
    }

    pub(crate) fn mj_remove_header_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                if ix < s.mj_headers.len() {
                    s.mj_headers.remove(ix);
                    cx.notify();
                }
            });
        }
    }

    pub(crate) fn mj_toggle_adv(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_adv_open = !s.mj_adv_open;
                cx.notify();
            });
        }
    }

    pub(crate) fn mj_toggle_cost_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_cost_edit = !s.mj_cost_edit;
                cx.notify();
            });
        }
    }

    /// API 下拉选值并收起（Provider 表单槽）。
    pub(crate) fn mj_api_pick(&mut self, ix: u8, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_api = ix;
                s.mj_api_popup = None;
                cx.notify();
            });
        }
    }

    pub(crate) fn mj_set_api_override(&mut self, ix: u8, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_api_override = ix;
                s.mj_api_popup = None;
                cx.notify();
            });
        }
    }

    pub(crate) fn mj_toggle_flag(&mut self, flag: u8, cx: &mut Context<Self>) {
        // 0 = deepseek 1 = devrole 2 = image 3 = reasoning
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                match flag {
                    0 => s.mj_deepseek = !s.mj_deepseek,
                    1 => s.mj_devrole = !s.mj_devrole,
                    2 => s.mj_image = !s.mj_image,
                    _ => s.mj_reasoning = !s.mj_reasoning,
                }
                cx.notify();
            });
        }
    }

    pub(crate) fn mj_tlm_set(&mut self, ix: usize, mode: MjTlmMode, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                if let Some(row) = s.mj_tlm.get_mut(ix) {
                    row.mode = mode;
                    cx.notify();
                }
            });
        }
    }

    pub(crate) fn mj_tlm_clear_all(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                for row in &mut s.mj_tlm {
                    row.mode = MjTlmMode::Omit;
                    row.value.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                }
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
        let (base, api_ix) = {
            let s = st.read(cx);
            (
                s.mj_base.read(cx).value().trim().to_string(),
                s.mj_api,
            )
        };
        let key_text = st.read(cx).mj_key.read(cx).value().trim().to_string();
        let api = if api_ix >= 4 {
            "openai-completions".to_string()
        } else {
            mj_api_name(api_ix)
        };
        self.mc_clear_error(cx);
        let Some((url, auth)) = pi_link::probe::custom_target(&base, &api) else {
            return self.mc_set_error(tr("models.import_base_url_required"), cx);
        };
        let key = match Self::mj_form_key(&key_text) {
            Ok(k) => k,
            Err(e) => return self.mc_set_error(&e, cx),
        };
        self.cfg.mj_speed = Some(MjSpeedState::Running);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let provider_bg = provider;
            let outcome = cx
                .background_executor()
                .spawn(async move { pi_link::probe::probe(&url, auth, &key) })
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
                        // 2、自动拉取：目录落盘 + 刷新内存（可用模型区即满）
                        if catalog.is_empty() {
                            c.cfg.mj_speed = Some(MjSpeedState::PassedEmpty);
                        } else {
                            match pi_link::catalog::save_catalog(&provider_bg, &catalog) {
                                Ok(()) => {
                                    c.refresh_provider_catalog(&provider_bg);
                                    c.cfg.mj_speed = Some(MjSpeedState::Passed {
                                        latency_ms,
                                        models: catalog.len(),
                                        seeded: false,
                                    });
                                }
                                Err(e) => {
                                    c.mc_set_error(&e, cx);
                                    c.cfg.mj_speed = Some(MjSpeedState::Failed(e));
                                }
                            }
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
    /// 该商模型目录（probe → save_catalog → 刷新内存）；失败挂错误条。
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
        let api_ix = entry
            .get("api")
            .and_then(|v| v.as_str())
            .map(mj_api_ix)
            .unwrap_or(4);
        let api = if api_ix >= 4 {
            "openai-completions".to_string()
        } else {
            mj_api_name(api_ix)
        };
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
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { pi_link::probe::probe(&url, auth, &key) })
                .await;
            let _ = this.update(cx, |c, cx| match outcome {
                pi_link::probe::ProbeOutcome::Ok { catalog, .. } if !catalog.is_empty() => {
                    if let Err(e) = pi_link::catalog::save_catalog(&provider, &catalog) {
                        c.mc_set_error(&e, cx);
                        return;
                    }
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
        let (model_id, base, key_text, model_api, model_headers) = {
            let s = st.read(cx);
            (
                s.mj_id.read(cx).value().trim().to_string(),
                s.mj_base.read(cx).value().trim().to_string(),
                s.mj_key.read(cx).value().trim().to_string(),
                s.mj_api_override,
                s.mj_headers_value(cx),
            )
        };
        let api = if model_api >= 4 {
            mj_api_name(st.read(cx).mj_api)
        } else {
            mj_api_name(model_api)
        };
        // provider 级 headers 从缓冲合并（模型行覆盖同名）
        let mut headers: Vec<(String, String)> = pi_link::models_json::provider_entry(
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
        for (k, v) in model_headers.into_iter().flatten() {
            headers.retain(|(n, _)| !n.eq_ignore_ascii_case(&k));
            headers.push((k, v));
        }
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

    /// 目录回填：目录未加载先拉（后台，1h 磁盘缓存），加载完直接按当前
    /// 表单模型 id 推荐回填**缓冲里的空字段**并重选回显。
    pub(crate) fn mj_catalog_fill(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        if self.cfg.mj_models_dev.is_none()
            || !matches!(self.cfg.mj_models_dev_state, MjCatalogState::Done { .. })
        {
            self.cfg.mj_models_dev_state = MjCatalogState::Running;
            cx.notify();
            cx.spawn(async move |this, cx| {
                let loaded = cx
                    .background_executor()
                    .spawn(async { pi_link::models_dev::load() })
                    .await;
                let _ = this.update(cx, |c, cx| match loaded {
                    Ok(entries) => {
                        let n = entries.len();
                        c.cfg.mj_models_dev = Some(std::sync::Arc::new(entries));
                        c.cfg.mj_models_dev_state = MjCatalogState::Idle;
                        let _ = n;
                        // 目录就绪后立即应用一次回填
                        c.mj_catalog_apply(&provider, ix, cx);
                    }
                    Err(e) => {
                        c.cfg.mj_models_dev_state = MjCatalogState::Err(e);
                        cx.notify();
                    }
                });
            })
            .detach();
            return;
        }
        self.mj_catalog_apply(&provider, ix, cx);
    }

    fn mj_catalog_apply(&mut self, provider: &str, ix: usize, cx: &mut Context<Self>) {
        let Some(entries) = self.cfg.mj_models_dev.clone() else {
            return;
        };
        let Some(entry) =
            pi_link::models_json::provider_entry(&self.cfg.mc_models_json, provider).cloned()
        else {
            return;
        };
        let Some(model) = entry.get("models").and_then(|m| m.get(ix)).cloned() else {
            return;
        };
        let query = model.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if query.is_empty() {
            return self.mc_set_error(tr("custom_models.model_id_required"), cx);
        }
        let base_url = entry
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let rec = pi_link::models_dev::recommend(&entries, query, provider, base_url);
        let mut model = model;
        let applied = pi_link::models_dev::fill_empty(&mut model, &rec.preset);
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
// 表单值读取（SettingsPanel 扩展，住在 custom_models.rs 的域内）
// ---------------------------------------------------------------------------

impl super::SettingsPanel {
    /// headers 行 → 有序键值对（空名丢弃；全空 = None）。
    pub(crate) fn mj_headers_value(&self, cx: &gpui::App) -> Option<Vec<(String, String)>> {
        let mut out: Vec<(String, String)> = Vec::new();
        for row in &self.mj_headers {
            let name = row.name.read(cx).value().trim().to_string();
            if name.is_empty() {
                continue;
            }
            let value = row.value.read(cx).value().to_string();
            out.retain(|(n, _)| *n != name);
            out.push((name, value));
        }
        (!out.is_empty()).then_some(out)
    }

    /// 模型编辑器全部扩展字段的读取快照（mj_apply_model 用）。
    pub(crate) fn mj_model_form(
        &self,
        cx: &gpui::App,
    ) -> MjModelForm {
        let num = |e: &gpui::Entity<TextInput>| e.read(cx).value().trim().to_string();
        MjModelForm {
            max_tokens: num(&self.mj_max_tokens),
            image: self.mj_image,
            api_override: self.mj_api_override,
            deepseek: self.mj_deepseek,
            devrole: self.mj_devrole,
            headers: self.mj_headers_value(cx),
            tlm: std::array::from_fn(|i| {
                (
                    self.mj_tlm[i].mode,
                    self.mj_tlm[i].value.read(cx).value().trim().to_string(),
                )
            }),
            cost: [
                num(&self.mj_cost_in),
                num(&self.mj_cost_out),
                num(&self.mj_cost_cr),
                num(&self.mj_cost_cw),
            ],
            cost_has_value: [
                num(&self.mj_cost_in),
                num(&self.mj_cost_out),
                num(&self.mj_cost_cr),
                num(&self.mj_cost_cw),
            ]
            .iter()
            .any(|s| !s.trim().is_empty()),
        }
    }
}

/// 模型编辑器扩展字段的保存期快照。
pub(crate) struct MjModelForm {
    pub max_tokens: String,
    pub image: bool,
    pub api_override: u8,
    pub deepseek: bool,
    pub devrole: bool,
    pub headers: Option<Vec<(String, String)>>,
    pub tlm: [(MjTlmMode, String); TLM_LEVEL_COUNT],
    pub cost: [String; 4],
    pub cost_has_value: bool,
}

/// headers 键值对 → JSON 对象（空表 = None 删键）。
fn headers_to_json(headers: &[(String, String)]) -> Option<serde_json::Value> {
    if headers.is_empty() {
        return None;
    }
    let mut obj = serde_json::Map::new();
    for (k, v) in headers {
        obj.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    Some(serde_json::Value::Object(obj))
}

/// 思考映射行 → JSON（全 Omit = None）。
fn tlm_to_json(tlm: &[(MjTlmMode, String); TLM_LEVEL_COUNT]) -> Option<serde_json::Value> {
    let mut obj = serde_json::Map::new();
    for (i, level) in TLM_LEVELS.iter().enumerate() {
        match tlm[i].0 {
            MjTlmMode::Omit => {}
            MjTlmMode::Null => {
                obj.insert((*level).to_string(), serde_json::Value::Null);
            }
            MjTlmMode::Custom => {
                let v = tlm[i].1.trim();
                if !v.is_empty() {
                    obj.insert((*level).to_string(), serde_json::Value::String(v.into()));
                }
            }
        }
    }
    (!obj.is_empty()).then(|| serde_json::Value::Object(obj))
}

/// cost 四项草稿 → 完整 cost JSON（pi-web parseCompleteModelCost：空 = 0，
/// 任一非数字 = None；全空 = None）。
fn cost_to_json(draft: &[String; 4]) -> Option<serde_json::Value> {
    if draft.iter().all(|s| s.trim().is_empty()) {
        return None;
    }
    let parse = |s: &str| -> Option<f64> {
        if s.trim().is_empty() {
            return Some(0.);
        }
        s.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.)
    };
    let (i, o, cr, cw) = (
        parse(&draft[0]),
        parse(&draft[1]),
        parse(&draft[2]),
        parse(&draft[3]),
    );
    match (i, o, cr, cw) {
        (Some(i), Some(o), Some(cr), Some(cw)) => Some(serde_json::json!({
            "input": i, "output": o, "cacheRead": cr, "cacheWrite": cw
        })),
        _ => None,
    }
}

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

fn mj_api_ix(api: &str) -> u8 {
    match api {
        "openai-responses" => 1,
        "anthropic-messages" => 2,
        "google-generative-ai" => 3,
        _ => 0,
    }
}

fn mj_api_name(ix: u8) -> String {
    match ix {
        1 => "openai-responses".into(),
        2 => "anthropic-messages".into(),
        3 => "google-generative-ai".into(),
        _ => "openai-completions".into(),
    }
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

/// 空对象/None 删除可选对象键，其余写入（保序）。
fn set_opt_obj(obj: &mut serde_json::Value, key: &str, value: Option<serde_json::Value>) {
    let map = obj.as_object_mut().expect("entry is an object");
    match value {
        None => {
            map.remove(key);
        }
        Some(v) => {
            map.insert(key.to_string(), v);
        }
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

    // Base URL 行（全宽）+ API 类型行（半宽下拉，默认 auto）
    let detail = detail
        .child(form_row(
            &tr("custom_models.base_url"),
            d.mj_base.clone().into_any_element(),
        ))
        .child(form_row(
            &tr("custom_models.api"),
            api_options_dropdown(weak, d),
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
    let project_scope = chat.cfg.mc_project_scope;
    let last_guard = enabled_count == 1;
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
            let last_one = last_guard && is_enabled;
            // 行点击 → 模型编辑器（在账本 models 数组里按 id 定位下标）
            let open_ref = (prov.clone(), m.id.clone());
            let weak_row = weak_rows_v.clone();
            div()
                .id(SharedString::from(format!("mj-row-{ix}")))
                .w_full()
                .h(px(row_h))
                .pl(px(12.))
                .pr(px(24.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .when(ix > 0, |dd| dd.border_t_1().border_color(rgb(t.border)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| {
                        let (prov, id) = &open_ref;
                        let ix = pi_link::models_json::provider_entry(&c.cfg.mc_models_json, prov)
                            .map(pi_link::models_json::provider_models)
                            .unwrap_or_default()
                            .iter()
                            .position(|mm| {
                                mm.get("id").and_then(|v| v.as_str()) == Some(id.as_str())
                            });
                        if let Some(ix) = ix {
                            c.mj_select(format!("mj:m:{prov}:{ix}"), cx);
                        }
                    });
                })
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
                    last_one || project_scope,
                    {
                        let r = r.clone();
                        move |c, cx| c.mc_toggle_model(r.clone(), !is_enabled, cx)
                    },
                ))
                .into_any_element()
        },
    );
    let section = section.child(list).child({
        // + 模型：手工加一个空白模型条目（导入之外的手工入口）
        let weak_add = weak.clone();
        let pname = provider.to_string();
        div()
            .id("mj-add-model")
            .h(px(30.))
            .px(px(8.))
            .rounded(px(5.))
            .flex()
            .items_center()
            .text_size(crate::appearance::ui_size(12.))
            .cursor_pointer()
            .text_color(rgb(t.text_dim))
            .hover(|s| {
                s.bg(rgb(t.bg_hover))
                    .text_color(rgb(crate::appearance::theme::theme().accent))
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_add.update(cx, |c, cx| c.mj_add_model(pname.clone(), cx));
            })
            .child(format!("+ {}", tr("settings.tab.models")))
    });
    detail.child(section)
}

/// headers 行编辑器（name input + value input + ✕）。
fn render_header_rows(
    weak: &gpui::WeakEntity<Chat>,
    d: &super::SettingsFormData,
    t: &crate::appearance::theme::Theme,
) -> Vec<gpui::AnyElement> {
    d.mj_headers
        .iter()
        .enumerate()
        .map(|(ix, row)| {
            let weak_del = weak.clone();
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .mb(px(4.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(180.))
                        .child(row.name.clone()),
                )
                .child(div().flex_1().min_w_0().child(row.value.clone()))
                .child(
                    div()
                        .id(SharedString::from(format!("mj-hdr-del-{ix}")))
                        .flex_shrink_0()
                        .size(px(20.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_dim))
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(rgb(t.bg_hover))
                                .text_color(rgb(crate::appearance::theme::theme().danger))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_del.update(cx, |c, cx| c.mj_remove_header_row(ix, cx));
                        })
                        .child("✕"),
                )
                .into_any_element()
        })
        .collect()
}

/// 思考档位点的配色（pi-web LEVEL_COLORS 对齐）。
fn tlm_level_color(level: &str) -> u32 {
    match level {
        "low" => 0x60a5fa,
        "medium" => 0xa78bfa,
        "high" => 0xf472b6,
        "xhigh" => 0xfb923c,
        "max" => 0xef4444,
        _ => 0x6b7280,
    }
}

/// 自定义模型编辑器（pi-web ModelDetail 对齐：能力/规格/价格/高级设置/
/// 测试/填入模型信息）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mj_model_editor(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    ix: usize,
    d: &super::SettingsFormData,
    error: &Option<String>,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let entry = pi_link::models_json::provider_entry(&chat.cfg.mc_models_json, name)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let model = entry
        .get("models")
        .and_then(|m| m.get(ix))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let provider_compat = entry.get("compat").cloned().unwrap_or(serde_json::json!({}));
    let mut compat_count = 0usize;
    if d.mj_deepseek {
        compat_count += 1;
    }
    if !d.mj_devrole {
        compat_count += 1;
    }
    compat_count += model
        .get("compat")
        .and_then(|c| c.as_object())
        .map(|o| {
            o.keys()
                .filter(|k| {
                    !(*k == "thinkingFormat"
                        || *k == "requiresReasoningContentOnAssistantMessages"
                        || *k == "supportsDeveloperRole")
                })
                .count()
        })
        .unwrap_or(0);
    let tlm_set = d
        .mj_tlm
        .iter()
        .filter(|r| r.mode != MjTlmMode::Omit)
        .count();
    let adv_summary = {
        let mut parts: Vec<String> = Vec::new();
        if d.mj_api_override < 4 {
            parts.push(format!("API: {}", mj_api_name(d.mj_api_override)));
        }
        if !d.mj_headers.is_empty() {
            parts.push(crate::i18n::tf(
                "models.headers_summary",
                &[("count", d.mj_headers.len().to_string())],
            ));
        }
        if compat_count > 0 {
            parts.push(crate::i18n::tf(
                "models.compat_summary",
                &[("count", compat_count.to_string())],
            ));
        }
        if tlm_set > 0 {
            parts.push(crate::i18n::tf(
                "models.thinking_summary",
                &[("count", tlm_set.to_string())],
            ));
        }
        if parts.is_empty() {
            tr("models.adv_defaults").to_string()
        } else {
            parts.join(" · ")
        }
    };

    // 头行：模型 + provider 名 + [测试] [保存] [移除]；测试状态行紧随
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
    let header = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(28.))
        .child(section_title(&tr("settings.tab.models")))
        .child(
            div()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(name.to_string())),
        )
        .child(div().flex_1())
        .child(config_button(
            "mj-test",
            weak,
            &tr("models.test"),
            Btn::Secondary,
            true,
            testing,
            {
                let n = name.to_string();
                move |c, cx| c.mj_test_start(n.clone(), ix, cx)
            },
        ))
        .child(config_button(
            "mj-mapply",
            weak,
            &tr("action.save"),
            Btn::Primary,
            true,
            false,
            {
                let n = name.to_string();
                move |c, cx| c.mj_apply_model(n.clone(), ix, cx)
            },
        ))
        .child(config_button(
            "mj-mdel",
            weak,
            &tr("action.remove"),
            Btn::Danger,
            true,
            false,
            {
                let n = name.to_string();
                move |c, cx| c.mj_delete_model(n.clone(), ix, cx)
            },
        ));

    let mut detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(header)
        .children(error.as_ref().map(|e| error_note(e)))
        .children(test_note.as_ref().map(|(text, ok)| {
            if *ok {
                note(text)
            } else {
                error_note(text)
            }
        }))
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(field(
                    &tr("custom_models.field_id"),
                    div().w(px(300.)).child(d.mj_id.clone()),
                ))
                .child(field(
                    &tr("field.name"),
                    div().w(px(300.)).child(d.mj_mname.clone()),
                )),
        )
        // -- 填入模型信息（models.dev）--
        .child(mj_catalog_fill_section(chat, weak, name, ix, d, t))
        // -- 能力 --
        .child(section_title(&tr("models.capabilities")))
        .child(
            div()
                .flex()
                .gap(px(20.))
                .child(mj_capability_row(weak, "mj-reasoning", &tr("models.cap_reasoning"), d.mj_reasoning, 3))
                .child(mj_capability_row(weak, "mj-image", &tr("models.cap_image"), d.mj_image, 2)),
        )
        // -- 模型规格 --
        .child(
            div()
                .flex()
                .items_center()
                .child(section_title(&tr("models.specs")))
                .child(div().flex_1())
                .child(mj_text_button(
                    weak,
                    "mj-cost-edit",
                    &if d.mj_cost_edit {
                        tr("models.finish_costs").to_string()
                    } else {
                        tr("models.edit_costs").to_string()
                    },
                    |c, cx| c.mj_toggle_cost_edit(cx),
                )),
        )
        .child(
            div()
                .flex()
                .gap(px(10.))
                .child(field(
                    &tr("custom_models.field_context"),
                    div().w(px(300.)).child(d.mj_ctx.clone()),
                ))
                .child(field(
                    &tr("models.max_output"),
                    div().w(px(300.)).child(d.mj_max_tokens.clone()),
                )),
        )
        .child(mj_cost_section(&model, d, t));

    // -- 高级设置（折叠）--
    let weak_adv = weak.clone();
    detail = detail
        .child(
            div()
                .mt(px(12.))
                .pt(px(8.))
                .border_t_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .child(
                    div()
                        .id("mj-adv-toggle")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_adv.update(cx, |c, cx| c.mj_toggle_adv(cx));
                        })
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(tr("models.advanced")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(crate::appearance::ui_size(10.))
                                .text_color(rgb(t.text_dim))
                                .child(SharedString::from(adv_summary)),
                        )
                        .child(crate::ui::icon(
                            if d.mj_adv_open { "chevron-up" } else { "chevron-down" },
                            14.,
                            t.text_muted,
                        )),
                ),
        );
    if d.mj_adv_open {
        detail = detail
            .child(field(
                &tr("models.api_override"),
                mj_api_override_dropdown(weak, d),
            ))
            .child(field(
                &tr("models.headers"),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(render_header_rows(weak, d, t))
                    .child({
                        let weak_add_hdr = weak.clone();
                        div()
                            .id("mj-add-header-m")
                            .mt(px(2.))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(crate::appearance::ui_size(11.))
                            .cursor_pointer()
                            .text_color(rgb(t.text_muted))
                            .hover(|s| {
                                s.text_color(rgb(crate::appearance::theme::theme().accent))
                            })
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_add_hdr.update(cx, |c, cx| c.mj_add_header_row(cx));
                            })
                            .child(format!("+ {}", tr("models.headers_add")))
                    }),
            ))
            .child(note(&tr("models.headers_hint_model")));
        if d.mj_reasoning {
            detail = detail
                .child(section_title(&tr("models.compatibility")))
                .child(mj_capability_row(
                    weak,
                    "mj-deepseek",
                    &tr("models.deepseek_compat"),
                    d.mj_deepseek,
                    0,
                ))
                .child(mj_capability_row(
                    weak,
                    "mj-devrole",
                    &tr("models.devrole"),
                    d.mj_devrole,
                    1,
                ))
                .child(mj_tlm_editor(weak, d, &provider_compat, t));
        }
    }
    detail.into_any_element()
}

/// 能力/兼容开关行（label + switch；pi-web Check 对齐）。
fn mj_capability_row(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    label: &str,
    on: bool,
    flag: u8,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(T().text_muted))
                .child(SharedString::from(label.to_string())),
        )
        .child(config_switch(id, weak, on, false, move |c, cx| {
            c.mj_toggle_flag(flag, cx)
        }))
        .into_any_element()
}

/// 小号文字按钮（编辑价格 / 目录来源这类行内链接钮）。
fn mj_text_button(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    label: &str,
    on_click: fn(&mut Chat, &mut gpui::Context<Chat>),
) -> gpui::AnyElement {
    let weak = weak.clone();
    div()
        .id(SharedString::from(id))
        .px(px(4.))
        .py(px(2.))
        .text_size(crate::appearance::ui_size(10.))
        .cursor_pointer()
        .text_color(rgb(T().accent))
        .hover(|s| s.opacity(0.8))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        })
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}

/// 填入模型信息区（按钮 + 来源 + 结果行）。
fn mj_catalog_fill_section(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    ix: usize,
    _d: &super::SettingsFormData,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let running = matches!(
        chat.cfg.mj_models_dev_state,
        MjCatalogState::Running
    );
    let status: Option<(String, bool)> = match &chat.cfg.mj_models_dev_state {
        MjCatalogState::Running => Some((tr("models.catalog_filling").to_string(), true)),
        MjCatalogState::Done { applied, note } => {
            let mut msg = if *applied > 0 {
                crate::i18n::tf("models.catalog_filled", &[("count", applied.to_string())])
            } else {
                tr("models.catalog_no_empty").to_string()
            };
            if !note.is_empty() {
                msg.push_str(&format!(" · {note}"));
            }
            Some((msg, true))
        }
        MjCatalogState::Err(e) => Some((e.clone(), false)),
        MjCatalogState::Idle => None,
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .pt(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(config_button(
                    "mj-catalog-fill",
                    weak,
                    &if running {
                        tr("models.catalog_filling")
                    } else {
                        tr("models.catalog_fill")
                    },
                    Btn::Secondary,
                    true,
                    running,
                    {
                        let n = name.to_string();
                        move |c, cx| c.mj_catalog_fill(n.clone(), ix, cx)
                    },
                ))
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("models.catalog_source")),
                ),
        )
        .children(status.as_ref().map(|(text, ok)| {
            if *ok {
                note(text)
            } else {
                error_note(text)
            }
        }))
        .into_any_element()
}

/// 每百万价格区（展示态 = 4 字段；编辑态 = 4 输入 + 全填提示）。
fn mj_cost_section(
    model: &serde_json::Value,
    d: &super::SettingsFormData,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let cost = model.get("cost").cloned().unwrap_or(serde_json::json!({}));
    let labels = [
        ("models.cost_input", "input"),
        ("models.cost_output", "output"),
        ("models.cost_cache_read", "cacheRead"),
        ("models.cost_cache_write", "cacheWrite"),
    ];
    if d.mj_cost_edit {
        let inputs = [
            d.mj_cost_in.clone(),
            d.mj_cost_out.clone(),
            d.mj_cost_cr.clone(),
            d.mj_cost_cw.clone(),
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(10.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_dim))
                    .child(tr("models.cost_per_m")),
            )
            .child(div().flex().gap(px(8.)).children(
                labels.iter().zip(inputs.iter()).map(|((label, _), input)| {
                    field(&tr(label), div().w(px(140.)).child(input.clone()))
                        .into_any_element()
                }),
            ))
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(10.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_dim))
                    .child(tr("models.cost_per_m")),
            )
            .child(div().flex().gap(px(16.)).children(labels.iter().map(
                |(label, key)| {
                    let v = cost.get(*key).and_then(|x| x.as_f64());
                    let shown = match v {
                        None => tr("models.not_provided").to_string(),
                        Some(n) => format!("${n}"),
                    };
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(10.))
                                .text_color(rgb(t.text_dim))
                                .child(tr(label)),
                        )
                        .child(
                            div()
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(12.))
                                .text_color(rgb(if v.is_none() { t.text_dim } else { t.text }))
                                .child(SharedString::from(shown)),
                        )
                        .into_any_element()
                },
            )))
            .into_any_element()
    }
}

/// API 下拉选项（pi-web API_OPTIONS；`None` 档位 = 沿用 Provider，仅
/// 模型 API 覆盖下拉出现）。
const MJ_API_OPTIONS: [(u8, &str); 4] = [
    (0, "openai-completions"),
    (1, "openai-responses"),
    (2, "anthropic-messages"),
    (3, "google-generative-ai"),
];

/// 下拉触发器（agents 页 sel_trigger 同款形态）；`w` 接 px 或 relative
///（半宽列用 relative），`dim` = 默认态暗色文字（pi-web Select 空 value
/// 用 --text-dim）。
fn mj_sel_trigger(value: &str, open: bool, w: impl Into<gpui::Length>, dim: bool) -> gpui::AnyElement {
    let t = T();
    div()
        .w(w.into())
        .h(px(30.))
        .flex_shrink_0()
        .px(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .flex()
        .items_center()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(if dim { t.text_dim } else { t.text }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(value.to_string())),
        )
        .child(crate::ui::icon(
            if open { "chevron-up" } else { "chevron-down" },
            12.,
            t.text_dim,
        ))
        .into_any_element()
}

/// 下拉菜单项（点选即落面板状态并收起）。
fn mj_dd_item(
    weak: gpui::WeakEntity<Chat>,
    label: String,
    selected: bool,
    pick: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + Copy + 'static,
) -> gpui::AnyElement {
    let t = T();
    div()
        .id(SharedString::from(format!("mj-ddit-{label}")))
        .h(px(30.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(if selected { t.accent } else { t.text }))
        .bg(rgb(if selected { t.bg_selected } else { t.bg }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, pick);
        })
        .child(SharedString::from(label))
        .into_any_element()
}

/// Provider 表单的 API 下拉（pi-web ProviderDetail `<Select>` 对齐）。
pub(crate) fn api_options_dropdown(
    weak: &gpui::WeakEntity<Chat>,
    d: &super::SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let open = d.mj_api_popup == Some(0);
    // 042-1：auto（4）= 缺省档（条目省略 api 键，端点形状 pi 侧自定）
    let label = if d.mj_api >= 4 {
        tr("models.api_auto").to_string()
    } else {
        MJ_API_OPTIONS
            .iter()
            .find(|(ix, _)| *ix == d.mj_api)
            .map(|(_, name)| (*name).to_string())
            .unwrap_or_else(|| "openai-completions".into())
    };
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();
    crate::ui::dropdown(
        "mj-api-dd",
        &d.mj_api_dd,
        open,
        move |_, cx| {
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.mj_api_popup = if s.mj_api_popup == Some(0) { None } else { Some(0) };
                        cx.notify();
                    });
                }
            });
        },
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.mj_api_popup.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            });
        },
        mj_sel_trigger(&label, open, gpui::relative(0.5), false),
        move || {
            let mut menu = div()
                .w(px(300.))
                .py(px(3.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .overflow_hidden();
            menu = menu.child(mj_dd_item(
                weak.clone(),
                tr("models.api_auto").to_string(),
                d.mj_api >= 4,
                |c, cx| c.mj_api_pick(4, cx),
            ));
            for (ix, name) in MJ_API_OPTIONS {
                menu = menu.child(mj_dd_item(
                    weak.clone(),
                    name.to_string(),
                    ix == d.mj_api,
                    move |c, cx| c.mj_api_pick(ix, cx),
                ));
            }
            menu.into_any_element()
        },
    )
}

/// 模型 API 覆盖下拉（pi-web ModelDetail 高级区 `<Select>` 对齐：非 required
/// 时首项 = `— 默认 / none —`，未选默认态触发器文字暗色）。
fn mj_api_override_dropdown(
    weak: &gpui::WeakEntity<Chat>,
    d: &super::SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let open = d.mj_api_popup == Some(1);
    let inherit = d.mj_api_override >= 4;
    let label = if inherit {
        tr("models.api_default_item").to_string()
    } else {
        MJ_API_OPTIONS
            .iter()
            .find(|(ix, _)| *ix == d.mj_api_override)
            .map(|(_, name)| (*name).to_string())
            .unwrap_or_default()
    };
    let weak_toggle = weak.clone();
    let weak_dismiss = weak.clone();
    crate::ui::dropdown(
        "mj-apio-dd",
        &d.mj_api_dd,
        open,
        move |_, cx| {
            let _ = weak_toggle.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        s.mj_api_popup = if s.mj_api_popup == Some(1) { None } else { Some(1) };
                        cx.notify();
                    });
                }
            });
        },
        move |_, cx| {
            let _ = weak_dismiss.update(cx, |c, cx| {
                if let Some(st) = c.dlg.settings.clone() {
                    st.update(cx, |s, cx| {
                        if s.mj_api_popup.take().is_some() {
                            cx.notify();
                        }
                    });
                }
            });
        },
        mj_sel_trigger(&label, open, px(300.), inherit),
        move || {
            let mut menu = div()
                .w(px(300.))
                .py(px(3.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(8.))
                .shadow_lg()
                .overflow_hidden();
            menu = menu.child(mj_dd_item(
                weak.clone(),
                tr("models.api_default_item").to_string(),
                d.mj_api_override >= 4,
                |c, cx| c.mj_set_api_override(4, cx),
            ));
            for (ix, name) in MJ_API_OPTIONS {
                menu = menu.child(mj_dd_item(
                    weak.clone(),
                    name.to_string(),
                    ix == d.mj_api_override,
                    move |c, cx| c.mj_set_api_override(ix, cx),
                ));
            }
            menu.into_any_element()
        },
    )
}

/// 思考等级映射编辑器（7 档 × Default/Disabled/Custom+输入）。
fn mj_tlm_editor(
    weak: &gpui::WeakEntity<Chat>,
    d: &super::SettingsFormData,
    _provider_compat: &serde_json::Value,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let any_set = d.mj_tlm.iter().any(|r| r.mode != MjTlmMode::Omit);
    let mut col = div().flex().flex_col().gap(px(2.));
    if any_set {
        col = col.child(
            div()
                .flex()
                .justify_end()
                .child(mj_text_button(weak, "mj-tlm-clear", &tr("models.tlm_clear").to_string(), |c, cx| {
                    c.mj_tlm_clear_all(cx)
                })),
        );
    }
    for (i, level) in TLM_LEVELS.iter().enumerate() {
        let row = &d.mj_tlm[i];
        let color = tlm_level_color(level);
        let weak_row = weak.clone();
        let weak_row2 = weak.clone();
        let weak_row3 = weak.clone();
        let mode = row.mode;
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(72.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .flex_shrink_0()
                        .child(
                            div()
                                .size(px(6.))
                                .rounded_full()
                                .bg(rgb(color))
                                .when(mode == MjTlmMode::Null, |dd| dd.opacity(0.3)),
                        )
                        .child(
                            div()
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(if mode == MjTlmMode::Null {
                                    t.text_dim
                                } else {
                                    t.text_muted
                                }))
                                .child(SharedString::from((*level).to_string())),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .overflow_hidden()
                        .child({
                            let w = weak_row.clone();
                            div()
                                .id(SharedString::from(format!("mj-tlm-{i}-def")))
                                .h(px(22.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .text_size(crate::appearance::ui_size(10.))
                                .bg(rgb(if mode == MjTlmMode::Omit { t.accent } else { t.bg_panel }))
                                .text_color(rgb(if mode == MjTlmMode::Omit { t.bg } else { t.text_muted }))
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.85))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = w.update(cx, |c, cx| c.mj_tlm_set(i, MjTlmMode::Omit, cx));
                                })
                                .child(tr("models.tlm_default"))
                        })
                        .child({
                            let w = weak_row2.clone();
                            div()
                                .id(SharedString::from(format!("mj-tlm-{i}-null")))
                                .h(px(22.))
                                .px(px(8.))
                                .border_l_1()
                                .border_color(rgb(t.border))
                                .flex()
                                .items_center()
                                .text_size(crate::appearance::ui_size(10.))
                                .bg(rgb(if mode == MjTlmMode::Null { 0xef4444 } else { t.bg_panel }))
                                .text_color(rgb(if mode == MjTlmMode::Null { 0xffffff } else { t.text_muted }))
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.85))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = w.update(cx, |c, cx| c.mj_tlm_set(i, MjTlmMode::Null, cx));
                                })
                                .child(tr("models.tlm_disabled"))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(rgb(if mode == MjTlmMode::Custom { t.accent } else { t.border }))
                        .overflow_hidden()
                        .child({
                            let w = weak_row3.clone();
                            div()
                                .id(SharedString::from(format!("mj-tlm-{i}-custom")))
                                .h(px(22.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .text_size(crate::appearance::ui_size(10.))
                                .bg(rgb(if mode == MjTlmMode::Custom { t.accent } else { t.bg_panel }))
                                .text_color(rgb(if mode == MjTlmMode::Custom { t.bg } else { t.text_muted }))
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.85))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = w.update(cx, |c, cx| c.mj_tlm_set(i, MjTlmMode::Custom, cx));
                                })
                                .child(tr("models.tlm_custom"))
                        })
                        .when(mode == MjTlmMode::Custom, |dd| {
                            dd.child(row.value.clone())
                        }),
                ),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .child(section_title(&tr("models.tlm")))
                .child(div().flex_1()),
        )
        .child(col)
        .into_any_element()
}


