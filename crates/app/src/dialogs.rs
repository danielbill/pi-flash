//! Modal dialogs: ModelSelect / GitDiff / SessionSearch / ImagePreview
//! (pi-web parity surfaces layered over the app root). Free function over
//! Chat state; entity split lands in phase E (ARCHITECTURE.md §2).

use std::path::PathBuf;

use gpui::{App, Div, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Dialog;
use crate::Chat;
use crate::TextInput;
use crate::{ComposerDown, ComposerUp, MODEL_PICKER_ROWS};
use crate::i18n::tr;
use crate::services::format::time_ago;
use crate::theme;
use crate::ui::{icon, icon_hover};

/// 弹窗公共外壳 = `ui::overlay::layer`（遮挡/外点关闭/ESC 关闭三条全局规则
/// 的唯一实现）+ 居中排布 + 卡片停传播。参数 `chat` 只用来取浮层焦点。
fn dialog_shell(chat: &Chat, weak: &gpui::WeakEntity<Chat>, panel: Div) -> Div {
    let weak_bg = weak.clone();
    crate::ui::overlay::layer(
        true,
        Some(&chat.dialog_focus),
        move |_w, cx| {
            let _ = weak_bg.update(cx, |c, cx| {
                c.dialog = None;
                cx.notify();
            });
        },
    )
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
            if let Some(Dialog::ModelSelect { input: filter_input, .. }) = chat.dialog.as_ref() {
                root = root.child(render_model_select(chat, weak, filter_input, t, cx));
            }
            if let Some(Dialog::GitDiff { path, patch }) = chat.dialog.as_ref() {
                root = root.child(render_git_diff(chat, weak, path, patch, t, cx));
            }
            if let Some(Dialog::SessionSearch { input }) = chat.dialog.as_ref() {
                root = root.child(render_session_search(chat, weak, input, t, cx));
            }
            if let Some(Dialog::ProjectPicker { input, fresh, scroll }) = chat.dialog.as_ref() {
                root = root.child(render_project_picker(chat, weak, input, *fresh, scroll, t, cx));
            }
            if let Some(Dialog::ImagePreview { image }) = chat.dialog.as_ref() {
                root = root.child(render_image_preview(chat, weak, image, t));
            }
            if let Some(Dialog::SessionInfo { kind }) = chat.dialog.as_ref() {
                root = root.child(crate::top_panels::session_info_dialog(
                    *kind,
                    chat,
                    weak,
                    t,
                    cx,
                ));
            }
            if let Some(Dialog::FileDirty { path }) = chat.dialog.as_ref() {
                root = root.child(render_file_dirty(chat, weak, path, t));
            }
            if let Some(Dialog::NewFile { input }) = chat.dialog.as_ref() {
                root = root.child(render_new_file(chat, weak, input, t));
            }
    root
}

/// 023 fileView：关闭带未保存修改的文件 tab 前确认（保存并关闭 / 不保存
/// 关闭 / 取消，对齐 Zed 的三选）。
fn render_file_dirty(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    path: &PathBuf,
    t: &theme::Theme,
) -> Div {
    let _ = chat;
    let weak_save = weak.clone();
    let weak_drop = weak.clone();
    let weak_cancel = weak.clone();
    let p_save = path.clone();
    let p_drop = path.clone();
    let name: SharedString = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .into();
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
            .when(!accent, |d| {
                d.border_1()
                    .border_color(gpui::rgba(theme::border_alpha(t, 0x8c)))
            })
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
            .rounded(px(12.))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(tr("未保存的修改").to_string())),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(
                        format!("{} {name}", tr("有未保存的修改：")),
                    )),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        btn("fv-dirty-cancel", tr("取消"), false).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_cancel.update(cx, |c, cx| {
                                    c.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(
                        btn("fv-dirty-drop", tr("不保存关闭"), false).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_drop.update(cx, |c, cx| {
                                    c.discard_file_tab(&p_drop, cx);
                                    c.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(
                        btn("fv-dirty-save", tr("保存并关闭"), true).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_save.update(cx, |c, cx| {
                                    c.save_file(&p_save, cx);
                                    c.discard_file_tab(&p_save, cx);
                                    c.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    ),
            ),
    )
}

/// 023 fileView：新建文件名字输入（Enter 提交 / Esc 取消，提交逻辑在
/// start_new_file 里挂的 on_submit/on_escape 上）。
fn render_new_file(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    t: &theme::Theme,
) -> Div {
    let _ = weak;
    dialog_shell(
        chat,
        weak,
        div()
            .w(px(380.))
            .p(px(18.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(theme::border_alpha(t, 0x8c)))
            .rounded(px(12.))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(tr("新建文件").to_string())),
            )
            .child(input.clone())
            .child(
                div()
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(
                        tr("在项目根下创建；Enter 确认，Esc 取消").to_string(),
                    )),
            ),
    )
}

/// ModelSelect dialog surface (extracted from render_dialogs).
fn render_model_select(chat: &Chat, weak: &gpui::WeakEntity<Chat>, filter_input: &gpui::Entity<TextInput>, t: &theme::Theme, cx: &App) -> Div {
                let sel = match &chat.dialog {
                    Some(Dialog::ModelSelect { sel, .. }) => *sel,
                    _ => 0,
                };
                let models = chat.filtered_models(cx);
                let rows: Vec<gpui::AnyElement> = models
                    .iter()
                    .enumerate()
                    .take(MODEL_PICKER_ROWS)
                    .map(|(ix, m)| {
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
                            .when(ix == sel, |d| d.bg(rgb(t.bg_selected)))
                            .hover(|s| s.bg(rgb(t.bg_selected)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let (p, mid) = (provider.clone(), id.clone());
                                let _ = weak_row.update(cx, |c, cx| {
                                    c.rt().update(cx, |r, cx| r.select_model(p, mid, cx));
                                    // picking is also the dismissal gesture
                                    c.dialog = None;
                                    cx.notify();
                                });
                            })
                            .flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(14.))
                                    .text_color(rgb(t.text))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_size(14.))
                                    .text_color(rgb(t.text_dim))
                                    .child(ctx),
                            )
                            .into_any_element()
                    })
                    .collect();
                let list_panel = if rows.is_empty() {
                    div()
                        .py_2()
                        .text_size(crate::appearance::ui_size(14.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("no models match"))
                        .into_any_element()
                } else {
                    div().flex().flex_col().gap_0p5().children(rows).into_any_element()
                };
                let panel = div()
                    .w(px(620.))
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
                                    .child(tr("选择模型")),
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
                                    .child(icon_hover("x", 12., t.text_muted)),
                            ),
                    )
                    .child(filter_input.clone())
                    .child(list_panel);
                // Enter: caught at the overlay as a bubbled key event — the
                // same layer the ESC and ↑/↓ handling lives on (all three
                // proven paths; the filter input's PressEnter subscription
                // chain did not fire reliably). InputState::enter propagates
                // the keystroke in single-line mode, so the event reaches
                // this handler.
                dialog_shell(chat, weak, panel)
                    .on_key_down({
                        let weak = weak.clone();
                        move |ev: &KeyDownEvent, _w, cx| {
                            if ev.keystroke.key != "enter" {
                                return;
                            }
                            cx.stop_propagation();
                            // applying drops the dispatching entities
                            // (dialog = None) — defer out of the dispatch
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                let _ = weak.update(cx, |c, cx| c.apply_model_sel(cx));
                            });
                        }
                    })
                    .on_action({
                        let weak = weak.clone();
                        move |_: &ComposerUp, _w, cx| {
                            let _ = weak.update(cx, |c, cx| c.move_model_sel(-1, cx));
                        }
                    })
                    .on_action({
                        let weak = weak.clone();
                        move |_: &ComposerDown, _w, cx| {
                            let _ = weak.update(cx, |c, cx| c.move_model_sel(1, cx));
                        }
                    })
}


/// 013 sessionSearchDialog + sessionSearchResultView: query on top, results
/// grouped by session below; a row click switches sessions and reveals the
/// GitDiff dialog surface (extracted from render_dialogs).
fn render_git_diff(chat: &Chat, weak: &gpui::WeakEntity<Chat>, path: &PathBuf, patch: &String, t: &theme::Theme, _cx: &App) -> Div {
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
                                                .child(icon_hover("git-branch", 12., t.accent))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_family(crate::markdown::MONO_FAMILY)
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
                                                .child(icon_hover("x", 12., t.text_muted)),
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
                                        .font_family(crate::markdown::MONO_FAMILY)
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
            c.dialog = None;
            cx.notify();
        });
    });
    let panel = div()
        .relative()
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded_lg()
        .p_2()
        .shadow_lg()
        .child(
            gpui::img(image.clone())
                .max_w(px(1040.))
                .max_h(px(680.))
                .rounded(px(6.)),
        )
        .child(div().absolute().top(px(6.)).right(px(6.)).child(close));
    dialog_shell(chat, weak, panel)
}


/// 004 projectManager 打开项目菜单：500×500 居中卡片——顶部搜索框，
/// 【打开文件夹】行（走 psp 目录选择器），下面是最近 30 天活动项目列表
/// （字母序、行高 40、吃满剩余高度超出滚动）。当前项目行尾打勾（004：从某项目
/// 新建会话打开时默认选中）。列表数据 `project_hits` 由打开器后台扫描
/// 回填，本函数只做搜索词过滤。
fn render_project_picker(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    fresh: bool,
    scroll: &gpui::ScrollHandle,
    t: &theme::Theme,
    _cx: &App,
) -> Div {
    // 004 v3：列表吃满弹窗剩余高度（面板固定 500，容纳几条就显示几条），
    // 超出出滚动条——不再按固定行数限高
    const ROW_H: f32 = 40.;
    let needle = chat.project_filter.trim().to_lowercase();
    let rows: Vec<&crate::ProjectEntry> = chat
        .project_hits
        .iter()
        .filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle))
        .collect();
    let mut list = div().flex().flex_col();
    if rows.is_empty() {
        let empty = if chat.project_hits.is_empty() {
            tr("最近 30 天没有打开过的项目")
        } else {
            tr("没有匹配的项目")
        };
        list = list.child(
            div()
                .py_4()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(empty),
        );
    }
    for (ix, p) in rows.iter().enumerate() {
        let path = p.path.clone();
        let weak_row = weak.clone();
        let name: SharedString = p.name.clone().into();
        let selected = crate::services::workspace::same_ws(
            &p.path.to_string_lossy(),
            &chat.cwd.to_string_lossy(),
        );
        list = list.child(
            div()
                .id(SharedString::from(format!("proj-{ix}")))
                .h(px(ROW_H))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                // fresh（新会话页来源）= 落全新草稿，不恢复 last_open——004
                // 选项目是为了在这个项目里开新会话；psp 来源保持切项目
                // 恢复上次会话的既定行为
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| {
                        if fresh {
                            c.new_session_in(path.clone(), cx);
                        } else {
                            c.switch_project(path.clone(), cx);
                        }
                    });
                })
                .child(icon("folder", 16., t.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(crate::appearance::ui_size(13.))
                        .text_color(rgb(t.text))
                        .child(name),
                )
                .when(selected, |d| d.child(icon("check", 14., t.accent))),
        );
    }
    // 打开文件夹 = 目录选择器（psp 同款；无 30 天活动项目时的主入口）；
    // 落点跟随 fresh：新会话页来源选完落新草稿，不恢复该目录的上次会话
    let weak_open = weak.clone();
    let open_folder = div()
        .id("proj-open-folder")
        .h(px(ROW_H))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(crate::appearance::ui_size(13.))
        .text_color(rgb(t.text))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak_open.update(cx, |c, cx| c.pick_project_folder(fresh, cx));
        })
        .child(icon("folder-plus", 16., t.text_muted))
        .child(tr("打开文件夹"));
    let panel = div()
        .w(px(500.))
        .h(px(500.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .shadow_lg()
        .child(input.clone())
        .child(open_folder)
        // 打开文件夹与项目列表之间的分隔线（用户 2026-10-07 定稿）
        .child(div().h(px(1.)).w_full().bg(gpui::rgba(crate::theme::border_alpha(t, 0x66))))
        // 列表吃满剩余高度：flex_1 + min_h_0 才会真的收缩滚动（工具定义
        // 列表同款）；滚动条 = ZED Regular 移植（可滚动即常显），absolute
        // 盖在滚动容器右缘、不随内容滚
        .child(
            div()
                .relative()
                .mt_1()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .id("project-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .flex()
                        .flex_col()
                        .pr(px(6.))
                        .child(list),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(8.))
                        .child(crate::ui::psp_scrollbar::menu_scrollbar(scroll)),
                ),
        );
    dialog_shell(chat, weak, panel)
}


/// matched message (pi-web SessionSearch, dialog-mounted per 013).
fn render_session_search(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    t: &theme::Theme,
    _cx: &App,
) -> Div {
    let status: SharedString = if chat.search_running {
        tr("搜索中…").into()
    } else if chat.search_needle.is_empty() {
        tr("输入关键词，搜索当前项目的会话内容").into()
    } else if chat.search_hits.is_empty() {
        tr("没有匹配结果").into()
    } else {
        format!("{} 条结果", chat.search_hits.len()).into()
    };
    let mut results = div().flex().flex_col();
    let mut last_session: Option<PathBuf> = None;
    for (hit_ix, hit) in chat.search_hits.iter().enumerate() {
        if last_session.as_ref() != Some(&hit.session_path) {
            last_session = Some(hit.session_path.clone());
            let age = time_ago(hit.modified);
            let label: SharedString = hit
                .session_name
                .clone()
                .unwrap_or_else(|| {
                    if hit.preview.is_empty() {
                        hit.session_path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    } else {
                        hit.preview.clone()
                    }
                })
                .into();
            results = results.child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(SharedString::from(label))
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(10.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(age)),
                    ),
            );
        }
        let path = hit.session_path.clone();
        let ts = hit.ts;
        let weak_row = weak.clone();
        results = results.child(
            div()
                .id(SharedString::from(format!("hit-{hit_ix}")))
                .px_3()
                .py_1p5()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let path = path.clone();
                    let _ = weak_row.update(cx, |c, cx| c.jump_to_hit(path, ts, cx));
                })
                .child(
                    div()
                        .text_xs()
                        .line_height(relative(1.5))
                        .text_color(rgb(t.text_muted))
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .gap_1()
                        .child(SharedString::from(hit.before.clone()))
                        .child(
                            div()
                                .px_0p5()
                                .rounded(px(3.))
                                .bg(gpui::hsla(
                                    0., 0., 0.5, 0.15,
                                ))
                                .text_color(rgb(t.accent))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(SharedString::from(hit.match_text.clone())),
                        )
                        .child(SharedString::from(hit.after.clone())),
                ),
        );
    }
    let panel = div()
                    .w(px(620.))
                    .max_h(px(640.))
                    .bg(rgb(t.bg_panel))
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(px(8.))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(input.clone())
                    .child({
                        let label: SharedString = if chat.search_truncated {
                            format!("{status} · {partial}", partial = tr("部分结果")).into()
                        } else {
                            status
                        };
                        div()
                            .px_1()
                            .text_xs()
                            .text_color(rgb(t.text_dim))
                            .child(label)
                    })
        .child(
            div()
                .id("search-results")
                .max_h(px(520.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(results),
        );
    dialog_shell(chat, weak, panel)
}
