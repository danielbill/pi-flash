//! psp 会话右键详情卡（v54）：338px 浅色浮层——标题（点击=打开并改名）、
//! 项目/最后活动/消息数信息行、置顶 pin 钮、删除确认流。锚右键点、点外
//! 即收。自 psp_overlays.rs 切出（itlq.12 超规拆分，纯结构搬移）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::{tf, tr};
use crate::services::format::time_ago;
use crate::services::workspace::same_path;
use crate::ui::popup::PopupEdges;

use super::psp_card_btns::{card_icon_btn, ghost_btn, info_row, solid_danger_btn};

/// 会话右键详情卡：338px 浅色浮层（020：与会话卡同步加宽 80），标题（点击=打开并改名）、项目/
/// 最后活动/消息数信息行、删除确认流。2026-10-15 定夺：悬停自动弹窗撤销，
/// 改右键召唤——锚点 = 右键点，统一算法四象限（左缘对齐鼠标 x、上/下方
/// 12px 让出光标防误点）；点卡外即收（右键别行 = 换卡）。
pub(crate) fn hover_card(
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
