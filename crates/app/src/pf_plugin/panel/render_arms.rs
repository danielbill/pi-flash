//! `render_node_inner` 抽出的两个自绘交互臂（vh67 清偿，逻辑逐字未动）：
//! Canvas 画布与 Switch 开关。结构臂仍留 render.rs。

use super::style::{hex_color, tone_color};
use super::*;

/// Canvas 臂：定宽高裁剪画布，paint 逐条绝对定位；有 command 时整块即按钮。
pub(super) fn render_canvas(
    width: &f32,
    height: &f32,
    paints: &Vec<CanvasPaint>,
    command: &String,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    // 画布容器：定宽高 + 裁剪（契约：paint 坐标不许逃逸出节点框）
    let mut canvas = div()
        .relative()
        .w(px(*width))
        .h(px(*height))
        .overflow_hidden();
    for p in paints {
        canvas = canvas.child(match p {
            CanvasPaint::Rect { x, y, w, h, fill } => {
                let mut d = div()
                    .absolute()
                    .left(px(*x))
                    .top(px(*y))
                    .w(px(*w))
                    .h(px(*h));
                if let Some(c) = hex_color(fill) {
                    d = d.bg(rgb(c));
                }
                d
            }
            CanvasPaint::Text {
                x,
                y,
                size,
                tone,
                value,
            } => div()
                .absolute()
                .left(px(*x))
                .top(px(*y))
                .text_size(px(*size))
                .text_color(rgb(tone_color(tone, t)))
                .child(SharedString::from(value.clone())),
        });
    }
    // 框内提示即按钮（contract canvas.command）：左键点画布回环插件命令
    if !command.is_empty() {
        return canvas
            .id(SharedString::from(format!("pf-canvas-{ix}-{command}")))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                super::button::click_command(weak.clone(), ix, command.clone(), String::new()),
            )
            .into_any_element();
    }
    canvas.into_any_element()
}

/// Switch 臂：轨道/knob 走 §10.4 尺寸（SW_MD 28x16·knob10），点击翻转回环命令。
pub(super) fn render_switch(
    value: &bool,
    label: &String,
    tip: &String,
    command: &String,
    disabled: &bool,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    // 契约 v3 开关：轨道/knob 走 §10.4 尺寸（SW_MD 28x16·knob10），
    // 点击翻转 → onCommand(command, {value: 新布尔})；声明式：
    // 插件收到回调改自己的数据，重渲后 value 自然到位
    let t_ = *t;
    let (track, knob_x, knob_color) = if *value {
        (rgb(t_.accent), 28. - 16. + 3., t_.bg)
    } else {
        (rgb(t_.text_faint), 3., t_.bg)
    };
    let weak = weak.clone();
    let cmd = command.clone();
    let next = !*value;
    let env = serde_json::json!({ "value": next }).to_string();
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(6.));
    if !label.is_empty() {
        row = row.child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(t_.text_muted))
                .child(SharedString::from(label.clone())),
        );
    }
    let mut sw = div()
        .id(SharedString::from(format!("pf-switch-{ix}-{cmd}")))
        .relative()
        .w(px(28.))
        .h(px(16.))
        .rounded_full()
        .bg(track)
        .cursor_pointer()
        .child(
            div()
                .absolute()
                .left(px(knob_x))
                .top(px(3.))
                .w(px(10.))
                .h(px(10.))
                .rounded_full()
                .bg(rgb(knob_color)),
        );
    if !*disabled {
        sw = sw.on_mouse_down(MouseButton::Left, move |_, window, cx| {
            let _ = weak.update(cx, |c, cx| {
                c.plugin_command_fire_env(ix, &cmd, &env, window, cx);
                cx.notify();
            });
        });
    }
    if !tip.is_empty() {
        sw = sw.with_tip_text(tip.clone());
    }
    row.child(sw).into_any_element()
}
