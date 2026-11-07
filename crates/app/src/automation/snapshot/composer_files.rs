//! composer/files/git/search 面：输入框、文件树与编辑器 tab、git、项目搜索。

use crate::ContentView;
use gpui::Context;
use serde_json::{Value, json};
use std::path::Path;

use super::{Chat, cwd_json, truncate};

pub(super) fn composer_surface(chat: &Chat) -> Value {
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

pub(super) fn files_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
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
            crate::PanelTab::PiNativeMarket => json!({"pi_native_market": true}),
            crate::PanelTab::PfMarket => json!({"pf_market": true}),
            crate::PanelTab::Search => json!({"search": true}),
            crate::PanelTab::SessionSearch => json!({"session_search": true}),
            crate::PanelTab::Plugin { pkg, page } => {
                json!({"plugin_tab": {"pkg": pkg, "page": page}})
            }
            crate::PanelTab::Observer { key } => json!({"observer": key}),
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

pub(super) fn git_surface(chat: &Chat) -> Value {
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
