//! models.json 自定义 provider / 模型编辑器（设置·模型页右栏的 mj_* 分支）。
//! 042 定稿：表单头【保存】/【移除】（043 同款），点保存即写盘——无页面级
//! footer；**无编辑缓冲**——读即现读 `models.json`，写即 read→mutate→write
//!（042-1：数据源从 PF 账本切到 pi 的 models.json，UI 与交互不变）。
//! 042 M2 对齐 pi-web ProviderDetail/ModelDetail：headers、导入模型
//!（discovery 多选）、模型测试、models.dev 填入模型信息、API 覆盖 /
//! 兼容（DeepSeek / developer role）/ 思考等级映射。

use super::*;
use pi_link::credentials::SecretVault;

impl Chat {
    /// 现读 `models.json`（缺失 → 空文档；坏文件 → 空文档 + 错误条由
    /// `mc_mj_error` 承担）。**不做内存副本**：读处读盘、写处
    /// read→mutate→write，不存在“打开面板时那份旧快照”把磁盘上别人的
    /// 改动覆盖掉的情形。
    fn mj_doc() -> serde_json::Value {
        pi_link::models_json::read().unwrap_or_else(|_| serde_json::json!({ "providers": {} }))
    }

    /// 写盘（整份 read→mutate→write 的末步；锁内 + 不可读拒写）。
    fn mj_write(&mut self, doc: &serde_json::Value, cx: &mut Context<Self>) {
        if let Err(e) = pi_link::models_json::save(doc) {
            self.mc_set_error(&e, cx);
        }
    }

    pub(crate) fn mj_select(&mut self, section: String, cx: &mut Context<Self>) {
        let over = mj_parse_override(&section);
        let (name, ix) = match &over {
            Some((p, _)) => (p.clone(), None),
            None => mj_parse_section(&section),
        };
        let entry = pi_link::models_json::provider_entry(&Self::mj_doc(), &name)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let model = match &over {
            // 覆盖模式：读 modelOverrides[id]；没有覆盖条目 → 用该模型**当前
            // 生效的定义**（pi 目录里的 contextWindow/maxTokens/reasoning/
            // input）预填，表单不再是空白、用户能看见真实参数，改哪项就覆盖哪项
            Some((_, id)) => {
                let mut m = entry
                    .get("modelOverrides")
                    .and_then(|o| o.get(id))
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                if !m.is_object() {
                    m = serde_json::json!({});
                }
                if m.as_object().is_some_and(|o| o.is_empty()) {
                    if let Some(base) = self
                        .catalog_for(self.project())
                        .into_iter()
                        .find(|mm| mm.provider == name && &mm.id == id)
                    {
                        m = serde_json::json!({
                            "contextWindow": base.context_window,
                            "maxTokens": base.max_tokens,
                            "reasoning": base.reasoning,
                            "input": base.input,
                        });
                    }
                }
                if let Some(obj) = m.as_object_mut() {
                    obj.insert("id".into(), serde_json::json!(id));
                }
                m
            }
            None => ix
                .and_then(|ix| entry.get("models").and_then(|m| m.get(ix)).cloned())
                .unwrap_or_else(|| serde_json::json!({})),
        };
        let get_str = |v: &serde_json::Value, k: &str| {
            v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
        };
        // 能力勾选回填：input 数组含 image（文本恒选不落 UI 态）
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
                s.mj_reasoning = model
                    .get("reasoning")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                s.mj_name
                    .update(cx, |ti, cx| ti.set_value(name.clone(), cx));
                s.mj_base
                    .update(cx, |ti, cx| ti.set_value(get_str(&entry, "baseUrl"), cx));
                s.mj_key_visible = false;
                // key 回填（042-1）：凭据统一在 auth.json —— PF 标记条目 →
                // 钥匙库解出明文预填（掩码态眼睛复核）；否则回落 `models.json`
                // 里用户自己写的 apiKey 原文。
                let key_display = pi_link::auth_json::resolve_secret(&name)
                    .unwrap_or_else(|| get_str(&entry, "apiKey"));
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
                // 内置模型＝pi 的定义，PF 只看不改（用户 2026-10-27 定稿）
                let ro = over.is_some();
                s.mj_id.update(cx, |ti, cx| ti.set_disabled(ro, cx));
                s.mj_ctx.update(cx, |ti, cx| ti.set_disabled(ro, cx));
                s.mj_max_tokens.update(cx, |ti, cx| ti.set_disabled(ro, cx));
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
        let mut doc = Self::mj_doc();
        let existing: Vec<String> = pi_link::models_json::providers(&doc)
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
            &mut doc,
            &name,
            serde_json::json!({ "name": name }),
        );
        self.mj_write(&doc, cx);
        self.mj_select(format!("mj:p:{name}"), cx);
        cx.notify();
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
        let mut doc = Self::mj_doc();
        let mut entry = pi_link::models_json::provider_entry(&doc, &old_name)
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
            if let Err(e) =
                pi_link::models_json::rename_provider(&mut doc, &old_name, &name)
            {
                return self.mc_set_error(&e, cx);
            }
            // 凭据跟名搬家：钥匙库条目 + auth.json 标记条目（无旧条目静默跳过）
            if let Some(secret) = pi_link::auth_json::resolve_secret(&old_name) {
                if pi_link::auth_json::store_secret(&name, &secret).is_ok() {
                    let _ = pi_link::auth_json::delete_secret(&old_name);
                }
            }
        }
        pi_link::models_json::upsert_provider(&mut doc, &name, entry);
        self.mj_write(&doc, cx);
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = format!("mj:p:{name}");
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
        // 凭据条目一并清（042-1：PF 标记条目 + 钥匙库；用户手写的明文/oauth
        // 不动——delete_secret 自带标记守卫）
        let _ = pi_link::auth_json::delete_secret(&name);
        self.cfg.mj_speed = None;
        self.mj_write(&doc, cx);
        // 内存目录同步摘掉（否则进程答案 / 启动 globals 里的模型会让列表项
        // 继续挂着）。列表成员资格 = models.json 里有没有条目，无隐藏清单。
        self.drop_provider_from_catalog(&name);
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

    /// 点模型名 → 打开模型详情弹窗（042-1 UI：内置/自定义列表同一入口）。
    /// 该模型在 `models.json` 里已有条目（自定义商 / 早已物化）→ 索引模式；
    /// 否则是 pi 的内置模型 → 覆盖模式（modelOverrides）。
    pub(crate) fn mj_open_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        let doc = Self::mj_doc();
        let ix = pi_link::models_json::provider_entry(&doc, &provider)
            .map(pi_link::models_json::provider_models)
            .unwrap_or_default()
            .iter()
            .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
        match ix {
            Some(ix) => self.mj_select(format!("mj:m:{provider}:{ix}"), cx),
            None => self.mj_select(override_section(&provider, &id), cx),
        }
    }

    pub(crate) fn mj_add_model(&mut self, provider: String, cx: &mut Context<Self>) {
        let mut doc = Self::mj_doc();
        let ix = pi_link::models_json::add_model(&mut doc, &provider);
        self.mj_write(&doc, cx);
        self.mj_select(format!("mj:m:{provider}:{ix}"), cx);
    }

    /// 把模型编辑器字段写回缓冲里的模型条目（精简版：编辑器只拥有 id /
    /// 上下文窗口 / 最大输出 / 能力勾选四个键；名称、价格、reasoning、兼容
    /// 等交【自动填参】从 models.dev 落——既有值原样保留不碰）。
    pub(crate) fn mj_apply_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        // mj_commit_model_form 自己就落盘，这里不必再 flush
        let _ = self.mj_commit_model_form(&provider, ix, cx);
        cx.notify();
    }

    /// 表单四键写进账本（保存与【自动填参】共用）：现读 → 改本条 → 立刻写盘。
    fn mj_commit_model_form(
        &mut self,
        provider: &str,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(st) = self.dlg.settings.clone() else {
            return false;
        };
        let (id, ctx, image, reasoning, max_tokens) = {
            let s = st.read(cx);
            (
                s.mj_id.read(cx).value().trim().to_string(),
                s.mj_ctx.read(cx).value().trim().to_string(),
                s.mj_image,
                s.mj_reasoning,
                s.mj_max_tokens.read(cx).value().trim().to_string(),
            )
        };
        if id.is_empty() {
            self.mc_set_error(tr("custom_models.model_id_required"), cx);
            return false;
        }
        let mut doc = Self::mj_doc();
        let Some(entry) = pi_link::models_json::provider_entry(&doc, provider).cloned() else {
            return false;
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
        // 推理：**恒显式写 bool**——ensure_model_defaults 对缺键补 true，删键
        // 会被悄悄改回“开”（「推理强度强行 off」根因）。
        obj.insert("reasoning".into(), serde_json::json!(reasoning));
        // 能力：文本恒在；仅 text = 删键走 ensure_model_defaults 缺省
        let mut input = vec!["text"];
        if image {
            input.push("image");
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
        pi_link::models_json::update_model(&mut doc, provider, ix, model);
        self.mj_write(&doc, cx);
        true
    }

    pub(crate) fn mj_delete_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        let mut doc = Self::mj_doc();
        pi_link::models_json::remove_model(&mut doc, &provider, ix);
        self.mj_write(&doc, cx);
        self.mj_select(format!("mj:p:{provider}"), cx);
    }

    /// 能力勾选切换（图片；文本恒选无动作）。
    pub(crate) fn mj_toggle_image(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_image = !s.mj_image;
                cx.notify();
            });
        }
    }

    /// 推理勾选切换。
    pub(crate) fn mj_toggle_reasoning(&mut self, cx: &mut Context<Self>) {
        if let Some(st) = self.dlg.settings.clone() {
            st.update(cx, |s, cx| {
                s.mj_reasoning = !s.mj_reasoning;
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
                            let mut doc = Self::mj_doc();
                            Self::mj_merge_catalog_into_ledger(&mut doc, &provider_bg, &catalog);
                            c.mj_write(&doc, cx);
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
        let Some(entry) = pi_link::models_json::provider_entry(&Self::mj_doc(), &provider).cloned()
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
                    // 目录直接合并进账本（自定义商唯一真相源）
                    let mut doc = Self::mj_doc();
                    Self::mj_merge_catalog_into_ledger(&mut doc, &provider, &catalog);
                    c.mj_write(&doc, cx);
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
            &Self::mj_doc(),
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
        // 查询键与 baseUrl 取自**已保存**条目（provider 名/baseUrl 参与 author 推断）
        let doc = Self::mj_doc();
        let Some(entry) = pi_link::models_json::provider_entry(&doc, &provider).cloned() else {
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
        if query.is_empty() {
            self.mc_set_error(tr("custom_models.model_id_required"), cx);
            return;
        }
        let base_url = entry
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.cfg.mj_models_dev_state = MjCatalogState::Running;
        cx.notify();
        let p_bg = provider.clone();
        let b_bg = base_url.clone();
        let q_bg = query.clone();
        cx.spawn(async move |this, cx| {
            let resolved = cx
                .background_executor()
                .spawn(async move { pi_link::openrouter::resolve(&p_bg, &b_bg, &q_bg) })
                .await;
            // 目录无此 id 是常态错误（本地化文案）；网络错带原文
            let shown = match &resolved {
                Ok(_) => None,
                Err(pi_link::openrouter::FillError::NoMatch) => {
                    Some(tr("models.catalog_no_exact_match").to_string())
                }
                Err(pi_link::openrouter::FillError::Net(m)) => Some(m.clone()),
            };
            let _ = this.update(cx, |c, cx| match resolved {
                Ok(e) => {
                    // 等待期间用户可能又改了表单：再落一次，回填不吃回刚敲的值
                    if c.mj_commit_model_form(&provider, ix, cx) {
                        c.mj_catalog_apply(&e, &provider, ix, cx);
                    } else {
                        c.cfg.mj_models_dev_state = MjCatalogState::Idle;
                        cx.notify();
                    }
                }
                Err(_) => {
                    c.cfg.mj_models_dev_state = MjCatalogState::Err(shown.unwrap_or_default());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn mj_catalog_apply(
        &mut self,
        e: &pi_link::openrouter::Entry,
        provider: &str,
        ix: usize,
        cx: &mut Context<Self>,
    ) {
        let mut doc = Self::mj_doc();
        let Some(entry) = pi_link::models_json::provider_entry(&doc, provider).cloned() else {
            return;
        };
        let Some(model) = entry.get("models").and_then(|m| m.get(ix)).cloned() else {
            return;
        };
        // 点填参就是“把这个 ID 的参数拉下来填上”：页面可见字段（上下文/最大
        // 输出/能力/推理）一律按目录覆盖，旧值属于上一个模型；名称/价格页面
        // 没有，只填空不动已有值。
        let mut model = model;
        // 成功不占提示栏（用户定稿）；只填空不覆盖的名称/价格也在这一步落。
        pi_link::openrouter::fill_from_entry(&mut model, e);
        pi_link::models_json::update_model(&mut doc, provider, ix, model);
        self.mj_write(&doc, cx);
        self.cfg.mj_models_dev_state = MjCatalogState::Done;
        // 表单重选回显（新字段进输入框）
        self.mj_select(format!("mj:m:{provider}:{ix}"), cx);
        cx.notify();
    }
}

// ---------------------------------------------------------------------------

// section key helpers
// ---------------------------------------------------------------------------

/// 内置模型的覆盖式详情 section：`mj:o:{provider}:{model_id}`。
/// 落盘目标是 `providers.<p>.modelOverrides.<id>`（pi 的部分覆盖语义）——只改
/// 指定字段、其余继承内置定义，**不会**像 `models[]` 那样把整条模型定义换成
/// PF 的缺省值（cost/reasoning/input 被冲掉）。
fn override_section(provider: &str, id: &str) -> String {
    format!("mj:o:{provider}:{id}")
}

/// `mj:o:{provider}:{model_id}` → (provider, model_id)。模型 id 可含 ':'（如
/// `nvidia/x:free`），所以只切第一个 ':'；provider id 不含 ':'。
fn mj_parse_override(section: &str) -> Option<(String, String)> {
    let rest = section.strip_prefix("mj:o:")?;
    let (p, id) = rest.split_once(':')?;
    (!p.is_empty() && !id.is_empty()).then(|| (p.to_string(), id.to_string()))
}

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

/// 自定义商【删除】确认浮层（042-1 UI：删除 = 连同其下所有模型）。
pub(crate) fn mj_delete_confirm_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let name = chat.cfg.mc_mj_delete_confirm.clone()?;
    let weak = cx.entity().downgrade();
    let message = crate::i18n::tf("custom_models.delete_confirm", &[("name", name)]);
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.mj_delete_provider_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.mj_delete_provider_close(true, cx));
        },
    ))
}

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
    // 042-1 门禁：测速只制约【+模型】。【保存】随时可用——新建商可以先把
    // key / baseUrl 存下来（连通性由测速与自动填参负责），否则「测速失败就
    // 什么都存不了」会把半成品配置锁死。
    let add_ok = chat
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

    // name 行：标题列 + 控件列（input 半宽；[删除][保存] 与 name input 同列右对齐）
    let remove_provider = name.to_string();
    let name_owned = name.to_string();
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(super::models::detail_button(
                            "mj-del",
                            weak,
                            &tr("action.delete"),
                            crate::ui::button::BtnVariant::Danger,
                            false,
                            move |c, cx| c.mj_ask_delete_provider(remove_provider.clone(), cx),
                        ))
                        .child(super::models::detail_button(
                            "mj-apply",
                            weak,
                            &tr("action.save"),
                            crate::ui::button::BtnVariant::Primary,
                            false,
                            move |c, cx| c.mj_apply_provider(name_owned.clone(), cx),
                        )),
                )
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
                    "mj-add-model",
                    weak,
                    &format!("+ {}", tr("settings.tab.models")),
                    crate::ui::button::BtnVariant::Secondary,
                    !add_ok,
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
            // 右侧聚簇：只看free（开关在左、文字在右，6px）+ 刷新钮（10px）
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child({
                        // 只看free：与内置详情共用同一面板状态
                        let weak_free = weak.clone();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
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
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(11.))
                                    .text_color(rgb(t.text_dim))
                                    .child(tr("models.free_only")),
                            )
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
            ),
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
        pi_link::models_json::provider_entry(&Chat::mj_doc(), provider)
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
                                        c.mj_open_model(prov.clone(), id.clone(), cx);
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
/// 能力勾选（文本恒选不可改 / 图片）→ **唯一一条**提示栏（表单错误 /
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
    // 两种目标：`mj:m:{prov}:{ix}`（models.json 条目）/ `mj:o:{prov}:{id}`
    //（内置模型覆盖，写 modelOverrides）
    let over = mj_parse_override(&selected);
    let (name, ix) = match &over {
        Some((p, _)) => (p.clone(), 0usize),
        None => {
            let rest = selected.strip_prefix("mj:m:")?;
            let (n, i) = rest.rsplit_once(':')?;
            (n.to_string(), i.parse::<usize>().ok()?)
        }
    };
    let over_id = over.as_ref().map(|(_, id)| id.clone());
    // 内置模型 = 只读参数视图：不给自动填参 / 测试 / 保存 / 移除，输入框禁用
    let read_only = over_id.is_some();
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
        .rounded_t(px(12.))
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
            testing || over_id.is_some(),
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
    // 填参成功不占提示栏（用户定稿）：只有查不到/网络错才提示。
    let filling = matches!(chat.cfg.mj_models_dev_state, MjCatalogState::Running);
    let catalog_line: Option<(String, bool)> = match &chat.cfg.mj_models_dev_state {
        MjCatalogState::Done => None,
        MjCatalogState::Err(e) => Some((e.clone(), false)),
        MjCatalogState::Running | MjCatalogState::Idle => None,
    };
    let status_line: Option<(String, bool)> = error
        .as_ref()
        .map(|e| (e.clone(), false))
        .or(test_note.clone())
        .or(catalog_line);

    // id 必须与底层 Provider 详情（mc-detail）区分：同一帧渲染两份同 id 容器
    // 会让 gpui 的元素状态互相踩（点击落不到弹窗内的按钮）
    let detail = detail_shell("mj-model-detail")
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
                .children((!read_only).then(|| {
                    let n = name.clone();
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_end()
                        .justify_end()
                        .child({
                            let _ = &n;
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
                                    let _ = weak_fill
                                        .update(cx, |c, cx| c.mj_catalog_fill(n.clone(), ix, cx));
                                })
                                .into_any_element()
                            }
                        })
                })),
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
        // 推理（用户定稿：能力之前；单勾选落 model.reasoning）
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(mj_cap_checkbox(
                    &tr("models.reasoning"),
                    d.mj_reasoning,
                    &weak,
                    (!read_only).then_some(
                        |c: &mut Chat, cx: &mut Context<Chat>| c.mj_toggle_reasoning(cx),
                    ),
                )),
        )
        // 能力：文本恒选不可改；图片勾选落 model.input
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
                            (!read_only).then_some(
                                |c: &mut Chat, cx: &mut Context<Chat>| c.mj_toggle_image(cx),
                            ),
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
                .when(!read_only, |d| d.h(px(500.)))
                .when(read_only, |d| d.max_h(px(560.)))
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
                .children((!read_only).then_some(footer)),
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
