//! market 域入口（040-1-Pi的扩展市场.md）：后续会有多个市场页（Pi 扩展市场 /
//! PF 插件市场 / 模型市场…），**每个市场一个 `<来源>_market.rs` 并列在本模块下**，
//! 不另开顶层模块。
//!
//! - [`pi_ext_market`]：Pi 扩展市场（数据源 npm registry），标准页 + topbar 标签；
//! - [`pi_native_market`]：Pi 原生扩展市场（pi.dev Package Catalog 页面移植，040-2）；
//! - [`prefetch`]：启动【延迟加载】阶段的市场预热（只预取首页，写首页快照）。
//!
//! 状态挂载：`Chat.aux.market`（[`MarketUi`]），各市场在内部各占一个子状态；
//! 加市场 = 加字段 + 加一支 `ContentView`，`Chat` 的簇数量不变。

pub(crate) mod menu;
pub(crate) mod pi_ext_market;
pub(crate) mod pi_native_market;
pub(crate) mod prefetch;

/// 市场域状态簇（挂 `Chat.aux`）。
#[derive(Default)]
pub(crate) struct MarketUi {
    /// Pi 扩展市场页状态（040-1）
    pub(crate) pi_ext: pi_ext_market::PiExtState,
    /// Pi 原生扩展市场页状态（040-2）
    pub(crate) native: pi_native_market::NativeState,
}
