//! UI primitives shared across all components (pi-web's icon-level layer:
//! ThinkingIcon / ThemeIcon / spinner etc. + the app-wide TextInput).

pub mod editor_input;
pub mod text_input;

pub use text_input::TextInput;

use gpui::{Animation, AnimationExt, SharedString, Styled, div, prelude::*, px};


/// Embedded-SVG icon (`crates/app/assets/icons/{name}.svg`).
pub fn icon(name: &'static str, size: f32, color: u32) -> gpui::AnyElement {
    gpui::svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .text_color(gpui::rgb(color))
        .size(gpui::px(size))
        .into_any_element()
}

/// icon hover 动效（v55 统一规格）：悬停上抬 1px + 放大 6%，120ms。
/// 仅用于**可交互图标按钮**；静态/状态图标用 icon()。
///
/// 实现（v55.3 定案）：gpui 的 `.hover()` refinement 只影响 paint 样式，
/// 布局属性（mt/w/h）写进去不会重排版（v55.1）、构造期样式切不走（v55.2
/// 未提交即弃）——形变必须自定义 Element：prepaint 建 hitbox 并挂 mouse
/// move 监听（悬停变化时 notify 本视图，与 gpui 自身 hover 同机制），
/// paint 期按 `hitbox.is_hovered` 选 `Transformation`（translate ∘ scale，
/// paint 期矩阵）。零状态，每帧自校正。
pub fn icon_hover(name: &'static str, size: f32, color: u32) -> gpui::AnyElement {
    HoverIcon {
        path: SharedString::from(format!("icons/{name}.svg")),
        size,
        color,
        id: SharedString::from(format!("ih-{name}-{size}")),
    }
    .into_any()
}

struct HoverIcon {
    path: SharedString,
    size: f32,
    color: u32,
    id: SharedString,
}

impl gpui::IntoElement for HoverIcon {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for HoverIcon {
    type RequestLayoutState = gpui::LayoutId;
    type PrepaintState = Option<gpui::Hitbox>;

    fn id(&self) -> Option<gpui::ElementId> {
        Some(gpui::ElementId::Name(self.id.clone().into()))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size = gpui::Size {
            width: gpui::px(self.size + 2.).into(),
            height: gpui::px(self.size + 2.).into(),
        };
        let layout_id = window.request_layout(style, None, cx);
        (layout_id, layout_id)
    }

    fn prepaint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        let _ = request_layout;
        let hitbox = window.insert_hitbox(bounds, gpui::HitboxBehavior::default());
        // 悬停变化 → window.refresh()（重绘下一帧，paint 按 is_hovered 重选
        // transformation；与 gpui 自身 hover 样式的 notify 机制一致）。
        // 上一帧悬停值存 element state（id 由框架经 prepaint 传入）。
        // gpui 的 hover 样式监听（div.rs Interactivity::paint）在
        // hover_style.is_some() 时自动挂"悬停变化 → notify"事件——借这个
        // 机制：interactivity 需要的话见 v55.3 注记。此处手工挂同款监听：
        // 与 Interactivity 一样在 capture 期比较 hitbox 前后状态，变化即
        // refresh。前后值存窗口级 element state（key 用本元素 id —— prepaint
        // 收到的 GlobalElementId 不可 clone，改用每元素唯一 ElementId 字符串
        // 配合 window.with_element_state 的 GlobalElementId 要求 → 退而求
        // 其次：状态直接放闭包外的 HitboxId → 不可能。最终方案：无条件
        // refresh（mousemove 高频但 refresh 幂等合并为一帧，开销可忽略——
        // gpui Interactivity 的实现也是逐 mousemove 比较后 notify，等价）。
        let probe = hitbox.clone();
        let was_hovered_cell = std::cell::Cell::new(false);
        let cell_ref = &was_hovered_cell;
        let _ = cell_ref;
        window.on_mouse_event({
            move |_: &gpui::MouseMoveEvent, phase, window, _cx| {
                if phase == gpui::DispatchPhase::Capture {
                    let now = probe.is_hovered(window);
                    // Cell 无法跨帧存活 —— 依赖 refresh 幂等：hover 中每
                    // mousemove 标脏一次，gpui 合帧后实际重绘频率不变
                    if now {
                        window.refresh();
                    }
                }
            }
        });
        Some(hitbox)
    }

    fn paint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) {
        let _ = request_layout;
        let hovered = prepaint
            .as_ref()
            .is_some_and(|h| h.is_hovered(window));
        // 与 Svg::Transformation::into_matrix 同式的矩阵：绕中心 scale，再平移
        let sf = window.scale_factor();
        let c = bounds.center();
        let cx = c.x.to_f64() as f32 * sf;
        let cy = c.y.to_f64() as f32 * sf;
        let (sx, sy) = if hovered { (1.06, 1.06) } else { (1.0, 1.0) };
        let lift = if hovered { -1.0 * sf } else { 0.0 };
        let mut matrix = gpui::TransformationMatrix::unit();
        // 与 Svg Transformation::into_matrix 同式：move origin → scale →
        // move back，再叠加整体上移 lift
        matrix = matrix.translate(gpui::point(
            gpui::ScaledPixels::from(cx + lift),
            gpui::ScaledPixels::from(cy),
        ));
        matrix = matrix.scale(gpui::size(sx, sy));
        matrix = matrix.translate(gpui::point(
            gpui::ScaledPixels::from(-cx),
            gpui::ScaledPixels::from(-cy),
        ));
        let _ = window.paint_svg(bounds, self.path.clone(), matrix, gpui::rgb(self.color).into(), _cx);
    }
}

/// Rotating arc spinner (pi-web RunningSessionIndicator: the loader SVG
/// spun 360°/0.9s, SMIL parity via with_animation).
pub fn spinner(size: f32, color: u32) -> gpui::AnyElement {
    gpui::svg()
        .path(SharedString::from("icons/loader.svg"))
        .text_color(gpui::rgb(color))
        .size(gpui::px(size))
        .with_animation(
            "spin",
            Animation::new(std::time::Duration::from_millis(900)).repeat(),
            |el, delta| {
                el.with_transformation(
                    gpui::Transformation::rotate(gpui::radians(
                        delta * std::f32::consts::TAU,
                    )),
                )
            },
        )
        .into_any_element()
}
