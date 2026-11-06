//! 搜索结果空态——Zed `project_search.rs:1049-1092` landing page 的逐值复刻。
//!
//! 标题（`LabelSize::Large` = **16px**）+ 副标题（`TextSize::Default` = 14px，
//! muted，下留 8px）+ 一排「图标 + 文案」行（`ButtonStyle::Subtle`：
//! 高 **22px**、`px 4`、`gap 4`、`rounded 4`、悬停 `ghost_element_hover`，
//! 图标 `IconSize::Small` = 14px）。**不加任何快捷键徽标**（用户 2026-10-21
//! 定案：一律不加快捷键）——点行等于点对应的开关按钮。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, WeakEntity, div, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::search::{Phase, SearchToggle};

/// 空态 = Zed `project_search.rs:1049-1092` 的 landing page：
/// 标题（`LabelSize::Large` = **16px**）+ 副标题（`TextSize::Default` = 14px，
/// muted，下留 8px）+ 一排「图标 + 文案」行（`ButtonStyle::Subtle`：
/// 高 **22px**、`px 4`、`gap 4`、`rounded 4`、悬停 `ghost_element_hover`，
/// 图标 `IconSize::Small` = 14px）。**不加任何快捷键徽标**（用户 2026-10-21
/// 定案：一律不加快捷键）——点行等于点对应的开关按钮。
pub(super) fn landing(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let ui = &chat.ws.search;
    let (heading, body): (&'static str, Option<&'static str>) = match ui.phase {
        Phase::Idle => (tr("search.landing_title"), None),
        Phase::Running if ui.scanned == 0 => (tr("search.loading_project"), None),
        Phase::Running => (tr("search.searching"), None),
        Phase::Done => (tr("search.no_results"), Some(tr("search.no_results_body"))),
    };
    let mut col = div().flex().flex_col().gap(px(4.)).child(
        div()
            .text_size(px(16.))
            .text_color(rgb(t.text))
            .child(SharedString::from(heading)),
    );
    if ui.phase == Phase::Idle {
        col = col.child(landing_rows(weak, t));
    } else if let Some(body) = body {
        col = col.child(
            div()
                .text_size(px(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(body)),
        );
    }
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .bg(rgb(t.bg))
        .child(col)
        .into_any_element()
}

/// landing 的五条「图标 + 文案」行（Zed 顺序：过滤 / 替换 / 正则 / 大小写 / 整词）。
fn landing_rows(weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(14.))
                .text_color(rgb(t.text_muted))
                // Zed：副标题 `.mb_2()` = 8px
                .mb(px(8.))
                .child(SharedString::from(tr("search.landing_hint"))),
        )
        .child(landing_row(
            "psl-paths",
            landing_glyph(None, t),
            tr("search.landing_paths"),
            weak,
            SearchToggle::Filters,
            t,
        ))
        .child(landing_row(
            "psl-replace",
            landing_glyph(Some("replace"), t),
            tr("search.landing_replace"),
            weak,
            SearchToggle::Replace,
            t,
        ))
        .child(landing_row(
            "psl-regex",
            landing_glyph(Some(GLYPH_REGEX), t),
            tr("search.landing_regex"),
            weak,
            SearchToggle::Regex,
            t,
        ))
        .child(landing_row(
            "psl-case",
            landing_glyph(Some(GLYPH_CASE), t),
            tr("search.landing_case"),
            weak,
            SearchToggle::CaseSensitive,
            t,
        ))
        .child(landing_row(
            "psl-word",
            landing_glyph(Some(GLYPH_WORD), t),
            tr("search.landing_whole_word"),
            weak,
            SearchToggle::WholeWord,
            t,
        ))
        .into_any_element()
}

/// 无 SVG 的三枚（Zed 本身就是字形图标：`.*` / `Aa` / `wd`）
const GLYPH_REGEX: &str = ".*";
const GLYPH_CASE: &str = "Aa";
const GLYPH_WORD: &str = "wd";

/// 行首图标位（14px，muted）：`Some(svg 名)` 走 svg，`Some(字形)` 走文字，
/// `None` = Zed 的 `IconName::Filter`（PF 的 `sliders-horizontal`）。
fn landing_glyph(icon: Option<&'static str>, t: &'static Theme) -> AnyElement {
    let slot = div()
        .w(px(14.))
        .h(px(14.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(t.text_muted));
    match icon {
        Some("replace") => crate::ui::icon("replace", 14., t.text_muted),
        Some("sliders-horizontal") | None => {
            crate::ui::icon("sliders-horizontal", 14., t.text_muted)
        }
        Some(glyph) => slot
            .font_family(crate::appearance::panel_font().family.clone())
            .text_size(px(11.))
            .child(SharedString::from(glyph))
            .into_any_element(),
    }
}

/// 一行：高 22、`px 4`、`gap 4`、`rounded 4`，悬停 `bg_hover`，点击切开关。
fn landing_row(
    id: &'static str,
    glyph: AnyElement,
    label: &'static str,
    weak: &WeakEntity<Chat>,
    toggle: SearchToggle,
    t: &'static Theme,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .id(id)
        // ui-raw: ok Zed landing 行几何逐值复刻（高22/px4/gap4/rounded4），非 PF 档位
        .h(px(22.))
        .px(px(4.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded(px(4.))
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |chat, cx| chat.search_toggle(toggle, cx));
        })
        .child(glyph)
        .child(
            div()
                .font_family(crate::appearance::panel_font().family.clone())
                .text_size(px(14.))
                .text_color(rgb(t.text))
                .child(SharedString::from(label)),
        )
        .into_any_element()
}
