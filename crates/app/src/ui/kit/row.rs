//! 行与文本块：`set_row`（标题/描述 + 右侧控件 + 底分隔线）、`field`、
//! `section_title`、`note` / `error_note`、`mono_text`。见 §10.3。

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, SharedString, div, px, rgb};

use crate::appearance::theme::theme as T;

/// ConfigSectionTitle：11px 600 大写感小标题。
pub(crate) fn section_title(text: &str) -> AnyElement {
    let t = T();
    div()
        .text_size(crate::appearance::ui_size(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// ConfigField：label 上、控件下。
pub(crate) fn field(label: &str, control: impl gpui::IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(section_title(label))
        .child(control)
        .into_any_element()
}

/// 说明行（note / describedby 的可见文本）。
pub(crate) fn note(text: &str) -> AnyElement {
    let t = T();
    div()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// 右对齐说明行（042-1 用户定稿：提示信息与操作栏同侧）。
pub(crate) fn note_right(text: &str) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_end()
        .text_right()
        .child(note(text))
        .into_any_element()
}

/// 右对齐错误行。
pub(crate) fn error_note_right(text: &str) -> AnyElement {
    div()
        .w_full()
        .flex()
        .justify_end()
        .text_right()
        .child(error_note(text))
        .into_any_element()
}

pub(crate) fn error_note(text: &str) -> AnyElement {
    div()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(T().danger))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// mono 值块（路径 / 命令等）。
pub(crate) fn mono_text(text: String, dim: bool) -> AnyElement {
    let t = T();
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(11.))
        .text_color(if dim { rgb(t.text_dim) } else { rgb(t.text) })
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(SharedString::from(text))
        .into_any_element()
}

/// set-row：标题+描述在左，控件在右，底分隔线（界面/其他两页单列 rows 版式）。
/// set-row：标题+描述在左，控件在右，底分隔线（界面/其他两页单列 rows 版式）；
/// desc 传空则只渲染标题。
pub(crate) fn set_row(
    title: &str,
    desc: &str,
    control: gpui::AnyElement,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let mut left = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            // 标题与「界面」页 section_label 同款（13 semibold text_muted）
            div()
                .text_size(crate::appearance::ui_size(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(title.to_string())),
        );
    if !desc.is_empty() {
        left = left.child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(desc.to_string())),
        );
    }
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .py(px(13.))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x40)))
        .child(left)
        .child(control)
        .into_any_element()
}
