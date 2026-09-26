//! gitPanel (022, simplified): Changes | History tabs + commit/push.
//! No diff view (006-adjacent decision recorded in the phase-E bead).
//! Data comes from services::git process calls — the same pattern zed's
//! git crate uses, so the zed panel skeleton port can reuse this layer.

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::i18n::tr;
use crate::services::git;
use crate::theme::theme as T;
use crate::ui::icon;

/// Active git panel tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitTab {
    Changes,
    History,
}

pub(crate) fn view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::Div {
    let t = T();
    let mut col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text));

    // tab header (022: Changes | History)
    let mut tabs = div()
        .flex()
        .flex_shrink_0()
        .border_b_1()
        .border_color(rgb(t.border));
    for (tab, label) in [(GitTab::Changes, tr("更改")), (GitTab::History, tr("历史"))] {
        let active = chat.git_tab == tab;
        tabs = tabs.child(
            div()
                .id(SharedString::from(format!("git-tab-{}", tab as u8)))
                .flex_1()
                .py_2()
                .text_xs()
                .text_center()
                .cursor_pointer()
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(if active { rgb(t.text) } else { rgb(t.text_muted) })
                .border_b_2()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.git_set_tab(tab, cx);
                }))
                .child(label),
        );
    }
    col = col.child(tabs);

    match chat.git_tab {
        GitTab::Changes => col = col.child(changes_body(chat, weak, cx)),
        GitTab::History => col = col.child(history_body(chat)),
    }

    if let Some(err) = &chat.git_error {
        col = col.child(
            div()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(rgb(0xf87171))
                .child(SharedString::from(err.clone())),
        );
    }
    col
}

fn changes_body(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let cwd = chat.cwd.clone();
    let mut list = div().id("git-changes").flex_1().min_h_0().overflow_y_scroll().flex().flex_col();

    if chat.git_files.is_empty() {
        list = list.child(
            div()
                .px_3()
                .py_2()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .child(tr("工作区干净")),
        );
    }
    for (ix, f) in chat.git_files.iter().enumerate() {
        let rel = f
            .path
            .strip_prefix(&cwd)
            .unwrap_or(&f.path)
            .to_string_lossy()
            .to_string();
        let staged = f.staged;
        let weak_row = weak.clone();
        list = list.child(
            div()
                .id(SharedString::from(format!("git-row-{ix}")))
                .px_3()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                // row click toggles staging (simplified checkbox)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| c.git_toggle_stage(ix, cx));
                })
                // staging checkbox
                .child(
                    div()
                        .size(px(14.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(rgb(if staged { t.accent } else { t.border }))
                        .bg(rgb(if staged { t.accent } else { t.bg }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(staged.then(|| icon("check", 10., t.accent_contrast))),
                )
                .child(
                    div()
                        .w(px(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(f.status.color()))
                        .child(f.status.badge()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(rgb(if staged { t.text_muted } else { t.text }))
                        .child(SharedString::from(rel)),
                ),
        );
    }

    // footer: commit message + commit / push (enabled by staged changes)
    let has_staged = chat.git_files.iter().any(|f| f.staged);
    let weak_btn = weak.clone();
    let weak_push = weak.clone();
    let footer = div()
        .flex_shrink_0()
        .border_t_1()
        .border_color(rgb(t.border))
        .p_2()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(chat.git_commit_input.clone())
        .child(
            div()
                .flex()
                .gap_1p5()
                .child(
                    div()
                        .id("git-commit-btn")
                        .flex_1()
                        .py_1p5()
                        .rounded(px(5.))
                        .text_xs()
                        .text_center()
                        .cursor_pointer()
                        .text_color(rgb(t.accent_contrast))
                        .bg(rgb(if has_staged { t.accent } else { t.bg_selected }))
                        .hover(|s| s.bg(rgb(t.accent_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_btn.update(cx, |c, cx| c.git_commit_staged(cx));
                        })
                        .child(tr("提交")),
                )
                .child(
                    div()
                        .id("git-push-btn")
                        .flex_1()
                        .py_1p5()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .text_xs()
                        .text_center()
                        .cursor_pointer()
                        .text_color(rgb(t.text))
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_push.update(cx, |c, cx| c.git_push_branch(cx));
                        })
                        .child(tr("推送")),
                ),
        );
    list.child(footer)
}

fn history_body(chat: &mut Chat) -> impl gpui::IntoElement {
    let t = T();
    let mut list = div().id("git-history").flex_1().min_h_0().overflow_y_scroll().flex().flex_col();
    if chat.git_log.is_empty() {
        list = list.child(
            div()
                .px_3()
                .py_2()
                .text_xs()
                .text_color(rgb(t.text_dim))
                .child(tr("暂无提交")),
        );
    }
    for c in &chat.git_log {
        list = list.child(
            div()
                .px_3()
                .py_1p5()
                .border_b_1()
                .border_color(rgb(t.border))
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(t.text))
                        .child(SharedString::from(c.subject.clone())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(format!(
                            "{} · {} · {}",
                            c.hash, c.author, c.date
                        ))),
                ),
        );
    }
    list
}

// ---------------------------------------------------------------------------
// Chat actions (settings-style cross-module impl; entities land in phase E)
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn git_set_tab(&mut self, tab: GitTab, cx: &mut gpui::Context<Self>) {
        self.git_tab = tab;
        if tab == GitTab::History && self.git_log.is_empty() {
            self.refresh_git_log();
        }
        cx.notify();
    }

    pub(crate) fn refresh_git_log(&mut self) {
        self.git_log = git::git_log(&self.cwd, 50);
    }

    fn git_toggle_stage(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        let Some(f) = self.git_files.get(ix) else { return };
        let (path, staged) = (f.path.clone(), f.staged);
        let r = if staged {
            git::git_unstage(&self.cwd, &path)
        } else {
            git::git_stage(&self.cwd, &path)
        };
        self.git_error = r.err();
        self.refresh_git();
        cx.notify();
    }

    pub(crate) fn git_commit_staged(&mut self, cx: &mut gpui::Context<Self>) {
        let msg = self.git_commit_input.read(cx).value().to_string();
        match git::git_commit(&self.cwd, &msg) {
            Ok(()) => {
                self.git_error = None;
                self.git_commit_input.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                self.refresh_git();
                self.refresh_git_log();
            }
            Err(e) => self.git_error = Some(e),
        }
        cx.notify();
    }

    pub(crate) fn git_push_branch(&mut self, cx: &mut gpui::Context<Self>) {
        self.git_error = Some(tr("推送中…").to_string());
        cx.notify();
        let cwd = self.cwd.clone();
        cx.spawn(async move |this, cx| {
            // push is network-bound: keep it off the frame path
            let r = cx
                .background_spawn(async move { git::git_push(&cwd) })
                .await;
            let _ = this.update(cx, |chat, cx| {
                chat.git_error = r.err().or_else(|| Some(tr("推送完成").to_string()));
                cx.notify();
            });
        })
        .detach();
    }
}
