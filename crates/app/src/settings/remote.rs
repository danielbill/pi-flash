//! 远程控制 · 微信（060 P4）：扫码面板 + 渠道开关 + 绑定信息。
//!
//! 单列页（与「界面」「其他」同形态，滚动在 body）。
//!
//! 二维码用 [`wxprobe::qr::qr_block_text`] 渲成**半块字符画**再按等宽字体
//! 逐行铺开 —— 不引入图片解码依赖就能扫（手机对屏扫即可）。

use super::*;

use crate::remote_control::QrState;

/// 多行等宽块：QR 字符画、长状态串都走它。
fn block(lines: Vec<String>, size: f32, dim: bool) -> gpui::AnyElement {
    let t = T();
    let mut col = div()
        .flex()
        .flex_col()
        .font_family(crate::markdown::MONO_FAMILY)
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
    out.push(section_title("扫码绑定"));
    match chat.remote.qr.clone() {
        QrState::Idle => {
            out.push(note("用手机微信扫码，把当前聊天绑定到这台机器的 pi-flash。"));
            out.push(config_button(
                "wx-qr-begin",
                weak,
                "获取二维码",
                Btn::Secondary,
                false,
                false,
                |c, cx| {
                    c.remote.begin_qr();
                    cx.notify();
                },
            ));
        }
        QrState::Loading => out.push(note("正在获取二维码…")),
        QrState::Ready { url } => {
            match wxprobe::qr::qr_block_text(&url, 2) {
                Ok(art) => {
                    out.push(block(art.lines().map(str::to_string).collect(), 14., false));
                    out.push(note("用手机微信「扫一扫」对屏扫码；约 2 分钟后过期。"));
                }
                Err(e) => out.push(error_note(&e)),
            }
            out.push(config_button(
                "wx-qr-refresh",
                weak,
                "重新获取",
                Btn::Secondary,
                false,
                false,
                |c, cx| {
                    c.remote.begin_qr();
                    cx.notify();
                },
            ));
        }
        QrState::Scanned => out.push(note("已扫码，请在手机上确认…")),
        QrState::Done { bot_id } => {
            let mut lines = vec!["✅ 绑定成功".to_string()];
            if let Some(b) = bot_id {
                lines.push(format!("bot_id = {b}"));
            }
            lines.push("发送 /帮助 查看可用命令。".into());
            out.push(block(lines, 12., false));
        }
        QrState::Expired => {
            out.push(error_note("二维码已过期，请重新获取。"));
            out.push(config_button(
                "wx-qr-again",
                weak,
                "获取二维码",
                Btn::Secondary,
                false,
                false,
                |c, cx| {
                    c.remote.begin_qr();
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
        "启用远程控制",
        config_switch("wx-enable", weak, chat.remote.is_running(), false, |c, cx| {
            let on = !c.remote.is_running();
            c.remote.set_enabled(on);
            cx.notify();
        }),
    ));
    col = col.child(note(
        "关停会立刻停掉长轮询线程并释放轮询锁（无悬挂线程）。",
    ));

    // ── 渠道状态 ────────────────────────────────────────────────────
    let running = chat.remote.is_running();
    let token = wxprobe::state::load();
    let has_token = wxprobe::transport::require_token(&token).is_ok();
    let bot_id = wxprobe::state::get_str(&token, "bot_id").unwrap_or_default();
    let status_lines = vec![
        format!(
            "渠道：{}",
            if running { "运行中" } else if has_token { "已停止（有 token）" } else { "未启动" }
        ),
        format!(
            "绑定：{}",
            if has_token {
                if bot_id.is_empty() {
                    "已扫码".to_string()
                } else {
                    bot_id
                }
            } else {
                "未扫码".to_string()
            },
        ),
    ];
    col = col.child(field("状态", block(status_lines, 11., true)));

    // ── 扫码面板 ────────────────────────────────────────────────────
    for el in qr_panel(chat, weak) {
        col = col.child(el);
    }

    col.into_any_element()
}
