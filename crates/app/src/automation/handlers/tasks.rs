//! 046 任务管理大卡的自动化 op：open/close（snapshot 走 dialogs 面的
//! `tasks` 键——卡状态随弹窗面一起上报，UI 测试据此轮询断言）。

use gpui::Context;
use serde_json::Value;

use crate::Chat;

use super::{OpResult, ok};
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        method::TASKS_OPEN => {
            let tab = params.get("tab").and_then(Value::as_u64).unwrap_or(0) as u8;
            chat.open_tasks_dialog(cx);
            if tab > 0 {
                if let Some(p) = chat.dlg.tasks.clone() {
                    let _ = p.update(cx, |p, _| p.tab = tab.min(2));
                }
            }
            cx.notify();
            ok()
        }
        method::TASKS_CLOSE => {
            chat.dlg.tasks = None;
            cx.notify();
            ok()
        }
        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
