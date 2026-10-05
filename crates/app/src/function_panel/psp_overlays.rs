//! psp 悬浮层 (v54): 项目路径 tooltip（深色，跟随鼠标）、会话 hover 详情卡
//! （侧栏右缘外 8px 锚定，改名/删除流）、⋯ 两级菜单（列表方式/排序方式带
//! 子菜单勾选态）、项目菜单、删除项目确认。全部挂在根渲染，事件坐标定位。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::i18n::{tr, tf};
use crate::services::format::time_ago;
use crate::services::workspace::same_path;
use crate::theme::{Theme, danger_wash, theme as T};
use crate::ui::icon;

/// 根渲染挂载的 psp 悬浮层合集（tooltip / 详情卡 / 菜单 / 确认）。
pub(crate) fn psp_overlays(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    // 全窗口定位层：absolute inset_0 铺满 root，悬浮子元素相对它定位，
    // 且不参与流布局（0 高容器会把 absolute 子元素裁掉——菜单曾因此不可见）。
    // **父层不得 occlude**：inset_0 的层一挡全窗口 hit-test 都断（行悬停
    // 背景闪烁的根因）；各浮层在自己的 bounds 内各自 occlude。
    let mut el = div()
        .id("psp-overlays")
        .absolute()
        .inset_0();
    if let Some((path, x, y)) = &chat.proj_tip {
        el = el.child(proj_tip(path, *x, *y));
    }
    if let Some(card) = &chat.hover_card {
        // 延迟显示（0.3s）：快速滑过会话列表时不创建视觉，不干扰行悬停
        if card.shown {
            el = el.child(hover_card(chat, t, cx));
        }
    }
    if chat.psp_menu.is_some() {
        el = el.child(menu_layer(chat, t, cx));
    }
    if let Some((path, x, y)) = chat.confirm_prj_del.clone() {
        el = el.child(confirm_project_del(&path, x, y, t, cx));
    }
    el.into_any_element()
}

/// 项目行 tooltip：短名 + 全路径（mono），深色底，跟随鼠标。
fn proj_tip(path: &std::path::PathBuf, x: f32, y: f32) -> impl gpui::IntoElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let full = path.to_string_lossy().to_string();
    div()
        .id("psp-proj-tip")
        .absolute()
        .left(px(x + 14.))
        .top(px(y + 16.))
        .max_w(px(300.))
        .px(px(11.))
        .py(px(8.))
        .rounded(px(8.))
        .bg(rgb(0x22312d))
        .shadow_lg()
        .flex()
        .flex_col()
        .gap(px(1.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(0xeef4f1))
                .child(SharedString::from(name)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(10.5))
                .font_family(crate::markdown::MONO_FAMILY)
                .text_color(rgb(0x9fbcb2))
                .child(SharedString::from(full)),
        )
}

/// 会话 hover 详情卡：258px 浅色浮层，标题（点击=打开并改名）、项目/
/// 最后活动/消息数信息行、删除确认流。300ms 离行宽限 + 进卡取消。
fn hover_card(chat: &mut Chat, t: &'static Theme, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let card = chat.hover_card.clone().expect("hover_card checked by caller");
    let mut found: Option<(String, u64, std::time::SystemTime, String)> = None;
    for g in &chat.projects {
        if let Some(info) = g.sessions.iter().find(|s| same_path(&s.path, &card.path)) {
            let title = info
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| {
                    if info.preview.is_empty() {
                        "(empty)".to_string()
                    } else {
                        info.preview.clone()
                    }
                });
            found = Some((g.name.clone(), info.message_count, info.modified, title));
            break;
        }
    }
    let Some((prj_name, msgs, modified, title)) = found else {
        return div().into_any_element();
    };
    let path = card.path.clone();
    let confirming = card.confirming;
    let anchor_x = chat.slp_w + 8.;
    let title_for_rename = title.clone();

    let mut el = div()
        .id("psp-hcard")
        .absolute()
        .left(px(anchor_x))
        // card.y = 悬停行的真实顶部（prepaint bounds 校正），卡与行对齐
        .top(px(card.y))
        .w(px(258.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x8c)))
        .rounded(px(12.))
        .shadow_lg()
        // 遮挡只作用于卡片自身 bounds（父层 occlude 会断全窗口 hit-test）
        .occlude()
        .px(px(13.))
        .pt(px(11.))
        .pb(px(9.))
        .flex()
        .flex_col()
        // 卡内点击不落穿（改名/删除按钮依赖）
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        // 进卡取消隐藏 / 离卡进入宽限
        .on_hover(cx.listener(|this, h: &bool, _w, cx| {
            if let Some(card) = this.hover_card.as_mut() {
                card.card_hovered = *h;
                card.hide_at = (!*h).then(std::time::Instant::now);
            }
            cx.notify();
        }))
        .children(if card.renaming {
            // 设计稿：标题原地变输入框（Enter 提交 / Esc 取消）
            card.rename_input.clone().map(|inp| {
                div().mt(px(2.)).child(inp).into_any_element()
            })
        } else {
            Some(
            div()
                .id("hc-name")
                .text_size(crate::appearance::ui_size(13.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .cursor_pointer()
                .px(px(5.))
                .py(px(1.))
                .mx(px(-5.))
                .my(px(-1.))
                .rounded(px(6.))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                // 点击标题 = 原地进入改名（不切换会话）
                .on_mouse_down(MouseButton::Left, cx.listener(
                    move |this, _: &gpui::MouseDownEvent, window, cx| {
                        let Some(card) = this.hover_card.as_mut() else { return };
                        let path = card.path.clone();
                        let prefill = title_for_rename.clone();
                        let weak_ok = cx.entity().downgrade();
                        let weak_esc = weak_ok.clone();
                        let input = cx.new(|cx| {
                            crate::ui::TextInput::new(cx).select_all_on_focus()
                        });
                        input.update(cx, |ti, cx| ti.set_value(prefill, cx));
                        input.update(cx, |ti, _cx| {
                            let weak_ok = weak_ok.clone();
                            let weak_esc = weak_esc.clone();
                            let path_ok = path.clone();
                            ti.set_on_submit(Box::new(move |v, cx| {
                                let _ = weak_ok.update(cx, |c, cx| {
                                    c.rename_card_commit(&path_ok, v.trim().to_string(), cx)
                                });
                            }));
                            let path_esc = path.clone();
                            ti.set_on_escape(Box::new(move |cx| {
                                let _ = weak_esc.update(cx, |c, cx| {
                                    c.rename_card_cancel(&path_esc, cx)
                                });
                            }));
                        });
                        let handle = input.read(cx).focus_handle();
                        window.focus(&handle);
                        if let Some(card) = this.hover_card.as_mut() {
                            card.renaming = true;
                            card.rename_input = Some(input);
                        }
                        cx.notify();
                    },
                ))
                .child(SharedString::from(title))
                .into_any_element(),
            )
        })
        .child(info_row("folder-closed", SharedString::from(prj_name), t))
        .child(info_row(
            "icon-clock-solid",
            SharedString::from(tf("{t}前", &[("t", time_ago(modified))])),
            t,
        ))
        .child(info_row(
            "message-square",
            SharedString::from(tf("{n} 条消息", &[("n", msgs.to_string())])),
            t,
        ))
        .child(
            div()
                .h(px(1.))
                .bg(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
                .mx(px(-13.))
                .mt(px(9.))
                .mb(px(7.)),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(6.))
                .children(if confirming {
                    // 原地换 取消/确认
                    vec![
                        ghost_btn("hc-cancel", tr("取消"), false, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                if let Some(c) = this.hover_card.as_mut() {
                                    c.confirming = false;
                                }
                                cx.notify();
                            },
                        ))
                        .into_any_element(),
                        solid_danger_btn("hc-ok", tr("确认"), {
                            let path = path.clone();
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.hover_card = None;
                                this.delete_session(path.clone(), cx);
                            })
                        })
                        .into_any_element(),
                    ]
                } else {
                    vec![ghost_btn("hc-del", tr("删除"), true, cx.listener(
                        |this, _: &gpui::MouseDownEvent, _w, cx| {
                            if let Some(c) = this.hover_card.as_mut() {
                                c.confirming = true;
                            }
                            cx.notify();
                        },
                    ))
                    .into_any_element()]
                }),
        );
    let _ = &mut el;
    el.into_any_element()
}

fn info_row(icon_name: &'static str, text: SharedString, t: &'static Theme) -> impl gpui::IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mt(px(7.5))
        .text_size(crate::appearance::ui_size(12.5))
        .text_color(rgb(t.text))
        .child(icon(icon_name, 15., t.text_muted))
        .child(text)
}

fn ghost_btn(
    id: &'static str,
    label: &str,
    danger: bool,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    let t = T();
    div()
        .id(id)
        .px(px(10.))
        .py(px(3.))
        .rounded(px(7.))
        .border_1()
        .border_color(if danger {
            gpui::rgba(crate::theme::danger_alpha(t, 0x73))
        } else {
            rgb(t.border).into()
        })
        .text_size(crate::appearance::ui_size(12.))
        .text_color(if danger { rgb(t.danger) } else { rgb(t.text_dim) })
        .cursor_pointer()
        .hover(|s| {
            if danger {
                s.bg(gpui::rgba(danger_wash(t)))
            } else {
                s.bg(rgb(t.bg_hover)).text_color(rgb(t.text))
            }
        })
        .on_mouse_down(MouseButton::Left, handler)
        .child(SharedString::from(label.to_string()))
}

fn solid_danger_btn(
    id: &'static str,
    label: &str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    let t = T();
    div()
        .id(id)
        .px(px(11.))
        .py(px(3.5))
        .rounded(px(7.))
        .bg(rgb(t.danger))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(0xffffff))
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .on_mouse_down(MouseButton::Left, handler)
        .child(SharedString::from(label.to_string()))
}

// ---------------------------------------------------------------------------
// ⋯ 两级菜单 + 项目菜单
// ---------------------------------------------------------------------------

type ApplyFn = Box<dyn Fn(&mut Chat, &mut gpui::Context<Chat>)>;

fn menu_layer(chat: &mut Chat, t: &'static Theme, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = match chat.psp_menu.clone().expect("checked by caller") {
        crate::PspMenu::Sort { x, y, .. } => (x, y),
        crate::PspMenu::Project { x, y, .. } => (x, y),
    };
    let mut layer = div()
        .id("psp-menu-layer")
        .absolute()
        .inset_0()
        .occlude()
        // 透明背板：点击任意处关闭
        .child(
            div()
                .absolute()
                .inset_0()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        this.psp_menu = None;
                        cx.notify();
                    },
                )),
        );
    let mut card = menu_card(x, y, t);
    match chat.psp_menu.clone().expect("re-checked") {
        crate::PspMenu::Sort { sub, .. } => {
            let mut view = menu_item("m-list", "icon-organize", tr("列表方式"), false, false);
            view = view.on_hover(cx.listener(|this, h: &bool, _w, cx| {
                if *h {
                    if let Some(crate::PspMenu::Sort { sub, .. }) = this.psp_menu.as_mut() {
                        *sub = Some(0);
                    }
                    cx.notify();
                }
            }));
            let mut sort = menu_item("m-sort", "icon-sort", tr("排序方式"), false, false);
            sort = sort.on_hover(cx.listener(|this, h: &bool, _w, cx| {
                if *h {
                    if let Some(crate::PspMenu::Sort { sub, .. }) = this.psp_menu.as_mut() {
                        *sub = Some(1);
                    }
                    cx.notify();
                }
            }));
            card = card
                .child(view.child(menu_chev(t)))
                .child(sort.child(menu_chev(t)));
            // 二级菜单（右侧弹出，锚定父项）
            if let Some(sub_ix) = sub {
                let mut sub_card = menu_card(x + 218., y + 4. + sub_ix as f32 * 34., t);
                let items: Vec<(&'static str, &str, bool, ApplyFn)> = if sub_ix == 0 {
                    vec![
                        (
                            "icon-grouped",
                            tr("项目分组列表"),
                            chat.list_mode == crate::ListMode::Grouped,
                            Box::new(|c, _cx| c.list_mode = crate::ListMode::Grouped),
                        ),
                        (
                            "icon-flat",
                            tr("最近会话列表"),
                            chat.list_mode == crate::ListMode::Flat,
                            Box::new(|c, _cx| c.list_mode = crate::ListMode::Flat),
                        ),
                    ]
                } else {
                    vec![
                        (
                            "icon-clock-solid",
                            tr("按更新时间排序"),
                            chat.sort_mode == crate::SortMode::Time,
                            Box::new(|c, _cx| c.sort_mode = crate::SortMode::Time),
                        ),
                        (
                            "icon-hand",
                            tr("手动排序"),
                            chat.sort_mode == crate::SortMode::Manual,
                            Box::new(|c, _cx| c.sort_mode = crate::SortMode::Manual),
                        ),
                    ]
                };
                for (i, (icon_name, label, active, apply)) in items.into_iter().enumerate() {
                    let weak = weak.clone();
                    let it = menu_item(
                        SharedString::from(format!("msub-{sub_ix}-{i}")),
                        icon_name,
                        label,
                        false,
                        active,
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = weak.update(cx, |c, cx| {
                            apply(c, cx);
                            c.persist_ui();
                            c.psp_menu = None;
                            cx.notify();
                        });
                    });
                    sub_card = sub_card.child(it);
                }
                layer = layer.child(sub_card);
            }
        }
        crate::PspMenu::Project { path, .. } => {
            card = card
                .child(
                    menu_item(
                        "pm-explorer",
                        "icon-filebrowser",
                        tr("在文件浏览器中打开"),
                        false,
                        false,
                    )
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        let path = path.clone();
                        move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak.update(cx, |c, cx| {
                                crate::actions_panels::open_in_explorer(&path);
                                c.psp_menu = None;
                                cx.notify();
                            });
                        }
                    }),
                )
                .child(
                    menu_item("pm-terminal", "icon-terminal-solid", tr("在终端中打开"), false, false)
                        .on_mouse_down(MouseButton::Left, {
                            let weak = weak.clone();
                            let path = path.clone();
                            move |_, window, cx| {
                                cx.stop_propagation();
                                let _ = weak.update(cx, |c, cx| {
                                    c.open_terminal_in(path.clone(), window, cx);
                                    c.dock_panel = crate::DockPanel::Files;
                                    c.persist_ui();
                                    c.psp_menu = None;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    menu_item(
                        "pm-del",
                        "icon-trash-solid",
                        tr("删除项目及所有会话"),
                        true,
                        false,
                    )
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        let path = path.clone();
                        move |ev: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
                            let _ = weak.update(cx, |c, cx| {
                                c.psp_menu = None;
                                c.confirm_prj_del = Some((path.clone(), x, y));
                                cx.notify();
                            });
                        }
                    }),
                );
        }
    }
    layer.child(card).into_any_element()
}

fn menu_card(x: f32, y: f32, t: &'static Theme) -> gpui::Div {
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .min_w(px(214.))
        .p(px(4.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x8c)))
        .rounded(px(9.))
        .shadow_lg()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
}

fn menu_item<S: Into<SharedString>>(
    id: S,
    icon_name: &'static str,
    label: &str,
    danger: bool,
    checked: bool,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(9.))
        .px(px(10.))
        .py(px(7.))
        .rounded(px(6.))
        .text_size(crate::appearance::ui_size(12.5))
        .text_color(if danger { rgb(t.danger) } else { rgb(t.text) })
        .cursor_pointer()
        .hover(|s| {
            if danger {
                s.bg(gpui::rgba(danger_wash(t)))
            } else {
                s.bg(rgb(t.bg_hover))
            }
        })
        .child(icon(icon_name, 15., if danger { t.danger } else { t.text_muted }))
        .child(div().flex_1().child(SharedString::from(label.to_string())))
        .children(checked.then(|| icon("check", 13., t.accent)))
}

fn menu_chev(t: &'static Theme) -> impl gpui::IntoElement {
    icon("chevron-right", 12., t.text_dim)
}

/// 删除项目确认浮层（锚定菜单点击处）。
fn confirm_project_del(
    path: &std::path::PathBuf,
    x: f32,
    y: f32,
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let path = path.clone();
    div()
        .id("psp-confirm-del")
        .absolute()
        .inset_0()
        .occlude()
        .child(
            div().absolute().inset_0().cursor_pointer().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.confirm_prj_del = None;
                    cx.notify();
                }),
            ),
        )
        .child(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .min_w(px(260.))
                .p(px(12.))
                .bg(rgb(t.bg))
                .border_1()
                .border_color(gpui::rgba(crate::theme::danger_alpha(t, 0x73)))
                .rounded(px(10.))
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(10.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(12.5))
                        .text_color(rgb(t.text))
                        .child(SharedString::from(tf(
                            "删除项目 {name} 及其所有会话？",
                            &[("name", name)],
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .child(ghost_btn("pd-cancel", tr("取消"), false, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.confirm_prj_del = None;
                                cx.notify();
                            },
                        )))
                        .child(solid_danger_btn("pd-ok", tr("确认删除"), {
                            let path = path.clone();
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.confirm_prj_del = None;
                                this.delete_project(path.clone(), cx);
                            })
                        })),
                ),
        )
        .into_any_element()
}
