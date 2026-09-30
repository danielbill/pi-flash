//! UI primitives shared across all components (pi-web's icon-level layer:
//! ThinkingIcon / ThemeIcon / spinner etc. + the app-wide TextInput).

pub mod editor_input;
pub mod text_input;

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
