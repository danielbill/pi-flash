//! session/composer 组：会话生命周期（new/open/switch/delete/pin）、消息
//! 三连（send/steer/followup）、中止、工具预设与状态带直调、composer 文本。

use gpui::Context;
use serde_json::{Value, json};

use super::{OpResult, bad, not_found, ok, params_path};
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        // ---- 会话 ----
        method::SESSION_NEW => {
            chat.new_session(cx);
            let key = chat.pool.active_key.clone();
            Ok(json!({"ok": true, "key": key}))
        }
        method::SESSION_OPEN => {
            let path = params_path(params, "path")?;
            if !path.is_file() {
                return Err(not_found(format!("会话文件不存在: {}", path.display())));
            }
            chat.open_session(path.clone(), false, cx);
            Ok(json!({"ok": true, "key": path.display().to_string()}))
        }
        method::SESSION_SWITCH => {
            let key = params
                .get("key")
                .or_else(|| params.get("path"))
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"key\": \"…\"} 或 {\"path\": \"…\"}"))?
                .to_string();
            let rt = chat.pool.runtimes.get(&key).cloned().ok_or_else(|| {
                not_found(format!(
                    "池里没有 key={key:?} 的会话（runtimes 见 ui.snapshot sessions 面）"
                ))
            })?;
            chat.switch_pool_rt(rt, cx);
            ok()
        }
        method::SESSION_DELETE => {
            let path = params_path(params, "path")?;
            chat.delete_session(path, cx);
            ok()
        }
        method::SESSION_PIN => {
            let path = params_path(params, "path")?;
            // **幂等**：缺省 = 置顶（pinned: true），显式 false 才取消。
            // 早期缺省是 toggle，脚本/测试连着调两次会互相撤销（e2e 实测
            // 三条 pin 只剩最后一条）——自动化 op 必须可重复调用。
            // UI 的按钮语义仍是 toggle（那是点击语义，与本 op 无关）。
            let want = params
                .get("pinned")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let now = chat.session_pinned(&path);
            if want != now {
                chat.session_pin_toggle(path, cx);
            }
            Ok(json!({"pinned": want}))
        }
        method::SESSION_SEND => {
            if let Some(t) = params.get("text").and_then(Value::as_str) {
                chat.set_input(t.to_string(), cx);
            }
            chat.send_input(cx);
            ok()
        }
        method::SESSION_STEER => {
            if let Some(t) = params.get("text").and_then(Value::as_str) {
                chat.set_input(t.to_string(), cx);
            }
            chat.steer_input(cx);
            ok()
        }
        method::SESSION_FOLLOWUP => {
            if let Some(t) = params.get("text").and_then(Value::as_str) {
                chat.set_input(t.to_string(), cx);
            }
            chat.follow_up_input(cx);
            ok()
        }
        method::SESSION_ABORT => {
            chat.abort_stream(cx);
            ok()
        }
        method::COMPOSER_SET_TEXT => {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            chat.set_input(text.to_string(), cx);
            ok()
        }
        method::SESSION_TOOLS_PRESET => {
            let preset = params
                .get("preset")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    bad("需要 {\"preset\": \"full|custom|default|read-only|chat-only\"}")
                })?;
            chat.mc_set_tools_preset(preset, cx);
            ok()
        }
        method::SESSION_PLAN_TOGGLE => {
            // 031-1：与工具菜单「计划模式」行同一条分派（草稿意图/spawn
            // 同步/防哑弹一体）；状态读 session 面 plan_enabled
            chat.mc_toggle_plan(cx);
            ok()
        }
        method::SESSION_STATUS_ACT => {
            let key = params
                .get("key")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"key\": \"usage|mcp\"}"))?;
            let Some(action) = crate::session::footer_bar::action_of(key) else {
                return Err(bad(format!("状态项 {key:?} 没有可点动作")));
            };
            chat.footer_status_act(action, cx);
            Ok(json!({"action": action.tag()}))
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
