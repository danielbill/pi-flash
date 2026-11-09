//! 问卷解析：transcript 里的 ask_user_question tool_call → AskCard
//! （自 ask_card.rs 外移；手解 args JSON，app 不直接依赖 serde derive）。

use super::{AskCard, AskOpt, AskQ};
use crate::session;
use pi_link::protocol::{Block, ExtUiMethod, ExtensionUiRequest};

// ---------------------------------------------------------------------------
// 解析：tool_call args → AskCard
// ---------------------------------------------------------------------------

/// 从 transcript 末尾向前找最近的 ask_user_question tool_call（args 已收全），
/// 返回（卷标识 = tool_call id，args 原文）。id 是「卷」的唯一标识：同一
/// 问卷的后续请求（含自定义答案追问）都带同一题面，靠它判新卷/旧卡作废。
pub(super) fn find_last_ask_call(
    rt: &session::runtime::SessionRuntime,
) -> Option<(String, String)> {
    for msg in rt.messages.iter().rev() {
        if !matches!(msg.role, session::messages::Role::Assistant) {
            continue;
        }
        for b in msg.blocks.iter().rev() {
            let Block::ToolCall {
                id,
                name,
                args,
                args_partial,
                ..
            } = b
            else {
                continue;
            };
            if name != "ask_user_question" || *args_partial {
                continue;
            }
            return Some((id.clone(), args.clone()));
        }
    }
    None
}

/// 手解 args JSON（app 不直接依赖 serde derive，pi-link 协议层用 Value 同款）
pub(super) fn parse_args(args: &str) -> Option<AskCard> {
    let v: serde_json::Value = serde_json::from_str(args).ok()?;
    let qs = v.get("questions")?.as_array()?;
    if qs.is_empty() || qs.len() > 10 {
        return None;
    }
    let mut questions = Vec::with_capacity(qs.len());
    for q in qs {
        let question = str_field(q, "question");
        if question.trim().is_empty() {
            return None;
        }
        let header = str_field(q, "header");
        let multi = q
            .get("multiSelect")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let opts_v = q.get("options")?.as_array()?;
        if opts_v.is_empty() || opts_v.len() > 12 {
            return None;
        }
        let mut options = Vec::with_capacity(opts_v.len());
        for o in opts_v {
            let label = str_field(o, "label");
            if label.trim().is_empty() {
                return None;
            }
            let desc = str_field(o, "description");
            let preview = o
                .get("preview")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty());
            options.push(AskOpt {
                label,
                desc,
                preview,
            });
        }
        let sel = vec![false; options.len()];
        questions.push(AskQ {
            header,
            question,
            multi,
            options,
            sel,
            other_sel: false,
            other_text: String::new(),
        });
    }
    Some(AskCard {
        call_id: String::new(),
        questions,
        active: 0,
        pending: None,
        open: false,
        pv_scroll: gpui::ScrollHandle::new(),
    })
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// 请求 title 是否对应当前题（title = `[header] question` + 折叠 preview 块）
pub(super) fn title_matches(req: &ExtensionUiRequest, q: &AskQ) -> bool {
    let title = match &req.method {
        ExtUiMethod::Select { title, .. } | ExtUiMethod::Input { title, .. } => title,
        _ => return false,
    };
    !q.question.is_empty() && title.contains(&q.question)
}
