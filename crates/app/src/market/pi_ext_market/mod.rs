//! Pi 扩展市场页（040-1）——页内状态机与动作（v4 单列卡片 + 翻页）。
//!
//! 文件组织：状态机与动作在本文件；渲染拆 [`render`]（页面主体 + 几何常量 +
//! `card_metrics`）/ [`toolbar`]（工具栏 + 右列宽）/ [`states`]（黄条红条/
//! 骨架/空态）；卡片子件在 [`card`]、翻页条在 [`pager`]。数据面在
//! [`pi_link::market::pi_ext_market`]（npm 搜索 + latest 清单 + 首页快照）。
//!
//! - **单列卡片**：一张卡 = 包名/版本/类型 chip + 两行内描述 + 作者·下载量·时间 +
//!   npm/repo 链接 + 安装命令条（**无复制键**，见 040-1 定稿）；
//! - **翻页**（官方式）：← 上一页 / 页码窗口 / 下一页 →；一页 50 条，
//!   **内存只留当前页**（页切换重新拉，kinds 表命中则只剩一次搜索）；
//! - 已装/有新版在渲染时实时派生；「已安装」不再用文字键表达（有卸载 icon 就够了）：
//!   未装 = 【安装】；有新版 = 【🗑】【更新】；已最新 = 【🗑】。**卸载永远在最左**；
//! - 动作列固定宽 104 → 跨行按钮严格对齐（不再随内容长度漂移）；
//! - 不再有安全提示条（用户定稿：别唠叨）。

use std::collections::{HashMap, HashSet};

use gpui::prelude::*;

use pi_link::market::pi_ext_market::{
    HomeSnapshot, MarketEntry, MarketKind, PAGE_SIZE, SNAPSHOT_MAX,
};

use crate::Chat;
use crate::ContentView;
use crate::PanelTab;
use crate::i18n::tr;
use crate::ui::TextInput;

mod card;
mod pager;
mod render;
mod states;
mod toolbar;

pub(crate) use render::page;

/// 每批补清单的条数：= 补全并发数，一批正好一轮请求（实测 8 条 ≈1.0-1.6s）。
const BATCH: usize = 8;
/// 最多可翻到第几页（npm 搜索分页深度保守上限；也是本次会话的内存天花板）。
const MAX_PAGES: usize = 20;
/// 列表 id（vlist 滚动句柄表按 id 常驻；换内容回顶也用它寻址）。
const LIST_ID: &str = "pi-ext-market-list";

/// 「未安装 / 已安装」筛选（040-1 UI 设计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ExtFilter {
    #[default]
    NotInstalled,
    Installed,
}

/// 排序（Downloads = npm 服务端序；Updated / Name = **当前页内**客户端排序，
/// npm registry search 无排序参数，跨页全局排序 v1 做不到，不装懂）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ExtSort {
    #[default]
    Downloads,
    Updated,
    Name,
}

/// Pi 扩展市场页状态（挂 `aux.market.pi_ext`）。
#[derive(Default)]
pub(crate) struct PiExtState {
    /// 排序（下拉选择，纯渲染期重排，0 网络）
    pub(crate) sort: ExtSort,
    /// 排序下拉展开态
    pub(crate) sort_open: bool,
    /// 排序下拉的收起防抖守卫（ensure_input 时建）
    pub(crate) sort_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    /// 搜索框（页面首次打开时创建；on_change 去抖后触发刷新）
    pub(crate) input: Option<gpui::Entity<TextInput>>,
    pub(crate) query: String,
    pub(crate) filter: ExtFilter,
    /// 当前页的条目（**只留当前页**，上限 PAGE_SIZE 条）
    pub(crate) entries: Vec<MarketEntry>,
    /// 会话内 kinds 表（name → 资源类型；不落盘）。翻回旧页时命中即 0 网络。
    pub(crate) kinds: HashMap<String, Vec<MarketKind>>,
    pub(crate) total: usize,
    /// 当前页（1 起）
    pub(crate) page: usize,
    /// 单飞闸：为真时不再发新页请求
    pub(crate) loading: bool,
    /// 当前 entries 来自首页快照、后台正在刷新（首批到达时整体替换）
    pub(crate) stale: bool,
    pub(crate) error: Option<String>,
    /// 网络失败但快照可用 → 顶部黄条
    pub(crate) offline: bool,
    /// 请求代际（换词/翻页/刷新 ++；回包不符即丢弃）
    pub(crate) gen_ix: u64,
    /// 行内操作中的包来源（`npm:<name>`）：转圈 + 全行按钮禁用
    pub(crate) pending: Option<String>,
    /// 已装载过（快照或网络），避免重复初始化
    pub(crate) loaded_once: bool,
}

impl PiExtState {
    /// 总页数：按服务端 total 折算，且不超过 [`MAX_PAGES`]。
    pub(crate) fn total_pages(&self) -> usize {
        let pages = self.total.div_ceil(PAGE_SIZE).max(1);
        pages.min(MAX_PAGES)
    }

    /// 翻页口径的唯一来源（渲染与点击处理共用）：返回 (当前页, 总页数)。
    ///
    /// 已安装页签 = 本地全量、不分页 → 固定 (1, 1)；若还用全局 total 折算，
    /// 点「下一页」会跳到一个不存在的第 2 页。
    pub(crate) fn pager(&self) -> (usize, usize) {
        if self.filter == ExtFilter::Installed {
            (1, 1)
        } else {
            (self.page, self.total_pages())
        }
    }
}

// ---------------------------------------------------------------------------
// 打开 / 装载
// ---------------------------------------------------------------------------

impl Chat {
    /// 打开「Pi扩展市场」标准页（菜单项、后续快捷键都走这里）。
    pub(crate) fn open_pi_ext_market(&mut self, cx: &mut gpui::Context<Self>) {
        // 入口总闸（dev-flags `pi_ext_market`，默认关）：关态下菜单钮不渲染，
        // 自动化动作也只是空转，页面永远打不开。
        if !crate::services::workspace::pi_ext_market_enabled() {
            return;
        }
        self.dlg.market_menu = None;
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::PiExtMarket))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(PanelTab::PiExtMarket);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(ContentView::PiExtMarket);
        self.pi_ext_ensure_input(cx);
        // 搜索词不存留：每次打开页面输入框都是干净的（用户定稿）
        self.aux.market.pi_ext.query.clear();
        if let Some(ix) = self.aux.market.pi_ext.input.as_ref() {
            ix.update(cx, |ti, cx| ti.set_value(String::new(), cx));
        }
        self.pi_ext_ensure_loaded(cx);
        // 已装条目的「有新版」依赖 082 的检测结果：还没跑过就顺手补一次
        if self.aux.pi_extension_updates.results.is_empty()
            && !self.aux.pi_extension_updates.checking
        {
            self.spawn_pi_extension_updates_check(cx);
        }
        cx.notify();
    }

    /// 搜索框懒创建：页面首次打开时建（TextInput 需要 `&mut Context`）。
    fn pi_ext_ensure_input(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.sort_guard.is_none() {
            self.aux.market.pi_ext.sort_guard = Some(cx.new(|_| crate::ui::DropdownState::new()));
        }
        if self.aux.market.pi_ext.input.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let input = cx.new(|cx| {
            let mut ti = TextInput::new(cx).placeholder(tr("market.search_placeholder"));
            // **打字不触发搜索**（npm 是海量远端数据，逐键打过去是灾难）；
            // 只有回车（= 输入框提交）才搜——页面上不再有「搜索」按钮。
            //
            // ⚠ 回调里**不能**回头 `input.read(cx)`：本回调就是 TextInput 自己
            // 在 update 中发出的，重复租会直接 panic「cannot read … while it
            // is already being updated」——就是「回车搜索程序崩掉」的原因。
            // 它已经把提交值传进来了，直接用。
            ti.set_on_submit(Box::new(move |v, cx| {
                let q = v.to_string();
                let _ = weak.update(cx, |c, cx| c.pi_ext_search_with(q, cx));
            }));
            ti
        });
        self.aux.market.pi_ext.input = Some(input);
    }

    /// 首次装载：读首页快照上屏 + 起一次后台刷新。已装载过则只补刷新。
    pub(crate) fn pi_ext_ensure_loaded(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.loaded_once {
            if self.aux.market.pi_ext.entries.is_empty() && !self.aux.market.pi_ext.loading {
                self.pi_ext_reload(cx);
            }
            return;
        }
        self.aux.market.pi_ext.loaded_once = true;
        // 快照只当「默认首页」用：带搜索词的快照既不上屏也不回填输入框
        // （否则重开应用还能看到上次的词——用户定稿：禁止留存）。
        if let Some(snap) = HomeSnapshot::load() {
            if snap.query.is_empty() {
                self.aux.market.pi_ext.entries = snap
                    .entries
                    .into_iter()
                    .filter(|e| e.is_extension())
                    .take(SNAPSHOT_MAX)
                    .collect();
                self.aux.market.pi_ext.stale = true;
            }
        }
        self.pi_ext_reload(cx);
    }

    /// 重新拉第一页（换词 / 手动刷新 / 重试共用）。
    pub(crate) fn pi_ext_reload(&mut self, cx: &mut gpui::Context<Self>) {
        crate::ui::vlist::reset_scroll(LIST_ID);
        self.aux.market.pi_ext.page = 1;
        self.pi_ext_fetch_page(1, cx);
    }

    /// 翻到指定页（页码条 / 上一页 / 下一页共用）。同页 = 无操作。
    pub(crate) fn pi_ext_goto(&mut self, page: usize, cx: &mut gpui::Context<Self>) {
        crate::ui::vlist::reset_scroll(LIST_ID);
        let page = page.max(1);
        if page == self.aux.market.pi_ext.page {
            return;
        }
        self.pi_ext_fetch_page(page, cx);
    }

    fn pi_ext_fetch_page(&mut self, page: usize, cx: &mut gpui::Context<Self>) {
        self.aux.market.pi_ext.page = page;
        self.aux.market.pi_ext.gen_ix += 1;
        self.aux.market.pi_ext.error = None;
        self.aux.market.pi_ext.offline = false;
        self.aux.market.pi_ext.loading = false;
        if page > 1 {
            // 翻页不留旧页内容（内存只留当前页）；快照只是首页，翻页时一并清掉
            self.aux.market.pi_ext.entries.clear();
            self.aux.market.pi_ext.stale = false;
        } else if !self.aux.market.pi_ext.entries.is_empty() {
            // 第 1 页重搜/刷新：现有内容按 stale 处理，首批到达时整体替换。
            // **不标 stale 的话 append 的「满 50 跳过」会把新结果全部丢掉**
            // ——「第二次起搜索永远不变」的根因（v18 真机反馈）
            self.aux.market.pi_ext.stale = true;
        }
        self.pi_ext_fetch(cx);
    }

    /// 显式搜索（回车）：值 → query → 回第一页。
    /// 打字**不**触发搜索（用户定稿：海量远端内容禁止逐键自动搜）。
    /// 用**已知词**搜（回车回调路径：值由 TextInput 传入，不碰实体）。
    pub(crate) fn pi_ext_search_with(&mut self, query: String, cx: &mut gpui::Context<Self>) {
        self.aux.market.pi_ext.query = query;
        self.pi_ext_reload(cx);
    }

    /// 筛选切换：纯客户端重算，**0 网络**。
    pub(crate) fn pi_ext_set_filter(&mut self, f: ExtFilter, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pi_ext.filter == f {
            return;
        }
        self.aux.market.pi_ext.filter = f;
        crate::ui::vlist::reset_scroll(LIST_ID);
        cx.notify();
    }

    /// 清错误 + 重试当前页。
    pub(crate) fn pi_ext_retry(&mut self, cx: &mut gpui::Context<Self>) {
        let page = self.aux.market.pi_ext.page;
        self.aux.market.pi_ext.error = None;
        self.aux.market.pi_ext.offline = false;
        self.pi_ext_fetch_page(page, cx);
    }
}

// ---------------------------------------------------------------------------
// 拉页（搜索 → 分批补清单 → 流式 append）
// ---------------------------------------------------------------------------

impl Chat {
    fn pi_ext_fetch(&mut self, cx: &mut gpui::Context<Self>) {
        // 不再因「loading 就 return」静默吞请求：那会让当前轮的 loading 永远不置位
        // （按钮全灰、下一页不可点）。轮次由 `gen_ix` 分步，旧回包自归废弃。
        self.aux.market.pi_ext.loading = true;
        self.aux.market.pi_ext.error = None;
        let gen_ix = self.aux.market.pi_ext.gen_ix;
        let query = self.aux.market.pi_ext.query.clone();
        let from = self.aux.market.pi_ext.page.saturating_sub(1) * PAGE_SIZE;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let searched = cx
                .background_executor()
                .spawn(
                    async move { pi_link::market::pi_ext_market::search(&query, from, PAGE_SIZE) },
                )
                .await;
            let page = match searched {
                Ok(p) => p,
                Err(e) => {
                    let _ = this.update(cx, |c, cx| c.pi_ext_fail(gen_ix, e, cx));
                    return;
                }
            };
            let total = page.total;
            // total 一到就上屏：摘要行与翻页条不必等 50 条清单补完（原来要等 7s+）
            let _ = this.update(cx, |c, cx| c.pi_ext_set_total(gen_ix, total, cx));
            let mut raw = page.entries;
            if raw.is_empty() {
                let _ = this.update(cx, |c, cx| c.pi_ext_finish(gen_ix, total, cx));
                return;
            }
            while !raw.is_empty() {
                let take = raw.len().min(BATCH);
                let batch: Vec<MarketEntry> = raw.drain(..take).collect();
                let filled = cx
                    .background_executor()
                    .spawn(async move {
                        let mut entries = batch;
                        let mut kinds = HashMap::new();
                        pi_link::market::pi_ext_market::fill_kinds(&mut entries, &mut kinds);
                        (entries, kinds)
                    })
                    .await;
                let ok = this
                    .update(cx, |c, cx| c.pi_ext_append(gen_ix, filled.0, filled.1, cx))
                    .is_ok();
                if !ok {
                    return; // 实体已亡（窗口关闭）：停
                }
            }
            let _ = this.update(cx, |c, cx| c.pi_ext_finish(gen_ix, total, cx));
        })
        .detach();
    }

    /// 一批清单结果上屏：合并 kinds → 过滤（只按「扩展」）→ append。
    /// 搜索回包一到就写 total（不等分批补清单）——摘要行/翻页条的显示前提。
    fn pi_ext_set_total(&mut self, gen_ix: u64, total: usize, cx: &mut gpui::Context<Self>) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        self.aux.market.pi_ext.total = total;
        cx.notify();
    }

    fn pi_ext_append(
        &mut self,
        gen_ix: u64,
        entries: Vec<MarketEntry>,
        kinds: HashMap<String, Vec<MarketKind>>,
        cx: &mut gpui::Context<Self>,
    ) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        // 快照内容 = 旧的第一页：首批到达时整体替换（不追加、不闪烁）
        if self.aux.market.pi_ext.stale {
            self.aux.market.pi_ext.entries.clear();
            self.aux.market.pi_ext.stale = false;
        }
        self.aux.market.pi_ext.kinds.extend(kinds);
        // 未安装/已安装不在这里过滤——那个状态随时会变，必须渲染时派生
        for e in entries {
            if !e.is_extension() {
                continue;
            }
            if self.aux.market.pi_ext.entries.len() >= PAGE_SIZE {
                break;
            }
            self.aux.market.pi_ext.entries.push(e);
        }
        cx.notify();
    }

    fn pi_ext_finish(&mut self, gen_ix: u64, total: usize, cx: &mut gpui::Context<Self>) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        self.aux.market.pi_ext.loading = false;
        self.aux.market.pi_ext.stale = false;
        self.aux.market.pi_ext.total = total;
        // 首页快照：第一页拉完覆盖写（唯一的磁盘缓存）。
        // **只存默认首页**（query 为空）：带词的结果不落盘，重开才不会
        // 把上次搜索的内容/词捞回来。
        if self.aux.market.pi_ext.page == 1 && self.aux.market.pi_ext.query.is_empty() {
            let snap = HomeSnapshot::new_now(
                &self.aux.market.pi_ext.query,
                self.aux
                    .market
                    .pi_ext
                    .entries
                    .iter()
                    .take(SNAPSHOT_MAX)
                    .cloned()
                    .collect(),
            );
            snap.save();
        }
        cx.notify();
    }

    fn pi_ext_fail(&mut self, gen_ix: u64, err: String, cx: &mut gpui::Context<Self>) {
        if gen_ix != self.aux.market.pi_ext.gen_ix {
            return;
        }
        self.aux.market.pi_ext.loading = false;
        // 有内容（快照或已拉到的一半）→ 黄条降级；完全没有 → 红条
        if self.aux.market.pi_ext.entries.is_empty() {
            self.aux.market.pi_ext.error = Some(err);
        } else {
            self.aux.market.pi_ext.offline = true;
        }
        cx.notify();
    }

    /// 已装来源集合（渲染与过滤共用；`cfg.mc_pkgs_global` 是 op 泵维护的活数据）。
    pub(crate) fn pi_ext_installed_sources(&self) -> HashSet<String> {
        self.cfg
            .mc_pkgs_global
            .iter()
            .map(pi_link::skills::entry_source)
            .filter(|s| !s.is_empty())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 行内动作（安装 / 更新 / 卸载）
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn pi_ext_install(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        let source = format!("npm:{name}");
        self.aux.market.pi_ext.pending = Some(source.clone());
        self.mc_install_package(source, cx);
    }

    pub(crate) fn pi_ext_update(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        let source = format!("npm:{name}");
        self.aux.market.pi_ext.pending = Some(source.clone());
        self.mc_update_package(source, cx);
    }

    pub(crate) fn pi_ext_remove(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        // 高危动作统一走 040 的确认浮层（cfg.pkg_confirm_remove）
        self.mc_ask_remove_package(format!("npm:{name}"), cx);
    }

    pub(crate) fn pi_ext_update_all(&mut self, cx: &mut gpui::Context<Self>) {
        self.mc_update_all_packages(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pager_uses_local_count_on_installed_tab() {
        let mut st = PiExtState::default();
        // 未安装：全局 total 折算，并截在 MAX_PAGES
        st.total = 12_365;
        st.page = 2;
        assert_eq!(st.pager(), (2, MAX_PAGES));
        // 已安装：本地全量、不分页 → 恒定 (1, 1)
        st.filter = ExtFilter::Installed;
        assert_eq!(st.pager(), (1, 1));
    }

    #[test]
    fn total_pages_caps_at_max() {
        let mut st = PiExtState::default();
        st.total = 12_365;
        assert_eq!(st.total_pages(), MAX_PAGES);
        st.total = PAGE_SIZE * 3;
        assert_eq!(st.total_pages(), 3);
        st.total = 0;
        assert_eq!(st.total_pages(), 1);
    }
}
