//! functionPanel (015): the ZED-style dock's views. Phase C keeps the
//! whole sidebar as one view fn; dock container + view split land in
//! phase D/E (ARCHITECTURE.md §2).

pub(crate) mod file_tree;
pub(crate) mod git_panel;

use gpui::{Context, Entity, KeyDownEvent, MouseButton, SharedString, div, list, prelude::*, px, relative, rgb};

use self::file_tree::collect_tree_rows;
use std::path::PathBuf;

use crate::Chat;
use crate::DockPanel;
use crate::services::git::GitStatus;
use crate::terminal;
use crate::terminal::TermStatus;
use crate::PanelTab;
use crate::session::messages::Role;
use std::collections::HashSet;
use crate::i18n::tr;
use crate::services::format::*;
use crate::theme::theme as T;
use crate::ui::icon;
use crate::ui::spinner;

pub(crate) fn sidebar(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    let t = T();
    let cwd_text: SharedString = chat.cwd.to_string_lossy().to_string().into();
    let branch: SharedString = if chat.branch.is_empty() {
        "no git".into()
    } else {
        chat.branch.clone().into()
    };
    let sessions_entity = entity.clone();
    let weak_for_sessions = weak.clone();
    let sidebar = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // brand + new + search
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .child(
                    div()
                        .text_base()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(t.text))
                        .child(tr("会话")),
                )
                .child(
                    div()
                        .flex()
                        .gap_1p5()
                        .child(
                            div()
                                .id("new-session")
                                .flex()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .py_1()
                                .rounded(px(7.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .bg(rgb(t.bg_hover))
                                .text_xs()
                                .text_color(rgb(t.text))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_selected)))
                                .on_mouse_down(MouseButton::Left, {
                                    let weak = weak_for_sessions.clone();
                                    move |_, _, cx| {
                                        let _ =
                                            weak.update(cx, |c, cx| c.new_session(cx));
                                    }
                                })
                                .child(icon("plus", 12., t.text))
                                .child(SharedString::from(tr("新建"))),
                        )
                        .child(
                            div()
                                .id("search")
                                .w(px(30.))
                                .h(px(26.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(7.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .bg(rgb(t.bg_hover))
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_selected)))
                                                                    .on_mouse_down(MouseButton::Left, cx.listener(
                                    |this, _: &gpui::MouseDownEvent, window, cx| {
                                        this.search_open = !this.search_open;
                                        let input = this.search_input.clone();
                                        input.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                                        if this.search_open {
                                            input.update(cx, |ti, cx| ti.focus(window, cx));
                                        }
                                        this.refresh_sessions();
                                        cx.notify();
                                    },
                                ))
.child(icon("search", 12., t.text_muted)),
                        ),
                ),
        )
        // project box
        .child(
            div()
                .id("project-frame")
                .mx_3()
                .mb_1p5()
                .px_2p5()
                .py_1p5()
                .rounded(px(7.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.assistant_bg))
                .text_xs()
                .text_color(rgb(t.text))
                .cursor_pointer()
                .hover(|s| s.border_color(rgb(t.text_dim)))
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        this.open_project_select(cx);
                    },
                ))
                .child(cwd_text),
        )
        // branch box
        .child(
            div()
                .mx_3()
                .mb_2()
                .px_2p5()
                .py_1p5()
                .rounded(px(7.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.assistant_bg))
                .flex()
                .items_center()
                .justify_between()
                .text_xs()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_color(rgb(t.text))
                        .child(icon("git-branch", 12., t.text))
                        .child(branch),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(tr("主分支")))
                        .child(icon("chevron-down", 10., t.text_muted)),
                ),
        )
        // session search row (pi-web SessionSearch input)
        .children(chat.search_open.then(|| {
            div()
                .id("session-search-row")
                .mx_3()
                .mb_1p5()
                .child(chat.search_input.clone())
        }))
        // sessions list (pi-web SessionSearch: query filters the list)
        .child({
            let q = chat.search_input.read(cx).value().to_lowercase();
            let session_display: Vec<usize> = if q.is_empty() {
                (0..chat.sessions.len()).collect()
            } else {
                chat.sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| {
                        i.preview.to_lowercase().contains(&q)
                            || i.name
                                .as_ref()
                                .is_some_and(|n| n.to_lowercase().contains(&q))
                            || i
                                .path
                                .to_string_lossy()
                                .to_lowercase()
                                .contains(&q)
                    })
                    .map(|(i, _)| i)
                    .collect()
            };
            if chat.sessions_list_count != session_display.len() {
                chat.sessions_list.reset(session_display.len());
                chat.sessions_list_count = session_display.len();
            }
            let session_display = std::sync::Arc::new(session_display);
            list(
                chat.sessions_list.clone(),
                move |ix, _window, cx| {
                    let chat = sessions_entity.read(cx);
                    let Some(orig) = session_display.get(ix).copied() else {
                        return div().into_any_element();
                    };
                    let Some(info) = chat.sessions.get(orig) else {
                        return div().into_any_element();
                    };
                let is_active = chat
                    .active_session_file
                    .as_deref()
                    == Some(info.path.as_path());
                let path = info.path.clone();
                let preview: SharedString = if info.preview.is_empty() {
                    "(empty)".into()
                } else {
                    info.preview.clone().into()
                };
                // pi-web title = session.name || first message
                let title: SharedString = info
                    .name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .map(SharedString::from)
                    .unwrap_or_else(|| preview.clone());
                let time_text = time_ago(info.modified);
                // pi-web runningSessionIds: the spinner replaces the
                // timestamp on the running session's row (one embedded
                // agent → the active session is the running one)
                let streaming = is_active
                    && (chat.agent_running
                        || chat
                            .state
                            .as_ref()
                            .is_some_and(|s| s.is_streaming));
                let hovered = chat.hovered_session == Some(ix);
                let confirming =
                    chat.confirm_delete.as_deref() == Some(info.path.as_path());
                // pi-web renaming: this row's content swaps for the input
                let renaming =
                    chat.renaming.as_deref() == Some(info.path.as_path());
                let weak_del2 = weak_for_sessions.clone();
                let weak_del3 = weak_for_sessions.clone();
                // pi-web: "删除 {title}？" truncates the title at 22 chars
                let confirm_title: String = {
                    let mut s = info
                        .name
                        .clone()
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| preview.clone().to_string());
                    if s.chars().count() > 22 {
                        s = s.chars().take(22).collect::<String>() + "…";
                    }
                    s
                };
                let weak = weak_for_sessions.clone();
                let weak_del = weak_for_sessions.clone();
                let weak_ren = weak_for_sessions.clone();
                let weak_hover = weak_for_sessions.clone();
                let p_del = info.path.clone();
                div()
                    .id(SharedString::from(format!("row-{ix}")))
                    .w_full()
                    .h(px(54.))
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .pl_3p5()
                    .pr_2()
                    .overflow_hidden()
                    .border_l_2()
                    // pi-web: confirm state paints the row red and
                    // swallows the row click
                    .when(confirming, |d| {
                        d.cursor(gpui::CursorStyle::Arrow)
                            .bg(gpui::rgba(0xef44440f))
                            .border_color(rgb(0xef4444))
                    })
                    .when(!confirming, |d| {
                        d.cursor_pointer()
                            .when(is_active, |d| {
                                d.bg(rgb(t.bg_selected))
                                    .border_color(rgb(t.accent))
                            })
                            .when(!is_active, |d| d.border_color(rgb(t.bg)))
                            .when(hovered && !is_active, |d| d.bg(rgb(t.bg_hover)))
                    })
                    .when(!confirming && !renaming, |d| {
                        d.on_mouse_down(MouseButton::Left, {
                            let p = path.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |c, cx| {
                                    c.open_session(p.clone(), false, cx)
                                });
                            }
                        })
                    })
                    .on_hover(move |h, _, cx| {
                        let _ = weak_hover.update(cx, |c, cx| {
                            // only the owning row may clear its own hover;
                            // row A's (false) must not erase row B's (true)
                            // when leave/enter events arrive out of order
                            if *h {
                                if c.hovered_session != Some(ix) {
                                    c.hovered_session = Some(ix);
                                    cx.notify();
                                }
                            } else if c.hovered_session == Some(ix) {
                                c.hovered_session = None;
                                cx.notify();
                            }
                        });
                    })
                    .children(if renaming {
                        // pi-web: "Rename: input fills the same row"
                        chat.rename_input.clone().map(|input| {
                            div().flex_1().min_w_0().child(input)
                        })
                    } else {
                        None
                    })
                    .children((!confirming && !renaming).then(|| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_xs()
                                    .line_height(relative(1.4))
                                    .font_weight(if is_active {
                                        gpui::FontWeight::MEDIUM
                                    } else {
                                        gpui::FontWeight::NORMAL
                                    })
                                    .text_color(rgb(t.text))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_size(px(11.))
                                    .min_w_0()
                                    .text_color(rgb(t.text_dim))
                                    .child(if streaming {
                                        spinner(14., t.accent)
                                    } else {
                                        SharedString::from(time_text.clone())
                                            .into_any_element()
                                    })
                                    .child(SharedString::from(crate::i18n::tf(
                                        "{n} 条消息",
                                        &[("n", info.message_count.to_string())],
                                    ))),
                            )
                    }))
                    .children(if confirming {
                        // pi-web delete confirmation: the row content
                        // swaps in place — "删除 {title}？" + red 删除/取消
                        Some(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(12.))
                                        .text_color(rgb(t.text))
                                        .child(SharedString::from(crate::i18n::tf(
                                            "删除 {title}？",
                                            &[("title", confirm_title)],
                                        ))),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("delok-{ix}")))
                                        .h(px(30.))
                                        .px(px(11.))
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .rounded(px(6.))
                                        .bg(rgb(0xef4444))
                                        .text_size(px(12.))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(rgb(0xffffff))
                                        .cursor_pointer()
                                        .flex_shrink_0()
                                        .child(icon("trash", 12., 0xffffff))
                                        .child(SharedString::from(tr("删除")))
                                        .on_mouse_down(MouseButton::Left, {
                                            let p = p_del.clone();
                                            move |_, _, cx| {
                                                let _ = weak_del2.update(cx, |c, cx| {
                                                    c.delete_session(p.clone(), cx)
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("delno-{ix}")))
                                        .h(px(30.))
                                        .px(px(11.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(6.))
                                        .border_1()
                                        .border_color(rgb(t.border))
                                        .bg(rgb(t.bg_hover))
                                        .text_size(px(12.))
                                        .text_color(rgb(t.text_muted))
                                        .cursor_pointer()
                                        .flex_shrink_0()
                                        .child(SharedString::from(tr("取消")))
                                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                            let _ = weak_del3.update(cx, |c, cx| {
                                                c.confirm_delete = None;
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                    } else {
                        None
                    })
                    .children(if hovered && !confirming && !renaming {
                        Some(
                            div()
                                .flex()
                                .gap_1()
                                .flex_shrink_0()
                                .child(
                                    div()
                                        .id(SharedString::from(format!("ren-{ix}")))
                                        .size(px(28.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(7.))
                                        .border_1()
                                        .border_color(rgb(t.border))
                                        .bg(rgb(t.bg_hover))
                                        .text_color(rgb(t.text_dim))
                                        .cursor_pointer()
                                        .hover(|s| {
                                            s.bg(rgb(t.bg_selected))
                                                .text_color(rgb(t.accent))
                                        })
                                        .on_mouse_down(MouseButton::Left, {
                                            let p = path.clone();
                                            move |_, _, cx| {
                                                cx.stop_propagation();
                                                let _ = weak_ren.update(cx, |c, cx| {
                                                    if c.active_session_file.as_deref()
                                                        == Some(p.as_path())
                                                    {
                                                        // pi-web: inline rename, no modal
                                                        let prefill = c
                                                            .state
                                                            .as_ref()
                                                            .and_then(|s| {
                                                                s.session_name.clone()
                                                            })
                                                            .or_else(|| {
                                                                c.messages
                                                                    .iter()
                                                                    .find(|m| {
                                                                        matches!(m.role, Role::User)
                                                                    })
                                                                    .map(|m| m.plain_text())
                                                            })
                                                            .map(|v| {
                                                                v.chars().take(50).collect::<String>()
                                                            })
                                                            .unwrap_or_default();
                                                        c.start_rename(p.clone(), prefill, cx);
                                                    } else {
                                                        c.open_session(p.clone(), true, cx);
                                                    }
                                                });
                                            }
                                        })
                                        .child(icon("pencil", 14., t.text_dim)),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("del-{ix}")))
                                        .size(px(28.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(7.))
                                        .border_1()
                                        .border_color(rgb(t.border))
                                        .bg(rgb(t.bg_hover))
                                        .text_color(rgb(t.text_dim))
                                        .cursor_pointer()
                                        .hover(|s| {
                                            s.bg(gpui::rgba(0xef444414))
                                                .text_color(rgb(0xef4444))
                                        })
                                        .on_mouse_down(MouseButton::Left, {
                                            let p = p_del.clone();
                                            move |ev, _, cx| {
                                                cx.stop_propagation();
                                                let _ = weak_del.update(cx, |c, cx| {
                                                    if ev.modifiers.shift {
                                                        // pi-web: shift+click deletes
                                                        // without confirmation
                                                        c.delete_session(p.clone(), cx);
                                                    } else {
                                                        c.confirm_delete = Some(p.clone());
                                                        cx.notify();
                                                    }
                                                });
                                            }
                                        })
                                        .child(icon("trash", 14., t.text_dim)),
                                ),
                        )
                    } else {
                        None
                    })
                    .into_any_element()
                }  // move closure
                )  // list(
            // sessions pane height = persisted fraction of the sidebar
            // (pi-web --sidebar-session-pane-height; default half)
            .flex_basis(relative(chat.sidebar_sessions_frac))
            .min_h_0()
            .overflow_hidden()
        })  // .child({ ... }) block
        ;
    sidebar
}

/// files view (021 groundwork): the directory file explorer, moved out of
/// the old sidebar; the zed project_panel skeleton lands in phase E.
pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    let t = T();
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(t.bg))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .border_t_1()
                .border_color(rgb(t.border))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_3()
                        .py_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(icon("chevron-down", 10., t.text))
                                .child(SharedString::from(tr("文件浏览器"))),
                        )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .text_color(rgb(t.text_muted))
                        .child(icon("monitor", 12., t.text_muted))
                        .child(icon("search", 12., t.text_muted))
                        .child(icon("upload", 12., t.text_muted))
                        // open terminal for the selected cwd (pi-web
                        // SessionSidebar explorer terminal button)
                        .child(
                            div()
                                .id("open-terminal")
                                .cursor_pointer()
                                .hover(|s| s.text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, cx.listener(
                                    |this, _: &gpui::MouseDownEvent, window, cx| {
                                        this.open_terminal(window, cx);
                                    },
                                ))
                                .child(icon("terminal", 13., t.text_muted)),
                        )
                        .child(if chat.git_add_del.0 + chat.git_add_del.1 > 0 {
                                    div()
                                        .text_xs()
                                        .font_family("Consolas")
                                        .text_color(rgb(0xd6a84b))
                                        .child(SharedString::from(format!(
                                            "+{} -{}",
                                            chat.git_add_del.0, chat.git_add_del.1
                                        )))
                                        .into_any_element()
                                } else {
                                    div().into_any_element()
                                })
                                .child(icon("refresh", 12., t.text_muted)),
                        ),
                )
                .child(
                    div()
                        .px_3()
                        .pb_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                        div()
                            .id("file-tree-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children({
                            let mut rows: Vec<gpui::AnyElement> = Vec::new();
                            let git_map: std::collections::HashMap<
                                PathBuf,
                                GitStatus,
                            > = chat
                                .git_files
                                .iter()
                                .map(|f| (f.path.clone(), f.status))
                                .collect();
                            let changed_dirs: HashSet<PathBuf> = git_map
                                .keys()
                                .filter_map(|p| p.parent().map(|d| d.to_path_buf()))
                                .collect();
                            collect_tree_rows(
                                &chat.cwd,
                                0,
                                &chat.expanded_dirs,
                                &git_map,
                                &changed_dirs,
                                &weak,
                                t,
                                &mut rows,
                            );
                            rows
                        }),
                        ),
                ),
        )
}

/// Dock container (015): fixed-width column whose border side flips with
/// dock_right (018 right-click); hosts the active view. The terminal view
/// element is built in main.rs render (right-panel host; phase E moves it
/// here).
pub(crate) fn dock(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    let t = T();
    let right = chat.dock_right;
    let view: gpui::AnyElement = match chat.dock_panel {
        DockPanel::Sessions => sidebar(chat, entity, weak, cx).into_any_element(),
        DockPanel::Files => files_view(chat, weak, cx).into_any_element(),
        DockPanel::Git => git_panel::view(chat, weak, cx).into_any_element(),
        DockPanel::Terminal => terminal_view(chat, weak, window, cx)
            .map(|d| d.into_any_element())
            .unwrap_or_else(|| {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(t.text_dim))
                    .child(tr("暂无终端会话"))
                    .into_any_element()
            }),
    };
    let mut col = div()
        .w(px(260.))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg));
    col = if right {
        col.border_l_1().border_color(rgb(t.border))
    } else {
        col.border_r_1().border_color(rgb(t.border))
    };
    col.child(view)
}

/// Terminal view host — verbatim from the former main.rs right-panel
/// block (tab bar + alacritty surface). Returns None when no terminal
/// tabs exist (dock renders its empty hint).
pub(crate) fn terminal_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> Option<gpui::Div> {
    let t = T();
    let terminal_el = if !chat.panel_tabs.is_empty() {
        let weak_for_tabs = weak.clone();
        let tabbar = div()
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .children(chat.panel_tabs.iter().enumerate().map(|(ix, tab)| {
                let active = chat.active_panel_tab == Some(ix);
                let (icon_name, label) = match tab {
                    PanelTab::Term(id) => {
                        let title = chat
                            .terminals
                            .iter()
                            .find(|t| t.id == *id)
                            .map(|t| t.title.clone())
                            .unwrap_or_default();
                        ("terminal", title)
                    }
                };
                let label: SharedString = label.into();
                let weak_tab = weak_for_tabs.clone();
                let weak_close = weak_for_tabs.clone();
                let term_focus: Option<gpui::FocusHandle> = match tab {
                    PanelTab::Term(id) => chat
                        .terminals
                        .iter()
                        .find(|t| t.id == *id)
                        .map(|t| t.focus.clone()),
                    _ => None,
                };
                div()
                    .id(SharedString::from(format!("ptab-{ix}")))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(12.))
                    .pr(px(6.))
                    .min_w(px(80.))
                    .max_w(px(180.))
                    .border_r_1()
                    .border_color(rgb(t.border))
                    .bg(if active { rgb(t.bg) } else { rgb(t.bg_panel) })
                    .text_xs()
                    .font_weight(if active {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        let _ = weak_tab.update(cx, |c, cx| {
                            c.active_panel_tab = Some(ix);
                            cx.notify();
                        });
                        if let Some(f) = term_focus.clone() {
                            window.focus(&f);
                        }
                    })
                    // middle-click closes the tab (pi-web TabBar auxclick)
                    .on_mouse_down(MouseButton::Middle, {
                        let w = weak_for_tabs.clone();
                        move |_, _, cx| {
                            let _ = w.update(cx, |c, cx| c.close_panel_tab(ix, cx));
                        }
                    })
                    .child(icon(icon_name, 13., if active { t.text } else { t.text_muted }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(label),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("ptab-x-{ix}")))
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_color(rgb(t.text_muted))
                            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, {
                                let w = weak_close.clone();
                                move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = w.update(cx, |c, cx| c.close_panel_tab(ix, cx));
                                }
                            })
                            .child(icon("x", 11., t.text_muted)),
                    )
                    .into_any_element()
            }));

        let body: Option<gpui::AnyElement> = chat
            .active_panel_tab
            .and_then(|ix| chat.panel_tabs.get(ix).cloned())
            .map(|tab| match tab {
                PanelTab::Term(id) => {
                    // terminal panel (header + banners + grid)
                    let tix = chat.terminals.iter().position(|t| t.id == id);
                    let Some(tix) = tix else {
                        return div().into_any_element();
                    };
                    let tab = &chat.terminals[tix];
                    let (dot, _status) = match &tab.status {
                        TermStatus::Ready => (0x4ade80, ""),
                        TermStatus::Exited(_) | TermStatus::Failed(_) => (0xf87171, ""),
                    };
                    let cwd_text: SharedString = tab.cwd.to_string_lossy().to_string().into();
                    let weak_restart = weak.clone();
                    let mut col = div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(38.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .pl(px(13.))
                                .pr(px(10.))
                                .bg(rgb(0x181b21))
                                .border_b_1()
                                .border_color(rgb(0x2f3540))
                                .child(
                                    div()
                                        .size(px(7.))
                                        .rounded_full()
                                        .flex_shrink_0()
                                        .bg(rgb(dot)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(11.))
                                        .font_family(terminal::FONT_FAMILY)
                                        .text_color(rgb(0x9ca3af))
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .overflow_hidden()
                                        .child(cwd_text),
                                )
                                .child(
                                    div()
                                        .id("term-restart")
                                        .h(px(27.))
                                        .px(px(8.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(5.))
                                        .border_1()
                                        .border_color(rgb(0x343a46))
                                        .text_color(rgb(0x9ca3af))
                                        .cursor_pointer()
                                        .hover(|s| {
                                            s.bg(rgb(0x242932)).text_color(rgb(0xe5e7eb))
                                        })
                                        .on_mouse_down(MouseButton::Left, {
                                            let rix = tix;
                                            move |_, _, cx| {
                                                let _ = weak_restart.update(cx, |c, cx| {
                                                    c.restart_terminal(rix, cx);
                                                });
                                            }
                                        })
                                        .child(icon("refresh", 12., 0x9ca3af)),
                                ),
                        );
                    match &tab.status {
                        TermStatus::Failed(e) => {
                            col = col.child(
                                div()
                                    .py(px(7.))
                                    .px(px(12.))
                                    .bg(rgb(0x321b1b))
                                    .border_b_1()
                                    .border_color(rgb(0x5f2424))
                                    .text_size(px(11.))
                                    .font_family(terminal::FONT_FAMILY)
                                    .text_color(rgb(0xfca5a5))
                                    .child(SharedString::from(e.clone())),
                            );
                        }
                        TermStatus::Exited(code) => {
                            let code_text = code
                                .map(|c| c.to_string())
                                .unwrap_or_else(|| tr("unknown").to_string());
                            col = col.child(
                                div()
                                    .py(px(7.))
                                    .px(px(12.))
                                    .text_size(px(11.))
                                    .font_family(terminal::FONT_FAMILY)
                                    .text_color(rgb(0x9ca3af))
                                    .child(SharedString::from(crate::i18n::tf(
                                        "Process exited with code {code_text}",
                                        &[("code_text", code_text)],
                                    ))),
                            );
                        }
                        TermStatus::Ready => {}
                    }
                    col.child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .bg(rgb(0x111318))
                            .child(
                                terminal::TerminalElement::new(tab, weak.clone())
                                    .track_focus(&tab.focus)
                                    .flex_1()
                                    .h_full()
                                    .on_mouse_down(MouseButton::Left, {
                                        let f = tab.focus.clone();
                                        move |_, window, _cx| {
                                            window.focus(&f);
                                        }
                                    })
                                    .on_key_down(cx.listener(
                                        |this, ev: &KeyDownEvent, _w, cx| {
                                            this.terminal_key(ev, cx);
                                        },
                                    )),
                            ),
                    )
                    .into_any_element()
                }
            });

        Some(
            div()
                .h_full()
                .flex_shrink_0()
                .flex()
                .child(
                    div()
                        .w_full()
                        .h_full()
                        .flex()
                        .flex_col()
                        .bg(rgb(t.bg))
                        .child(
                            div()
                                .flex()
                                .h(px(36.))
                                .flex_shrink_0()
                                .bg(rgb(t.bg_panel))
                                .border_b_1()
                                .border_color(rgb(t.border))
                                .child(tabbar)
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .id("panel-close")
                                        .w(px(36.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .text_color(rgb(t.text_muted))
                                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                        .on_mouse_down(MouseButton::Left, cx.listener(
                                            |this, _: &gpui::MouseDownEvent, window, cx| {
                                                window.focus(&this.focus);
                                                cx.notify();
                                            },
                                        ))
                                        .child(icon("x", 13., t.text_muted)),
                                ),
                        )
                        .children(body),
                ),
        )
    } else {
        None
    };
    terminal_el
}
