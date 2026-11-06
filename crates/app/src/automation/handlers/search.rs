//! search 组：083 项目搜索（开面板/查询/开关/步进/全部替换），状态断言
//! 读 search 面。

use gpui::Context;
use serde_json::Value;

use super::OpResult;
use crate::automation::snapshot;
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(chat: &mut Chat, cx: &mut Context<Chat>, m: &str, params: &Value) -> OpResult {
    match m {
        method::SEARCH_OPEN => {
            chat.open_project_search(cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_QUERY => {
            let text = params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            chat.set_search_query(text, cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_TOGGLE => {
            let which = params
                .get("which")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            chat.search_toggle_by_name(&which, cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_STEP => {
            let forward = params
                .get("forward")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            chat.search_step(forward, cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_REPLACE_ALL => {
            chat.search_replace_all(cx);
            Ok(snapshot::search_surface(chat))
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
