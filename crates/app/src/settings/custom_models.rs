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
    pub(crate) fn mj_doc() -> serde_json::Value {
        pi_link::models_json::read().unwrap_or_else(|_| serde_json::json!({ "providers": {} }))
    }

    /// 写盘（整份 read→mutate→write 的末步；锁内 + 不可读拒写）。
    fn mj_write(&mut self, doc: &serde_json::Value, cx: &mut Context<Self>) {
        if let Err(e) = pi_link::models_json::save(doc) {
            self.mc_set_error(&e, cx);
        }
    }

    /// 换选中项（section 驱动整页回填）：现读 `models.json` 后交给
    /// [`Self::mj_fill_from`]。点列表进来的路径走 [`Self::mj_open_model`]
    /// （**同一份 doc 里选索引 + 回填**，不受并发写盘影响）。
    pub(crate) fn mj_select(&mut self, section: String, cx: &mut Context<Self>) {
        let doc = Self::mj_doc();
        self.mj_fill_from(section, &doc, cx);
    }

    /// section 回填本体：`doc` 必须是与索引解析同源的快照。
    fn mj_fill_from(&mut self, section: String, doc: &serde_json::Value, cx: &mut Context<Self>) {
        let over = mj_parse_override(&section);
        let (name, ix) = match &over {
            Some((p, _)) => (p.clone(), None),
            None => mj_parse_section(&section),
        };
        let entry = pi_link::models_json::provider_entry(doc, &name)
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
        // 提示栏只属于本次操作：换选中项即清（否则上次的「未能获取该模型参数」
        // 会一直挂到下次填参）
        self.cfg.mj_models_dev_state = MjCatalogState::Idle;
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
        // 登记：这几个表单变量现在装的是哪个 section 的值
        self.cfg.mj_form_for = Some(section);
    }

    /// 模型详情的表单变量清空（关闭详情时调用）：id / 上下文 / 最大输出 /
    /// 能力勾选全部归零，并注销「表单属于哪个 section」——下一个模型绝不会
    /// 继承上一个模型的任何值（此前正是这里没清，出现「点别的行还是旧值」）。
    pub(crate) fn mj_clear_model_form(&mut self, cx: &mut Context<Self>) {
        self.cfg.mj_form_for = None;
        self.cfg.mj_fill_entry = None;
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        st.update(cx, |s, cx| {
            for ti in [&s.mj_id, &s.mj_ctx, &s.mj_max_tokens] {
                ti.update(cx, |ti, cx| ti.set_value(String::new(), cx));
            }
            s.mj_image = false;
            s.mj_reasoning = false;
            cx.notify();
        });
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

    /// 弹窗「自定义」卡进来：**纯 UI 草稿**（section = `mj:new`），不写任何
    /// 文件——列表里的对象只在【保存】时诞生（042-1 用户定稿：没保存前不该
    /// 在列表上看见新对象）。表单清空、无删除钮，名字在头行输入。
    pub(crate) fn mj_open_add(&mut self, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        self.cfg.mj_test = None;
        self.cfg.mj_speed = None;
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        st.update(cx, |s, cx| {
            s.section = "mj:new".to_string();
            s.error = None;
            s.mj_image = false;
            s.mj_reasoning = false;
            s.mj_key_visible = false;
            s.mj_name
                .update(cx, |ti, cx| ti.set_value(String::new(), cx));
            s.mj_base
                .update(cx, |ti, cx| ti.set_value(String::new(), cx));
            s.mj_key.update(cx, |ti, cx| {
                ti.set_value(String::new(), cx);
                ti.set_masked(true, cx);
            });
            // 模型行用不到（新商无模型），一并清空且可编辑
            s.mj_id.update(cx, |ti, cx| {
                ti.set_value(String::new(), cx);
                ti.set_disabled(false, cx);
            });
            s.mj_ctx.update(cx, |ti, cx| {
                ti.set_value(String::new(), cx);
                ti.set_disabled(false, cx);
            });
            s.mj_max_tokens.update(cx, |ti, cx| {
                ti.set_value(String::new(), cx);
                ti.set_disabled(false, cx);
            });
            cx.notify();
        });
        cx.notify();
    }

    /// 测速/刷新探测到的官方清单 → **清单仓**���custom-provider.json + 内存镜像）。
    /// 2026-11-01 定稿：探测只补缺不覆盖（手调参数保留）、不整批入账本；用户
    /// 勾选哪个才把哪个复制进 models.json（[`Self::mj_materialize_selection`]）。
    fn mj_store_probe_catalog(&mut self, provider: &str, catalog: &[pi_link::protocol::ModelInfo]) {
        crate::services::custom_models_store::merge_probed(
            &mut self.cfg.mj_probe_cache,
            provider,
            catalog,
        );
        let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
    }

    /// 勾选物化（2026-11-01 定稿，自定义商专用）：enable → 把该模型以完整
    /// definition 追加进 models.json `providers.<p>.models[]`（id/api/baseUrl
    /// 取骨架 + name/contextWindow 取展示条目 + ensure_model_defaults）；
    /// disable → 从 models[] 移除；models[] 空时**骨架保留**（baseUrl 在，
    /// pi applyModelsJson 不 throw）。内置商不走这里（pi 静态注册表即持久）。
    /// 返回 Err 文案供调用方挂错误条。
    pub(crate) fn mj_materialize_selection(
        &mut self,
        provider: &str,
        id: &str,
        enable: bool,
    ) -> Result<(), String> {
        let mut doc = Self::mj_doc();
        let Some(mut entry) = pi_link::models_json::provider_entry(&doc, provider).cloned() else {
            return Ok(()); // 不是账本里的商（内置/扩展）：不物化
        };
        let existing_ix = pi_link::models_json::provider_models(&entry)
            .iter()
            .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(id));
        if enable {
            if existing_ix.is_some() {
                return Ok(());
            }
            // 定义优先从清单仓原样复制（手填/手调参数无损，off/on 往返不失）；
            // 仓里没有（从未在仓）才按展示条目 + 骨架最小构造
            if let Some(mut stashed) = crate::services::custom_models_store::definition(
                &self.cfg.mj_probe_cache,
                provider,
                id,
            )
            .cloned()
            {
                // 骨架 api/baseUrl 兜底（仓定义缺了会撞 modelFromJson 硬 throw）
                if let Some(o) = stashed.as_object_mut() {
                    if o.get("api")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .is_empty()
                    {
                        if let Some(api) = entry.get("api").and_then(|v| v.as_str()) {
                            o.insert("api".into(), serde_json::Value::String(api.to_string()));
                        }
                    }
                    if o.get("baseUrl")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .is_empty()
                    {
                        if let Some(base) = entry.get("baseUrl").and_then(|v| v.as_str()) {
                            o.insert(
                                "baseUrl".into(),
                                serde_json::Value::String(base.to_string()),
                            );
                        }
                    }
                }
                pi_link::models_json::ensure_model_defaults(&mut stashed);
                if !entry.get("models").is_some_and(|m| m.is_array()) {
                    entry["models"] = serde_json::json!([]);
                }
                if let Some(arr) = entry.get_mut("models").and_then(|m| m.as_array_mut()) {
                    arr.push(stashed);
                }
                return Ok(());
            }
            // 展示条目（探测缓存 / 目录）补 name/contextWindow
            let shown = self
                .provider_models_for_list(provider)
                .into_iter()
                .find(|m| m.id == id);
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), serde_json::Value::String(id.to_string()));
            if let Some(api) = entry.get("api").and_then(|v| v.as_str()) {
                obj.insert("api".into(), serde_json::Value::String(api.to_string()));
            }
            if let Some(base) = entry.get("baseUrl").and_then(|v| v.as_str()) {
                obj.insert(
                    "baseUrl".into(),
                    serde_json::Value::String(base.to_string()),
                );
            }
            if let Some(m) = &shown {
                if !m.name.is_empty() {
                    obj.insert("name".into(), serde_json::Value::String(m.name.clone()));
                }
                if let Some(cw) = m.context_window {
                    obj.insert("contextWindow".into(), serde_json::json!(cw));
                }
            }
            let mut model = serde_json::Value::Object(obj);
            // 仓定义（探测的最小形状）可能缺 api/baseUrl——modelFromJson 硬
            // throw，骨架补齐（已有值不动）
            if let Some(o) = model.as_object_mut() {
                if o.get("api")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    if let Some(api) = entry.get("api").and_then(|v| v.as_str()) {
                        o.insert("api".into(), serde_json::Value::String(api.to_string()));
                    }
                }
                if o.get("baseUrl")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    if let Some(base) = entry.get("baseUrl").and_then(|v| v.as_str()) {
                        o.insert(
                            "baseUrl".into(),
                            serde_json::Value::String(base.to_string()),
                        );
                    }
                }
            }
            pi_link::models_json::ensure_model_defaults(&mut model);
            if !entry.get("models").is_some_and(|m| m.is_array()) {
                entry["models"] = serde_json::json!([]);
            }
            if let Some(arr) = entry.get_mut("models").and_then(|m| m.as_array_mut()) {
                arr.push(model);
            }
        } else {
            let Some(ix) = existing_ix else {
                return Ok(());
            };
            if let Some(arr) = entry.get_mut("models").and_then(|m| m.as_array_mut()) {
                arr.remove(ix);
            }
            // 空 models[] 删键（骨架 baseUrl/api/name 保留，pi 不 throw）
            if entry
                .get("models")
                .and_then(|m| m.as_array())
                .is_some_and(|a| a.is_empty())
            {
                if let Some(o) = entry.as_object_mut() {
                    o.remove("models");
                }
            }
        }
        pi_link::models_json::upsert_provider(&mut doc, provider, entry);
        pi_link::models_json::save(&doc)
    }

    /// 把编辑器字段应用进缓冲并落盘（provider 名称改动走重命名）。
    pub(crate) fn mj_apply_provider(&mut self, old_name: String, cx: &mut Context<Self>) {
        self.mj_apply_provider_inner(old_name, true, cx);
    }

    /// 测速自动保存专用（P2-3）：`honor_form_name=false` 时表单改名**不生效**
    /// ——按盘上老名保存（改名走头行【保存】，测速只保存连通性字段）。
    fn mj_apply_provider_inner(
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

    /// 点模型名 → 打开模型详情弹窗（042-1 UI：内置/自定义列表同一入口）。
    /// 该模型在 `models.json` 里已有条目（自定义商 / 早已物化）→ 索引模式；
    /// 否则是 pi 的内置模型 → 覆盖模式（modelOverrides）。
    pub(crate) fn mj_open_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        // 记下来源 section：弹窗关闭要退回这里（内置商的模型来自合并列表 /
        // 内置详情，那只属于 section = 商 id，不是 `mj:p:商`）
        self.cfg.mj_return = self
            .dlg
            .settings
            .as_ref()
            .map(|st| st.read(cx).section.clone())
            .filter(|s| !s.is_empty());
        // 一份 doc 内完成「选索引 → 回填」：以前是这里读一次盘、mj_select 再读
        // 一次，中间若有异步写盘（测速合并目录会 append 模型）把数组挪了位，
        // 弹出来的就不是点的那个模型了
        let doc = Self::mj_doc();
        let models = pi_link::models_json::provider_entry(&doc, &provider)
            .map(pi_link::models_json::provider_models)
            .unwrap_or_default();
        let field = mj_field;
        // 精确 id 命中 → 索引模式（编辑账本条目）。否则只在「同一 id 的另
        // 一种写法」（忽略大小写 / slug 等价，**名称不参与**）且命中**唯一**时
        // 才认；多条相似或无命中 → 覆盖模式（只读的 pi 定义视图）。
        // 宁可不猜：曾因拿 name 当匹配维度，点 GLM-4.6V 弹出了另一个模型
        //（账本里 id=glm-5.3-flash-free、name=GLM-4.6V）。
        let ix = models
            .iter()
            .position(|m| field(m, "id") == id)
            .or_else(|| {
                let mut hit: Option<usize> = None;
                for (i, m) in models.iter().enumerate() {
                    if pi_link::model_ref::matches_ref(field(m, "id"), &id) {
                        if hit.is_some() {
                            return None;
                        }
                        hit = Some(i);
                    }
                }
                hit
            });
        let section = match ix {
            Some(ix) => format!("mj:m:{provider}:{ix}"),
            None => override_section(&provider, &id),
        };
        self.mj_fill_from(section, &doc, cx);
    }

    pub(crate) fn mj_add_model(&mut self, provider: String, cx: &mut Context<Self>) {
        // 草稿行不是「从列表进详情」：关闭退回 `mj:p:商`（mj_return=None 即该默认）
        self.cfg.mj_return = None;
        // 草稿行**不预写占位**（P1-1）：validate 拒绝空 id，预写必失败且
        // 后续按 ix 保存时越界静默丢数据。ix = 盘上现有模型数，保存时走
        // mj_commit_model_form 的追加分支落盘；保存前列表不显示（与
        // 「没保存前不该在列表上看见新对象」的 042-1 定稿一致）
        let ix = pi_link::models_json::provider_entry(&Self::mj_doc(), &provider)
            .map(pi_link::models_json::provider_models)
            .unwrap_or_default()
            .len();
        self.mj_select(format!("mj:m:{provider}:{ix}"), cx);
    }

    /// 把模型编辑器字段写回缓冲里的模型条目（精简版：编辑器只拥有 id /
    /// 上下文窗口 / 最大输出 / 能力勾选四个键；名称、价格、reasoning、兼容
    /// 等交【自动填参】从 models.dev 落——既有值原样保留不碰）。
    pub(crate) fn mj_apply_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        // mj_commit_model_form 自己就落盘，这里不必再 flush
        if !self.mj_commit_model_form(&provider, ix, cx) {
            cx.notify();
            return;
        }
        // 内存目录同步：手动新建的商（自动测速拉不到目录，初始 0 条）保存模型后
        // 列表必须立刻出现这一条——以前只写盘不刷新，「可用模型」恒 0 条
        self.refresh_provider_catalog(&provider);
        // 保存成功 → 自动关闭详情页（用户定稿）：走与 ✕ 同一条退路（退回来源
        // section，并清空模型表单变量）
        let target = self
            .cfg
            .mj_return
            .clone()
            .unwrap_or_else(|| format!("mj:p:{provider}"));
        self.cfg.mj_return = None;
        self.mj_clear_model_form(cx);
        self.mj_select(target, cx);
    }

    /// 表单四键写进账本（保存与【自动填参】共用）：现读 → 改本条 → 立刻写盘。
    fn mj_commit_model_form(&mut self, provider: &str, ix: usize, cx: &mut Context<Self>) -> bool {
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
        // 填参结果里的名称/价格在这里补（页面没有这两项，只填空不覆盖）；
        // 仅当「填参时查的 id」与本次保存的 id 一致才用，避免串到别的模型
        if let Some((queried, e)) = self.cfg.mj_fill_entry.clone() {
            if model.get("id").and_then(|v| v.as_str()) == Some(queried.as_str()) {
                pi_link::openrouter::fill_name_cost(&mut model, &e);
            }
        }
        // pi 对扩展注册模型要求完整形状（缺 input/reasoning/cost 聊天即崩）：
        // 编辑器删键后的缺省在此统一补齐
        pi_link::models_json::ensure_model_defaults(&mut model);
        // ① 清单仓恒写（2026-11-01 定稿：完整定义的真相在 PF 侧）
        let model_id = model
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if model_id.is_empty() {
            return false;
        }
        crate::services::custom_models_store::upsert_model(
            &mut self.cfg.mj_probe_cache,
            provider,
            &model_id,
            model.clone(),
        );
        let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
        // ② models.json 按投影规则：编辑已勾选的行 → 原位更新；【+模型】
        // 手填（草稿追加，显式创建 = 勾选）→ 追加；未勾选的行保存 → 只进仓，
        // 不进 models.json（投影规则）
        let models_len = entry
            .get("models")
            .and_then(|m| m.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if ix < models_len {
            let row_id = entry["models"][ix]
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if row_id == model_id {
                pi_link::models_json::update_model(&mut doc, provider, ix, model);
            } else {
                // 行 id 被改名编辑：原位替换即可（该行本来在投影里）
                pi_link::models_json::update_model(&mut doc, provider, ix, model);
            }
        } else if ix == models_len {
            pi_link::models_json::push_model(&mut doc, provider, model);
        } else {
            return false;
        }
        self.mj_write(&doc, cx);
        true
    }

    pub(crate) fn mj_delete_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        // 删掉后回到商详情：来源 section 一并清（否则下次弹窗会退回旧视图）
        self.cfg.mj_return = None;
        let mut doc = Self::mj_doc();
        // 删之前先记住 id：内存目录按 provider+id 摘（和磁盘同一步）
        let id = pi_link::models_json::provider_entry(&doc, &provider)
            .and_then(|e| e.get("models"))
            .and_then(|m| m.get(ix))
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        pi_link::models_json::remove_model(&mut doc, &provider, ix);
        self.mj_write(&doc, cx);
        // 清单仓同步删（模型详情【移除】= 删除该模型，不是取消勾选）
        if let Some(id) = &id {
            crate::services::custom_models_store::remove_model(
                &mut self.cfg.mj_probe_cache,
                &provider,
                id,
            );
            let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
        }
        // 内存目录同步摘掉该模型（否则 Provider 详情的可用模型列表还挂着它）
        if let Some(id) = id {
            self.pool
                .globals
                .models
                .retain(|m| !(m.provider == provider && m.id == id));
            for models in self.pool.models_by_cwd.values_mut() {
                models.retain(|m| !(m.provider == provider && m.id == id));
            }
        }
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
                .and_then(|v| pi_link::credentials::KeyringVault.get(&v).ok().flatten())
                .ok_or_else(|| tr("model.key_not_in_vault").to_string()),
            pi_link::credentials::StoreMode::Ref => {
                Err(tr("models.adv_ref_unresolved").to_string())
            }
        }
    }

    /// 测速（042-1 / 2026-10-31 三模块定稿）：读**表单**值做连通探测
    ///（[`pi_link::probe::probe_connect`]：baseUrl + key 即可，不依赖 api
    /// type——模块①的 infer_api 只服务 pi 注册，与探测解耦）。能通 =
    /// ① 自动保存全表单 ② 目录合并进账本并刷新内存；连通但无目录 =
    /// PassedEmpty 提示手动 +模型；不通 = 红条错误。
    pub(crate) fn mj_speedtest_start(&mut self, provider: String, cx: &mut Context<Self>) {
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let base = {
            let s = st.read(cx);
            s.mj_base.read(cx).value().trim().to_string()
        };
        let key_text = st.read(cx).mj_key.read(cx).value().trim().to_string();
        self.mc_clear_error(cx);
        if base.is_empty() {
            return self.mc_set_error(tr("models.import_base_url_required"), cx);
        }
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
                .spawn(async move { pi_link::probe::probe_connect(&base, &key) })
                .await;
            let _ = this.update(cx, |c, cx| {
                match outcome {
                    pi_link::probe::ProbeOutcome::Ok {
                        catalog,
                        latency_ms,
                        ..
                    } => {
                        // 1、自动保存（表单全字段）。草稿（provider_bg 为空，
                        // 表单一开头就填着名字）→ 以 old_name="" 新建；已存在
                        // 的商按**盘上老名字**保存，表单改名不随测速生效
                        //（honor_form_name=false，P2-3；改名走头行【保存】）
                        let mut form_name = String::new();
                        if let Some(st) = c.dlg.settings.clone() {
                            form_name = st.read(cx).mj_name.read(cx).value().trim().to_string();
                            c.mj_apply_provider_inner(provider_bg.clone(), false, cx);
                        }
                        // 草稿测速后 section 已切成 mj:p:{name}：后续合并用真名
                        let target = if provider_bg.is_empty() {
                            form_name
                        } else {
                            provider_bg.clone()
                        };
                        // 没填名字 = 存不了；别往 models.json 里塞空名条目
                        if target.is_empty() {
                            c.cfg.mj_speed = None;
                            c.mc_set_error(tr("custom_models.name_required"), cx);
                            return;
                        }
                        // 2、自动拉取：目录进**展示缓存**（2026-11-01 定稿：不整批
                        // 入账本，勾选哪个才物化哪个）；可用模型区经
                        // provider_models_for_list 即时列出
                        if catalog.is_empty() {
                            c.cfg.mj_speed = Some(MjSpeedState::PassedEmpty);
                        } else {
                            c.mj_store_probe_catalog(&target, &catalog);
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
        // key 解析：PF 保存的商 key 在凭据路径（auth.json 标记 → 钥匙库），
        // **不在 models.json**——只读 entry.apiKey 会拿到空串、探测必 401。
        // 回落 entry.apiKey 兼容用户手写明文（P1-2）。
        let key_text = pi_link::auth_json::resolve_secret(&provider).unwrap_or_else(|| {
            entry
                .get("apiKey")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        });
        self.mc_clear_error(cx);
        if base.is_empty() {
            return self.mc_set_error(tr("models.import_base_url_required"), cx);
        }
        let key = match Self::mj_form_key(&key_text) {
            Ok(k) => k,
            Err(e) => return self.mc_set_error(&e, cx),
        };
        // 与测速同款三模块：连通探测不依赖条目的 api 字段（2026-10-31 定稿）
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { pi_link::probe::probe_connect(&base, &key) })
                .await;
            let _ = this.update(cx, |c, cx| match outcome {
                pi_link::probe::ProbeOutcome::Ok { catalog, .. } if !catalog.is_empty() => {
                    // 目录进展示缓存（2026-11-01 定稿：勾选才物化进账本）
                    c.mj_store_probe_catalog(&provider, &catalog);
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
        let headers: Vec<(String, String)> =
            pi_link::models_json::provider_entry(&Self::mj_doc(), &provider)
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
                    pi_link::model_test::TestOutcome::Ok {
                        latency_ms,
                        status,
                        text,
                    } => MjTestState::Ok {
                        latency_ms,
                        status,
                        text,
                    },
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
        // 查询键 = **表单里的模型ID**（不落盘）。填参只是「把这个 ID 的参数
        // 拉下来填到表单里」，落盘只在【保存】时发生——以前这里先调
        // mj_commit_model_form 落了一次盘，等于点填参就偷偷保存。
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        let id = st.read(cx).mj_id.read(cx).value().trim().to_string();
        if id.is_empty() {
            self.mc_set_error(tr("custom_models.model_id_required"), cx);
            return;
        }
        // 版本名：账本条目（若有）优先，缺则回落内存目录里同商的官方 name。
        // 官方 id 常常只是家族名（DeepSeek 的 deepseek-flash），版本名才落到
        // 具体型号；两个检索键并列交给 openrouter 裁决（见 query_candidates）。
        let doc = Self::mj_doc();
        let entry = pi_link::models_json::provider_entry(&doc, &provider).cloned();
        let mut name = entry
            .as_ref()
            .and_then(|e| e.get("models"))
            .and_then(|m| m.get(ix))
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            if let Some(m) = self
                .catalog_for(self.project())
                .into_iter()
                .find(|m| m.provider == provider && m.id == id)
            {
                name = m.name;
            }
        }
        let queries = pi_link::model_ref::query_candidates(&id, &name);
        let base_url = entry
            .as_ref()
            .and_then(|e| e.get("baseUrl"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.cfg.mj_models_dev_state = MjCatalogState::Running;
        cx.notify();
        let p_bg = provider.clone();
        let b_bg = base_url.clone();
        let q_bg = queries.clone();
        cx.spawn(async move |this, cx| {
            let resolved = cx
                .background_executor()
                .spawn(async move { pi_link::openrouter::resolve_many(&p_bg, &b_bg, &q_bg) })
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
                Ok(e) => c.mj_catalog_fill_to_form(&e, &id, cx),
                Err(_) => {
                    c.cfg.mj_models_dev_state = MjCatalogState::Err(shown.unwrap_or_default());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 填参结果 → **只写表单**（不落盘）：上下文/最大输出/能力/推理按目录覆盖，
    /// 并把条目暂存起来，等【保存】时再补名称/价格（见 `mj_commit_model_form`）。
    fn mj_catalog_fill_to_form(
        &mut self,
        e: &pi_link::openrouter::Entry,
        queried_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        st.update(cx, |s, cx| {
            if let Some(n) = e.context_window {
                s.mj_ctx
                    .update(cx, |ti, cx| ti.set_value(n.to_string(), cx));
            }
            if let Some(n) = e.max_tokens {
                s.mj_max_tokens
                    .update(cx, |ti, cx| ti.set_value(n.to_string(), cx));
            }
            if !e.input.is_empty() {
                s.mj_image = e.input.iter().any(|x| x == "image");
            }
            s.mj_reasoning = e.reasoning;
        });
        // 名称/价格留到保存时落盘；id 变了就不复用（防止串到别的模型）
        self.cfg.mj_fill_entry = Some((queried_id.to_string(), e.clone()));
        self.cfg.mj_models_dev_state = MjCatalogState::Done;
        cx.notify();
    }
}

// ---------------------------------------------------------------------------

// section key helpers
// ---------------------------------------------------------------------------

/// PF 托管判据（2026-10-31 用户定稿）：auth.json 里有 PF 标记条目
/// （`!'/…/pif-secret.exe' get <provider>` 命令引用——人工不可能手写该命令
/// 形态）= PF 创建的条目；否则视为 models.json 手写。**只用于落盘分流**
/// （UI 不区分来源）：托管条目 api 随推断刷新 + key 走凭据分流；手写条目
/// 只写表单拥有的键（name/baseUrl），其余原样保留。零额外记账。
fn mj_managed(provider: &str) -> bool {
    !provider.is_empty()
        && pi_link::auth_json::read_credential(provider)
            .ok()
            .flatten()
            .is_some_and(|c| pi_link::auth_json::is_pf_managed(&c))
}

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

/// 模型条目里某个字符串键（缺 / 非字符串 = 空串）。
fn mj_field<'a>(model: &'a serde_json::Value, key: &str) -> &'a str {
    model.get(key).and_then(|v| v.as_str()).unwrap_or("")
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

    // 草稿态（未保存的新商）：没有可删的对象，不给删除钮
    let is_new = name.is_empty();
    // name 行：标题列 + 控件列（input 半宽；[删除][保存] 与 name input 同列右对齐）
    let remove_provider = name.to_string();
    let name_owned = name.to_string();
    let detail = detail_shell("mc-detail").p(px(15.)).child(form_row(
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
                    .children((!is_new).then(|| {
                        super::models::detail_button(
                            "mj-del",
                            weak,
                            &tr("action.delete"),
                            crate::ui::button::BtnVariant::Danger,
                            false,
                            move |c, cx| c.mj_ask_delete_provider(remove_provider.clone(), cx),
                        )
                    }))
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
    ));

    // API Key 行：控件全宽（右缘眼睛，掩码显隐已存 key）；保存钮在操作栏
    let weak_eye = weak.clone();
    let detail = detail.child(form_row(
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
                .justify_end()
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
                    !add_ok || is_new,
                    {
                        let n = name.to_string();
                        move |c, cx| c.mj_add_model(n.clone(), cx)
                    },
                )),
        )
        // 详情页唯一的信息栏（用户定稿）：两按钮下方、右对齐，同一时刻只有
        // 一条。优先级：表单错误 > 测速结果（seeded 通过态不显示）。
        .children(if let Some(e) = error.as_ref() {
            vec![error_note_right(e)]
        } else {
            match chat.cfg.mj_speed.as_ref() {
                Some(MjSpeedState::Running) => vec![note_right(&tr("models.speed_testing"))],
                Some(MjSpeedState::Passed {
                    latency_ms,
                    models,
                    seeded: false,
                }) => vec![note_right(&crate::i18n::tf(
                    "models.speed_pass",
                    &[
                        ("ms", latency_ms.to_string()),
                        ("count", models.to_string()),
                    ],
                ))],
                Some(MjSpeedState::PassedEmpty { .. }) => {
                    vec![error_note_right(&tr("models.autofetch_failed_hint"))]
                }
                Some(MjSpeedState::Failed(e)) => vec![error_note_right(e)],
                _ => Vec::new(),
            }
        });

    // API 段与可用模型区之间的分隔线（内置详情同款）
    let detail = detail.child(
        div()
            .h(px(1.))
            .w_full()
            .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
    );

    // 可用模型区（内置同规格）；草稿态不渲染——还没保存，没有可挂的模型
    if is_new {
        return detail.into_any_element();
    }
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
    // 罗列单点（账本 ∪ 目录 ∪ 探测展示缓存；2026-11-01 定稿）
    let models: Vec<pi_link::protocol::ModelInfo> = chat.provider_models_for_list(provider);
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
                            let _ =
                                weak_fetch.update(cx, |c, cx| c.mj_fetch_models(fp.clone(), cx));
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
    let shown = super::models::model_list_shown(&models, &d.model_filter_value, d.model_free_only);
    if models.len() > 8 {
        section = section.child(d.model_filter.clone());
    }
    // 列表行与内置商详情**同一实现**（super::models::model_list_vlist）：两行行、
    // 点行开模型详情、点 id 复制引用、右侧启用开关。行高/筛选/元素 id 前缀都在
    // 那边统一，这里只管取数据
    let list = super::models::model_list_vlist(
        weak,
        provider,
        &shown,
        "mj-model-list",
        "mj",
        chat.cfg.mc_state.all_enabled,
        &enabled_set,
        chat.cfg.mc_project_scope,
        &chat.cfg.mc_state.pins,
    );
    detail.child(section.child(list))
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
    // 硬保证：详情页显示的就是当前 section 的模型。表单变量若还残留着别的
    // section（关闭/切换时没清干净，或任何缓存污染），这里立刻按盘上真值
    // 重填一次——绝不再把上一个模型的值端给用户看。
    if chat.cfg.mj_form_for.as_deref() != Some(selected.as_str()) {
        let weak_fix = cx.entity().downgrade();
        let sec = selected.clone();
        cx.defer(move |cx| {
            let _ = weak_fix.update(cx, |c, cx| {
                let now = c
                    .dlg
                    .settings
                    .as_ref()
                    .map(|st| st.read(cx).section.clone());
                // 期间又切走了 → 不再回填旧 section
                if now.as_deref() == Some(sec.as_str()) {
                    c.mj_select(sec.clone(), cx);
                }
            });
        });
    }
    // 内置模型 = 只读参数视图：不给自动填参 / 测试 / 保存 / 移除，输入框禁用
    let read_only = over_id.is_some();
    let d = super::SettingsFormData::snapshot(panel.read(cx), cx);
    let error = panel.read(cx).error.clone();
    let t = T();
    let weak = cx.entity().downgrade();
    // 关闭返回来源 section（点列表进详情时记下）；从【+模型】草稿进 → 退回
    // `mj:p:商`（旧行为）
    let back = chat.cfg.mj_return.clone();
    let dismiss = {
        let weak = weak.clone();
        let n = name.clone();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            let _ = weak.update(cx, |c, cx| {
                let target = back.clone().unwrap_or_else(|| format!("mj:p:{n}"));
                c.cfg.mj_return = None;
                // 先清模型表单变量：关掉详情不留任何上个模型的值
                c.mj_clear_model_form(cx);
                c.mj_select(target, cx);
            });
        }
    };

    // 测试钮即时态 + 状态行文案（表单错误 / 测试结果 / 自动填参结果）
    let test_state = chat.cfg.mj_test.clone();
    let testing = matches!(test_state, Some(MjTestState::Running));
    let test_note: Option<(String, bool)> = match &test_state {
        Some(MjTestState::Running) => Some((tr("models.test_running").to_string(), false)),
        Some(MjTestState::Ok {
            latency_ms,
            status,
            text,
        }) => Some((
            if text.trim().is_empty() {
                crate::i18n::tf(
                    "models.test_passed_short",
                    &[
                        ("ms", latency_ms.to_string()),
                        ("status", status.to_string()),
                    ],
                )
            } else {
                crate::i18n::tf(
                    "models.test_passed",
                    &[
                        ("ms", latency_ms.to_string()),
                        ("status", status.to_string()),
                        ("text", text.clone()),
                    ],
                )
            },
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

    // 页脚：[测试][移除][保存] 右对齐（标准 §5.1 按钮，经 detail_button 三钮
    // 同宽 84；2026-11-01 用户定稿：移除在保存左侧）；状态行（表单错误 /
    // 测试 / 自动填参结果）+ 三个设置行在滚动体
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
            "mj-mdel",
            &weak,
            &tr("action.remove"),
            crate::ui::button::BtnVariant::Danger,
            false,
            {
                let n = name.clone();
                move |c, cx| c.mj_delete_model(n.clone(), ix, cx)
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
    let detail =
        detail_shell("mj-model-detail")
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
                                        let _ = weak_fill.update(cx, |c, cx| {
                                            c.mj_catalog_fill(n.clone(), ix, cx)
                                        });
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
            .child(div().flex().items_center().gap(px(16.)).child(
                mj_cap_checkbox(
                    &tr("models.reasoning"),
                    d.mj_reasoning,
                    &weak,
                    (!read_only).then_some(|c: &mut Chat, cx: &mut Context<Chat>| {
                        c.mj_toggle_reasoning(cx)
                    }),
                ),
            ))
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
                            .child(mj_cap_checkbox(&tr("models.cap_text"), true, &weak, None))
                            .child(mj_cap_checkbox(
                                &tr("models.cap_image"),
                                d.mj_image,
                                &weak,
                                (!read_only).then_some(|c: &mut Chat, cx: &mut Context<Chat>| {
                                    c.mj_toggle_image(cx)
                                }),
                            )),
                    ),
            )
            // 提示栏（唯一一条）：表单错误 / 测试结果 / 填参结果三选一
            .children(status_line.as_ref().map(
                |(text, ok)| {
                    if *ok { note(text) } else { error_note(text) }
                },
            ));
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
                .text_color(rgb(if toggle.is_none() { t.text_dim } else { t.text }))
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
