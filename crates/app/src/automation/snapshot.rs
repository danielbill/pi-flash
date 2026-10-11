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

pub(crate) const SURFACES: [&str; 9] = [
    "app",
    "sessions",
    "session",
    "composer",
    "files",
    "git",
    "settings",
    "dialogs",
    "term",
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
        "dialogs" => dialogs_surface(chat, cx),
        "term" => term_surface(chat, window, cx),
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

/// 终端网格快照：每个 tab 的状态/焦点/可见网格前若干行文本（键入回显、
/// 提示符出现与否的断言载体）。rows 截前 6 行、cols 全宽；只读不写。
fn term_surface(chat: &Chat, window: &gpui::Window, _cx: &Context<Chat>) -> Value {
    let tabs = chat
        .terminals
        .iter()
        .map(|t| {
            let focused = t.focus.is_focused(window);
            let mode = *t.term.lock().mode();
            let (show_cursor, alt_screen) = (
                mode.contains(alacritty_terminal::term::TermMode::SHOW_CURSOR),
                mode.contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
            );
            let rows = crate::terminal::snapshot(&t.term.lock(), t.rows, None);
            let text: Vec<String> = rows
                .into_iter()
                .take(6)
                .map(|r| r.text.trim_end().to_string())
                .collect();
            let status = match &t.status {
                crate::terminal::TermStatus::Ready => json!("ready"),
                crate::terminal::TermStatus::Exited(c) => json!({ "exited": c }),
            };
            json!({
                "id": t.id,
                "cwd": t.cwd.display().to_string(),
                "status": status,
                "focused": focused,
                "cols": t.cols,
                "rows": t.rows,
                "preedit": t.preedit,
                "show_cursor": show_cursor,
                "alt_screen": alt_screen,
                "grid": text,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "terminals": tabs,
        "active_terminal": chat.active_terminal,
        "content_view": content_view_str(chat.content_view),
    })
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
    // 窗口几何（0uq 截图配套）：物理像素宽高可直接对 ui.screenshot 的
    // width/height 断言
    let scale = window.scale_factor();
    let viewport = window.viewport_size();
    json!({
        "app": "pi-flash",
        "version": env!("CARGO_PKG_VERSION"),
        "theme": crate::theme::theme_name(),
        "lang": crate::i18n::LANG_LABELS[crate::i18n::lang_ix()],
        "cwd": chat.cwd.display().to_string(),
        "branch": chat.branch,
        "booted": chat.booted,
        "focused": focused_str(chat, window, cx),
        "window": {
            "width_px": (f32::from(viewport.width) * scale).round() as i64,
            "height_px": (f32::from(viewport.height) * scale).round() as i64,
            "scale_factor": scale,
        },
        "dock_panel": chat.dock_panel.as_str(),
        "content_view": content_view_str(chat.content_view),
        "active_key": chat.active_key,
        "active_file": opt_path(chat.active_file.as_ref()),
        "terminals": chat.terminals.len(),
        "active_terminal": chat.active_terminal,
        // 031/040 full+ 档扩展面板：按钮可用性（full+ 档即可，勾选即生效）
        "plugin_menu": {
            "available": chat
                .runtimes
                .get(&chat.active_key)
                .map(|rt| rt.read(cx).tool_preset_key() == "custom")
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
            let mut pending_sources: Vec<String> = p.pending.iter().cloned().collect();
            pending_sources.sort_by_key(|s| pi_link::skills::display_source(s).to_lowercase());
            json!({
                "pending": p.pending.len(),
                "pending_sources": pending_sources,
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
        "pill_anchor": chat.pill_anchor.map(|a| json!({
            "center_x": f32::from(a.center_x),
            "top": f32::from(a.top),
        })),
        // 081 自动更新（新会话页版本号位状态断言）
        "update": {
            "state": match &chat.update {
                crate::services::updater::UpdateState::Idle => "idle",
                crate::services::updater::UpdateState::Downloading { .. } => "downloading",
                crate::services::updater::UpdateState::Ready { .. } => "ready",
                crate::services::updater::UpdateState::Available { .. } => "available",
            },
            "target": match &chat.update {
                crate::services::updater::UpdateState::Downloading { version }
                | crate::services::updater::UpdateState::Ready { version }
                | crate::services::updater::UpdateState::Available { version } => Some(version.clone()),
                crate::services::updater::UpdateState::Idle => None,
            },
            "auto_update": crate::services::workspace::auto_update(),
            "changelog_open": chat.changelog.as_ref().map(|p| p.version.clone()),
        },
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
    // 面包屑分段文本（023 导航栏）：与渲染共用 breadcrumb_segments（防漂移），
    // 盘符/根合成一段的回归就靠这条断言
    let crumbs: Vec<String> = active_file
        .as_ref()
        .map(|p| {
            crate::editor::view::breadcrumb_segments(&chat.cwd, p)
                .iter()
                .map(|(t, _)| t.to_string())
                .collect()
        })
        .unwrap_or_default();
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
                    // 024 P2：光标 doc 字节偏移（pif-ui 原子跳词/reveal 断言）
                    "editor_cursor": f.and_then(|f| f.editor.as_ref()).map(|e| e.read(cx).cursor()).unwrap_or(0),
                    "pending": f.map(|f| f.reload_pending).unwrap_or(false),
                }))
            }
            _ => None,
        })
        .collect();
    json!({
        "cwd": chat.cwd.display().to_string(),
        "crumbs": crumbs,
        // 023 外部改动检测的监听面：cwd 递归 watch + cwd 外打开文件的
        // 单文件目录 watch（Zed single-file worktree 同款）——断言「工作区外
        // 文件也有监听」就靠这条
        "watch_roots": chat
            .file_watches
            .keys()
            .map(|p| p.display().to_string())
            .collect::<Vec<String>>(),
        "expanded": expanded,
        "rows": rows,
        "file_tabs": file_tabs,
        "ext_probe": {"runs": chat.ext_probe.0, "hits": chat.ext_probe.1},
        "open_tabs": chat.panel_tabs.iter().map(|t| match t {
            crate::PanelTab::Term(i) => json!({"term": i}),
            crate::PanelTab::File(p) => json!({"file": p.display().to_string()}),
            crate::PanelTab::Changelog => json!({"changelog": true}),
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
                // 040 扩展页：op 进行中 + 当前列表（安装后刷新断言用）
                "busy": chat.pkg_op.is_some(),
                "pkg_sources": chat
                    .mc_pkgs_global
                    .iter()
                    .map(pi_link::skills::entry_source)
                    .collect::<Vec<String>>(),
                "mc_state": format!("{:?}", chat.mc_state),
                "mc_project_scope": chat.mc_project_scope,
                // 043 MCP 页：列表 + 表单字段快照（列表断言 / 保存回读用）
                "mcp_servers": chat
                    .mcp_servers
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "scope": format!("{:?}", s.scope),
                            "enabled": s.enabled,
                            "exposure": s.exposure,
                        })
                    })
                    .collect::<Vec<_>>(),
                "mcp_name_value": p.mcp_name.read(cx).value().to_string(),
                "mcp_add_value": p.mcp_add.read(cx).value().to_string(),
            })
        }
    }
}

fn dialogs_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    let dialog = chat.dialog.as_ref().map(|d| match d {
        Dialog::ProviderPicker { .. } => "provider_picker",
        Dialog::GitDiff { .. } => "git_diff",
        Dialog::SessionSearch { .. } => "session_search",
        Dialog::ProjectPicker { .. } => "project_picker",
        Dialog::ImagePreview { .. } => "image_preview",
        Dialog::SessionInfo { .. } => "session_info",
        Dialog::FileDirty { .. } => "file_dirty",
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
        // 模型菜单可见行数（UI 测试断言用：空列表 = 新会话页拉不到目录；
        // 042 起数据源 = pill 化模型菜单的 picker_models）
        "model_rows": match chat.model_picker.as_ref() {
            Some(_) => chat.picker_models(cx).0.len(),
            None => 0,
        },
        "project_hits": project_hits,
        "project_filter": chat.project_filter,
        "confirm_delete": opt_path(chat.confirm_delete.as_ref()),
        // 040 扩展页卸载确认浮层（Some=开着，值为待卸载来源）
        "pkg_confirm_remove": chat.pkg_confirm_remove.clone(),
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
        crate::ContentView::Changelog => "changelog",
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
