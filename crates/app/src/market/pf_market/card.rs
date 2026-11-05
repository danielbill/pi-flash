//! PF 插件市场——市场卡本体（090-4）：**PF v1 定稿内部设计，不得擅自发挥**
//! 72×72 预览槽 · 名称 16 semibold · 描述 · meta 行（发布人 · 发布时间 ····
//! [安装]）。安装/更新动作挂卡内按钮（pf_install / pf_update）。

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, IntoElement, MouseButton, SharedString, px, rgb};

use pi_link::pf_plugins::{IndexEntry, LedgerEntry};

use super::{Chat, MONO, accent_alpha, installed_in, ledger_version_of, pf_updatable};
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::market::pi_native_market::design::Metrics;

/// 卡片作者行：**域与包名**，从 npm 全名派生（不写死发布人）。
/// `@pi-flash-plugins/quick-panel` → `pi-flash-plugins · quick-panel`；
/// 无 scope 的包名原样返回。
fn owner_line(name: &str) -> String {
    match name.strip_prefix('@').and_then(|rest| rest.split_once('/')) {
        Some((scope, slug)) if !scope.is_empty() && !slug.is_empty() => {
            format!("{scope} · {slug}")
        }
        _ => name.to_string(),
    }
}

pub(super) fn card(
    weak: &gpui::WeakEntity<Chat>,
    e: &IndexEntry,
    m: &Metrics,
    t: &'static Theme,
    busy: bool,
    icons: &[(String, SharedString)],
    ledger: &[LedgerEntry],
) -> AnyElement {
    let installed = installed_in(ledger, &e.name);
    // 090-10：已装且索引版本更高 → 卡片出「更新」钮 + 版本号 ↑
    let updatable = installed
        && ledger_version_of(ledger, &e.name).is_some_and(|v| pf_updatable(v, &e.version));
    // 窄屏（<768）：与 pi 页同口径转单列，卡宽吃满行
    let width = if m.narrow {
        gpui::div().w_full()
    } else {
        gpui::div().flex_1().min_w_0()
    };

    // ── 预览槽：72×72 顶部对齐 · 底色跟卡片（t.bg，不抢眼）· 无边框 ──
    // icon 用 hint 档（text_faint）= 非行动信息，不抢标题/安装钮的注意力。
    // 装了就用**插件自己的图标**（与状态栏 tab 同一资产，`plugins/<id>/assets/…`），
    // 未装/无 status_bar 的包回通用 package。
    // 取图链：status_bar 图标 → 市场清单声明的包图标（pf-market/1 `icon`，
    // 无 status_bar 的 v3 插件从这里给卡片图；仅已安装时解析——
    // 资产随包落盘才存在）→ 通用 package。
    let plugin_icon = pi_link::pf_plugins::dir_aliases(&e.name)
        .into_iter()
        .find_map(|alias| {
            icons
                .iter()
                .find(|(pkg, _)| *pkg == alias)
                .map(|(_, icon)| icon.clone())
        })
        .or_else(|| {
            let icon = e.icon.as_deref()?;
            if !installed {
                return None;
            }
            let alias = pi_link::pf_plugins::dir_aliases(&e.name)
                .into_iter()
                .next()?;
            Some(crate::pf_plugin::contrib_icon_path(&alias, icon))
        });
    // 插件自己的图标用**正常图标色**（text_muted）；还没装的包没有图标资产，
    // 回通用 package 时才降成 hint 档（text_faint），不抢标题/安装钮
    let icon_el: AnyElement = match plugin_icon {
        Some(path) => gpui::svg()
            .path(path)
            .text_color(rgb(t.text_muted))
            .size(px(30.))
            .into_any_element(),
        None => crate::ui::icon("package", 28., t.text_faint),
    };
    let preview = gpui::div()
        .size(px(72.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(t.bg))
        .rounded(px(8.))
        .child(icon_el);

    // ── 名称（16 semibold）+ 版本号（mono，跟在标题后）──
    let name = gpui::div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            gpui::div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(16.))
                .child(e.display.zh.clone()),
        )
        .children((!e.version.is_empty()).then(|| {
            let mut v = gpui::div()
                .flex()
                .items_center()
                .gap(px(2.))
                .font_family(MONO)
                .text_size(px(11.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(format!("v{}", e.version)));
            if updatable {
                // ↑ 可更新标记（对齐 settings-扩展页范式），行内跟在版本号后
                v = v.child(crate::ui::icon("arrow-up", 11., t.accent));
            }
            v
        }));

    // ── 描述（无则不渲染；卡宽自然约束换行）──
    let desc = e
        .desc
        .as_ref()
        .map(|d| gpui::div().text_color(rgb(t.text_dim)).child(d.zh.clone()));

    // ── meta 行：发布人 · 发布时间 ···· [安装]（同行，贴底）──
    let meta = gpui::div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mt_auto()
        .child(
            gpui::div()
                .text_size(px(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(owner_line(&e.name))),
        )
        .child(
            gpui::div()
                .text_size(px(11.))
                .text_color(rgb(t.text_faint))
                .child(e.published_at.clone()),
        )
        .child(gpui::div().flex_1())
        .child(install_btn(weak, e, installed, updatable, busy, t));

    let body = gpui::div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(name)
        .children(desc)
        .child(meta);

    // ── 卡片：150px 高（定稿）· padding 10 · items_start ──
    // 背景 = 内容区底色 `t.bg`（与最外层同色，2026-10-23 用户定夺），
    // 靠 1px 描边成型；段盒底是 bg_panel，因此卡片会显得“浮”在上面。
    width
        .h(px(150.))
        .p(px(10.))
        .flex()
        .items_start()
        .gap(px(12.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        // hover：**不上底色**（bg_hover 会上出一块脏色，用户 2026-10-23 否决），
        // 只给描边加 accent，且加粗到 2px（1px 在浅色主题下几乎看不出来）
        .hover(|s| s.border_2().border_color(accent_alpha(t, 0.42)))
        .child(preview)
        .child(body)
        .into_any_element()
}

/// 安装钮（v1 定稿）：边框盒内 `[`+安装+`]`；busy = spinner；
/// 已装 = ✓ 绿（accent，不可点）；已装且索引有更高版 = `[`+更新+`]`
/// （090-10，点击走 `pf_update` 覆盖安装链路）。
fn install_btn(
    weak: &gpui::WeakEntity<Chat>,
    e: &IndexEntry,
    installed: bool,
    updatable: bool,
    busy: bool,
    t: &'static Theme,
) -> AnyElement {
    // 已装且无更新：✓ 绿（accent，不可点）
    if installed && !updatable {
        return gpui::div()
            .h(px(33.3))
            .px(px(9.36))
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .text_color(rgb(t.accent))
            .child(crate::ui::icon(
                "check",
                crate::ui::tokens::icon::CHECK_CARD,
                t.accent,
            ))
            .into_any_element();
    }

    let label: AnyElement = if busy {
        crate::ui::spinner(11., t.accent)
    } else {
        let name = e.name.clone();
        let weak_c = weak.clone();
        // 更新链路：同款按钮壳，点击走 pf_update（停→覆盖→重载）
        let text = if updatable {
            tr("market.btn.update")
        } else {
            tr("market.btn.install")
        };
        gpui::div()
            .id(SharedString::from(format!("pf-market-install-{}", e.name)))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let name = name.clone();
                let _ = weak_c.update(cx, |c, cx| {
                    if updatable {
                        c.pf_update(name, cx);
                    } else {
                        c.pf_install(name, false, cx);
                    }
                });
            })
            .child(bracket("[", t))
            .child(text)
            .child(bracket("]", t))
            .into_any_element()
    };

    let mut box_el = gpui::div()
        .id(SharedString::from(format!(
            "pf-market-install-box-{}",
            e.name
        )))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .h(px(33.3))
        .min_w(px(72.))
        .px(px(9.36))
        .border_1()
        .border_color(rgb(t.border))
        .text_size(px(11.))
        .font_family(MONO)
        .text_color(rgb(t.text_dim));
    // v1 定稿：busy = 描边 accent 52%；否则 hover 描边 accent 52% + 文字回全亮
    if busy {
        box_el = box_el.border_color(accent_alpha(t, 0.52));
    } else {
        box_el = box_el.cursor_pointer().hover(|s| {
            s.border_color(accent_alpha(t, 0.52))
                .text_color(rgb(t.text))
        });
    }
    box_el.child(label).into_any_element()
}

/// 括号子元素（px4.6 + accent 色）。
fn bracket(ch: &'static str, t: &'static Theme) -> AnyElement {
    gpui::div()
        .px(px(4.6))
        .text_color(rgb(t.accent))
        .child(SharedString::from(ch))
        .into_any_element()
}
