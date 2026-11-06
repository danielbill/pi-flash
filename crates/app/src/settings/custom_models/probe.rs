//! 测速与目录拉取（导入发现）：mj_speedtest_start / mj_fetch_models /
//! mj_test_start / models.dev 目录自动填参（mj_catalog_fill）。

use super::*;

impl Chat {
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
        let i_bg = id.clone();
        let n_bg = name.clone();
        cx.spawn(async move |this, cx| {
            // 042-2 解析链：models.dev 单文件 TOML → OpenRouter → None（落 0）
            let resolved = cx
                .background_executor()
                .spawn(async move { pi_link::model_price::resolve(&p_bg, &b_bg, &i_bg, &n_bg) })
                .await;
            // 目录无此 id 是常态错误（本地化文案）；网络错带原文
            let shown = match &resolved {
                Ok(_) => None,
                Err(pi_link::model_price::FillError::NoMatch) => {
                    Some(tr("models.catalog_no_exact_match").to_string())
                }
                Err(pi_link::model_price::FillError::Net(m)) => Some(m.clone()),
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
        e: &pi_link::model_price::Entry,
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
            // 价格四框回显目录价：只填空/零（手填的非零值是用户定价，不冲）
            let cost_num = |k: &str| -> Option<f64> {
                match k {
                    "input" => e.cost.input,
                    "output" => e.cost.output,
                    "cacheRead" => e.cost.cache_read,
                    _ => e.cost.cache_write,
                }
            };
            for (ti, k) in [
                (&s.mj_cost_in, "input"),
                (&s.mj_cost_out, "output"),
                (&s.mj_cost_cr, "cacheRead"),
                (&s.mj_cost_cw, "cacheWrite"),
            ] {
                let Some(v) = cost_num(k) else { continue };
                let cur_empty_or_zero = ti
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<f64>()
                    .map(|n| n == 0.0)
                    .unwrap_or(true);
                if cur_empty_or_zero {
                    ti.update(cx, |ti, cx| ti.set_value(v.to_string(), cx));
                }
            }
        });
        // 名称/价格留到保存时落盘；id 变了就不复用（防止串到别的模型）
        self.cfg.mj_fill_entry = Some((queried_id.to_string(), e.clone()));
        self.cfg.mj_models_dev_state = MjCatalogState::Done;
        cx.notify();
    }
}
