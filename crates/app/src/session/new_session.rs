//! 012 新会话页 (newSession)：启动时项目|会话列表为空、或用户新建会话
//! 时，聊天区显示这一页（pi-web ChatWindow `isEmptyNew` parity）——空会话
//! 不再只是「消息列为空 + 底部悬浮 composer」，而是整页的
//! 「标题 + 背景 logo + inputpanel + 额外操作栏」。
//!
//! 布局（docs/模块设计/012-新会话页.md，v5 按 zcode 参考截图实测比例）：
//! 1. 背景 logo = 水印方案：墨迹高 51% 聊天区、中线 32%（跨度 ~6%..57%），
//!    text 色低 alpha（深 ~10%/浅 ~6%），下沿被 composer 遮住；
//! 2. 欢迎语「Hi，打算让我做点什么？」中心 37%（压墨迹下半部，zcode 同款
//!    关系），加大字号、斜体、内置 JetBrains Mono；
//! 3. inputpanel + 操作栏簇中心 56%（zcode 实测 ~56%）。
//!
//! 三条绝对定位带实现（百分比高由 flex 链解析，033 同机制）：logo 带
//! 顶对齐 64% 高带内居中 ⇒ 中线 32%；欢迎语带顶对齐 74% ⇒ 中心 37%；
//! 内容簇带底对齐 88% ⇒ 中心 56%。带间重叠无害（无 bg/无 handler 的带
//! 不注册 hitbox，v1 双带已验证）。
//!
//! 触发判据在 `session::main_column`（messages 空且 agent 未跑）；composer
//! 以 hero 模式（`input::input_area(.., hero=true)`）在正常流里排版，宽度
//! 仍由消息列约束（px(15) + max_w 920）。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::i18n::tr;
use crate::session::input;
use crate::theme::theme as T;
use crate::ui::{icon, icon_alpha};

/// 背景 logo「墨迹」目标高 = 51% 聊天区高（zcode 参考实测：Z 墨迹跨度
/// 6.4%..57.3%）。logo-marks.svg 墨迹只占 43.75% 盒高，盒子按视口高补偿
/// （聊天区 ≈ 92% 视口 ⇒ 0.51×0.92/0.4375 ≈ 1.07）。
const LOGO_INK_RATIO: f32 = 112. / 256.;
const LOGO_SCREEN: f32 = 0.32 / LOGO_INK_RATIO;
/// logo 带：顶对齐 64% 高带内居中 ⇒ 墨迹中线 32% 聊天区高。
const LOGO_BAND: f32 = 0.5;
/// 欢迎语带：顶对齐 74% 高带内居中 ⇒ 欢迎语中心 37%（压墨迹下半部）。
const HEADING_BAND: f32 = 0.74;
/// 内容簇带（inputpanel + 操作栏）：底对齐 88% ⇒ 簇中心 56%。
const CONTENT_BAND: f32 = 0.88;
/// 操作栏高度（012：固定 40px）。
const ACTION_BAR_H: f32 = 40.;

/// 新会话页整页。占满聊天区（`flex_1`），内部两层：背景 logo（绝对、装饰）
/// 与内容簇（绝对、可交互）。
pub(crate) fn page(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
    viewport_h: f32,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let t = T();
    div()
        .id("new-session")
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_hidden()
        .child(logo_backdrop(t, viewport_h * LOGO_SCREEN))
        .child(
            // 欢迎语带：顶对齐 74% 高带内居中 ⇒ 中心 37%（压墨迹下半部）
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(relative(HEADING_BAND))
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(heading(t)),
        )
        .child(
            // 内容簇带：底对齐 88% 高带内居中 ⇒ 簇中心 56%
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h(relative(CONTENT_BAND))
                .px(px(15.))
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(
                    // 与消息列同宽（session_list 的 px(15) + max_w 920）
                    div()
                        .w_full()
                        .max_w(px(920.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(input::input_area(
                            chat,
                            weak,
                            streaming,
                            input_focused,
                            true,
                            cx,
                        ))
                        .child(action_bar(chat, weak, t)),
                ),
        )
        .into_any_element()
}

/// 背景 logo：盒子 ~1.07×视口高（墨迹达 51% 聊天区高，见 LOGO_SCREEN），
/// 顶对齐 64% 高带内居中 ⇒ 墨迹中线 32%，下沿没人接（composer 簇 56% 居中
/// 会盖住 45%+ 以下的部分）。text 色低 alpha（深 ~10%、浅 ~6%）。纯装饰层：
/// 无 id/无 handler，不参与命中。
fn logo_backdrop(t: &'static crate::theme::Theme, size: f32) -> AnyElement {
    let a = if t.dark { 0x1a } else { 0x10 };
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(relative(LOGO_BAND))
        .flex()
        .flex_col()
        .justify_center()
        .items_center()
        .child(icon_alpha("logo-marks", size, (t.text << 8) | a))
        .into_any_element()
}

/// 欢迎语「Hi，打算让我做点什么？」——zcode 参考图问候语同款关系（中心
/// 37%，压水印下半部）：加大字号、斜体、内置 JetBrains Mono（中文回退
/// 系统字体，italic 由回退链合成）。
fn heading(t: &'static crate::theme::Theme) -> AnyElement {
    div()
        .text_size(crate::appearance::ui_size(40.))
        .font_family(crate::markdown::MONO_FAMILY)
        .italic()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text))
        .child(SharedString::from(tr("Hi，打算让我做点什么？")))
        .into_any_element()
}

/// inputpanel 下方的额外操作栏（012 第 4 条）：与 inputpanel 同宽、间隔
/// 5px、高 40px、无边框。左 = 目录图标 + 当前项目名（无项目时显示
/// 「选择项目」；004：点击进入打开项目菜单，菜单里的【打开文件夹】才是
/// 目录选择器），右 = pi-flash 版本号小字（012：不显示 pi 版本号，搜索
/// 入口只在功能面板）。
fn action_bar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
) -> AnyElement {
    let project: SharedString = chat
        .cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| tr("选择项目").to_string())
        .into();
    let w_open = weak.clone();
    div()
        .id("new-session-bar")
        .mt(px(5.))
        .w_full()
        .h(px(ACTION_BAR_H))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        // 打开项目
        .child(
            div()
                .id("ns-open-project")
                .h(px(28.))
                .px(px(9.))
                .flex()
                .items_center()
                .gap(px(7.))
                .rounded(px(8.))
                .cursor_pointer()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_open.update(cx, |c, cx| c.open_project_picker(cx));
                })
                .child(icon("folder", 16., t.text_muted))
                .child(project),
        )
        // pi-flash 版本号（placeholder 同款淡色，用户 2026-10-07 定稿）
        .child(
            div()
                .ml_auto()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(format!("v{}", env!("CARGO_PKG_VERSION")))),
        )
        .into_any_element()
}
