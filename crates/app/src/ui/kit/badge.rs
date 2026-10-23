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

/// 状态徽标：灰底胶囊 + 7px 状态点 + 10px 文本（scope_tag 同槽位家族，
/// 点色由调用方按语义给：等待 = WARN 黄、就绪/运行 = UNREAD 绿、
/// 失败 = t.danger、中性 = text_faint）。见 §10.4。
pub(crate) fn state_badge(label: &str, dot: u32) -> AnyElement {
    let t = T();
    div()
        .flex()
        .items_center()
        .gap(px(5.))
        .px(px(5.))
        .py(px(1.))
        .rounded(px(3.))
        .bg(gpui::hsla(0., 0., 0.5, 0.12))
        .child(status_dot(dot))
        .child(
            div()
                .text_size(crate::appearance::ui_size(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .into_any_element()
}
