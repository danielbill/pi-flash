//! 微信 Bot 左列面板（060，dock 的 `DockPanel::Wx` 分支）：启用开关 +
//! 渠道状态 + 绑定信息 + 扫码入口。自设置页「微信 Bot」页签迁移
//!（2026-10-23 定夺：页签永久删除）。
//!
//! 二维码本体**不在这渲染**：面板宽 250–500 放不下 49 模块实体方块码，
//! 扫码走 [`super::render_wx_qr`] 的大卡弹窗（居中 8px/模块，扫码成功
//! 自动关窗落回工作界面）；面板按钮只负责 `begin_qr`（幂等）+ 开窗。
//!
//! 控件取 ui/kit（config_switch / section_title / note / error_note）；
//! 多行等宽块 [`mono_lines`] 由扫码弹窗共用。

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::kit::{self, Btn};

use super::QrState;

/// 多行等宽块：QR 字符画、长状态串都走它。扫码弹窗（`Dialog::WxQr`）复用。
pub(crate) fn mono_lines(lines: Vec<String>, size: f32, dim: bool) -> gpui::AnyElement {
    let t = T();
    let mut col = div()
        .flex()
        .flex_col()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(size))
        .text_color(if dim { rgb(t.text_dim) } else { rgb(t.text) });
    for l in lines {
        col = col.child(div().whitespace_nowrap().child(l));
    }
    col.into_any_element()
}

/// 小节：标题 + 内容列（面板窄版 field，标题与内容同列左对齐）。
fn section(title: &str, content: Vec<gpui::AnyElement>) -> gpui::AnyElement {
    let mut col = div()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(kit::section_title(title));
    for c in content {
        col = col.child(c);
    }
    col.into_any_element()
}

/// 面板主体：控制栏（标题 + 启用开关）+ 滚动 body（状态/绑定/扫码）。
pub(crate) fn panel_view(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let mut col = div()
        .id("wx-panel")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(ctrl_bar(chat, weak, t));

    let mut body = div()
        .id("wx-panel-scroll")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(14.))
        .px(px(12.))
        .pt(px(10.))
        .pb(px(12.))
        .child(kit::note(&tr("remote.enable_hint")));

    // ── 渠道状态 ────────────────────────────────────────────────────
    let running = chat.aux.remote.is_running();
    let token = wxprobe::state::load();
    let has_token = wxprobe::transport::require_token(&token).is_ok();
    let bot_id = wxprobe::state::get_str(&token, "bot_id").unwrap_or_default();
    let mut status_lines = vec![
        tr("remote.channel").replace(
            "{status}",
            match running {
                true => tr("remote.status_running"),
                false if has_token => tr("remote.status_stopped"),
                _ => tr("remote.status_not_started"),
            },
        ),
        tr("remote.binding").replace(
            "{status}",
            match has_token {
                true if bot_id.is_empty() => tr("remote.bound_scanned"),
                true => &bot_id,
                false => tr("remote.bound_not_scanned"),
            },
        ),
    ];
    // 「运行中」≠ 收得到消息：长轮询最近一拍的错误跟着亮出来
    if let Some(err) = chat.aux.remote.poll_error.as_deref() {
        status_lines.push(tr("remote.poll_error").replace("{err}", err));
    }
    body = body.child(section(
        &tr("remote.field_status"),
        vec![mono_lines(status_lines, 11., true)],
    ));

    // ── 绑定码：微信里 `/bind <code>` 回填 ───────────────────────────
    let active = chat.pool.active_key.clone();
    let code = chat.aux.remote.bind_code(&active);
    let mut bind_lines = vec![tr("remote.bind_code").replace("{code}", &code)];
    match chat.aux.remote.bound.as_deref() {
        Some(k) => bind_lines.push(tr("remote.bound_session").replace("{k}", k)),
        None => bind_lines.push(tr("remote.bound_unbound").into()),
    }
    body = body.child(section(
        &tr("remote.field_binding"),
        vec![mono_lines(bind_lines, 11., true)],
    ));

    // ── 扫码绑定：按钮开 `Dialog::WxQr` 大卡弹窗（头注：码在弹窗渲染） ──
    let mut qr_content: Vec<gpui::AnyElement> = Vec::new();
    match &chat.aux.remote.qr {
        QrState::Idle => qr_content.push(kit::note(&tr("remote.qr_desc"))),
        QrState::Loading => qr_content.push(kit::note(&tr("remote.qr_loading"))),
        QrState::Ready { .. } => qr_content.push(kit::note(&tr("remote.qr_hint"))),
        QrState::Scanned => qr_content.push(kit::note(&tr("remote.qr_confirm"))),
        QrState::Done { bot_id } => {
            let mut lines = vec![tr("remote.qr_done").to_string()];
            if let Some(b) = bot_id {
                lines.push(format!("bot_id = {b}"));
            }
            lines.push(tr("remote.qr_help").into());
            qr_content.push(mono_lines(lines, 11., true));
        }
        QrState::Expired => qr_content.push(kit::error_note(&tr("remote.qr_expired"))),
        QrState::Error(m) => qr_content.push(kit::error_note(m)),
    }
    qr_content.push(kit::config_button(
        "wx-qr-begin",
        weak,
        // 过期=重取，其余（含弹窗被关后的 Ready/Scanned 复看）=获取/重开
        if matches!(chat.aux.remote.qr, QrState::Expired) {
            tr("remote.qr_refresh")
        } else {
            tr("remote.qr_begin")
        },
        Btn::Secondary,
        true,
        false,
        |c, cx| {
            // 幂等：已在扫码/已出码时不重发；弹窗内扫码成功自动关窗
            c.aux.remote.begin_qr();
            c.dlg.dialog = Some(crate::Dialog::WxQr);
            cx.notify();
        },
    ));
    body = body.child(section(&tr("remote.qr_title"), qr_content));

    col = col.child(body);
    col.into_any_element()
}

/// 控制栏（38px，psp 控制栏同档）：「微信」标签 + 右缘启用开关。
/// 开关 = 渠道总开关（关停即停长轮询线程，060 P4 验收项）。
fn ctrl_bar(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> gpui::AnyElement {
    let running = chat.aux.remote.is_running();
    div()
        .id("wx-panel-bar")
        .w_full()
        .flex_shrink_0()
        .h(px(crate::ui::tokens::panel_ctrl::BAR_H))
        .flex()
        .items_center()
        .gap(px(6.))
        .pl(px(12.))
        .pr(px(10.))
        .child(
            gpui::svg()
                .path(SharedString::from("icons/smartphone.svg"))
                .text_color(rgb(if running { t.accent } else { t.text_dim }))
                .size(px(16.)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(tr("remote.panel_title")),
        )
        .child(div().flex_1())
        .child(kit::config_switch(
            "wx-enable",
            weak,
            running,
            false,
            |c, cx| {
                let on = !c.aux.remote.is_running();
                c.aux.remote.set_enabled(on);
                cx.notify();
            },
        ))
        .into_any_element()
}
