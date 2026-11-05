//! gitPanel (v54 设计版): 头部单行（项目名 + Changes(N)/History tabs 右靠，
//! 激活连体）· Changes（View Diff + Stage All ∨ / 变更树复选 / 底部
//! ⎇branch + ↑N Push / commit message 融入式大区 / Commit Tracked ∨ /
//! 最近提交条 + uncommit）· History（提交列表：标题 + ↑ 推送小钮 + 元信息）。
//!
//! itlq.12 超规拆分后本文件只剩壳层与跨页共享 actions：Changes 页 =
//! `changes`（含 stage/commit 动作），History 页 = `history`。

use std::path::PathBuf;

use gpui::{MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::services::git;
use crate::*;

mod changes;
mod history;

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
        .project()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| tr("statusbar.no_project").to_string())
        .into();
    let n_changes = chat.ws.git_files.len();
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
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
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
                    div()
                        .flex()
                        .child(git_tab(
                            GitTab::Changes,
                            &changes_label,
                            chat.ws.git_tab,
                            weak.clone(),
                            cx,
                        ))
                        .child(git_tab(
                            GitTab::History,
                            "History",
                            chat.ws.git_tab,
                            weak.clone(),
                            cx,
                        )),
                ),
        );

    col = match chat.ws.git_tab {
        GitTab::Changes => col.child(changes::changes_body(chat, weak, cx)),
        GitTab::History => col.child(history::history_body(chat, weak, cx)),
    };

    if let Some(err) = &chat.ws.git_error {
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
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .rounded_tl(px(7.))
                .rounded_tr(px(7.))
                .text_color(rgb(t.text))
                .font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .when(!active, |d| {
            d.text_color(rgb(t.text_muted))
                .hover(|s| s.text_color(rgb(t.text)))
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| c.git_set_tab(tab, cx));
        })
        .child(SharedString::from(label.to_string()))
}

// ---------------------------------------------------------------------------
// Chat actions（跨页共享 + 面板生命周期）
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn git_set_tab(&mut self, tab: GitTab, cx: &mut gpui::Context<Self>) {
        self.ws.git_tab = tab;
        if tab == GitTab::History && self.ws.git_log.is_empty() {
            self.refresh_git_log();
        }
        cx.notify();
    }

    pub(crate) fn refresh_git_log(&mut self) {
        // 无项目态：无提交历史可言
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        self.ws.git_log = git::git_log(&cwd, 50);
    }

    pub(crate) fn git_push_branch(&mut self, cx: &mut Context<Self>) {
        self.ws.git_error = Some(tr("git.pushing").to_string());
        cx.notify();
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            // push is network-bound: keep it off the frame path
            let r = cx
                .background_spawn(async move { git::git_push(&cwd) })
                .await;
            let _ = this.update(cx, |chat, cx| {
                chat.ws.git_error = r.err().or_else(|| Some(tr("git.pushed").to_string()));
                cx.notify();
            });
        })
        .detach();
    }
}

impl Chat {
    /// git status/numstat 查询（pi-flash-9rk0：git 后台化，它本来就不是
    /// 主角）：两个同步 git 子进程（暖盘各 ~90ms）曾是切项目/启动主线程
    /// 冻结的主体——后台跑完换入，面板数据晚 ~150ms 无感。请求序号 + cwd
    /// 双守卫：换项目或 fs watch 合批泵又发了一次刷新时，这次的陈旧结果
    /// 直接作废（最后一次说了算）。换项目瞬间先清旧项目的 git 面，数据
    /// 换入前不留上一个项目的痕迹。
    pub(crate) fn refresh_git(&mut self, cx: &mut Context<Self>) {
        self.ws.git_refresh_gen += 1;
        let seq = self.ws.git_refresh_gen;
        // 无项目态：清空 git 面（无工作区即无状态），照常走换入守卫
        let Some(cwd) = self.ws.cwd.clone() else {
            if !self.ws.git_files.is_empty() || self.ws.git_add_del != (0, 0) {
                self.ws.git_files.clear();
                self.ws.git_add_del = (0, 0);
                self.ws.git_files_cwd = PathBuf::new();
                cx.notify();
            }
            return;
        };
        if self.ws.git_files_cwd != cwd {
            self.ws.git_files_cwd = cwd.clone();
            self.ws.git_files.clear();
            self.ws.git_add_del = (0, 0);
        }
        let cwd_bg = cwd.clone();
        cx.spawn(async move |this, cx| {
            let (files, add_del) = cx
                .background_spawn(async move { (git_status_files(&cwd_bg), git_numstat(&cwd_bg)) })
                .await;
            let _ = this.update(cx, |chat, cx| {
                if chat.ws.git_refresh_gen != seq || chat.ws.cwd.as_deref() != Some(cwd.as_path()) {
                    return;
                }
                chat.ws.git_files = files;
                chat.ws.git_add_del = add_del;
                chat.rebuild_tree();
                cx.notify();
            });
        })
        .detach();
    }
}
