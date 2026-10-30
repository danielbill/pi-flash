//! 会话页模版（030）：桌面会话线与各「功能线」（插件注入线）**共用一份**
//! 页面骨架——操作栏 / 消息流 / 悬浮 composer / 新会话页。
//!
//! 为什么要有模版：线之间的差异只该是「注入」，不该是「复制一份页面」或
//! 「骨架里散点 `if is_feature_line`」。历史病灶正是后者——空会话判据、欢迎语、
//! 扩展更新卡、操作栏簇四处各判一次，任一处漏判就长出「有页面没身份」的
//! 孤儿页（2026-10-25 重启事故）。
//!
//! 模版注入面（[`SessionPageSkin`]）：
//! 1. [`SessionPageSkin::fn_bar`] —— 操作栏右侧簇（线的专属控件）；
//! 2. [`SessionPageSkin::keep_on_empty`] —— 空会话留在会话页还是让位给
//!    新会话页欢迎版式；
//! 3. [`SessionPageSkin::greeting_key`] / [`SessionPageSkin::plugin_updates`]
//!    —— 新会话页的欢迎语与扩展更新卡。
//!
//! 取值唯一出口 = [`crate::session::skin`]（按活跃会话所属线选皮肤）。

use gpui::AnyElement;

use crate::Chat;

/// 一条「线」在会话页模版上的全部差异。
#[derive(Clone, Copy)]
pub(crate) struct SessionPageSkin {
    /// 空会话时留在会话页（false = 让位给新会话页欢迎版式）
    pub keep_on_empty: bool,
    /// 新会话页欢迎语（i18n 键）
    pub greeting_key: &'static str,
    /// 新会话页是否挂 Pi 扩展更新卡（只属于真正的新会话页）
    pub plugin_updates: bool,
    /// 操作栏右侧注入簇；None = 不注入。
    ///
    /// 用函数指针而不是预生成的 `AnyElement`：簇里要读会话态、要挂 listener
    /// （`cx.entity().downgrade()`），只能在渲染当帧构造。
    pub fn_bar: Option<fn(&mut Chat, &mut gpui::Context<Chat>) -> AnyElement>,
}

/// 桌面会话线：空会话走欢迎页，欢迎语用默认文案，挂扩展更新卡，
/// 操作栏无注入。
pub(crate) static DESKTOP: SessionPageSkin = SessionPageSkin {
    keep_on_empty: false,
    greeting_key: "new_session.greeting",
    plugin_updates: true,
    fn_bar: None,
};
