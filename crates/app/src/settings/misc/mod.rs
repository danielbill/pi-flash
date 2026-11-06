//! 设置「其他」页 (v54): pi 版本 / 启动恢复 / 加载时间窗口 / 自动折叠时间 /
//! 会话显示数 / 提示音 / 工作区数据目录。set-row 版式（标题+描述左，控件右）。
//! 展示思考 / 文件树 Git 标识已挪至「界面」页。
//!
//! itlq.2 拆分（只搬不改）：本文件 = 页骨架与各行；controls = 步进器/
//! 目录行/开关等小构件。

use super::*;

pub(crate) mod controls;
use controls::{clean_stepper, dir_row, stepper, switch};

pub(crate) fn mc_misc_view(_chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let mut col = div().id("mc-misc").flex().flex_col();

    // 启动恢复 switch
    let restore_on = crate::services::workspace::startup_restore();
    col = col.child(set_row(
        tr("settings.restore_title"),
        tr("settings.restore_hint"),
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

    // 加载时间窗口（7/14/30 天档位）：启动按最近活动清单加载窗口内会话
    let days = crate::services::workspace::load_window_days();
    col = col.child(set_row(
        tr("settings.load_time_window"),
        tr("settings.load_window_hint"),
        {
            let weak = weak.clone();
            window_row(days, &weak, t)
        },
        t,
    ));

    // 自动折叠时间（1/3/7 天档位）：启动时项目内会话全部早于该阈值 → 项目
    // 默认折叠（内部按小时算）。置于「会话显示数」上方（两者都是列表行为）
    let collapse_h = crate::services::workspace::auto_collapse_hours();
    col = col.child(set_row(
        tr("settings.auto_collapse_time"),
        tr("settings.auto_collapse_hint"),
        {
            let weak = weak.clone();
            collapse_row(collapse_h, &weak, t)
        },
        t,
    ));

    // 090 排队护栏：运行中发送方式（排队=本地队列等彻底跑完 / 插话=steer）
    let send_mode = crate::services::workspace::send_mode_pref();
    col = col.child(set_row(
        tr("settings.send_mode"),
        tr("settings.send_mode_hint"),
        {
            let weak = weak.clone();
            send_mode_row(send_mode, &weak, t)
        },
        t,
    ));

    // 会话显示数（3-10）：各项目默认显示的会话数量（psp 初始页大小）
    let sess_n = crate::services::workspace::session_display_count() as u64;
    col = col.child(set_row(
        tr("settings.session_display_count"),
        tr("settings.session_count_hint"),
        stepper(
            "misc-sess",
            sess_n,
            3,
            10,
            1,
            |v| v.to_string(),
            || crate::services::workspace::session_display_count() as u64,
            |v| {
                let mut s = crate::services::workspace::app_settings();
                s.session_display_count = Some(v);
                crate::services::workspace::save_app_settings(&s);
            },
            weak,
            t,
        ),
        t,
    ));

    // 自动清理会话（052）：−/值/+ 步进，范围 [0,100]，默认 0 = 关闭；
    // 最后活动超出天数的非置顶会话启动时移入回收站。置于「提示音」前（052 定）
    col = col.child(set_row(
        tr("settings.auto_clean_title"),
        tr("settings.auto_clean_hint"),
        clean_stepper(weak, t),
        t,
    ));

    // 提示音 switch（v60 从界面页挪入）
    let sound_on = crate::services::workspace::load_sound_pref();
    col = col.child(set_row(
        tr("settings.notification_sound"),
        tr("settings.sound_hint"),
        switch(
            "misc-sound",
            sound_on,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        crate::services::workspace::save_sound_pref(on);
                        c.broadcast_sound_on(on, cx);
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

    // 自动更新 switch（081：默认 on——启动后台检查 release，on=下载 staging
    // 待重启，off=只在新会话页提示新版本号 ↑ 可点击开 release 页）
    let auto_on = crate::services::workspace::auto_update();
    col = col.child(set_row(
        tr("about.auto_update"),
        tr("settings.auto_update_hint"),
        switch(
            "misc-auto-update",
            auto_on,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.auto_update = Some(on);
                        crate::services::workspace::save_app_settings(&s);
                        // 关→开：立即补一次检查（Idle 才发，避免与进行中的流程叠加）
                        if on && matches!(c.aux.update, crate::services::updater::UpdateState::Idle)
                        {
                            c.spawn_update_check(cx);
                        }
                        cx.notify();
                    });
                }
            },
            t,
        ),
        t,
    ));

    // 工作区数据目录：pf 自有目录（~/.pi-flash）在上、pi 目录（~/.pi/agent）
    // 在下，上下两行；每行右侧「打开」按钮（系统文件浏览器打开，右对齐）
    let pf_dir = pi_link::paths::dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    col = col.child(set_row(
        tr("settings.data_dir"),
        tr("settings.memory_dir"),
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(dir_row("pf", &pf_dir, t))
            .child(dir_row("pi", &format!("{home}\\.pi\\agent"), t))
            .into_any_element(),
        t,
    ));

    col.into_any_element()
}

/// 档位：7/14/30 天一排（仿语言按钮，active 描边）。
fn window_row(
    days: u64,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    const CHOICES: [(u64, &str); 3] = [
        (7, "misc.window_7d"),
        (14, "misc.window_14d"),
        (30, "misc.window_30d"),
    ];
    div()
        .flex()
        .gap(px(8.))
        .children(CHOICES.iter().map(|(v, key)| {
            let active = days == *v;
            let weak_item = weak.clone();
            let value = *v;
            let label = tr(*key);
            div()
                .id(SharedString::from(format!("misc-window-{v}")))
                .w(px(72.))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(5.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if active {
                    rgb(t.bg_selected)
                } else {
                    rgb(t.bg_panel)
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                // §5.4：选项文字统一 text_muted，选中只加粗不变色
                // （选中感由 accent 描边 + bg_selected 承担）
                .text_color(rgb(t.text_muted))
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

/// 090 排队护栏：运行中发送方式两档（排队=本地队列等彻底跑完（默认）/
/// 插话=steer 立即插进当前轮）。仿 window_row 档位按钮，active 描边。
fn send_mode_row(
    mode: &str,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    const CHOICES: [(&str, &str); 2] = [
        ("queue", "settings.send_queue"),
        ("steer", "settings.send_steer"),
    ];
    div()
        .flex()
        .gap(px(8.))
        .children(CHOICES.iter().map(|(v, key)| {
            let active = mode == *v;
            let label = tr(*key);
            let weak_item = weak.clone();
            let value = *v;
            div()
                .id(SharedString::from(format!("misc-send-{v}")))
                .w(px(72.))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(5.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if active {
                    rgb(t.bg_selected)
                } else {
                    rgb(t.bg_panel)
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                // §5.4：选项文字统一 text_muted，选中只加粗不变色
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |_c, cx| {
                        crate::services::workspace::save_send_mode(value);
                        cx.notify();
                    });
                })
                .child(SharedString::from(label.to_string()))
                .into_any_element()
        }))
        .into_any_element()
}

/// 档位：1/3/7 天一排（仿语言按钮，active 描边）——内部按小时（24/72/168）
/// 存取与比较，档位值即三档小时数。
fn collapse_row(
    hours: u64,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    const CHOICES: [(u64, &str); 3] = [
        (24, "misc.collapse_1d"),
        (72, "misc.collapse_3d"),
        (168, "misc.collapse_7d"),
    ];
    div()
        .flex()
        .gap(px(8.))
        .children(CHOICES.iter().map(|(v, key)| {
            let active = hours == *v;
            let label = tr(*key);
            let weak_item = weak.clone();
            let value = *v;
            div()
                .id(SharedString::from(format!("misc-collapse-{v}")))
                .w(px(72.))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(5.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if active {
                    rgb(t.bg_selected)
                } else {
                    rgb(t.bg_panel)
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                // §5.4：选项文字统一 text_muted，选中只加粗不变色
                // （选中感由 accent 描边 + bg_selected 承担）
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |_c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.auto_collapse_hours = Some(value);
                        crate::services::workspace::save_app_settings(&s);
                        cx.notify();
                    });
                })
                .child(SharedString::from(label.to_string()))
                .into_any_element()
        }))
        .into_any_element()
}
