//! 012 新会话页 (newSession)：启动时项目|会话列表为空、或用户新建会话
//! 时，聊天区显示这一页（pi-web ChatWindow `isEmptyNew` parity）——空会话
//! 不再只是「消息列为空 + 底部悬浮 composer」，而是整页的
//! 「标题 + 背景 logo + inputpanel + 额外操作栏」。
//!
//! 布局（docs/模块设计/012-新会话页.md）：
//! 1. 标题「让我们做点什么！」水平居中，落在 inputpanel 上方；
//! 2. app logo 放到 6×，用主题淡色作背景图：水平居中、垂直上移 10%
//!    （中线落在屏高 40%）；
//! 3. inputpanel 不垂直居中、下移 10%（内容簇中线落在屏高 60%）；
//! 4. inputpanel 下方一条额外操作栏（004 指定：新会话页的【打开项目】）。
//!
//! 「上/下移 10%」用两段 80% 高的带子实现：上带顶对齐 + 带内居中 ⇒ 中线
//! 0.4H；下带底对齐 + 带内居中 ⇒ 中线 0.6H。百分比高度由 flex 链上的确定
//! 高度解析（与 033 导航条 `h(relative(0.75))` 同机制）。
//!
//! 触发判据在 `session::main_column`（messages 空且 agent 未跑）；composer
//! 以 hero 模式（`input::input_area(.., hero=true)`）在正常流里排版，宽度
//! 仍由消息列约束（px(15) + max_w 920）。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::i18n::tr;
use crate::session::input;
use crate::theme::theme as T;
use crate::ui::icon;

/// 背景 logo = 6×（基准取侧栏/空态徽章的 32px 方框）。
const LOGO_SCALE: f32 = 6.;
/// logo 基准尺寸（与 032 空态徽章同款 32px）。
const LOGO_BASE: f32 = 32.;
/// 上/下偏移 10% 的实现带高：80% 高 + 带内居中 = 中线 40% / 60%。
const BAND: f32 = 0.8;
/// 操作栏高度（胶囊控件行 28 + 上下 5 呼吸）。
const ACTION_BAR_H: f32 = 38.;

/// 新会话页整页。占满聊天区（`flex_1`），内部两层：背景 logo（绝对、装饰）
/// 与内容簇（绝对、可交互）。
pub(crate) fn page(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
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
        .child(logo_backdrop(t))
        .child(
            // 内容簇带：底对齐 80% 高 ⇒ 簇中线 60%（inputpanel 下移 10%）
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h(relative(BAND))
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
                        .child(heading(t))
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

/// 背景 logo：6× 主题淡色（text_faint），水平居中；顶对齐 80% 高带内居中
/// ⇒ 中线 40%（向上偏移 10%）。纯装饰层：无 id/无 handler，不参与命中。
fn logo_backdrop(t: &'static crate::theme::Theme) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(relative(BAND))
        .flex()
        .flex_col()
        .justify_center()
        .items_center()
        .child(icon("logo-marks", LOGO_BASE * LOGO_SCALE, t.text_faint))
        .into_any_element()
}

/// 标题「让我们做点什么！」——codex 参考图里 "What should we work on?" 的位置。
fn heading(t: &'static crate::theme::Theme) -> AnyElement {
    div()
        .pb(px(26.))
        .text_size(crate::appearance::ui_size(30.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text))
        .child(SharedString::from(tr("让我们做点什么！")))
        .into_any_element()
}

/// inputpanel 下方的额外操作栏（012 第 4 条）。左 = 【打开项目】+ 当前项目名
/// （004：新会话页的打开项目入口在这里；点击走 psp 同款目录选择器），
/// 右 = 会话搜索（013 弹窗）。
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
        .unwrap_or_else(|| tr("打开项目").to_string())
        .into();
    let w_open = weak.clone();
    let w_search = weak.clone();
    div()
        .id("new-session-bar")
        .mt(px(8.))
        .w_full()
        .h(px(ACTION_BAR_H))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
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
                    let _ = w_open.update(cx, |c, cx| c.pick_project_folder(cx));
                })
                .child(icon("icon-project", 16., t.text_muted))
                .child(project),
        )
        // 会话搜索
        .child(
            div()
                .id("ns-session-search")
                .ml_auto()
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_search.update(cx, |c, cx| c.open_session_search(cx));
                })
                .child(icon("search", 16., t.text_muted)),
        )
        .into_any_element()
}
