//! 消息操作栏（用户消息的复制/编辑、agent 轮的「新分支」），从
//! `messages.rs` 拆出以守住单文件行数上限（ARCHITECTURE.md §6）。
//!
//! 「新分支」挂在 **agent 回复** 的操作栏上（用户定案）：分支保留到本轮回复
//! 为止，用户消息不会被丢弃——一切从 clone 的地方开始。详见 `fork::ForkAnchor`。

use gpui::prelude::*;
use gpui::{MouseButton, SharedString, div, px, rgb};

use crate::i18n::tr;
use crate::theme;
use crate::ui::icon;
use crate::Chat;
use crate::session::fork::ForkAnchor;

/// 用户消息下的操作栏（复制 / 编辑 + 时间戳）。
///
/// pi-web UserMessageView `.msg-actions` parity：
/// - act = 图标 12 + 文字，gap 4，11.5px；hover 变主题色
/// - 显影 = `hovered`（状态驱动：行 on_hover → Chat.bar_hover）
/// - pi-web 的「新会话」按钮在用户消息上（branch before = 丢弃这条用户消息），
///   本项目按用户定案移到 agent 轮操作栏（见 `fork_pill`）
pub(crate) fn user_action_bar(
    msg_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    text: &str,
    ts: Option<i64>,
    bar_revealed: bool,
    t: &theme::Theme,
) -> gpui::Div {
    let weak_copy = weak.clone();
    let weak_edit = weak.clone();
    let copy_text = text.to_string();
    let edit_text = text.to_string();
    // 操作栏 act（主界面UI设计-2.html .msg-actions）：图标 12 + 文字
    // gap 4。图标用 icon()（工具卡同款 gpui::svg+显式色，唯一被证明
    // 在列表内稳定渲染的路径；svg 上的 group_hover 会让 copy.svg 丢失）
    let action = |id: String, icon_name: &'static str, label: SharedString, busy: bool| {
        // busy = fork 在飞（pi-web forking 态）：主题色 + not-allowed + 无 hover
        let fg = if busy { t.accent } else { t.text_dim };
        let mut b = div()
            .id(SharedString::from(id))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_color(rgb(fg))
            .child(icon(icon_name, 12., fg))
            .child(label);
        if busy {
            b = b.cursor_not_allowed();
        } else {
            b = b.cursor_pointer().hover(|s| s.text_color(rgb(t.text)));
        }
        b
    };

    // 栏字号 11.5（ui_size）
    let mut actions = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .text_size(crate::appearance::ui_size(11.5));
    // 设计稿无「已复制」反馈态：点击即写剪贴板，栏不变
    let copy_pill = action(
        format!("copy-{msg_ix}"),
        "copy",
        SharedString::from(tr("复制")),
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
        let text = copy_text.clone();
        let _ = weak_copy.update(cx, |_c, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        });
    });
    actions = actions.child(copy_pill);
    actions = actions.child(
        action(
            format!("edit-{msg_ix}"),
            "pencil",
            SharedString::from(tr("编辑")),
            false,
        )
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let _ = weak_edit.update(cx, |c, cx| {
                    c.with_active_editor(cx, |r, _| r.input = edit_text.clone());
                    let focus = c.focus.clone();
                    window.focus(&focus);
                });
            }),
    );
    // 操作栏（主界面UI设计-2.html .msg-actions）：acts + .when 时间戳
    // 都在栏内；显影 = 状态驱动（row on_hover → Chat.bar_hover，
    // pi-web hovered parity），不依赖 group_hover hitbox
    let mut actions_wrap = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .opacity(if bar_revealed { 1. } else { 0. })
        .child(actions);
    if let Some(ts) = ts {
        actions_wrap = actions_wrap.child(
            div()
                .ml(px(4.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(crate::services::format::fmt_msg_time(ts))),
        );
    }
    div()
        .flex()
        .items_center()
        .justify_end()
        .mt(px(6.))
        .pr(px(4.))
        .child(actions_wrap)
}


/// agent 轮操作栏（主界面UI设计-2.html `.as-stats`）：新分支 / 复制 +
/// 用时 / 时间戳。
///
/// gap 14、字号 11.5；「新分支」与复制是 act（图标 12 + gap 4，hover 提亮），
/// 用时与时间戳是浅一档的普通 span。显影 = 状态驱动（col on_hover →
/// Chat.bar_hover），`forking` 期间常显（pi-web `hovered || forking`）。
/// 设计稿无计费/用量 → 不渲染 usage 行（用户定案 v62-3）。
pub(crate) fn assistant_action_bar(
    start_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    turn_text: &str,
    last_ts: Option<i64>,
    turn_user_ts: Option<i64>,
    is_working: bool,
    bar_revealed: bool,
    fork: Option<ForkAnchor>,
    t: &theme::Theme,
) -> gpui::Div {
    let forking = fork.as_ref().is_some_and(|f| f.forking);
    let mut bar = div()
        .flex()
        .items_center()
        .gap(px(14.))
        .mt(px(6.))
        .text_size(crate::appearance::ui_size(11.5))
        .text_color(rgb(t.text_dim))
        // pi-web: `hovered || forking` —— 分支在飞时栏不许消失，否则
        // 「创建中…」提示连同 disabled 态一起看不见
        .opacity(if bar_revealed || forking { 1. } else { 0. });
    if !turn_text.trim().is_empty() {
        let weak_copy = weak.clone();
        let copy_text = turn_text.to_string();
        bar = bar.child(
            div()
                .id(SharedString::from(format!("acopy-{start_ix}")))
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(t.text)))
                // icon()（工具卡同款显式色 svg）——列表内唯一稳定渲染路径
                .child(icon("copy", 12., t.text_dim))
                .child(SharedString::from(tr("复制")))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_copy.update(cx, |_c, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_text.clone()));
                    });
                }),
        );
    }
    // 「新分支」（用户定案：挂在 agent 轮操作栏上，用户消息不被丢弃）
    if let Some(anchor) = fork {
        bar = bar.child(fork_pill(start_ix, weak, &anchor, t));
    }
    // 用时取轮内末条消息（as-stats 普通 span：浅一档）
    if let (Some(end), Some(start)) = (last_ts, turn_user_ts) {
        bar = bar.child(
            div()
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(format!(
                    "{}{}",
                    tr("用时"),
                    crate::services::format::fmt_duration_ms(end - start)
                ))),
        );
    }
    // 时间戳在栏内（as-stats 普通 span）；流式尾部隐藏
    if !is_working {
        if let Some(ts) = last_ts {
            bar = bar.child(
                div()
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(crate::services::format::fmt_msg_time(ts))),
            );
        }
    }
    bar
}

/// agent 轮操作栏里的「新分支」pill。
///
/// - 目标 = `ForkAnchor`：保留到本轮 agent 回复为止（fork 到下一条用户消息
///   之前）；本轮是尾部时走 rpc `clone`（整段复制）
/// - in-flight：文案「创建中…」、主题色、`not-allowed`、不挂 handler
///   （pi-web `forking` parity，顺带防连点）
/// - 锚点未就绪（`clickable()` 为假）时只渲染外观，不接点击——宁可不点，
///   也不能把分支开在错的地方
pub(crate) fn fork_pill(
    start_ix: usize,
    weak: &gpui::WeakEntity<Chat>,
    anchor: &ForkAnchor,
    t: &theme::Theme,
) -> gpui::AnyElement {
    let forking = anchor.forking;
    let label = SharedString::from(tr(if forking { "创建中…" } else { "新分支" }));
    // 图标 12 + 文字，gap 4（与用户栏 act 同尺）
    let color = if forking { t.accent } else { t.text_dim };
    let mut pill = div()
        .id(SharedString::from(format!("afork-{start_ix}")))
        .flex()
        .items_center()
        .gap(px(4.))
        .text_color(rgb(color))
        .child(icon("git-branch", 12., color))
        .child(label);
    if forking {
        return pill.cursor_not_allowed().into_any_element();
    }
    if !anchor.clickable() {
        return pill.into_any_element();
    }
    let next_user = anchor.next_user.clone();
    let weak = weak.clone();
    pill = pill
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let next_user = next_user.clone();
            let _ = weak.update(cx, |c, cx| {
                c.rt().update(cx, |r, cx| r.fork_from_turn(next_user, cx))
            });
        });
    pill.into_any_element()
}
