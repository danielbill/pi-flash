//! model/project 组：模型选择弹窗（开/点行）与项目切换/新建/项目菜单。

use gpui::Context;
use serde_json::Value;

use super::{OpResult, bad, not_found, ok, params_path};
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(chat: &mut Chat, cx: &mut Context<Chat>, m: &str, params: &Value) -> OpResult {
    match m {
        method::MODEL_PICKER_OPEN => {
            // 042：模型菜单纯 pill 化后的无坐标入口（锚点走右下兜底）；
            // 列表内容本身来自 Chat 的共享目录，不依赖本会话进程
            chat.open_model_picker(cx);
            ok()
        }
        method::MODEL_PICK => {
            // 与 model_picker::view 的行 on_row 闭包同链（↑/↓/Enter 是 GPUI
            // action，合成按键在自动化里到不了卡片，方法直调才可靠）
            let get = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_string);
            let (provider, id) = match (get("provider"), get("id")) {
                (Some(p), Some(i)) if !p.is_empty() && !i.is_empty() => (p, i),
                _ => return Err(bad("需要 {\"provider\": \"商\", \"id\": \"模型id\"}")),
            };
            chat.rt()
                .update(cx, |r, cx| r.select_model(provider, id, cx));
            chat.dlg.model_picker = None;
            cx.notify();
            ok()
        }
        method::PROJECT_SWITCH => {
            let cwd = params_path(params, "cwd")?;
            if !cwd.is_dir() {
                return Err(not_found(format!("目录不存在: {}", cwd.display())));
            }
            chat.switch_project(cwd, cx);
            ok()
        }

        method::PROJECT_NEW_SESSION_IN => {
            let cwd = params_path(params, "cwd")?;
            if !cwd.is_dir() {
                return Err(not_found(format!("目录不存在: {}", cwd.display())));
            }
            chat.new_session_in(cwd, cx);
            ok()
        }
        method::PROJECT_PICKER_OPEN => {
            let fresh = params
                .get("fresh")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            chat.open_project_picker(fresh, cx);
            ok()
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
