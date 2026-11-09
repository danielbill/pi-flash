//! 001-4 pfctx_manager 宿主执行面：agent 设置 op 的接收队列 + 设置页同链分发。
//!
//! 链路：扩展 `pfctx_manager` set → `pi.appendEntry("pfctx-manager-op")` →
//! `entry_appended` 实时推宿主 → `on_event` 收进本队列 → 120ms 泵
//! [`execute_pending`] 在 **Chat 上下文**逐 op 执行——每个 op 调设置页同款
//! 函数（含 sync_gpui_tokens / 字号 sync / 子代理重绑等完整副作用），与人工
//! 点击完全同链。执行留痕 `pfctx.log [agent]`；hub 写自动 `[write]`。
//!
//! 队列 = 静态 Mutex<VecDeque>（pf-svc `SvcShared.queues` 同款先例）：跨
//! RPC 线程（on_event）与主线程（泵）的单生产多消费信箱，容量天然有界
//! （agent 工具调用频率）。

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;

/// 单条设置 op（扩展目录键 = 点路径，与设置页函数一一对应）。
pub(crate) struct PfctxOp {
    pub session: String,
    pub setting: String,
    pub value: Value,
}

static QUEUE: Mutex<VecDeque<PfctxOp>> = Mutex::new(VecDeque::new());

pub(crate) fn push(session: &str, setting: &str, value: Value) {
    QUEUE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back(PfctxOp {
            session: session.to_string(),
            setting: setting.to_string(),
            value,
        });
}

fn drain() -> Vec<PfctxOp> {
    QUEUE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
        .collect()
}

/// 120ms 泵消费：在 Chat 上下文逐 op 执行（设置页同链）。
pub(crate) fn execute_pending(chat: &mut crate::Chat, cx: &mut gpui::Context<crate::Chat>) {
    let ops = drain();
    if ops.is_empty() {
        return;
    }
    for op in ops {
        let line = match execute_one(chat, cx, &op.setting, &op.value) {
            Ok(msg) => format!("{} = {} → {}", op.setting, op.value, msg),
            Err(e) => format!("{} 拒绝: {}（值 = {}）", op.setting, e, op.value),
        };
        pf_context::log("agent", &format!("[{}] {}", op.session, line));
    }
    cx.notify();
}

fn execute_one(
    chat: &mut crate::Chat,
    cx: &mut gpui::Context<crate::Chat>,
    setting: &str,
    value: &Value,
) -> Result<String, String> {
    use crate::services::workspace as ws;

    match setting {
        // ---- 带副作用链的 setter（与设置页调用点同函数）----
        "subagents.capacity" => {
            let v = num(value)? as u64;
            ws::set_subagent_capacity(v);
            Ok(format!("容量已设（双写 agents/config.json）"))
        }
        "subagents.enabled" => {
            let b = bool_of(value)?;
            ws::set_subagent_module_switch(b);
            chat.apply_subagent_switch(cx); // 全池 pending + 空闲立即重绑
            Ok(format!("模块开关 = {b}（空闲会话已重绑）"))
        }
        "appearance.theme" => {
            let id = string_of(value)?;
            if !crate::appearance::persist_theme(&id) {
                return Err(format!("未知主题 id: {id:?}"));
            }
            crate::appearance::sync_gpui_tokens(cx);
            Ok(format!("主题 = {id:?}（token 已重映射）"))
        }
        "appearance.lang" => {
            let ix = num(value)? as usize;
            crate::i18n::set_lang(ix);
            ws::save_lang_pref(ix);
            Ok(format!("语言 = ix{ix}"))
        }
        "appearance.session_font" | "appearance.panel_font" | "appearance.markdown_font" => {
            let spec: pf_context::sections::FontSpec = serde_json::from_value(value.clone())
                .map_err(|e| format!("字体对象 {{family,size}}: {e}"))?;
            let slot = match setting {
                "appearance.session_font" => crate::appearance::FontSlot::Session,
                "appearance.panel_font" => crate::appearance::FontSlot::Panel,
                _ => crate::appearance::FontSlot::File,
            };
            crate::appearance::save_font(slot, spec);
            Ok("字体已设（字号已 sync）".into())
        }
        "shell.sound" => {
            let b = bool_of(value)?;
            ws::save_sound_pref(b);
            chat.broadcast_sound_on(b, cx);
            if b {
                ws::play_notify_sound();
            }
            Ok(format!("提示音 = {b}"))
        }
        "models.default_model" => {
            // null = 取消；"provider/model" = 成对设置
            let pref = if value.is_null() {
                None
            } else {
                let s = string_of(value)?;
                let (p, m) = s
                    .split_once('/')
                    .ok_or_else(|| format!("需要 \"provider/model\"，得到 {s:?}"))?;
                Some((p.to_string(), m.to_string()))
            };
            ws::set_default_model_pref(pref);
            Ok("默认模型已设".into())
        }
        "agent.self_debug" => {
            let b = bool_of(value)?;
            pf_context::hub()
                .settings
                .set::<pf_context::sections::Agent>(|a| a.self_debug = b);
            chat.apply_subagent_switch(cx); // 工具清单注入/移除 = 重绑
            Ok(format!("自我调试 = {b}（空闲会话已重绑）"))
        }

        // ---- 纯快照项：薄壳 diff 链（save_app_settings → hub.set）----
        "chat.send_mode" => {
            let mode = string_of(value)?;
            if mode != "queue" && mode != "steer" {
                return Err("需要 \"queue\" 或 \"steer\"".into());
            }
            ws::save_send_mode(&mode);
            Ok(format!("send_mode = {mode:?}"))
        }
        "chat.show_thinking" => patch(bool_of(value)?, |s, v| s.show_thinking = Some(v)),
        "chat.git_markers" => patch(bool_of(value)?, |s, v| s.git_markers = Some(v)),
        "chat.md_live_preview" => patch(bool_of(value)?, |s, v| s.md_live_preview = Some(v)),
        "chat.content_width" => patch(num(value)? as u64, |s, v| s.chat_content_width = Some(v)),
        "sessions.startup_restore" => patch(bool_of(value)?, |s, v| s.restore = Some(v)),
        "sessions.load_window_days" => {
            patch(num(value)? as u64, |s, v| s.load_window_days = Some(v))
        }
        "sessions.display_count" => {
            patch(num(value)? as u64, |s, v| s.session_display_count = Some(v))
        }
        "sessions.auto_collapse_hours" => {
            patch(num(value)? as u64, |s, v| s.auto_collapse_hours = Some(v))
        }
        "sessions.auto_clean_days" => patch(num(value)? as u64, |s, v| s.auto_clean_days = Some(v)),
        "index.at_index_ttl_secs" => {
            patch(num(value)? as u64, |s, v| s.at_index_ttl_secs = Some(v))
        }
        "index.at_index_max_projects" => {
            patch(num(value)? as u64, |s, v| s.at_index_max_projects = Some(v))
        }
        "shell.autohide_icons" => patch(bool_of(value)?, |s, v| s.autohide_icons = Some(v)),
        "updates.auto_update" => patch(bool_of(value)?, |s, v| s.auto_update = Some(v)),

        other => Err(format!(
            "目录外参数: {other:?}（先 query 查看可用项；恒拒项 = subagents.max_depth / appearance.icon_theme）"
        )),
    }
}

/// 纯快照项统一入口：改 AppSettings 视图字段 → save_app_settings（diff → hub.set）。
fn patch<T>(
    v: T,
    f: impl FnOnce(&mut crate::services::workspace::AppSettings, T),
) -> Result<String, String> {
    let mut s = crate::services::workspace::app_settings();
    f(&mut s, v);
    crate::services::workspace::save_app_settings(&s);
    Ok("已应用".into())
}

fn num(value: &Value) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("需要 number，得到 {value}"))
}

fn bool_of(value: &Value) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("需要 boolean，得到 {value}"))
}

fn string_of(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("需要 string，得到 {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 队列 FIFO 与清空语义（跨线程信箱的最小契约）。
    #[test]
    fn push_then_drain_is_fifo_and_empties() {
        push("s1", "subagents.capacity", Value::from(6));
        push("s1", "chat.send_mode", Value::from("steer"));
        let ops = drain();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].setting, "subagents.capacity");
        assert_eq!(ops[1].setting, "chat.send_mode");
        assert!(drain().is_empty(), "drain 即清空");
    }
}
