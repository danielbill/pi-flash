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

/// icon hover 动效（v55 统一规格）：悬停上抬 1px + 尺寸 +1px（≈scale 1.06）。
/// 仅用于**可交互图标按钮**；静态/状态图标用 icon()。
///
/// 实现注记（v55.1 修正）：动效必须由**外层按钮容器**的 hover 驱动——
/// svg 自身 `.hover()` 的判定在容器 hitbox 之内时不可达，挂在 svg 上等于
/// 没写（上一版就栽在这）。本 helper 返回一个包装 div：`.hover_map` 在
/// 悬停态给内部图标换 18px→19px 的放大版本并负 margin 上抬。gpui 无样式
/// transition，动效是瞬间的（与设计稿 0.12s ease 的差异已知）。
pub fn icon_hover(name: &'static str, size: f32, color: u32) -> gpui::AnyElement {
    let path: SharedString = SharedString::from(format!("icons/{name}.svg"));
    div()
        .id(SharedString::from(format!("ih-{name}-{size}")))
        .flex()
        .items_center()
        .justify_center()
        .size(px(size + 2.))
        .hover(move |s| s.mt(px(-1.)).text_color(gpui::rgb(color)))
        .child(
            gpui::svg()
                .path(path.clone())
                .text_color(gpui::rgb(color))
                .size(px(size)),
        )
        .into_any_element()
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
