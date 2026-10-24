//! 徽标 / 状态点（靛蓝 reasoning 徽标、scope 标签、7px 状态点）。
//! 尺寸见 `docs/UI设计/UI组件规范.md` §10.4。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, div, px, rgb};

use crate::appearance::theme::theme as T;

pub(crate) fn indigo_bg() -> gpui::Hsla {
    gpui::hsla(0.63, 0.86, 0.62, 0.12)
}
pub(crate) fn indigo_fg() -> gpui::Hsla {
    gpui::hsla(0.63, 0.86, 0.62, 0.85)
}

/// 7px 状态点。
pub(crate) fn status_dot(color: u32) -> AnyElement {
    div()
        .size(px(7.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(color))
        .into_any_element()
}

/// scope 徽标：项目 = 靛蓝，其余 = 灰。
pub(crate) fn scope_tag(label: &str, project: bool) -> AnyElement {
    let t = T();
    let dim: gpui::Hsla = rgb(t.text_dim).into();
    div()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(3.))
        .bg(if project {
            indigo_bg()
        } else {
            gpui::hsla(0., 0., 0.5, 0.12)
        })
        .text_size(crate::appearance::ui_size(10.))
        .text_color(if project { indigo_fg() } else { dim })
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}
