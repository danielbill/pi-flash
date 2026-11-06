//! settings 组：设置页开收、扩展安装/卸载流、MCP 表单、051 自定义 Provider
//! （mj_*）与模型开关（mc_toggle）。每个 op 与对应按钮走同一条 Chat 方法，
//! 不绕过任何保存/测速链路。

use gpui::Context;
use serde_json::Value;

use super::{OpResult, bad, ok};
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        // ---- 设置/弹窗 ----
        method::SETTINGS_OPEN => {
            let tab = params.get("tab").and_then(Value::as_u64).unwrap_or(0) as u8;
            chat.open_settings(tab, cx);
            ok()
        }
        method::SETTINGS_CLOSE => {
            chat.dlg.settings = None;
            cx.notify();
            ok()
        }
        // 040 扩展页安装直调：走与安装按钮完全相同的 mc_install_package 链路
        method::SETTINGS_INSTALL_EXT => {
            let source = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:… 或整条 pi install …\"}"))?
                .to_string();
            chat.mc_install_package(source, cx);
            ok()
        }
        // 040 卸载确认流：先弹居中确认浮层，再应答确认
        method::SETTINGS_REMOVE_EXT => {
            let source = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:…\"}"))?
                .to_string();
            chat.mc_ask_remove_package(source, cx);
            ok()
        }
        method::SETTINGS_PKG_REMOVE_CONFIRM => {
            let confirmed = params.get("ok").and_then(Value::as_bool).unwrap_or(false);
            chat.mc_remove_dialog_close(confirmed, cx);
            ok()
        }
        method::SETTINGS_MCP_SAVE => {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"text\": \"粘贴内容\"}"))?
                .to_string();
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(st) = chat.dlg.settings.clone() {
                st.update(cx, |s, cx| {
                    s.mcp_add.update(cx, |ti, cx| ti.set_value(text, cx));
                    s.mcp_name.update(cx, |ti, cx| ti.set_value(name, cx));
                    cx.notify();
                });
            }
            chat.mcp_save_submit(cx);
            ok()
        }
        // ---- 051 自定义 Provider 页（UI 测试直调）：目标 provider 从当前
        // section 推导：`mj:p:{name}` = 已存商；`mj:new` = 草稿。
        method::SETTINGS_MJ_OPEN_MODEL => {
            let get = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_string);
            let (provider, id) = match (get("provider"), get("id")) {
                (Some(p), Some(i)) if !p.is_empty() && !i.is_empty() => (p, i),
                _ => return Err(bad("需要 {\"provider\": \"商\", \"id\": \"模型id\"}")),
            };
            chat.mj_open_model(provider, id, cx);
            ok()
        }
        method::SETTINGS_MJ_NEW => {
            chat.mj_open_add(cx);
            ok()
        }
        method::SETTINGS_MJ_SELECT => {
            let section = params
                .get("section")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    bad("需要 {\"section\": \"mj:p:名字 / mj:m:名字:ix / mj:o:商:模型id\"}")
                })?
                .to_string();
            chat.mj_select(section, cx);
            ok()
        }
        method::SETTINGS_MJ_FORM => {
            let get = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_string);
            if let Some(st) = chat.dlg.settings.clone() {
                st.update(cx, |s, cx| {
                    if let Some(v) = get("name") {
                        s.mj_name.update(cx, |ti, cx| ti.set_value(v, cx));
                    }
                    if let Some(v) = get("base") {
                        s.mj_base.update(cx, |ti, cx| ti.set_value(v, cx));
                    }
                    if let Some(v) = get("key") {
                        s.mj_key.update(cx, |ti, cx| ti.set_value(v, cx));
                    }
                    cx.notify();
                });
            }
            ok()
        }
        method::SETTINGS_MJ_SAVE => {
            let old = mj_provider_from_section(chat, cx)
                .ok_or_else(|| bad("当前 section 不是自定义商（需要 mj:p:名字 或 mj:new）"))?;
            chat.mj_apply_provider(old, cx);
            ok()
        }
        method::SETTINGS_MJ_SPEEDTEST => {
            let provider = mj_provider_from_section(chat, cx)
                .ok_or_else(|| bad("当前 section 不是自定义商（需要 mj:p:名字 或 mj:new）"))?;
            chat.mj_speedtest_start(provider, cx);
            ok()
        }
        method::SETTINGS_MJ_ADD_MODEL => {
            let provider = mj_provider_from_section(chat, cx)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad("需要已保存的商（section = mj:p:名字）"))?;
            chat.mj_add_model(provider, cx);
            ok()
        }
        method::SETTINGS_MJ_MODEL_FORM => {
            let get = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_string);
            let get_bool = |k: &str| params.get(k).and_then(Value::as_bool);
            if let Some(st) = chat.dlg.settings.clone() {
                st.update(cx, |s, cx| {
                    if let Some(v) = get("id") {
                        s.mj_id.update(cx, |ti, cx| ti.set_value(v, cx));
                    }
                    if let Some(v) = get("ctx") {
                        s.mj_ctx.update(cx, |ti, cx| ti.set_value(v, cx));
                    }
                    if let Some(v) = get("max_tokens") {
                        s.mj_max_tokens.update(cx, |ti, cx| ti.set_value(v, cx));
                    }
                    // 042-2 价格四框（手填 = 显式覆盖）
                    for (k, ti) in [
                        ("cost_in", &s.mj_cost_in),
                        ("cost_out", &s.mj_cost_out),
                        ("cost_cr", &s.mj_cost_cr),
                        ("cost_cw", &s.mj_cost_cw),
                    ] {
                        if let Some(v) = get(k) {
                            ti.update(cx, |ti, cx| ti.set_value(v, cx));
                        }
                    }
                    if let Some(v) = get_bool("image") {
                        s.mj_image = v;
                    }
                    if let Some(v) = get_bool("reasoning") {
                        s.mj_reasoning = v;
                    }
                    cx.notify();
                });
            }
            ok()
        }
        method::SETTINGS_MJ_MODEL_SAVE => {
            let (provider, ix) = mj_model_from_section(chat, cx)
                .ok_or_else(|| bad("当前 section 不是模型编辑器（mj:m:名字:ix）"))?;
            chat.mj_apply_model(provider, ix, cx);
            ok()
        }
        method::SETTINGS_MJ_MODEL_CLOSE => {
            // 与弹窗 ✕/点外同源：退回来源 section（列表进来的内置模型 → 商 id，
            // 自定义模型 → mj:p:商；【+模型】草稿 → mj:p:商）
            let section = chat
                .dlg
                .settings
                .as_ref()
                .map(|st| st.read(cx).section.clone())
                .unwrap_or_default();
            let is_model = section.starts_with("mj:m:") || section.starts_with("mj:o:");
            if !is_model {
                return Err(bad(
                    "当前 section 不是模型详情（mj:m:名字:ix / mj:o:商:模型id）",
                ));
            }
            let provider = match section.strip_prefix("mj:m:") {
                Some(rest) => rest
                    .rsplit_once(':')
                    .map(|(p, _)| p)
                    .unwrap_or("")
                    .to_string(),
                None => mj_parse_override_provider(&section).unwrap_or_default(),
            };
            let target = chat
                .cfg
                .mj_return
                .clone()
                .or_else(|| (!provider.is_empty()).then(|| format!("mj:p:{provider}")));
            chat.cfg.mj_return = None;
            // 与弹窗 ✕ 同源：关详情即清模型表单变量（不留上一个模型的值）
            chat.mj_clear_model_form(cx);
            match target {
                Some(t) => chat.mj_select(t, cx),
                None => return Err(bad("无处可退：没有来源 section")),
            }
            ok()
        }
        method::SETTINGS_MJ_AUTOFILL => {
            let (provider, ix) = mj_model_from_section(chat, cx)
                .ok_or_else(|| bad("当前 section 不是模型编辑器（mj:m:名字:ix）"))?;
            chat.mj_catalog_fill(provider, ix, cx);
            ok()
        }
        method::SETTINGS_MJ_FETCH => {
            let provider = mj_provider_from_section(chat, cx)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad("需要已保存的商（section = mj:p:名字）"))?;
            chat.mj_fetch_models(provider, cx);
            ok()
        }
        method::SETTINGS_MC_TOGGLE => {
            let r = params
                .get("ref")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"ref\": \"商/模型id\", \"enable\": bool}"))?
                .to_string();
            let enable = params
                .get("enable")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            chat.mc_toggle_model(r, enable, cx);
            ok()
        }
        method::SETTINGS_MJ_DELETE => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"name\": \"商名\"}"))?
                .to_string();
            let ok_del = params.get("ok").and_then(Value::as_bool).unwrap_or(true);
            chat.mj_ask_delete_provider(name, cx);
            chat.mj_delete_provider_close(ok_del, cx);
            ok()
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}

/// 当前设置面板 section 里的自定义商名（`mj:p:{name}` → Some(name)，
/// `mj:new` 草稿 → Some("")，其他 → None）。
fn mj_provider_from_section(chat: &Chat, cx: &Context<Chat>) -> Option<String> {
    let st = chat.dlg.settings.as_ref()?;
    let section = st.read(cx).section.clone();
    if section == "mj:new" {
        return Some(String::new());
    }
    section.strip_prefix("mj:p:").map(str::to_string)
}

/// 当前模型编辑器 section 解析（`mj:m:{prov}:{ix}` → Some((prov, ix))）。
/// `mj:o:{商}:{模型id}` → 商名（模型 id 可含 ':'，只切第一个）。
fn mj_parse_override_provider(section: &str) -> Option<String> {
    let (p, id) = section.strip_prefix("mj:o:")?.split_once(':')?;
    (!p.is_empty() && !id.is_empty()).then(|| p.to_string())
}

fn mj_model_from_section(chat: &Chat, cx: &Context<Chat>) -> Option<(String, usize)> {
    let st = chat.dlg.settings.as_ref()?;
    let section = st.read(cx).section.clone();
    let rest = section.strip_prefix("mj:m:")?;
    let (prov, ix) = rest.rsplit_once(':')?;
    let ix: usize = ix.parse().ok()?;
    (!prov.is_empty()).then(|| (prov.to_string(), ix))
}
