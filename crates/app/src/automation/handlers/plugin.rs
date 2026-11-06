//! plugin/plugin_picker/mcp_picker 组：PF 插件驱动（dock/菜单/契约 v3
//! fn_bar/徽标/命令/弹窗/启停）、扩展勾选面板与 MCP 勾选面板。

use gpui::{Context, Window};
use serde_json::{Value, json};

use super::{OpResult, bad, ok, params_path};
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        method::PLUGIN_MENU => {
            // 文件树右键贡献项回调（与右键菜单点击同一条路：plugin_menu_fire）
            let ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            let id = params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"id\": \"set-quick\"}"))?;
            let path = params_path(params, "path")?;
            chat.plugin_menu_fire(ix, id, &path, window, cx);
            ok()
        }
        method::PLUGIN_FNBAR => {
            // 契约 v3 fn_bar：直调 chrome（与功能栏渲染同一条缓存链），
            // 返回当前 icon 清单供断言。session 键口径与 UI 渲染一致
            // （session_file 优先，跨进程重启稳定）
            let key = chat
                .pool
                .runtimes
                .get(&chat.pool.active_key)
                .and_then(|rt| {
                    rt.read(cx)
                        .state
                        .as_ref()
                        .and_then(|s| s.session_file.clone().or_else(|| s.session_id.clone()))
                })
                .unwrap_or_default();
            let rows: Vec<Value> = chat
                .plugin_fn_bar_chrome(&key, cx)
                .into_iter()
                .map(|(ix, icon, tone, tip)| {
                    json!({ "ix": ix, "icon": icon.to_string(), "tone": tone, "tip": tip })
                })
                .collect();
            Ok(json!({ "items": rows }))
        }
        method::PLUGIN_BADGE => {
            // 契约 v3 session_list_render：会话行徽标清单。与 function_panel
            // 行渲染同一条缓存链（plugin_badge）；args {path}，回当前徽标图标/tone。
            // 清单 = 宿主渲染前最终结果：含 PF 主体 pin（置顶时首条）+ 上限 5。
            // 徽标是否"画出来"由这条断言（宿主渲染前取到的最终清单）。
            let path = params_path(params, "path")?;
            let items: Vec<Value> = chat
                .plugin_badge(&path, cx)
                .into_iter()
                .map(|(icon, tone)| json!({ "icon": icon.to_string(), "tone": tone }))
                .collect();
            Ok(json!({ "items": items }))
        }
        method::PLUGIN_FNBAR_CLICK => {
            // 契约 v3 fn_bar：icon 点击同路径（command 回环 env 带 session）。
            // session 键 = **pool.active_key 规范键**（005 §②）：与挂载台账键、
            // spawn 注入的 PF_SESSION_KEY 同源——此前取 pi 侧 session_file/
            // session_id（草稿会话两者皆空 → 空 key，出码归属丢失；真机
            // 2026-11-03），且与台账键漂移。唯一键源，三通道一致。
            let ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            let key = chat.pool.active_key.clone();
            chat.plugin_fn_bar_click(ix, &key, window, cx);
            ok()
        }
        method::PLUGIN_COMMAND_ENV => {
            // 契约 v3 命令直调：{ix?, command, env 对象}（与 switch 翻转同链）
            let ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            let id = params
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"command\": \"toggle-enabled\"}"))?;
            let env = params.get("env").cloned().unwrap_or(Value::Null);
            chat.plugin_command_fire_env(ix, id, &env.to_string(), window, cx);
            ok()
        }
        method::PLUGIN_POPUP_CLOSE => {
            chat.plugin_popup_close(cx);
            ok()
        }
        method::PLUGIN_TOGGLE => {
            // 90-8 L3：停用/启用与市场页开关同链（停用 = evict + unload）
            let pkg = params
                .get("pkg")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"pkg\": \"pf-…\"}"))?
                .to_string();
            let enable = params
                .get("enable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            chat.pf_toggle_plugin(pkg, enable, cx);
            ok()
        }
        method::PLUGIN_DOCK => {
            let ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            if chat.plugins.plugins.get(ix).is_none_or(|e| e.disabled) {
                return Err(bad(format!("插件 ix={ix} 不存在或已停用")));
            }
            // dock="tab" 形态路由到预览页签（与状态栏点击同口径）
            if chat.plugin_dock_is_tab(ix) {
                chat.open_plugin_tab(ix, cx);
            } else {
                chat.open_plugin_dock(ix, cx);
            }
            ok()
        }
        method::PI_EXTENSION_PICKER_OPEN => {
            chat.open_pi_extension_picker(cx);
            ok()
        }
        method::PI_EXTENSION_PICKER_TOGGLE => {
            let src = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:…\"}"))?;
            chat.pi_extension_picker_toggle(src, cx);
            ok()
        }
        method::PI_EXTENSION_PICKER_CANCEL => {
            chat.pi_extension_picker_cancel(cx);
            ok()
        }
        method::MCP_PICKER_OPEN => {
            chat.open_mcp_picker(cx);
            ok()
        }
        method::MCP_PICKER_TOGGLE => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"name\": \"…\"}"))?;
            chat.mcp_picker_toggle(name, cx);
            ok()
        }
        method::MCP_PICKER_CANCEL => {
            chat.dlg.mcp_picker = None;
            cx.notify();
            ok()
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
