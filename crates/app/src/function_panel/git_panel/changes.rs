//! git 面板 Changes 页（itlq.12 自 git_panel.rs 切出，纯结构搬移）：
//! View Diff + Stage All 操作行、变更树（目录嵌套 + 文件复选，GitNode
//! 拍平渲染）、底部 branch/Push + 融入式 commit message 大区 +
//! Commit Tracked + 最近提交条。

use std::path::{Path, PathBuf};

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::services::git;
use crate::ui::{icon, icon_hover};

/// 变更树节点（目录嵌套 + 文件复选）。
#[derive(Debug, Clone)]
enum GitNode {
    Dir {
        name: String,
        children: Vec<GitNode>,
    },
    File {
        ix: usize,
    },
}

fn build_git_tree(chat: &Chat) -> Vec<GitNode> {
    // stable copy with indices into chat.ws.git_files
    let mut files: Vec<(PathBuf, usize)> = chat
        .ws
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
            .strip_prefix(chat.ws.cwd.as_deref().unwrap_or(Path::new("")))
            .unwrap_or(&path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        let segs: Vec<&str> = rel.iter().map(|s| s.as_str()).collect();
        insert(&mut roots, &segs, ix);
    }
    roots
}

pub(crate) fn changes_body(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let branch: SharedString = if chat.ws.branch.is_empty() {
        "no git".into()
    } else {
        chat.ws.branch.clone().into()
    };
    let ahead = chat
        .ws
        .cwd
        .as_deref()
        .map(git::git_ahead_count)
        .unwrap_or(0);
    let last_commit = chat.ws.git_log.first().cloned();

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
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                // 选中行优先，否则第一个变更
                                let target = this
                                    .ws
                                    .git_files
                                    .iter()
                                    .find(|f| Some(&f.path) == this.ws.git_selected.as_ref())
                                    .or_else(|| this.ws.git_files.first())
                                    .map(|f| f.path.clone());
                                if let Some(p) = target {
                                    this.open_git_diff(p, cx);
                                }
                            }),
                        )
                        .child(icon_hover("icon-viewdiff", 13., t.text_muted))
                        .child(SharedString::from(tr("git.view_diff"))),
                )
                .child(div().flex_1())
                .child(
                    crate::ui::button_base(
                        "git-stage-all",
                        crate::ui::BtnSize::Sm,
                        crate::ui::BtnVariant::Secondary,
                    )
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                            // 无项目态：没有可暂存的仓库，no-op
                            let Some(cwd) = this.ws.cwd.clone() else {
                                return;
                            };
                            this.ws.git_error = git::git_stage_all(&cwd).err();
                            this.refresh_git(cx);
                            cx.notify();
                        }),
                    )
                    .child(SharedString::from(tr("git.stage_all")))
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
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
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
                            crate::ui::button_base(
                                "git-push",
                                crate::ui::BtnSize::Sm,
                                crate::ui::BtnVariant::Secondary,
                            )
                            .ml_auto()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                    this.git_push_branch(cx);
                                }),
                            )
                            .child(icon_hover("arrow-up", 12., t.text))
                            .child(SharedString::from(format!("{ahead} Push")))
                            .child(icon(
                                "chevron-down",
                                10.,
                                t.text_dim,
                            )),
                        ),
                )
                // 融入式 commit message 大区（无边框）
                .child(chat.ws.git_commit_input.clone())
                .child(
                    div().flex().items_center().gap(px(8.)).child(
                        crate::ui::button_base(
                            "git-commit-btn",
                            crate::ui::BtnSize::Sm,
                            crate::ui::BtnVariant::Secondary,
                        )
                        .ml_auto()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.git_commit_staged(cx);
                            }),
                        )
                        .child(SharedString::from(tr("git.staged")))
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
                        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
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
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        // 无项目态：没有可撤销提交的仓库，no-op
                                        let Some(cwd) = this.ws.cwd.clone() else {
                                            return;
                                        };
                                        match git::git_uncommit(&cwd) {
                                            Ok(()) => {
                                                this.ws.git_error = None;
                                                this.refresh_git(cx);
                                                this.refresh_git_log();
                                            }
                                            Err(e) => this.ws.git_error = Some(e),
                                        }
                                        cx.notify();
                                    }),
                                )
                                .child(icon_hover("refresh", 13., t.text_muted)),
                        )
                })),
        )
        .into_any_element()
}

/// 变更树 section：目录嵌套（拍平行）+ 行尾复选。
fn section(chat: &Chat, weak: gpui::WeakEntity<Chat>, t: &'static Theme) -> gpui::AnyElement {
    if chat.ws.git_files.is_empty() {
        return div()
            .px(px(8.))
            .py(px(6.))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(tr("git.clean"))
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
                let f = &chat.ws.git_files[*ix];
                let name = f
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| f.path.to_string_lossy().to_string());
                let staged = f.staged;
                let untracked = f.status == crate::services::git::GitStatus::Untracked;
                let selected = chat.ws.git_selected.as_deref() == Some(f.path.as_path());
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
                            d.bg(rgb(t.bg_hover)).border_1().border_color(rgb(t.accent))
                        })
                        .when(!selected, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                        // 行点击 = 选中（View Diff 目标）
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_row.update(cx, |c, cx| {
                                c.ws.git_selected = Some(path.clone());
                                cx.notify();
                            });
                        })
                        .child(if untracked {
                            // 未跟踪：绿 + 徽标
                            icon("plus", 15., crate::appearance::theme::UNREAD)
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

impl Chat {
    fn git_toggle_stage(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let Some(f) = self.ws.git_files.get(ix) else {
            return;
        };
        let (path, staged) = (f.path.clone(), f.staged);
        let r = if staged {
            git::git_unstage(&cwd, &path)
        } else {
            git::git_stage(&cwd, &path)
        };
        self.ws.git_error = r.err();
        self.refresh_git(cx);
        cx.notify();
    }

    pub(crate) fn git_commit_staged(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let msg = self.ws.git_commit_input.read(cx).value().to_string();
        match git::git_commit(&cwd, &msg) {
            Ok(()) => {
                self.ws.git_error = None;
                self.ws
                    .git_commit_input
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
                self.refresh_git(cx);
                self.refresh_git_log();
            }
            Err(e) => self.ws.git_error = Some(e),
        }
        cx.notify();
    }
}
