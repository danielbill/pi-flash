//! 设置面板公共件 —— pi-web `SettingsUi.tsx` 的 GPUI 复刻（Config* 系列）。
//! 尺寸节奏与 pi-web 对齐：侧栏行 30px、按钮 small 28 / default 32、开关
//! 32×18（small 24×14）、状态点 7px、scope 徽标 10px；颜色走主题 + 语义色
//! （绿 0x4ade80 / 红 0xef4444 / 警 0xd97706 / 靛蓝徽标 hsla）。

use gpui::prelude::FluentBuilder;
use gpui::{AnyElement, SharedString, FontWeight, MouseButton, Window, div, px, rgb};

use super::*;

pub(crate) const GREEN: u32 = 0x4ade80;
pub(crate) const RED: u32 = 0xef4444;
pub(crate) const WARN: u32 = 0xd97706;

/// 靛蓝小徽标（reasoning "T"、项目 scope）——pi-web rgba(99,102,241,…)。
pub(crate) fn indigo_bg() -> gpui::Hsla {
    gpui::hsla(0.63, 0.86, 0.62, 0.12)
}
pub(crate) fn indigo_fg() -> gpui::Hsla {
    gpui::hsla(0.63, 0.86, 0.62, 0.85)
}

// ---------------------------------------------------------------------------
// buttons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Btn {
    Primary,
    Secondary,
    Danger,
}

/// ConfigButton：variant × size，点击回调直接拿到 Chat（动作都在 Chat 上）。
pub(crate) fn config_button(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    variant: Btn,
    small: bool,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let t = T();
    let (h, hpad, ts) = if small {
        (px(28.), px(10.), 11.)
    } else {
        (px(32.), px(14.), 12.)
    };
    let mut el = div()
        .id(id.into())
        .h(h)
        .px(hpad)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_size(crate::appearance::ui_size(ts))
        .child(SharedString::from(label.to_string()));
    el = match variant {
        Btn::Primary => el
            .border_1()
            .border_color(rgb(t.accent))
            .bg(rgb(t.accent))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(t.accent_contrast)),
        Btn::Secondary => el
            .border_1()
            .border_color(rgb(t.border))
            .text_color(rgb(t.text_muted)),
        Btn::Danger => el
            .border_1()
            .border_color(gpui::hsla(0., 0.84, 0.6, 0.35))
            .bg(gpui::hsla(0., 0.84, 0.6, 0.06))
            .text_color(rgb(RED)),
    };
    if disabled {
        return el.opacity(0.5).into_any_element();
    }
    el = el.cursor_pointer().hover(move |s| {
        match variant {
            Btn::Primary => s.bg(rgb(t.accent_hover)),
            Btn::Danger => s.bg(gpui::hsla(0., 0.84, 0.6, 0.12)),
            _ => s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)),
        }
    });
    let weak = weak.clone();
    el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        let _ = weak.update(cx, |c, cx| on_click(c, cx));
    })
    .into_any_element()
}

// ---------------------------------------------------------------------------
// switch / dot / tag
// ---------------------------------------------------------------------------

/// ConfigSwitch 32×18，knob 12。
pub(crate) fn config_switch(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    on: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let t = T();
    let mut sw = div()
        .id(id.into())
        .w(px(32.))
        .h(px(18.))
        .flex_shrink_0()
        .rounded(px(9.))
        .border_1()
        .border_color(if on { rgb(t.accent) } else { rgb(t.border) })
        .bg(if on { rgb(t.accent) } else { rgb(t.bg_selected) })
        .flex()
        .items_center()
        .child(
            div()
                .ml(if on { px(14.) } else { px(2.) })
                .size(px(12.))
                .rounded_full()
                .bg(if on { rgb(t.bg) } else { rgb(t.text_muted) }),
        );
    if disabled {
        return sw.opacity(0.5).into_any_element();
    }
    sw = sw.cursor_pointer();
    let weak = weak.clone();
    sw.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| on_toggle(c, cx));
    })
    .into_any_element()
}

/// 组头小开关（ConfigSidebarGroupSwitch）：{enabled}/{total} + 24×14 开关。
pub(crate) fn group_switch(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    count_text: String,
    checked: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let t = T();
    let row = div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .font_family(crate::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(count_text)),
        )
        .child({
            let mut sw = div()
                .w(px(24.))
                .h(px(14.))
                .rounded(px(7.))
                .border_1()
                .border_color(if checked { rgb(t.accent) } else { rgb(t.border) })
                .bg(if checked { rgb(t.accent) } else { rgb(t.bg_selected) })
                .flex()
                .items_center()
                .child(
                    div()
                        .ml(if checked { px(10.) } else { px(2.) })
                        .size(px(8.))
                        .rounded_full()
                        .bg(if checked { rgb(t.bg) } else { rgb(t.text_muted) }),
                );
            if !disabled {
                sw = sw.cursor_pointer();
            } else {
                sw = sw.opacity(0.5);
            }
            sw
        });
    if disabled {
        return row.into_any_element();
    }
    let weak = weak.clone();
    row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| on_toggle(c, cx));
    })
    .into_any_element()
}

/// 7px 状态点。
pub(crate) fn status_dot(color: u32) -> AnyElement {
    div()
        .size(px(7.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(color))
        .into_any_element()
}

/// scope 徽标：项目 = 靛蓝，其余 = 灰。
pub(crate) fn scope_tag(label: &str, project: bool) -> AnyElement {
    let t = T();
    let dim: gpui::Hsla = rgb(t.text_dim).into();
    div()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(3.))
        .bg(if project { indigo_bg() } else { gpui::hsla(0., 0., 0.5, 0.12) })
        .text_size(crate::appearance::ui_size(10.))
        .text_color(if project { indigo_fg() } else { dim })
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// text blocks
// ---------------------------------------------------------------------------

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

pub(crate) fn error_note(text: &str) -> AnyElement {
    div()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(RED))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// mono 值块（路径 / 命令等）。
pub(crate) fn mono_text(text: String, dim: bool) -> AnyElement {
    let t = T();
    div()
        .font_family(crate::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(11.))
        .text_color(if dim { rgb(t.text_dim) } else { rgb(t.text) })
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(SharedString::from(text))
        .into_any_element()
}

/// ConfigDetailGrid 一行：左 label 固定宽，右 value。
pub(crate) fn grid_row(label: &str, value: impl gpui::IntoElement) -> AnyElement {
    let t = T();
    div()
        .flex()
        .gap(px(14.))
        .child(
            div()
                .w(px(120.))
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text))
                .child(value),
        )
        .into_any_element()
}

/// 复选小块（工具/资源勾选）：14px 方框 + 标签，点击即切换。
pub(crate) fn check_chip(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    checked: bool,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let t = T();
    let mut row = div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(14.))
                .rounded(px(3.))
                .border_1()
                .border_color(rgb(if checked { t.accent } else { t.border }))
                .bg(rgb(if checked { t.accent } else { t.bg_panel }))
                .flex()
                .items_center()
                .justify_center()
                .when(checked, |d| d.child(crate::ui::icon("check", 10., 0xffffff))),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(if disabled { t.text_dim } else { t.text_muted }))
                .child(SharedString::from(label.to_string())),
        );
    if disabled {
        return row.opacity(0.55).into_any_element();
    }
    row = row.cursor_pointer();
    let weak = weak.clone();
    row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| on_click(c, cx));
    })
    .into_any_element()
}

// ---------------------------------------------------------------------------
// sidebar pieces
// ---------------------------------------------------------------------------

/// ConfigSidebar 基座（240px 列，bg_panel + 右边框）。
pub(crate) fn sidebar_shell(id: &'static str) -> gpui::Stateful<gpui::Div> {
    let t = T();
    div()
        .id(id)
        .w(px(240.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(t.bg_panel))
        .border_r_1()
        .border_color(rgb(t.border))
}

/// ConfigSidebarList：滚动区 + 底部 list-action 槽位由调用方拼。
pub(crate) fn sidebar_list() -> gpui::Stateful<gpui::Div> {
    div()
        .id("cfg-sb-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(6.))
        .pt(px(8.))
}

/// ConfigSidebarItem：30px 行（激活 bg_selected + semibold）。返回基座，
/// 调用方 `.child(...)` 拼内容并用 [`sidebar_item_on`] 挂点击。
pub(crate) fn sidebar_item(
    id: impl Into<SharedString>,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    div()
        .id(id.into())
        .h(px(30.))
        .px(px(8.))
        .mb(px(1.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(crate::appearance::ui_size(12.))
        .cursor_pointer()
        .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
        .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
        .text_color(rgb(if active { t.text } else { t.text_muted }))
        .hover(|s| s.bg(rgb(t.bg_hover)))
}

/// 侧栏行点击（section 切换 + 清错误）。
pub(crate) fn select_section(
    weak: &gpui::WeakEntity<Chat>,
    section: String,
) -> impl Fn(&mut Window, &mut gpui::App) + 'static {
    let weak = weak.clone();
    move |_, cx| {
        let _ = weak.update(cx, |c, cx| {
            if let Some(st) = c.settings.clone() {
                st.update(cx, |s, cx| {
                    s.section = section.clone();
                    s.error = None;
                    cx.notify();
                });
            }
        });
    }
}

/// ConfigSidebarGroupLabel：10px 大写组标题，右侧 aside（计数+开关）。
pub(crate) fn group_header(label: &str, aside: Option<AnyElement>) -> AnyElement {
    let t = T();
    div()
        .flex()
        .items_center()
        .px(px(8.))
        .pt(px(8.))
        .pb(px(3.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(crate::appearance::ui_size(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .children(aside)
        .into_any_element()
}

/// ConfigListAction：列表底部固定「+ 添加 …」行。
pub(crate) fn list_action(
    id: &'static str,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    active: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let t = T();
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(6.))
        .pt(px(8.))
        .pb(px(6.))
        .border_t_1()
        .border_color(rgb(t.border))
        .child(
            div()
                .id(SharedString::from(format!("{id}-go")))
                .h(px(30.))
                .px(px(8.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(crate::appearance::ui_size(12.))
                .cursor_pointer()
                .text_color(rgb(if active { t.accent } else { t.text_dim }))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .child(crate::ui::icon_hover("plus", 13., t.text_dim))
                .child(SharedString::from(label.to_string()))
                .on_mouse_down(MouseButton::Left, {
                    let weak = weak.clone();
                    move |_, _, cx| {
                        let _ = weak.update(cx, |c, cx| on_click(c, cx));
                    }
                }),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// detail + footer
// ---------------------------------------------------------------------------

/// ConfigDetail 基座：flex-1 滚动 + 20px 内边距。
pub(crate) fn detail_shell(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(20.))
        .text_size(crate::appearance::ui_size(12.))
        .flex()
        .flex_col()
        .gap_4()
}

/// ConfigFooter：min-height 52px，status 左 / actions 右。
pub(crate) fn footer(
    status: Option<AnyElement>,
    actions: Vec<AnyElement>,
) -> AnyElement {
    let t = T();
    div()
        .flex_shrink_0()
        .min_h(px(52.))
        .px(px(14.))
        .py(px(9.))
        .border_t_1()
        .border_color(rgb(t.border))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .children(status),
        )
        .children(actions)
        .into_any_element()
}
