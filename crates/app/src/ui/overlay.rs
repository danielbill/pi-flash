//! 浮层（overlay）公共基座 —— **所有**弹窗/弹层的唯一外壳。
//!
//! 全局规则（用户口径，任何新浮层都必须走这里）：
//! 1. 铺满父容器 + `occlude()`：鼠标**永不穿透**到下层（点击/滚轮/悬停都停在
//!    这一层）——裸挂 `absolute` 而不 occlude 的浮层会让事件漏进底下的消息
//!    列表/终端（`input.rs` 胶囊、`psp_overlays` 都踩过）；
//! 2. 点浮层外任意处关闭；
//! 3. ESC 关闭（layer 自己 `track_focus`，调用方传 `chat.dialog_focus`）；
//! 4. 点卡片本身**不**关闭 —— 卡片套一层 [`stop_click`]；
//! 5. 有可见内容的卡片再挂一个 × 关闭钮（[`close_btn`] / [`panel_header`]）。
//!
//! 定位由调用方决定：layer 是 `absolute inset_0` 的空壳，卡片既可以居中
//! （`.flex().items_center().justify_center()`），也可以锚在别处（topbar 正
//! 下方、鼠标坐标、胶囊上方……）。
//!
//! 两处**故意例外**（不是浮层，各有更贴切的语义）：
//! - `/` `@` 补全菜单：它是 composer 的补全 UI，跟着输入框一起排布，ESC 由
//!   输入框自己解释成「取消补全」（`input.rs` escape 分支），点外收起用
//!   capture 阶段的 `on_mouse_down_out` 且**故意**让这次点击继续落到下层
//!   （用户直接点发送仍然发送）。
//! - 悬停提示（tooltip / hover 详情卡）：跟随鼠标出现，收起由 hover 状态机
//!   决定，没有「外点关闭」的概念。

use gpui::{App, ElementId, FocusHandle, MouseButton, SharedString, Window, div, prelude::*, px, rgb};

use crate::theme::Theme;

/// 浮层外壳：遮挡 + 外点关闭 + ESC 关闭。
///
/// `dismiss` 会同时挂在「外点」和「ESC」两个 handler 上，所以要求 `Clone`
/// （捕获 `WeakEntity` 的闭包天然满足）。
pub fn layer(
    dim: bool,
    focus: Option<&FocusHandle>,
    dismiss: impl Fn(&mut Window, &mut App) + Clone + 'static,
) -> gpui::Div {
    let outside = dismiss.clone();
    let mut el = div()
        .absolute()
        .inset_0()
        // 规则 1：这一层就是鼠标的终点（子元素后绘制不受影响）
        .occlude()
        // 规则 2：点在卡片外 = 关（卡片自己 stop_propagation）
        .on_mouse_down(MouseButton::Left, move |_, w, cx| outside(w, cx));
    if dim {
        el = el.bg(gpui::hsla(0., 0., 0., 0.35));
    }
    if let Some(focus) = focus {
        // 规则 3：ESC 关。焦点挂在浮层上（点浮层/卡片内即获得焦点）
        el = el
            .track_focus(focus)
            .on_key_down(move |ev: &gpui::KeyDownEvent, w, cx| {
                if ev.keystroke.key == "escape" {
                    dismiss(w, cx);
                }
            });
    }
    el
}

/// 规则 4：卡片外壳。点卡片本身不触发「外点关闭」。
pub fn stop_click(el: gpui::Div) -> gpui::Div {
    el.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// 规则 5：× 关闭钮（22×22 圆角，hover 变亮）。
pub fn close_btn(
    id: impl Into<ElementId>,
    t: &Theme,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id.into())
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, w, cx| dismiss(w, cx))
        .child(crate::ui::icon_hover("x", 12., t.text_muted))
        .into_any_element()
}
/// 大卡片窗框 —— **设置弹窗的原始窗体**（0.7×0.98、chrome 36px 顶条 + ×、
/// 可选左导航列），抽出来给「系统提示词 / 工具定义」两个面板直接复用，不再
/// 自造窗体。调用方负责外面那层 [`layer`] 与居中：
///
/// ```ignore
/// overlay::layer(true, Some(&chat.dialog_focus), dismiss)
///     .flex().items_center().justify_center()
///     .child(overlay::big_card("标题", Some(nav), body, t, close))
/// ```
///
/// `title` 为空串 = 无标题（设置弹窗用左导航当身份）。
pub fn big_card(
    title: &str,
    nav: Option<gpui::AnyElement>,
    body: gpui::AnyElement,
    t: &Theme,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::Div {
    let mut head = div()
        .h(px(36.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .pl(px(16.))
        .bg(rgb(t.chrome))
        // overflow_hidden 的裁剪是纯矩形（无圆角），顶条不自己倒角的话方形角
        // 会从弹窗圆角外露出来（四角尖尖角）
        .rounded_t(px(10.))
        .border_b_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x73)));
    if !title.is_empty() {
        head = head.child(
            div()
                .text_size(crate::appearance::ui_size(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(title.to_string())),
        );
    }
    head = head.child(div().flex_1()).child(
        div()
            .id("big-card-close")
            .mr(px(8.))
            .size(px(30.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .text_color(rgb(t.text_muted))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0xd8626a)).text_color(rgb(0xffffff)))
            .on_mouse_down(MouseButton::Left, move |_, w, cx| dismiss(w, cx))
            .child(crate::ui::icon_hover("x", 13., t.text_muted)),
    );

    let mut row = div().flex_1().min_h_0().flex();
    if let Some(nav) = nav {
        row = row.child(nav);
    }
    div()
        .w(gpui::relative(0.7))
        .h(gpui::relative(0.98))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(10.))
        .shadow_lg()
        .flex()
        .flex_col()
        .overflow_hidden()
        // 卡片内点击不冒泡到遮罩（否则点卡片任意处都会关）
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(head)
        .child(row.child(body))
}
