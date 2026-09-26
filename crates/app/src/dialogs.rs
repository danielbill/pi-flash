//! Modal dialogs: ModelSelect / BranchTree / ProjectSelect / GitDiff
//! (pi-web parity surfaces layered over the app root). Free function over
//! Chat state; entity split lands in phase E (ARCHITECTURE.md §2).

use std::path::PathBuf;

use gpui::{App, Div, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Dialog;
use crate::Chat;
use crate::i18n::tr;
use crate::services::branch::*;
use crate::services::workspace::same_ws;
use pi_link::sessions::list_sessions;
use pi_link::protocol::TreeNode;
use crate::theme;
use crate::theme::theme as T;
use crate::ui::icon;

pub(crate) fn render_dialogs(
    mut root: Div,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
    cx: &App,
) -> Div {
        // dialogs (bodies verbatim from the former inline section)
            if let Some(Dialog::ModelSelect { input: filter_input }) = chat.dialog.as_ref() {
                let flt = filter_input.read(cx).value().to_lowercase();
                // enabledModels whitelist narrows the picker (pi-web /api/models
                // resolveVisibleModels parity)
                let picker_enabled = !chat.mc_state.all_enabled;
                let rows: Vec<gpui::AnyElement> = chat
                    .available_models
                    .iter()
                    .filter(|m| {
                        if picker_enabled {
                            let r = format!("{}/{}", m.provider, m.id);
                            if !chat.mc_state.enabled.iter().any(|e| e == &r) {
                                return false;
                            }
                        }
                        flt.is_empty()
                            || m.id.to_lowercase().contains(&flt)
                            || m.name.to_lowercase().contains(&flt)
                            || m.provider.to_lowercase().contains(&flt)
                    })
                    .take(12)
                    .map(|m| {
                        let provider = m.provider.clone();
                        let id = m.id.clone();
                        let weak_row = weak.clone();
                        let label: SharedString =
                            format!("{} / {}", m.provider, m.label()).into();
                        let ctx: SharedString = m
                            .context_window
                            .map(|c| format!("{}k", c / 1000))
                            .unwrap_or_default()
                            .into();
                        div()
                            .id(SharedString::from(format!("model-{provider}-{id}")))
                            .w_full()
                            .px_3()
                            .py_1p5()
                            .cursor_pointer()
                            .rounded_md()
                            .hover(|s| s.bg(rgb(t.bg_selected)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let (p, mid) = (provider.clone(), id.clone());
                                let _ = weak_row.update(cx, |c, cx| c.select_model(p, mid, cx));
                            })
                            .flex()
                            .justify_between()
                            .child(div().text_xs().text_color(rgb(t.text)).child(label))
                            .child(div().text_xs().text_color(rgb(t.text_dim)).child(ctx))
                            .into_any_element()
                    })
                    .collect();
                let list_panel = if rows.is_empty() {
                    div()
                        .py_2()
                        .text_xs()
                        .text_color(rgb(t.text_dim))
                        .child("no models match")
                        .into_any_element()
                } else {
                    div().flex().flex_col().gap_0p5().children(rows).into_any_element()
                };
                root = root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.35))
                        .track_focus(&chat.dialog_focus)
                        .on_key_down({
                            let weak = weak.clone();
                            move |ev: &KeyDownEvent, _w, cx| {
                                if ev.keystroke.key == "escape" {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(520.))
                                .max_h(px(560.))
                                .bg(rgb(t.bg_panel))
                                .border_1()
                                .border_color(rgb(t.border))
                                .rounded_lg()
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .shadow_lg()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(rgb(t.text))
                                                .child("select model"),
                                        )
                                        .child(
                                            div()
                                                .id("model-close")
                                                .px_2()
                                                .cursor_pointer()
                                                .text_color(rgb(t.text_muted))
                                                .hover(|s| s.text_color(rgb(t.text)))
                                                .on_mouse_down(MouseButton::Left, {
                                                    let weak = weak.clone();
                                                    move |_, _, cx| {
                                                        let _ = weak.update(cx, |c, cx| {
                                                            c.dialog = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(icon("x", 12., t.text_muted)),
                                        ),
                                )
                                .child(filter_input.clone())
                                .child(list_panel),
                        ),
                );
            }
            if chat.dialog.as_ref().is_some_and(|d| matches!(d, Dialog::BranchTree)) {
                let t = T();
                let weak = weak.clone();
                let (has_session, tree, leaf_id) = match &chat.branch_tree {
                    Some((tree, leaf)) => (chat.agent.session.is_some(), tree.clone(), leaf.clone()),
                    None => (chat.agent.session.is_some(), Vec::new(), None),
                };
                let has_branches = tree_has_branches(&tree);
                let active_path = build_active_path(&tree, leaf_id.as_deref());
                let top_level = select_top_level_branches(&tree);

                // ── node rows (BranchNavigator TreeNodeView, token-level) ──
                fn push_node(
                    node: &TreeNode,
                    skipped: usize,
                    label: &str,
                    is_last: bool,
                    parent_lines: &[bool],
                    active_path: &std::collections::HashSet<String>,
                    weak: &gpui::WeakEntity<Chat>,
                    out: &mut Vec<gpui::AnyElement>,
                ) {
                    let t = T();
                    let is_on_path = active_path.contains(&node.id);
                    let is_active = is_on_path;
                    let role = node.role.clone().unwrap_or_default();
                    let mut row = div()
                        .w_full()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)));
                    // indent guide lines
                    for has_line in parent_lines {
                        row = row.child(
                            div()
                                .w(px(16.))
                                .h_full()
                                .border_l_1()
                                .border_color(if *has_line {
                                    rgb(t.border)
                                } else {
                                    gpui::rgba(0x00000000)
                                }),
                        );
                    }
                    // connector: vertical line + horizontal tick
                    let mut connector = div()
                        .w(px(16.))
                        .h_full()
                        .flex()
                        .items_center()
                        .border_l_1()
                        .border_color(rgb(t.border));
                    if !node.children.is_empty() || skipped > 0 {
                        connector = connector.child(
                            div()
                                .w(px(9.))
                                .h(px(1.))
                                .bg(rgb(t.border)),
                        );
                    }
                    row = row.child(connector);
                    // node dot
                    row = row.child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .mr_1p5()
                            .flex_shrink_0()
                            .bg(if is_active {
                                rgb(t.accent)
                            } else if is_on_path {
                                rgb(t.text_dim)
                            } else {
                                rgb(t.border)
                            }),
                    );
                    // role badge
                    if role == "user" || role == "assistant" {
                        row = row.child(
                            div()
                                .px_1()
                                .mr_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(if role == "user" {
                                    rgb(t.accent)
                                } else {
                                    rgb(t.border)
                                })
                                .text_size(px(9.))
                                .line_height(px(14.))
                                .text_color(if role == "user" {
                                    rgb(t.accent)
                                } else {
                                    rgb(t.text_dim)
                                })
                                .child(if role == "user" {
                                    "U"
                                } else {
                                    "A"
                                }),
                        );
                    }
                    // skipped indicator
                    if skipped > 0 {
                        row = row.child(
                            div()
                                .text_size(px(10.))
                                .mr_1()
                                .text_color(rgb(t.text_dim))
                                .child(SharedString::from(format!("+{skipped}"))),
                        );
                    }
                    // label
                    row = row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(11.))
                            .font_weight(if is_active {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(if is_active {
                                rgb(t.text)
                            } else if is_on_path {
                                rgb(t.text_dim)
                            } else {
                                rgb(0x9ca3af)
                            })
                            .child(SharedString::from(label.to_string())),
                    );
                    // click = fork from this node's user message
                    if let Some(entry_id) = node.forkable_entry_id() {
                        let entry_id = entry_id.to_string();
                        let weak_click = weak.clone();
                        row = row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_click.update(cx, |c, cx| {
                                c.fork_from_entry(entry_id.clone(), cx);
                            });
                        });
                    }
                    out.push(row.into_any_element());
                    let n_children = node.children.len();
                    for (i, child) in node.children.iter().enumerate() {
                        let (rep, sk, lab) = compress_chain(child);
                        push_node(
                            &rep,
                            sk,
                            &lab,
                            i == n_children - 1,
                            &[
                                parent_lines,
                                &[!is_last as bool],
                            ]
                            .concat(),
                            active_path,
                            weak,
                            out,
                        );
                    }
                }

                let mut rows: Vec<gpui::AnyElement> = Vec::new();
                for (i, node) in top_level.iter().enumerate() {
                    let (rep, sk, lab) = compress_chain(node);
                    push_node(
                        &rep,
                        sk,
                        &lab,
                        i == top_level.len() - 1,
                        &[],
                        &active_path,
                        &weak,
                        &mut rows,
                    );
                }

                let body: gpui::AnyElement = if !has_session {
                    div()
                        .px_4()
                        .py_2p5()
                        .text_xs()
                        .text_color(rgb(t.text_muted))
                        .child(tr("无活动会话"))
                        .into_any_element()
                } else if !has_branches || rows.is_empty() {
                    div()
                        .px_4()
                        .py_2p5()
                        .text_xs()
                        .text_color(rgb(t.text_muted))
                        .child(tr("暂无分支"))
                        .into_any_element()
                } else {
                    div()
                        .px_3()
                        .pt_1()
                        .pb_2()
                        .max_h(px(260.))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .children(rows)
                        .into_any_element()
                };

                root = root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.35))
                        .track_focus(&chat.dialog_focus)
                        .on_key_down({
                            let weak = weak.clone();
                            move |ev: &KeyDownEvent, _w, cx| {
                                if ev.keystroke.key == "escape" {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(520.))
                                .max_h(px(560.))
                                .bg(rgb(t.bg_panel))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .shadow_lg()
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(rgb(t.text))
                                                .child(tr("分支")),
                                        )
                                        .child(
                                            div()
                                                .id("branch-close")
                                                .px_2()
                                                .cursor_pointer()
                                                .text_color(rgb(t.text_muted))
                                                .hover(|s| s.text_color(rgb(t.text)))
                                                .on_mouse_down(MouseButton::Left, {
                                                    let weak = weak.clone();
                                                    move |_, _, cx| {
                                                        let _ = weak.update(cx, |c, cx| {
                                                            c.dialog = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(icon("x", 12., t.text_muted)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(t.text_dim))
                                        .child(tr("点击节点：从该用户消息处创建分支新会话")),
                                )
                                .child(body),
                        ),
                );
            }
            if chat.dialog.as_ref().is_some_and(|d| matches!(d, Dialog::ProjectSelect)) {
                let t = T();
                let weak = weak.clone();
                // recent projects: unique cwds by latest activity (getRecentProjects parity)
                let mut latest: std::collections::HashMap<String, (PathBuf, std::time::SystemTime)> =
                    std::collections::HashMap::new();
                for s in list_sessions(200) {
                    let entry = latest.entry(s.cwd.clone()).or_insert((PathBuf::from(&s.cwd), s.modified));
                    if s.modified > entry.1 {
                        entry.1 = s.modified;
                    }
                }
                let mut projects: Vec<(String, PathBuf)> = latest.into_iter().map(|(k, v)| (k, v.0)).collect();
                projects.sort_by(|a, b| a.0.cmp(&b.0));
                let current = chat.cwd.to_string_lossy().to_string();

                let mut rows: Vec<gpui::AnyElement> = Vec::new();
                for (cwd_text, cwd_path) in &projects {
                    let is_current = same_ws(cwd_text, &current);
                    let cwd_clone = cwd_path.clone();
                    let weak_row = weak.clone();
                    rows.push(
                        div()
                            .id(SharedString::from(format!("proj-{}", cwd_text)))
                            .w_full()
                            .px_3()
                            .py_2()
                            .rounded(px(7.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .when(is_current, |d| {
                                d.bg(rgb(t.bg_selected))
                                    .border_1()
                                    .border_color(rgb(t.accent))
                            })
                            .when(!is_current, |d| {
                                d.border_1()
                                    .border_color(rgb(t.border))
                                    .hover(|s| s.bg(rgb(t.bg_hover)))
                            })
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let p = cwd_clone.clone();
                                let _ = weak_row.update(cx, |c, cx| {
                                    c.switch_project(p, cx);
                                });
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(t.text))
                                    .child(SharedString::from(cwd_text.clone())),
                            )
                            .child(if is_current {
                                icon("check", 12., t.accent)
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            })
                            .into_any_element()
                );
                }
                root = root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.35))
                        .track_focus(&chat.dialog_focus)
                        .on_key_down({
                            let weak = weak.clone();
                            move |ev: &KeyDownEvent, _w, cx| {
                                if ev.keystroke.key == "escape" {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(520.))
                                .max_h(px(560.))
                                .bg(rgb(t.bg_panel))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .shadow_lg()
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(rgb(t.text))
                                                .child(tr("选择项目")),
                                        )
                                        .child(
                                            div()
                                                .id("project-close")
                                                .px_2()
                                                .cursor_pointer()
                                                .text_color(rgb(t.text_muted))
                                                .hover(|s| s.text_color(rgb(t.text)))
                                                .on_mouse_down(MouseButton::Left, {
                                                    let weak = weak.clone();
                                                    move |_, _, cx| {
                                                        let _ = weak.update(cx, |c, cx| {
                                                            c.dialog = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(icon("x", 12., t.text_muted)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(t.text_dim))
                                        .child(tr("切换后仅显示该项目的会话，并恢复上次打开的会话")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1p5()
                                        .max_h(px(380.))
                                        .overflow_hidden()
                                        .children(rows),
                                ),
                        ),
                );
            }
            if let Some(Dialog::FilePreview { path }) = chat.dialog.as_ref() {
                let path_text: SharedString = path.to_string_lossy().to_string().into();
                let (content, meta_line) = match chat.file_cache.get(path) {
                    Some(fc) => {
                        let meta = Chat::file_meta(path, &fc.content);
                        (fc.content.clone(), meta)
                    }
                    None => ("(loading)".to_string(), String::new()),
                };
                let mut body = content;
                if body.chars().count() > 80000 {
                    body = body.chars().take(80000).collect();
                    body.push_str("\n\n\u{2026} (truncated)");
                }
                root = root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.35))
                        .track_focus(&chat.dialog_focus)
                        .on_key_down({
                            let weak = weak.clone();
                            move |ev: &KeyDownEvent, _w, cx| {
                                if ev.keystroke.key == "escape" {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(760.))
                                .max_h(px(640.))
                                .bg(rgb(t.bg_panel))
                                .border_1()
                                .border_color(rgb(t.border))
                                .rounded(px(8.))
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .shadow_lg()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(t.text))
                                        .child(path_text),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(t.text_dim))
                                        .child(SharedString::from(meta_line)),
                                )
                                .child(
                                    div()
                                        .id("file-preview-body")
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .bg(rgb(t.bg))
                                        .rounded(px(6.))
                                        .p_2()
                                        .font_family("Consolas")
                                        .text_size(px(11.))
                                        .text_color(rgb(t.text))
                                        .child(SharedString::from(body)),
                                ),
                        ),
                );
            }
            if let Some(Dialog::GitDiff { path, patch }) = chat.dialog.as_ref() {
                let path_text: SharedString = path.to_string_lossy().to_string().into();
                let mut body = patch.clone();
                if body.chars().count() > 60000 {
                    body = body.chars().take(60000).collect();
                    body.push_str("\n\n\u{2026} (truncated)");
                }
                root = root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., 0.35))
                        .track_focus(&chat.dialog_focus)
                        .on_key_down({
                            let weak = weak.clone();
                            move |ev: &KeyDownEvent, _w, cx| {
                                if ev.keystroke.key == "escape" {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.dialog = None;
                                        cx.notify();
                                    });
                                }
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(760.))
                                .max_h(px(640.))
                                .bg(rgb(t.bg_panel))
                                .border_1()
                                .border_color(rgb(t.border))
                                .rounded(px(8.))
                                .p_4()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .shadow_lg()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(icon("git-branch", 12., t.accent))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_family("Consolas")
                                                        .text_color(rgb(t.text_muted))
                                                        .child(path_text),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("diff-close")
                                                .px_2()
                                                .cursor_pointer()
                                                .text_color(rgb(t.text_muted))
                                                .hover(|s| s.text_color(rgb(t.text)))
                                                .on_mouse_down(MouseButton::Left, {
                                                    let weak = weak.clone();
                                                    move |_, _, cx| {
                                                        let _ = weak.update(cx, |c, cx| {
                                                            c.dialog = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(icon("x", 12., t.text_muted)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .max_h(px(520.))
                                        .overflow_hidden()
                                        .p_2()
                                        .rounded(px(6.))
                                        .bg(rgb(t.bg))
                                        .font_family("Consolas")
                                        .text_size(px(11.))
                                        .text_color(rgb(t.text))
                                        .child(SharedString::from(body)),
                                ),
                        ),
                );
            }
    root
}
