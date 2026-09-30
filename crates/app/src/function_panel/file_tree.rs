//! Directory file tree rows (021 dirTreeView groundwork; FileExplorer.tsx
//! TreeNodeView parity). zed project_panel skeleton lands in phase D/E.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::services::git::GitStatus;
use crate::ui::icon;
use crate::theme;

/// Recursive file-explorer rows (FileExplorer.tsx TreeNodeView parity):
/// 24px rows, indent 8+depth*14, directories toggle lazily on click,
/// files open the preview dialog.
pub(crate) fn collect_tree_rows(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    git_map: &std::collections::HashMap<PathBuf, GitStatus>,
    changed_dirs: &HashSet<PathBuf>,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
    out: &mut Vec<gpui::AnyElement>,
) {
    // defensive depth cap: pathological trees must not kill the app
    // (Windows main stack; see .cargo/config.toml for the 16MB bump)
    if depth >= 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if e.file_type().map(|ty| ty.is_dir()).unwrap_or(false) {
            dirs.push(e.path());
        } else {
            files.push(e.path());
        }
    }
    dirs.sort();
    files.sort();
    dirs.truncate(300);
    files.truncate(300);
    for path in dirs.into_iter().chain(files.into_iter()) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let is_dir = path.is_dir();
        let open = is_dir && expanded.contains(&path);
        let mut row = div()
            .id(SharedString::from(format!("tree-{}", path.display())))
            .w_full()
            .h(px(24.))
            .flex()
            .items_center()
            .gap_1()
            .overflow_hidden()
            .pl(px(8. + depth as f32 * 14.))
            .pr(px(8.))
            .rounded(px(4.))
            .text_xs()
            .text_color(rgb(t.text))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)));
        if is_dir {
            row = row
                .child(
                    div()
                        .flex_shrink_0()
                        .child(if open {
                            icon("chevron-down", 10., t.text_dim)
                        } else {
                            icon("chevron-right", 10., t.text_dim)
                        }),
                )
                .child(
                    div().flex_shrink_0().child(icon("folder", 14., t.text_dim)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(name)),
                );
            let weak_toggle = weak.clone();
            let dir_path = path.clone();
            row = row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let d = dir_path.clone();
                let _ = weak_toggle.update(cx, |c, cx| {
                    if !c.expanded_dirs.remove(&d) {
                        c.expanded_dirs.insert(d);
                    }
                    cx.notify();
                });
            });
        } else {
            row = row
                .child(div().w(px(10.)).flex_shrink_0())
                .child(
                    div().flex_shrink_0().child(icon("file", 14., t.text_dim)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(name)),
                );
            let weak_open = weak.clone();
            let fp = path.clone();
            let is_changed = git_map.contains_key(&path);
            row = row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_open.update(cx, |c, cx| {
                    if is_changed {
                        c.open_git_diff(fp.clone(), cx);
                    } else {
                        c.open_file_tab(fp.clone(), cx);
                    }
                });
            });
        }
        // git badge on files; dot on directories containing changes
        if is_dir {
            if changed_dirs.contains(&path) {
                row = row.child(
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .ml_auto()
                        .flex_shrink_0()
                        .bg(rgb(0xd6a84b)),
                );
            }
        } else if let Some(st) = git_map.get(&path) {
            let color = st.color();
            row = row.child(
                div()
                    .ml_auto()
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(color))
                    .child(st.badge()),
            );
        }
        out.push(row.into_any_element());
        if open {
            collect_tree_rows(&path, depth + 1, expanded, git_map, changed_dirs, weak, t, out);
        }
    }
}
