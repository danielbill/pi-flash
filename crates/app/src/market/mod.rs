//! market 域入口：多个市场页（Pi Dev 扩展社区 / PF 插件市场…），
//! **每个市场一个子模块并列在本模块下**，不另开顶层模块。
//!
//! - [`pi_native_market`]：Pi 原生扩展市场（pi.dev Package Catalog 页面移植，
//!   040-2；数据层复用 pi-link `market::pi_ext_market` 的 npm registry 客户端）；
//! - [`pf_market`] + [`pf_installed`]：PF 插件市场页（090-4）+ 其【已安装】
//!   管理页签（版式对齐 settings-扩展）。
//!
//! 历史：040-1「Pi 扩展市场」标准页已退役（2026-10-22 定夺不上菜单；
//! 2026-11-06 连代码一并删除），`open_pi_ext_market` / `prefetch` 随之清除。
//!
//! 状态挂载：`Chat.aux.market`（[`MarketUi`]），各市场在内部各占一个子状态；
//! 加市场 = 加字段 + 加一支 `ContentView`，`Chat` 的簇数量不变。

pub(crate) mod menu;
pub(crate) mod pf_installed;
pub(crate) mod pf_market;
pub(crate) mod pi_native_market;
pub(crate) mod shell;

/// 市场域状态簇（挂 `Chat.aux`）。
#[derive(Default)]
pub(crate) struct MarketUi {
    /// Pi 原生扩展市场页状态（040-2）
    pub(crate) native: pi_native_market::NativeState,
    /// PF 插件市场页状态（090-4）
    pub(crate) pf: pf_market::PfState,
}
