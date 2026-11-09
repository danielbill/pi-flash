//! 031-1 P3 计划审批弹窗：**标准模态卡**，骨架 1:1 抄
//! `dialogs/ext_ui/dialog.rs::render_ext_dialog`（overlay::layer 遮罩 +
//! 居中卡片 + 40px 标题栏 + info 滚动区 + footer 按钮行——不手搓）。
//! 卡宽 = 聊天内容区宽（`workspace::chat_content_width()`，composer 胶囊
//! /消息列同源），高 = 94% 上下居中；标题「计划」+ 标准 × 控件（§5.7）。
//! 正文 100% 卡宽 markdown（`editor::markdown::render` 复用）；footer 两钮
//! 一行 250×50、间距 50 并排居中。Esc/×/点外 = 否决路径（计划模式保持），
//! 焦点走老控件链（dialog_focus + focus_arbitrate 白名单）。

use gpui::{Context, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::*;

/// 弹窗 UI 态（挂 `DlgUi.plan`；动作在 state.rs，渲染在本文件）。
pub(crate) struct PlanUi {
    /// Some(plan 正文) = 审批弹窗在屏。present_plan 工具事件触发（Session
    /// Event::PlanPresent）；同一时刻仅一份挂起裁决，开着时后来的丢弃。
    pub approval: Option<String>,
    /// 计划正文滚动句柄（info 区独立滚动）
    pub scroll: gpui::ScrollHandle,
}

impl PlanUi {
    pub(crate) fn new(_cx: &mut Context<Chat>) -> Self {
        Self {
            approval: None,
            scroll: gpui::ScrollHandle::new(),
        }
    }
}

/// 渲染审批弹窗（调用方保证 `chat.dlg.plan.approval.is_some()`；未挂起时
/// 返回空元素，挂载点无需再判）。
pub(crate) fn render_plan_approval(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(plan) = chat.dlg.plan.approval.clone() else {
        return div().into_any_element();
    };
    let scroll = chat.dlg.plan.scroll.clone();
    let weak_close = weak.clone();
    // Esc / 点外 / 标题 × = 否决路径（需求：否决/取消（Esc）——计划模式
    // 保持，agent 随 abort 停止，用户在 composer 自行输入新想法）
    let weak_dismiss = weak_close.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| c.mc_plan_reject(cx));
    };
    // 标准模态 = render_ext_dialog 同款骨架：layer 遮罩 + 居中卡片 +
    // stop_click（卡片内点击不冒泡成否决）；焦点 = dialog_focus（ESC 派发）
    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::stop_click(
            div()
                // 卡宽 = 聊天内容区宽（composer 胶囊/消息列同一真源），
                // 高 = 94% 上下居中；上限防小窗溢出
                .w(px(crate::services::workspace::chat_content_width()))
                .max_w(relative(0.96))
                .h(relative(0.94))
                .bg(rgb(t.bg_panel))
                .border_1()
                .border_color(rgb(t.border))
                .rounded_lg()
                .overflow_hidden()
                .flex()
                .flex_col()
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                // ── 标题栏 40px（§5.7：标题左、关闭钮右上；ext 弹窗同款） ──
                .child(
                    div()
                        .h(px(40.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_4()
                        .border_b_1()
                        .border_color(rgb(t.border))
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(14.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(t.text))
                                .child(tr("plan.title")),
                        )
                        .child(crate::ui::overlay::close_btn(
                            "plan-close",
                            t,
                            move |_, cx| {
                                let _ = weak_close.update(cx, |c, cx| c.mc_plan_reject(cx));
                            },
                        )),
                )
                // ── 计划正文（100% 卡宽，独立滚动，markdown 复用件） ──
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .relative()
                        .child(
                            div()
                                .id("plan-body")
                                .absolute()
                                .top(px(0.))
                                .left(px(0.))
                                .w_full()
                                .h_full()
                                .px_4()
                                .py_3()
                                .overflow_y_scroll()
                                .track_scroll(&scroll)
                                .child(crate::editor::markdown::render(&plan, t, false)),
                        )
                        .child(crate::ui::psp_scrollbar::menu_scrollbar(&scroll)),
                )
                // ── footer 按钮行（ext 弹窗同款：border_t + px_4 py_3）；
                //    背景同左侧功能面板（nav 层）；两钮一行 300×50、间距 50
                //    并排居中，文字居中（2026-11-09 定稿） ──
                .child(
                    div()
                        .flex_shrink_0()
                        .bg(rgb(t.nav))
                        .flex()
                        .justify_center()
                        .gap(px(50.))
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(rgb(t.border))
                        .child(plan_btn(
                            "plan-approve",
                            weak.clone(),
                            tr("plan.approve").into(),
                            t,
                            |c, cx| c.mc_plan_approve(cx),
                        ))
                        .child(plan_btn(
                            "plan-reject",
                            weak.clone(),
                            tr("plan.reject").into(),
                            t,
                            |c, cx| c.mc_plan_reject(cx),
                        )),
                ),
        ))
        .into_any_element()
}

/// 两钮同款 Secondary 定制大钮（2026-11-09 定稿：300×50 并排居中，文字
/// 居中）。点击 stop_propagation 不触发浮层外点关闭。
fn plan_btn(
    id: &'static str,
    weak: gpui::WeakEntity<Chat>,
    label: SharedString,
    t: &Theme,
    on: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w(px(300.))
        .h(px(50.))
        .px(px(14.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_size(crate::appearance::ui_size(12.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| on(c, cx));
        })
        .child(label)
        .into_any_element()
}
