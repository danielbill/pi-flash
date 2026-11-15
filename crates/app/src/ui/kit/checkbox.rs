//! 勾选框（14px 方框 + 选中打勾）。见 §10.4。
//!
//! 原先在 `session/mcp_picker.rs` 与 `session/pi_extension_picker.rs` 各有
//! 一份逐字符相同的实现（090 收口时上移）。

use gpui::prelude::*;
use gpui::{AnyElement, div, px, rgb};

use crate::appearance::theme::theme as T;

/// 14px 勾选框：选中 = accent 底 + 反色勾，未选 = 描边 + 面板底。
pub(crate) fn checkbox(checked: bool) -> AnyElement {
    let t = T();
    div()
        .size(px(14.))
        .flex_shrink_0()
        .rounded(px(3.))
        .border_1()
        .border_color(rgb(if checked { t.accent } else { t.border }))
        .bg(rgb(if checked { t.accent } else { t.bg_panel }))
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |d| {
            d.child(crate::ui::icon("check", 10., t.accent_contrast))
        })
        .into_any_element()
}
