//! Pi 原生扩展市场——页面渲染（040-2）：变高虚拟列表的分段装配
//! （最近发布 / 头+工具栏 / 卡行×N / 页码条）与竖向滚动条。

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, Styled};

use pi_link::market::pi_ext_market::MarketEntry;

use super::card::{empty_line, index_head, pager_block, recent_block, row_shell, row_slice};
use super::controls::{action_bar, pager};
use super::design::{self, Metrics};
use super::{Chat, NativeKind};
use crate::ui::TextInput;

/// 工具栏的入参快照（列表项闭包要 `'static`，状态得先拷成独立值）。
#[derive(Clone)]
pub(crate) struct BarModel {
    pub(crate) input: Option<gpui::Entity<TextInput>>,
    pub(crate) kind_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    pub(crate) sort_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    pub(crate) kind: NativeKind,
    pub(crate) sort: super::NativeSort,
    pub(crate) kind_open: bool,
    pub(crate) sort_open: bool,
}

impl BarModel {
    pub(crate) fn of(st: &super::NativeState) -> Self {
        Self {
            input: st.input.clone(),
            kind_guard: st.kind_guard.clone(),
            sort_guard: st.sort_guard.clone(),
            kind: st.kind,
            sort: st.sort,
            kind_open: st.kind_open,
            sort_open: st.sort_open,
        }
    }
}

/// 页面主体（content.rs `ContentView::PiNativeMarket` 分支）。
pub(crate) fn page(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
) -> AnyElement {
    let vw = f32::from(window.viewport_size().width);
    let content_w = vw
        - if chat.layout.panes_hidden {
            0.
        } else {
            chat.layout.slp_w
        };
    let m = Metrics::new(content_w);
    let st = &chat.aux.market.native;

    // 可见条目（过滤+排序的唯一口径）；闭包要 'static，故整表 own 一份
    let visible: Vec<MarketEntry> = st.visible().into_iter().cloned().collect();
    let cols = if m.narrow { 1 } else { 2 };
    let rows = visible.len().div_ceil(cols).max(1);
    // 列表项：0 = 最近发布；1 = 头+工具栏；2..2+rows = 卡行；末项 = 页码条
    let items = 2 + rows + 1;
    if st.list.item_count() != items {
        // 项高只跟**项数**走（卡定高、头部定高）→ 只按项数判断要不要 reset，
        // 否则每次过滤/打字都丢滚动位
        st.list.reset(items);
    }

    let recent_src = st.recent.clone();
    let loading = st.loading;
    // 类型过滤下的空窗期（搜索已落地、kinds 还在补）：显示加载中而非「无匹配」，
    // 否则选类型后 ~15s 里页面看起来像坏了（0 条 + 旧 pager）。
    let kinds_loading = st.kinds_pending
        && st.kind != NativeKind::All
        && st.entries.iter().all(|e| e.kinds.is_empty());
    let empty_text: &'static str = if loading {
        "Loading packages…"
    } else if st.error.is_some() {
        "Failed to load. Try Reset."
    } else if kinds_loading {
        "Loading type info…"
    } else {
        "No packages match this filter."
    };
    let bar = BarModel::of(st);
    let shown = visible.len();
    // 类型过滤是**客户端**过滤（npm 无类型查询参数）：过滤集只覆盖当前页 50 条，
    // 服务端 total/pager 对过滤集没有意义——计数与分页改按过滤集本页真实值，
    // 不再显示未过滤全集的 13,084 / 266 页。
    let kind_only = st.kind != NativeKind::All;
    let (total, page_ix, total_pages) = if kind_only {
        (shown, 1, 1)
    } else {
        (st.total, st.page, st.total_pages())
    };
    let filtered = !st.filter.trim().is_empty() || st.kind != NativeKind::All;
    let pending = st.pending.clone();
    let installed_set = chat.pi_ext_installed_sources();
    let weak_c = weak.clone();
    let m_c = m;
    let list = gpui::list(st.list.clone(), move |ix, _w, _cx| {
        if ix == 0 {
            return recent_block(&recent_src, &weak_c, loading, &m_c);
        }
        if ix == 1 {
            return index_head(
                action_bar(bar.clone(), &weak_c, &m_c),
                shown,
                total,
                page_ix,
                filtered,
                &m_c,
            );
        }
        if ix == 1 + rows + 1 {
            return pager_block(pager(&weak_c, page_ix, total_pages, &m_c, loading), &m_c);
        }
        let r = ix - 2;
        let mut row = row_shell(&m_c);
        if visible.is_empty() {
            row = row.child(empty_line(empty_text, loading || kinds_loading));
        } else {
            let start = r * cols;
            for e in visible.iter().skip(start).take(cols) {
                let busy = pending.as_deref() == Some(e.source().as_str());
                let installed = installed_set.contains(&e.source());
                row = row.child(super::package::package_card(
                    e, &weak_c, &m_c, busy, installed,
                ));
            }
            if visible.len().saturating_sub(start) < cols {
                row = row.child(gpui::div().flex_1());
            }
        }
        row_slice(row, &m_c)
    })
    .flex_1()
    .min_h_0();

    let mut root = gpui::div()
        .id("pi-native-market")
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .bg(gpui::rgb(design::BG_DEEP))
        .child(list);

    // 竖向滚动条（变高 ListState：与 083 搜索结果同挂法）
    if st.list.max_offset_for_scrollbar().height > gpui::px(0.) {
        let handle = crate::ui::list_handle::ListStateHandle(st.list.clone());
        root = root.child(
            gpui::div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(gpui::px(2.))
                .w(gpui::px(8.))
                .child(gpui_component::scroll::Scrollbar::vertical(&st.sb, &handle)),
        );
    }
    root.into_any_element()
}
