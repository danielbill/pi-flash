//! 数据化界面快照：Chat/SessionRuntime 的字段 → JSON。
//!
//! 原则：只读 pub(crate) 字段与既有 read API（settings 走
//! SettingsFormData::snapshot 同源先例），绝不为了快照改 app 状态。
//! surface 名与 method::UI_SNAPSHOT 的 params.surface 对应。

use crate::session::runtime::SessionRuntime;
use crate::{Chat, ContentView, Dialog};
use gpui::{Context, Focusable};
use pi_link::protocol::Block;
use serde_json::{Value, json};
use std::path::Path;
use std::time::UNIX_EPOCH;

pub(crate) const SURFACES: [&str; 10] = [
    "app", "sessions", "session", "composer", "files", "git", "settings", "dialogs", "term",
    "search",
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

/// snapshot 的 cwd 字段：无项目态 = 空串（不指代任何真实目录）
fn cwd_json(chat: &Chat) -> String {
    chat.ws
        .cwd
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

fn surface_one(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>, name: &str) -> Value {
    match name {
        "app" => app_surface(chat, window, cx),
        "sessions" => sessions_surface(chat, cx),
        "session" => session_surface(chat, cx),
        "composer" => composer_surface(chat),
        "files" => files_surface(chat, cx),
        "git" => git_surface(chat),
        "settings" => settings_surface(chat, cx),
        "dialogs" => dialogs_surface(chat, cx),
        "term" => term_surface(chat, window, cx),
        "search" => search_surface(chat),
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
    if let Some(c) = &chat.composer.composer {
        if c.read(cx).focus_handle_in(cx).is_focused(window) {
            return json!("composer");
        }
    }
    if chat.layout.focus.is_focused(window) {
        return json!("chat");
    }
    if chat.dlg.dialog_focus.is_focused(window) {
        return json!("dialog");
    }
    if chat
        .ws
        .git_commit_input
        .read(cx)
        .focus_handle()
        .is_focused(window)
    {
        return json!("git_commit");
    }
    if chat
        .aux
        .ext_input
        .read(cx)
        .focus_handle()
        .is_focused(window)
    {
        return json!("ext_input");
    }
    for t in &chat.ws.terminals {
        if t.focus.is_focused(window) {
            return json!(format!("terminal:{}", t.id));
        }
    }
    // 文件编辑器（input.focus editor 的对向探针）：此前焦点落在编辑器报
    // "other"，而 other 被文档定义为「不该发生时的线索」——焦点类断言反被误导
    for tab in &chat.ws.panel_tabs {
        if let crate::PanelTab::File(p) = tab {
            if let Some(ed) = chat.ws.file_cache.get(p).and_then(|f| f.editor.as_ref()) {
                if ed.read(cx).focus_handle(cx).is_focused(window) {
                    return json!("editor");
                }
            }
        }
    }
    json!("other")
}

/// 终端网格快照：每个 tab 的状态/焦点/可见网格前若干行文本（键入回显、
/// 提示符出现与否的断言载体）。rows 截前 6 行、cols 全宽；只读不写。
fn term_surface(chat: &Chat, window: &gpui::Window, _cx: &Context<Chat>) -> Value {
    let tabs = chat
        .ws
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
        "active_terminal": chat.ws.active_terminal,
        "content_view": content_view_str(chat.layout.content_view),
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
        "theme": crate::appearance::theme::theme_name(),
        "lang": crate::i18n::LANG_LABELS[crate::i18n::lang_ix()],
        "cwd": cwd_json(chat),
        "branch": chat.ws.branch,
        "booted": chat.pool.booted,
        "focused": focused_str(chat, window, cx),
        "window": {
            "width_px": (f32::from(viewport.width) * scale).round() as i64,
            "height_px": (f32::from(viewport.height) * scale).round() as i64,
            "scale_factor": scale,
        },
        "dock_panel": chat.layout.dock_panel.as_str(),
        "content_view": content_view_str(chat.layout.content_view),
        // 【微信通话】tab 关闭态（2026-10-25 用户定夺：关闭跨重启保持）——
        // 启动回放/关 tab 落盘的断言载体（pif-ui wait --path app.wx_tab_closed）
        "wx_tab_closed": !chat.wx_tab_open(),
        // 微信会话归属（排障）：工作目录 / 会话指针 / 会话运行时 cwd
        "wx_cwd": chat.aux.remote.wx_cwd().map(|p| p.to_string_lossy().to_string()),
        "wx_line_key": chat.aux.remote.line_key().map(str::to_string),
        "wx_line_cwd": chat
            .aux
            .remote
            .line_key()
            .and_then(|k| chat.pool.runtimes.get(k))
            .map(|rt| rt.read(cx).cwd.to_string_lossy().to_string()),
        "wx_running": chat.aux.remote.is_running(),
        "active_key": chat.pool.active_key,
        "active_file": opt_path(chat.psp.active_file.as_ref()),
        "terminals": chat.ws.terminals.len(),
        "active_terminal": chat.ws.active_terminal,
        // 031/040 full+ 档扩展面板：按钮可用性（full+ 档即可，勾选即生效）
        "plugin_menu": {
            "available": chat
                .pool.runtimes
                .get(&chat.pool.active_key)
                .map(|rt| rt.read(cx).tool_preset_key() == "custom")
                .unwrap_or(false),
        },
        // 快照键沿用 pi-web 契约名 plugin_picker（Rust 标识符已改 pi_extension_picker）
        "plugin_picker": chat.dlg.pi_extension_picker.as_ref().map(|p| {
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
                    .cfg.mc_pkgs_global
                    .iter()
                    .chain(chat.cfg.mc_pkgs_project.iter())
                    .map(pi_link::skills::entry_source)
                    .filter(|s| !s.is_empty() && sel(s))
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                "project_unselected": count(&chat.cfg.mc_pkgs_project, false),
                "global_unselected": count(&chat.cfg.mc_pkgs_global, false),
            })
        }),
        // 瞬时提示（set_status）：运行中拦截 / 对话中途禁改 等断言用
        "status_toast": chat.dlg.status_toast.as_ref().map(|(m, _)| m.clone()),
        "pill_menu": chat.dlg.pill_menu.map(|m| match m {
            crate::PillMenu::Thinking => "thinking",
            crate::PillMenu::Tools => "tools",
        }),
        // 斜杠/@ 补全菜单（031）
        "composer_menu": composer_menu,
        "pill_anchor": chat.dlg.pill_anchor.map(|a| json!({
            "center_x": f32::from(a.center_x),
            "top": f32::from(a.top),
        })),
        // 081 自动更新（新会话页版本号位状态断言）
        "update": {
            "state": match &chat.aux.update {
                crate::services::updater::UpdateState::Idle => "idle",
                crate::services::updater::UpdateState::Downloading { .. } => "downloading",
                crate::services::updater::UpdateState::Ready { .. } => "ready",
                crate::services::updater::UpdateState::Available { .. } => "available",
            },
            "target": match &chat.aux.update {
                crate::services::updater::UpdateState::Downloading { version }
                | crate::services::updater::UpdateState::Ready { version }
                | crate::services::updater::UpdateState::Available { version } => Some(version.clone()),
                crate::services::updater::UpdateState::Idle => None,
            },
            "progress": if matches!(
                &chat.aux.update,
                crate::services::updater::UpdateState::Downloading { .. }
            ) {
                // 按相位取对应通道：解压段读解压百分比，否则下载百分比
                if crate::services::updater::update_phase()
                    == crate::services::updater::PHASE_EXTRACT
                {
                    crate::services::updater::extract_percent()
                } else {
                    crate::services::updater::download_percent()
                }
            } else {
                None
            },
            "phase": if matches!(
                &chat.aux.update,
                crate::services::updater::UpdateState::Downloading { .. }
            ) {
                match crate::services::updater::update_phase() {
                    crate::services::updater::PHASE_DOWNLOAD => "download",
                    crate::services::updater::PHASE_EXTRACT => "extract",
                    _ => "idle",
                }
            } else {
                "idle"
            },
            "auto_update": crate::services::workspace::auto_update(),
            "changelog_open": chat.aux.changelog.as_ref().map(|p| p.version.clone()),
            "changelog_error": chat.aux.changelog.as_ref().is_some_and(|p| p.fetch_failed),
        },
    })
}

fn sessions_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    let sessions: Vec<Value> = chat
        .pool
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
                "unread": chat.psp.unread.contains(&s.path),
                "pinned": chat
                    .psp
                    .session_pins
                    .iter()
                    .any(|p| crate::services::workspace::same_path(p, &s.path)),
                "loaded": chat.pool.runtimes.contains_key(&s.path.display().to_string()),
            })
        })
        .collect();
    let projects: Vec<Value> = chat
        .psp.projects
        .iter()
        .map(|g| json!({"name": g.name, "path": g.path.display().to_string(), "sessions": g.sessions.len()}))
        .collect();
    // 伪标签（draft 已发送、pi 未落盘）：验收断言「发送瞬间标签出现」用
    let pending_tabs: Vec<Value> = chat
        .psp
        .pending_tabs
        .iter()
        .map(|e| json!({"key": e.key, "preview": truncate(&e.preview, 120), "cwd": e.cwd}))
        .collect();
    // 会话置顶（020）：有序表（index 0 = 最近置顶），pin/unpin 事务断言用
    let session_pins: Vec<String> = chat
        .psp
        .session_pins
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    // 内存探针：各缓存（条数, 字节）+ 队列 + 活跃会话 transcript 规模——
    // 长跑内存回归验收用（退役/驱逐前后对照）
    let (active_msgs, active_sys_msgs) = chat
        .pool
        .runtimes
        .get(&chat.pool.active_key)
        .map(|rt| rt.read(cx).mem_probe())
        .unwrap_or((0, 0));
    let (img_n, img_bytes) = crate::session::messages::image_cache_probe();
    let (md_n, md_bytes) = crate::editor::markdown::md_parse_probe();
    let (hl_n, hl_bytes) = crate::editor::markdown::hl_cache_probe();
    let (limg_n, limg_bytes) = crate::editor::markdown::local_img_probe();
    let (args_n, args_bytes) = crate::session::messages::tool_args_cache_probe();
    let (mm_n, mm_bytes) = crate::editor::markdown::render::mermaid::mermaid_probe();
    let (math_n, math_bytes) = crate::editor::markdown::render::math::math_probe();
    let mem = json!({
        "runtimes": chat.pool.runtimes.len(),
        "active_msgs": active_msgs,
        "active_system_msgs": active_sys_msgs,
        "asset_evict_pending": crate::services::asset_evict::pending_len(),
        "img_cache": {"len": img_n, "bytes": img_bytes},
        "md_parse": {"len": md_n, "bytes": md_bytes},
        "hl_cache": {"len": hl_n, "bytes": hl_bytes},
        "local_img": {"len": limg_n, "bytes": limg_bytes},
        "tool_args": {"len": args_n, "bytes": args_bytes},
        "mermaid": {"len": mm_n, "bytes": mm_bytes},
        "math": {"len": math_n, "bytes": math_bytes},
    });
    json!({
        "cwd": cwd_json(chat),
        "active_key": chat.pool.active_key,
        "active_file": opt_path(chat.psp.active_file.as_ref()),
        "runtimes": chat.pool.runtimes.keys().cloned().collect::<Vec<_>>(),
        "sessions": sessions,
        "projects": projects,
        "pending_tabs": pending_tabs,
        "session_pins": session_pins,
        "mem": mem,
    })
}

/// 活跃会话的运行态（agent 是否在跑、流式状态、消息尾部、模型）。
fn session_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
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
        "text": truncate(&chat.composer.input, 2000),
        "pending_images": chat.composer.pending_images.len(),
        "history_len": chat.composer.history.len(),
        "composer_open": chat.composer.composer.is_some(),
    })
}

/// 083 项目搜索面：开关/计数/阶段 + 结果块预览（前 6 块、每块前 12 行）。
pub(crate) fn search_surface(chat: &Chat) -> Value {
    let ui = &chat.ws.search;
    let blocks: Vec<Value> = ui
        .blocks
        .iter()
        .take(6)
        .map(|block| {
            let lines: Vec<Value> = block
                .segments
                .iter()
                .flatten()
                .take(12)
                .map(|line| {
                    json!({
                        "line": line.line,
                        "hit": line.hit,
                        "ordinal": line.ordinal,
                        // 正文截断到 200 字符（快照只做断言，不搬运整段代码）
                        "text": line.text.chars().take(200).collect::<String>(),
                    })
                })
                .collect();
            json!({
                "path": block.rel,
                "path_only": block.path_only,
                "count": block.count,
                "segments": block.segments.len(),
                "lines": lines,
            })
        })
        .collect();
    json!({
        "shown": chat.layout.content_view == ContentView::Search,
        "query": ui.query_text,
        "replace": ui.replace_text,
        "phase": match ui.phase {
            crate::search::Phase::Idle => "idle",
            crate::search::Phase::Running => "running",
            crate::search::Phase::Done => "done",
        },
        "opts": crate::search::opts_labels(ui.opts),
        "show_replace": ui.show_replace,
        "show_filters": ui.show_filters,
        "files": ui.files,
        "path_files": ui.path_files,
        "matches": ui.matches,
        "scanned": ui.scanned,
        "active": ui.active,
        "limit": ui.limit,
        "error": ui.error,
        "notice": ui.notice.as_ref().map(|(m, bad)| json!({"text": m, "failed": bad})),
        "block_count": ui.blocks.len(),
        "line_h": crate::search::LINE_H,
        "header_h": crate::search::HEADER_H,
        // 结果区可滚动距离（>0 才挂滚动条；UI 测试断言滚动条前提）
        "scroll_max": f32::from(ui.list.max_offset_for_scrollbar().height),
        "blocks": blocks,
    })
}

fn files_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    // topbar 标签条滚动位（016）：句柄原值 + 激活 tab 的视口坐标
    let tabs_off = chat.layout.tabs_scroll.offset();
    let tabs_max = chat.layout.tabs_scroll.max_offset();
    let tabs_vp = chat.layout.tabs_scroll.bounds();
    let active_tab_ix = (chat.layout.content_view != ContentView::Chat)
        .then_some(chat.ws.active_panel_tab)
        .flatten();
    let active_tab_bounds =
        active_tab_ix.and_then(|ix| chat.layout.tabs_scroll.bounds_for_item(ix));
    let rows: Vec<Value> = chat
        .ws
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
    let mut expanded: Vec<String> = chat
        .ws
        .expanded_dirs
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    expanded.sort();
    // 021 行内编辑（Zed EditState 移植）状态：新建/改名行开合、键入值与
    // 校验态——行内编辑的事务断言全靠这条
    let tree_edit = chat.dlg.tree_edit.as_ref().map(|e| {
        let (kind, subject) = match &e.kind {
            crate::function_panel::TreeEditKind::New { dir } => ("new", dir.display().to_string()),
            crate::function_panel::TreeEditKind::Rename { path } => {
                ("rename", path.display().to_string())
            }
        };
        json!({
            "kind": kind,
            "subject": subject,
            "is_dir": e.is_dir,
            "value": e.input.read(cx).value(),
            "validation": match &e.validation {
                crate::function_panel::TreeValidation::None => "none",
                crate::function_panel::TreeValidation::Error(_) => "error",
                crate::function_panel::TreeValidation::Warning(_) => "warning",
            },
        })
    });
    // 文件 tab 明细（023）：编辑器状态事务断言用（dirty/conflict/预览态）
    let active_file = chat.active_file_path();
    // 面包屑分段文本（023 导航栏）：与渲染共用 breadcrumb_segments（防漂移），
    // 盘符/根合成一段的回归就靠这条断言
    let crumbs: Vec<String> = active_file
        .as_ref()
        .map(|p| {
            // 无项目态：无工作区相对化基準，直接用绝对路径段
            crate::editor::view::breadcrumb_segments(
                chat.ws.cwd.as_deref().unwrap_or_else(|| Path::new("")),
                p,
            )
            .iter()
            .map(|(t, _)| t.to_string())
            .collect()
        })
        .unwrap_or_default();
    let file_tabs: Vec<Value> = chat
        .ws.panel_tabs
        .iter()
        .filter_map(|t| match t {
            crate::PanelTab::File(p) => {
                let f = chat.ws.file_cache.get(p);
                Some(json!({
                    "file": p.display().to_string(),
                    "active": active_file.as_ref().is_some_and(|a| crate::services::workspace::same_path(a, p)),
                    "dirty": f.map(|f| f.dirty).unwrap_or(false),
                    "conflict": f.and_then(|f| f.conflict.as_ref()).map(|c| format!("{c:?}")),
                    "md_source": f.map(|f| f.md_source).unwrap_or(false),
                    "has_editor": f.map(|f| f.editor.is_some()).unwrap_or(false),
                    // 二进制占位 tab（无法打开灰屏）：has_editor 恒 false 的
                    // 区分位，pif-ui 断言「ttf 开出灰屏 tab」用
                    "binary": f.map(|f| f.binary).unwrap_or(false),
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
        "cwd": cwd_json(chat),
        "crumbs": crumbs,
        // 023 外部改动检测的监听面：cwd 递归 watch + cwd 外打开文件的
        // 单文件目录 watch（Zed single-file worktree 同款）——断言「工作区外
        // 文件也有监听」就靠这条
        "watch_roots": chat
            .ws.file_watches
            .keys()
            .map(|p| p.display().to_string())
            .collect::<Vec<String>>(),
        "expanded": expanded,
        "rows": rows,
        "tree_edit": tree_edit,
        "file_tabs": file_tabs,
        "ext_probe": {"runs": chat.ws.ext_probe.0, "hits": chat.ws.ext_probe.1},
        "open_tabs": chat.ws.panel_tabs.iter().map(|t| match t {
            crate::PanelTab::Term(i) => json!({"term": i}),
            crate::PanelTab::File(p) => json!({"file": p.display().to_string()}),
            crate::PanelTab::Changelog => json!({"changelog": true}),
            crate::PanelTab::PiExtMarket => json!({"pi_ext_market": true}),
            crate::PanelTab::PiNativeMarket => json!({"pi_native_market": true}),
            crate::PanelTab::PfMarket => json!({"pf_market": true}),
            crate::PanelTab::Search => json!({"search": true}),
            crate::PanelTab::SessionSearch => json!({"session_search": true}),
            crate::PanelTab::Wx => json!({"wx": true}),
            crate::PanelTab::Plugin { pkg, page } => {
                json!({"plugin_tab": {"pkg": pkg, "page": page}})
            }
        }).collect::<Vec<_>>(),
        // 文件树标题项目下拉（树根跨项目快切）：开合/键盘高亮/项目清单，
        // files.project_menu + input.keys 的事务断言靠这条
        "proj_menu": chat.dlg.tree_proj_menu.as_ref().map(|m| json!({
            "sel": m.sel,
            "items": chat.psp.projects.iter().map(|g| json!({
                "name": g.name,
                "path": g.path.display().to_string(),
            })).collect::<Vec<_>>(),
        })),
        // 021 Pin 收藏夹：存储清单（绝对路径）+ 视图开合（rows 即视图行，
        // depth 0 = 收藏根行），files.pin / files.pin_view 的事务断言靠这两条
        "pins": chat.ws.pinned.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "pin_view": chat.ws.pin_view,
        // 016 topbar 标签条滚动位（zed `tab_bar_scroll_handle`）：Ctrl+←/→
        // 跟随激活 tab 的回归断言面。offset_x 为 gpui 原值（负向，0 =
        // 最左）；active_x0/x1 已换算成**视口坐标**（0..viewport_w 内 =
        // 完整可见），active = null 表示会话视图（固定段不参与滚动）
        "tabs_scroll": {
            "offset_x": f32::from(tabs_off.x),
            "max_x": f32::from(tabs_max.width),
            "viewport_w": f32::from(tabs_vp.size.width),
            "active_ix": active_tab_ix,
            "active_x0": active_tab_bounds.map(|b| f32::from(b.left() + tabs_off.x - tabs_vp.left())),
            "active_x1": active_tab_bounds.map(|b| f32::from(b.right() + tabs_off.x - tabs_vp.left())),
        },
    })
}

fn git_surface(chat: &Chat) -> Value {
    json!({
        "branch": chat.ws.branch,
        "tab": format!("{:?}", chat.ws.git_tab),
        "error": chat.ws.git_error,
        "add_del": {"add": chat.ws.git_add_del.0, "del": chat.ws.git_add_del.1},
        "files": chat.ws.git_files.iter().map(|f| json!({
            "path": f.path.display().to_string(),
            "status": format!("{:?}", f.status),
            "staged": f.staged,
        })).collect::<Vec<_>>(),
    })
}

fn settings_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    match &chat.dlg.settings {
        None => json!({"open": false}),
        Some(panel) => {
            let p = panel.read(cx);
            json!({
                "open": true,
                "tab": p.tab,
                "section": p.section,
                "error": p.error,
                // 040 扩展页：op 进行中 + 当前列表（安装后刷新断言用）
                "busy": chat.cfg.pkg_op.is_some(),
                "pkg_sources": chat
                    .cfg.mc_pkgs_global
                    .iter()
                    .map(pi_link::skills::entry_source)
                    .collect::<Vec<String>>(),
                "mc_state": format!("{:?}", chat.cfg.mc_state),
                "mc_project_scope": chat.cfg.mc_project_scope,
                // 技能页：列表 + 休眠态（开关断言用）
                "skills": chat
                    .cfg
                    .mc_skills
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "scope": format!("{:?}", s.scope),
                            "disabled": s.disable_invocation,
                            "path": s.path.to_string_lossy(),
                        })
                    })
                    .collect::<Vec<_>>(),
                // 043 MCP 页：列表 + 表单字段快照（列表断言 / 保存回读用）
                "mcp_servers": chat
                    .cfg.mcp_servers
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
    let dialog = chat.dlg.dialog.as_ref().map(|d| match d {
        Dialog::ProviderPicker { .. } => "provider_picker",
        Dialog::GitDiff { .. } => "git_diff",
        Dialog::ProjectPicker { .. } => "project_picker",
        Dialog::ImagePreview { .. } => "image_preview",
        Dialog::SessionInfo { .. } => "session_info",
        Dialog::FileDirty { .. } => "file_dirty",
        Dialog::WxQr => "wx_qr",
        Dialog::WxNewSession => "wx_new_session",
    });
    // 打开项目菜单：列表数据随弹窗一起上报（扫描是异步回填，UI 测试
    // 据此轮询就绪）
    let project_hits: Vec<Value> = match chat.dlg.dialog.as_ref() {
        Some(Dialog::ProjectPicker { .. }) => chat
            .psp
            .project_hits
            .iter()
            .map(|p| json!({"name": p.name, "path": p.path.display().to_string()}))
            .collect(),
        _ => Vec::new(),
    };
    // 文件搜索命中随弹窗上报（后台扫盘异步回填，UI 测试据此轮询就绪）
    json!({
        "dialog": dialog,
        // 模型菜单可见行数（UI 测试断言用：空列表 = 新会话页拉不到目录；
        // 042 起数据源 = pill 化模型菜单的 picker_models）
        "model_rows": match chat.dlg.model_picker.as_ref() {
            Some(_) => chat.picker_models(cx).0.len(),
            None => 0,
        },
        "project_hits": project_hits,
        "project_filter": chat.psp.project_filter,
        // 键盘选中行（0 = 打开文件夹，1.. = 项目行；默认选中断言 + ↑/↓
        // 按键导航断言的数据化出口）
        "project_sel": chat.psp.project_sel,
        "confirm_delete": opt_path(chat.dlg.confirm_delete.as_ref()),
        // 021 文件树删除确认浮层（Some=开着，值为待删路径；file.delete /
        // file.delete_confirm 两段式 op 断言开合用）+ trash 失败持久弹层
        "tree_confirm_del": opt_path(chat.dlg.tree_confirm_del.as_ref()),
        "tree_del_error": chat.dlg.tree_del_error.clone(),
        // 删除在飞清单长度（trash 已后台化：op 断言「删除完成」可轮询归零）
        "tree_deleting": chat.dlg.tree_deleting.len(),
        // 040 扩展页卸载确认浮层（Some=开着，值为待卸载来源）
        "pkg_confirm_remove": chat.cfg.pkg_confirm_remove.clone(),
        "renaming": opt_path(chat.dlg.renaming.as_ref()),
        // 编辑区右键菜单（file.context_menu 置槽；null=关）——开合/收起
        // 断言的数据化出口（此前只能开，断言靠 shot 像素）
        "editor_menu": chat.dlg.editor_menu.as_ref().map(|(p, x, y)| json!({
            "path": p.display().to_string(),
            "x": x,
            "y": y,
        })),
        // 031 输入面板右键菜单（input.context_menu 置槽；null=关）
        "input_menu": chat.dlg.input_menu.map(|(x, y)| json!({
            "x": x,
            "y": y,
        })),
        // 消息内容区右键菜单（null=关）
        "msg_menu": chat.dlg.msg_menu.map(|(x, y)| json!({
            "x": x,
            "y": y,
        })),
        "ext_dialog": chat.aux.ext_dialog.is_some(),
        "toast": chat.dlg.status_toast.as_ref().map(|(t, _)| truncate(t, 200)),
        "settings_open": chat.dlg.settings.is_some(),
        // 060 微信 Bot：扫码弹窗的实时状态（pif-ui 可据此断言 UI 状态）
        "wx_qr": match &chat.aux.remote.qr {
            crate::remote_control::QrState::Idle => "idle",
            crate::remote_control::QrState::Loading => "loading",
            crate::remote_control::QrState::Ready { .. } => "ready",
            crate::remote_control::QrState::Scanned { .. } => "scanned",
            crate::remote_control::QrState::Done { .. } => "done",
            crate::remote_control::QrState::Expired { .. } => "expired",
            crate::remote_control::QrState::Error { .. } => "error",
        },
    })
}

fn content_view_str(v: crate::ContentView) -> &'static str {
    match v {
        crate::ContentView::Chat => "chat",
        crate::ContentView::Term => "term",
        crate::ContentView::File => "file",
        crate::ContentView::Changelog => "changelog",
        crate::ContentView::PiExtMarket => "pi-ext-market",
        crate::ContentView::PiNativeMarket => "pi-native-market",
        crate::ContentView::PfMarket => "pf-market",
        crate::ContentView::Search => "search",
        crate::ContentView::SessionSearch => "session_search",
        crate::ContentView::Wx => "wx",
        crate::ContentView::Plugin => "plugin",
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
