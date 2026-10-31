//! psp 悬浮层 (v54): 项目路径 tooltip（深色，跟随鼠标）、会话/项目右键
//! 详情卡（锚右键点、点外即收；会话卡改名/删除流，项目卡删除确认/会话
//! 目录/新建会话）、⋯ 两级菜单（列表方式/排序方式带子菜单勾选态）。
//! 全部挂在根渲染，事件坐标定位。
//!
//! 定位统一走 `ui::popup::PopupEdges`（2026-10-11 用户算法：锚点在视口
//! 上半 → 向下弹，反之向上；左半 → 向右弹，反之向左）——列表拉到底时
//! 菜单/详情卡不再溢出下缘，窄窗时不再溢出右缘。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::{tf, tr};
use crate::services::format::time_ago;
use crate::services::workspace::{same_path, same_ws};
use crate::ui::popup::PopupEdges;
use crate::ui::{WithTip, icon};

/// 根渲染挂载的 psp 悬浮层合集（tooltip / 详情卡 / 菜单 / 确认）。
pub(crate) fn psp_overlays(
    chat: &mut Chat,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let vp = window.viewport_size();
    let vp = (f32::from(vp.width), f32::from(vp.height));
    // 全窗口定位层：absolute inset_0 铺满 root，悬浮子元素相对它定位，
    // 且不参与流布局（0 高容器会把 absolute 子元素裁掉——菜单曾因此不可见）。
    // **父层不得 occlude**：inset_0 的层一挡全窗口 hit-test 都断（行悬停
    // 背景闪烁的根因）；各浮层在自己的 bounds 内各自 occlude。
    let mut el = div().id("psp-overlays").absolute().inset_0();
    if let Some((path, x, y)) = &chat.dlg.proj_tip {
        el = el.child(proj_tip(path, *x, *y, vp, t));
    }
    if chat.dlg.editor_menu.is_some() {
        el = el.child(super::psp_menus::editor_menu_layer(chat, vp, t, cx));
    }
    // 输入面板右键菜单（031：编辑块 撤销/恢复/全选 + 剪切/复制/粘贴/删除）
    if chat.dlg.input_menu.is_some() {
        el = el.child(super::psp_menus::input_menu_layer(chat, vp, t, cx));
    }
    // 消息内容区右键菜单（单项「复制」= 复制当前消息划选）
    if chat.dlg.msg_menu.is_some() {
        el = el.child(super::psp_menus::msg_menu_layer(chat, vp, t, cx));
    }
    // 终端右键菜单（复制/粘贴/全选）；tab 已关则不再挂（防挂到空气里）
    if let Some((tab_id, x, y)) = chat.dlg.term_menu
        && chat.ws.terminals.iter().any(|t| t.id == tab_id)
    {
        el = el.child(term_menu_layer(chat, tab_id, x, y, vp, t, cx));
    }
    if chat.dlg.hover_card.is_some() {
        // 右键详情卡：打开即渲染（无悬停延迟），点卡外/再右键别行收换
        el = el.child(hover_card(chat, vp, t, cx));
    }
    if chat.dlg.proj_card.is_some() {
        // 项目行右键详情卡：同 hover_card 语义（点卡外/再右键别行收换）
        el = el.child(project_card(chat, vp, t, cx));
    }
    // 插件格子右键菜单：同一浮层挂载点，样式与文件树菜单同源
    if chat.plugins.plugins.iter().any(|p| p.cell_menu.is_some()) {
        el = el.child(crate::pf_plugin::cell_menu_layer(chat, vp, t, cx));
    }
    // 插件弹窗（契约 v3 popups）：非模态居中卡，点外/× 收（同一浮层基座）
    if chat.plugins.popup.is_some() {
        el = el.child(crate::pf_plugin::plugin_popup_layer(chat, vp, t, cx));
    }
    if chat.dlg.tree_menu.is_some() {
        el = el.child(super::tree_menu::tree_menu_layer(chat, vp, t, cx));
    }
    // 文件树标题项目下拉（树根跨项目快切）：标题左键 / files.project_menu
    if chat.dlg.tree_proj_menu.is_some() {
        el = el.child(tree_proj_menu_layer(chat, vp, t, cx));
    }
    // topbar 标签右键菜单（zed tab_bar 菜单移植；tab 已关不再挂——防挂到
    // 空气里，顺带收槽位）
    if let Some((target, x, y)) = chat.dlg.tab_menu {
        let alive = match target {
            crate::TabMenuTarget::Session => true,
            crate::TabMenuTarget::Panel(ix) => ix < chat.ws.panel_tabs.len(),
        };
        if alive {
            el = el.child(super::tab_menu::layer(chat, target, x, y, vp, t, cx));
        } else {
            chat.dlg.tab_menu = None;
        }
    }
    if let Some(path) = chat.dlg.tree_confirm_del.clone() {
        // zed build_removal_prompt 同款：删除打开中的文件时未保存修改并入
        // 删除确认（一次确认 = 删文件 + 弃缓冲）；目录删除统计其子树内
        // 全部打开中的脏 tab（zed 对整个待删集合数 dirty_buffers）
        let dirty_count = chat
            .ws
            .file_cache
            .iter()
            .filter(|(p, f)| f.dirty && (same_path(p, &path) || p.starts_with(&path)))
            .count();
        el = el.child(tree_delete_confirm(
            &path,
            dirty_count,
            &chat.dlg.dialog_focus,
            cx,
        ));
    }
    if chat.dlg.tree_del_error.is_some() {
        el = el.child(tree_del_error_layer(chat, t, cx));
    }
    el.into_any_element()
}

/// 项目行 tooltip：短名 + 全路径（mono），随主题（与 toast 同族：bg_panel
/// + border + text），跟随鼠标。锚点 = 光标，统一算法：光标在右半 → 贴
/// 光标左侧弹，下半 → 贴光标上方弹。t 从外层传入（psp_overlays 已取）。
fn proj_tip(
    path: &std::path::PathBuf,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static crate::appearance::theme::Theme,
) -> impl gpui::IntoElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let full = path.to_string_lossy().to_string();
    PopupEdges::around((x, y), vp, 14., 16.)
        .apply(
            div()
                .id("psp-proj-tip")
                .absolute()
                .max_w(px(300.))
                .px(px(11.))
                .py(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(1.)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(name)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(10.5))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(full)),
        )
}

/// 会话右键详情卡：338px 浅色浮层（020：与会话卡同步加宽 80），标题（点击=打开并改名）、项目/
/// 最后活动/消息数信息行、删除确认流。2026-10-15 定夺：悬停自动弹窗撤销，
/// 改右键召唤——锚点 = 右键点，统一算法四象限（左缘对齐鼠标 x、上/下方
/// 12px 让出光标防误点）；点卡外即收（右键别行 = 换卡）。
fn hover_card(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let card = chat
        .dlg
        .hover_card
        .clone()
        .expect("hover_card checked by caller");
    let mut found: Option<(String, u64, std::time::SystemTime, String)> = None;
    for g in &chat.psp.projects {
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
    // 置顶态（020 会话置顶 ②）：渲染期判定一次，pin 钮的图标文案随它翻转；
    // 点击只 toggle 不收卡——用户能直接看到置顶态变化
    let pinned = chat.session_pinned(&card.path);
    let weak_hc_pin = cx.entity().downgrade();
    // 统一算法四象限：左缘对齐鼠标 x、上/下方让 12px（光标不压卡，防误点）
    let dir = PopupEdges::around((card.x, card.y), vp, 0., 12.);
    let mut el = div()
        .id("psp-hcard")
        .absolute()
        .w(px(338.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(12.))
        .shadow_lg()
        // 遮挡只作用于卡片自身 bounds（父层 occlude 会断全窗口 hit-test）
        .occlude()
        .px(px(13.))
        .pt(px(11.))
        .pb(px(9.))
        .flex()
        .flex_col()
        // 卡内点击不落穿（改名/删除按钮依赖；左右键都拦——右键穿下去会
        // 在身下的行上再开一张卡）
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Right, |_, _, cx| {
            cx.stop_propagation();
        });
    // 点卡外即收（上下文卡语义）。用 capture 阶段的 `on_mouse_down_out`
    //（capture 只判「点不在卡 bounds 内」，本次点击照常冒泡给下层 = 点哪
    // 去哪），不铺全屏 catcher：铺了会遮挡整窗 hit-test。右键别行 = 收旧
    // 开新（行的右键处理器同帧置新卡）
    {
        let weak = cx.entity().downgrade();
        el = el.on_mouse_down_out(move |_, _w, cx| {
            let _ = weak.update(cx, |c, cx| c.hover_card_rename_dismiss(cx));
        });
    }
    el = dir.apply(el);
    el = el
        .children(if card.renaming {
            // 设计稿：标题原地变输入框（Enter 提交 / Esc 取消）
            card.rename_input
                .clone()
                .map(|inp| div().mt(px(2.)).child(inp).into_any_element())
        } else {
            Some(
                div()
                    .id("hc-name")
                    .text_size(crate::appearance::ui_size(13.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .overflow_hidden()
                    // 标题最多三行（2026-11 定夺）：够放下长会话名，再多就截断
                    // 加省略号；line_clamp 自带 overflow_hidden
                    .line_clamp(3)
                    .text_ellipsis()
                    .cursor_pointer()
                    .px(px(5.))
                    .py(px(1.))
                    .mx(px(-5.))
                    .my(px(-1.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    // 点击标题 = 原地进入改名（不切换会话）
                    // 点击标题 = 原地进入改名（不切换会话）；构建/聚焦细节在
                    // Chat::hover_card_begin_rename（与自动化入口共用）
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                            this.hover_card_begin_rename(cx);
                        }),
                    )
                    .child(SharedString::from(title))
                    .into_any_element(),
            )
        })
        .child(info_row("folder-closed", SharedString::from(prj_name), t))
        .child(info_row(
            "icon-clock-solid",
            // time_ago 已自带「前」后缀（format.rs 的 tf("time.d_ago")）——
            // 这里再拼一次就成「1天前前」
            SharedString::from(time_ago(modified)),
            t,
        ))
        .child(info_row(
            "message-square",
            SharedString::from(tf("psp.messages_count", &[("n", msgs.to_string())])),
            t,
        ))
        .child(
            div()
                .h(px(1.))
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .mx(px(-13.))
                .mt(px(9.))
                .mb(px(7.)),
        )
        .child(
            // 操作栏：pin 钮在最左（贴卡内左缘），删除/确认组仍右对齐
            //（020 会话置顶 ②）
            div()
                .flex()
                .items_center()
                .child(card_icon_btn(
                    &weak_hc_pin,
                    "hc-pin",
                    // 已置顶 → pin-off（与 tooltip「取消置顶」同频翻转）
                    if pinned { "pin-off" } else { "pin" },
                    if pinned {
                        tr("psp.unpin")
                    } else {
                        tr("psp.pin")
                    },
                    t.text,
                    {
                        let path = path.clone();
                        move |c, cx| c.session_pin_toggle(path.clone(), cx)
                    },
                ))
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .children(if confirming {
                            // 原地换 取消/确认
                            vec![
                                ghost_btn(
                                    "hc-cancel",
                                    tr("action.cancel"),
                                    false,
                                    cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        if let Some(c) = this.dlg.hover_card.as_mut() {
                                            c.confirming = false;
                                        }
                                        cx.notify();
                                    }),
                                )
                                .into_any_element(),
                                solid_danger_btn("hc-ok", tr("action.confirm"), {
                                    let path = path.clone();
                                    cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                        this.dlg.hover_card = None;
                                        this.delete_session(path.clone(), cx);
                                    })
                                })
                                .into_any_element(),
                            ]
                        } else {
                            vec![
                                ghost_btn(
                                    "hc-del",
                                    tr("action.delete"),
                                    true,
                                    cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                        if let Some(c) = this.dlg.hover_card.as_mut() {
                                            c.confirming = true;
                                        }
                                        cx.notify();
                                    }),
                                )
                                .into_any_element(),
                            ]
                        }),
                ),
        );
    let _ = &mut el;
    el.into_any_element()
}

fn info_row(
    icon_name: &'static str,
    text: SharedString,
    t: &'static Theme,
) -> impl gpui::IntoElement {
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

/// 项目行右键详情卡（020）：样式对齐会话右键详情卡（同 338px 卡座，2026-10-16
/// 定夺两卡同步加宽 80）——
/// 标题加粗、信息行两行（文件夹 icon + 绝对路径全量折行 mono / 会话
/// icon + N 个会话）、分隔线下动作栏 = 标签栏项目行同款图标大档三钮
/// （垃圾桶=删除及其所有会话；folder-input=资源管理器打开**项目会话
/// 目录**；新会话=在此项目新建）。锚点 = 右键点，统一算法四象限；
/// 点卡外即收（同 hover_card 语义，右键别行 = 收旧开新）；删除走卡内
/// 取消/确认 两键（会话卡同款确认态），确认复用 delete_project。
fn project_card(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let card = chat
        .dlg
        .proj_card
        .clone()
        .expect("proj_card checked by caller");
    let Some(g) = chat
        .psp
        .projects
        .iter()
        .find(|g| same_path(&g.path, &card.path))
    else {
        return div().into_any_element();
    };
    let title: SharedString = g.name.clone().into();
    let full: SharedString = g.path.to_string_lossy().to_string().into();
    let count = tf("psp.sessions_count", &[("n", g.sessions.len().to_string())]);
    // 项目会话目录 = pi sessions 根下按 cwd 编码的组目录（--d---…--）——
    // 资源管理器入口进的是这里而非项目目录（2026-10-16 定夺）。组目录还没
    // 建（该项目无落盘会话）时退回 sessions 根，保证点了总有出处
    let sess_dir = pi_link::sessions::sessions_root().map(|root| {
        let dir = root.join(pi_link::sessions::group_name_for_cwd(
            &g.path.to_string_lossy(),
        ));
        if dir.is_dir() { dir } else { root }
    });
    let path = card.path.clone();
    let confirming = card.confirming;
    let weak = cx.entity().downgrade();
    // 统一算法四象限：左缘对齐鼠标 x、上/下方让 12px（光标不压卡，防误点）
    let dir = PopupEdges::around((card.x, card.y), vp, 0., 12.);
    let mut el = div()
        .id("psp-pcard")
        .absolute()
        .w(px(338.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(12.))
        .shadow_lg()
        // 遮挡只作用于卡片自身 bounds（父层 occlude 会断全窗口 hit-test）
        .occlude()
        .px(px(13.))
        .pt(px(11.))
        .pb(px(9.))
        .flex()
        .flex_col()
        // 卡内点击不落穿（动作按钮依赖；左右键都拦——右键穿下去会在
        // 身下的行上再开一张卡）
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Right, |_, _, cx| {
            cx.stop_propagation();
        });
    // 点卡外即收（上下文卡语义，hover_card 同款 capture 落点判断）
    {
        let weak = weak.clone();
        el = el.on_mouse_down_out(move |_, _w, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.dlg.proj_card = None;
                cx.notify();
            });
        });
    }
    el = dir.apply(el);
    el = el
        // 标题：项目名加粗（会话卡标题同字号；项目不可改名，纯展示）
        .child(
            div()
                .text_size(crate::appearance::ui_size(13.5))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(t.text))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(title),
        )
        // 路径行：会话卡信息行同构，唯路径走小字 mono（proj_tip 同族）。
        // 全路径必须可见（2026-10-16 定夺）：折行展示，不截断不省略；
        // 字号比首版再小一档（11 → 10），多行时图标顶对齐首行
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .mt(px(7.5))
                .child(icon("folder-closed", 15., t.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(crate::appearance::ui_size(10.))
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_color(rgb(t.text_muted))
                        .child(full),
                ),
        )
        .child(info_row("icon-new-chat", SharedString::from(count), t))
        .child(
            div()
                .h(px(1.))
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .mx(px(-13.))
                .mt(px(9.))
                .mb(px(7.)),
        )
        .child(
            div()
                .flex()
                .justify_end()
                // 三钮并排：间距比首版放宽（6 → 10，2026-10-16 定夺）
                .gap(px(10.))
                .children(if confirming {
                    // 原地换 取消/确认（会话卡同款确认态）
                    vec![
                        ghost_btn(
                            "pjc-cancel",
                            tr("action.cancel"),
                            false,
                            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                                if let Some(c) = this.dlg.proj_card.as_mut() {
                                    c.confirming = false;
                                }
                                cx.notify();
                            }),
                        )
                        .into_any_element(),
                        solid_danger_btn("pjc-ok", tr("psp.confirm_delete"), {
                            let path = path.clone();
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.dlg.proj_card = None;
                                this.delete_project(path.clone(), cx);
                            })
                        })
                        .into_any_element(),
                    ]
                } else {
                    vec![
                        card_icon_btn(
                            &weak,
                            "pjc-del",
                            "icon-trash-solid",
                            tr("psp.delete_project"),
                            t.danger,
                            |c, cx| {
                                if let Some(card) = c.dlg.proj_card.as_mut() {
                                    card.confirming = true;
                                }
                                cx.notify();
                            },
                        )
                        .into_any_element(),
                        // 会话目录（非项目目录）在资源管理器中打开
                        card_icon_btn(
                            &weak,
                            "pjc-explorer",
                            "folder-input",
                            tr("psp.open_session_dir"),
                            t.text,
                            {
                                let sess_dir = sess_dir.clone();
                                move |c, cx| {
                                    if let Some(d) = &sess_dir {
                                        crate::services::shell_open::reveal_in_explorer(d);
                                    }
                                    c.dlg.proj_card = None;
                                    cx.notify();
                                }
                            },
                        )
                        .into_any_element(),
                        card_icon_btn(
                            &weak,
                            "pjc-new",
                            "icon-new-chat",
                            tr("psp.new_session_in_project"),
                            t.text,
                            {
                                let path = path.clone();
                                move |c, cx| {
                                    c.dlg.proj_card = None;
                                    c.new_session_in(path.clone(), cx);
                                }
                            },
                        )
                        .into_any_element(),
                    ]
                }),
        );
    el.into_any_element()
}

/// 项目卡动作栏 icon 钮：26×26 盒 + 标签栏项目行同款图标大档（LG）；
/// hover = 盒底 bg_hover + 图标着色（del 悬停变 danger 红，同项目行
/// 垃圾桶语义；new 悬停变常字色）。点击 = 应用动作（收卡/确认态切换
/// 由动作自带）。
fn card_icon_btn(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    icon_name: &'static str,
    tip: &'static str,
    hover_color: u32,
    apply: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    let weak = weak.clone();
    div()
        .id(id)
        .w(px(26.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .group(id)
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .with_tip(tip)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                apply(c, cx);
            });
        })
        .child(
            gpui::svg()
                .path(SharedString::from(format!("icons/{icon_name}.svg")))
                .text_color(rgb(t.text_dim))
                .group_hover(id, move |s| s.text_color(rgb(hover_color)))
                .size(px(crate::ui::tokens::icon::LG))
                .into_any_element(),
        )
}

fn ghost_btn(
    id: &'static str,
    label: &str,
    danger: bool,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    crate::ui::button(
        id,
        label.to_string(),
        crate::ui::BtnSize::Sm,
        if danger {
            crate::ui::BtnVariant::Danger
        } else {
            crate::ui::BtnVariant::Secondary
        },
        false,
        handler,
    )
}

fn solid_danger_btn(
    id: &'static str,
    label: &str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    // §5.1 唯一按钮表：实心红不存在，破坏性主行动 = Danger 描边款
    crate::ui::button(
        id,
        label.to_string(),
        crate::ui::BtnSize::Sm,
        crate::ui::BtnVariant::Danger,
        false,
        handler,
    )
}

// ---------------------------------------------------------------------------
// psp 菜单（⋯ 项目菜单已于 020-1 撤除：浏览器/终端入口在文件树操作栏；
// 项目删除确认原为垃圾桶直弹独立浮层，2026-10-16 并入项目行右键详情卡）
// ---------------------------------------------------------------------------

// psp 菜单原语（menu_card/menu_item 族/menu_sep）已于 090 重构上移
// ui::menu（全 app 统一菜单形制）；此处 re-export 维持既有导入路径。
pub(crate) use crate::ui::menu::{
    menu_card, menu_item, menu_item_asset, menu_item_sized, menu_sep,
};

/// 删除项目确认浮层已撤（2026-10-16）：项目行垃圾桶钮移除后无入口，
/// 删除确认并入项目行右键详情卡的卡内 取消/确认 两键（会话卡同款）。

// ---------------------------------------------------------------------------
// 021 文件树右键菜单（目录/文件两套条目）+ 删除确认
// ---------------------------------------------------------------------------

/// 文件树标题项目下拉（树根跨项目快切）：一览 psp.projects（会话列表里
/// 的全部项目，当前项目勾选），↑/↓ 高亮 + Enter 切换、鼠标点击直切。
///
/// 键盘路径 = overlay::confirm 同款：render 焦点仲裁把 dialog_focus 交给
/// 下拉层（track_focus），layer 的 on_key_down 接 ↑/↓/Enter（ESC 由
/// overlay::layer 自带）。锚点 = 标题 paint 期 bounds（layout.ft_title_
/// bounds）：贴标题下缘左对齐（列表盖在树体上，dropdown 语义）；下方放
/// 不下才贴标题上缘向上弹。切换语义见 tree_proj_menu_pick（活跃会话不动）。
fn tree_proj_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let menu = chat.dlg.tree_proj_menu.clone().expect("checked by caller");
    // 标题 bounds 未量到（首帧 / 面板刚好收起）→ 收槽位不挂（防挂到空气
    // 里，tab_menu 同款守卫）
    let Some(tb) = chat.layout.ft_title_bounds.borrow().as_ref().map(|b| {
        (
            f32::from(b.origin.x),
            f32::from(b.origin.y),
            f32::from(b.size.width),
            f32::from(b.size.height),
        )
    }) else {
        chat.dlg.tree_proj_menu = None;
        return div().into_any_element();
    };
    // (名字, 路径, 是否当前)：当前态渲染期判定一次，行闭包只吃快照
    let items: Vec<(String, std::path::PathBuf, bool)> =
        chat.psp
            .projects
            .iter()
            .map(|g| {
                (
                    g.name.clone(),
                    g.path.clone(),
                    chat.ws.cwd.as_deref().is_some_and(|cur| {
                        same_ws(&g.path.to_string_lossy(), &cur.to_string_lossy())
                    }),
                )
            })
            .collect();
    let weak_nav = weak.clone();
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.tree_proj_menu = None;
    })
    // ↑/↓ 移高亮、Enter 切换（裸修饰键才接管；ESC 落到 layer 自己的
    // escape 分支）。无输入实体参与派发，apply 不需要 defer 出派发周期
    .on_key_down(move |ev: &gpui::KeyDownEvent, _w, cx| {
        let k = &ev.keystroke;
        let delta = match k.key.as_str() {
            "up" if !k.modifiers.modified() => Some(-1i32),
            "down" if !k.modifiers.modified() => Some(1),
            _ => None,
        };
        if let Some(delta) = delta {
            cx.stop_propagation();
            let _ = weak_nav.update(cx, |c, cx| c.tree_proj_menu_move(delta, cx));
            return;
        }
        if k.key == "enter" && !k.modifiers.modified() {
            cx.stop_propagation();
            let _ = weak_nav.update(cx, |c, cx| c.tree_proj_menu_apply(cx));
        }
    });
    let row_h = 28.;
    let max_h = 320.;
    // 估算高度（行数 × 行高 + 上下 p4）判下方放不放得下；超高滚动兜底
    let est_h = ((items.len() as f32) * row_h + 8.).min(max_h);
    let below = tb.1 + tb.3 + 4.;
    let fits_below = below + est_h <= vp.1 - 8.;
    let sel_bg = gpui::rgba((t.text << 8) | 0x1a);
    let mut card = div()
        .id("ftp-card")
        .absolute()
        .left(px(tb.0))
        .w(px(240.))
        .max_h(px(max_h))
        .overflow_y_scroll()
        .p(px(4.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(8.))
        .shadow_lg()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        });
    card = if fits_below {
        card.top(px(below))
    } else {
        card.bottom(px(vp.1 - tb.1 + 4.))
    };
    for (ix, (name, _path, current)) in items.into_iter().enumerate() {
        let row_weak = weak.clone();
        let selected = menu.sel == ix;
        card = card.child(
            div()
                .id(SharedString::from(format!("ftp-{ix}")))
                .h(px(row_h))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(6.))
                .overflow_hidden()
                .text_size(crate::appearance::ui_size(11.5))
                // 键盘高亮 = 10% 前景薄纱常驻（psp 会话行/tree 选中同色系）
                .when(selected, |d| d.bg(sel_bg).hover(move |s| s.bg(sel_bg)))
                .when(!selected, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                .child(icon("folder", 14., t.text_dim))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(rgb(if current { t.accent } else { t.text }))
                        .child(SharedString::from(name)),
                )
                .children(current.then(|| icon("check", 12., t.accent)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = row_weak.update(cx, |c, cx| {
                        c.tree_proj_menu_pick(ix, cx);
                    });
                }),
        );
    }
    layer.child(card).into_any_element()
}

/// 编辑区右键菜单（2026-10-11 用户定案）：编辑块（复制/粘贴/剪切/全选）
/// + 文件动作块（由 editor/kind.rs 的 FileTypeHandler 声明：浏览器/预览/
/// 资源管理器/终端 + 改名/删除，本渲染器零类型特判），
/// 样式与文件树菜单同套（menu_card/menu_item/menu_sep）。编辑四项
/// 先把焦点还给编辑器再派发 Input 动作（菜单打开不改变焦点归属，
/// 但点击可能让焦点漂移，保险起见每次归位）。
/// 终端右键菜单（复制 / 粘贴 / 全选）：动作作用于右键那一个终端 tab
/// （不依赖 active_terminal），完成后焦点还给终端——菜单是终端的附属
/// UI，收起后键盘要能接着用。
fn term_menu_layer(
    chat: &mut Chat,
    tab_id: usize,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let has_sel = chat
        .ws
        .terminals
        .iter()
        .find(|t| t.id == tab_id)
        .is_some_and(|t| t.selection.is_some());
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.term_menu = None;
    });
    let card = menu_card(edges, t)
        .child(term_menu_item(
            &weak,
            "tm-copy",
            "copy",
            tr("action.copy"),
            has_sel,
            tab_id,
            |c, tab, cx| {
                c.term_copy(tab, cx);
            },
        ))
        .child(term_menu_item(
            &weak,
            "tm-paste",
            "clipboard",
            tr("action.paste"),
            true,
            tab_id,
            |c, tab, cx| {
                c.term_paste(tab, cx);
            },
        ))
        .child(term_menu_item(
            &weak,
            "tm-select-all",
            "check",
            tr("action.select_all"),
            true,
            tab_id,
            |c, tab, cx| {
                c.term_select_all(tab, cx);
            },
        ));
    layer.child(card).into_any_element()
}

/// 终端菜单动作项：点击 = 应用动作（作用于该 tab）+ 收菜单 + 焦点回终端。
fn term_menu_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    icon_name: &'static str,
    label: &str,
    enabled: bool,
    tab_id: usize,
    apply: fn(&mut Chat, usize, &mut gpui::Context<Chat>),
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let item = menu_item(
        SharedString::from(id.to_string()),
        icon_name,
        label,
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            apply(c, tab_id, cx);
            c.dlg.term_menu = None;
            // 焦点回终端：菜单收起后键盘接着用（与打开菜单时的聚焦对称）
            if let Some(tab) = c.ws.terminals.iter().find(|t| t.id == tab_id) {
                let f = tab.focus.clone();
                window.focus(&f);
            }
            cx.notify();
        });
    });
    if enabled { item } else { item.opacity(0.45) }
}

/// 删除失败持久弹层：trash Err 必看（状态栏 toast 一闪容易漏）。
fn tree_del_error_layer(
    chat: &mut Chat,
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let msg = chat.dlg.tree_del_error.clone().expect("checked by caller");
    let weak_ok = weak.clone();
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.tree_del_error = None;
    });
    let card = crate::ui::overlay::stop_click(
        div()
            .min_w(px(320.))
            .p(px(14.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x73)))
            .rounded(px(10.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.5))
                    .text_color(rgb(t.text))
                    .child(crate::i18n::tf(
                        "extensions.remove_failed_msg",
                        &[("msg", msg)],
                    )),
            )
            .child(div().flex().justify_end().child(crate::ui::button(
                "del-err-ok",
                crate::i18n::tr("extensions.ok"),
                crate::ui::BtnSize::Md,
                crate::ui::BtnVariant::Secondary,
                false,
                move |_, _, cx| {
                    let _ = weak_ok.update(cx, |c, cx| {
                        c.dlg.tree_del_error = None;
                        cx.notify();
                    });
                },
            ))),
    );
    layer
        .flex()
        .items_center()
        .justify_center()
        .child(card)
        .into_any_element()
}

/// 删除确认：统一 overlay::confirm（自动居中 + 取消/确认两键）。
fn tree_delete_confirm(
    path: &std::path::PathBuf,
    dirty_count: usize,
    focus: &gpui::FocusHandle,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let weak_cancel = weak.clone();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // zed 同款：打开中的脏文件删除时明确「未保存更改将丢失」，一次确认
    // = 删文件 + 弃缓冲（discard_file_tab 强关，不再二次确认）；目录删除
    // 报其子树内全部受影响的脏 tab 数
    let message = if dirty_count > 0 {
        crate::i18n::tf(
            "tree.delete_confirm_dirty",
            &[("name", name), ("n", dirty_count.to_string())],
        )
    } else {
        crate::i18n::tf("tree.delete_confirm", &[("name", name)])
    };
    crate::ui::overlay::confirm(
        focus,
        message,
        move |_w, cx| {
            let _ = weak_cancel.update(cx, |c, cx| {
                c.dlg.tree_confirm_del = None;
                cx.notify();
            });
        },
        // 确认走统一方法（取槽→清槽→commit），自动化 op file.delete_confirm
        // 也调它——回调里不再自持路径副本
        move |_w, cx| {
            let _ = weak.update(cx, |c, cx| c.tree_delete_confirm_commit(cx));
        },
    )
}
