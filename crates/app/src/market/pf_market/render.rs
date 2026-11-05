//! PF 插件市场——页面骨架与分段渲染（090-4）：页主体、头段（页签行 + 计数
//! + 错误/失效行）、卡行装配、扫描中/空态、页码段包装。
//!
//! 分段骨架与 pi 页同构（少第 0 段「最近发布」、少工具栏）；段外壳几何走
//! [`crate::market::shell`] 公共件，色板注入 PF 主题 token（见 [`super`] 头
//! 映射表）。卡片内部在 [`super::card`]，页码条在 [`super::pager`]。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, px, rgb};

use pi_link::pf_plugins::IndexEntry;

use super::card::card;
use super::pager::pager;
use super::{Chat, MONO, PfTab, line, muted, spaced_label};
use crate::appearance::theme::Theme;
use crate::i18n::{tf, tr};
use crate::market::pi_native_market::design::{Metrics, px18, sp};
use crate::market::pi_native_market::thousands;

/// 居中外壳（`market::shell` 公共件；`pf_installed` 从本路径导入）。
pub(crate) use crate::market::shell::centered;

/// 页面主体（content.rs `ContentView::PfMarket` 分支）。
///
/// 分段骨架与 pi 页同构（少第 0 段「最近发布」、少工具栏）：
/// `0 = 头段（标题+计数）｜1..1+rows = 卡行｜末项 = 页码条`。
pub(crate) fn page(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
) -> AnyElement {
    let t = crate::appearance::theme::theme();
    let vw = f32::from(window.viewport_size().width);
    let content_w = vw
        - if chat.layout.panes_hidden {
            0.
        } else {
            chat.layout.slp_w
        };
    let m = Metrics::new(content_w);
    let st = &chat.aux.market.pf;

    // **两个页签共用一套骨架**（2026-10-24 用户定夺）：头段在这里画一次，
    // body 按页签分派；各页签只提供内容，不许再各画一套根容器。
    let loading = st.loading;
    let entries: Vec<IndexEntry> = if st.tab == PfTab::All && !loading {
        super::index().packages.clone()
    } else {
        Vec::new()
    };
    let total = if st.tab == PfTab::All {
        entries.len()
    } else {
        st.ledger.len()
    };
    let head = head_segment(st.error.clone(), &st.stale, total, st.tab, weak, &m, t);
    let body: AnyElement = if st.tab == PfTab::Installed {
        super::super::pf_installed::body(chat, weak, &m)
    } else {
        all_body(chat, weak, &m, t, entries)
    };
    page_frame(head, body, &m, t)
}

/// 【所有插件】body：扫描中整块居中；否则卡片区（可滚、吃满中间）+ 页码器贴底。
fn all_body(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    m: &Metrics,
    t: &'static Theme,
    entries: Vec<IndexEntry>,
) -> AnyElement {
    let st = &chat.aux.market.pf;
    let loading = st.loading;
    let pending = st.pending.clone();
    let ledger = st.ledger.clone();
    // 插件卡预览图标：优先用**插件自己的状态栏图标**（`plugins/<id>/assets/…`，
    // 与状态栏 tab 同一个资产），未装载 / 无 status_bar 才回通用 package。
    let icons: Vec<(String, SharedString)> = chat
        .plugins
        .plugins
        .iter()
        .filter_map(|p| {
            crate::pf_plugin::status_bar_icon_path(p).map(|icon| (p.manifest.pkg.clone(), icon))
        })
        .collect();
    let cols = if m.narrow { 1 } else { 2 };
    let weak_c = weak.clone();
    let m = *m;

    let mut col = gpui::div()
        .id("pf-market-body")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .w_full()
        .flex()
        .flex_col();
    if loading {
        // 扫描中：整块居中（转圈 + 提示）；空态只在扫完且真为空时才出
        col = col.child(
            centered(
                slice_shell(&m, false, true, t)
                    .h_full()
                    .child(scanning_area(t)),
                &m,
            )
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex(),
        );
    } else {
        // 卡片区：吃满中间剩余高度（可滚），页码器固定贴底
        let mut cards = gpui::div()
            .id("pf-market-cards")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .w_full()
            .flex()
            .flex_col()
            .overflow_y_scroll();
        // 卡行**不再各自包段盒**：外层已经是全高段盒（两侧描边），行里再画
        // 一层 slice_shell 就会多出一条内线（2026-10-24 实测「左边两道竖线」）
        if entries.is_empty() {
            let mut row = row_shell(&m);
            row = row.child(empty_line(tr("market.pf.empty"), t));
            cards = cards.child(row);
        } else {
            for chunk in entries.chunks(cols) {
                let mut row = row_shell(&m);
                for e in chunk {
                    let busy = pending.as_deref() == Some(e.name.as_str());
                    row = row.child(card(&weak_c, e, &m, t, busy, &icons, &ledger));
                }
                if chunk.len() < cols {
                    row = row.child(gpui::div().flex_1());
                }
                cards = cards.child(row);
            }
        }
        col = col.child(
            centered(
                slice_shell(&m, false, false, t)
                    .flex_1()
                    .min_h_0()
                    .child(cards),
                &m,
            )
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex(),
        );
        // 页码器：末段（上分隔线 + 底描边），始终贴面板底部
        col = col.child(pager_block(&m, t));
    }
    col.into_any_element()
}

/// 页面骨架（**两个页签共用**）：头段 + body + 底部 `shell_pad`（与顶部对称）。
fn page_frame(head: AnyElement, body: AnyElement, m: &Metrics, t: &'static Theme) -> AnyElement {
    gpui::div()
        .w_full()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(head)
        .child(body)
        .pb(px(m.shell_pad))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 分段骨架（`market::shell` 公共件 + PF 主题 token 注入；pi 页 card.rs 同构：
// 一只盒子拆成段，各自画侧边描边）
// ---------------------------------------------------------------------------

/// 段外壳：两侧描边 + 面板底；`top/bottom` = 首/末段补边。
/// （PF 页：底 = bg_panel、线 = border@70%，全走主题 token。）
pub(crate) fn slice_shell(m: &Metrics, top: bool, bottom: bool, t: &Theme) -> gpui::Div {
    crate::market::shell::slice_shell(m, top, bottom, rgb(t.bg_panel), line(t))
}

/// 卡片头（标题件 + 可选右端件）。pi 页 `.content-card-header` 同构。
fn card_header(
    title: AnyElement,
    right: Option<AnyElement>,
    m: &Metrics,
    t: &'static Theme,
) -> AnyElement {
    crate::market::shell::card_header(title, right, m, line(t))
}

/// 页签行：`所有插件 | 已安装`（090-4 v4）。活动页签全亮，另一签
/// text_dim + hover 回亮，点击页内切换（pf_switch_tab）。
fn tab_row(tab: PfTab, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    gpui::div()
        .flex()
        .items_baseline()
        .gap(px(10.))
        .child(tab_label(
            "pf-tab-all",
            spaced_label(tr("market.pf.head")),
            tab == PfTab::All,
            weak,
            PfTab::All,
            t,
        ))
        .child(
            gpui::div()
                .font_family(MONO)
                .text_size(px(px18::TITLE_FS))
                .text_color(muted(t))
                .child(SharedString::from("|")),
        )
        .child(tab_label(
            "pf-tab-installed",
            spaced_label(tr("market.pf.tab_installed")),
            tab == PfTab::Installed,
            weak,
            PfTab::Installed,
            t,
        ))
        .into_any_element()
}

fn tab_label(
    id: &'static str,
    label: SharedString,
    active: bool,
    weak: &gpui::WeakEntity<Chat>,
    target: PfTab,
    t: &'static Theme,
) -> AnyElement {
    let weak = weak.clone();
    let mut el = gpui::div()
        .id(id)
        .font_family(MONO)
        .text_size(px(px18::TITLE_FS))
        .cursor_pointer()
        .text_color(rgb(if active { t.text } else { t.text_dim }))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| c.pf_switch_tab(target, cx));
        });
    if !active {
        el = el.hover(|s| s.text_color(rgb(t.text)));
    }
    el.child(label).into_any_element()
}

/// ① 头段：页签行 + 计数（+ 安装错误行）。**首段自带 `pt(shell_pad)`**——
/// pi 页这个上内距原本挂在「最近发布」段上，本页没有那段，必须自己补，
/// 否则头段贴着内容区顶边（用户原话：上边距没了）。
pub(crate) fn head_segment(
    err: Option<String>,
    stale: &[crate::pf_plugin::StalePlugin],
    total: usize,
    tab: PfTab,
    weak: &gpui::WeakEntity<Chat>,
    m: &Metrics,
    t: &'static Theme,
) -> AnyElement {
    // 计数只服务【所有插件】（页内条目 / 总数）；【已安装】不显示任何数字
    let count = (tab == PfTab::All).then(|| {
        gpui::div()
            .font_family(MONO)
            .text_size(px(px18::COUNT_FS))
            .text_color(muted(t))
            .child(SharedString::from(format!(
                "1-{} / {}",
                total,
                thousands(total)
            )))
    });
    let err_el = err.map(|e| {
        gpui::div()
            .px(px(m.card_pad_body))
            .pb(px(sp::BODY_GAP))
            .text_size(px(11.))
            .text_color(rgb(t.danger))
            .child(e)
    });
    // 失效包常驻通知行（090-10 迁移体验）：WARN 黄点（规范 §6「内部中断」
    // 语义，非外部错误）+ 说明档 ui(11) text_dim。两个页签都可见，更新完
    // 自动消失——启动 toast 只播一次，用户不在电脑前错过了靠这行兜底。
    let stale_el = (!stale.is_empty()).then(|| {
        let list = stale
            .iter()
            .map(|s| format!("{} v{}", s.title_zh, s.version))
            .collect::<Vec<_>>()
            .join("、");
        gpui::div()
            .id("pf-stale-notice")
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(m.card_pad_body))
            .pb(px(sp::BODY_GAP))
            .text_size(px(11.))
            .text_color(rgb(t.text_dim))
            .child(crate::ui::kit::status_dot(crate::appearance::theme::WARN))
            .child(SharedString::from(tf(
                "market.pf.stale_head",
                &[("n", stale.len().to_string()), ("list", list)],
            )))
    });
    centered(
        slice_shell(m, true, false, t)
            .child(card_header(
                tab_row(tab, weak, t),
                count.map(IntoElement::into_any_element),
                m,
                t,
            ))
            .children(stale_el)
            .children(err_el),
        m,
    )
    .pt(px(m.shell_pad))
    .into_any_element()
}

/// 卡行：左右两道内距 + 上内距（pi 页由工具栏容器给 `pt`，本页没有工具栏，
/// 这一份挪到卡行段自己身上）+ 行间距。
fn row_shell(m: &Metrics) -> gpui::Div {
    crate::market::shell::row_shell(m, true)
}

/// 扫描中：居中转圈 + 提示。字号对齐 Pi 市场的扫描提示（`px18::BODY_FS`）。
fn scanning_area(t: &'static Theme) -> AnyElement {
    gpui::div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(9.))
        .text_size(px(px18::BODY_FS))
        .text_color(muted(t))
        .child(crate::ui::spinner(16., t.text_dim))
        .child(tr("market.pf.scanning").to_string())
        .into_any_element()
}

/// 空态行（pi 页 `.packages-empty` 同构：纯文本，无插图）；水平居中。
fn empty_line(text: &'static str, t: &'static Theme) -> AnyElement {
    gpui::div()
        .flex()
        .justify_center()
        .w_full()
        .py(px(24.))
        .text_size(px(px18::BODY_FS))
        .text_color(muted(t))
        .child(text)
        .into_any_element()
}

/// ③ 页码段（PF 本地索引恒单页；贴面板底，无外壳底留白）。
fn pager_block(m: &Metrics, t: &'static Theme) -> AnyElement {
    crate::market::shell::pager_block(m, pager(1, 1, t), true, 0., rgb(t.bg_panel), line(t))
}
