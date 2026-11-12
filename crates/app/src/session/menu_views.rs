//! 统一输入菜单弹层的两条横杠视图：搜索栏 + tab 条。
//!
//! 从 slash_menu_view 拆出（2026-11-12 架构守卫：fn 400 行 / 文件 1000 行
//! 双红线）；`menu.rs` 留弹层壳与条目渲染，条样式只有这一份。
//! tab 条交互链几何一律取 ui::tokens 令牌（ui-raw 守卫）。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use super::menu::{BAND_H, MenuTrigger, STRIP_H};
use crate::Chat;
use crate::MenuKind;
use crate::i18n::tr;
use crate::ui::WithTip;
use crate::ui::tokens::{btn, radius, space};

/// 搜索栏（菜单底色同体；万能搜索入口）。纯展示链无交互，几何字面量
/// 不受 ui-raw 约束。
pub(crate) fn menu_search_bar(
    chat: &Chat,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    div()
        .flex_shrink_0()
        .h(px(36.))
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .bg(rgb(t.bg))
        .child(crate::ui::icon("search", 14., t.text_faint))
        .child(
            div().flex_1().min_w_0().children(
                chat.composer
                    .menu_search
                    .as_ref()
                    .map(|i| i.clone().into_any_element()),
            ),
        )
        .into_any_element()
}

/// tab 条 + 「打开文件」钮 + Tab插入 提示。
pub(crate) fn menu_tab_strip(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    trigger: Option<&MenuTrigger>,
    searching: bool,
    active_kind: Option<MenuKind>,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    // ---- tab 项（topbar tab_body 复用：连体态，条上不画横线）----
    let tab = |kind: MenuKind| -> AnyElement {
        let active = !searching && active_kind == Some(kind);
        let enabled = trigger.is_some_and(|tr| chat.menu_tab_available(kind, tr));
        let badge = div()
            .font_family(crate::editor::markdown::MONO_FAMILY)
            // 前缀随 tab 文字：面板字体 ui(11)（031-2 四轮：搜索框/tab 栏统一
            // 面板字体，字号 −1；mono 仅条目名保留）
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(if active { t.accent } else { t.text_faint }))
            .child(SharedString::from(kind.prefix().to_string()))
            .into_any_element();
        let tab = crate::titlebar::tab_body(
            None,
            tr(kind.label_key()).into(),
            active,
            0.,
            t,
            None,
            Some(badge),
            false,
        )
        // 菜单变体：无圆角 + 填满条高（与搜索栏接洽）；抹掉 tab_body 连体
        // 态的左右 1px 边线与 mb(-1)（条下已是 5px 同色带无线可盖，-1 只会让
        // 文字偏下 + 满高边线成「插入痕」，2026-11-11 用户定稿）
        .h(px(STRIP_H - BAND_H))
        .rounded(px(0.))
        .mb(px(0.))
        // tab 文字 = 面板字体−1（2026-11-11 用户定稿；覆盖 tab_body 的 ui(12)）
        .text_size(crate::appearance::ui_size(11.))
        .border_l_0()
        .border_r_0()
        .when(!enabled, |d| {
            d.opacity(0.45).cursor(gpui::CursorStyle::Arrow)
        });
        let tab = if enabled {
            let weak = weak.clone();
            tab.id(SharedString::from(format!("menu-tab-{}", kind.prefix())))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| c.menu_switch_tab(kind, cx));
                })
                .into_any_element()
        } else {
            tab.into_any_element()
        };
        tab
    };
    div()
        .flex_shrink_0()
        .h(px(STRIP_H))
        .flex()
        .items_end()
        .px(px(space::SP4))
        // tab 栏钉面板字体槽（与搜索框同源）：挂载点是会话区，ambient
        // 字体 = 会话字体槽（用户可设霞鹜文楷等），不钉会与搜索框分叉
        .font_family(SharedString::from(
            crate::appearance::panel_font().family.clone(),
        ))
        .bg(rgb(t.chrome))
        // 条下 5px 菜单底色带（2026-11-11 用户定稿）：激活 tab 的
        // mb(-1) 连体缘落在这条带上，接缝不可见
        .border_b_5()
        .border_color(rgb(t.bg))
        .children(MenuKind::ALL.iter().map(|k| tab(*k)))
        .child(
            // 整高居中壳：条 items_end 对 24px 钮不友好（mb 手调对
            // 不齐 tab 文字中线），壳内 items_center 兜底
            div().h_full().flex().items_center().child(
                // 浅图标钮（bar.rs eye_button 同款 24×24 r5）：系统文件
                // 选择器，选中文件以 `@path ` 插入
                div()
                    .id("menu-open-file")
                    .ml(px(space::SP2))
                    .size(px(btn::SM_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::INLINE))
                    .text_color(rgb(t.text_muted))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .with_tip("menu.open_file")
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| c.menu_open_file(cx));
                        }
                    })
                    .child(crate::ui::icon("plus", 16., t.text_muted)),
            ),
        )
        .child(
            div()
                .ml_auto()
                .h_full()
                .flex()
                .items_center()
                .px(px(space::SP2))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_soft))
                .child(tr("menu.tab_insert")),
        )
        .into_any_element()
}
