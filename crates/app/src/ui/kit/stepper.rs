//! 步进器（stepper）：−/+ 两个 24px 档位按钮夹居中数值（min_w 28，12.5
//! semibold），到边界后对应按钮置灰禁点。首用 = 其他页会话显示数 / 自动
//! 清理天数（052），2026-11-07 自 settings/misc/controls.rs 上移为全局件
//! （子代理页「全局设置」行最大并发随之换用标准件）。

use gpui::prelude::*;
use gpui::{MouseButton, SharedString, div, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;

/// 步进控件（通用版）：−/+ 两个 24px 档位按钮夹居中数值，到边界后对应按钮
/// 置灰禁点。`render` 生成中间显示（"10" / "3 天" / "关闭"），`cur` 在点击
/// 时重读当前值（不吃渲染快照陈值），夹回 [min, max] 后交 `apply` 落盘。
pub(crate) fn stepper(
    id_prefix: &'static str,
    value: u64,
    min: u64,
    max: u64,
    step: i64,
    render: impl Fn(u64) -> String + 'static,
    cur: impl Fn() -> u64 + Clone + 'static,
    apply: impl Fn(u64) + Clone + 'static,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = T();
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
        ))
        .into_any_element()
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
) -> gpui::AnyElement {
    let t = T();
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
