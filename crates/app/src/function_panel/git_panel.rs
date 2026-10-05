//! gitPanel (v54 设计版): 头部单行（项目名 + Changes(N)/History tabs 右靠，
//! 激活连体）· Changes（View Diff + Stage All ∨ / 变更树复选 / 底部
//! ⎇branch + ↑N Push / commit message 融入式大区 / Commit Tracked ∨ /
//! 最近提交条 + uncommit）· History（提交列表：标题 + ↑ 推送小钮 + 元信息）。

use std::path::PathBuf;

use gpui::{MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::i18n::tr;
use crate::services::git;
use crate::theme::{Theme, theme as T};
use crate::ui::{icon, icon_hover};

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
    let proj: SharedString = chat
        .cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .into();
    let n_changes = chat.git_files.len();
    let changes_label = format!("Changes ({n_changes})");

    let mut col = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text))
        // 头部单行：项目名（淡色，截断）+ tabs 右靠（激活连体）
        .child(
            div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .border_b_1()
                .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
                .child(
                    div()
                        .min_w(px(100.))
                        .max_w(relative(0.5))
                        .pl(px(14.))
                        .pr(px(4.))
                        .py(px(5.))
                        .text_size(crate::appearance::ui_size(11.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text_soft))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(proj),
                )
                .child(div().flex_1())
                .child(
                    div().flex().child(git_tab(GitTab::Changes, &changes_label, chat.git_tab, weak.clone(), cx))
                        .child(git_tab(GitTab::History, "History", chat.git_tab, weak.clone(), cx)),
                ),
        );

    col = match chat.git_tab {
        GitTab::Changes => col.child(changes_body(chat, weak, cx)),
        GitTab::History => col.child(history_body(chat, weak, cx)),
    };

    if let Some(err) = &chat.git_error {
        col = col.child(
            div()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.danger))
                .child(SharedString::from(err.clone())),
        );
    }
    col
}

/// 连体 tab：激活 = bg 填充 + 边框（底无边）+ 顶圆角 + 压底线。
/// git 面板全部文字 = 面板设置值 -1（字体大小设置.md §1）。
fn git_tab(
    tab: GitTab,
    label: &str,
    active_tab: GitTab,
    weak: gpui::WeakEntity<Chat>,
    _cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let active = tab == active_tab;
    div()
        .id(SharedString::from(format!("git-tab-{}", tab as u8)))
        .px(px(9.))
        .pt(px(6.))
        .pb(px(5.))
        .mb(px(-1.))
        .text_size(crate::appearance::ui_size(11.))
        .cursor_pointer()
        .when(active, |d| {
            d.bg(rgb(t.bg))
                .border_1()
                .border_b_0()
                .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
                .rounded_tl(px(7.))
                .rounded_tr(px(7.))
                .text_color(rgb(t.text))
                .font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .when(!active, |d| {
            d.text_color(rgb(t.text_muted)).hover(|s| s.text_color(rgb(t.text)))
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| c.git_set_tab(tab, cx));
        })
        .child(SharedString::from(label.to_string()))
}

/// 变更树节点（目录嵌套 + 文件复选）。
#[derive(Debug, Clone)]
enum GitNode {
    Dir { name: String, children: Vec<GitNode> },
    File { ix: usize },
}

fn build_git_tree(chat: &Chat) -> Vec<GitNode> {
    // stable copy with indices into chat.git_files
    let mut files: Vec<(PathBuf, usize)> = chat
        .git_files
        .iter()
        .enumerate()
        .map(|(ix, f)| (f.path.clone(), ix))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut roots: Vec<GitNode> = Vec::new();
    fn insert(roots: &mut Vec<GitNode>, segs: &[&str], ix: usize) {
        let (head, rest) = segs.split_first().expect("non-empty");
        if rest.is_empty() {
            roots.push(GitNode::File { ix });
            return;
        }
        if let Some(pos) = roots
            .iter()
            .position(|n| matches!(n, GitNode::Dir { name, .. } if name == head))
        {
            if let GitNode::Dir { children, .. } = &mut roots[pos] {
                insert(children, rest, ix);
            }
            return;
        }
        let mut children = Vec::new();
        insert(&mut children, rest, ix);
        roots.push(GitNode::Dir {
            name: head.to_string(),
            children,
        });
    }
    for (path, ix) in files {
        let rel: Vec<String> = path
            .strip_prefix(&chat.cwd)
            .unwrap_or(&path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        let segs: Vec<&str> = rel.iter().map(|s| s.as_str()).collect();
        insert(&mut roots, &segs, ix);
    }
    roots
}

fn changes_body(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let branch: SharedString = if chat.branch.is_empty() {
        "no git".into()
    } else {
        chat.branch.clone().into()
    };
    let ahead = git::git_ahead_count(&chat.cwd);
    let last_commit = chat.git_log.first().cloned();

    div()
        .id("gv-changes")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        // 操作行: View Diff … Stage All
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(7.))
                .flex_shrink_0()
                .child(
                    div()
                        .id("git-viewdiff")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(6.))
                        .py(px(3.))
                        .rounded(px(6.))
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                // 选中行优先，否则第一个变更
                                let target = this
                                    .git_files
                                    .iter()
                                    .find(|f| Some(&f.path) == this.git_selected.as_ref())
                                    .or_else(|| this.git_files.first())
                                    .map(|f| f.path.clone());
                                if let Some(p) = target {
                                    this.open_git_diff(p, cx);
                                }
                            },
                        ))
                        .child(icon_hover("icon-viewdiff", 13., t.text_muted))
                        .child(SharedString::from("View Diff")),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id("git-stage-all")
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .px(px(9.))
                        .py(px(3.5))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg))
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.git_error = git::git_stage_all(&this.cwd).err();
                                this.refresh_git();
                                cx.notify();
                            },
                        ))
                        .child(SharedString::from(tr("Stage All")))
                        .child(icon("chevron-down", 10., t.text_dim)),
                ),
        )
        // 变更树
        .child(
            div()
                .id("git-tree")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(6.))
                .pb(px(6.))
                .pt(px(2.))
                .child(section(chat, weak.clone(), t)),
        )
        // 底部：branch/Push + commit 区 + Commit Tracked + 最近提交
        .child(
            div()
                .flex_shrink_0()
                .border_t_1()
                .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
                .px(px(10.))
                .pt(px(8.))
                .pb(px(7.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .text_size(crate::appearance::ui_size(11.))
                        .child(icon("git-branch", 14., t.text_muted))
                        .child(SharedString::from(branch))
                        .child(
                            div()
                                .id("git-push")
                                .ml_auto()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .px(px(9.))
                                .py(px(3.5))
                                .rounded(px(7.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .bg(rgb(t.bg))
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, cx.listener(
                                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.git_push_branch(cx);
                                    },
                                ))
                                .child(icon_hover("arrow-up", 12., t.text))
                                .child(SharedString::from(format!("{ahead} Push")))
                                .child(icon("chevron-down", 10., t.text_dim)),
                        ),
                )
                // 融入式 commit message 大区（无边框）
                .child(chat.git_commit_input.clone())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("git-commit-btn")
                                .ml_auto()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .px(px(9.))
                                .py(px(3.5))
                                .rounded(px(7.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .bg(rgb(t.bg))
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)))
                                .on_mouse_down(MouseButton::Left, cx.listener(
                                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.git_commit_staged(cx);
                                    },
                                ))
                                .child(SharedString::from(tr("提交已暂存")))
                                .child(icon("chevron-down", 10., t.text_dim)),
                        ),
                )
                // 最近提交条 + uncommit
                .children(last_commit.map(|c| {
                    let subject: SharedString = c.subject.clone().into();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .border_t_1()
                        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
                        .pt(px(7.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text_muted))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(subject),
                        )
                        .child(
                            div()
                                .id("git-uncommit")
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .text_color(rgb(t.text_muted))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, cx.listener(
                                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                                        match git::git_uncommit(&this.cwd) {
                                            Ok(()) => {
                                                this.git_error = None;
                                                this.refresh_git();
                                                this.refresh_git_log();
                                            }
                                            Err(e) => this.git_error = Some(e),
                                        }
                                        cx.notify();
                                    },
                                ))
                                .child(icon_hover("refresh", 13., t.text_muted)),
                        )
                })),
        )
        .into_any_element()
}

/// 变更树 section：目录嵌套（拍平行）+ 行尾复选。
fn section(chat: &Chat, weak: gpui::WeakEntity<Chat>, t: &'static Theme) -> gpui::AnyElement {
    if chat.git_files.is_empty() {
        return div()
            .px(px(8.))
            .py(px(6.))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(tr("工作区干净"))
            .into_any_element();
    }
    let tree = build_git_tree(chat);
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    collect_rows(chat, &weak, &tree, 0, t, &mut rows);
    div().flex().flex_col().children(rows).into_any_element()
}

fn collect_rows(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    nodes: &[GitNode],
    depth: usize,
    t: &'static Theme,
    out: &mut Vec<gpui::AnyElement>,
) {
    for node in nodes {
        match node {
            GitNode::Dir { name, children } => {
                out.push(
                    div()
                        .id(SharedString::from(format!("gd-{depth}-{name}")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .pl(px(8. + depth as f32 * 14.))
                        .py(px(4.5))
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text))
                        .child(icon_hover("folder-open", 15., t.text_muted))
                        .child(SharedString::from(name.clone()))
                        .into_any_element(),
                );
                collect_rows(chat, weak, children, depth + 1, t, out);
            }
            GitNode::File { ix } => {
                let f = &chat.git_files[*ix];
                let name = f
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| f.path.to_string_lossy().to_string());
                let staged = f.staged;
                let untracked = f.status == crate::services::git::GitStatus::Untracked;
                let selected = chat.git_selected.as_deref() == Some(f.path.as_path());
                let weak_row = weak.clone();
                let path = f.path.clone();
                out.push(
                    div()
                        .id(SharedString::from(format!("gr-{ix}")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .pl(px(8. + depth as f32 * 14.))
                        .pr(px(8.))
                        .py(px(4.5))
                        .text_size(crate::appearance::ui_size(11.))
                        .cursor_pointer()
                        .when(selected, |d| {
                            d.bg(rgb(t.bg_hover))
                                .border_1()
                                .border_color(rgb(t.accent))
                        })
                        .when(!selected, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                        // 行点击 = 选中（View Diff 目标）
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_row.update(cx, |c, cx| {
                                c.git_selected = Some(path.clone());
                                cx.notify();
                            });
                        })
                        .child(if untracked {
                            // 未跟踪：绿 + 徽标
                            icon("plus", 15., crate::theme::UNREAD)
                        } else {
                            icon("file", 15., t.text_muted)
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(name)),
                        )
                        .child(checkbox(staged, weak.clone(), *ix, t))
                        .into_any_element(),
                );
            }
        }
    }
}

fn checkbox(
    checked: bool,
    weak: gpui::WeakEntity<Chat>,
    ix: usize,
    t: &'static Theme,
) -> impl gpui::IntoElement {
    div()
        .id(SharedString::from(format!("gcb-{ix}")))
        .size(px(14.))
        .rounded(px(3.))
        .border_1()
        .border_color(rgb(if checked { t.accent } else { t.border }))
        .bg(rgb(if checked { t.accent } else { t.bg }))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .children(checked.then(|| icon("check", 10., t.accent_contrast)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| c.git_toggle_stage(ix, cx));
        })
}

/// History: 提交列表（标题 + ↑ 推送小钮；元信息：作者 · 时间 · hash）。
fn history_body(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let mut list = div()
        .id("git-history")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px(px(6.))
        .py(px(4.));
    if chat.git_log.is_empty() {
        list = list.child(
            div()
                .px(px(8.))
                .py(px(6.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(tr("暂无提交")),
        );
    }
    for c in &chat.git_log {
        let subject: SharedString = c.subject.clone().into();
        let meta: SharedString = format!("{} · {} · {}", c.author, c.date, c.hash).into();
        let weak_push = weak.clone();
        list = list.child(
            div()
                .id(SharedString::from(format!("gc-{}", c.hash)))
                .px(px(8.))
                .pt(px(8.))
                .pb(px(9.))
                .border_b_1()
                .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x4d)))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(7.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(subject),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("gc-push-{}", c.hash)))
                                .size(px(17.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(t.border))
                                .text_color(rgb(t.text_dim))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = weak_push.update(cx, |c, cx| c.git_push_branch(cx));
                                })
                                .child(icon("arrow-up", 10., t.text_dim)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .mt(px(4.))
                        .pl(px(2.))
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_dim))
                        .child(div().size(px(12.)).rounded_full().border_1().border_color(rgb(t.border)).bg(rgb(t.bg)))
                        .child(meta),
                ),
        );
    }
    list.into_any_element()
}

// ---------------------------------------------------------------------------
// Chat actions
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

    pub(crate) fn git_push_branch(&mut self, cx: &mut Context<Self>) {
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
