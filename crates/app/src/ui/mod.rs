//! UI primitives shared across all components (pi-web's icon-level layer:
//! ThinkingIcon / ThemeIcon / spinner etc. + the app-wide TextInput).

pub mod composer_input;
pub mod dropdown;
pub mod list_handle;
pub mod overlay;
pub mod psp_scrollbar;
pub mod text_input;
pub mod vlist;

pub use composer_input::ComposerInput;
pub use dropdown::{dropdown, DropdownState};
pub use vlist::{VListHeight, vlist};
pub use text_input::TextInput;

use gpui::{Animation, AnimationExt, SharedString, Styled, prelude::*};


/// Embedded-SVG icon (`crates/app/assets/icons/{name}.svg`).
pub fn icon(name: &'static str, size: f32, color: u32) -> gpui::AnyElement {
    gpui::svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .text_color(gpui::rgb(color))
        .size(gpui::px(size))
        .into_any_element()
}

/// icon at RGBA（`0xRRGGBBAA`，alpha 生效）：超大水印等纯装饰用法。
pub fn icon_alpha(name: &'static str, size: f32, rgba: u32) -> gpui::AnyElement {
    gpui::svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .text_color(gpui::rgba(rgba))
        .size(gpui::px(size))
        .into_any_element()
}

/// 完整资产路径版 icon（`icons/file_icons/rust.svg`）：Zed 文件图标主题
/// 表里存的就是这种路径，直取不再拼前缀。
pub fn icon_path(path: &'static str, size: f32, color: u32) -> gpui::AnyElement {
    gpui::svg()
        .path(SharedString::from(path))
        .text_color(gpui::rgb(color))
        .size(gpui::px(size))
        .into_any_element()
}

/// provider id → sprite 符号名（pi-web `ProviderIcon.tsx` 的映射表）。
/// 不在表内的 provider 走首字母方块兜底（与 pi-web 相同）。
fn provider_symbol(id: &str) -> Option<&'static str> {
    Some(match id {
        "anthropic" => "anthropic",
        "openai" | "openai-codex" => "openai",
        "google" | "google-vertex" => "google",
        "ant-ling" => "antgroup",
        "deepseek" => "deepseek",
        "groq" => "groq",
        "mistral" => "mistral",
        "moonshotai" | "moonshotai-cn" | "moonshot" => "moonshot",
        "minimax" | "minimax-cn" => "minimax",
        "fireworks" => "fireworks",
        "huggingface" => "huggingface",
        "cerebras" => "cerebras",
        "openrouter" => "openrouter",
        "xai" | "grok" => "xai",
        "cloudflare-ai-gateway" | "cloudflare-workers-ai" => "cloudflare",
        "vercel-ai-gateway" => "vercel",
        "github-copilot" => "githubcopilot",
        "amazon-bedrock" => "aws",
        "azure-openai-responses" => "azure",
        "kimi-coding" => "kimi",
        "nvidia" => "nvidia",
        "opencode" | "opencode-go" => "opencode",
        "qwen" => "qwen",
        "xiaomi" | "xiaomi-token-plan-ams" | "xiaomi-token-plan-cn" | "xiaomi-token-plan-sgp" => {
            "xiaomimimo"
        }
        "zai" | "zai-coding-cn" => "zai",
        "zhipu" => "zhipu",
        "cohere" => "cohere",
        "perplexity" => "perplexity",
        "together" => "together",
        _ => return None,
    })
}

/// Provider logo（设置-模型页侧栏等处）：已知 provider 渲染对应
/// `icons/provider/*.svg`（gpui 按 alpha 着色，logo 一律单色 tint），
/// 未知 provider 用首字母圆角方块兜底——pi-web `ProviderIcon` 同款。
pub fn provider_icon(id: &str, size: f32, color: u32) -> gpui::AnyElement {
    if let Some(symbol) = provider_symbol(id) {
        return gpui::svg()
            .path(SharedString::from(format!("icons/provider/{symbol}.svg")))
            .text_color(gpui::rgb(color))
            .size(gpui::px(size))
            .flex_shrink_0()
            .into_any_element();
    }
    // 兜底：按 -/_ 切分取前两段首字母（"freeflow" → FF，"amazon-bedrock" → AB）
    let label: String = id
        .split(['-', '_'])
        .filter(|p| !p.is_empty())
        .take(2)
        .filter_map(|p| p.chars().next())
        .flat_map(|c| c.to_uppercase())
        .collect();
    let label = if label.is_empty() { "?".to_string() } else { label };
    let t = crate::theme::theme();
    gpui::div()
        .w(gpui::px(size))
        .h(gpui::px(size))
        .flex_shrink_0()
        .rounded(gpui::px(4.))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .flex()
        .items_center()
        .justify_center()
        .text_size(gpui::px((size * 0.42).max(8.)))
        .text_color(gpui::rgb(color))
        .child(SharedString::from(label))
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
        _cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size = gpui::Size {
            width: gpui::px(self.size + 2.).into(),
            height: gpui::px(self.size + 2.).into(),
        };
        let layout_id = window.request_layout(style, None, _cx);
        (layout_id, layout_id)
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        let _ = request_layout;
        // hitbox 必须在 prepaint 建（insert_hitbox debug_assert_prepaint）；
        // 悬停监听在 paint 期经 PrepaintState 注册（on_mouse_event 只允许
        // paint 期调用）
        Some(window.insert_hitbox(bounds, gpui::HitboxBehavior::default()))
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
        // 无条件挂 mousemove 监听（paint 期注册）：悬停中标脏保持续渲染，
        // 离开悬停的那次 move 也经它触发重绘（refresh 幂等合帧，开销即
        // gpui 自身 hover 样式的同款成本）
        if let Some(hitbox) = prepaint.as_ref() {
            let probe = hitbox.clone();
            window.on_mouse_event(
                move |_: &gpui::MouseMoveEvent, phase, window, _cx| {
                    if phase == gpui::DispatchPhase::Capture && probe.is_hovered(window) {
                        window.refresh();
                    }
                },
            );
        }
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

/// 透明测量元素：prepaint 把子元素布局高度写入 slot。读数天然滞后一帧
/// （本帧 build 时读到的是上一帧 prepaint 写入值），只适合弱实时场景，
/// 如导航刻度条按 composer 高度做垂直偏移（033）。
pub fn measure_height(
    id: impl Into<gpui::ElementId>,
    slot: &std::rc::Rc<std::cell::Cell<f32>>,
    child: impl gpui::IntoElement,
) -> MeasureHeight {
    MeasureHeight {
        id: id.into(),
        slot: slot.clone(),
        child: child.into_any_element(),
    }
}

pub struct MeasureHeight {
    id: gpui::ElementId,
    slot: std::rc::Rc<std::cell::Cell<f32>>,
    child: gpui::AnyElement,
}

impl gpui::IntoElement for MeasureHeight {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for MeasureHeight {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        Some(self.id.clone())
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
    ) -> (gpui::LayoutId, ()) {
        // 布局完全委托子元素（透明包装），本元素的尺寸即子元素尺寸
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _request: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.slot.set(bounds.size.height.to_f64() as f32);
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        _request: &mut (),
        _prepaint: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.child.paint(window, cx);
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
