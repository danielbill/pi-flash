//! models.json 自定义 provider / 模型编辑器（设置·模型页右栏的
//! mj_* 分支）：缓冲在 Chat.mc_models_json，底部「保存」整文件落盘。

use super::*;
use pi_link::credentials::SecretVault;

impl Chat {
    pub(crate) fn mj_select(&mut self, section: String, cx: &mut Context<Self>) {
        let (name, ix) = mj_parse_section(&section);
        let entry = pi_link::models_json::provider_entry(&self.mc_models_json, &name)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let model = ix
            .and_then(|ix| entry.get("models").and_then(|m| m.get(ix)).cloned())
            .unwrap_or_else(|| serde_json::json!({}));
        let api = entry.get("api").and_then(|v| v.as_str()).unwrap_or("openai-completions");
        let reasoning = model.get("reasoning").and_then(|v| v.as_bool()).unwrap_or(false);
        let get_str = |v: &serde_json::Value, k: &str| {
            v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
        };
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = section;
                s.error = None;
                s.mj_api = mj_api_ix(api);
                s.mj_reasoning = reasoning;
                s.mj_name.update(cx, |ti, cx| ti.set_value(name.clone(), cx));
                s.mj_base.update(cx, |ti, cx| ti.set_value(get_str(&entry, "baseUrl"), cx));
                s.mj_key.update(cx, |ti, cx| ti.set_value(get_str(&entry, "apiKey"), cx));
                s.mj_id.update(cx, |ti, cx| ti.set_value(get_str(&model, "id"), cx));
                s.mj_mname.update(cx, |ti, cx| ti.set_value(get_str(&model, "name"), cx));
                let ctx = model.get("contextWindow").and_then(|v| v.as_u64()).map(|v| v.to_string()).unwrap_or_default();
                s.mj_ctx.update(cx, |ti, cx| ti.set_value(ctx, cx));
                cx.notify();
            });
        }
    }

    /// 把编辑器字段应用进缓冲（provider 名称改动走重命名）。
    pub(crate) fn mj_apply_provider(&mut self, old_name: String, cx: &mut Context<Self>) {
        let Some(st) = self.settings.clone() else { return };
        let (name, base, key, api) = {
            let s = st.read(cx);
            (
                s.mj_name.read(cx).value().trim().to_string(),
                s.mj_base.read(cx).value().trim().to_string(),
                s.mj_key.read(cx).value().trim().to_string(),
                mj_api_name(s.mj_api),
            )
        };
        if name.is_empty() {
            return self.mc_set_error(tr("Provider 名称不能为空"), cx);
        }
        let mut entry = pi_link::models_json::provider_entry(&self.mc_models_json, &old_name)
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        entry["api"] = serde_json::Value::String(api);
        set_opt_str(&mut entry, "baseUrl", &base);
        // apiKey 分流（051）：明文 → 凭据库 + $PF_KEY_* 引用；高级引用原样；
        // 库不可用降级明文。落盘仍走底部「保存」（缓冲只改内存）。
        self.mj_store_key(&mut entry, &name, &key, cx);
        if name != old_name {
            if let Err(e) = pi_link::models_json::rename_provider(&mut self.mc_models_json, &old_name, &name) {
                return self.mc_set_error(&e, cx);
            }
            // 凭据库条目跟名搬家（无旧条目则静默跳过）
            pi_link::models_json::rename_provider_key(&old_name, &name, &pi_link::credentials::KeyringVault);
        }
        pi_link::models_json::upsert_provider(&mut self.mc_models_json, &name, entry);
        self.mc_mj_dirty = true;
        self.mc_mj_saved = false;
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = format!("mj:p:{name}");
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(crate) fn mj_delete_provider(&mut self, name: String, cx: &mut Context<Self>) {
        pi_link::models_json::remove_provider(&mut self.mc_models_json, &name);
        // 凭据库条目一并清（幂等；条目不存在是 Ok）
        let _ = pi_link::credentials::KeyringVault
            .delete(&pi_link::credentials::env_var_name(&name));
        self.mc_mj_dirty = true;
        self.mc_mj_saved = false;
        if let Some(st) = self.settings.clone() {
            let next = pi_link::models_json::providers(&self.mc_models_json)
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
            pi_link::models_json::set_provider_key(entry, name, key, &pi_link::credentials::KeyringVault)
        {
            self.mc_set_error(tr("系统凭据库不可用，已降级为文件存储"), cx);
        }
    }

    /// 新建自定义 provider（添加面板表单）。
    pub(crate) fn mj_add_provider(
        &mut self,
        name: String,
        base: String,
        key: String,
        api: u8,
        cx: &mut Context<Self>,
    ) {
        self.mc_clear_error(cx);
        let name = name.trim().to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return self.mc_set_error(tr("Provider 名称只能使用字母、数字、_ 和 -"), cx);
        }
        if pi_link::models_json::provider_entry(&self.mc_models_json, &name).is_some() {
            return self.mc_set_error(&crate::i18n::tf("已存在名为「{name}」的 Provider", &[("name", name)]), cx);
        }
        let mut entry = serde_json::json!({ "api": mj_api_name(api) });
        set_opt_str(&mut entry, "baseUrl", base.trim());
        self.mj_store_key(&mut entry, &name, key.trim(), cx);
        pi_link::models_json::upsert_provider(&mut self.mc_models_json, &name, entry);
        self.mc_mj_dirty = true;
        self.mc_mj_saved = false;
        if let Some(st) = self.settings.clone() {
            st.update(cx, |s, cx| {
                s.section = format!("mj:p:{name}");
                cx.notify();
            });
        }
        self.mj_select(format!("mj:p:{name}"), cx);
        cx.notify();
    }

    pub(crate) fn mj_add_model(&mut self, provider: String, cx: &mut Context<Self>) {
        let ix = pi_link::models_json::add_model(&mut self.mc_models_json, &provider);
        self.mc_mj_dirty = true;
        self.mc_mj_saved = false;
        self.mj_select(format!("mj:m:{provider}:{ix}"), cx);
    }

    pub(crate) fn mj_apply_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        let Some(st) = self.settings.clone() else { return };
        let (id, mname, ctx, reasoning) = {
            let s = st.read(cx);
            (
                s.mj_id.read(cx).value().trim().to_string(),
                s.mj_mname.read(cx).value().trim().to_string(),
                s.mj_ctx.read(cx).value().trim().to_string(),
                s.mj_reasoning,
            )
        };
        if id.is_empty() {
            return self.mc_set_error(tr("模型 ID 不能为空"), cx);
        }
        let mut model = serde_json::json!({ "id": id });
        set_opt_str(&mut model, "name", &mname);
        if let Ok(n) = ctx.parse::<u64>() {
            model["contextWindow"] = serde_json::json!(n);
        }
        if reasoning {
            model["reasoning"] = serde_json::json!(true);
        }
        pi_link::models_json::update_model(&mut self.mc_models_json, &provider, ix, model);
        self.mc_mj_dirty = true;
        self.mc_mj_saved = false;
        cx.notify();
    }

    pub(crate) fn mj_delete_model(&mut self, provider: String, ix: usize, cx: &mut Context<Self>) {
        pi_link::models_json::remove_model(&mut self.mc_models_json, &provider, ix);
        self.mc_mj_dirty = true;
        self.mc_mj_saved = false;
        self.mj_select(format!("mj:p:{provider}"), cx);
    }

    /// 底部保存：整个 models.json 落盘。
    pub(crate) fn mj_save(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = &self.mc_mj_error {
            return self.mc_set_error(&crate::i18n::tf("无法读取 models.json，为避免覆盖已禁用保存：{e}", &[("e", e.clone())]), cx);
        }
        if let Err(e) = pi_link::models_json::write(&self.mc_models_json) {
            return self.mc_set_error(&e, cx);
        }
        self.mc_mj_dirty = false;
        self.mc_mj_saved = true;
        cx.notify();
    }

    /// Skills toggle: write `disable-model-invocation` into SKILL.md
    /// (pi-web PATCH /api/skills parity).
    pub(crate) fn mc_cli_op(&mut self, args: Vec<String>, done: String, cx: &mut Context<Self>) {
        let Some(tx) = self.op_tx.clone() else { return };

        let cwd = self.cwd.clone();
        std::thread::spawn(move || {
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = match pi_link::vendor::run_cli(&cwd, &arg_refs) {
                Ok(_) => done,
                Err(e) => crate::i18n::tf(
                    tr("pi {} 失败: {}"),
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
        map.insert(key.to_string(), serde_json::Value::String(value.to_string()));
    }
}


pub(crate) fn mj_provider_editor(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    mj_name: &gpui::Entity<crate::TextInput>,
    mj_base: &gpui::Entity<crate::TextInput>,
    mj_key: &gpui::Entity<crate::TextInput>,
    mj_api: u8,
    error: &Option<String>,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    let in_catalog = chat
        .catalog_for(&chat.cwd)
        .iter()
        .any(|m| m.provider == name);
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(28.))
                .child(section_title(&tr("Provider")))
                .child(div().flex_1())
                .children(in_catalog.then(|| {
                    let enabled = chat.mc_state.all_enabled
                        || chat.mc_state.enabled.iter().any(|e| {
                            models_config::ref_provider(e) == name
                        });
                    config_switch(
                        "mj-prov-sw",
                        weak,
                        enabled,
                        chat.mc_project_scope,
                        {
                            let n = name.to_string();
                            move |c, cx| c.mc_toggle_provider(&n, !enabled, cx)
                        },
                    )
                }))
                .child(config_button("mj-del", weak, &tr("删除"), Btn::Danger, true, false, {
                    let n = name.to_string();
                    move |c, cx| c.mj_delete_provider(n.clone(), cx)
                })),
        )
        .children(error.as_ref().map(|e| error_note(e)))
        .child(field(
            &tr("Provider 名称"),
            div().w(px(320.)).child(mj_name.clone()),
        ))
        .child(field(
            "Base URL",
            div().w(px(420.)).child(mj_base.clone()),
        ))
        .child(field(
            "API Key",
            div().w(px(420.)).child(mj_key.clone()),
        ))
        .child(field("API", api_options_row(weak, mj_api)))
        .child(note("留空 Base URL 使用 pi 内置端点；API Key 支持 ENV 变量名、!shell-command 或明文"))
        .child(note("改动先缓冲，点底部「保存」写入 ~/.pi/agent/models.json"))
        .child(
            div()
                .pt(px(4.))
                .child(config_button("mj-apply", weak, &tr("应用更改"), Btn::Secondary, false, false, {
                    let n = name.to_string();
                    move |c, cx| c.mj_apply_provider(n.clone(), cx)
                })),
        );
    // 唤起一次 select 的填充由行点击负责；这里补渲染期 id 提示
    let _ = t;
    detail.into_any_element()
}

/// 自定义模型编辑器。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mj_model_editor(
    weak: &gpui::WeakEntity<Chat>,
    name: &str,
    ix: usize,
    mj_id: &gpui::Entity<crate::TextInput>,
    mj_mname: &gpui::Entity<crate::TextInput>,
    mj_ctx: &gpui::Entity<crate::TextInput>,
    mj_reasoning: bool,
    error: &Option<String>,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    let _ = t;
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(28.))
                .child(section_title(&tr("模型")))
                .child(
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(name.to_string())),
                )
                .child(div().flex_1())
                .child(config_button("mj-mdel", weak, &tr("移除"), Btn::Danger, true, false, {
                    let n = name.to_string();
                    move |c, cx| c.mj_delete_model(n.clone(), ix, cx)
                })),
        )
        .children(error.as_ref().map(|e| error_note(e)))
        .child(field(
            &tr("ID"),
            div().w(px(320.)).child(mj_id.clone()),
        ))
        .child(field(
            &tr("名称"),
            div().w(px(320.)).child(mj_mname.clone()),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_muted))
                        .child(tr("推理模型（T 徽标）")),
                )
                .child(config_switch("mj-reasoning", weak, mj_reasoning, false, |c, cx| {
                    if let Some(st) = c.settings.clone() {
                        st.update(cx, |s, cx| {
                            s.mj_reasoning = !s.mj_reasoning;
                            cx.notify();
                        });
                    }
                })),
        )
        .child(field(
            &tr("上下文窗口（tokens）"),
            div().w(px(200.)).child(mj_ctx.clone()),
        ))
        .child(note("改动先缓冲，点底部「保存」写入 ~/.pi/agent/models.json"))
        .child(
            div()
                .pt(px(4.))
                .child(config_button("mj-mapply", weak, &tr("应用更改"), Btn::Secondary, false, false, {
                    let n = name.to_string();
                    move |c, cx| c.mj_apply_model(n.clone(), ix, cx)
                })),
        );
    detail.into_any_element()
}

/// 添加 Provider 面板：已知 provider 目录（未配置）+ 自定义端点表单。
pub(crate) fn mj_add_panel(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    mj_name: &gpui::Entity<crate::TextInput>,
    mj_base: &gpui::Entity<crate::TextInput>,
    mj_key: &gpui::Entity<crate::TextInput>,
    mj_api: u8,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    // 未配置的 catalog providers（点选 → API Key 详情）
    let provider_ids = chat.mc_provider_ids();
    let known: Vec<String> = provider_ids
        .iter()
        .filter(|p| !chat.mc_configured(p) && !chat.mc_oauth(p))
        .cloned()
        .collect();
    let mut grid = div().flex().flex_wrap().gap(px(8.)).max_w(px(720.));
    for p in known.iter() {
        let weak_item = weak.clone();
        let pid = (*p).clone();
        grid = grid.child(
            div()
                .id(SharedString::from(format!("mj-known-{p}")))
                .min_w(px(200.))
                .px(px(12.))
                .py(px(9.))
                .rounded(px(7.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .cursor_pointer()
                .hover(|s| s.border_color(rgb(t.accent)).bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| c.mc_select_provider(pid.clone(), cx));
                })
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(p.clone())),
                )
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_dim))
                        .child("API Key"),
                ),
        );
    }

    // 自定义端点表单
    // 详情内边距与列表 15px 统一（040 扩展页定稿；detail_shell 默认 p20）
    let detail = detail_shell("mc-detail")
        .p(px(15.))
        .child(section_title(&tr("添加 Provider")))
        .child(note("从目录选择（写入 auth.json），或创建 OpenAI / Anthropic 兼容的自定义端点（写入 models.json）"))
        .when(!known.is_empty(), |d| d.child(grid))
        .child(
            div()
                .mt(px(8.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(section_title(&tr("自定义端点")))
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(field(&tr("名称"), div().w(px(200.)).child(mj_name.clone())))
                        .child(field("Base URL", div().w(px(300.)).child(mj_base.clone())))
                        .child(field("API Key", div().w(px(240.)).child(mj_key.clone()))),
                )
                .child(field("API", api_options_row(weak, mj_api)))
                .child(
                    div()
                        .child(config_button("mj-create", weak, &tr("创建"), Btn::Primary, false, false, |c, cx| {
                            let Some(st) = c.settings.clone() else { return };
                            let (name, base, key, api) = st.read(cx).editor_snapshot(cx);
                            c.mj_add_provider(name, base, key, api, cx);
                        })),
                ),
        );
    detail.into_any_element()
}

/// API 四选一按钮排（openai-completions / responses / anthropic / google）。
pub(crate) fn api_options_row(
    weak: &gpui::WeakEntity<Chat>,
    selected: u8,
) -> gpui::AnyElement {
    let t = T();
    const OPTIONS: [(u8, &str); 4] = [
        (0, "openai-completions"),
        (1, "openai-responses"),
        (2, "anthropic-messages"),
        (3, "google-generative-ai"),
    ];
    div()
        .flex()
        .flex_wrap()
        .gap(px(6.))
        .children(OPTIONS.iter().map(|(ix, label)| {
            let active = selected == *ix;
            let weak_opt = weak.clone();
            div()
                .id(SharedString::from(format!("mj-api-{ix}")))
                .h(px(28.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(if active { t.accent } else { t.border }))
                .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
                .font_family(crate::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(if active { t.text } else { t.text_muted }))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_opt.update(cx, |c, cx| {
                        if let Some(st) = c.settings.clone() {
                            st.update(cx, |s, cx| {
                                s.mj_api = *ix;
                                cx.notify();
                            });
                        }
                    });
                })
                .child(SharedString::from(*label))
                .into_any_element()
        }))
        .into_any_element()
}
