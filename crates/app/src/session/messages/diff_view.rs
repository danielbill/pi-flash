//! 写文件工具与 diff 三视图（032）：write/edit 工具识别与路径提取、
//! turn_written_files 聚合、split_files_view / diff_cell / patch_text_view。
//! 自 cards.rs 拆出（090 重构）。

use super::*;

use gpui::{FontWeight, SharedString, TextAlign, div, prelude::*, px, relative, rgb, rgba};
use pi_link::protocol::Block;

use super::tool_card::{ARGS_CACHE_BUDGET_BYTES, READ_PATH_CACHE};
use crate::appearance::theme;
use crate::i18n::tr;

/// pi-web isWriteToolName（tool-names.ts）。
fn is_write_tool_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == "write" || n.starts_with("write_") || n.ends_with(".write") || n.ends_with("_write")
}

fn is_file_writing_tool(name: &str) -> bool {
    is_write_tool_name(name)
        || crate::session::diff::is_edit_tool_name(name)
        || crate::session::diff::is_apply_patch_tool_name(name)
}

fn read_tool_path(args: &str) -> Option<String> {
    // 带线程局部 LRU（同 tool_preview 的理由）：turn_written_files 每帧
    // 扫整轮文件写入工具，write/edit 的 args 内嵌整个文件内容
    const CAP: usize = 256;
    let key = (crate::editor::markdown::hash_str(args), args.len());
    READ_PATH_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0.as_str() == args {
                let out = entry.1.1.clone();
                cache.push_back(entry);
                return out;
            }
        }
        let v: serde_json::Value = serde_json::from_str(args).ok()?;
        let out = v
            .get("file_path")
            .or_else(|| v.get("path"))
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        cache.push_back((key, (args.to_string(), out.clone())));
        while cache.len() > CAP
            || cache.iter().map(|(k, _)| k.1).sum::<usize>() > ARGS_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        out
    })
}

/// 探针（automation snapshot）：两个 args 缓存合计（条数，源文字节）
pub(crate) fn tool_args_cache_probe() -> (usize, usize) {
    let preview = PREVIEW_CACHE.with(|cell| {
        let cache = cell.borrow();
        (cache.len(), cache.iter().map(|(k, _)| k.1).sum::<usize>())
    });
    let read_path = READ_PATH_CACHE.with(|cell| {
        let cache = cell.borrow();
        (cache.len(), cache.iter().map(|(k, _)| k.1).sum::<usize>())
    });
    (preview.0 + read_path.0, preview.1 + read_path.1)
}

/// pi-web readApplyPatchPaths parity：appliedFiles > preview > 输入解析，
/// 剔除 delete，failures 无 applied 时写零文件。
fn apply_patch_written_paths(args: &str, details: Option<&serde_json::Value>) -> Vec<String> {
    let delete_paths: Vec<String> = args
        .lines()
        .filter_map(|l| l.strip_prefix("*** Delete File: "))
        .map(|p| p.trim().to_string())
        .collect();
    let has_failures = details
        .and_then(|d| d.get("result"))
        .and_then(|r| r.get("failures"))
        .and_then(|f| f.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if let Some(applied) = details
        .and_then(|d| d.get("result"))
        .and_then(|r| r.get("appliedFiles"))
        .and_then(|a| a.as_array())
    {
        let applied: Vec<String> = applied
            .iter()
            .filter_map(|x| x.as_str())
            .filter(|p| !p.is_empty() && !delete_paths.contains(&p.to_string()))
            .map(str::to_string)
            .collect();
        return applied;
    }
    if has_failures {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if let Some(d) = details {
        if let Some(files) = crate::session::diff::apply_patch_preview_to_files(d) {
            for f in files {
                if let Some(p) = f.new_path {
                    paths.push(p);
                }
            }
        }
    }
    if paths.is_empty() {
        if let Some(files) = crate::session::diff::parse_apply_patch_input(args) {
            for f in files {
                if let Some(p) = f.new_path {
                    paths.push(p);
                }
            }
        }
    }
    paths
}

/// pi-web extractTurnWrittenFiles parity：本轮实际写过的文件（工具调用
/// 为准，去重保序，相对路径按 cwd 解析）。
pub(super) fn turn_written_files(turn: &[&Msg]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let cwd = std::env::current_dir().ok();
    let push =
        |raw: String, seen: &mut std::collections::HashSet<String>, out: &mut Vec<String>| {
            let path = std::path::Path::new(&raw);
            let resolved = if path.is_absolute() {
                raw.clone()
            } else {
                match &cwd {
                    Some(c) => c.join(raw).to_string_lossy().to_string(),
                    None => return,
                }
            };
            if seen.insert(resolved.clone()) {
                out.push(resolved);
            }
        };
    for m in turn {
        for b in &m.blocks {
            let Block::ToolCall {
                name,
                args,
                is_error,
                result_arrived,
                details,
                ..
            } = b
            else {
                continue;
            };
            if *is_error || !*result_arrived || !is_file_writing_tool(name) {
                continue;
            }
            if crate::session::diff::is_apply_patch_tool_name(name) {
                for p in apply_patch_written_paths(args, details.as_ref()) {
                    push(p, &mut seen, &mut out);
                }
            } else if let Some(p) = read_tool_path(args) {
                push(p, &mut seen, &mut out);
            }
        }
    }
    out
}

/// pi-web SplitFilesView：双栏 split diff，maxHeight 560 滚动；多文件时
/// 显示文件头（pi-web sticky 头在 gpui 无对应，退化为普通行）。
pub(crate) fn split_files_view(
    files: &[crate::session::diff::DiffFile],
    t: &theme::Theme,
) -> gpui::AnyElement {
    use crate::session::diff::Row;
    let show_headers = files.len() > 1;
    let mut wrap = div()
        .id(SharedString::from(SharedString::from(format!(
            "sd-{}",
            files.len()
        ))))
        .max_h(px(560.))
        .overflow_y_scroll()
        .min_w_0()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::sess_size(-2.))
        .line_height(relative(1.55));
    for (fix, file) in files.iter().enumerate() {
        let mut fcol = div().min_w_0();
        if fix > 0 {
            fcol = fcol.border_t_1().border_color(rgb(t.border));
        }
        if show_headers {
            let header = |title: Option<&String>, left: bool| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(10.))
                    .py(px(5.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(rgb(t.text_dim))
                    .when(left, |d| d.border_r_1().border_color(rgb(t.border)))
                    .child(SharedString::from(
                        title
                            .map(|s| s.clone())
                            .unwrap_or_else(|| tr("messages.before").to_string()),
                    ))
                    .into_any_element()
            };
            // 右栏标题：newPath 缺省显示「之后」
            let right_title = match &file.new_path {
                Some(p) => Some(p.clone()),
                None => None,
            };
            fcol = fcol.child(
                div()
                    .flex()
                    .min_w_0()
                    .bg(rgb(t.bg_panel))
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .child(header(file.old_path.as_ref(), true))
                    .child(header(right_title.as_ref(), false)),
            );
        }
        for row in &file.rows {
            match row {
                Row::Hunk(_) => {}
                Row::Line { left, right } => {
                    fcol = fcol
                        .child(diff_cell(left, true, t))
                        .child(diff_cell(right, false, t));
                }
            }
        }
        wrap = wrap.child(fcol);
    }
    wrap.into_any_element()
}

/// pi-web SplitDiffCellView：行号槽 42px + 标记列 18px + 文本。
fn diff_cell(
    cell: &crate::session::diff::Cell,
    left_side: bool,
    t: &theme::Theme,
) -> gpui::AnyElement {
    use crate::session::diff::CellKind;
    let bg = match cell.kind {
        CellKind::Added => gpui::rgba(theme::rgba_a(0x22c55e, 0.12)),
        CellKind::Removed => gpui::rgba(theme::rgba_a(0xf87171, 0.13)),
        CellKind::Empty => rgba(t.bg_subtle),
        CellKind::Context => gpui::rgba(0),
    };
    let marker = match cell.kind {
        CellKind::Added => "+",
        CellKind::Removed => "-",
        _ => " ",
    };
    let marker_color = match cell.kind {
        CellKind::Added => 0x22c55e,
        CellKind::Removed => 0xf87171,
        _ => t.text_dim,
    };
    let line_no = cell.line_no.map(|n| n.to_string()).unwrap_or_default();
    div()
        .flex()
        .min_w_0()
        .bg(bg)
        .when(left_side, |d| d.border_r_1().border_color(rgb(t.border)))
        .child(
            div()
                .w(px(42.))
                .px(px(6.))
                .flex_shrink_0()
                .text_align(TextAlign::Right)
                .text_color(rgb(t.text_dim))
                .bg(rgb(t.bg_panel))
                .border_r_1()
                .border_color(rgb(t.border))
                .child(SharedString::from(line_no)),
        )
        .child(
            div()
                .w(px(18.))
                .px(px(5.))
                .flex_shrink_0()
                .text_color(rgb(marker_color))
                .when(
                    matches!(cell.kind, CellKind::Added | CellKind::Removed),
                    |d| d.font_weight(FontWeight::BOLD),
                )
                .child(SharedString::from(marker)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(10.))
                .text_color(rgb(if matches!(cell.kind, CellKind::Empty) {
                    t.text_dim
                } else {
                    t.text
                }))
                .child(SharedString::from(if cell.text.is_empty() {
                    "\u{a0}".to_string()
                } else {
                    cell.text.clone()
                })),
        )
        .into_any_element()
}

/// pi-web PatchTextView：单栏回退（unified 解析失败时），maxHeight 520。
pub(crate) fn patch_text_view(text: &str, t: &theme::Theme) -> gpui::AnyElement {
    div()
        .id("ptext")
        .max_h(px(520.))
        .overflow_y_scroll()
        .min_w_0()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::sess_size(-2.))
        .line_height(relative(1.55))
        .children(text.lines().enumerate().map(|(i, line)| {
            let kind = if line.starts_with("@@") {
                0
            } else if line.starts_with('+') && !line.starts_with("+++") {
                1
            } else if line.starts_with('-') && !line.starts_with("---") {
                2
            } else {
                3
            };
            let (bg, color, bar) = match kind {
                0 => (
                    gpui::rgba(theme::rgba_a(0x60a5fa, 0.12)),
                    t.accent,
                    t.accent,
                ),
                1 => (
                    gpui::rgba(theme::rgba_a(0x22c55e, 0.12)),
                    0x22c55e,
                    0x22c55e,
                ),
                2 => (
                    gpui::rgba(theme::rgba_a(0xf87171, 0.13)),
                    0xf87171,
                    0xf87171,
                ),
                _ => (gpui::rgba(0), t.text, 0),
            };
            div()
                .flex()
                .min_w_0()
                .bg(bg)
                .when(kind != 3, |d| d.border_l_3().border_color(rgb(bar)))
                .when(kind == 3, |d| d.border_l_3().border_color(gpui::rgba(0)))
                .child(
                    div()
                        .w(px(48.))
                        .px(px(8.))
                        .flex_shrink_0()
                        .text_align(TextAlign::Right)
                        .text_color(rgb(t.text_dim))
                        .bg(rgb(t.bg_panel))
                        .border_r_1()
                        .border_color(rgb(t.border))
                        .child(SharedString::from((i + 1).to_string())),
                )
                .child(div().px(px(10.)).min_w_0().text_color(rgb(color)).child(
                    SharedString::from(if line.is_empty() {
                        "\u{a0}".to_string()
                    } else {
                        line.to_string()
                    }),
                ))
        }))
        .into_any_element()
}
