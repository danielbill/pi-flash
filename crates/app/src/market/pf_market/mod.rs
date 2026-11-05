//! PF 插件市场页（090-4 v3）——**只对齐 pi market 页的 layout**：
//! 外壳几何（`design::{Metrics,sp,px18}` 的壳内距/左右对称留白/列宽/分段骨架/
//! 双列网格/行距/gap）+ 结构分段（头段「标题+计数」→ 双列卡行 → 页码条），
//! **无「最近发布」段**、**无工具栏**（2026-10-23 用户定夺）。
//!
//! 文件组织：公域扫描索引单例 + 页状态 + 动作在本文件；渲染（骨架/头段/
//! 页签行/页主体）在 [`render`]、卡片在 [`card`]、页码条在 [`pager`]。
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

use gpui::SharedString;

use pi_link::pf_plugins;

use crate::Chat;
use crate::appearance::theme::{Theme, border_alpha, rgba_a};
use crate::i18n::tf;
use crate::market::pi_native_market::design as pnd;

mod card;
mod pager;
mod render;

pub(crate) use render::{centered, page, slice_shell};

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

pub(crate) fn accent_alpha(t: &Theme, a: f32) -> gpui::Rgba {
    gpui::rgba(rgba_a(t.accent, a))
}

/// 大写短标签的字距模拟（pi 页 `design::spaced` 同款 hair space）。
pub(crate) fn spaced_label(s: &str) -> SharedString {
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
    std::thread::spawn(|| match pi_link::market::pf_scan::fetch() {
        Ok(ix) => {
            eprintln!("[pf-market] 公域扫描完成（{} 包）", ix.packages.len());
            *PF_INDEX.write().unwrap() = Some(ix);
        }
        Err(e) => eprintln!("[pf-market] 公域扫描失败：{e}"),
    });
    Default::default()
}

pub(crate) fn installed_in(ledger: &[pi_link::pf_plugins::LedgerEntry], name: &str) -> bool {
    ledger.iter().any(|l| l.name == name)
}

/// 已装版本是否落后于索引版本（090-10 更新判定）。两边都是 npm 语义版本，
/// 能解析就逐段比较；任一解析失败回落字符串不等（覆盖安装无害，宁误报
/// 不漏报）。索引版本 ≤ 已装版本（含降级）不算更新。
pub(crate) fn pf_updatable(installed: &str, index: &str) -> bool {
    if installed.is_empty() || index.is_empty() {
        return false;
    }
    match (
        semver::Version::parse(installed),
        semver::Version::parse(index),
    ) {
        (Ok(a), Ok(b)) => b > a,
        _ => installed != index,
    }
}

/// 台账里某包的已装版本（未装 = None）。
pub(crate) fn ledger_version_of<'a>(
    ledger: &'a [pi_link::pf_plugins::LedgerEntry],
    name: &str,
) -> Option<&'a str> {
    ledger
        .iter()
        .find(|l| l.name == name)
        .map(|l| l.version.as_str())
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
    /// 公域扫描进行中（进页先转圈，扫完才落空态/卡片）
    pub loading: bool,
    /// 最近一次失败原因（显示在头段下方，下一次动作自动清除）
    pub error: Option<String>,
    /// 台账镜像（open 时重读；安装完成后重读）
    pub ledger: Vec<pi_link::pf_plugins::LedgerEntry>,
    /// 失效包（090-10 迁移体验）：台账启用、清单被本版拒载（旧 schema 等）。
    /// 启动扫描写入；更新/启用重载成功、卸载时按包名移除。驱动市场头部
    /// 常驻通知行 + 已安装页标签——toast 只播一次，错过了这里还看得到。
    pub stale: Vec<crate::pf_plugin::StalePlugin>,
}

impl Default for PfState {
    fn default() -> Self {
        Self {
            tab: PfTab::All,
            section: None,
            pending: None,
            removing: None,
            confirm_remove: None,
            loading: false,
            error: None,
            ledger: Vec::new(),
            stale: Vec::new(),
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
        // 公域扫描（分发规范 §2）：后台拉取，完成换表 + 重绘；失败保持空清单。
        // 进页先置 loading：页面正中转圈「扫描 Pi-Flash 插件……」，扫完才落空态/卡片
        self.aux.market.pf.loading = true;
        let scan = cx
            .background_executor()
            .spawn(async { pi_link::market::pf_scan::fetch() });
        cx.spawn(async move |this, cx| {
            let scanned = scan.await;
            let _ = this.update(cx, |c, cx| {
                if let Ok(ix) = scanned {
                    *PF_INDEX.write().unwrap() = Some(ix);
                }
                c.aux.market.pf.loading = false;
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
    ///
    /// `reattach`：更新链路传 true——装完重载后把包内 pi 扩展重挂回当前
    /// 会话（更新前 pf_update 已摘除）；全新安装传 false（无旧挂载要补）。
    fn pf_install(&mut self, name: String, reattach: bool, cx: &mut gpui::Context<Self>) {
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
                        c.reload_plugin_by_pkg(pkg.clone(), reattach, cx);
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

    /// 更新（090-10）：①停（evict 自带 unload + purge 会话清单；再摘活跃
    /// 进程 ext——顺带释放 cwasm 的 mmap，否则 Windows 覆盖目录必 os error 32）
    /// → ②覆盖（`pf_plugins::install` 本就是覆盖语义）→ ③重载（pf_install
    /// 完成 Ok 分支的 reload + reattach）。
    pub(crate) fn pf_update(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.pending.is_some() {
            return;
        }
        if let Some(ix) = self.plugin_ix_by_pkg(&name) {
            self.plugin_evict(ix);
            self.plugin_detach_active_ext(ix, cx);
        }
        self.pf_install(name, true, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::pf_updatable;

    #[test]
    fn updatable_semver_compare() {
        // 逐段数值比较：1.10 > 1.9（字符串比较会错判）
        assert!(pf_updatable("1.9.0", "1.10.0"));
        assert!(pf_updatable("1.0.0", "2.0.0"));
        // 预发布号：同版本号下 rc < 正式
        assert!(pf_updatable("1.2.0-rc.1", "1.2.0"));
        // 相等 / 降级 / 索引更旧：不出更新
        assert!(!pf_updatable("1.2.0", "1.2.0"));
        assert!(!pf_updatable("2.0.0", "1.9.9"));
    }

    #[test]
    fn updatable_fallback_and_empty() {
        // 非 semver 回落字符串不等（宁误报不漏报，覆盖安装无害）
        assert!(pf_updatable("dev", "1.0.0"));
        assert!(!pf_updatable("1.0.0", "1.0.0"));
        // 空版本（dev 侧载无版本号 / 索引缺失）不出更新
        assert!(!pf_updatable("", "1.0.0"));
        assert!(!pf_updatable("1.0.0", ""));
        assert!(!pf_updatable("", ""));
    }
}
