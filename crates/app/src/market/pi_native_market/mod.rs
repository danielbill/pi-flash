//! Pi 原生扩展市场（040-2）——pi.dev `Package Catalog` 页的桌面移植。
//!
//! 设计稿：`docs/UI设计/仿-pi.dev-Packages-页面设计/pi-packages.html`
//! （同目录 `DESIGN-HANDOFF.md` 是导出工具给的视觉契约：**像素优先**）。
//! 规格：`docs/模块设计/040-2-Pi 扩展市场.md`。
//!
//! 与 040-1「Pi 扩展市场」的关系：**数据面同一个**（npm registry，见
//! [`pi_link::market::pi_ext_market`]），**界面与信息组织完全不同**——040-1 是
//! PF 自己设计的单列卡 + 工具栏；本页照抄 pi.dev 官网目录页：四列「最近发布」
//! 条 + 双列大卡（预览块 + 徽标 + 命令条）+ 页码条。
//!
//! 三条实现口径（改之前先读 `docs/agents/market.md`）：
//!
//! 1. **配色/尺寸全部字面量**：本页是「pi.dev 原生皮肤」，不跟随 PF 主题
//!    （设计稿 dark 单档）。真值全部住在 [`design`]，页面代码里不许出现色值。
//! 2. **安装 = 复制命令**（设计稿 `data-copy` 的语义，不是安装动作）：点「安装」
//!    把 `pi install npm:<pkg>` 写入剪贴板，按钮闪 1.2s「已复制」。**不加**二次
//!    确认、不查已装状态——设计稿里没有这两个概念。
//! 3. **打字只过滤当前页**（设计稿 JS 同款：0 网络、160ms 去抖）；**回车**才
//!    发服务端搜索（npm 上逐键搜是灾难）。两者共用同一个输入框，靠
//!    `query == filter` 判定「服务端已按同词搜过」，避免二次过滤把结果吃掉。

mod card;
mod controls;
pub(crate) mod design;

use std::time::Duration;

use gpui::prelude::*;
use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};

use pi_link::market::pi_ext_market::{MarketEntry, MarketKind, PAGE_SIZE};

use crate::Chat;
use crate::ContentView;
use crate::PanelTab;
use crate::i18n::tr;
use crate::ui::TextInput;

use design::{Metrics, px18, sp};

/// 「最近发布」条数（设计稿四列 × 两行）。
pub(crate) const RECENT_N: usize = 8;
/// 页码深度上限（与 040-1 同口径：npm 搜索分页不无限翻）。
const MAX_PAGES: usize = 20;
/// 打字过滤去抖（设计稿 JS 160ms）。
const FILTER_DEBOUNCE_MS: u64 = 160;

/// 类型筛选（设计稿 `<select name="type">` 的选项集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum NativeKind {
    #[default]
    All,
    Extension,
    Skill,
    Theme,
    Prompt,
}

impl NativeKind {
    const ALL: [NativeKind; 5] = [
        NativeKind::All,
        NativeKind::Extension,
        NativeKind::Skill,
        NativeKind::Theme,
        NativeKind::Prompt,
    ];

    /// 与 npm 清单 `kinds` 对齐的键（All = None = 不过滤）。
    fn key(self) -> Option<&'static str> {
        match self {
            NativeKind::All => None,
            NativeKind::Extension => Some("extension"),
            NativeKind::Skill => Some("skill"),
            NativeKind::Theme => Some("theme"),
            NativeKind::Prompt => Some("prompt"),
        }
    }

    /// 下拉与徽标共用文案（设计稿全小写，浏览器渲染由 `text-transform` 大写）。
    fn label(self) -> &'static str {
        match self {
            NativeKind::All => "All types",
            NativeKind::Extension => "extension",
            NativeKind::Skill => "skill",
            NativeKind::Theme => "theme",
            NativeKind::Prompt => "prompt",
        }
    }
}

/// 排序（设计稿 `<select name="sort">`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum NativeSort {
    /// npm 服务端序（权重 ≈ 下载量）
    #[default]
    Downloads,
    /// 按发布时间倒序（**当前页内**客户端排序）
    Recent,
    /// 名称 A-Z（当前页内）
    Name,
}

impl NativeSort {
    const ALL: [NativeSort; 3] = [NativeSort::Downloads, NativeSort::Recent, NativeSort::Name];

    fn label(self) -> &'static str {
        match self {
            NativeSort::Downloads => "Most downloads",
            NativeSort::Recent => "Recently published",
            NativeSort::Name => "A-Z",
        }
    }
}

/// 页面状态（挂 `aux.market.native`）。
pub(crate) struct NativeState {
    /// 搜索框（页面首次打开时创建）
    pub(crate) input: Option<gpui::Entity<TextInput>>,
    /// 服务端搜索词（回车/搜索钮）
    pub(crate) query: String,
    /// 页内实时过滤词（打字，0 网络）
    pub(crate) filter: String,
    pub(crate) kind: NativeKind,
    pub(crate) sort: NativeSort,
    /// 类型/排序两个下拉的收起守卫（共享一个实例，互斥由 open 标志各管）
    pub(crate) kind_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    pub(crate) sort_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    pub(crate) kind_open: bool,
    pub(crate) sort_open: bool,
    pub(crate) entries: Vec<MarketEntry>,
    pub(crate) recent: Vec<MarketEntry>,
    pub(crate) total: usize,
    pub(crate) page: usize,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    /// 请求代际（换词/翻页/重置 ++；回包不符即丢）
    pub(crate) gen_ix: u64,
    pub(crate) loaded_once: bool,
    /// 行内安装中的包来源（`npm:<name>`）：按钮转圈 + 全卡动作禁用
    pub(crate) pending: Option<String>,
    /// 过滤去抖代际（每键 ++；到点的旧轮次不再 notify）
    pub(crate) filter_gen: u64,
    /// kinds 后台补齐在途（fetch 落地置位、补齐/失败清除）：空窗期空态要显示
    /// 「类型信息加载中」而不是「无匹配」——否则选类型后 ~15s 里像坏了。
    pub(crate) kinds_pending: bool,
    /// 会话级 kinds 表（name → 资源类型；首载时从磁盘缓存灌入）：重查补齐
    /// 命中即 0 网络，选类型立即出结果。落盘 = `KindsCache`（跨会话）。
    pub(crate) kinds: std::collections::HashMap<String, Vec<MarketKind>>,
    /// 变高虚拟列表（整页分段：最近发布 / 头+工具栏 / 卡行×N / 页码条）
    pub(crate) list: gpui::ListState,
    /// 列表竖向滚动条状态
    pub(crate) sb: gpui_component::scroll::ScrollbarState,
}

impl Default for NativeState {
    fn default() -> Self {
        Self {
            input: None,
            query: String::new(),
            filter: String::new(),
            kind: NativeKind::All,
            sort: NativeSort::default(),
            kind_guard: None,
            sort_guard: None,
            kind_open: false,
            sort_open: false,
            entries: Vec::new(),
            recent: Vec::new(),
            total: 0,
            page: 1,
            loading: false,
            error: None,
            gen_ix: 0,
            loaded_once: false,
            pending: None,
            filter_gen: 0,
            kinds_pending: false,
            kinds: std::collections::HashMap::new(),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(2000.)),
            sb: gpui_component::scroll::ScrollbarState::default(),
        }
    }
}

impl NativeState {
    pub(crate) fn total_pages(&self) -> usize {
        self.total.div_ceil(PAGE_SIZE).max(1).min(MAX_PAGES)
    }

    /// 当前页可见条目（过滤 + 排序的唯一口径；count 文案也读它）。
    fn visible(&self) -> Vec<&MarketEntry> {
        // 服务端已按 `query` 搜过时不再二次过滤同词（否则 npm 按 keywords
        // 命中的条目会被描述里没这个词判掉）
        let needle = if self.filter.trim().is_empty() || self.query.trim() == self.filter.trim() {
            ""
        } else {
            self.filter.trim()
        };
        let needle_lc = needle.to_lowercase();
        let mut list: Vec<&MarketEntry> = self
            .entries
            .iter()
            .filter(|e| {
                if needle_lc.is_empty() {
                    return true;
                }
                let hay = format!(
                    "{} {} {}",
                    e.name,
                    e.description,
                    e.author.clone().unwrap_or_default()
                )
                .to_lowercase();
                hay.contains(&needle_lc)
            })
            .filter(|e| match self.kind.key() {
                None => true,
                Some(k) => e.kinds.iter().any(|x| x.key() == k),
            })
            .collect();
        match self.sort {
            NativeSort::Downloads => {}
            NativeSort::Recent => list.sort_by(|a, b| b.updated.cmp(&a.updated)),
            NativeSort::Name => {
                list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }
        }
        list
    }
}

/// 条目主类型（徽标文案；无清单 = package）。
pub(crate) fn primary_kind(e: &MarketEntry) -> &'static str {
    match e.kinds.first() {
        Some(k) => k.key(),
        None => "package",
    }
}

// ---------------------------------------------------------------------------
// 打开 / 装载 / 拉页
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

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
            return card::recent_block(&recent_src, &weak_c, loading, &m_c);
        }
        if ix == 1 {
            return card::index_head(
                controls::action_bar(bar.clone(), &weak_c, &m_c),
                shown,
                total,
                page_ix,
                filtered,
                &m_c,
            );
        }
        if ix == 1 + rows + 1 {
            return card::pager_block(
                controls::pager(&weak_c, page_ix, total_pages, &m_c, loading),
                &m_c,
            );
        }
        let r = ix - 2;
        let mut row = card::row_shell(&m_c);
        if visible.is_empty() {
            row = row.child(card::empty_line(empty_text, loading || kinds_loading));
        } else {
            let start = r * cols;
            for e in visible.iter().skip(start).take(cols) {
                let busy = pending.as_deref() == Some(e.source().as_str());
                let installed = installed_set.contains(&e.source());
                row = row.child(card::package_card(e, &weak_c, &m_c, busy, installed));
            }
            if visible.len().saturating_sub(start) < cols {
                row = row.child(gpui::div().flex_1());
            }
        }
        card::row_slice(row, &m_c)
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

/// 工具栏的入参快照（列表项闭包要 `'static`，状态得先拷成独立值）。
#[derive(Clone)]
pub(crate) struct BarModel {
    input: Option<gpui::Entity<TextInput>>,
    kind_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    sort_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    kind: NativeKind,
    sort: NativeSort,
    kind_open: bool,
    sort_open: bool,
}

impl BarModel {
    fn of(st: &NativeState) -> Self {
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

/// 千分位（`12,365`）：设计稿结果计数用 `5,573` 这种写法。
pub(crate) fn thousands(n: usize) -> String {
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

/// 页面文案：设计稿里的英文原串（官网同款，**不进 i18n 表**——这是移植稿的
/// 「原文照抄」部分；只有 安装/已复制 走 i18n，因为设计稿自己就是中文）。
pub(crate) fn label(s: &str) -> SharedString {
    SharedString::from(s.to_string())
}

/// 页内「install」按钮文案（busy 时改「…」由 card 侧处理）。
pub(crate) fn install_label() -> SharedString {
    SharedString::from(tr("market.install").to_string())
}

/// 类型/排序下拉的一行（设计稿 select 选项：全小写 + 大写化由渲染做）。
pub(crate) fn dropdown_row(
    id: String,
    text: &str,
    selected: bool,
    on_click: impl Fn(&mut gpui::App) + 'static,
) -> AnyElement {
    let mut row = gpui::div()
        .id(SharedString::from(id))
        // ui-raw: ok pi.dev select 选项 py7 逐值复刻（§10.8 登记）
        .w_full()
        .px(gpui::px(sp::CONTROL_PAD_X))
        .py(gpui::px(7.))
        .flex()
        .items_center()
        .cursor_pointer()
        .text_size(gpui::px(px18::META_FS))
        .font_family(design::MONO)
        .text_color(design::text())
        .hover(|s| s.bg(design::control_bg_hover()))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| on_click(cx))
        .child(SharedString::from(text.to_string()));
    if selected {
        row = row.bg(design::control_bg_hover());
    }
    row.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_link::market::pi_ext_market::MarketKind;

    fn entry(name: &str, desc: &str, kinds: Vec<MarketKind>, updated: &str) -> MarketEntry {
        MarketEntry {
            name: name.to_string(),
            description: desc.to_string(),
            kinds,
            updated: updated.to_string(),
            ..Default::default()
        }
    }

    fn state(entries: Vec<MarketEntry>) -> NativeState {
        NativeState {
            entries,
            ..Default::default()
        }
    }

    #[test]
    fn filter_matches_name_desc_author() {
        let mut a = entry("pi-alpha", "does things", vec![], "2026-01-01");
        a.author = Some("alice".into());
        let st = state(vec![a, entry("beta", "other", vec![], "2026-01-02")]);
        assert_eq!(st.visible().len(), 2);
        let mut st = st;
        st.filter = "alpha".into();
        assert_eq!(st.visible().len(), 1);
        st.filter = "alice".into();
        assert_eq!(st.visible().len(), 1);
        st.filter = "zzz".into();
        assert!(st.visible().is_empty());
    }

    #[test]
    fn server_query_is_not_reapplied_locally() {
        // 服务端已按同词搜过 → 描述里没有该词也不许判掉
        let st = {
            let mut s = state(vec![entry("mcp-thing", "no match word", vec![], "x")]);
            s.query = "mcp".into();
            s.filter = "mcp".into();
            s
        };
        assert_eq!(st.visible().len(), 1);
        // 词不同则本地过滤照常生效
        let st = {
            let mut s = state(vec![entry("mcp-thing", "no match word", vec![], "x")]);
            s.query = "mcp".into();
            s.filter = "brandnew".into();
            s
        };
        assert!(st.visible().is_empty());
    }

    #[test]
    fn kind_filter_uses_manifest_kinds() {
        let st = {
            let mut s = state(vec![
                entry("a", "", vec![MarketKind::Extension], "x"),
                entry("b", "", vec![MarketKind::Skill], "x"),
                entry("c", "", vec![], "x"),
            ]);
            s.kind = NativeKind::Extension;
            s
        };
        let names: Vec<String> = st.visible().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["a".to_string()]);
    }

    #[test]
    fn sort_by_name_and_recent() {
        let mut st = state(vec![
            entry("b", "", vec![], "2026-01-02"),
            entry("a", "", vec![], "2026-01-03"),
        ]);
        st.sort = NativeSort::Name;
        assert_eq!(st.visible()[0].name, "a");
        st.sort = NativeSort::Recent;
        assert_eq!(st.visible()[0].name, "a");
    }

    #[test]
    fn total_pages_caps() {
        let mut st = NativeState::default();
        st.total = PAGE_SIZE * 3;
        assert_eq!(st.total_pages(), 3);
        st.total = 0;
        assert_eq!(st.total_pages(), 1);
        st.total = PAGE_SIZE * 100;
        assert_eq!(st.total_pages(), MAX_PAGES);
    }
}
