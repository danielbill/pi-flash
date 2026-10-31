//! 模态弹窗域（090 重构自顶层 dialogs.rs 目录化）：外壳 [`dialog_shell`] +
//! 分发器 [`render_dialogs`]，小弹窗（文件脏关闭确认 / Git diff / 图片预览）
//! 留本文件；两个大 picker 拆 provider_picker.rs / project_picker.rs；
//! ask_card（整卷问卷卡）与 ext_ui（扩展阻塞交互件）同域归队。
//! （pi-web parity surfaces layered over the app root.）
use crate::*;

pub(crate) mod ask_card;
pub(crate) mod ext_ui;
mod project_picker;
mod provider_picker;

use project_picker::render_project_picker;
use provider_picker::render_provider_picker;

use std::path::PathBuf;

use gpui::{App, Div, MouseButton, SharedString, div, px, rgb};

use crate::Chat;
use crate::Dialog;
use crate::appearance::theme;
use crate::i18n::{tf, tr};
use crate::ui::icon_hover;

/// 弹窗公共外壳 = `ui::overlay::layer`（遮挡/外点关闭/ESC 关闭三条全局规则
/// 的唯一实现）+ 居中排布 + 卡片停传播。参数 `chat` 只用来取浮层焦点。
fn dialog_shell(chat: &Chat, weak: &gpui::WeakEntity<Chat>, panel: Div) -> Div {
    let weak_bg = weak.clone();
    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_bg.update(cx, |c, cx| {
            c.dlg.dialog = None;
            cx.notify();
        });
    })
    .flex()
    .items_center()
    .justify_center()
    .child(crate::ui::overlay::stop_click(panel))
}

pub(crate) fn render_dialogs(
    mut root: Div,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
    cx: &App,
) -> Div {
    // dialogs mount as a CHILD of the chat root — on top of the content,
    // never replacing it (replacing the root blanks the whole UI behind
    // the dialog; settings parity = content stays visible beneath)
    if let Some(Dialog::ProviderPicker { input, scroll }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_provider_picker(chat, weak, input, scroll, t, cx));
    }
    if let Some(Dialog::GitDiff { path, patch }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_git_diff(chat, weak, path, patch, t, cx));
    }
    if let Some(Dialog::ProjectPicker {
        input,
        fresh,
        scroll,
    }) = chat.dlg.dialog.as_ref()
    {
        root = root.child(render_project_picker(
            chat, weak, input, *fresh, scroll, t, cx,
        ));
    }
    if let Some(Dialog::ImagePreview { image }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_image_preview(chat, weak, image, t));
    }
    if let Some(Dialog::SessionInfo { kind }) = chat.dlg.dialog.as_ref() {
        root = root.child(crate::top_panels::session_info_dialog(
            *kind, chat, weak, t, cx,
        ));
    }
    if let Some(Dialog::FileDirty { paths }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_file_dirty(chat, weak, paths, t));
    }
    root
}

/// 023 fileView：关闭带未保存修改的文件 tab 前确认（保存并关闭 / 不保存
/// 关闭 / 取消，对齐 Zed 的三选）。
fn render_file_dirty(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    paths: &[PathBuf],
    t: &theme::Theme,
) -> Div {
    let _ = chat;
    let weak_save = weak.clone();
    let weak_drop = weak.clone();
    let weak_cancel = weak.clone();
    let p_save: Vec<PathBuf> = paths.to_vec();
    let p_drop: Vec<PathBuf> = paths.to_vec();
    // 单文件照旧显示文件名；批量（标签右键 Close Others/All 收拢）显示
    // 数量 + 前 3 个名字（zed「N unsaved」同款语义，三键对整批生效）
    let name: SharedString = match paths {
        [p] => p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
            .into(),
        many => {
            let shown: Vec<String> = many
                .iter()
                .take(3)
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.display().to_string())
                })
                .collect();
            let more = if many.len() > 3 { " …" } else { "" };
            format!(
                "{} {}{}",
                tf("conflict.unsaved_n_files", &[("n", many.len().to_string())]),
                shown.join("、"),
                more
            )
            .into()
        }
    };
    let btn = |id: &'static str, label: &'static str, accent: bool| {
        div()
            .id(id)
            .px(px(12.))
            .py(px(5.))
            .rounded(px(7.))
            .text_size(crate::appearance::ui_size(12.))
            .cursor_pointer()
            .when(accent, |d| {
                d.bg(rgb(t.accent)).text_color(rgb(t.accent_contrast))
            })
            .when(!accent, |d| d.border_1().border_color(rgb(t.border)))
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .child(SharedString::from(label.to_string()))
    };
    dialog_shell(
        chat,
        weak,
        div()
            .w(px(380.))
            .p(px(18.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(theme::border_alpha(t, 0x8c)))
            .rounded(px(10.))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(tr("conflict.unsaved_count").to_string())),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(format!(
                        "{} {name}",
                        tr("conflict.unsaved_list")
                    ))),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        btn("fv-dirty-cancel", tr("action.cancel"), false).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_cancel.update(cx, |c, cx| {
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(
                        btn("fv-dirty-drop", tr("conflict.close_without_saving"), false)
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                let _ = weak_drop.update(cx, |c, cx| {
                                    // 不保存关闭 = 整批丢弃（批量收拢时对全部
                                    // 脏文件生效；单关时集合只有一个）
                                    for p in &p_drop {
                                        c.discard_file_tab(p, cx);
                                    }
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        btn("fv-dirty-save", tr("conflict.save_and_close"), true).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_save.update(cx, |c, cx| {
                                    for p in &p_save {
                                        c.save_file(p, cx);
                                        c.discard_file_tab(p, cx);
                                    }
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    ),
            ),
    )
}

/// 013 sessionSearchDialog + sessionSearchResultView: query on top, results
/// grouped by session below; a row click switches sessions and reveals the
/// GitDiff dialog surface (extracted from render_dialogs).
fn render_git_diff(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    path: &PathBuf,
    patch: &String,
    t: &theme::Theme,
    _cx: &App,
) -> Div {
    let path_text: SharedString = path.to_string_lossy().to_string().into();
    let mut body = patch.clone();
    if body.chars().count() > 60000 {
        body = body.chars().take(60000).collect();
        body.push_str("\n\n\u{2026} (truncated)");
    }
    let panel = div()
        .w(px(760.))
        .max_h(px(640.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(10.))
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
                        .child(icon_hover("git-branch", 12., t.accent))
                        .child(
                            div()
                                // §2 mono 辅助标签档（原 text_xs 裸字号）
                                .text_size(crate::appearance::ui_size(11.))
                                .font_family(crate::editor::markdown::MONO_FAMILY)
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
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            }
                        })
                        .child(icon_hover("x", 13., t.text_muted)),
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
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text))
                .child(SharedString::from(body)),
        );
    dialog_shell(chat, weak, panel)
}

/// composer 缩略图点击大图预览（v58）：dialog_shell 金标准外壳（点外关闭
/// /ESC/遮挡），图片居中按 max 限宽高等比缩放（messages.rs 结果图同款，
/// img 尊重 max 约束）；无头部控件——ESC 或点击弹窗外任意处关闭。
fn render_image_preview(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    image: &std::sync::Arc<gpui::Image>,
    t: &theme::Theme,
) -> Div {
    let weak_close = weak.clone();
    // 图片预览也要有看得见的关闭按钮（浮层规则 5）：× 绝对定位在卡片右上角
    let close = crate::ui::overlay::close_btn("image-preview-close", t, move |_w, cx| {
        let _ = weak_close.update(cx, |c, cx| {
            c.dlg.dialog = None;
            cx.notify();
        });
    });
    let panel = div()
        .relative()
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(10.))
        .p_2()
        .shadow_lg()
        .child(crate::session::messages::fitted_image(image.clone(), 1040., 680.).rounded(px(6.)))
        .child(div().absolute().top(px(6.)).right(px(6.)).child(close));
    dialog_shell(chat, weak, panel)
}

// ---------------------------------------------------------------------------
// 弹窗打开器 + 会话搜索（P2 归并自 actions_dialogs.rs）
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn open_git_diff(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // 无项目态：没有 git 仓库可 diff（git 面本为空，防御）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let untracked = self
            .ws
            .git_files
            .iter()
            .any(|f| f.path == path && f.status == GitStatus::Untracked);
        let patch = git_file_diff(&cwd, &path, untracked);
        self.dlg.dialog = Some(Dialog::GitDiff { path, patch });
        cx.notify();
    }

    /// Jump to a search hit: open (or switch to) the session, then reveal the
    /// matched row — located by payload timestamp, falling back to the first
    /// message containing the needle.
    pub(crate) fn jump_to_hit(&mut self, path: PathBuf, ts: Option<i64>, cx: &mut Context<Self>) {
        let needle = self.psp.search_needle.clone();
        let key = path.to_string_lossy().to_string();
        let already_open = self.pool.active_key == key || self.pool.runtimes.contains_key(&key);
        if self.pool.active_key != key {
            self.open_session(path.clone(), false, cx);
        }
        let rt = self.rt();
        let located = rt.update(cx, |r, cx| r.locate_message(ts, &needle, cx));
        if !located && !already_open {
            // messages still loading (pool miss) — apply after the reconcile
            self.psp.pending_locate = Some((path, ts, needle));
        }
        cx.notify();
    }

    /// 会话信息弹窗（系统提示词 / 工具定义，窗体 = 设置弹窗那套大卡片）
    /// 开关。入口两处（2026-10-16）：会话功能栏保留项之外，主入口迁到
    /// 上下文用量面板的入口行（session/input/ctx_tip.rs `ctx_actions_row`）。
    /// 同一面再点一次收起，换面直接切。
    ///
    /// 数据不在这里拉：pi 0.86+ 把系统提示词与工具声明写进 transcript 的
    /// system 消息，runtime 每次 get_messages 都 replay 一份（见
    /// `pi_link::transcript::transcript_system`），所以打开即是最新。
    pub(crate) fn open_session_info(&mut self, kind: crate::TopPanel, cx: &mut Context<Self>) {
        self.dlg.dialog = if self.session_info_open(kind) {
            None
        } else {
            Some(Dialog::SessionInfo { kind })
        };
        cx.notify();
    }

    /// 该面的弹窗是否正开着（会话功能栏点亮用）。
    pub(crate) fn session_info_open(&self, kind: crate::TopPanel) -> bool {
        matches!(self.dlg.dialog, Some(Dialog::SessionInfo { kind: k }) if k == kind)
    }
}
