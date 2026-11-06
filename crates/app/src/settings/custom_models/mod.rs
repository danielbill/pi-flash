//! models.json 自定义 provider / 模型编辑器（设置·模型页右栏的 mj_* 分支）。
//! 042 定稿：表单头【保存】/【移除】（043 同款），点保存即写盘——无页面级
//! footer；**无编辑缓冲**——读即现读 `models.json`，写即 read→mutate→write
//!（042-1：数据源从 PF 账本切到 pi 的 models.json，UI 与交互不变）。
//! 042 M2 对齐 pi-web ProviderDetail/ModelDetail：headers、导入模型
//!（discovery 多选）、模型测试、models.dev 填入模型信息、API 覆盖 /
//! 兼容（DeepSeek / developer role）/ 思考等级映射。
//!
//! itlq.2 按区块拆子模块（只搬不改）：本文件 = section 基础与选择流 +
//! section key 解析助手；materialize = models.json 物化/修复；apply = 商
//! 保存/删除；model_form = 模型行编辑；probe = 测速/目录拉取；render =
//! 详情渲染浮层；modal = 模型编辑器弹窗。

use super::*;
use pi_link::credentials::SecretVault;

pub(crate) mod apply;
pub(crate) mod materialize;
pub(crate) mod modal;
pub(crate) mod model_form;
pub(crate) mod probe;
pub(crate) mod render;

// 供 settings/ 内兄弟模块与 shell/render 原路径使用（调用点零改动）。
pub(crate) use modal::mj_model_modal_overlay;
pub(crate) use render::{mj_delete_confirm_overlay, mj_provider_editor};

impl Chat {
    /// 现读 `models.json`（缺失 → 空文档；坏文件 → 空文档 + 错误条由
    /// `mc_mj_error` 承担）。**不做内存副本**：读处读盘、写处
    /// read→mutate→write，不存在“打开面板时那份旧快照”把磁盘上别人的
    /// 改动覆盖掉的情形。（2026-11-04 起读盘走 mtime 校验缓存
    /// `services::mj_doc_cache`——每次调用 stat 一次，任何写盘（含外部）
    /// 下一次读立即可见，副本语义不破；修复每键全窗重建时的读盘风暴。）
    pub(crate) fn mj_doc() -> serde_json::Value {
        crate::services::mj_doc_cache::read()
            .unwrap_or_else(|_| serde_json::json!({ "providers": {} }))
    }

    /// 写盘（整份 read→mutate→write 的末步；锁内 + 不可读拒写）。
    fn mj_write(&mut self, doc: &serde_json::Value, cx: &mut Context<Self>) {
        if let Err(e) = pi_link::models_json::save(doc) {
            self.mc_set_error(&e, cx);
        }
        crate::services::mj_doc_cache::invalidate();
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
                    // 目录（进程答案/磁盘）优先；静态注册表模型不在 catalog_for
                    // （性能铁律不灌），回落罗列单点（含 registry 静态）取参数
                    if let Some(base) = self
                        .catalog_for(self.project())
                        .into_iter()
                        .find(|mm| mm.provider == name && &mm.id == id)
                        .or_else(|| {
                            self.provider_models_for_list(&name)
                                .into_iter()
                                .find(|mm| &mm.id == id)
                        })
                    {
                        m = serde_json::json!({
                            "contextWindow": base.context_window,
                            "maxTokens": base.max_tokens,
                            "reasoning": base.reasoning,
                            "input": base.input,
                        });
                        // 内置模型目录价回显（042-2）：pi 定义自带 cost 四键，
                        // 详情弹窗直接可见（只读视图，不存在误改问题）
                        if let Some(c) = &base.cost {
                            if let Some(obj) = m.as_object_mut() {
                                obj.insert(
                                    "cost".into(),
                                    serde_json::json!({
                                        "input": c.input,
                                        "output": c.output,
                                        "cacheRead": c.cache_read,
                                        "cacheWrite": c.cache_write,
                                    }),
                                );
                            }
                        }
                    }
                }
                if let Some(obj) = m.as_object_mut() {
                    obj.insert("id".into(), serde_json::json!(id));
                }
                m
            }
            None => ix
                .and_then(|ix| entry.get("models").and_then(|m| m.get(ix)).cloned())
                // 自定义商仓草稿（mj_open_model 登记）：账本无此行，用仓定义预填
                //（探测拉到的 name/contextWindow、手调过的 maxTokens 等原样可见）；
                // 仓里也没有 → 至少把点的 id 填上，别给用户一张空表
                .or_else(|| {
                    let did = self.cfg.mj_draft_store_id.as_deref()?;
                    Some(
                        crate::services::custom_models_store::definition(
                            &self.cfg.mj_probe_cache,
                            &name,
                            did,
                        )
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({ "id": did })),
                    )
                })
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
                // 价格四框回填（042-2）；0 显示为空（0 = 没定价，别让用户
                // 把缺省 0 当成真价）
                let cost_num = |k: &str| -> String {
                    model
                        .get("cost")
                        .and_then(|c| c.get(k))
                        .and_then(|v| v.as_f64())
                        .filter(|n| *n != 0.0)
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                };
                for (ti, k) in [
                    (&s.mj_cost_in, "input"),
                    (&s.mj_cost_out, "output"),
                    (&s.mj_cost_cr, "cacheRead"),
                    (&s.mj_cost_cw, "cacheWrite"),
                ] {
                    ti.update(cx, |ti, cx| ti.set_value(cost_num(k), cx));
                }
                // 内置模型＝pi 的定义，PF 只看不改（用户 2026-10-27 定稿）
                let ro = over.is_some();
                s.mj_id.update(cx, |ti, cx| ti.set_disabled(ro, cx));
                s.mj_ctx.update(cx, |ti, cx| ti.set_disabled(ro, cx));
                s.mj_max_tokens.update(cx, |ti, cx| ti.set_disabled(ro, cx));
                for ti in [&s.mj_cost_in, &s.mj_cost_out, &s.mj_cost_cr, &s.mj_cost_cw] {
                    ti.update(cx, |ti, cx| ti.set_disabled(ro, cx));
                }
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
        self.cfg.mj_draft_store_id = None;
        let Some(st) = self.dlg.settings.clone() else {
            return;
        };
        st.update(cx, |s, cx| {
            for ti in [
                &s.mj_id,
                &s.mj_ctx,
                &s.mj_max_tokens,
                &s.mj_cost_in,
                &s.mj_cost_out,
                &s.mj_cost_cr,
                &s.mj_cost_cw,
            ] {
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
        self.cfg.mj_draft_store_id = None;
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
            for ti in [&s.mj_cost_in, &s.mj_cost_out, &s.mj_cost_cr, &s.mj_cost_cw] {
                ti.update(cx, |ti, cx| {
                    ti.set_value(String::new(), cx);
                    ti.set_disabled(false, cx);
                });
            }
            cx.notify();
        });
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
