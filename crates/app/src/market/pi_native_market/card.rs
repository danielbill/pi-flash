//! Pi 原生扩展市场 —— 「最近发布」条、index 卡分段外壳（040-2）。
//!
//! 全部尺寸/颜色来自 [`super::design`]；本文件只描述结构与设计稿的对应关系：
//!
//! ```text
//! recent_card  = .packages-recent-card + .packages-recent-list（4 列小卡）
//! index 卡      = .packages-index-card ÷ 三段：头+工具栏 / 卡行×N / 页码条
//! ```
//!
//! **分段**是性能决定的（040-2 §性能）：整页塞进一个滚动容器会逼 GPUI 每帧
//! 重排全部 50 张卡（含 50 段 3 行 clamp 的长描述），实测每次重渲染 ~100ms，
//! 鼠标一划就卡死。改法 = `gpui::list` 变高虚拟化，把同一条边框拆成 3 段各自
//! 画（左/右描边 + 面板底色连续，拼起来视觉上仍是设计稿那一整只盒子）。
//! 包卡本体（`package_card`）在 [`super::package`]。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};

use super::design::{self as d, Metrics, px18, sp};
use super::time::ago_label;
use super::{Chat, label, thousands};
use pi_link::market::pi_ext_market::MarketEntry;

/// 「最近发布」小卡（设计 `.packages-recent-item`）。
fn recent_item(e: &MarketEntry, ago: &str, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let url = format!("https://www.npmjs.com/package/{}", e.name);
    let w = weak.clone();
    gpui::div()
        .id(SharedString::from(format!("pi-native-recent-{}", e.name)))
        .min_w_0()
        .flex()
        .flex_col()
        .gap(gpui::px(sp::RECENT_ROW_GAP))
        .p(gpui::px(sp::RECENT_PAD))
        .border_1()
        .border_color(d::line())
        .cursor_pointer()
        .hover(|s| s.border_color(d::accent_alpha(0.42)).bg(d::tint()))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let url = url.clone();
            let _ = w.update(cx, |_, _| crate::services::shell_open::open_url(&url));
        })
        // 名称：mono 700 / 单行省略（设计 overflow+ellipsis+nowrap）
        .child(
            gpui::div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .font_family(d::MONO)
                .font_weight(gpui::FontWeight::BOLD)
                .text_size(gpui::px(px18::BTN_FS))
                .text_color(d::text())
                .child(SharedString::from(e.name.clone())),
        )
        // 摘要：衬线单行省略
        .child(
            gpui::div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .font_family(d::SERIF)
                .text_size(gpui::px(13.5))
                .text_color(d::muted())
                .child(SharedString::from(e.description.clone())),
        )
        // 时间：mono 小号大写 + accent
        .child(
            gpui::div()
                .font_family(d::MONO)
                .text_size(gpui::px(px18::T12SMALL))
                .text_color(gpui::rgb(d::ACCENT))
                .child(label(&d::spaced(&ago.to_uppercase(), false))),
        )
        .into_any_element()
}

/// 卡片头（标题 + 可选右端件）。设计 `.content-card-header`。
/// 容器骨架走 `market::shell` 公共件（色板注入 design 字面量），mono 大写
/// 标签是本页自己的东西，在这里构造。
pub(super) fn card_header(title: &str, right: Option<AnyElement>, m: &Metrics) -> AnyElement {
    crate::market::shell::card_header(
        gpui::div()
            .font_family(d::MONO)
            .text_size(gpui::px(px18::TITLE_FS))
            .text_color(d::text())
            .child(label(&d::spaced(&title.to_uppercase(), false)))
            .into_any_element(),
        right,
        m,
        d::line(),
    )
}

// ---------------------------------------------------------------------------
// 分段外壳（`market::shell` 公共件 + design 字面量色板注入；090-4：几何共享、
// 色板独立）
// ---------------------------------------------------------------------------

/// 居中外壳：定宽的段盒子在整行里居中（`mx_auto` 在列表项里不可靠——
/// 列表项根元素不参与 flex 对齐，曾表现为「页码段比卡行右移 36px」）。
pub(super) fn centered(inner: gpui::Div, m: &Metrics) -> gpui::Div {
    // 定宽段 + **显式左右留白**（不用 justify_center：列表项宽度不定时居中是
    // no-op，曾让「最近发布」段贴左、index 段居中）
    crate::market::shell::centered(inner, m)
}

/// 段外壳：两侧描边 + 面板底；`top/bottom` = 首/末段补边。
pub(super) fn slice_shell(m: &Metrics, top: bool, bottom: bool) -> gpui::Div {
    crate::market::shell::slice_shell(m, top, bottom, d::panel(), d::line())
}

/// ① 「最近发布」整块（含外壳内距与段间 1rem 间隙）。
pub(super) fn recent_block(
    recent: &[MarketEntry],
    weak: &gpui::WeakEntity<Chat>,
    loading: bool,
    m: &Metrics,
) -> AnyElement {
    let mut items: Vec<AnyElement> = recent
        .iter()
        .map(|e| recent_item(e, &ago_label(&e.updated), weak))
        .collect();
    // 首屏未回包：四条骨架（与真卡同高，避免跳版）
    if items.is_empty() && loading {
        items = (0..4)
            .map(|i| {
                gpui::div()
                    .id(SharedString::from(format!("pi-native-recent-skel-{i}")))
                    .h(gpui::px(d::recent_item_h()))
                    .w_full()
                    .border_1()
                    .border_color(d::line())
                    .into_any_element()
            })
            .collect();
    }

    let cols = if m.narrow { 1 } else { 4 };
    let mut list = gpui::div().flex().flex_col().gap(gpui::px(sp::RECENT_GAP));
    let mut it = items.into_iter();
    loop {
        let mut row = gpui::div().flex().gap(gpui::px(sp::RECENT_GAP)).w_full();
        let mut filled = 0;
        for _ in 0..cols {
            match it.next() {
                Some(el) => {
                    filled += 1;
                    row = row.child(gpui::div().flex_1().min_w_0().child(el));
                }
                // 末行不足四列：补空位，保持网格列宽不被拉宽（设计 grid 同款）
                None => row = row.child(gpui::div().flex_1().min_w_0()),
            }
        }
        if filled == 0 {
            break;
        }
        list = list.child(row);
    }

    let card = gpui::div()
        .w(gpui::px(m.shell_inner_w()))
        .flex()
        .flex_col()
        .border_1()
        .border_color(d::line())
        .bg(d::panel())
        .child(card_header("Recently published", None, m))
        .child(gpui::div().p(gpui::px(m.card_pad_head)).child(list));

    centered(card, m)
        .pt(gpui::px(m.shell_pad))
        .pb(gpui::px(sp::BLOCK_GAP))
        .into_any_element()
}

/// ② index 卡上段：标题 + 计数 + 工具栏。
pub(super) fn index_head(
    bar: AnyElement,
    shown: usize,
    total: usize,
    page: usize,
    filtered: bool,
    m: &Metrics,
) -> AnyElement {
    let total = if total > 0 { total } else { shown };
    // 设计稿口径：`1-50 / 5,573`（千分位 + 斜杠空格）
    let range = if shown == 0 {
        format!("0 / {}", thousands(total))
    } else if !filtered && page > 1 {
        let start = (page - 1) * pi_link::market::pi_ext_market::PAGE_SIZE + 1;
        format!(
            "{}-{} / {}",
            thousands(start),
            thousands(start + shown - 1),
            thousands(total)
        )
    } else {
        format!("1-{} / {}", shown, thousands(total))
    };
    let count = gpui::div()
        .font_family(d::MONO)
        .text_size(gpui::px(px18::COUNT_FS))
        .text_color(d::muted())
        .child(SharedString::from(range));

    centered(
        slice_shell(m, true, false)
            .child(card_header(
                "All packages",
                Some(count.into_any_element()),
                m,
            ))
            .child(
                gpui::div()
                    .px(gpui::px(m.card_pad_body))
                    .pt(gpui::px(m.card_pad_body))
                    .pb(gpui::px(sp::BODY_GAP))
                    .child(bar),
            ),
        m,
    )
    .into_any_element()
}

/// ③ 卡行：左右两道内距 + 行间距；卡本身等高（`Metrics::card_h`）。
pub(super) fn row_shell(m: &Metrics) -> gpui::Div {
    crate::market::shell::row_shell(m, false)
}

/// 行 → 外壳（两侧描边 + 居中）。
pub(super) fn row_slice(row: gpui::Div, m: &Metrics) -> AnyElement {
    centered(slice_shell(m, false, false).child(row), m).into_any_element()
}

/// ④ index 卡下段：页码条（末尾带上外壳 4rem 底留白）。
pub(super) fn pager_block(pager: AnyElement, m: &Metrics) -> AnyElement {
    crate::market::shell::pager_block(m, pager, false, sp::SHELL_PAD_BOTTOM, d::panel(), d::line())
}

/// 空态 / 加载态行（设计 `.packages-empty`：纯文本，无插图）；加载态在文字前挂转圈。
pub(super) fn empty_line(text: &'static str, loading: bool) -> AnyElement {
    let mut line = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(9.))
        .py(gpui::px(24.))
        .font_family(d::SERIF)
        .text_size(gpui::px(px18::BODY_FS))
        .text_color(d::muted());
    if loading {
        line = line.child(crate::ui::spinner(14., d::ACCENT));
    }
    line.child(label(text)).into_any_element()
}
