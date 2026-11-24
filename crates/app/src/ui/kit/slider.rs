//! 连续量控件：range slider（设置-界面「会话内容宽度」首个消费方）。
//! 180×18 / 轨道 4 / 旋钮 12（§5.11、§10.4）。

use gpui::prelude::*;
use gpui::{AnyElement, MouseButton, Window, div, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;

/// 轨道可用宽（旋钮中心在两端内缩半个旋钮，实际拖动行程 = 本值 - 旋钮径）
const SLIDER_W: f32 = 180.;
/// 元素高 = 命中带高（轨道只有 4px，鼠标却要容易按中）
const SLIDER_H: f32 = 18.;
const SLIDER_TRACK_H: f32 = 4.;
const SLIDER_KNOB: f32 = 12.;

/// 会话内容宽度滑块（pi-web `chatContentWidth` 的 range 控件 parity）。
///
/// 交互：点轨道直接跳值、按住拖动连续改；拖动中只走内存预览
/// （[`crate::services::workspace::preview_chat_content_width`]，每帧一个
/// MouseMove，逐帧写 JSON 没必要），松手才落盘。
///
/// 实现走 `canvas` + 窗口级 `on_mouse_event`（与 `ui/psp_scrollbar.rs` 同
/// 一套机制）：元素级 `on_mouse_move` 只覆盖指针在元素内的移动，拖出 180px
/// 轨道就断，必须用窗口级监听才能在整窗范围跟手。`window.on_mouse_event`
/// 只允许 paint 期注册，故三件套挂在 canvas 的 paint 闭包里、每帧重注册。
pub(crate) fn width_slider(
    id: &'static str,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    weak: &gpui::WeakEntity<Chat>,
) -> AnyElement {
    let t = T();
    let geom = SliderGeom {
        value,
        min,
        max,
        step,
        track: rgb(t.border).into(),
        fill: rgb(t.accent).into(),
        knob: rgb(t.bg_panel).into(),
    };
    let weak = weak.clone();
    div()
        .id(id)
        .relative()
        .w(px(SLIDER_W))
        .h(px(SLIDER_H))
        .flex_shrink_0()
        .cursor_pointer()
        .child(
            gpui::canvas(
                |_, _, _| {},
                move |bounds, _, window, _cx| {
                    geom.paint(bounds, &weak, window);
                },
            )
            .absolute()
            .inset_0(),
        )
        .into_any_element()
}

/// 滑块一帧的绘制参数 + 交互（值映射与事件处理按同一套几何算）。
struct SliderGeom {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    track: gpui::Hsla,
    fill: gpui::Hsla,
    knob: gpui::Hsla,
}

/// 鼠标 x → 值：旋钮中心行程 [bounds.left+knob/2, bounds.right-knob/2] 映射
/// 到 [min, max]，量化到 step 后钳住。
fn value_at(
    bounds: gpui::Bounds<gpui::Pixels>,
    min: f32,
    max: f32,
    step: f32,
    x: gpui::Pixels,
) -> f32 {
    let knob = px(SLIDER_KNOB);
    let x0 = bounds.left() + knob / 2.;
    let x1 = bounds.right() - knob / 2.;
    let frac = f32::from(x - x0) / f32::from(x1 - x0);
    let v = min + frac.clamp(0., 1.) * (max - min);
    ((v / step).round() * step).clamp(min, max)
}

/// 值落地（拖动中）：只写内存预览 + 重绘，落盘留给松手
/// （[`crate::services::workspace::save_app_settings_now`]）。
fn apply_value(weak: &gpui::WeakEntity<Chat>, v: f32, cx: &mut gpui::App) {
    crate::services::workspace::preview_chat_content_width(v.max(0.) as u32);
    let _ = weak.update(cx, |_, cx| cx.notify());
}

thread_local! {
    /// 滑块拖拽中。必须是跨帧状态：`on_mouse_event` 监听在 paint 期每帧重
    /// 注册，闭包里的局部变量活不过一次重绘——拖拽中每次改值都会 notify
    /// 重绘，用局部 Cell 做拖拽标志会在第一帧就自己复位（同
    /// `ui/psp_scrollbar.rs` 的 DRAG 处理）。单窗口应用，thread_local 够用。
    static WIDTH_DRAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl SliderGeom {
    fn paint(
        &self,
        bounds: gpui::Bounds<gpui::Pixels>,
        weak: &gpui::WeakEntity<Chat>,
        window: &mut Window,
    ) {
        let knob = px(SLIDER_KNOB);
        let x0 = bounds.left() + knob / 2.;
        let x1 = bounds.right() - knob / 2.;
        let frac = ((self.value - self.min) / (self.max - self.min)).clamp(0., 1.);
        let knob_x = x0 + (x1 - x0) * frac;
        let mid_y = bounds.center().y;
        let track_bounds = gpui::Bounds::new(
            gpui::point(x0, mid_y - px(SLIDER_TRACK_H) / 2.),
            gpui::size(x1 - x0, px(SLIDER_TRACK_H)),
        );
        let radius = gpui::Corners::all(px(SLIDER_TRACK_H / 2.));
        let plain =
            |b: gpui::Bounds<gpui::Pixels>, color: gpui::Hsla, r: gpui::Corners<gpui::Pixels>| {
                gpui::quad(
                    b,
                    r,
                    color,
                    gpui::Edges::default(),
                    gpui::transparent_black(),
                    gpui::BorderStyle::default(),
                )
            };
        window.paint_quad(plain(track_bounds, self.track, radius));
        if knob_x > x0 {
            let done = gpui::Bounds::new(
                track_bounds.origin,
                gpui::size(knob_x - x0, px(SLIDER_TRACK_H)),
            );
            window.paint_quad(plain(done, self.fill, radius));
        }
        let knob_bounds = gpui::Bounds::new(
            gpui::point(knob_x - knob / 2., mid_y - knob / 2.),
            gpui::size(knob, knob),
        );
        window.paint_quad(gpui::quad(
            knob_bounds,
            gpui::Corners::all(knob / 2.),
            self.knob,
            gpui::Edges::all(px(1.5)),
            self.fill,
            gpui::BorderStyle::default(),
        ));

        // ---- 鼠标三件套（capture 相；命中才吞事件；窗口级监听每帧重注册）----
        let hit = bounds;
        let (min, max, step) = (self.min, self.max, self.step);
        // 按下：跳值（点轨道即定位）+ 起拖
        {
            let weak = weak.clone();
            window.on_mouse_event(move |ev: &gpui::MouseDownEvent, phase, _, cx| {
                if phase != gpui::DispatchPhase::Capture
                    || ev.button != MouseButton::Left
                    || !hit.contains(&ev.position)
                {
                    return;
                }
                cx.stop_propagation();
                WIDTH_DRAG.with(|d| d.set(true));
                apply_value(&weak, value_at(hit, min, max, step, ev.position.x), cx);
            });
        }
        // 移动：拖动中跟手（指针移出 180px 轨道也继续）
        {
            let weak = weak.clone();
            window.on_mouse_event(move |ev: &gpui::MouseMoveEvent, phase, _, cx| {
                if phase != gpui::DispatchPhase::Capture || !WIDTH_DRAG.with(|d| d.get()) {
                    return;
                }
                if !ev.dragging() {
                    WIDTH_DRAG.with(|d| d.set(false));
                    return;
                }
                cx.stop_propagation();
                apply_value(&weak, value_at(hit, min, max, step, ev.position.x), cx);
            });
        }
        // 松开：一次性落盘（拖动期只写内存预览）
        window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, _cx| {
            if phase != gpui::DispatchPhase::Capture || !WIDTH_DRAG.with(|d| d.get()) {
                return;
            }
            WIDTH_DRAG.with(|d| d.set(false));
            crate::services::workspace::save_app_settings_now();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轨道 180px / 旋钮 12px：可用行程 168px，两端旋钮中心对应 min/max。
    /// 数值域用真实设置区间（560..1200，步进 20）。
    fn track() -> gpui::Bounds<gpui::Pixels> {
        gpui::Bounds::new(gpui::point(px(0.), px(0.)), gpui::size(px(180.), px(18.)))
    }

    #[test]
    fn width_slider_maps_x_to_quantized_value() {
        // 左端（旋钮中心）= min、右端 = max、中点 = 中值
        assert_eq!(value_at(track(), 560., 1200., 20., px(6.)), 560.);
        assert_eq!(value_at(track(), 560., 1200., 20., px(174.)), 1200.);
        assert_eq!(value_at(track(), 560., 1200., 20., px(90.)), 880.);
    }

    #[test]
    fn width_slider_clamps_outside_track_and_quantizes() {
        assert_eq!(value_at(track(), 560., 1200., 20., px(-40.)), 560.);
        assert_eq!(value_at(track(), 560., 1200., 20., px(400.)), 1200.);
        // 未落在整档上 → 就近 20px 档（x=12 ≈ 583 → 580）
        assert_eq!(value_at(track(), 560., 1200., 20., px(12.)), 580.);
    }
}
