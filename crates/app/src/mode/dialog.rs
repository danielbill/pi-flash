//! 031-1 P3 计划审批弹窗：会话区 100%×100% 模态（挂 session/mod.rs 的
//! chat-wrap 浮层槽，无标题条——markdown 自带 h1，不造新轮子）。
//!
//! 版式 = 定稿原型 docs/UI设计/计划模式-原型.html：上 80% markdown 预览
//! （`editor::markdown::render` 复用件，内容列 760px 居中），下 20% 操作
//! 面板 600px 三行堆叠——两钮同款 Secondary 正常态（不用 accent 诱导）、
//! 全左对齐、键位提示并入文案朴实表达（「否决 / 取消（Esc）」）。
//! 分隔线宽度 = 会话内容区宽度（与预览列同宽居中，非整区通铺）。
//! 三动作通道见 state.rs（§2 裁决 5）。

use gpui::{Context, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::*;

/// 弹窗 UI 态（挂 `DlgUi.plan`；动作在 state.rs，渲染在本文件）。
pub(crate) struct PlanUi {
    /// Some(plan 正文) = 审批弹窗在屏。present_plan 工具事件触发（Session
    /// Event::PlanPresent）；同一时刻仅一份挂起裁决，开着时后来的丢弃。
    pub approval: Option<String>,
    /// 「输入其他想法，回车确认」输入框
    pub input: gpui::Entity<crate::ui::TextInput>,
    /// 计划正文滚动句柄（预览列独立滚动）
    pub scroll: gpui::ScrollHandle,
    /// 浮层焦点（overlay::layer track_focus 契约：ESC = 否决路径）
    pub focus: gpui::FocusHandle,
}

impl PlanUi {
    pub(crate) fn new(cx: &mut gpui::Context<Chat>) -> Self {
        Self {
            approval: None,
            input: cx
                .new(|cx| crate::ui::TextInput::new(cx).placeholder(tr("plan.input_placeholder"))),
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
        }
    }
}

/// 内容列宽度：与 markdown 预览列同宽（分隔线「会话内容区宽度」裁决的
/// 落点；原型 760px）。
const CONTENT_W: f32 = 760.;
/// 操作面板宽（需求：600px 居中）。
const OPS_W: f32 = 600.;

/// 渲染审批弹窗（调用方保证 `chat.dlg.plan.approval.is_some()`；未挂起时
/// 返回空元素，挂载点无需再判）。
pub(crate) fn render_plan_approval(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let Some(plan) = chat.dlg.plan.approval.clone() else {
        return div().into_any_element();
    };
    // 输入提交每帧接线（幂等覆盖；ext_show 同款模式——闭包捕获 weak 实体）
    let weak_submit = weak.clone();
    chat.dlg.plan.input.update(cx, |ti, _cx| {
        ti.set_on_submit(Box::new(move |v, cx| {
            let submitted = v.trim().to_string();
            let _ = weak_submit.update(cx, |c, cx| c.mc_plan_feedback(submitted, cx));
        }));
    });
    let focus = chat.dlg.plan.focus.clone();
    let scroll = chat.dlg.plan.scroll.clone();
    // Esc / 点外 = 否决路径（需求：否决/取消（Esc））。面板 100% 覆盖后
    // 不存在「点外」，Esc 是唯一键盘出口。
    let weak_dismiss = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| c.mc_plan_reject_abort(cx));
    };
    crate::ui::overlay::layer(false, Some(&focus), dismiss)
        // 整层就是面板本体：不透明 bg 完全覆盖会话区（100%×100%）
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // ── 上 80%：markdown 预览（editor-markdown 复用件，内容列居中） ──
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
                                .max_w(px(CONTENT_W))
                                .mx_auto()
                                .px(px(32.))
                                .pt(px(28.))
                                .pb(px(40.))
                                .child(crate::editor::markdown::render(&plan, t, false)),
                        ),
                )
                .child(crate::ui::psp_scrollbar::menu_scrollbar(&scroll)),
        )
        // ── 下 20%：操作面板（600px 居中，三行堆叠） ──
        .child(
            div()
                .h(relative(0.2))
                .min_h(px(158.))
                .flex_shrink_0()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                // 分隔线：宽度 = 会话内容区宽度（与预览列同宽居中），非整区通铺
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(0.))
                        .right(px(0.))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w(px(CONTENT_W))
                                .max_w_full()
                                .h(px(1.))
                                .bg(rgb(t.border)),
                        ),
                )
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
                        ))
                        .child(crate::ui::overlay::stop_click(
                            div()
                                .w_full()
                                .child(chat.dlg.plan.input.clone().into_any_element()),
                        )),
                ),
        )
        .into_any_element()
}

/// 三行同款 Secondary 按钮（§5.1 BTN_LG 32 / px14 / ui12 / r6；全左对齐，
/// 无图标无键帽——键位提示已在文案里）。点击 stop_propagation 不触发浮层
/// 外点关闭。
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
        .h(px(32.))
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
