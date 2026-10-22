//! 微信 Bot（060 P4，历史名「远程控制」已正名，见 remote_control::mod 头注）：
//! 扫码面板 + 渠道开关 + 绑定信息。
//!
//! 单列页（与「界面」「其他」同形态，滚动在 body）。
//!
//! 二维码用 [`wxprobe::qr::qr_block_text`] 渲成**半块字符画**再按等宽字体
//! 逐行铺开 —— 不引入图片解码依赖就能扫（手机对屏扫即可）。

use super::*;

use crate::remote_control::QrState;

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

/// 扫码面板：按 `QrState` 给出对应的一屏。
fn qr_panel(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> Vec<gpui::AnyElement> {
    let t = T();
    let mut out: Vec<gpui::AnyElement> = Vec::new();
    out.push(section_title(tr("remote.dlg_title")));
    match chat.aux.remote.qr.clone() {
        QrState::Idle => {
            out.push(note(tr("remote.qr_desc")));
            out.push(config_button(
                "wx-qr-begin",
                weak,
                tr("remote.qr_begin"),
                Btn::Secondary,
                false,
                false,
                |c, cx| {
                    c.aux.remote.begin_qr();
                    cx.notify();
                },
            ));
        }
        QrState::Loading => out.push(note(tr("remote.qr_loading"))),
        QrState::Ready { url } => {
            match wxprobe::qr::qr_block_text(&url, 2) {
                Ok(art) => {
                    out.push(mono_lines(
                        art.lines().map(str::to_string).collect(),
                        14.,
                        false,
                    ));
                    out.push(note(tr("remote.qr_hint")));
                }
                Err(e) => out.push(error_note(&e)),
            }
            out.push(config_button(
                "wx-qr-refresh",
                weak,
                tr("remote.qr_refresh"),
                Btn::Secondary,
                false,
                false,
                |c, cx| {
                    c.aux.remote.begin_qr();
                    cx.notify();
                },
            ));
        }
        QrState::Scanned => out.push(note(tr("remote.qr_scanned"))),
        QrState::Done { bot_id } => {
            let mut lines = vec![tr("remote.qr_done").to_string()];
            if let Some(b) = bot_id {
                lines.push(format!("bot_id = {b}"));
            }
            lines.push(tr("remote.qr_help").into());
            out.push(mono_lines(lines, 12., false));
        }
        QrState::Expired => {
            out.push(error_note(tr("remote.qr_expired")));
            out.push(config_button(
                "wx-qr-again",
                weak,
                tr("remote.qr_begin"),
                Btn::Secondary,
                false,
                false,
                |c, cx| {
                    c.aux.remote.begin_qr();
                    cx.notify();
                },
            ));
        }
        QrState::Error(m) => out.push(error_note(&m)),
    }
    let _ = t;
    out
}

pub(crate) fn mc_remote_view(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let mut col = div()
        .id("mc-remote")
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(14.));

    // ── 开关：关停即停线程（P4 验收项） ──────────────────────────────
    col = col.child(field(
        tr("remote.enable"),
        config_switch(
            "wx-enable",
            weak,
            chat.aux.remote.is_running(),
            false,
            |c, cx| {
                let on = !c.aux.remote.is_running();
                c.aux.remote.set_enabled(on);
                cx.notify();
            },
        ),
    ));
    col = col.child(note(tr("remote.enable_hint")));

    // ── 渠道状态 ────────────────────────────────────────────────────
    let running = chat.aux.remote.is_running();
    let token = wxprobe::state::load();
    let has_token = wxprobe::transport::require_token(&token).is_ok();
    let bot_id = wxprobe::state::get_str(&token, "bot_id").unwrap_or_default();
    let status_lines = vec![
        format!("{}", tr("remote.channel"),).replace(
            "{status}",
            match running {
                true => tr("remote.status_running"),
                false if has_token => tr("remote.status_stopped"),
                _ => tr("remote.status_not_started"),
            },
        ),
        format!("{}", tr("remote.binding"),).replace(
            "{status}",
            match has_token {
                true => {
                    if bot_id.is_empty() {
                        tr("remote.bound_scanned")
                    } else {
                        &bot_id
                    }
                }
                false => tr("remote.bound_not_scanned"),
            },
        ),
    ];
    col = col.child(field(
        tr("remote.field_status"),
        mono_lines(status_lines, 11., true),
    ));

    // ── 绑定码：设置页展示，用户在微信里 `/bind <code>` 回填 ──────────
    let active = chat.pool.active_key.clone();
    let code = chat.aux.remote.bind_code(&active);
    let mut bind_lines = vec![tr("remote.bind_code").replace("{code}", &code)];
    match chat.aux.remote.bound.as_deref() {
        Some(k) => bind_lines.push(tr("remote.bound_session").replace("{k}", k)),
        None => bind_lines.push(tr("remote.bound_unbound").into()),
    }
    col = col.child(field(
        tr("remote.field_binding"),
        mono_lines(bind_lines, 11., true),
    ));

    // ── 扫码面板 ────────────────────────────────────────────────────
    for el in qr_panel(chat, weak) {
        col = col.child(el);
    }

    col.into_any_element()
}
