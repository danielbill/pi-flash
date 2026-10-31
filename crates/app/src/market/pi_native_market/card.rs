//! Pi 原生扩展市场 —— 「最近发布」条、index 卡分段、包卡本体（040-2）。
//!
//! 全部尺寸/颜色来自 [`super::design`]；本文件只描述结构与设计稿的对应关系：
//!
//! ```text
//! recent_card  = .packages-recent-card + .packages-recent-list（4 列小卡）
//! index 卡      = .packages-index-card ÷ 三段：头+工具栏 / 卡行×N / 页码条
//! package_card = article[data-package-card]（左预览块 | 右信息列）
//! ```
//!
//! **分段**是性能决定的（040-2 §性能）：整页塞进一个滚动容器会逼 GPUI 每帧
//! 重排全部 50 张卡（含 50 段 3 行 clamp 的长描述），实测每次重渲染 ~100ms，
//! 鼠标一划就卡死。改法 = `gpui::list` 变高虚拟化，把同一条边框拆成 3 段各自
//! 画（左/右描边 + 面板底色连续，拼起来视觉上仍是设计稿那一整只盒子）。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};

use super::design::{self as d, Metrics, px18, sp};
use super::{install_label, label, primary_kind};
use crate::Chat;
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
        .map(|e| recent_item(e, &d::ago_label(&e.updated), weak))
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
        format!("0 / {}", super::thousands(total))
    } else if !filtered && page > 1 {
        let start = (page - 1) * pi_link::market::pi_ext_market::PAGE_SIZE + 1;
        format!(
            "{}-{} / {}",
            super::thousands(start),
            super::thousands(start + shown - 1),
            super::thousands(total)
        )
    } else {
        format!("1-{} / {}", shown, super::thousands(total))
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

/// 单张包卡：左预览块（16:10 占位图）| 右信息列（名/描述/元信息/徽标/链接/命令条）。
pub(super) fn package_card(
    e: &MarketEntry,
    weak: &gpui::WeakEntity<Chat>,
    m: &Metrics,
    busy: bool,
    installed: bool,
) -> AnyElement {
    let kind = primary_kind(e);

    // 预览块：底色 + 网格纹理 + 三条底纹（设计 74%/46%/61% 宽的三根 1px 线）
    let frame = {
        let mut grid_bg = gpui::div().absolute().inset_0().overflow_hidden();
        // 0.8rem(14.4px) 网格：竖线 + 横线
        let cols = (m.frame_w / 14.4).ceil() as usize;
        let rows = (m.frame_h / 14.4).ceil() as usize;
        for i in 0..cols {
            grid_bg = grid_bg.child(
                gpui::div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(gpui::px(i as f32 * 14.4))
                    .w(gpui::px(1.))
                    .bg(d::line()),
            );
        }
        for i in 0..rows {
            grid_bg = grid_bg.child(
                gpui::div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(gpui::px(i as f32 * 14.4))
                    .h(gpui::px(1.))
                    .bg(d::line()),
            );
        }
        gpui::div()
            .relative()
            .flex_shrink_0()
            .w(gpui::px(m.frame_w))
            .h(gpui::px(m.frame_h))
            .border_1()
            .border_color(d::line())
            .bg(d::panel_soft())
            .child(grid_bg)
            .child(
                gpui::div()
                    .absolute()
                    .inset_0()
                    .p(gpui::px(sp::FRAME_PAD))
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap(gpui::px(sp::FRAME_GAP))
                    .child(
                        gpui::div()
                            .h(gpui::px(1.))
                            .w(gpui::relative(0.74))
                            .bg(gpui::rgb(d::ACCENT)),
                    )
                    .child(
                        gpui::div()
                            .h(gpui::px(1.))
                            .w(gpui::relative(0.46))
                            .bg(gpui::rgba(crate::appearance::theme::rgba_a(0x9fa4ab, 0.7))),
                    )
                    .child(
                        gpui::div()
                            .h(gpui::px(1.))
                            .w(gpui::relative(0.61))
                            .bg(gpui::rgba(crate::appearance::theme::rgba_a(0x9fa4ab, 0.7))),
                    ),
            )
    };

    let (badge_border, badge_bg, badge_fg) = d::badge_colors((kind != "package").then_some(kind));
    let badge = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(4.5))
        .px(gpui::px(sp::BADGE_PAD_X))
        .py(gpui::px(sp::BADGE_PAD_Y))
        .border_1()
        .border_color(badge_border)
        .when(kind != "package", |s| s.bg(badge_bg))
        .text_size(gpui::px(px18::BADGE_FS))
        .line_height(gpui::px(sp::BADGE_LINE_H))
        .font_family(d::MONO)
        .text_color(badge_fg)
        .child(label(&d::spaced(&kind.to_uppercase(), false)));

    // 元信息行：作者 · 下载/月 · 时间
    let mut meta = gpui::div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(gpui::px(13.5))
        .font_family(d::MONO)
        .text_size(gpui::px(px18::META_FS))
        .line_height(gpui::px(px18::META_FS * 1.6))
        .text_color(d::muted());
    if let Some(a) = e.author.clone().filter(|a| !a.is_empty()) {
        meta = meta.child(SharedString::from(a));
    }
    if e.monthly_downloads > 0 {
        meta = meta.child(SharedString::from(format!(
            "{}/mo",
            pi_link::market::pi_ext_market::format_downloads(e.monthly_downloads)
        )));
    }
    meta = meta.child(SharedString::from(d::ago_label(&e.updated)));

    // 链接行：npm / repo / report（设计稿三枚图标 + 文案）
    let mut links = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(sp::LINK_ITEM_GAP));
    links = links.child(link_item(
        "npm",
        format!("https://www.npmjs.com/package/{}", e.name),
        weak,
    ));
    if let Some(repo) = e.repo.clone().filter(|r| !r.is_empty()) {
        links = links.child(link_item("repo", web_url(&repo), weak));
    }
    links = links.child(link_item(
        "report",
        format!(
            "https://github.com/earendil-works/pi/issues/new?template=package-report.yml&labels=package-report&title=Package+Report%3A%3A{}",
            e.name
        ),
        weak,
    ));

    // 命令条右端三态：已装 = ✓（success 绿，不可点）；busy = 转圈；其余 = install。
    let w = weak.clone();
    let name = e.name.clone();
    let install_btn: AnyElement = if installed {
        gpui::div()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .h(gpui::px(sp::COPY_H))
            .px(gpui::px(sp::COPY_PAD_X))
            .font_family(d::MONO)
            // ✓ 已装态：字号 24（12.96/16 两档都被用户否过——看不清「已装」）。
            .text_size(gpui::px(24.))
            .line_height(gpui::px(24.))
            .text_color(gpui::rgb(0x5db87a))
            .child(SharedString::from("\u{2713}"))
            .into_any_element()
    } else {
        let mut btn = gpui::div()
            .id(SharedString::from(format!("pi-native-install-{}", e.name)))
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .h(gpui::px(sp::COPY_H))
            .min_w(gpui::px(72.))
            .px(gpui::px(sp::COPY_PAD_X))
            .border_1()
            .border_color(d::line())
            .text_size(gpui::px(sp::COPY_FS))
            .font_family(d::MONO)
            .text_color(gpui::rgba(crate::appearance::theme::rgba_a(0xd5d8db, 0.8)))
            .child(if busy {
                crate::ui::spinner(11., d::ACCENT).into_any_element()
            } else {
                gpui::div()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, {
                        let w = w.clone();
                        let name = name.clone();
                        move |_, _, cx| {
                            let name = name.clone();
                            let _ = w.update(cx, |c, cx| c.native_install(name, cx));
                        }
                    })
                    .child(bracket("["))
                    .child(install_label())
                    .child(bracket("]"))
                    .into_any_element()
            });
        if busy {
            btn = btn.border_color(d::accent_alpha(0.52));
        } else {
            btn = btn
                .cursor_pointer()
                .hover(|s| s.border_color(d::accent_alpha(0.52)).text_color(d::text()));
        }
        btn.into_any_element()
    };

    let info = gpui::div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(gpui::px(sp::PKG_GAP))
        .p(gpui::px(sp::PKG_PAD))
        .child(name_link(e, weak))
        .child(
            gpui::div()
                .h(gpui::px(sp::DESC_LINE_H * 3.))
                .flex_shrink_0()
                .line_clamp(3)
                .font_family(d::SERIF)
                .text_size(gpui::px(px18::DESC_FS))
                .line_height(gpui::px(sp::DESC_LINE_H))
                .text_color(d::copy())
                .child(SharedString::from(e.description.clone())),
        )
        .child(meta)
        .child(gpui::div().flex().gap(gpui::px(6.3)).child(badge))
        .child(links)
        // 弹性占位：命令条贴卡片底（设计 `margin-top:auto`）
        .child(gpui::div().flex_1())
        .child(
            gpui::div()
                .flex()
                .items_center()
                .justify_between()
                .gap(gpui::px(sp::INSTALL_GAP))
                .pt(gpui::px(sp::INSTALL_PAD_Y))
                .pr(gpui::px(sp::INSTALL_PAD_R))
                .pb(gpui::px(sp::INSTALL_PAD_Y))
                .pl(gpui::px(sp::INSTALL_PAD_L))
                .border_1()
                .border_color(d::line_strong())
                .bg(d::panel())
                .child(
                    gpui::div()
                        .min_w_0()
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .font_family(d::MONO)
                        .text_size(gpui::px(sp::CODE_FS))
                        .line_height(gpui::px(sp::CODE_LINE_H))
                        .child(
                            gpui::div()
                                .text_color(d::muted())
                                .child(SharedString::from("$")),
                        )
                        .child(
                            gpui::div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(d::text())
                                .child(SharedString::from(format!(" pi install npm:{}", e.name))),
                        ),
                )
                .child(install_btn),
        );

    let card = if m.narrow {
        // <768：预览转顶部（设计 `grid-template-columns:1fr` + 底部描边）
        gpui::div()
            .w_full()
            .flex()
            .flex_col()
            .h(gpui::px(m.card_h()))
            .border_1()
            .border_color(d::line_strong())
            .bg(d::panel())
            .child(
                gpui::div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(gpui::px(sp::NARROW_PREVIEW_H))
                    .p(gpui::px(sp::PREVIEW_PAD))
                    .border_b_1()
                    .border_color(d::line())
                    .bg(d::control_bg())
                    .child(frame),
            )
            .child(info)
    } else {
        gpui::div()
            .flex_1()
            .min_w_0()
            .h(gpui::px(m.card_h()))
            .flex()
            .border_1()
            .border_color(d::line_strong())
            .bg(d::panel())
            .child(
                gpui::div()
                    .flex_shrink_0()
                    .w(gpui::px(m.preview_col_w))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(gpui::px(sp::PREVIEW_PAD))
                    .border_r_1()
                    .border_color(d::line())
                    .bg(d::control_bg())
                    .child(frame),
            )
            .child(info)
    };
    card.into_any_element()
}

/// 包名 = 指向 pi.dev 详情页的链接（设计稿 `.packages-name a`；hover 变 accent
/// ——gpui 没有 text-decoration，用颜色表达）。
fn name_link(e: &MarketEntry, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let url = format!("https://pi.dev/packages/{}", e.name);
    let w = weak.clone();
    gpui::div()
        .id(SharedString::from(format!("pi-native-name-{}", e.name)))
        .cursor_pointer()
        .font_family(d::MONO)
        .font_weight(gpui::FontWeight::BOLD)
        .text_size(gpui::px(px18::NAME_FS))
        .line_height(gpui::px(px18::NAME_FS * 1.35))
        .text_color(d::text())
        // 常显 1px 下划线，色 = --accent-underline-muted 的 accent 暗蓝（0.55）
        .text_decoration_1()
        .text_decoration_color(d::accent_alpha(0.55))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let url = url.clone();
            let _ = w.update(cx, |_, _| crate::services::shell_open::open_url(&url));
        })
        .child(SharedString::from(e.name.clone()))
        .into_any_element()
}

/// 按钮括号（设计 `::before/::after` = `[` `]`，accent 色，0.7ch 内距）。
fn bracket(ch: &'static str) -> AnyElement {
    gpui::div()
        .px(gpui::px(4.6))
        .text_color(gpui::rgb(d::ACCENT))
        .child(SharedString::from(ch))
        .into_any_element()
}

/// 行内链接（图标 + 文案；hover 变 accent。下划线 gpui 不支持，故只做色变）。
fn link_item(kind: &'static str, url: String, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let icon = match kind {
        "npm" => "npm",
        "repo" => "github",
        _ => "flag",
    };
    let w = weak.clone();
    gpui::div()
        .id(SharedString::from(format!("pi-native-link-{kind}-{url}")))
        .flex()
        .items_center()
        .gap(gpui::px(sp::LINK_GAP))
        .cursor_pointer()
        .text_size(gpui::px(px18::META_FS))
        .line_height(gpui::px(px18::META_FS))
        .font_family(d::MONO)
        .text_color(d::muted())
        // hover：文字与下划线一齐提到 accent 实色（用户定夺 2026-10-22）
        .hover(|mut s| {
            let st = s.text.get_or_insert_with(Default::default);
            st.color = Some(gpui::rgb(d::ACCENT).into());
            st.underline = Some(gpui::UnderlineStyle {
                thickness: gpui::px(1.),
                color: Some(gpui::rgb(d::ACCENT).into()),
                ..Default::default()
            });
            s
        })
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let url = url.clone();
            let _ = w.update(cx, |_, _| crate::services::shell_open::open_url(&url));
        })
        .child(crate::ui::icon(icon, sp::LINK_ICON, 0x9fa4ab))
        .child(label(kind))
        .into_any_element()
}

/// npm `repository` 字段可能是 `github.com/a/b` 这类裸写法 → 补协议。
fn web_url(repo: &str) -> String {
    if repo.starts_with("http://") || repo.starts_with("https://") {
        repo.to_string()
    } else {
        format!("https://{}", repo.trim_start_matches("git://"))
    }
}
