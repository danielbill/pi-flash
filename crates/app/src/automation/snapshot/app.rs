//! app/term 面：窗口级状态、终端网格与斜杠菜单/更新/插件弹窗等全局探测。

use gpui::{Context, Focusable};
use serde_json::{Value, json};

use super::{Chat, content_view_str, cwd_json, opt_path};

/// 当前焦点落在谁身上（焦点类 bug 的数据化探针）：与已知句柄逐一比对。
/// "other" = 焦点在未登记的输入上——正是不该发生时的线索。
pub(super) fn focused_str(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>) -> Value {
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
pub(super) fn term_surface(chat: &Chat, window: &gpui::Window, _cx: &Context<Chat>) -> Value {
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

pub(super) fn app_surface(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>) -> Value {
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
            // 已应用的 OS 窗口标题（zi2s.9 ctx.ui.setTitle 台架断言出口；
            // None 时与启动标题 "pi-flash" 一致）
            "title": if chat.aux.ext_title.is_some() {
                chat.aux.ext_title_applied.clone()
            } else {
                "pi-flash".to_string()
            },
        },
        "dock_panel": chat.layout.dock_panel.as_str(),
        // dock_panel 快照串区分不了 Plugin(ix)（as_str 恒 "sessions"，旧键
        // 名兼容）：kind + ix 是 pif-ui 断言插件面板态的载体（030-流式卡死
        // 台架 first user）
        "dock_panel_kind": match chat.layout.dock_panel {
            crate::DockPanel::Default => "sessions",
            crate::DockPanel::Git => "git",
            crate::DockPanel::Plugin(_) => "plugin",
        },
        "plugin_panel_ix": match chat.layout.dock_panel {
            crate::DockPanel::Plugin(ix) => json!(ix),
            _ => Value::Null,
        },
        "content_view": content_view_str(chat.layout.content_view),
        // 契约 v3 插件弹窗（pif-ui 断言载体：open/插件/内容树）
        "plugin_popup": match chat.plugins.popup.as_ref() {
            Some(p) => json!({
                "open": true,
                "ix": p.ix,
                "id": p.id,
                "title": p.title,
                "body": p.body.clone().unwrap_or(Value::Null),
            }),
            None => json!({ "open": false }),
        },
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
        // 090-10 迁移体验：台账启用却被本版 PF 拒载的失效包（市场常驻提示数据源）。
        // 只读；L3 断言「旧 schema 包启动后进提示、更新后消失」
        "pf_market_stale": chat.aux.market.pf.stale.iter().map(|s| json!({
            "name": s.name,
            "version": s.version,
            "title": s.title_zh,
            "kind": match &s.kind {
                crate::pf_plugin::StaleKind::SchemaUnsupported { got } => format!("schema:{got}"),
                crate::pf_plugin::StaleKind::Invalid(_) => "invalid".to_string(),
            },
        })).collect::<Vec<_>>(),
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
