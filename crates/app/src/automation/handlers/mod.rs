//! op 分发：method → 直调 Chat 方法（app 是方法调用驱动的 shell，这是
//! 最贴近真实用户操作、也最稳的执行路径）。改状态的 op 一律以 cx.notify()
//! 收尾（GPUI 铁律：状态变 UI 动）。
//!
//! 结构：dispatch 按 method 首段前缀（`session.send` → "session"）路由到
//! 各组子模块的 dispatch，组内再按完整 method 精确匹配。未路由的前缀与
//! 组内未匹配的 method 都落同一条 unknown_method 回包（消息含原 m，与拆
//! 前单 match 兜底逐字一致）。op 清单唯一事实源仍是
//! `pi_link::automation::method` 常量表。

use gpui::{Context, Window};
use serde_json::{Value, json};
use std::path::PathBuf;

use crate::Chat;

mod app;
mod files;
mod git;
mod input;
mod panel;
mod pickers;
mod plugin;
mod search;
mod session;
mod settings;

type OpResult = Result<Value, (String, String)>;

pub(super) fn dispatch(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    let prefix = m.split_once('.').map_or(m, |(p, _)| p);
    match prefix {
        "app" | "ui" | "theme" | "lang" | "dialog" | "update" => {
            app::dispatch(chat, window, cx, m, params)
        }
        "session" | "composer" => session::dispatch(chat, cx, m, params),
        "panel" | "content" | "terminal" => panel::dispatch(chat, window, cx, m, params),
        "file" | "files" => files::dispatch(chat, window, cx, m, params),
        "git" => git::dispatch(chat, cx, m, params),
        "settings" => settings::dispatch(chat, cx, m, params),
        "plugin" | "plugin_picker" | "mcp_picker" => plugin::dispatch(chat, window, cx, m, params),
        "model" | "project" => pickers::dispatch(chat, cx, m, params),
        "search" => search::dispatch(chat, cx, m, params),
        "input" => input::dispatch(chat, window, cx, m, params),
        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}

fn bad(msg: impl Into<String>) -> (String, String) {
    ("bad_params".into(), msg.into())
}

fn not_found(msg: impl Into<String>) -> (String, String) {
    ("not_found".into(), msg.into())
}

fn ok() -> OpResult {
    Ok(json!({"ok": true}))
}

fn params_path(params: &Value, key: &str) -> Result<PathBuf, (String, String)> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| bad(format!("需要 {{\"{key}\": \"路径\"}}")))
}
