//! 031-1 P3 计划审批弹窗：宽度 = 会话区 100%，高度 = 90% 上下居中（挂
//! session/mod.rs 的 chat-wrap 浮层槽），带 40px 标题栏（「计划」+ 标准 ×
//! 控件，§5.7）。无输入框——否决（Esc/×/按钮）后用户在 composer 自行输入
//! （2026-11-08 简化裁决）。计划正文 100% 宽，markdown 复用
//! `editor::markdown::render`；下方两钮同款 Secondary 600px 居中。
//! 焦点走老控件链：`overlay::layer(dialog_focus)` + focus_arbitrate 白名单
//! （shell/render/focus.rs），Esc = 否决路径。三动作通道见 state.rs。

use gpui::{Context, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::*;

/// 弹窗 UI 态（挂 `DlgUi.plan`；动作在 state.rs，渲染在本文件）。
pub(crate) struct PlanUi {
    /// Some(plan 正文) = 审批弹窗在屏。present_plan 工具事件触发（Session
    /// Event::PlanPresent）；同一时刻仅一份挂起裁决，开着时后来的丢弃。
    pub approval: Option<String>,
    /// 计划正文滚动句柄（预览列独立滚动）
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

/// 操作面板宽（2026-11-08 定稿：两钮 600px 居中）。
const OPS_W: f32 = 600.;

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
        let _ = weak_dismiss.update(cx, |c, cx| c.mc_plan_reject_abort(cx));
    };
    // 老控件链（dialog_shell 同款）：overlay::layer(dialog_focus) + 整面
    // 面板 stop_click——点击面板不冒泡成否决；ESC 由 layer 派发
    crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), dismiss)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::stop_click(
            div()
                .w_full()
                .h(relative(0.9))
                .flex()
                .flex_col()
                .bg(rgb(t.bg))
                // ── 标题栏 40px（§5.7：标题左、关闭钮右上） ──
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
                                let _ = weak_close.update(cx, |c, cx| c.mc_plan_reject_abort(cx));
                            },
                        )),
                )
                // ── 计划正文（100% 宽，markdown 复用件） ──
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
                                .overflow_y_scroll()
                                .track_scroll(&scroll)
                                .child(
                                    div()
                                        .px(px(24.))
                                        .pt(px(24.))
                                        .pb(px(40.))
                                        .child(crate::editor::markdown::render(&plan, t, false)),
                                ),
                        )
                        .child(crate::ui::psp_scrollbar::menu_scrollbar(&scroll)),
                )
                // ── 操作面板：两钮 600px 居中，通铺分隔线 ──
                .child(
                    div()
                        .flex_shrink_0()
                        .border_t_1()
                        .border_color(rgb(t.border))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .py(px(16.))
                        .child(
                            div()
                                .w(px(OPS_W))
                                .max_w_full()
                                .px(px(16.))
                                .flex()
                                .flex_col()
                                .gap(px(8.))
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
                                    |c, cx| c.mc_plan_reject_abort(cx),
                                )),
                        ),
                ),
        ))
        .into_any_element()
}

/// 两钮同款 Secondary 按钮（§5.1 h36 / px14 / ui12 / r6；全左对齐，无图标
/// 无键帽——键位提示已在文案里）。点击 stop_propagation 不触发浮层外点关闭。
fn plan_btn(
    id: &'static str,
    weak: gpui::WeakEntity<Chat>,
    label: SharedString,
    t: &Theme,
    on: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w_full()
        .h(px(36.))
        .px(px(14.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .flex()
        .items_center()
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
