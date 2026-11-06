//! 「其他」页小构件：stepper / clean_stepper / step_btn / dir_row /
//! mono_value / switch。

use super::*;

/// 步进控件（通用版）：−/+ 两个 24px 档位按钮夹居中数值，到边界后对应按钮
/// 置灰禁点。`render` 生成中间显示（"10" / "3 天" / "关闭"），`cur` 在点击
/// 时重读当前值（不吃渲染快照陈值），夹回 [min, max] 后交 `apply` 落盘。
/// 现用于会话显示数（3-10）与自动清理天数（0-100，052）。
pub(super) fn stepper(
    id_prefix: &'static str,
    value: u64,
    min: u64,
    max: u64,
    step: i64,
    render: impl Fn(u64) -> String + 'static,
    cur: impl Fn() -> u64 + Clone + 'static,
    apply: impl Fn(u64) + Clone + 'static,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(step_btn(
            format!("{id_prefix}-dec"),
            "−",
            value > min,
            -step,
            min,
            max,
            step,
            cur.clone(),
            apply.clone(),
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
                .child(SharedString::from(render(value))),
        )
        .child(step_btn(
            format!("{id_prefix}-inc"),
            "+",
            value < max,
            step,
            min,
            max,
            step,
            cur,
            apply,
            weak,
            t,
        ))
        .into_any_element()
}

/// 052 自动清理天数步进：[0,100] 步长 5，默认 0 显示「关闭」（= 不自动清理）。
pub(super) fn clean_stepper(
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
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
        t,
    )
}

/// 步进按钮：enabled=false 时置灰且不挂点击；点击把 cur() 对齐步长网格后
/// ± step、夹回 [min, max] 交 apply 落盘（weak.update 只为拿 Chat 的 notify）。
fn step_btn(
    id: String,
    label: &str,
    enabled: bool,
    delta: i64,
    min: u64,
    max: u64,
    step: i64,
    cur: impl Fn() -> u64 + 'static,
    apply: impl Fn(u64) + 'static,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let base = div()
        .id(SharedString::from(id))
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
                    let c = cur() as i64;
                    // 对齐步长网格再步进：存量非格点值（如老默认 3）也能
                    // 干净落到 0/5/…/100；step=1 时恒等，不影响会话显示数
                    let base = c - c.rem_euclid(step);
                    apply((base + delta).clamp(min as i64, max as i64) as u64);
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
