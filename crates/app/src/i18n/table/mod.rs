//! i18n 翻译表（数据表，**唯一真值落点**）——按 key 前缀分域，本文件只聚合不落数据。
//!
//! 行内四列：`(key, zh-CN, zh-TW, en)`；**繁/英留空 = 尚未翻译**，此时 tr()
//! 回落 zh-CN（与迁移前一致）；漏译清单由 src/i18n_audit.rs 盯着（只减不增）。
//!
//! 新增文案：找对 key 前缀域的分表文件，一行一条追加；表行繁/英空着会被挂进
//! BACKLOG，补齐后从 BACKLOG 删掉。新增分表：mod 声明 + PARTS 追加一份即可，
//! audit 按目录自动收录（锚点 = 各分表文件里的数据常量声明；本文件刻意不含
//! 该字样，防误命中）。
//!
//! 各分表文件整体 `#[rustfmt::skip]`（audit 按行解析：行 trim 后以 '(' 起 +
//! 切 4 列，rustfmt 折宽行会破坏解析）；本文件不含数据行，不参与 skip。

mod agents;
mod base;
mod editor;
mod extensions;
mod market;
mod models;
mod session;
mod settings;

/// 各分表聚合（tr() 平铺线性查找）。命名**刻意避开** audit 的解析锚点——
/// 误命中会把聚合视图当数据行重复计数。
pub(super) const PARTS: &[&[(&str, &str, &str, &str)]] = &[
    base::TABLE,
    session::TABLE,
    market::TABLE,
    models::TABLE,
    settings::TABLE,
    editor::TABLE,
    agents::TABLE,
    extensions::TABLE,
];

/// 关于页信息块：这两个常量现在是**键**，文案在分表同一行（改文案不必动
/// 键）。about.rs 直接 `tr(ABOUT_*)`。
pub const ABOUT_DESC: &str = "about.tagline";
pub const ABOUT_NOTE: &str = "about.note";
