//! 设置面板公共件 —— pi-web `SettingsUi.tsx` 的 GPUI 复刻（Config* 系列）。
//! 尺寸走 `crate::ui::tokens`（UI 组件规范）；颜色走主题语义色
//! （danger 系见 theme.rs；状态色绿 0x4ade80 / 警 0xd97706 / 靛蓝徽标 hsla）。

use gpui::{AnyElement, SharedString, FontWeight, MouseButton, Window, div, px, rgb};

use super::*;

pub(crate) const GREEN: u32 = 0x4ade80;
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
/// 尺寸/配色委托 `ui::button`（§5.1 唯一按钮表）：small→MD(28)，default→LG(32)。
pub(crate) fn config_button(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    variant: Btn,
    small: bool,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let size = if small {
        crate::ui::BtnSize::Md
    } else {
        crate::ui::BtnSize::Lg
    };
    let variant = match variant {
        Btn::Primary => crate::ui::BtnVariant::Primary,
        Btn::Secondary => crate::ui::BtnVariant::Secondary,
        Btn::Danger => crate::ui::BtnVariant::Danger,
    };
    let weak = weak.clone();
    crate::ui::button(
        id,
        label.to_string(),
        size,
        variant,
        disabled,
        move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        },
    )
}

// ---------------------------------------------------------------------------
// switch / dot / tag
// ---------------------------------------------------------------------------

/// 开关视觉（SW_MD 28×16·knob10，`ui::tokens::switch`）：无 id/无事件，
/// [`switch_base`]（独立开关）与 [`group_switch`]（组头行内）共用。
fn switch_el(on: bool) -> gpui::Div {
    use crate::ui::tokens::switch as sw;
    let t = T();
    div()
        .w(px(sw::MD_W))
        .h(px(sw::MD_H))
        .flex_shrink_0()
        .rounded(px(sw::MD_H / 2.))
        .border_1()
        .border_color(if on { rgb(t.accent) } else { rgb(t.border) })
        .bg(if on { rgb(t.accent) } else { rgb(t.bg_selected) })
        .flex()
        .items_center()
        .child(
            div()
                .ml(px(if on {
                    sw::MD_ON_ML
                } else {
                    sw::MD_OFF_ML
                }))
                .size(px(sw::MD_KNOB))
                .rounded_full()
                .bg(if on { rgb(t.bg) } else { rgb(t.text_muted) }),
        )
}

/// 开关视觉唯一入口（独立开关）。`apply` 收到新状态 bool 由调用方自行
/// 解释；Chat 场景走 [`config_switch`]。
pub(crate) fn switch_base(
    id: impl Into<SharedString>,
    on: bool,
    disabled: bool,
    apply: impl Fn(bool, &mut gpui::App) + 'static,
) -> AnyElement {
    let el = switch_el(on).id(id.into());
    if disabled {
        return el.opacity(0.5).into_any_element();
    }
    el.cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            apply(!on, cx);
        })
        .into_any_element()
}

/// ConfigSwitch：设置页行内开关（Chat 回调形态）。
pub(crate) fn config_switch(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    on: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let weak = weak.clone();
    switch_base(id, on, disabled, move |_, cx| {
        let _ = weak.update(cx, |c, cx| on_toggle(c, cx));
    })
}

/// 组头小开关行（ConfigSidebarGroupSwitch）：{enabled}/{total} + 开关。
/// 开关尺寸随 2026-10-09 定稿与全局统一为 SW_MD 28×16（原 24×14 小档废除）。
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
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(count_text)),
        )
        .child({
            let mut sw = switch_el(checked);
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
        .text_color(rgb(T().danger))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// mono 值块（路径 / 命令等）。
pub(crate) fn mono_text(text: String, dim: bool) -> AnyElement {
    let t = T();
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(11.))
        .text_color(if dim { rgb(t.text_dim) } else { rgb(t.text) })
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(SharedString::from(text))
        .into_any_element()
}

/// 复选小块（工具/资源勾选）：14px 方框 + 标签，点击即切换。
// ---------------------------------------------------------------------------
// sidebar pieces
// ---------------------------------------------------------------------------

/// 设置页左列表列宽（Providers / 扩展 / MCP 清单 / 技能）：四页统一 350，
/// 基准 = 扩展页（2026-10-09 用户定稿「全部和扩展的列表宽度对齐」）。
pub(crate) const LIST_W: f32 = 350.;

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

// ---------------------------------------------------------------------------
// rows
// ---------------------------------------------------------------------------

/// set-row：标题+描述在左，控件在右，底分隔线（界面/其他两页单列 rows 版式）。
pub(crate) fn set_row(
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
                    // 标题与「界面」页 section_label 同款（13 semibold text_muted）
                    div()
                        .text_size(crate::appearance::ui_size(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text_muted))
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
