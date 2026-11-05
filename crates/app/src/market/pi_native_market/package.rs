//! Pi 原生扩展市场 —— 单张包卡（040-2，设计 `article[data-package-card]`）：
//! 左预览块（16:10 占位图）| 右信息列（名/描述/元信息/徽标/链接/命令条）。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};

use super::design::{self as d, Metrics, px18, sp};
use super::time::ago_label;
use super::{Chat, install_label, label, primary_kind};
use pi_link::market::pi_ext_market::MarketEntry;

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
    meta = meta.child(SharedString::from(ago_label(&e.updated)));

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
