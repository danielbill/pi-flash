//! PF 插件市场页（090-4 v3）——**只对齐 pi market 页的 layout**：
//! 外壳几何（`design::{Metrics,sp,px18}` 的壳内距/左右对称留白/列宽/分段骨架/
//! 双列网格/行距/gap）+ 结构分段（头段「标题+计数」→ 双列卡行 → 页码条），
//! **无「最近发布」段**、**无工具栏**（2026-10-23 用户定夺）。
//!
//! 三条红线：
//!
//! 1. **卡片内部保持 PF 原有设计**（v1 定稿，不得擅自发挥）：72×72 预览槽 ·
//!    名称 16 semibold · 描述（text_dim）· meta 行（发布人 · 发布时间 ···· [安装]）。
//!    不加 pi 的类型徽标 / 命令条 / 版本·PF 下限行 / 网格纹理预览框。
//! 2. **PF market 没有搜索**：不出输入框、不出搜索钮、不出筛选/排序/重置，
//!    也不出控件上方的字段标签行（原网站那行标签也是不可见的）。
//! 3. **上边距与左右留白对称**：pi 页的首段（「最近发布」）自己带
//!    `pt(shell_pad)`；本页没有那一段，故首段（头段）自己补同样的上内距。
//!
//! theme 全部 PF 语义 token（`theme()`），不跟 pi 页的深色字面量皮肤：
//!
//! ```text
//! pi design            → PF token
//! line（段内细线）      = theme.border @70%
//! muted（计数/图标）     = theme.text_faint
//! muted_strong（页码字）= theme.text_dim
//! panel（段盒底）        = theme.bg_panel
//! panel_soft（控件底）   = theme.tool_bg
//! accent / text         = 同名 token
//! ```
//!
//! 数据面：registry 公域扫描（`pi_link::market::pf_scan`，**无内置索引**，
//! 分发规范 §2）+ 台账 `plugins.json`；单页全量；「安装」是**真安装**
//! （`pf_plugins::install`）。

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, IntoElement, MouseButton, SharedString, Styled, px, rgb};

use pi_link::pf_plugins::{self, IndexEntry};

use crate::Chat;
use crate::appearance::theme::{Theme, border_alpha, rgba_a};
use crate::i18n::{tf, tr};
use crate::market::pi_native_market::design::{self as pnd, Metrics, px18, sp};
use crate::market::pi_native_market::thousands;

/// mono 字体（与 040-2 design::MONO 同值；该常量在私有 mod 里不可引）。
pub(crate) const MONO: &str = "JetBrains Mono";

// ---------------------------------------------------------------------------
// 色板映射
// ---------------------------------------------------------------------------

fn line(t: &Theme) -> gpui::Rgba {
    gpui::rgba(border_alpha(t, 0xB4))
}

pub(crate) fn muted(t: &Theme) -> gpui::Rgba {
    gpui::rgb(t.text_faint)
}

fn accent_alpha(t: &Theme, a: f32) -> gpui::Rgba {
    gpui::rgba(rgba_a(t.accent, a))
}

/// 大写短标签的字距模拟（pi 页 `design::spaced` 同款 hair space）。
fn spaced_label(s: &str) -> SharedString {
    SharedString::from(pnd::spaced(&s.to_uppercase(), false))
}

// ---------------------------------------------------------------------------
// 公域扫描索引（单例）
// ---------------------------------------------------------------------------

/// 市场索引（分发规范 §2：**无内置索引**）。数据源 = registry 清单包
/// `@pi-flash-plugins/market`（插件仓每次发版自动更新），扫描链见
/// pi_link::market::pf_scan。拉不到 = 空清单（stderr 告警），无兜底。
static PF_INDEX: std::sync::RwLock<Option<pi_link::pf_plugins::MarketIndex>> =
    std::sync::RwLock::new(None);

fn index() -> pi_link::pf_plugins::MarketIndex {
    if let Some(ix) = PF_INDEX.read().unwrap().as_ref() {
        return ix.clone();
    }
    *PF_INDEX.write().unwrap() = Some(Default::default());
    std::thread::spawn(|| {
        match pi_link::market::pf_scan::fetch() {
            Ok(ix) => {
                eprintln!("[pf-market] 公域扫描完成（{} 包）", ix.packages.len());
                *PF_INDEX.write().unwrap() = Some(ix);
            }
            Err(e) => eprintln!("[pf-market] 公域扫描失败：{e}"),
        }
    });
    Default::default()
}

fn installed_in(ledger: &[pi_link::pf_plugins::LedgerEntry], name: &str) -> bool {
    ledger.iter().any(|l| l.name == name)
}

/// 索引快照（pub 口子给 pf_installed；整表克隆一次后本地连查）。
pub(crate) fn index_snapshot() -> pi_link::pf_plugins::MarketIndex {
    index()
}

// ---------------------------------------------------------------------------
// 状态（挂 Chat.aux.market.pf）
// ---------------------------------------------------------------------------

/// 页内页签（090-4 v4）：所有插件 = 市场卡列表；已安装 = 管理视图
/// （[`super::pf_installed`]，版式对齐 settings-扩展）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PfTab {
    #[default]
    All,
    Installed,
}

/// 页面状态。台账在打开页面时从磁盘重读（唯一真相在文件，内存不缓存写）。
pub struct PfState {
    /// 页内页签（所有插件 / 已安装）
    pub tab: PfTab,
    /// 已安装页签选中的插件名（None = 首个）
    pub section: Option<String>,
    /// 安装进行中的包名（该卡按钮转 spinner，其余动作忽略）
    pub pending: Option<String>,
    /// 卸载进行中的插件名（详情垃圾桶转 spinner）
    pub removing: Option<String>,
    /// 卸载确认浮层挂起的插件名（040 同款机制）
    pub confirm_remove: Option<String>,
    /// 最近一次失败原因（显示在头段下方，下一次动作自动清除）
    pub error: Option<String>,
    /// 台账镜像（open 时重读；安装完成后重读）
    pub ledger: Vec<pi_link::pf_plugins::LedgerEntry>,
    /// 变高虚拟列表（分段：头段 / 卡行×N / 页码条）
    pub list: gpui::ListState,
    /// 列表竖向滚动条状态
    pub sb: gpui_component::scroll::ScrollbarState,
}

impl Default for PfState {
    fn default() -> Self {
        Self {
            tab: PfTab::All,
            section: None,
            pending: None,
            removing: None,
            confirm_remove: None,
            error: None,
            ledger: Vec::new(),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(2000.)),
            sb: gpui_component::scroll::ScrollbarState::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// 打开 / 动作
// ---------------------------------------------------------------------------

impl Chat {
    /// 打开「Pi-Flash 插件」标准页（菜单项是唯一入口）。
    pub(crate) fn open_pf_market(&mut self, cx: &mut gpui::Context<Self>) {
        self.dlg.market_menu = None;
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::PfMarket))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(crate::PanelTab::PfMarket);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(crate::ContentView::PfMarket);
        self.aux.market.pf.ledger = pf_plugins::load_ledger();
        // 公域扫描（分发规范 §2）：后台拉取，完成换表 + 重绘；失败保持空清单
        let scan = cx
            .background_executor()
            .spawn(async { pi_link::market::pf_scan::fetch() });
        cx.spawn(async move |this, cx| {
            let scanned = scan.await;
            let _ = this.update(cx, |c, cx| {
                if let Ok(ix) = scanned {
                    *PF_INDEX.write().unwrap() = Some(ix);
                }
                c.aux.market.pf.ledger = pf_plugins::load_ledger();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 页内页签切换（所有插件 ⇄ 已安装）。进出都重读台账：安装/卸载的
    /// 唯一真相在 plugins.json，切换是零成本的同步点；选中项复位（列表可能已变）。
    pub(crate) fn pf_switch_tab(&mut self, tab: PfTab, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.tab == tab {
            return;
        }
        let pf = &mut self.aux.market.pf;
        pf.tab = tab;
        pf.section = None;
        pf.ledger = pf_plugins::load_ledger();
        cx.notify();
    }

    /// 安装。后台线程跑 npm pack → sha512 校验 → 解包落盘 → 台账。
    fn pf_install(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.pending.is_some() {
            return;
        }
        let Some(entry) = index().entry(&name).cloned() else {
            return; // 索引外：理论上不可达（按钮只按索引渲染）
        };
        // engines.pf 下限（semver；app 已有依赖，pi-link 不引）
        if !entry.min_pf.is_empty() {
            let cur = semver::Version::parse(env!("CARGO_PKG_VERSION")).ok();
            let req = semver::VersionReq::parse(&entry.min_pf).ok();
            if let (Some(cur), Some(req)) = (cur, req) {
                if !req.matches(&cur) {
                    self.aux.market.pf.error = Some(tf(
                        "market.pf.need_newer_pf",
                        &[
                            ("min", entry.min_pf.clone()),
                            ("cur", env!("CARGO_PKG_VERSION").to_string()),
                        ],
                    ));
                    cx.notify();
                    return;
                }
            }
        }
        self.aux.market.pf.pending = Some(name.clone());
        self.aux.market.pf.error = None;
        let pkg = name.clone(); // 装完成功后要按包名落运行态
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { pf_plugins::install(&entry) })
                .await;
            let _ = this.update(cx, |c, cx| {
                c.aux.market.pf.pending = None;
                match result {
                    Ok(_) => {
                        c.aux.market.pf.error = None;
                        // 装完立即落运行态（台账=安装真相，扫描只认台账；不靠重启）
                        c.reload_plugin_by_pkg(pkg.clone(), cx);
                        // 首启不等编译：装完立刻后台预热编译产物（pf_plugin 编译缓存）
                        crate::pf_plugin::spawn_precompile_installed();
                    }
                    Err(e) => c.aux.market.pf.error = Some(e),
                }
                c.aux.market.pf.ledger = pf_plugins::load_ledger();
                cx.notify();
            });
        })
        .detach();
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

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

    // 【已安装】页签 = 管理视图（pf_installed，版式对齐 settings-扩展）
    if st.tab == PfTab::Installed {
        return super::pf_installed::page(chat, weak, &m);
    }

    // 卡片数据（闭包要 'static，故整表 own 一份）
    let entries: Vec<IndexEntry> = index().packages.clone();
    let cols = if m.narrow { 1 } else { 2 };
    let rows = entries.len().div_ceil(cols).max(1);
    let items = 1 + rows + 1;
    if st.list.item_count() != items {
        // 项高只跟项数走（卡定高、头段定高）→ 只按项数判断 reset，否则丢滚动位
        st.list.reset(items);
    }

    let total = entries.len();
    let error = st.error.clone();
    let pending = st.pending.clone();
    let ledger = st.ledger.clone();
    let tab = st.tab;
    let installed_total = st.ledger.len();
    let weak_c = weak.clone();
    let m_c = m;
    let list = gpui::list(st.list.clone(), move |ix, _w, _cx| {
        let t = crate::appearance::theme::theme();
        if ix == 0 {
            return head_segment(
                error.clone(),
                total,
                tab,
                installed_total,
                &weak_c,
                &m_c,
                t,
            );
        }
        if ix == 1 + rows {
            return pager_block(&m_c, t);
        }
        let r = ix - 1;
        let mut row = row_shell(&m_c);
        if entries.is_empty() {
            row = row.child(empty_line(tr("market.pf.empty"), t));
        } else {
            let start = r * cols;
            for e in entries.iter().skip(start).take(cols) {
                let busy = pending.as_deref() == Some(e.name.as_str());
                row = row.child(card(&weak_c, e, &m_c, t, busy, &ledger));
            }
            if entries.len().saturating_sub(start) < cols {
                row = row.child(gpui::div().flex_1());
            }
        }
        row_slice(row, &m_c, t)
    })
    .flex_1()
    .min_h_0();

    let mut root = gpui::div()
        .id("pf-market-list")
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(list);

    // 竖向滚动条（变高 ListState：与 pi market 页同挂法）
    if st.list.max_offset_for_scrollbar().height > px(0.) {
        let handle = crate::ui::list_handle::ListStateHandle(st.list.clone());
        root = root.child(
            gpui::div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(2.))
                .w(px(8.))
                .child(gpui_component::scroll::Scrollbar::vertical(&st.sb, &handle)),
        );
    }
    root.into_any_element()
}

// ---------------------------------------------------------------------------
// 分段骨架（pi 页 card.rs 同构：一只盒子拆成段，各自画侧边描边）
// ---------------------------------------------------------------------------

/// 居中外壳：定宽段盒子显式左右留白（列表项里 `mx_auto` 不可靠，pi 页同款）。
pub(crate) fn centered(inner: gpui::Div, m: &Metrics) -> gpui::Div {
    gpui::div()
        .pl(px(m.side_pad()))
        .pr(px(m.side_pad()))
        .child(inner)
}

/// 段外壳：两侧描边 + 面板底；`top/bottom` = 首/末段补边。
pub(crate) fn slice_shell(m: &Metrics, top: bool, bottom: bool, t: &Theme) -> gpui::Div {
    let mut boxed = gpui::div()
        // 定宽（`w_full()` 在变高列表项里拿不到父宽，会整段塌掉——pi 页同坑）
        .w(px(m.shell_inner_w()))
        .flex()
        .flex_col()
        .bg(rgb(t.bg_panel))
        .border_l_1()
        .border_r_1()
        .border_color(line(t));
    if top {
        boxed = boxed.border_t_1();
    }
    if bottom {
        boxed = boxed.border_b_1();
    }
    boxed
}

/// 卡片头（标题件 + 可选右端件）。pi 页 `.content-card-header` 同构。
fn card_header(
    title: AnyElement,
    right: Option<AnyElement>,
    m: &Metrics,
    t: &'static Theme,
) -> AnyElement {
    gpui::div()
        .flex()
        .items_baseline()
        .justify_between()
        .gap(px(sp::BLOCK_GAP))
        .px(px(m.card_pad_head))
        .py(px(sp::HEADER_PAD_Y))
        .border_b_1()
        .border_color(line(t))
        .child(title)
        .children(right)
        .into_any_element()
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
    total: usize,
    tab: PfTab,
    installed_total: usize,
    weak: &gpui::WeakEntity<Chat>,
    m: &Metrics,
    t: &'static Theme,
) -> AnyElement {
    let count_text = match tab {
        // 市场页签沿用 pi 页的「页内条目 / 总数」口径；已安装无分页，只报总数
        PfTab::All => format!("1-{} / {}", total, thousands(total)),
        PfTab::Installed => thousands(installed_total),
    };
    let count = gpui::div()
        .font_family(MONO)
        .text_size(px(px18::COUNT_FS))
        .text_color(muted(t))
        .child(SharedString::from(count_text));
    let err_el = err.map(|e| {
        gpui::div()
            .px(px(m.card_pad_body))
            .pb(px(sp::BODY_GAP))
            .text_size(px(11.))
            .text_color(rgb(t.danger))
            .child(e)
    });
    centered(
        slice_shell(m, true, false, t)
            .child(card_header(tab_row(tab, weak, t), Some(count.into_any_element()), m, t))
            .children(err_el),
        m,
    )
    .pt(px(m.shell_pad))
    .into_any_element()
}

/// 卡行：左右两道内距 + 上内距（pi 页由工具栏容器给 `pt`，本页没有工具栏，
/// 这一份挪到卡行段自己身上）+ 行间距。
fn row_shell(m: &Metrics) -> gpui::Div {
    gpui::div()
        .w_full()
        .flex()
        .gap(px(sp::BLOCK_GAP))
        .px(px(m.card_pad_body))
        .pt(px(m.card_pad_body))
        .pb(px(sp::BLOCK_GAP))
}

/// 卡行 → 外壳（两侧描边 + 居中）。
fn row_slice(row: gpui::Div, m: &Metrics, t: &'static Theme) -> AnyElement {
    centered(slice_shell(m, false, false, t).child(row), m).into_any_element()
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

/// ③ 页码段（末尾带上外壳 4rem 底留白）。PF 本地索引恒单页。
fn pager_block(m: &Metrics, t: &'static Theme) -> AnyElement {
    centered(
        slice_shell(m, false, true, t).child(
            gpui::div()
                .px(px(m.card_pad_body))
                .pt(px(sp::BODY_GAP))
                .pb(px(m.card_pad_body))
                .child(pager(1, 1, t)),
        ),
        m,
    )
    .pb(px(sp::SHELL_PAD_BOTTOM))
    .into_any_element()
}

// ---------------------------------------------------------------------------
// 卡片：**PF v1 定稿内部设计**（不得擅自发挥）
// ---------------------------------------------------------------------------

fn card(
    weak: &gpui::WeakEntity<Chat>,
    e: &IndexEntry,
    m: &Metrics,
    t: &'static Theme,
    busy: bool,
    ledger: &[pi_link::pf_plugins::LedgerEntry],
) -> AnyElement {
    let installed = installed_in(ledger, &e.name);
    // 窄屏（<768）：与 pi 页同口径转单列，卡宽吃满行
    let width = if m.narrow {
        gpui::div().w_full()
    } else {
        gpui::div().flex_1().min_w_0()
    };

    // ── 预览槽：72×72 顶部对齐 · 底色跟卡片（t.bg，不抢眼）· 无边框 ──
    // icon 用 hint 档（text_faint）= 非行动信息，不抢标题/安装钮的注意力
    let preview = gpui::div()
        .size(px(72.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(t.bg))
        .rounded(px(8.))
        .text_color(rgb(t.text_faint))
        .child(crate::ui::icon("package", 28., t.text_faint));

    // ── 名称（16 semibold —— 源码层级：标题明显大于内容）──
    let name = gpui::div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(16.))
        .child(e.display.zh.clone());

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
                .child(if e.publisher.is_empty() {
                    "Pi-Flash".to_string()
                } else {
                    e.publisher.clone()
                }),
        )
        .child(
            gpui::div()
                .text_size(px(11.))
                .text_color(rgb(t.text_faint))
                .child(e.published_at.clone()),
        )
        .child(gpui::div().flex_1())
        .child(install_btn(weak, e, installed, busy, t));

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
/// 已装 = ✓ 绿（accent，不可点）。只有这一个动作。
fn install_btn(
    weak: &gpui::WeakEntity<Chat>,
    e: &IndexEntry,
    installed: bool,
    busy: bool,
    t: &'static Theme,
) -> AnyElement {
    // 已装：✓ 绿（accent，不可点）
    if installed {
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
        gpui::div()
            .id(SharedString::from(format!("pf-market-install-{}", e.name)))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let name = name.clone();
                let _ = weak_c.update(cx, |c, cx| c.pf_install(name, cx));
            })
            .child(bracket("[", t))
            .child(tr("market.btn.install"))
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

// ---------------------------------------------------------------------------
// 页码条（pi 页 controls.rs 同构；PF 本地索引恒单页）
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum PageBtn {
    Num(usize),
    Gap,
}

/// 页码窗口（纯函数）：≤7 页全显示；否则 首/末 + 当前前后各 1。
fn page_window(page: usize, total: usize) -> Vec<PageBtn> {
    if total <= 7 {
        return (1..=total).map(PageBtn::Num).collect();
    }
    let mut out = vec![PageBtn::Num(1)];
    let start = page.saturating_sub(1).max(2);
    let end = (page + 1).min(total - 1);
    if start > 2 {
        out.push(PageBtn::Gap);
    }
    for p in start..=end.max(start) {
        out.push(PageBtn::Num(p));
    }
    if end < total - 1 {
        out.push(PageBtn::Gap);
    }
    out.push(PageBtn::Num(total));
    out
}

fn pager(page: usize, total_pages: usize, t: &'static Theme) -> AnyElement {
    let mut pages = gpui::div().flex().items_center().gap(px(6.3)).flex_wrap();
    for b in page_window(page, total_pages) {
        match b {
            PageBtn::Gap => {
                pages = pages.child(
                    gpui::div()
                        .min_w(px(sp::PAGER_CELL))
                        .h(px(sp::PAGER_CELL))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(muted(t))
                        .font_family(MONO)
                        .text_size(px(px18::PAGER_FS))
                        .child(SharedString::from("…")),
                )
            }
            PageBtn::Num(p) => {
                let active = p == page;
                let mut cell = gpui::div()
                    .id(SharedString::from(format!("pf-market-page-{p}")))
                    .min_w(px(sp::PAGER_CELL))
                    .h(px(sp::PAGER_CELL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(sp::PAGER_PAD_X))
                    .border_1()
                    .font_family(MONO)
                    .text_size(px(px18::PAGER_FS))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(p.to_string()));
                cell = if active {
                    // 当前页格子：底色与卡片一致（t.bg），不铺 hover 洗底
                    cell.border_color(accent_alpha(t, 0.42))
                        .bg(rgb(t.bg))
                        .text_color(rgb(t.text))
                } else {
                    cell.border_color(line(t))
                };
                pages = pages.child(cell);
            }
        }
    }

    let prev = nav_button("pf-market-prev", tr("market.prev"), false, t);
    let next = nav_button("pf-market-next", tr("market.next"), false, t);

    gpui::div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(sp::PAGER_GAP))
        .pt(px(sp::PAGER_PT))
        .border_t_1()
        .border_color(line(t))
        .child(prev)
        .child(pages)
        .child(next)
        .into_any_element()
}

/// ← / → 大格（单页恒禁用）。
fn nav_button(
    id: &'static str,
    text: &'static str,
    enabled: bool,
    t: &'static Theme,
) -> AnyElement {
    let base = gpui::div()
        .id(id)
        .min_w(px(sp::PAGER_CELL_MIN_W))
        .h(px(sp::PAGER_CELL))
        .flex()
        .items_center()
        .justify_center()
        .px(px(sp::PAGER_PAD_X))
        .border_1()
        .border_color(line(t))
        .font_family(MONO)
        .text_size(px(px18::PAGER_FS))
        .child(text);
    if enabled {
        base.text_color(rgb(t.text_dim)).into_any_element()
    } else {
        base.text_color(gpui::rgba(rgba_a(t.text_faint, 0.55)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_short_lists_fully() {
        assert_eq!(
            page_window(1, 3),
            vec![PageBtn::Num(1), PageBtn::Num(2), PageBtn::Num(3)]
        );
        assert_eq!(
            page_window(1, 7),
            (1..=7).map(PageBtn::Num).collect::<Vec<_>>()
        );
    }

    #[test]
    fn window_clamps_edges() {
        let w = page_window(1, 20);
        assert_eq!(w[0], PageBtn::Num(1));
        assert_eq!(w[1], PageBtn::Num(2));
        assert_eq!(w[2], PageBtn::Gap);
        assert_eq!(w.last(), Some(&PageBtn::Num(20)));
    }
}
