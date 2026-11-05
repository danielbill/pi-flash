//! psp 行的槽位与占位行（itlq.12 自 psp_row_views.rs 切出，纯结构搬移）：
//! 会话行状态槽（15px：转圈/彩点/悬停 pin 钮）、置顶组头行、伪标签行
//! （draft 已发送、pi 未落盘的占位）。主行渲染在 psp_row_views。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::WithTip;
use crate::ui::tokens::align;
use crate::ui::{icon, spark_spinner};

/// 会话行状态槽（15px）：常态 = 转圈/彩点/空；悬停换成置顶钮（020 会话
/// 置顶 ①；MD 档中性色）。**任何行都不常显 pin**——置顶组行也不例外
/// （2026-10-18 二改：常显 pin 白占状态槽，转圈/彩点看不到）。悬停态图标
/// 随置顶态翻转：未置顶 = pin（置顶）、已置顶 = pin-off（取消置顶）。
/// pin 钮只在可见时存在（tracked hover 态门控），不可见的钮不拦截点击
/// ——状态槽那 15px 未悬停时要留给以后可能的槽位交互。
pub(crate) fn session_status_slot(
    chat: &Chat,
    path: &std::path::Path,
    running: bool,
    awaiting: bool,
    hovered: bool,
    pinned: bool,
    weak: gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    div()
        .relative()
        .size(px(15.))
        // 状态与 pin 互斥渲染（pin 是描边 icon，叠加会从缝隙露出
        // 底下的转圈/彩点）：未悬停才画状态
        .when(!hovered, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if awaiting {
                        // 橙 = 后台会话阻塞请求等你回答（#问题卡不弹）：
                        // agent 其实挂着在等 select，转圈是骗人的——此态
                        // 优先级最高，盖过转圈/错误/未读
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(crate::appearance::theme::ATTENTION))
                            .into_any_element()
                    } else if running {
                        spark_spinner(15., t.accent)
                    } else if chat.psp.turn_errors.contains(path) {
                        // 红 = 外部错误/崩溃（t.danger，§6 danger 系唯一表达）
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(t.danger))
                            .into_any_element()
                    } else if chat.psp.turn_warnings.contains(path) {
                        // 黄 = 内部中断（上限截断/中止）
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(crate::appearance::theme::WARN))
                            .into_any_element()
                    } else if chat.psp.unread.contains(path) {
                        // 青蓝 = 完成通知（绿色退出通知语义，仅表达正确性）
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(crate::appearance::theme::NOTICE))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
        })
        .when(hovered, |d| {
            // 闭包要求 'static：先转 owned 再 move 进闭包（move 捕获 &Path
            // 引用本身，to_path_buf 写在闭包体内救不了）
            let path_pin = path.to_path_buf();
            d.child(
                div()
                    .id("pin-toggle")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .with_tip(if pinned { "psp.unpin" } else { "psp.pin" })
                    // 拦住冒泡：点 pin 不是点行（不开会话）
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = weak.update(cx, |c, cx| {
                            c.session_pin_toggle(path_pin.clone(), cx);
                        });
                    })
                    .child(icon(
                        if pinned { "pin-off" } else { "pin" },
                        crate::ui::tokens::icon::MD,
                        t.text_dim,
                    )),
            )
        })
        .into_any_element()
}

/// 置顶组头行（020 会话置顶 ③）：folder-bookmark icon（MD 档；中性
/// text_dim——pin/置顶不用主题色，不抢注意力，2026-10-18 用户定夺）
/// +「置顶」；样式对齐项目行（同高同字号同对齐线）。零交互：不收折/
/// 不拖拽/无右键卡——置顶是全局单组，组序恒在最顶，头行没有可操作面。
pub(crate) fn pinned_header_row(t: &'static Theme) -> gpui::AnyElement {
    div()
        .id("psp-pinned-header")
        .h(px(crate::ui::tokens::row::MAIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(align::ROW_PL))
        .pr(px(align::LIST_PX))
        .rounded(px(8.))
        .child(icon(
            "folder-bookmark",
            crate::ui::tokens::icon::MD,
            t.text_dim,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(t.text_dim))
                .child(tr("psp.pinned_group")),
        )
        .into_any_element()
}

/// 伪标签行（draft 已发送、pi 未落盘的占位）：行高/字号/选中与悬停底色
/// 同 psp_row_views::session_row_view（ZCode TaskListItem 样式），左侧状态
/// 槽 = spinner（此刻进程正在 boot/跑，与真标签的 running 态同视觉），标题
/// = 首条消息首行（真标签落地后的 preview 与它一致，衔接无跳变）。没有文
/// 件就没有 hover 卡 / 改名 / 删除——点击只做 switch_to。FileBound 落地
/// 后本行被真标签顶替。`shift` = 所属项目块的避让位移（020 拖拽）。
pub(crate) fn pending_row_view(
    chat: &Chat,
    key: &str,
    shift: f32,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(entry) = chat.psp.pending_tabs.iter().find(|e| e.key == key) else {
        return div().into_any_element();
    };
    let is_active = chat.pool.active_key == entry.key;
    let title: SharedString = if entry.preview.is_empty() {
        tr("session.new").to_string().into()
    } else {
        entry.preview.clone().into()
    };
    let row_sel = gpui::rgba((t.text << 8) | 0x1a); // 10%（与 session 行同色）
    let panel_family = crate::appearance::panel_font().family;
    let key_click = entry.key.clone();
    let w_click = weak.clone();
    div()
        // 与 session 行同理：滚动容器 overflow 压缩时行不许收缩
        .flex_shrink_0()
        // 所属项目块的避让平移（相对偏移，不动布局）
        .relative()
        .top(px(shift))
        .child(
            div()
                .id(SharedString::from(format!("ps-draft-{key}")))
                // bg 层绝对定位锚 + hover 组（与 session 行同款）
                .relative()
                .group("psrow")
                .w_full()
                .h(px(crate::ui::tokens::row::MAIN))
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(align::ROW_PL))
                .pr(px(align::LIST_PX))
                .rounded(px(8.))
                .cursor_pointer()
                // 色条内缩 1px 同 session 行（相邻色条间留隙）
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(1.))
                        .bottom(px(1.))
                        .rounded(px(8.))
                        .when(is_active, |d| d.bg(row_sel))
                        .when(!is_active, |d| d.group_hover("psrow", |s| s.bg(row_sel))),
                )
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let key = key_click.clone();
                    let _ = w_click.update(cx, |c, cx| {
                        if let Some(rt) = c.pool.runtimes.get(&key).cloned() {
                            c.switch_pool_rt(rt, cx);
                        }
                    });
                })
                .child(
                    // 状态槽（15px，与会话行同位）：伪标签期间轮次必在飞
                    div()
                        .size(px(15.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(spark_spinner(15., t.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(panel_family)
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text))
                        .child(title),
                )
                // 时间区占位：与 session 行同宽，标题截断位对齐
                .child(div().flex_shrink_0().w(px(38.))),
        )
        .into_any_element()
}
