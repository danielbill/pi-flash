//! op 分发：method → 直调 Chat 方法（app 是方法调用驱动的 shell，这是
//! 最贴近真实用户操作、也最稳的执行路径）。改状态的 op 一律以 cx.notify()
//! 收尾（GPUI 铁律：状态变 UI 动）。

use super::snapshot;
use crate::{Chat, ContentView, DockPanel};
use gpui::{Context, Keystroke, Window};
use pi_link::automation::method;
use serde_json::{json, Value};
use std::path::PathBuf;

type OpResult = Result<Value, (String, String)>;

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

pub(super) fn dispatch(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        // ---- 查 ----
        method::APP_INFO => Ok(snapshot::app_info(chat, window, cx)),
        method::UI_SNAPSHOT => {
            let name = params.get("surface").and_then(Value::as_str);
            match snapshot::surface(chat, window, cx, name) {
                Some(mut v) => {
                    // --only 裁剪：只留指定顶层键（files 面整棵树能到几千行，
                    // 断言往往只要一两个键；CLI snapshot --only 走这里）
                    let keys: Vec<String> = match params.get("only") {
                        Some(Value::String(s)) => vec![s.clone()],
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect(),
                        _ => vec![],
                    };
                    if !keys.is_empty() {
                        if let Value::Object(map) = &mut v {
                            map.retain(|k, _| keys.iter().any(|x| x == k));
                        }
                    }
                    Ok(v)
                }
                None => Err(bad(format!(
                    "未知 surface {:?}（可用: {:?}）",
                    name,
                    snapshot::SURFACES
                ))),
            }
        }

        // ---- app ----
        method::APP_QUIT => {
            cx.quit();
            Ok(json!({"quitting": true}))
        }
        method::THEME_SET => {
            let name = params.get("name").and_then(Value::as_str).ok_or_else(|| bad("需要 {\"name\": \"mist|default|dark|rose\"}"))?;
            if !crate::theme::set_by_name(name) {
                return Err(bad(format!("未知主题: {name:?}")));
            }
            crate::appearance::sync_gpui_tokens(cx);
            cx.notify();
            ok()
        }
        method::LANG_SET => {
            let ix = match params.get("ix").and_then(Value::as_u64) {
                Some(ix) => ix as usize,
                None => {
                    let name = params.get("name").and_then(Value::as_str).ok_or_else(|| bad("需要 {\"ix\": 0} 或 {\"name\": \"English\"}"))?;
                    crate::i18n::LANG_LABELS
                        .iter()
                        .position(|l| *l == name)
                        .ok_or_else(|| bad(format!("未知语言: {name:?}（可用 {:?}，或 ix 0/1/2）", crate::i18n::LANG_LABELS)))?
                }
            };
            crate::i18n::set_lang(ix);
            cx.notify();
            ok()
        }
        method::INPUT_KEYS => {
            let keys = params.get("keys").and_then(Value::as_str).ok_or_else(|| bad("需要 {\"keys\": \"ctrl-s\"}"))?;
            let ks = Keystroke::parse(keys).map_err(|e| ("bad_params".to_string(), e.to_string()))?;
            // keystroke 会进真实 UI 事件链（输入框/弹层回调里 weak.update
            // Chat），而本 op 自身已包在 chat.update 里——同步派发必触发
            // entity_map 重入断言（entity_map.rs "already being updated"，
            // 004 项目菜单弹窗的 ESC 首次暴露）。推迟到本效果周期尾（逃出
            // chat.update）再派发；真实用户按键本就不在 chat.update 里，
            // 此改动只是让自动化路径与之一致。dispatched 无法同步取得，
            // 改报 deferred（事务范式照旧：exec → wait/snapshot 断言效果）。
            window.defer(cx, move |window, cx| {
                window.dispatch_keystroke(ks, cx);
            });
            Ok(json!({"deferred": true}))
        }
        method::INPUT_FOCUS => {
            // 元素级 focus（非坐标点击）：焦点/键位类 bug 的自动化入口。
            // window.focus 只设焦点 id + refresh，无同步回调，chat.update
            // 内直调安全（与 dispatch_keystroke 的 defer 不同因）。
            let target = params
                .get("target")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"target\": \"composer|chat|git_commit|terminal\"}"))?;
            let handle = match target {
                "composer" => chat
                    .composer
                    .as_ref()
                    .map(|c| c.read(cx).focus_handle_in(cx))
                    .unwrap_or_else(|| chat.focus.clone()),
                "chat" => chat.focus.clone(),
                "git_commit" => chat.git_commit_input.read(cx).focus_handle(),
                "terminal" => {
                    let ix = chat.active_terminal.unwrap_or(0);
                    chat.terminals
                        .get(ix)
                        .map(|t| t.focus.clone())
                        .ok_or_else(|| not_found("没有打开的终端"))?
                }
                other => {
                    return Err(bad(format!(
                        "未知 focus 目标: {other:?}（composer|chat|git_commit|terminal）"
                    )))
                }
            };
            window.focus(&handle);
            cx.notify();
            Ok(json!({"ok": true, "target": target}))
        }
        method::TERMINAL_OPEN => {
            // 打开（或聚焦）工作区终端；键入回显断言读 term 快照面
            chat.open_terminal(None, window, cx);
            Ok(json!({
                "ok": true,
                "terminals": chat.terminals.len(),
                "active_terminal": chat.active_terminal,
                "content_view": match chat.content_view {
                    ContentView::Chat => "chat",
                    ContentView::Term => "term",
                    ContentView::File => "file",
                },
            }))
        }

        // ---- 会话 ----
        method::SESSION_NEW => {
            chat.new_session(cx);
            let key = chat.active_key.clone();
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
            let rt = chat
                .runtimes
                .get(&key)
                .cloned()
                .ok_or_else(|| not_found(format!("池里没有 key={key:?} 的会话（runtimes 见 ui.snapshot sessions 面）")))?;
            chat.switch_to(rt, cx);
            ok()
        }
        method::SESSION_DELETE => {
            let path = params_path(params, "path")?;
            chat.delete_session(path, cx);
            ok()
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

        // ---- 输入与面板 ----
        method::COMPOSER_SET_TEXT => {
            let text = params.get("text").and_then(Value::as_str).unwrap_or_default();
            chat.set_input(text.to_string(), cx);
            ok()
        }
        method::PANEL_DOCK => {
            let panel = params.get("panel").and_then(Value::as_str).ok_or_else(|| bad("需要 {\"panel\": \"sessions|files|git\"}"))?;
            chat.dock_panel = match panel {
                "sessions" => DockPanel::Sessions,
                "files" => DockPanel::Files,
                "git" => DockPanel::Git,
                other => return Err(bad(format!("未知 dock 面板: {other:?}"))),
            };
            cx.notify();
            ok()
        }
        method::CONTENT_VIEW => {
            let view = params.get("view").and_then(Value::as_str).ok_or_else(|| bad("需要 {\"view\": \"chat|term|file\"}"))?;
            chat.set_content_view(match view {
                "chat" => ContentView::Chat,
                "term" => ContentView::Term,
                "file" => ContentView::File,
                other => return Err(bad(format!("未知内容视图: {other:?}"))),
            });
            cx.notify();
            ok()
        }
        method::FILE_OPEN => {
            let path = params_path(params, "path")?;
            if !path.is_file() {
                return Err(not_found(format!("文件不存在: {}", path.display())));
            }
            chat.open_file_tab(path, cx);
            ok()
        }
        // 023 fileView：保存 / 关闭。close 对脏缓冲会转成确认弹窗——事务
        // 范式里用 ui.snapshot 的 dialogs surface + file.close 收尾。
        method::FILE_SAVE => {
            let path = params_path(params, "path")?;
            chat.save_file(&path, cx);
            ok()
        }
        method::FILE_CLOSE => {
            let path = params_path(params, "path")?;
            if !chat
                .panel_tabs
                .iter()
                .any(|t| matches!(t, crate::PanelTab::File(p) if crate::services::workspace::same_path(p, &path)))
            {
                return Err(not_found(format!("文件 tab 未打开: {}", path.display())));
            }
            chat.close_file_tab(&path, cx);
            ok()
        }
        method::FILE_SET_TEXT => {
            let path = params_path(params, "path")?;
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"text\": \"内容\"}"))?
                .to_string();
            let ed = chat
                .file_cache
                .get(&path)
                .and_then(|f| f.editor.clone())
                .ok_or_else(|| not_found("文件没有编辑器（未渲染）"))?;
            ed.update(cx, |st, scx| st.set_value(text, window, scx));
            // set_value 发 Change → dirty 按「编辑器值 != 磁盘真值」比较落位
            cx.notify();
            ok()
        }
        method::FILE_VIEW_MODE => {
            let path = params_path(params, "path")?;
            let mode = params
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"mode\": \"source|preview\"}"))?;
            let source = match mode {
                "source" => true,
                "preview" => false,
                other => return Err(bad(format!("未知 mode: {other}"))),
            };
            match chat.file_cache.get_mut(&path) {
                Some(ft) => ft.md_source = source,
                None => return Err(not_found(format!("文件 tab 未打开: {}", path.display()))),
            }
            cx.notify();
            ok()
        }
        method::FILES_TOGGLE_DIR => {
            let path = params_path(params, "path")?;
            if !chat.expanded_dirs.remove(&path) {
                chat.expanded_dirs.insert(path);
            }
            chat.rebuild_tree();
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

        // ---- git ----
        method::GIT_STAGE => {
            let r = git_stage_op(chat, params, true);
            chat.git_error = r.err();
            chat.refresh_git();
            cx.notify();
            ok()
        }
        method::GIT_UNSTAGE => {
            let r = git_stage_op(chat, params, false);
            chat.git_error = r.err();
            chat.refresh_git();
            cx.notify();
            ok()
        }
        method::GIT_COMMIT => {
            let msg = params.get("message").and_then(Value::as_str).unwrap_or_default();
            chat.git_commit_input.update(cx, |ti, cx| ti.set_value(msg.to_string(), cx));
            chat.git_commit_staged(cx);
            Ok(json!({"ok": chat.git_error.is_none(), "error": chat.git_error}))
        }
        method::GIT_PUSH => {
            chat.git_push_branch(cx);
            Ok(json!({"ok": true, "note": "推送异步进行，git 面看 error 字段"}))
        }
        method::GIT_REFRESH => {
            chat.refresh_git();
            cx.notify();
            ok()
        }
        method::GIT_SET_TAB => {
            let tab = params.get("tab").and_then(Value::as_str).ok_or_else(|| bad("需要 {\"tab\": \"changes|history\"}"))?;
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

        // ---- 设置/弹窗 ----
        method::SETTINGS_OPEN => {
            let tab = params.get("tab").and_then(Value::as_u64).unwrap_or(0) as u8;
            chat.open_settings(tab, cx);
            ok()
        }
        method::SETTINGS_CLOSE => {
            chat.settings = None;
            cx.notify();
            ok()
        }
        // 040 扩展页安装直调：走与安装按钮完全相同的 mc_install_package 链路
        method::SETTINGS_INSTALL_EXT => {
            let source = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:… 或整条 pi install …\"}"))?
                .to_string();
            chat.mc_install_package(source, cx);
            ok()
        }
        // 040 卸载确认流：先弹居中确认浮层，再应答确认
        method::SETTINGS_REMOVE_EXT => {
            let source = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:…\"}"))?
                .to_string();
            chat.mc_ask_remove_package(source, cx);
            ok()
        }
        method::SETTINGS_PKG_REMOVE_CONFIRM => {
            let confirmed = params.get("ok").and_then(Value::as_bool).unwrap_or(false);
            chat.mc_remove_dialog_close(confirmed, cx);
            ok()
        }
        method::WX_QR_OPEN => {
            chat.remote.begin_qr();
            chat.dialog = Some(crate::Dialog::WxQr);
            cx.notify();
            ok()
        }
        method::DIALOG_CLOSE => {
            chat.dialog = None;
            cx.notify();
            ok()
        }
        method::PROJECT_PICKER_OPEN => {
            let fresh = params.get("fresh").and_then(Value::as_bool).unwrap_or(false);
            chat.open_project_picker(fresh, cx);
            ok()
        }
        method::MODEL_PICKER_OPEN => {
            // 042：模型菜单纯 pill 化后的无坐标入口（锚点走右下兜底）；
            // 列表内容本身来自 Chat 的共享目录，不依赖本会话进程
            chat.open_model_picker(cx);
            ok()
        }
        method::SESSION_TOOLS_PRESET => {
            let preset = params
                .get("preset")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"preset\": \"full|custom|default|read-only|chat-only\"}"))?;
            chat.mc_set_tools_preset(preset, cx);
            ok()
        }
        method::PLUGIN_PICKER_OPEN => {
            chat.open_plugin_picker(cx);
            ok()
        }
        method::PLUGIN_PICKER_TOGGLE => {
            let src = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:…\"}"))?;
            chat.plugin_picker_toggle(src, cx);
            ok()
        }
        method::PLUGIN_PICKER_CANCEL => {
            chat.plugin_picker_cancel(cx);
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
            chat.mcp_picker = None;
            cx.notify();
            ok()
        }
        method::SETTINGS_MCP_SAVE => {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"text\": \"粘贴内容\"}"))?
                .to_string();
            let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            if let Some(st) = chat.settings.clone() {
                st.update(cx, |s, cx| {
                    s.mcp_add.update(cx, |ti, cx| ti.set_value(text, cx));
                    s.mcp_name.update(cx, |ti, cx| ti.set_value(name, cx));
                    cx.notify();
                });
            }
            chat.mcp_save_submit(cx);
            ok()
        }
        method::UI_SCREENSHOT => super::screenshot::shot(window, params),

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}

/// stage/unstage 共用体：{"path": "..."} 单文件 或 {"all": true}（unstage
/// 无现成全量函数，逐个 reset 已登记的 staged 文件）。
fn git_stage_op(chat: &mut Chat, params: &Value, stage: bool) -> Result<(), String> {
    if params.get("all").and_then(Value::as_bool).unwrap_or(false) && !stage {
        let staged: Vec<PathBuf> = chat
            .git_files
            .iter()
            .filter(|f| f.staged)
            .map(|f| f.path.clone())
            .collect();
        let mut last = Ok(());
        for p in staged {
            last = crate::services::git::git_unstage(&chat.cwd, &p);
        }
        last
    } else if params.get("all").and_then(Value::as_bool).unwrap_or(false) {
        crate::services::git::git_stage_all(&chat.cwd)
    } else {
        let path = params_path(params, "path").map_err(|e| e.1)?;
        if stage {
            crate::services::git::git_stage(&chat.cwd, &path)
        } else {
            crate::services::git::git_unstage(&chat.cwd, &path)
        }
    }
}
