//! tooltip 统一入口（UI组件规范 §5.10「悬浮提示」唯一代码落点）。
//!
//! 应用内一切悬停提示（icon 钮、文字、任意可交互元素）一律经由此模块
//! 挂载，禁止直接摸 `gpui_component::tooltip::Tooltip`——弹出机制、提示
//! 卡样式、文案出口（i18n）都在这里收口，改一处全局生效。
//!
//! 机制：gpui 原生 tooltip 手势（`StatefulInteractiveElement::tooltip`，
//! 悬停约 300ms 自动弹出、移开即收），应用层不写 hover 状态机；提示卡
//! 视图 = gpui-component `Tooltip` 默认盘。
//!
//! 用法（tooltip 手势只挂 stateful 元素，`.id()` 必须先行）：
//!
//! ```ignore
//! div().id("psp-new-session")
//!     .with_tip("psp.new_session")   // i18n 键，显示期过 tr 随语言切换
//!     ...
//! ```
//!
//! 会话列表 psp 的项目路径悬浮条/会话详情卡是自绘 overlay（psp_overlays.rs，
//! 「详情卡」语义：可交互、可改名、有宽限期），不是 tooltip，不走这里。

use gpui::{AnyView, App, SharedString, Window, div, prelude::*, px, relative, rgb};
use gpui_component::text::Text;

/// 构建提示卡视图（gpui-component Tooltip 默认盘）。样式收口在此。
pub fn tip_view(text: impl Into<Text>, window: &mut Window, cx: &mut App) -> AnyView {
    gpui_component::tooltip::Tooltip::new(text).build(window, cx)
}

/// 同 `tip_view`，另带键位 chip（`Kbd` 右挂，`with_tip_action` 专用）。
fn tip_view_kbd(
    text: impl Into<Text>,
    kbd: Option<gpui_component::Kbd>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    gpui_component::tooltip::Tooltip::new(text)
        .key_binding(kbd)
        .build(window, cx)
}

/// 给可交互元素挂统一 tooltip。
pub trait WithTip: StatefulInteractiveElement + Sized {
    /// i18n 键：每次显示期过 `i18n::tr`，随语言切换。
    fn with_tip(self, key: &'static str) -> Self {
        // 键翻译留在闭包内（每次弹出重译），语言切换后无需重建元素
        self.tooltip(move |window, cx| tip_view(crate::i18n::tr(key), window, cx))
    }

    /// 动态文案（快捷键拼接等）：直排，不走 i18n。
    // 统一入口的另一半，供动态文案调用点使用；首个调用点落地即摘
    #[allow(dead_code)]
    fn with_tip_text(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        // tooltip 闭包是 Fn（每次弹出重建视图），SharedString 廉价克隆
        self.tooltip(move |window, cx| tip_view(text.clone(), window, cx))
    }

    /// 错误详情浮层（2026-11-07 用户定稿）：**固定 350px 宽、高 100-300px
    /// （min_h/max_h 夹制）、超高滚动**，多行文本 pre-wrap。默认盘（
    /// `with_tip_text`）自适应宽高装不下详情——多行错误走这里。
    fn with_tip_detail(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        self.tooltip(move |window, cx| {
            let text = text.clone();
            gpui_component::tooltip::Tooltip::element(move |_, _| {
                let t = crate::appearance::theme::theme();
                div()
                    .id("tip-detail-body")
                    .w(px(350.))
                    .min_h(px(100.))
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .p(px(10.))
                    .text_size(crate::appearance::ui_size(11.))
                    .line_height(relative(1.5))
                    .text_color(rgb(t.text))
                    .child(SharedString::from(text.clone()))
            })
            .build(window, cx)
        })
    }

    /// i18n 键 + 动作键位 chip（截图样式「文字 + Ctrl+X 灰底键帽」）。
    /// 键位每次弹出查 keymap（`binding_for_action`）——绑定改了 chip 自动
    /// 跟随，唯一事实源是 keymap；动作没绑键则只显文字不显 chip。
    fn with_tip_action(self, key: &'static str, action: &dyn gpui::Action) -> Self {
        let action = action.boxed_clone();
        self.tooltip(move |window, cx| {
            let kbd = gpui_component::Kbd::binding_for_action(action.as_ref(), None, window);
            tip_view_kbd(crate::i18n::tr(key), kbd, window, cx)
        })
    }
}

impl<E: StatefulInteractiveElement> WithTip for E {}
