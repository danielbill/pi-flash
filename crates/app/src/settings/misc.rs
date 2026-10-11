//! 设置「其他」页 (v54): pi 版本 / 启动恢复 / 加载时间窗口 / 会话显示数 /
//! 提示音 / 工作区数据目录。set-row 版式（标题+描述左，控件右）。
//! 展示思考 / 文件树 Git 标识已挪至「界面」页。

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

    // 会话显示数（3-10）：各项目默认显示的会话数量（psp 初始页大小）
    let sess_n = crate::services::workspace::session_display_count() as u64;
    col = col.child(set_row(
        tr("会话显示数"),
        tr("各项目默认显示的会话数量"),
        stepper(sess_n, 3, 10, weak, t),
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
        tr("自动更新"),
        tr("允许Pi-Flash自动更新"),
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
                        if on && matches!(c.update, crate::services::updater::UpdateState::Idle) {
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
        tr("工作区数据目录"),
        tr("会话记忆 / 布局状态存放位置"),
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

/// 会话显示数 +/- 控件：两个 24px 档位按钮夹居中数值，到边界后对应按钮
/// 置灰禁点。
fn stepper(
    value: u64,
    min: u64,
    max: u64,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(step_btn(
            "misc-sess-dec",
            "−",
            value > min,
            -1,
            min,
            max,
            weak,
            t,
        ))
        .child(
            div()
                .min_w(px(28.))
                .flex()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                // §5.4：与页内其他文字同色，层级只由加粗表达
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(value.to_string())),
        )
        .child(step_btn(
            "misc-sess-inc",
            "+",
            value < max,
            1,
            min,
            max,
            weak,
            t,
        ))
        .into_any_element()
}

/// 步进按钮：enabled=false 时置灰且不挂点击；点击把当前值 + delta 后
/// 夹回 [min, max] 存盘。
fn step_btn(
    id: &'static str,
    label: &str,
    enabled: bool,
    delta: i64,
    min: u64,
    max: u64,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    let base = div()
        .id(id)
        .size(px(24.))
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .flex()
        .items_center()
        .justify_center()
        .text_size(crate::appearance::ui_size(13.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(if enabled { t.text_dim } else { t.text_muted }));
    let btn = if enabled {
        let weak_click = weak.clone();
        base.cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_click.update(cx, |_c, cx| {
                    let mut s = crate::services::workspace::app_settings();
                    let cur = s.session_display_count.unwrap_or(10) as i64;
                    s.session_display_count =
                        Some((cur + delta).clamp(min as i64, max as i64) as u64);
                    crate::services::workspace::save_app_settings(&s);
                    cx.notify();
                });
            })
    } else {
        base
    };
    btn.child(SharedString::from(label.to_string()))
        .into_any_element()
}

/// 目录行：mono 路径占满左侧，「打开」按钮右对齐（系统文件浏览器打开）。
fn dir_row(key: &str, path: &str, t: &'static crate::theme::Theme) -> gpui::AnyElement {
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
                // psp 菜单「在终端中打开」同款 icon
                .child(crate::ui::icon("icon-terminal-solid", 16., t.text_dim))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    crate::actions_panels::open_in_explorer(&open_path);
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
fn switch(
    id: &'static str,
    on: bool,
    apply: impl Fn(bool, &mut gpui::App) + 'static,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    let _ = t; // 配色统一走 theme 语义，入参保留兼容既有调用点
    switch_base(id, on, false, apply)
}

