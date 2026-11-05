//! psp 项目路径 tooltip（v54）：项目行悬停的深色小卡（短名 + 全路径
//! mono），跟随鼠标。自 psp_overlays.rs 切出（itlq.12 超规拆分，纯结构
//! 搬移）。

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::ui::popup::PopupEdges;

/// 项目行 tooltip：短名 + 全路径（mono），随主题（与 toast 同族：bg_panel
/// + border + text），跟随鼠标。锚点 = 光标，统一算法：光标在右半 → 贴
/// 光标左侧弹，下半 → 贴光标上方弹。t 从外层传入（psp_overlays 已取）。
pub(crate) fn proj_tip(
    path: &std::path::PathBuf,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static crate::appearance::theme::Theme,
) -> impl gpui::IntoElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let full = path.to_string_lossy().to_string();
    PopupEdges::around((x, y), vp, 14., 16.)
        .apply(
            div()
                .id("psp-proj-tip")
                .absolute()
                .max_w(px(300.))
                .px(px(11.))
                .py(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(1.)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(name)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(10.5))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(full)),
        )
}
