//! 数据化界面快照：Chat/SessionRuntime 的字段 → JSON。
//!
//! 原则：只读 pub(crate) 字段与既有 read API（settings 走
//! SettingsFormData::snapshot 同源先例），绝不为了快照改 app 状态。
//! surface 名与 method::UI_SNAPSHOT 的 params.surface 对应。

use crate::session::runtime::SessionRuntime;
use crate::{Chat, Dialog};
use gpui::Context;
use pi_link::protocol::Block;
use serde_json::{json, Value};
use std::time::UNIX_EPOCH;

pub(crate) const SURFACES: [&str; 8] = [
    "app",
    "sessions",
    "session",
    "composer",
    "files",
    "git",
    "settings",
    "dialogs",
];

pub(crate) fn surface(
    chat: &Chat,
    window: &gpui::Window,
    cx: &Context<Chat>,
    name: Option<&str>,
) -> Option<Value> {
    match name {
        None => {
            let mut all = json!({});
            for s in SURFACES {
                all[s] = surface_one(chat, window, cx, s);
            }
            Some(all)
        }
        Some(n) => {
            if SURFACES.contains(&n) {
                Some(surface_one(chat, window, cx, n))
            } else {
                None
            }
        }
    }
}

fn surface_one(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>, name: &str) -> Value {
    match name {
        "app" => app_surface(chat, window, cx),
        "sessions" => sessions_surface(chat),
        "session" => session_surface(chat, cx),
        "composer" => composer_surface(chat),
        "files" => files_surface(chat, cx),
        "git" => git_surface(chat),
        "settings" => settings_surface(chat, cx),
        "dialogs" => dialogs_surface(chat),
        _ => Value::Null,
    }
}

pub(crate) fn app_info(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>) -> Value {
    let mut v = app_surface(chat, window, cx);
    v["pid"] = json!(std::process::id());
    v["surfaces"] = json!(SURFACES);
    v
}

/// 当前焦点落在谁身上（焦点类 bug 的数据化探针）：与已知句柄逐一比对。
/// "other" = 焦点在未登记的输入上——正是不该发生时的线索。
fn focused_str(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>) -> Value {
    if let Some(c) = &chat.composer {
        if c.read(cx).focus_handle_in(cx).is_focused(window) {
            return json!("composer");
        }
    }
    if chat.focus.is_focused(window) {
        return json!("chat");
    }
    if chat.dialog_focus.is_focused(window) {
        return json!("dialog");
    }
    if chat.git_commit_input.read(cx).focus_handle().is_focused(window) {
        return json!("git_commit");
    }
    if chat.ext_input.read(cx).focus_handle().is_focused(window) {
        return json!("ext_input");
    }
    for t in &chat.terminals {
        if t.focus.is_focused(window) {
            return json!(format!("terminal:{}", t.id));
        }
    }
    json!("other")
}

fn app_surface(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>) -> Value {
    // 斜杠/@ 补全菜单（031）：形态 + 候选（insert 值，断言模糊命中/触发条件）
    let menu_kind = chat.active_menu(cx);
    let composer_menu = json!({
        "kind": match menu_kind {
            Some(crate::MenuKind::Slash) => "slash",
            Some(crate::MenuKind::At) => "at",
            None => "none",
        },
        "items": if menu_kind.is_some() {
            chat.menu_items(cx)
                .iter()
                .map(|i| i.insert.clone())
                .take(20)
                .collect::<Vec<_>>()
        } else {
            Vec::<String>::new()
        },
    });
    json!({
        "app": "pi-flash",
        "version": env!("CARGO_PKG_VERSION"),
        "theme": crate::theme::theme_name(),
        "lang": crate::i18n::LANG_LABELS[crate::i18n::lang_ix()],
        "cwd": chat.cwd.display().to_string(),
        "branch": chat.branch,
        "booted": chat.booted,
        "focused": focused_str(chat, window, cx),
        "dock_panel": chat.dock_panel.as_str(),
        "content_view": content_view_str(chat.content_view),
        "active_key": chat.active_key,
        "active_file": opt_path(chat.active_file.as_ref()),
        "terminals": chat.terminals.len(),
        "active_terminal": chat.active_terminal,
        // 031 自定义档选择面板：开合 + 临时勾选数（确认前不动 runtime）
        // 插件菜单：按钮是否可用（自定义档 + 还没开聊）+ 三层各自条数
        "plugin_menu": {
            "available": chat
                .runtimes
                .get(&chat.active_key)
                .map(|rt| {
                    let r = rt.read(cx);
                    r.tool_preset_key() == "custom" && r.messages.is_empty()
                })
                .unwrap_or(false),
        },
        "plugin_picker": chat.plugin_picker.as_ref().map(|p| {
            let sel = |src: &str| p.pending.contains(src);
            let count = |list: &[Value], want: bool| {
                list.iter()
                    .map(pi_link::skills::entry_source)
                    .filter(|s| !s.is_empty() && sel(s) == want)
                    .count()
            };
            json!({
                "pending": p.pending.len(),
                "selected": chat
                    .mc_pkgs_global
                    .iter()
                    .chain(chat.mc_pkgs_project.iter())
                    .map(pi_link::skills::entry_source)
                    .filter(|s| !s.is_empty() && sel(s))
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                "project_unselected": count(&chat.mc_pkgs_project, false),
                "global_unselected": count(&chat.mc_pkgs_global, false),
            })
        }),
        // 瞬时提示（set_status）：运行中拦截 / 对话中途禁改 等断言用
        "status_toast": chat.status_toast.as_ref().map(|(m, _)| m.clone()),
        "pill_menu": chat.pill_menu.map(|m| match m {
            crate::PillMenu::Thinking => "thinking",
            crate::PillMenu::Tools => "tools",
        }),
        // 斜杠/@ 补全菜单（031）
        "composer_menu": composer_menu,
        "pill_anchor": chat.pill_anchor.map(|p| json!({
            "x": f32::from(p.x),
            "y": f32::from(p.y),
        })),
    })
}

fn sessions_surface(chat: &Chat) -> Value {
    let sessions: Vec<Value> = chat
        .sessions
        .iter()
        .map(|s| {
            json!({
                "path": s.path.display().to_string(),
                "id": s.id,
                "cwd": s.cwd,
                "name": s.name,
                "preview": truncate(&s.preview, 120),
                "message_count": s.message_count,
                "modified_ms": s
                    .modified
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                "unread": chat.unread.contains(&s.path),
                "loaded": chat.runtimes.contains_key(&s.path.display().to_string()),
            })
        })
        .collect();
    let projects: Vec<Value> = chat
        .projects
        .iter()
        .map(|g| json!({"name": g.name, "path": g.path.display().to_string(), "sessions": g.sessions.len()}))
        .collect();
    json!({
        "cwd": chat.cwd.display().to_string(),
        "active_key": chat.active_key,
        "active_file": opt_path(chat.active_file.as_ref()),
        "runtimes": chat.runtimes.keys().cloned().collect::<Vec<_>>(),
        "sessions": sessions,
        "projects": projects,
    })
}

/// 活跃会话的运行态（agent 是否在跑、流式状态、消息尾部、模型）。
fn session_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    let Some(rt) = chat.runtimes.get(&chat.active_key) else {
        return json!({"active_key": chat.active_key, "exists": false});
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
        "state": state_json(&r),
        "messages": messages_json(&r),
    })
}

fn model_json(r: &SessionRuntime) -> Value {
    match r.state.as_ref().and_then(|s| s.model.as_ref()) {
        Some(m) => json!({
            "id": m.id, "name": m.name, "provider": m.provider,
            "context_window": m.context_window,
        }),
        None => Value::Null,
    }
}

fn state_json(r: &SessionRuntime) -> Value {
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
fn messages_json(r: &SessionRuntime) -> Value {
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

fn composer_surface(chat: &Chat) -> Value {
    json!({
        "text": truncate(&chat.input, 2000),
        "pending_images": chat.pending_images.len(),
        "history_len": chat.history.len(),
        "composer_open": chat.composer.is_some(),
    })
}

fn files_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    let rows: Vec<Value> = chat
        .tree_rows
        .iter()
        .map(|r| {
            json!({
                "path": r.path.display().to_string(),
                "name": r.name,
                "depth": r.depth,
                "is_dir": r.is_dir,
                "expanded": r.expanded,
                "git": r.git.map(|g| format!("{g:?}")),
                "changed_dot": r.changed_dot,
                "ignored": r.ignored,
            })
        })
        .collect();
    let mut expanded: Vec<String> =
        chat.expanded_dirs.iter().map(|p| p.display().to_string()).collect();
    expanded.sort();
    // 文件 tab 明细（023）：编辑器状态事务断言用（dirty/conflict/预览态）
    let active_file = chat.active_file_path();
    let file_tabs: Vec<Value> = chat
        .panel_tabs
        .iter()
        .filter_map(|t| match t {
            crate::PanelTab::File(p) => {
                let f = chat.file_cache.get(p);
                Some(json!({
                    "file": p.display().to_string(),
                    "active": active_file.as_ref().is_some_and(|a| crate::services::workspace::same_path(a, p)),
                    "dirty": f.map(|f| f.dirty).unwrap_or(false),
                    "conflict": f.and_then(|f| f.conflict.as_ref()).map(|c| format!("{c:?}")),
                    "md_source": f.map(|f| f.md_source).unwrap_or(false),
                    "has_editor": f.map(|f| f.editor.is_some()).unwrap_or(false),
                    "content_len": f.map(|f| f.content.len()).unwrap_or(0),
                    "editor_len": f.and_then(|f| f.editor.as_ref()).map(|e| e.read(cx).value().len()).unwrap_or(0),
                    "pending": f.map(|f| f.reload_pending).unwrap_or(false),
                }))
            }
            _ => None,
        })
        .collect();
    json!({
        "cwd": chat.cwd.display().to_string(),
        "expanded": expanded,
        "rows": rows,
        "file_tabs": file_tabs,
        "ext_probe": {"runs": chat.ext_probe.0, "hits": chat.ext_probe.1},
        "open_tabs": chat.panel_tabs.iter().map(|t| match t {
            crate::PanelTab::Term(i) => json!({"term": i}),
            crate::PanelTab::File(p) => json!({"file": p.display().to_string()}),
        }).collect::<Vec<_>>(),
    })
}

fn git_surface(chat: &Chat) -> Value {
    json!({
        "branch": chat.branch,
        "tab": format!("{:?}", chat.git_tab),
        "error": chat.git_error,
        "add_del": {"add": chat.git_add_del.0, "del": chat.git_add_del.1},
        "files": chat.git_files.iter().map(|f| json!({
            "path": f.path.display().to_string(),
            "status": format!("{:?}", f.status),
            "staged": f.staged,
        })).collect::<Vec<_>>(),
    })
}

fn settings_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    match &chat.settings {
        None => json!({"open": false}),
        Some(panel) => {
            let p = panel.read(cx);
            json!({
                "open": true,
                "tab": p.tab,
                "section": p.section,
                "error": p.error,
                "mc_state": format!("{:?}", chat.mc_state),
                "mc_project_scope": chat.mc_project_scope,
            })
        }
    }
}

fn dialogs_surface(chat: &Chat) -> Value {
    let dialog = chat.dialog.as_ref().map(|d| match d {
        Dialog::ModelSelect { .. } => "model_select",
        Dialog::GitDiff { .. } => "git_diff",
        Dialog::SessionSearch { .. } => "session_search",
        Dialog::ProjectPicker { .. } => "project_picker",
        Dialog::ImagePreview { .. } => "image_preview",
        Dialog::SessionInfo { .. } => "session_info",
        Dialog::FileDirty { .. } => "file_dirty",
        Dialog::NewFile { .. } => "new_file",
        Dialog::WxQr => "wx_qr",
    });
    // 打开项目菜单：列表数据随弹窗一起上报（扫描是异步回填，UI 测试
    // 据此轮询就绪）
    let project_hits: Vec<Value> = match chat.dialog.as_ref() {
        Some(Dialog::ProjectPicker { .. }) => chat
            .project_hits
            .iter()
            .map(|p| json!({"name": p.name, "path": p.path.display().to_string()}))
            .collect(),
        _ => Vec::new(),
    };
    json!({
        "dialog": dialog,
        "project_hits": project_hits,
        "project_filter": chat.project_filter,
        "confirm_delete": opt_path(chat.confirm_delete.as_ref()),
        "renaming": opt_path(chat.renaming.as_ref()),
        "ext_dialog": chat.ext_dialog.is_some(),
        "toast": chat.status_toast.as_ref().map(|(t, _)| truncate(t, 200)),
        "settings_open": chat.settings.is_some(),
        // 060 远程控制：扫码弹窗的实时状态（pif-ui 可据此断言 UI 状态）
        "wx_qr": match &chat.remote.qr {
            crate::remote_control::QrState::Idle => "idle",
            crate::remote_control::QrState::Loading => "loading",
            crate::remote_control::QrState::Ready { .. } => "ready",
            crate::remote_control::QrState::Scanned => "scanned",
            crate::remote_control::QrState::Done { .. } => "done",
            crate::remote_control::QrState::Expired => "expired",
            crate::remote_control::QrState::Error(_) => "error",
        },
        "wx_running": chat.remote.is_running(),
        "wx_bound": chat.remote.bound,
        "wx_bind_code": chat.remote.bind_code(&chat.active_key),
    })
}

fn content_view_str(v: crate::ContentView) -> &'static str {
    match v {
        crate::ContentView::Chat => "chat",
        crate::ContentView::Term => "term",
        crate::ContentView::File => "file",
    }
}

fn opt_path(p: Option<&std::path::PathBuf>) -> Value {
    match p {
        Some(p) => json!(p.display().to_string()),
        None => Value::Null,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n).collect();
        format!("{cut}…")
    }
}
