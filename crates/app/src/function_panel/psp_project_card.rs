//! psp 项目行右键详情卡（020）：样式对齐会话右键详情卡（同 338px 卡座），
//! 标题加粗、路径/会话数信息行、分隔线下三图标动作栏（删除带卡内确认态 /
//! 会话目录 / 新建会话）。锚右键点、点外即收。自 psp_overlays.rs 切出
//!（itlq.12 超规拆分，纯结构搬移）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::{tf, tr};
use crate::services::workspace::same_path;
use crate::ui::icon;
use crate::ui::popup::PopupEdges;

use super::psp_card_btns::{card_icon_btn, ghost_btn, info_row, solid_danger_btn};

/// 项目行右键详情卡（020）：样式对齐会话右键详情卡（同 338px 卡座，2026-10-16
/// 定夺两卡同步加宽 80）——
/// 标题加粗、信息行两行（文件夹 icon + 绝对路径全量折行 mono / 会话
/// icon + N 个会话）、分隔线下动作栏 = 标签栏项目行同款图标大档三钮
/// （垃圾桶=删除及其所有会话；folder-input=资源管理器打开**项目会话
/// 目录**；新会话=在此项目新建）。锚点 = 右键点，统一算法四象限；
/// 点卡外即收（同 hover_card 语义，右键别行 = 收旧开新）；删除走卡内
/// 取消/确认 两键（会话卡同款确认态），确认复用 delete_project。
pub(crate) fn project_card(
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
