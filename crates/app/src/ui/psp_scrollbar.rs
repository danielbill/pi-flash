//! psp 会话列表滚动条（v55）——**ZED `ui/components/scrollbar.rs` 移植**
//! （Regular 样式最小子集，结构一一对应）：
//! - ScrollbarState（Entity）：ThumbState 状态机 + parent hover + autohide
//! - ScrollbarElement：prepaint 算 thumb 布局 + parent hitbox；paint 画
//!   thumb（0.7 最大透明度混合 + autohide 淡出）；事件三件套
//! - mousedown：thumb 上=拖拽、轨道=翻页，`stop_propagation` 不穿透
//! - mousemove：parent 进入/经过 → 显示 + hover 检测；拖拽跟随
//! - mouseup：结束拖拽；parent 离开 → `schedule_auto_hide`（3s 淡出）
//!
//! 用户行为规格：panel 内常显（parent hover 驱动，与 zed `ParentHoverEvent::
//! Entered → show_scrollbars` 一致）、离开 3s 淡出、thumb 悬停加宽、贴边无
//! track。`menu_scrollbar` = / 菜单变体（v58）：可滚动即常显，无 hover 门。

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    point, px, relative, rgb, size, AnyElement, App, Bounds, Corners, DispatchPhase, Edges,
    Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, LayoutId,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Style, Window, prelude::*,
};

/// ZED `ScrollbarStyle::Regular.to_pixels()`
const WIDTH: f32 = 6.;
/// ZED `SCROLLBAR_PADDING`——thumb 容器相对 track 的内缩（含命中区加宽）
const PADDING: f32 = 4.;
/// 悬停/拖拽加宽（用户定稿 +4px：6→10）
const WIDTH_ACTIVE: f32 = 10.;
/// ZED `MINIMUM_THUMB_SIZE`
const MIN_THUMB: f32 = 25.;
/// ZED `MAXIMUM_OPACITY`
const MAX_OPACITY: f32 = 0.7;
/// autohide（ZED ScrollbarAutoHide 语义：离开 parent 后保持到计时结束）
const AUTOHIDE_MS: u64 = 3000;

/// 共享面板状态（挂 Chat；滚动条元素与容器 on_hover 两处读写）。
#[derive(Clone, Default)]
pub struct PspScrollbarState {
    parent_hovered: Rc<Cell<bool>>,
    /// 上次"panel 悬停"刷新时刻（autohide 基准；悬停中持续刷新 = 常显）
    last_active: Rc<Cell<Option<Instant>>>,
}


impl PspScrollbarState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 容器 on_hover 调用（zed 的 update_parent_hovered 等价入口）。
    pub fn set_parent_hovered(&self, hovered: bool) {
        self.parent_hovered.set(hovered);
        if hovered {
            self.last_active.set(Some(Instant::now()));
        }
    }
}

pub fn psp_scrollbar(state: &PspScrollbarState, scroll: &gpui::ScrollHandle) -> AnyElement {
    ScrollbarElement {
        state: state.clone(),
        scroll: scroll.clone(),
        always_visible: false,
    }
    .into_any()
}

/// 菜单用变体：可滚动即常显（弹出菜单指针通常不在其上，parent-hover
/// 驱动的显隐永远等不到进入），其余行为（thumb 拖拽/翻页/悬停加宽）同源。
pub fn menu_scrollbar(scroll: &gpui::ScrollHandle) -> AnyElement {
    ScrollbarElement {
        state: PspScrollbarState::default(),
        scroll: scroll.clone(),
        always_visible: true,
    }
    .into_any()
}

struct ScrollbarElement {
    state: PspScrollbarState,
    scroll: gpui::ScrollHandle,
    /// true = 不看 parent hover，可滚动即常显（/ 菜单）
    always_visible: bool,
}

/// prepaint 产物（zed ScrollbarLayout + parent hitbox）
struct Layout {
    thumb_bounds: Bounds<Pixels>,
    /// 命中区 = thumb 容器（含 padding，好拾取）
    hit_bounds: Bounds<Pixels>,
    parent_hitbox: Hitbox,
}

impl IntoElement for ScrollbarElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ScrollbarElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<Layout>;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name(if self.always_visible {
            "menu-scrollbar".into()
        } else {
            "psp-scrollbar".into()
        }))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // zed: absolute inset 0, size 100%
        let style = Style {
            position: gpui::Position::Absolute,
            inset: Edges::default(),
            size: size(relative(1.), relative(1.)).map(Into::into),
            ..Default::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let parent_hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        let viewport_h = self.scroll.bounds().size.height.to_f64() as f32;
        let max_off = self.scroll.max_offset().height.to_f64() as f32;
        if max_off <= 0. || viewport_h <= 0. {
            return None;
        }
        // zed thumb_ranges: visible % → size，min 25px
        let visible_pct = viewport_h / (viewport_h + max_off);
        let track_h = bounds.size.height.to_f64() as f32 - PADDING * 2.;
        let thumb_len = (track_h * visible_pct).max(MIN_THUMB).min(track_h);
        let off_y = (-self.scroll.offset().y.to_f64() as f32).clamp(0., max_off);
        let start = (off_y / max_off) * (track_h - thumb_len);

        // zed: track 锚 TopRight；Regular 样式 thumb 容器 dilate(-PADDING)
        let thumb_bounds = Bounds::<Pixels>::from_corner_and_size(
            gpui::Corner::TopRight,
            point(
                bounds.right() - px(PADDING),
                bounds.top() + px(PADDING + start),
            ),
            size(px(WIDTH), px(thumb_len)),
        );
        // 命中区比 thumb 宽（padding 外扩，zed 用 track_bounds 做命中）
        let hit_bounds = Bounds::<Pixels>::from_corner_and_size(
            gpui::Corner::TopRight,
            point(bounds.right(), bounds.top()),
            size(px(WIDTH + PADDING * 2.), bounds.size.height),
        );

        Some(Layout {
            thumb_bounds,
            hit_bounds,
            parent_hitbox,
        })
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request: &mut (),
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let Some(layout) = prepaint.take() else {
            return;
        };
        let t = crate::theme::theme();
        let parent_hovered = self.always_visible
            || (self.state.parent_hovered.get() && layout.parent_hitbox.is_hovered(window));

        // ---- 可见性（zed VisibilityState 简化）：parent 内常显；离开后
        // AUTOHIDE_MS 内保持（由 mousemove 持续刷新 last_active），超时渐隐 ----
        // zed reveal：offset/内容高度相对上帧变化 = 用户滚动 → 重置
        // autohide 计时（淡出后滚轮一动立刻复现）
        let off_now = self.scroll.offset().y.to_f64() as f32;
        let max_now = self.scroll.max_offset().height.to_f64() as f32;
        let scrolled = LAST_OFFSET.with(|c| {
            let prev = c.get();
            c.set(Some((off_now, max_now)));
            prev.is_some_and(|(po, pm)| {
                (po - off_now).abs() > 0.5 || (pm - max_now).abs() > 0.5
            })
        });
        if scrolled {
            self.state.last_active.set(Some(Instant::now()));
        }
        let hovered = layout.hit_bounds.contains(&window.mouse_position());
        let active = self.state.last_active.get();
        let opacity = if parent_hovered || hovered {
            MAX_OPACITY
        } else if let Some(at) = active {
            let el = at.elapsed();
            if el < Duration::from_millis(AUTOHIDE_MS) {
                MAX_OPACITY
            } else if el < Duration::from_millis(AUTOHIDE_MS + 1000) {
                // 1s 线性淡出
                MAX_OPACITY
                    * (1. - (el.as_millis() - AUTOHIDE_MS as u128) as f32 / 1000.).max(0.)
            } else {
                0.
            }
        } else {
            0.
        };

        // thumb 上悬停（zed update_hovered_thumb）
        let thumb_hovered = layout.thumb_bounds.contains(&window.mouse_position());
        let dragging = DRAG.with(|c| c.borrow().is_some());

        if opacity > 0. {
            // 悬停/拖拽加宽 4px（用户定稿）：右缘锚定不动，向左扩
            let mut draw_bounds = layout.thumb_bounds;
            if thumb_hovered || dragging {
                draw_bounds.origin.x -= px(4.);
                draw_bounds.size.width += px(4.);
            }
            let base = gpui::Hsla::from(rgb(t.text));
            let a = if thumb_hovered || dragging {
                0.38 * opacity / MAX_OPACITY
            } else {
                0.20 * opacity / MAX_OPACITY
            };
            let color = gpui::hsla(base.h, base.s, base.l, a.min(1.));
            // zed Regular: 全圆角（clamp 到尺寸半宽）
            window.paint_quad(gpui::quad(
                draw_bounds,
                Corners::all(px(WIDTH_ACTIVE / 2.)),
                color,
                Edges::default(),
                gpui::transparent_black(),
                gpui::BorderStyle::default(),
            ));
        }
        window.set_cursor_style(gpui::CursorStyle::Arrow, &layout.parent_hitbox);

        // ---- 事件三件套（zed 同款：全部 stop_propagation）----
        let capture = if dragging {
            DispatchPhase::Capture
        } else {
            DispatchPhase::Bubble
        };

        // mousedown：thumb=拖拽 / track=翻页
        {
            let scroll = self.scroll.clone();
            let st = self.state.clone();
            let hit = layout.hit_bounds.clone();
            let thumb = layout.thumb_bounds.clone();
            window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
                if phase != capture || ev.button != gpui::MouseButton::Left {
                    return;
                }
                if !hit.contains(&ev.position) {
                    return;
                }
                cx.stop_propagation();
                if thumb.contains(&ev.position) {
                    let offset = ev.position.y - thumb.origin.y;
                    DRAG.with(|c| *c.borrow_mut() = Some(offset.to_f64() as f32));
                    st.last_active.set(Some(Instant::now()));
                } else {
                    // zed compute_click_offset(TrackClick)：thumb 中心对准点击
                    let viewport_h = scroll.bounds().size.height.to_f64() as f32;
                    let max_off = scroll.max_offset().height.to_f64() as f32;
                    let track_h = hit.size.height.to_f64() as f32 - PADDING * 2.;
                    let visible_pct = viewport_h / (viewport_h + max_off);
                    let thumb_len = (track_h * visible_pct).max(MIN_THUMB).min(track_h);
                    let rel = (ev.position.y.to_f64() as f32
                        - hit.origin.y.to_f64() as f32
                        - PADDING
                        - thumb_len / 2.)
                    .clamp(0., track_h - thumb_len);
                    let new_off = if track_h > thumb_len {
                        rel / (track_h - thumb_len) * max_off
                    } else {
                        0.
                    };
                    scroll.set_offset(point(px(0.), px(-new_off)));
                    st.last_active.set(Some(Instant::now()));
                }
                window.refresh();
            });
        }

        // mousemove：拖拽跟随 + parent 进入显示 + hover 加宽
        {
            let scroll = self.scroll.clone();
            let st = self.state.clone();
            let hit = layout.hit_bounds.clone();
            let parent = layout.parent_hitbox.clone();
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                if phase != capture {
                    return;
                }
                let drag_y: Option<f32> = DRAG.with(|c| *c.borrow());
                if let Some(drag_off) = drag_y {
                    if ev.dragging() {
                        let viewport_h = scroll.bounds().size.height.to_f64() as f32;
                        let max_off = scroll.max_offset().height.to_f64() as f32;
                        let track_h = hit.size.height.to_f64() as f32 - PADDING * 2.;
                        let visible_pct = viewport_h / (viewport_h + max_off);
                        let thumb_len = (track_h * visible_pct).max(MIN_THUMB).min(track_h);
                        let rel = (ev.position.y.to_f64() as f32
                            - hit.origin.y.to_f64() as f32
                            - PADDING
                            - drag_off)
                        .clamp(0., track_h - thumb_len);
                        let new_off = if track_h > thumb_len {
                            rel / (track_h - thumb_len) * max_off
                        } else {
                            0.
                        };
                        scroll.set_offset(point(px(0.), px(-new_off)));
                        window.refresh();
                        cx.stop_propagation();
                    }
                    return;
                }
                // parent 悬停中：持续刷新 last_active（常显）；进入时已由
                // 容器 on_hover 置位。thumb hover 状态变化时刷新重绘。
                if parent.is_hovered(window) {
                    st.last_active.set(Some(Instant::now()));
                }
                let now_hover = hit.contains(&window.mouse_position());
                let was = THUMB_HOVER.with(|c| c.get());
                if now_hover != was {
                    THUMB_HOVER.with(|c| c.set(now_hover));
                    window.refresh();
                }
            });
        }

        // mouseup：结束拖拽 + 离开 parent 时启动 autohide
        {
            let st = self.state.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _| {
                if phase != capture {
                    return;
                }
                if DRAG.with(|c| c.borrow().is_some()) {
                    DRAG.with(|c| c.borrow_mut().take());
                    st.last_active.set(Some(Instant::now()));
                    window.refresh();
                }
            });
        }
    }
}

// 拖拽上下文（thumb 内偏移, px）。thread_local：与 gpui 事件单线程模型一致。
thread_local! {
    static DRAG: std::cell::RefCell<Option<f32>> = const { std::cell::RefCell::new(None) };
    static THUMB_HOVER: Cell<bool> = const { Cell::new(false) };
    /// 上帧 (offset.y, max_offset.height)——zed ScrollbarPosition reveal 检测
    static LAST_OFFSET: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
}
