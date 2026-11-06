//! 模型行编辑：打开模型详情（内置覆盖/账本条目/仓草稿三分流）、表单
//! 提交（mj_commit_model_form）、删除、能力开关（image/reasoning）。

use super::*;

impl Chat {
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
            Some(ix) => {
                self.cfg.mj_draft_store_id = None;
                format!("mj:m:{provider}:{ix}")
            }
            // 自定义商「仓里有、账本没有」（未勾选）的模型：**可编辑仓草稿**——
            // 追加槽 + 仓定义预填（mj_fill_from 认 mj_draft_store_id）；保存只
            // 进仓、移除删仓条目。用户 2026-11-02 定稿：自定义商详情一律可
            // 编辑，只读覆盖视图只属于内置模型（0.3.2 曾把探测模型误落只读）
            None if self.mc_is_custom_provider(&provider) => {
                self.cfg.mj_draft_store_id = Some(id.clone());
                format!("mj:m:{provider}:{}", models.len())
            }
            None => {
                self.cfg.mj_draft_store_id = None;
                override_section(&provider, &id)
            }
        };
        self.mj_fill_from(section, &doc, cx);
    }

    pub(crate) fn mj_add_model(&mut self, provider: String, cx: &mut Context<Self>) {
        // 草稿行不是「从列表进详情」：关闭退回 `mj:p:商`（mj_return=None 即该默认）
        self.cfg.mj_return = None;
        // 【+模型】是空白草稿，不是仓草稿（否则表单会预填上一次点开的仓模型）
        self.cfg.mj_draft_store_id = None;
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
        let (id, ctx, image, reasoning, max_tokens, costs) = {
            let s = st.read(cx);
            let cost_val = |ti: &gpui::Entity<crate::ui::text_input::TextInput>| -> Option<
                Result<f64, String>,
            > {
                let t = ti.read(cx).value().trim().to_string();
                if t.is_empty() {
                    return None; // 空 = 沿用现值
                }
                Some(t.parse::<f64>().map_err(|_| t))
            };
            (
                s.mj_id.read(cx).value().trim().to_string(),
                s.mj_ctx.read(cx).value().trim().to_string(),
                s.mj_image,
                s.mj_reasoning,
                s.mj_max_tokens.read(cx).value().trim().to_string(),
                [
                    cost_val(&s.mj_cost_in),
                    cost_val(&s.mj_cost_out),
                    cost_val(&s.mj_cost_cr),
                    cost_val(&s.mj_cost_cw),
                ],
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
        // 价格四框（042-2）：**手填 = 显式覆盖**——有值即写该键，空 = 沿用；
        // 任一框非数字 → 报错不落盘。写价即记 manual 源（最高优先级：后续
        // 自动填参跳过本模型，手填价永不被目录价冲掉）。
        let cost_keys = ["input", "output", "cacheRead", "cacheWrite"];
        let mut manual_priced = false;
        for (res, key) in costs.into_iter().zip(cost_keys) {
            match res {
                Some(Ok(v)) => {
                    let cost = obj.entry("cost").or_insert_with(|| serde_json::json!({}));
                    if !cost.is_object() {
                        *cost = serde_json::json!({});
                    }
                    cost.as_object_mut()
                        .expect("cost is object")
                        .insert(key.to_string(), serde_json::json!(v));
                    manual_priced = true;
                }
                Some(Err(_bad)) => {
                    self.mc_set_error(tr("models.cost_all_required"), cx);
                    return false;
                }
                None => {}
            }
        }
        if manual_priced {
            obj.insert(
                pi_link::model_price::SOURCE_KEY.into(),
                serde_json::json!(pi_link::model_price::SOURCE_MANUAL),
            );
        }
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
        // 填参结果里的名称/价格在这里补（页面没有这两项，只填空/零不覆盖）；
        // 仅当「填参时查的 id」与本次保存的 id 一致才用，避免串到别的模型。
        // 手填价（priceSource = manual）是最高优先级：跳过自动补价。
        let price_is_manual = model
            .get(pi_link::model_price::SOURCE_KEY)
            .and_then(|v| v.as_str())
            == Some(pi_link::model_price::SOURCE_MANUAL);
        if let Some((queried, e)) = self.cfg.mj_fill_entry.clone() {
            if !price_is_manual
                && model.get("id").and_then(|v| v.as_str()) == Some(queried.as_str())
            {
                if !e.source.is_empty() {
                    model.as_object_mut().expect("model is object").insert(
                        pi_link::model_price::SOURCE_KEY.into(),
                        serde_json::json!(e.source.clone()),
                    );
                }
                pi_link::model_price::fill_name_cost(&mut model, &e);
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
        // 仓草稿改了 id → 旧键删掉，否则仓里留一条幽灵（列表多出一行）
        let draft_from_store = self.cfg.mj_draft_store_id.is_some();
        if let Some(old) = self.cfg.mj_draft_store_id.clone() {
            if old != model_id {
                crate::services::custom_models_store::remove_model(
                    &mut self.cfg.mj_probe_cache,
                    provider,
                    &old,
                );
            }
        }
        crate::services::custom_models_store::upsert_model(
            &mut self.cfg.mj_probe_cache,
            provider,
            &model_id,
            model.clone(),
        );
        let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
        // ② models.json 按投影规则：编辑已勾选的行 → 原位更新；【+模型】
        // 手填（草稿追加，显式创建 = 勾选）→ 追加；**仓草稿**（从列表进、
        // 账本无此行 = 未勾选）保存 → 只进仓，不进 models.json；但
        // enabledModels 已显式勾选它（存量漏物化态）→ 顺手物化补齐
        let models_len = entry
            .get("models")
            .and_then(|m| m.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let mut ledger_changed = true;
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
            let r = format!("{provider}/{model_id}");
            let explicitly_enabled =
                !self.cfg.mc_state.all_enabled && self.cfg.mc_state.enabled.iter().any(|e| e == &r);
            if !draft_from_store || explicitly_enabled {
                pi_link::models_json::push_model(&mut doc, provider, model);
            } else {
                ledger_changed = false;
            }
        } else {
            return false;
        }
        if ledger_changed {
            self.mj_write(&doc, cx);
        }
        true
    }

    pub(crate) fn mj_delete_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        // 删掉后回到商详情：来源 section 一并清（否则下次弹窗会退回旧视图）
        self.cfg.mj_return = None;
        let mut doc = Self::mj_doc();
        // 删之前先记住 id：内存目录按 provider+id 摘（和磁盘同一步）。
        // 仓草稿（账本无此行）→ id 取登记的仓 id
        let draft_id = self.cfg.mj_draft_store_id.take();
        let id = pi_link::models_json::provider_entry(&doc, &provider)
            .and_then(|e| e.get("models"))
            .and_then(|m| m.get(ix))
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or(draft_id);
        if pi_link::models_json::remove_model(&mut doc, &provider, ix) {
            self.mj_write(&doc, cx);
        }
        // 清单仓同步删（模型详情【移除】= 删除该模型，不是取消勾选）
        if let Some(id) = &id {
            crate::services::custom_models_store::remove_model(
                &mut self.cfg.mj_probe_cache,
                &provider,
                id,
            );
            let _ = crate::services::custom_models_store::write(&self.cfg.mj_probe_cache);
            // enabledModels 里精确 `商/模型id` 的 pattern 一并清（glob 留着）
            self.mc_prune_patterns(&provider, Some(id), cx);
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
    pub(super) fn mj_form_key(key_text: &str) -> Result<String, String> {
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
}
