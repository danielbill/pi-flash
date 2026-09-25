//! UI primitives shared across all components (pi-web's icon-level layer:
//! ThinkingIcon / ThemeIcon / spinner etc. + the app-wide TextInput).

pub mod text_input;

pub use text_input::TextInput;

use gpui::{Animation, AnimationExt, SharedString, Styled, prelude::*};

use crate::theme::theme as T;

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

/// Toolbar pill (icon + label, hover highlight).
pub fn pill(
    id: &'static str,
    icon_name: &'static str,
    label: SharedString,
) -> gpui::AnyElement {
    let t = T();
    gpui::div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(gpui::rgb(t.border))
        .flex()
        .items_center()
        .gap_1p5()
        .text_xs()
        .text_color(gpui::rgb(t.text_muted))
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgb(t.bg_hover)).text_color(gpui::rgb(t.text)))
        .child(icon(icon_name, 12., t.text_muted))
        .child(label)
        .into_any_element()
}
