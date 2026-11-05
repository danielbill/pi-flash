//! Pi 扩展市场——页面主体渲染（040-1）：工具栏 / 摘要 / 列表 / 翻页的装配，
//! 与行数据 `RowData` 的渲染期派生（已装/有新版不落状态）。
//!
//! 几何常量与 `card_metrics` 住在文件头（卡片子件 [`super::card`]、骨架
//! [`super::states`] 共用同一套量）；状态机与动作在 [`super`]，黄条/骨架/
//! 空态在 [`super::states`]，工具栏与右列宽在 [`super::toolbar`]。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, SharedString, Styled};
use pi_link::market::pi_ext_market::{MarketEntry, days_since};

use super::card::{dot, pkg_card};
use super::pager::pager;
use super::states::{banner, empty_area, skeleton_rows};
use super::toolbar::{side_col_w, toolbar};
use super::{Chat, ExtFilter, ExtSort, LIST_ID};
use crate::appearance::theme::Theme;
use crate::appearance::theme::theme as T;
use crate::i18n::{tf, tr};
use crate::ui::tokens::space;
use crate::ui::{BtnSize, BtnVariant};

/// 缩略槽宽高。
pub(super) const THUMB_W: f32 = 88.;
pub(super) const THUMB_H: f32 = 72.;
/// 左缘内缩 = 列表容器内缩 SP5(10)：工具栏 / 摘要 / 翻页条的**左缘与卡片外框同一竖线**。
pub(super) const PAGE_LEFT: f32 = space::SP5;
/// 翻页条高（底部高度）。列表底部留白与它同高：
/// 「已安装」页签没有翻页条，若不留这一尺寸，末卡就胶在窗口底边上。
pub(super) const PAGER_H: f32 = 44.;
/// 行右边距 = SP7(16) = 滚动条 8 + 间隙 8：滚动条压不到卡片。
///
/// 为何不用容器 pr：`vlist` 的滚动条是 wrapper 内的
/// 绝对定位归属，容器 pr 会把 wrapper（连同滚动条）一起左移，
/// 滚动条仍压在卡片上。只有「行内 mr」能让卡片退让滚动条。
pub(super) const ROW_PAD_R: f32 = space::SP7;
/// 右缘内缩 = 行右边距：工具栏按钮 / 摘要 / 翻页条 ⊥ **卡片外框右缘**。
pub(super) const PAGE_RIGHT: f32 = ROW_PAD_R;
/// 工具栏（input 行）上 padding。
pub(super) const TOOLBAR_PT: f32 = 25.;
/// 工具栏（input 行）下 padding。
pub(super) const TOOLBAR_PB: f32 = 15.;
/// 摘要行下 padding：摘要行（含「全部更新」按钮）下缘 → 列表 = 10
/// （卡片自带 mt SP4(8)，这里只补差额）。
pub(super) const SUMMARY_PB: f32 = 10. - space::SP4;
/// 描述行高（行高 18 × 行数）。用 `line_clamp` 硬截行（gpui 按行数 shape），
/// 不用固定高度盖裁（会出现半行）。
pub(super) const DESC_LINE_H: f32 = 18.;
/// 描述最多行数。
pub(super) const DESC_LINES: usize = 3;
/// 整页内容栏上限（工具栏/摘要/列表/翻页同栏，超宽窗口整体居中）
pub(super) const PAGE_MAX_W: f32 = 1080. + PAGE_LEFT + ROW_PAD_R;

/// 卡片高度一套量（均跟面板字号缩放）。
///
/// 卡高**由内部部件算出来**，不写死字面量：写死一旦面板字号
/// 变就会被 flex 挤掉描述的第三行（半行问题）。
pub(super) mod card_metrics {
    use super::*;
    /// 名称行行高（字号 15 / 行高 20）。
    pub fn head_h() -> f32 {
        f32::from(crate::appearance::ui_size(20.))
    }
    /// 描述行高（18 × 行数）。
    pub fn desc_h() -> f32 {
        f32::from(crate::appearance::ui_size(super::DESC_LINE_H)) * super::DESC_LINES as f32
    }
    /// 底行高（MD 按钮档 28；页面按钮统一 MD）。
    pub fn bottom_h() -> f32 {
        crate::ui::tokens::btn::MD_H
    }
    /// 文本列的子元素：标题 / 描述 / 弹性占位 / 底行 —— 共 4 个。
    /// flex 列的 gap 在**6 个相邻对之间**各生效 1 次，共 3 个间隙。
    const TEXT_COL_ITEMS: usize = 4;
    /// 卡片内容高（不含内距与边框）。
    pub fn body_h() -> f32 {
        head_h() + space::SP2 * (TEXT_COL_ITEMS - 1) as f32 + desc_h() + bottom_h()
    }
    /// 卡片总高（含内距与边框）。
    pub fn card_h() -> f32 {
        body_h() + space::SP5 * 2. + 2.
    }
    /// 列表行高（卡片 + 行间空隙）。
    pub fn row_h() -> f32 {
        card_h() + space::SP4
    }
}

/// 行状态（渲染时派生，不落状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowStatus {
    NotInstalled,
    UpdateAvailable,
    Installed,
}

pub(super) struct RowData {
    pub(super) name: String,
    pub(super) desc: String,
    pub(super) version: String,
    pub(super) author: String,
    pub(super) downloads: String,
    pub(super) ago: String,
    pub(super) repo: Option<String>,
    pub(super) cmd: String,
    pub(super) status: RowStatus,
    pub(super) busy: bool,
    /// 已装条目在 `cfg.mc_pkgs_global` 里的下标（未装 = None）
    pub(super) installed_ix: Option<usize>,
    /// 已装且启用中（settings 条目四数组未置空）
    pub(super) enabled: bool,
}

/// 页面主体（content.rs `ContentView::PiExtMarket` 分支）。
pub(crate) fn page(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
) -> AnyElement {
    let t: &'static Theme = T();
    // 工具栏右端分段控件 / 摘要行右端「全部更新」共用列宽：两处右缘都贴
    // PAGE_RIGHT，等宽才能左右缘同时对齐（设计稿竖向对齐）。
    let side_w = side_col_w(window);
    let st = &chat.aux.market.pi_ext;
    let installed = chat.pi_ext_installed_sources();
    let pending_source = st.pending.clone();
    let busy_any = chat.cfg.pkg_op.is_some();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let rows: Vec<RowData> = if st.filter == ExtFilter::Installed {
        // 已装页签 = **本地全量**：npm 搜索只回当前页 ≤50 条，装过的包未必
        // 都在里面（离线更是 0 条）。直接列 `mc_pkgs_global`；npm 缓存里有
        // 的字段借用，缺的（版本/描述）读本地 package.json 补齐。每次渲染
        // 每包 2 个小文件读，仅本页签生效，量级 = 已装包数，可接受。
        let mut rows = Vec::new();
        for (ix, v) in chat.cfg.mc_pkgs_global.iter().enumerate() {
            let source = pi_link::skills::entry_source(v);
            if source.is_empty() {
                continue;
            }
            let name = pi_link::skills::display_source(&source).to_string();
            let cached = st.entries.iter().find(|e| e.name == name);
            let status = if chat.aux.pi_extension_updates.state_of(&source)
                == Some(crate::services::pi_extension_updates::PkgUpdateState::UpdateAvailable)
            {
                RowStatus::UpdateAvailable
            } else {
                RowStatus::Installed
            };
            rows.push(RowData {
                name,
                desc: cached
                    .map(|e| e.description.clone())
                    .filter(|d| !d.is_empty())
                    .or_else(|| pi_link::skills::package_description(&source))
                    .unwrap_or_default(),
                version: pi_link::skills::package_installed_version(&source)
                    .or_else(|| cached.map(|e| e.version.clone()))
                    .unwrap_or_default(),
                author: cached.and_then(|e| e.author.clone()).unwrap_or_default(),
                downloads: cached
                    .map(|e| pi_link::market::pi_ext_market::format_downloads(e.monthly_downloads))
                    .unwrap_or_default(),
                ago: cached
                    .map(|e| ago_label(&e.updated, now))
                    .unwrap_or_default(),
                repo: cached.and_then(|e| e.repo.clone()),
                cmd: String::new(), // 已装没有「安装命令」可抄
                status,
                busy: pending_source.as_deref() == Some(source.as_str()),
                installed_ix: Some(ix),
                enabled: !pi_link::skills::entry_disabled(v),
            });
        }
        rows
    } else {
        let mut list: Vec<&MarketEntry> = st
            .entries
            .iter()
            .filter(|e| match st.filter {
                ExtFilter::Installed => installed.contains(&e.source()),
                ExtFilter::NotInstalled => !installed.contains(&e.source()),
            })
            .collect();
        match st.sort {
            ExtSort::Downloads => {} // npm 服务端序（权重 ≈ 下载量）
            ExtSort::Updated => list.sort_by(|a, b| b.updated.cmp(&a.updated)),
            ExtSort::Name => list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        }
        list.into_iter()
            .map(|e| {
                let source = e.source();
                let status = if !installed.contains(&source) {
                    RowStatus::NotInstalled
                } else if chat.aux.pi_extension_updates.state_of(&source)
                    == Some(crate::services::pi_extension_updates::PkgUpdateState::UpdateAvailable)
                {
                    RowStatus::UpdateAvailable
                } else {
                    RowStatus::Installed
                };
                let installed_ix = chat
                    .cfg
                    .mc_pkgs_global
                    .iter()
                    .position(|v| pi_link::skills::entry_source(v) == source);
                let enabled = installed_ix
                    .and_then(|ix| chat.cfg.mc_pkgs_global.get(ix))
                    .map(|v| !pi_link::skills::entry_disabled(v))
                    .unwrap_or(true);
                RowData {
                    name: e.name.clone(),
                    desc: e.description.clone(),
                    version: e.version.clone(),
                    author: e.author.clone().unwrap_or_default(),
                    downloads: pi_link::market::pi_ext_market::format_downloads(
                        e.monthly_downloads,
                    ),
                    ago: ago_label(&e.updated, now),
                    repo: e.repo.clone(),
                    cmd: e.install_command(),
                    status,
                    busy: pending_source.as_deref() == Some(source.as_str()),
                    installed_ix,
                    enabled,
                }
            })
            .collect()
    };

    // 内容列：工具栏 / 摘要 / 列表 / 翻页全部住这里，外层 items_center
    // 让它在超宽窗口下**整体居中**（v20 只限列表不居中，用户驳回）
    let mut col = gpui::div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(toolbar(chat, weak, t, side_w));

    if let Some(banner) = banner(chat, weak, t) {
        col = col.child(banner);
    }
    // 摘要行：已装页签 = 本地条数；未安装页签 = npm 结果数（total 未知不占位）
    // 行内一律 items_end：文字底缘与「全部更新」按钮底缘同一条线（用户定稿：下对齐）
    let searching = st.loading;
    if st.filter == ExtFilter::Installed {
        col = col.child(
            gpui::div()
                .flex_shrink_0()
                .flex()
                .items_end()
                .justify_start()
                .gap(gpui::px(space::SP3))
                .pl(gpui::px(PAGE_LEFT))
                .pr(gpui::px(PAGE_RIGHT))
                .pt(gpui::px(0.))
                .pb(gpui::px(SUMMARY_PB))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(gpui::rgb(t.text_soft))
                // 请求未回包时的等待动画（常驻最左，不跳位）
                .children(searching.then(|| crate::ui::spinner(12., t.accent)))
                .child(
                    gpui::div()
                        .text_color(gpui::rgb(t.text_dim))
                        .child(SharedString::from(thousands(rows.len()))),
                )
                .child(tf("market.installed_summary", &[("n", String::new())]))
                // 「全部更新」在摘要行右端
                .child(gpui::div().flex_1())
                .child(update_all_button(chat, weak, busy_any, side_w)),
        );
    } else {
        let count = if st.total > 0 {
            st.total
        } else {
            st.entries.len()
        };
        col = col.child(
            gpui::div()
                .flex_shrink_0()
                .flex()
                .items_end()
                .justify_start()
                .gap(gpui::px(space::SP3))
                .pl(gpui::px(PAGE_LEFT))
                .pr(gpui::px(PAGE_RIGHT))
                .pt(gpui::px(0.))
                .pb(gpui::px(SUMMARY_PB))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(gpui::rgb(t.text_soft))
                .children(searching.then(|| crate::ui::spinner(12., t.accent)))
                .child(
                    gpui::div()
                        .text_color(gpui::rgb(t.text_dim))
                        .child(SharedString::from(if count == 0 {
                            "—".to_string()
                        } else {
                            thousands(count)
                        })),
                )
                .child(tr("market.results").to_string())
                .child(dot(t))
                .child(tr("market.only_ext").to_string())
                // 「全部更新」在摘要行右端
                .child(gpui::div().flex_1())
                .child(update_all_button(chat, weak, busy_any, side_w)),
        );
    }

    let list_area = if rows.is_empty() {
        if st.loading {
            skeleton_rows(t)
        } else {
            empty_area(chat, t)
        }
    } else {
        crate::ui::vlist::vlist(
            LIST_ID,
            rows.len(),
            // 行高 = 卡片高 + 行间空隙（由 card_metrics 统一算，与卡片实际高度一致）
            card_metrics::row_h(),
            crate::ui::vlist::VListHeight::Fill,
            false,
            true,
            tr("market.empty"),
            None,
            {
                let weak = weak.clone();
                move |ix, _window, _cx| pkg_card(&rows[ix], &weak, t, busy_any)
            },
        )
    };
    col = col.child(
        gpui::div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .pl(gpui::px(PAGE_LEFT))
            .child(list_area),
    );

    // 翻页条：未安装页签**恒常渲染**。底部永远占 44px，列表不会被撑到底边、
    // 末条不被削；total 未知时只有第 1 页（翻页按钮禁用）。
    // 翻页条**两个页签都渲染**（已安装页签 = 本地全量、无分页 → 整条禁用占位），
    // 底部结构与未安装页签完全一致：末卡不会贴到窗口底边，页面高度也不跳。
    //
    // 页数口径：未安装 = npm 搜索全局 total 折算；已安装 = **本地条数**
    // （已装列表是本地全量、不分页 → 只有第 1 页，不能用全局 total
    // 算出「第 1 / 248 页」这种说走的计数）。
    let (pager_page, pager_pages) = st.pager();
    col = col.child(pager(weak, pager_page, pager_pages, t));
    gpui::div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .bg(gpui::rgb(t.bg))
        .child(
            gpui::div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    gpui::div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .max_w(gpui::px(PAGE_MAX_W))
                        .flex()
                        .flex_col()
                        .child(col),
                ),
        )
        .into_any_element()
}

/// 「全部更新 (n)」按钮：放在摘要行右端（与结果数同行，右对齐）。
/// 宽度取 `side_w`（= 工具栏分段控件宽）→ 与上方 switch 左右缘对齐。
fn update_all_button(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    busy_any: bool,
    side_w: f32,
) -> AnyElement {
    let n = chat.aux.pi_extension_updates.available();
    let label = if n > 0 {
        format!("{} {n}", tr("market.update_all"))
    } else {
        tr("market.update_all").to_string()
    };
    let disabled = n == 0 || busy_any || chat.aux.pi_extension_updates.checking;
    let weak_all = weak.clone();
    // 走 button_base 才能带 w_full（ui::button 不支持自加宽度）
    let el = crate::ui::button_base("pi-ext-update-all", BtnSize::Md, BtnVariant::Secondary)
        .w_full()
        .child(label);
    let el = if disabled {
        el.opacity(0.5)
    } else {
        el.on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak_all.update(cx, |c, cx| c.pi_ext_update_all(cx));
        })
    };
    gpui::div()
        .w(gpui::px(side_w))
        .flex_shrink_0()
        .child(el)
        .into_any_element()
}

/// 千分位（12,365）。
pub(super) fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 「2 天前 / 今天」文案（`days_since` 纯函数出天数）。
pub(super) fn ago_label(iso: &str, now_unix: i64) -> String {
    match days_since(iso, now_unix) {
        Some(0) => tr("market.today").to_string(),
        Some(d) if d > 0 => tf("market.days_ago", &[("n", d.to_string())]),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(12365), "12,365");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
