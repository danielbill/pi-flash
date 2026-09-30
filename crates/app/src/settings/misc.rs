//! 设置「其他」页 (v54): pi 版本 / 启动恢复 / 默认加载项目数 / 工作区数据
//! 目录。set-row 版式（标题+描述左，控件右）。

use super::*;

pub(crate) fn mc_misc_view(
    _chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let mut col = div()
        .id("mc-misc")
        .flex()
        .flex_col()
        .child(section_title(tr("其他"), t));

    // pi 版本（vendored）
    col = col.child(set_row(
        tr("pi 版本"),
        tr("vendored pi-coding-agent"),
        mono_value(&format!(
            "v{}",
            pi_link::vendor::vendored_version().unwrap_or_default()
        )),
        t,
    ));

    // 启动恢复 switch
    let restore_on = crate::services::workspace::startup_restore();
    col = col.child(set_row(
        tr("启动恢复"),
        tr("启动时恢复上次激活的项目与会话"),
        switch(
            "misc-restore",
            restore_on,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |_c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.restore = Some(on);
                        crate::services::workspace::save_app_settings(&s);
                        cx.notify();
                    });
                }
            },
            t,
        ),
        t,
    ));

    // 默认加载项目数 stepper（1–10）
    let projects = crate::services::workspace::project_count();
    col = col.child(set_row(
        tr("默认加载项目数"),
        tr("启动时按最近使用加载的项目数量"),
        {
            let weak = weak.clone();
            stepper("misc-projects", projects, 1., 10., move |v, cx| {
                let _ = weak.update(cx, |_c, cx| {
                    let mut s = crate::services::workspace::app_settings();
                    s.projects = Some(v as usize);
                    crate::services::workspace::save_app_settings(&s);
                    cx.notify();
                });
            }, t)
        },
        t,
    ));

    // 工作区数据目录
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    col = col.child(set_row(
        tr("工作区数据目录"),
        tr("会话记忆 / 布局状态存放位置"),
        mono_value(&format!("{home}\\.pi\\agent")),
        t,
    ));

    col.into_any_element()
}

fn section_title(text: &str, t: &'static crate::theme::Theme) -> gpui::AnyElement {
    div()
        .text_size(px(16.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(t.text))
        .mb(px(4.))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// set-row：标题+描述在左，控件在右，底分隔线。
fn set_row(
    title: &str,
    desc: &str,
    control: gpui::AnyElement,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .py(px(13.))
        .border_b_1()
        .border_color(gpui::rgba(0xafc4ba40))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(title.to_string())),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(desc.to_string())),
                ),
        )
        .child(control)
        .into_any_element()
}

fn mono_value(text: &str) -> gpui::AnyElement {
    let t = T();
    div()
        .font_family("Consolas")
        .text_size(px(12.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// 34×19 开关。
fn switch(
    id: &'static str,
    on: bool,
    apply: impl Fn(bool, &mut gpui::App) + 'static,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w(px(34.))
        .h(px(19.))
        .rounded_full()
        .bg(rgb(if on { t.accent } else { t.border }))
        .relative()
        .cursor_pointer()
        .child(
            div()
                .absolute()
                .top(px(2.))
                .when(on, |d| d.left(px(17.)))
                .when(!on, |d| d.left(px(2.)))
                .size(px(15.))
                .rounded_full()
                .bg(rgb(0xffffff)),
        )
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            apply(!on, cx);
        })
        .into_any_element()
}

/// − n + 步进器。
fn stepper(
    id: &'static str,
    value: usize,
    min: f32,
    max: f32,
    apply: impl Fn(f32, &mut gpui::App) + Clone + 'static,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    let v = value as f32;
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .rounded(px(7.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .child(step_btn(
            SharedString::from(format!("{id}-dec")),
            "−",
            v > min,
            {
                let apply = apply.clone();
                move |cx| apply(v - 1., cx)
            },
            t,
        ))
        .child(
            div()
                .min_w(px(28.))
                .text_align(gpui::TextAlign::Center)
                .font_family("Consolas")
                .text_size(px(12.5))
                .text_color(rgb(t.text))
                .child(SharedString::from(value.to_string())),
        )
        .child(step_btn(
            SharedString::from(format!("{id}-inc")),
            "+",
            v < max,
            {
                let apply = apply.clone();
                move |cx| apply(v + 1., cx)
            },
            t,
        ))
        .into_any_element()
}

fn step_btn(
    id: SharedString,
    glyph: &'static str,
    enabled: bool,
    apply: impl Fn(&mut gpui::App) + 'static,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w(px(26.))
        .h(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.))
        .text_color(rgb(if enabled { t.text } else { t.text_faint }))
        .cursor_pointer()
        .when(enabled, |d| {
            d.hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    apply(cx);
                })
        })
        .child(SharedString::from(glyph))
        .into_any_element()
}
