//! 微信通信页（060；`ContentView::Wx` 内容区全屏页，三横菜单唯一入口）。
//! 2026-10-23 定夺：自 dock 侧栏迁出，topbar/statusbar 图标全撤；
//! **扫码弹窗（`Dialog::WxQr`）同步废除**——二维码直接绘在本页内。
//! 启用开关 + 渠道状态 + 绑定信息 + 扫码（内联）。
//!
//! 控件取 ui/kit（config_switch / section_title / note / error_note）；
//! 多行等宽块 [`mono_lines`] 供状态/绑定小节与扫码结果共用。

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::kit::{self, Btn};

use super::QrState;

/// 多行等宽块：QR 结果、状态串、长 bot_id 都走它。
fn mono_lines(lines: Vec<String>, size: f32, dim: bool) -> gpui::AnyElement {
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

/// 页面主体：控制栏（标题 + 启用开关）+ 滚动 body（状态/绑定/扫码）。
pub(crate) fn page(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let mut col = div()
        .id("wx-panel")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
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

    // ── 扫码绑定：二维码**内联绘在本页**（2026-10-23 定夺，弹窗已废）──
    let mut qr_content: Vec<gpui::AnyElement> = Vec::new();
    match &chat.aux.remote.qr {
        QrState::Idle => qr_content.push(kit::note(&tr("remote.qr_desc"))),
        QrState::Loading => qr_content.push(kit::note(&tr("remote.qr_loading"))),
        QrState::Ready { url } => {
            qr_content.push(qr_grid(url));
            qr_content.push(kit::note(&tr("remote.qr_hint")));
        }
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
        // 过期=重取，其余（含 Ready/Scanned 复看）=获取/重开
        if matches!(chat.aux.remote.qr, QrState::Expired) {
            tr("remote.qr_refresh")
        } else {
            tr("remote.qr_begin")
        },
        Btn::Secondary,
        true,
        false,
        |c, cx| {
            // 幂等：已在扫码/已出码时不重发
            c.aux.remote.begin_qr();
            cx.notify();
        },
    ));
    body = body.child(section(&tr("remote.qr_title"), qr_content));

    col = col.child(body);
    col.into_any_element()
}

/// 二维码**实心方块**绘制（自扫码弹窗迁入，2026-10-23 弹窗废除）。
/// 不用字符画：字形抗锯齿会在模块之间留细缝，手机容易解不出来
/// （用户实测扫不上）。按行把连续深色段合并成一个方块，49x49 的
/// 网格约 600 个元素，不是 2401 个。
fn qr_grid(url: &str) -> gpui::AnyElement {
    const MOD: f32 = 8.; // 8px/模块；49 模块 = 392px
    const DARK: u32 = 0x111827;
    let grid = match wxprobe::qr::qr_grid(url, 4) {
        Ok(g) => g,
        Err(e) => {
            return kit::error_note(&crate::i18n::tf("remote.qr_gen_failed", &[
                ("e", e.to_string()),
            ]))
        }
    };
    let side = grid.size as f32 * MOD;
    let mut canvas = div()
        .w(px(side))
        .h(px(side))
        .flex_shrink_0()
        .bg(rgb(0xff_ffff))
        .flex()
        .flex_col();
    for y in 0..grid.size {
        let mut row = div().h(px(MOD)).flex().flex_row().flex_shrink_0();
        let mut cursor = 0usize;
        for (x, len) in grid.runs(y) {
            if x > cursor {
                row = row.child(div().w(px((x - cursor) as f32 * MOD)).h(px(MOD)));
            }
            row = row
                .child(div().w(px(len as f32 * MOD)).h(px(MOD)).bg(rgb(DARK)));
            cursor = x + len;
        }
        if cursor < grid.size {
            row = row.child(div().w(px((grid.size - cursor) as f32 * MOD)).h(px(MOD)));
        }
        canvas = canvas.child(row);
    }
    div()
        .flex()
        .justify_center()
        .child(canvas)
        .into_any_element()
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

impl Chat {
    /// 打开「微信通信」全屏页（三横菜单是唯一入口；automation
    /// `content.view wx` / `panel.dock wx` 也落到这里）。
    pub(crate) fn open_wx(&mut self, cx: &mut gpui::Context<Self>) {
        self.dlg.market_menu = None;
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::Wx))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(crate::PanelTab::Wx);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(crate::ContentView::Wx);
        cx.notify();
    }
}
