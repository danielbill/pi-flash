//! session 面：活跃会话运行态（agent/流式/模型/消息尾/状态栏）。

use crate::session::runtime::SessionRuntime;
use gpui::Context;
use pi_link::protocol::Block;
use serde_json::{Value, json};

use super::{Chat, opt_path, truncate};

/// 活跃会话的运行态（agent 是否在跑、流式状态、消息尾部、模型）。
pub(super) fn session_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    let Some(rt) = chat.pool.runtimes.get(&chat.pool.active_key) else {
        return json!({"active_key": chat.pool.active_key, "exists": false});
    };
    let r = rt.read(cx);
    json!({
        "exists": true,
        "key": r.key,
        "file": opt_path(r.file.as_ref()),
        "cwd": r.cwd.display().to_string(),
        "status": truncate(&r.status, 200),
        "agent_running": r.agent_running,
        "bash_running": r.bash_running,
        "streaming": r.streaming_content,
        "waiting": r.phase_waiting,
        "phase_label": r.phase_label(),
        "has_process": r.agent.session.is_some(),
        "compacting": r.compacting,
        "forking": r.forking,
        "input": truncate(&r.input, 400),
        "tools_preset": r.tools_preset,
        // 031：本档的会话插件集 + 胶囊标签口径
        "ext_sources": r.ext_sources,
        "tools_preset_label": r.tool_preset_label(),
        "thinking_override": r.thinking_override,
        "model": model_json(&r),
        // 草稿态 pill 实际显示的模型（与 input/area.rs 同一条链：state →
        // 手选 → new_session_default 兜底）；有 state 时与 model 同值
        "draft_model": draft_model_json(chat, &r),
        "state": state_json(&r),
        "messages": messages_json(&r),
        // 扩展 UI 面（test/ext-ui 台架断言出口；数据与渲染同源，见 dialogs/ext_ui.rs）
        // 034：ext_status 已下沉到运行时（per-session，防切会话串味）
        "ext_status": r.ext_status.clone(),
        // 031-1 计划模式投影（pif-ui 断言出口：/plan 切换 → 广播 → record；
        // 菜单勾选与防哑弹回滚都读这两个字段）
        "plan_enabled": r.plan_enabled,
        "plan_pending": r.plan_pending,
        // 034 会话区底部状态栏（渲染面）：开关 / 是否占位 / 可点动作
        "status_bar": status_bar_json(&r),
        "ext_widgets": chat
            .aux
            .ext_widgets
            .iter()
            .map(|(k, lines, above)| json!({"key": k, "lines": lines, "above_editor": above}))
            .collect::<Vec<_>>(),
        "ext_notice": chat
            .aux
            .ext_notice
            .as_ref()
            .map(|(msg, ty)| json!({"message": msg, "type": ty})),
    })
}

/// 034 会话区底部状态栏快照：开关 + 是否占位 + 每项的动作/禁用态。
/// 与 `session/footer_bar.rs` 共用同一张动作表（`action_of`）。
pub(super) fn status_bar_json(r: &SessionRuntime) -> Value {
    let enabled = crate::services::workspace::session_status_bar_on();
    // 与渲染同一口径：默认全显示；hidden_keys = 用户逐键静音（ext_status_muted），
    // 无动作键 action = null（纯文本，不可点）
    let muted = crate::services::workspace::ext_status_muted_keys();
    let mut items: Vec<Value> = Vec::new();
    let mut hidden: Vec<&str> = Vec::new();
    for (key, text) in r.ext_status.iter().filter(|(_, text)| !text.is_empty()) {
        if muted.iter().any(|m| m == key) {
            hidden.push(key.as_str());
            continue;
        }
        let action = crate::session::footer_bar::action_of(key);
        items.push(json!({
            "key": key,
            "text": truncate(text, 200),
            "action": action.map(|a| a.tag()),
            "disabled": r.agent_running
                && matches!(action, Some(crate::session::footer_bar::StatusAction::Command(_))),
        }));
    }
    items.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
    json!({
        "enabled": enabled,
        "visible": enabled && !items.is_empty(),
        "items": items,
        "hidden_keys": hidden,
    })
}

pub(super) fn model_json(r: &SessionRuntime) -> Value {
    match r.state.as_ref().and_then(|s| s.model.as_ref()) {
        Some(m) => json!({
            "id": m.id, "name": m.name, "provider": m.provider,
            "context_window": m.context_window,
        }),
        None => Value::Null,
    }
}

/// 草稿 pill 的模型（镜像 `input/area.rs` 的三段链）：草稿手选 `pending_model`
/// （无活进程时的最新意图）→ state 模型 → `new_session_default` 兜底。
pub(super) fn draft_model_json(chat: &Chat, r: &SessionRuntime) -> Value {
    let picked = r
        .pending_model
        .clone()
        .or_else(|| {
            r.state
                .as_ref()
                .and_then(|s| s.model.as_ref())
                .map(|m| (m.provider.clone(), m.id.clone()))
        })
        .or_else(|| chat.new_session_default().0);
    match picked {
        Some((provider, id)) => json!({
            "provider": provider,
            "name": chat.model_display_name(&provider, &id),
            "id": id,
        }),
        None => Value::Null,
    }
}

pub(super) fn state_json(r: &SessionRuntime) -> Value {
    match &r.state {
        None => Value::Null,
        Some(s) => json!({
            "is_streaming": s.is_streaming,
            "thinking_level": s.thinking_level,
            "session_name": s.session_name,
            "session_file": s.session_file,
            "session_id": s.session_id,
            "message_count": s.message_count,
        }),
    }
}

/// 消息尾部（最多 20 条）：role + 文本拼接预览 + 工具调用计数。
pub(super) fn messages_json(r: &SessionRuntime) -> Value {
    let take = r.messages.len().saturating_sub(20);
    let msgs: Vec<Value> = r.messages[take..]
        .iter()
        .map(|m| {
            let mut text = String::new();
            let mut tools = 0usize;
            for b in &m.blocks {
                match b {
                    Block::Text { text: t, .. } => text.push_str(t),
                    Block::ToolCall { .. } => tools += 1,
                    _ => {}
                }
            }
            let role = match m.role {
                crate::session::messages::Role::User => "user",
                crate::session::messages::Role::Assistant => "assistant",
                crate::session::messages::Role::Custom => "custom",
                crate::session::messages::Role::Bash => "bash",
            };
            // bash 卡：命令/输出/终态从 BashInfo 取（blocks 恒空）
            let bash = m.bash.as_ref().map(|b| {
                json!({
                    "command": b.command,
                    "output": truncate(b.output.trim(), 400),
                    "exit_code": b.exit_code,
                    "cancelled": b.cancelled,
                    "excluded": b.excluded,
                    "running": b.running,
                })
            });
            json!({
                "role": role,
                "text": truncate(text.trim(), 400),
                "tool_calls": tools,
                "bash": bash,
            })
        })
        .collect();
    json!({"count": r.messages.len(), "tail": msgs})
}
