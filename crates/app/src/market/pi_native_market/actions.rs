//! Pi 原生扩展市场——Chat 动作链（040-2）：打开/装载、页内过滤去抖、
//! 服务端搜索、类型/排序/重置、真安装、拉页与回包落地。

use std::time::Duration;

use gpui::prelude::*;

use pi_link::market::pi_ext_market::{MarketEntry, PAGE_SIZE};

use crate::Chat;
use crate::ContentView;
use crate::PanelTab;
use crate::ui::TextInput;

use super::design;
use super::{FILTER_DEBOUNCE_MS, NativeKind, NativeSort, RECENT_N};

impl Chat {
    /// 打开「Pi 扩展市场」标准页（菜单项、自动化动作都走这里）。
    /// **不加 dev-flag 闸门**（2026-10-23 用户定夺）：菜单里恒有这一项，就必须
    /// 恒能打开——否则项在、点了没反应（旗标关的数据目录曾整页点不开）。
    /// `pi_native_market` 旗标自此已无读取点（workspace 侧闸门函数 2026-10-24
    /// 随死码清掉；prefetch 只预取 040-1 页、读的是 `pi_ext_market`；
    /// pi-link `KNOWN` 暂留该键，仅为启动状态行不缺项）。
    pub(crate) fn open_pi_native_market(&mut self, cx: &mut gpui::Context<Self>) {
        self.dlg.market_menu = None;
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::PiNativeMarket))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(PanelTab::PiNativeMarket);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(ContentView::PiNativeMarket);
        self.native_ensure_input(cx);
        self.native_ensure_loaded(cx);
        cx.notify();
    }

    /// 搜索框 + 两个下拉的守卫懒创建（都要 `&mut Context`）。
    fn native_ensure_input(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.native.kind_guard.is_none() {
            self.aux.market.native.kind_guard = Some(cx.new(|_| crate::ui::DropdownState::new()));
        }
        if self.aux.market.native.sort_guard.is_none() {
            self.aux.market.native.sort_guard = Some(cx.new(|_| crate::ui::DropdownState::new()));
        }
        if self.aux.market.native.input.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let input = cx.new(|cx| {
            let mut ti = TextInput::new(cx)
                .placeholder("Filter packages…")
                // 深肤页：主题 token 是按 PF 主题算的（浅色主题下光标=墨黑、
                // 占位=深棕），在本页深底上几乎看不见——按页内色板覆盖：
                // 光标 = 页正文白（design TEXT），占位 = 页内灰（design muted）。
                .caret_color(design::TEXT)
                .placeholder_color(design::MUTED)
                .flat()
                .form();
            // 打字 = 页内过滤（0 网络，160ms 去抖）；回车 = 服务端搜索。
            // ⚠ 两个回调都在 TextInput 自己的 update 里发出：**不许回头读这个
            // 实体**（重复租 = panic），值已经传进来了。
            let w_change = weak.clone();
            ti.set_on_change(Box::new(move |v, cx| {
                let v = v.to_string();
                let _ = w_change.update(cx, |c, cx| c.native_set_filter(v, cx));
            }));
            let w_submit = weak.clone();
            ti.set_on_submit(Box::new(move |v, cx| {
                let v = v.to_string();
                let _ = w_submit.update(cx, |c, cx| c.native_search_with(v, cx));
            }));
            ti
        });
        self.aux.market.native.input = Some(input);
    }

    /// 首次装载（无本地快照：本页不落盘，直接拉）。已装载过且空了才补拉。
    pub(crate) fn native_ensure_loaded(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.native.loaded_once {
            if self.aux.market.native.entries.is_empty() && !self.aux.market.native.loading {
                self.native_reload(cx);
            }
            return;
        }
        self.aux.market.native.loaded_once = true;
        self.aux.market.native.kinds = pi_link::market::pi_ext_market::KindsCache::load();
        self.native_reload(cx);
    }

    /// 回第一页重拉（换词 / 重置 / 重试共用）。
    pub(crate) fn native_reload(&mut self, cx: &mut gpui::Context<Self>) {
        self.aux.market.native.page = 1;
        self.native_fetch(1, cx);
    }

    /// 翻到指定页（页码条 / 上一页 / 下一页共用）。
    pub(crate) fn native_goto(&mut self, page: usize, cx: &mut gpui::Context<Self>) {
        let page = page.clamp(1, self.aux.market.native.total_pages());
        if page == self.aux.market.native.page {
            return;
        }
        // 翻页回顶（设计稿是整页滚动，新页必须从标题处看起）
        self.aux.market.native.list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: gpui::px(0.),
        });
        self.native_fetch(page, cx);
    }

    /// 页内过滤（打字）。**0 网络**，但**去抖 160ms 再重排**（设计稿 JS 同口径）：
    /// 每键都重排 50 张卡在慢机器上会顿；输入框自身的回显与这里无关（内件
    /// 自己重绘），所以延迟只影响列表。
    pub(crate) fn native_set_filter(&mut self, v: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.native.filter == v {
            return;
        }
        self.aux.market.native.filter = v;
        self.aux.market.native.filter_gen += 1;
        let want = self.aux.market.native.filter_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(FILTER_DEBOUNCE_MS))
                .await;
            let _ = this.update(cx, |c, cx| {
                if c.aux.market.native.filter_gen == want {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 回车/搜索钮 = 服务端搜索（回第一页）。
    pub(crate) fn native_search_with(&mut self, v: String, cx: &mut gpui::Context<Self>) {
        self.aux.market.native.query = v;
        self.native_reload(cx);
    }

    /// 重置（设计稿 `a[href="/packages"]`）：清词 + 类型 + 排序，回第一页。
    pub(crate) fn native_reset(&mut self, cx: &mut gpui::Context<Self>) {
        let st = &mut self.aux.market.native;
        st.query.clear();
        st.filter.clear();
        st.kind = NativeKind::All;
        st.sort = NativeSort::Downloads;
        st.kind_open = false;
        st.sort_open = false;
        if let Some(ix) = st.input.as_ref() {
            ix.update(cx, |ti, cx| ti.set_value(String::new(), cx));
        }
        self.native_reload(cx);
    }

    /// 类型筛选（下拉）：**重查第一页**（pi.dev `?type=` 口径——类型切换 =
    /// 一次新查询）。装载链落地后会重补 kinds，首装填充失败的坏态随重查
    /// 自愈（旧实现纯渲染期过滤，kinds 一旦没补上就恒 0 且永不重试）。
    pub(crate) fn native_set_kind(&mut self, k: NativeKind, cx: &mut gpui::Context<Self>) {
        self.aux.market.native.kind = k;
        self.aux.market.native.kind_open = false;
        self.native_reload(cx);
    }

    /// 排序（下拉）；纯渲染期重排，0 网络。
    pub(crate) fn native_set_sort(&mut self, s: NativeSort, cx: &mut gpui::Context<Self>) {
        self.aux.market.native.sort = s;
        self.aux.market.native.sort_open = false;
        cx.notify();
    }

    /// 「install」= **真安装**：走 040 设置页同一条 `pi install npm:<name>`
    /// CLI 管线（`mc_install_package` → `pkg_op` 单飞 → op pump 收尾刷新）。
    /// 运行中按钮转圈（`pkg_op` 全局单飞，与设置页/040-1 共用）。
    pub(crate) fn native_install(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        let source = format!("npm:{name}");
        self.aux.market.native.pending = Some(source.clone());
        self.mc_install_package(source, cx);
    }

    /// 类型徽标所需的 `pi` 清单类型（后台补齐，到齐后整页刷新一次）。
    fn native_fetch(&mut self, page: usize, cx: &mut gpui::Context<Self>) {
        let st = &mut self.aux.market.native;
        st.page = page;
        st.gen_ix += 1;
        st.loading = true;
        st.error = None;
        st.kinds_pending = true;
        let gen_ix = st.gen_ix;
        let query = st.query.clone();
        let from = page.saturating_sub(1) * PAGE_SIZE;
        // 只有第一页带「最近发布」（设计稿那一栏是全局的，不随页码变）
        let want_recent = page == 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_executor()
                .spawn(async move {
                    let page = pi_link::market::pi_ext_market::search(&query, from, PAGE_SIZE)?;
                    let recent = if want_recent {
                        pi_link::market::pi_ext_market::search_recent(RECENT_N).unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    Ok::<_, String>((page, recent))
                })
                .await;
            let (page, recent) = match fetched {
                Ok(v) => v,
                Err(e) => {
                    let _ = this.update(cx, |c, cx| c.native_fail(gen_ix, e, cx));
                    return;
                }
            };
            let total = page.total;
            let entries = page.entries;
            let ok = this
                .update(cx, |c, cx| {
                    c.native_land(gen_ix, total, entries, recent, cx)
                })
                .is_ok();
            if !ok {
                return;
            }
            // 类型徽标：50 个包的 `latest` 清单（并发 8，约 3-6s）。整批到齐再刷
            // 一次，不做流式 append——本页卡片等高，逐批上屏会跳版。
            // kinds 表（会话级 + 磁盘缓存）作起点：命中即 0 网络，重查立即出结果。
            let (names, table) = this
                .update(cx, |c, _| {
                    if c.aux.market.native.gen_ix != gen_ix {
                        return (Vec::new(), Default::default());
                    }
                    (
                        c.aux.market.native.entries.clone(),
                        c.aux.market.native.kinds.clone(),
                    )
                })
                .unwrap_or_default();
            if names.is_empty() {
                return;
            }
            let filled = cx
                .background_executor()
                .spawn(async move {
                    let mut entries = names;
                    let mut kinds = table;
                    pi_link::market::pi_ext_market::fill_kinds(&mut entries, &mut kinds);
                    (entries, kinds)
                })
                .await;
            let _ = this.update(cx, |c, cx| {
                let (entries, table) = filled;
                // 磁盘缓存同步落一份（失败静默）；会话表照常更新
                pi_link::market::pi_ext_market::KindsCache::save(&table);
                c.aux.market.native.kinds = table;
                c.native_kinds_land(gen_ix, entries, cx);
            });
        })
        .detach();
    }

    fn native_fail(&mut self, gen_ix: u64, err: String, cx: &mut gpui::Context<Self>) {
        if gen_ix != self.aux.market.native.gen_ix {
            return;
        }
        self.aux.market.native.loading = false;
        self.aux.market.native.kinds_pending = false;
        self.aux.market.native.error = Some(err);
        cx.notify();
    }

    fn native_land(
        &mut self,
        gen_ix: u64,
        total: usize,
        entries: Vec<MarketEntry>,
        recent: Vec<MarketEntry>,
        cx: &mut gpui::Context<Self>,
    ) {
        if gen_ix != self.aux.market.native.gen_ix {
            return;
        }
        let st = &mut self.aux.market.native;
        st.loading = false;
        st.error = None;
        st.total = total;
        st.entries = entries;
        if !recent.is_empty() {
            st.recent = recent;
        }
        cx.notify();
    }

    fn native_kinds_land(
        &mut self,
        gen_ix: u64,
        entries: Vec<MarketEntry>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.aux.market.native.kinds_pending = false;
        if gen_ix != self.aux.market.native.gen_ix
            || entries.len() != self.aux.market.native.entries.len()
        {
            return;
        }
        self.aux.market.native.entries = entries;
        cx.notify();
    }
}
