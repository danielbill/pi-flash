//! git 组：stage/unstage/commit/push/refresh/set_tab（git 面直调）。

use gpui::Context;
use serde_json::{Value, json};
use std::path::PathBuf;

use super::{OpResult, bad, ok, params_path};
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        // ---- git ----
        method::GIT_STAGE => {
            let r = git_stage_op(chat, params, true);
            chat.ws.git_error = r.err();
            chat.refresh_git(cx);
            cx.notify();
            ok()
        }
        method::GIT_UNSTAGE => {
            let r = git_stage_op(chat, params, false);
            chat.ws.git_error = r.err();
            chat.refresh_git(cx);
            cx.notify();
            ok()
        }
        method::GIT_COMMIT => {
            let msg = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            chat.ws
                .git_commit_input
                .update(cx, |ti, cx| ti.set_value(msg.to_string(), cx));
            chat.git_commit_staged(cx);
            Ok(json!({"ok": chat.ws.git_error.is_none(), "error": chat.ws.git_error}))
        }
        method::GIT_PUSH => {
            chat.git_push_branch(cx);
            Ok(json!({"ok": true, "note": "推送异步进行，git 面看 error 字段"}))
        }
        method::GIT_REFRESH => {
            chat.refresh_git(cx);
            cx.notify();
            ok()
        }
        method::GIT_SET_TAB => {
            let tab = params
                .get("tab")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"tab\": \"changes|history\"}"))?;
            chat.git_set_tab(
                match tab {
                    "changes" => crate::function_panel::git_panel::GitTab::Changes,
                    "history" => crate::function_panel::git_panel::GitTab::History,
                    other => return Err(bad(format!("未知 git 页: {other:?}"))),
                },
                cx,
            );
            ok()
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}

/// stage/unstage 共用体：{"path": "..."} 单文件 或 {"all": true}（unstage
/// 无现成全量函数，逐个 reset 已登记的 staged 文件；批量只保留最后一个
/// 错误——更早的失败被吞，详情走 git_error 进 git 面可见）。
fn git_stage_op(chat: &mut Chat, params: &Value, stage: bool) -> Result<(), String> {
    if params.get("all").and_then(Value::as_bool).unwrap_or(false) && !stage {
        let staged: Vec<PathBuf> = chat
            .ws
            .git_files
            .iter()
            .filter(|f| f.staged)
            .map(|f| f.path.clone())
            .collect();
        let mut last = Ok(());
        if let Some(cwd) = chat.ws.cwd.as_deref() {
            for p in staged {
                last = crate::services::git::git_unstage(cwd, &p);
            }
        }
        last
    } else if let Some(cwd) = chat.ws.cwd.as_deref() {
        if params.get("all").and_then(Value::as_bool).unwrap_or(false) {
            crate::services::git::git_stage_all(cwd)
        } else {
            let path = params_path(params, "path").map_err(|e| e.1)?;
            if stage {
                crate::services::git::git_stage(cwd, &path)
            } else {
                crate::services::git::git_unstage(cwd, &path)
            }
        }
    } else {
        // 无项目态：没有可操作的仓库
        Err("no project".into())
    }
}
