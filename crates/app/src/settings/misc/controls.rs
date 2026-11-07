//! 「其他」页小构件：clean_stepper / dir_row / mono_value / switch。
//! 通用 stepper 已上移 [`crate::ui::kit::stepper`]（2026-11-07）。

use super::*;

/// 052 自动清理天数步进：[0,100] 步长 5，默认 0 显示「关闭」（= 不自动清理）。
pub(super) fn clean_stepper(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    stepper(
        "misc-clean",
        crate::services::workspace::auto_clean_days(),
        0,
        100,
        5,
        |v| {
            if v == 0 {
                tr("misc.auto_clean_off").to_string()
            } else {
                crate::i18n::tf("misc.auto_clean_days", &[("n", v.to_string())])
            }
        },
        crate::services::workspace::auto_clean_days,
        |v| {
            let mut s = crate::services::workspace::app_settings();
            s.auto_clean_days = Some(v);
            crate::services::workspace::save_app_settings(&s);
        },
        weak,
    )
}

/// 目录行：mono 路径占满左侧，「打开」按钮右对齐（系统文件浏览器打开）。
pub(super) fn dir_row(
    key: &str,
    path: &str,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let open_path = std::path::PathBuf::from(path);
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().flex_1().min_w_0().truncate().child(mono_value(path)))
        .child(
            div()
                .id(SharedString::from(format!("misc-open-{key}")))
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                // 文件树标题栏「在资源管理器中打开」同款 icon
                .child(crate::ui::icon("folder-input", 16., t.text_dim))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    crate::services::shell_open::open_in_explorer(&open_path);
                }),
        )
        .into_any_element()
}

fn mono_value(text: &str) -> gpui::AnyElement {
    let t = T();
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// 开关（原 34×19 私有实现 → 委托 [`switch_base`]，SW_MD 28×16 全局统一）。
pub(super) fn switch(
    id: &'static str,
    on: bool,
    apply: impl Fn(bool, &mut gpui::App) + 'static,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let _ = t; // 配色统一走 theme 语义，入参保留兼容既有调用点
    switch_base(id, on, false, apply)
}
