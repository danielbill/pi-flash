//! psp 会话列表滚动条（v55）——ZED Regular 移植（`ui/components/scrollbar.rs`
//! 的最小子集）。替换 gpui-component Scrollbar：那个组件 track 常显（半透
//! 明条被误读为第二条滚动条）、Hover 模式不管可见性、未显示时点击穿透、
//! thumb 宽度是常量——四个坑都绕不开，自绘（~200 行）比 fork 干净。
//!
//! 行为规格（用户定稿）：
//! - 只有 thumb、无 track；贴 dock 右缘（视觉贴边）
//! - 鼠标在 panel 内 = 显示；离开 panel 3s 后 1s 淡出
//! - thumb 悬停加宽 2px（6→8px）方便拾取；可拖拽；轨道点击 = 翻页
//! - 全部鼠标事件 stop_propagation，不穿透到 session 行

use std::rc::Rc;
use std::cell::Cell;

use gpui::{
    px, rgb, size, AnyElement, Bounds, DispatchPhase, Edges, Element, ElementId, GlobalElementId, Pixels,
    HitboxBehavior, InspectorElementId, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Style,
    Window, prelude::*,
};

/// thumb 常规宽（贴边细条）
const THUMB_W: f32 = 6.;
/// thumb 悬停/拖拽宽（+2px 拾取）
const THUMB_W_ACTIVE: f32 = 8.;
/// 最小 thumb 长度（ZED MINIMUM_THUMB_SIZE）
const THUMB_MIN_LEN: f32 = 25.;
/// 上下呼吸位
const TRACK_INSET: f32 = 2.;
/// 淡出参数（离开 panel 后）
const FADE_DELAY_MS: u128 = 3000;
const FADE_LEN_MS: u128 = 1000;

/// 共享状态：panel 悬停 + 上次离开时刻（放 Chat 上会引入借用环，独立 Rc）。
#[derive(Clone, Default)]
pub struct PspScrollbarState {
    /// 鼠标是否在滚动容器内（显示条件之一）
    pub panel_hovered: Rc<Cell<bool>>,
    /// 上次离开 panel 的时刻（淡出用）
    pub left_at: Rc<Cell<Option<std::time::Instant>>>,
    /// 拖拽中的 thumb 起点偏移（Some = 拖拽中）
    drag_y: Rc<Cell<Option<f32>>>,
}

impl PspScrollbarState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// 创建滚动条元素（挂在滚动容器的平级 absolute 层内）。
pub fn psp_scrollbar(
    state: &PspScrollbarState,
    scroll: &gpui::ScrollHandle,
) -> AnyElement {
    PspScrollbarEl {
        state: state.clone(),
        scroll: scroll.clone(),
    }
    .into_any()
}

struct PspScrollbarEl {
    state: PspScrollbarState,
    scroll: gpui::ScrollHandle,
}

impl IntoElement for PspScrollbarEl {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for PspScrollbarEl {
    type RequestLayoutState = ();
    type PrepaintState = Option<gpui::Hitbox>;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name("psp-scrollbar".into()))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        let mut style = Style::default();
        style.size = size(px(12.).into(), px(1.).into());
        style.flex_grow = 1.;
        (window.request_layout(style, None, _cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request: &mut (),
        window: &mut Window,
        _cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        Some(window.insert_hitbox(_bounds, HitboxBehavior::default()))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request: &mut (),
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut gpui::App,
    ) {
        let hitbox = match prepaint.as_ref() {
            Some(h) => h.clone(),
            None => return,
        };
        let scroll = self.scroll.clone();
        let st = self.state.clone();
        let theme_t = crate::theme::theme();

        // ---- 几何 ----
        let viewport_h = scroll.bounds().size.height.to_f64() as f32;
        let max_off = scroll.max_offset().height.to_f64() as f32;
        let track_h = bounds.size.height.to_f64() as f32;
        let off_y = (-scroll.offset().y.to_f64() as f32).clamp(0., max_off.max(0.));
        let thumb_len = if max_off <= 0. {
            0.
        } else {
            (track_h * viewport_h / (viewport_h + max_off)).max(THUMB_MIN_LEN).min(track_h)
        };
        let thumb_y0 = TRACK_INSET
            + if max_off <= 0. {
                0.
            } else {
                (off_y / max_off) * (track_h - TRACK_INSET * 2. - thumb_len)
            };
        let hovered = hitbox.is_hovered(window);
        let dragging = st.drag_y.get().is_some();
        let thumb_w = if hovered || dragging { THUMB_W_ACTIVE } else { THUMB_W };

        // ---- 悬停中持续重绘（跟随 offset/透明度）----
        if hovered || dragging {
            let probe = hitbox.clone();
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, _| {
                if phase == DispatchPhase::Capture {
                    let _ = probe.is_hovered(window);
                    window.refresh();
                }
            });
        }

        // ---- 可见性：panel 内 || 淡出窗口内 ----
        let opacity = if dragging {
            1.
        } else if st.panel_hovered.get() || hovered {
            1.
        } else if let Some(left) = st.left_at.get() {
            let elapsed_ms = left.elapsed().as_millis();
            if elapsed_ms <= FADE_DELAY_MS {
                1.
            } else if elapsed_ms < FADE_DELAY_MS + FADE_LEN_MS {
                1. - (elapsed_ms - FADE_DELAY_MS) as f32 / FADE_LEN_MS as f32
            } else {
                0.
            }
        } else {
            0.
        };

        if opacity > 0. && thumb_len > 0. {
            let base = rgb(theme_t.text);
            // 常显 20%、悬停/拖拽 38%（text 薄纱，主题自适应）
            let alpha = if hovered || dragging { 0.38 } else { 0.20 } * opacity;
            let color = gpui::hsla(
                gpui::Hsla::from(base).h,
                gpui::Hsla::from(base).s,
                gpui::Hsla::from(base).l,
                alpha,
            );
            let thumb = Bounds::from_corner_and_size(
                gpui::Corner::TopRight,
                gpui::point(
                    bounds.right() - px(1.),
                    bounds.top() + px(thumb_y0),
                ),
                size(px(thumb_w), px(thumb_len)),
            );
            window.paint_quad(gpui::quad(
                thumb,
                gpui::Corners::all(px(thumb_w / 2.)),
                color,
                Edges::default(),
                gpui::transparent_black(),
                gpui::BorderStyle::default(),
            ));
        }

        // ---- 事件（全部 stop_propagation：不穿透 session 行）----
        // mousedown：thumb 上 = 开始拖拽；轨道上 = 翻页
        {
            let scroll = scroll.clone();
            let st = st.clone();
            window.on_mouse_event(
                move |ev: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !hitbox.is_hovered(window) {
                        return;
                    }
                    cx.stop_propagation();
                    let viewport_h = scroll.bounds().size.height.to_f64() as f32;
                    let max_off = scroll.max_offset().height.to_f64() as f32;
                    if max_off <= 0. {
                        return;
                    }
                    let track_h = bounds.size.height.to_f64() as f32;
                    let thumb_len = (track_h * viewport_h / (viewport_h + max_off))
                        .max(THUMB_MIN_LEN)
                        .min(track_h);
                    let off_y = (-scroll.offset().y.to_f64() as f32).clamp(0., max_off);
                    let thumb_y0 = TRACK_INSET
                        + (off_y / max_off) * (track_h - TRACK_INSET * 2. - thumb_len);
                    let rel_y = (ev.position.y - bounds.top()).to_f64() as f32;
                    if rel_y >= thumb_y0 && rel_y <= thumb_y0 + thumb_len {
                        // 拖拽：记录指针在 thumb 内的偏移
                        st.drag_y.set(Some(rel_y - thumb_y0));
                    } else {
                        // 轨道点击 = 翻页（thumb 中心对准点击点）
                        let new_thumb_y = (rel_y - thumb_len / 2. - TRACK_INSET)
                            .max(0.)
                            .min(track_h - TRACK_INSET * 2. - thumb_len);
                        let new_off = new_thumb_y / (track_h - TRACK_INSET * 2. - thumb_len)
                            * max_off;
                        scroll.set_offset(gpui::point(px(0.), px(-new_off)));
                        window.refresh();
                    }
                    window.refresh();
                },
            );
        }
        // mousemove：拖拽跟随
        {
            let scroll = scroll.clone();
            let st = st.clone();
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, _| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let Some(drag_off) = st.drag_y.get() else { return };
                let viewport_h = scroll.bounds().size.height.to_f64() as f32;
                let max_off = scroll.max_offset().height.to_f64() as f32;
                if max_off <= 0. {
                    return;
                }
                let track_h = bounds.size.height.to_f64() as f32;
                let thumb_len = (track_h * viewport_h / (viewport_h + max_off))
                    .max(THUMB_MIN_LEN)
                    .min(track_h);
                let rel_y = (ev.position.y - bounds.top()).to_f64() as f32 - drag_off;
                let pct = (rel_y - TRACK_INSET)
                    .max(0.)
                    / (track_h - TRACK_INSET * 2. - thumb_len).max(1.);
                scroll.set_offset(gpui::point(px(0.), px(-pct.clamp(0., 1.) * max_off)));
                window.refresh();
            });
        }
        // mouseup：结束拖拽
        {
            let st = st.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                if st.drag_y.get().is_some() {
                    st.drag_y.set(None);
                    window.refresh();
                }
            });
        }
    }
}
