//! 设置「其他」页 (v54): pi 版本 / 启动恢复 / 默认加载会话数 / 工作区数据
//! 目录。set-row 版式（标题+描述左，控件右）。

use super::*;

pub(crate) fn mc_misc_view(
    _chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let mut col = div().id("mc-misc").flex().flex_col();

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

    // 展示思考 switch（思考块始终渲染；关（默认）=新思考块收成一行，
    // 开=默认展开全文。用户手动开合过的块以显式状态为准）
    let show_thinking = crate::services::workspace::show_thinking();
    col = col.child(set_row(
        tr("展示思考"),
        tr("开启时思考块默认展开全文，关闭时默认收起为一行"),
        switch(
            "misc-show-thinking",
            show_thinking,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |_c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.show_thinking = Some(on);
                        crate::services::workspace::save_app_settings(&s);
                        cx.notify();
                    });
                }
            },
            t,
        ),
        t,
    ));

    // 加载时间窗口（7/14/30 天档位）：启动按最近活动清单加载窗口内会话
    let days = crate::services::workspace::load_window_days();
    col = col.child(set_row(
        tr("加载时间窗口"),
        tr("加载最近几天内活跃的会话"),
        {
            let weak = weak.clone();
            window_row(days, &weak, t)
        },
        t,
    ));

    // 提示音 switch（v60 从界面页挪入）
    let sound_on = crate::services::workspace::load_sound_pref();
    col = col.child(set_row(
        tr("提示音"),
        tr("agent 运行结束播放系统提示音"),
        switch(
            "misc-sound",
            sound_on,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.sound_on = on;
                        crate::services::workspace::save_sound_pref(on);
                        if on {
                            crate::services::workspace::play_notify_sound();
                        }
                        cx.notify();
                    });
                }
            },
            t,
        ),
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

/// 档位：7/14/30 天一排（仿语言按钮，active 描边）。
fn window_row(
    days: u64,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    const CHOICES: [(u64, &str); 3] = [(7, "7 天"), (14, "14 天"), (30, "30 天")];
    div()
        .flex()
        .gap(px(8.))
        .children(CHOICES.iter().map(|(v, label)| {
            let active = days == *v;
            let weak_item = weak.clone();
            let value = *v;
            div()
                .id(SharedString::from(format!("misc-window-{v}")))
                .w(px(72.))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(5.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                .flex()
                .items_center()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(rgb(if active { t.text } else { t.text_muted }))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |_c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.load_window_days = Some(value);
                        crate::services::workspace::save_app_settings(&s);
                        cx.notify();
                    });
                })
                .child(SharedString::from(label.to_string()))
                .into_any_element()
        }))
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
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x40)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(title.to_string())),
                )
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(11.5))
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
        .font_family(crate::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
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

