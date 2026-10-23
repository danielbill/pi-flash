//! 微信通信页（060；`ContentView::Wx` 内容区全屏页，三横菜单唯一入口）。
//! 2026-10-24 两栏重构（截图驱动，对齐 ZCode 远控入口版式）：
//! 左「手机扫码连接」= 状态卡 + 内联二维码（虚线框）；右「使用 Bot Channel」
//! = 微信渠道卡 + 机器人管理（展开绑定码 / bot_id）。
//!
//! **刻意不搬**（与截图的差异，均为定夺非遗漏）：
//! - 飞书 / Lark / Telegram 渠道卡 —— 060 范围只有微信（§0）；
//! - 「无法扫码？可以在手机上打开链接 + 复制链接」行 —— PF 的码只能扫，
//!   没有 H5 工作区链接可复制。
//!
//! 控件取 ui/kit（card / state_badge / note / config_button / button_base）；
//! [`mono_lines`] 供状态串与 bot_id 共用。

use gpui::{FontWeight, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::kit::{self, Btn};

use super::QrState;

/// 微信品牌绿（simple-icons wechat 主色，渠道卡 icon 着色）。
const WX_GREEN: u32 = 0x07c160;

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

/// 栏头：图标 + 标题（两栏同构）。
fn col_header(icon: &'static str, title: &str) -> gpui::AnyElement {
    let t = T();
    let title_el = div()
        .text_size(crate::appearance::ui_size(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text))
        .child(SharedString::from(title.to_string()));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(crate::ui::icon(icon, 16., t.text))
        .child(title_el)
        .into_any_element()
}

/// 状态卡右缘的动作钮（渠道级停启）。
enum CardAction {
    Stop,
    Start,
}

/// 左栏状态卡：标题 + 状态徽标 + 右缘动作钮 + 一句提示。
/// 状态优先级：扫码流程进行中 > 渠道启停态；「等待手机连接 / 已就绪 /
/// 停止」的默认形态与截图对齐（Ready 分支）。
fn scan_status_card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    running: bool,
    has_token: bool,
) -> gpui::AnyElement {
    // 徽标点色：等待 = WARN 黄、就绪/运行/已绑 = UNREAD 绿、
    // 过期/失败 = danger、中性（获取中/未启用）= text_faint
    let (title, badge, dot, hint, action): (&str, &str, u32, String, Option<CardAction>) =
        match &chat.aux.remote.qr {
            QrState::Loading => (
                tr("remote.scan_wait"),
                tr("remote.st_getting"),
                t.text_faint,
                tr("remote.qr_loading").to_string(),
                None,
            ),
            QrState::Ready { .. } => (
                tr("remote.scan_wait"),
                tr("remote.st_ready"),
                crate::appearance::theme::WARN,
                tr("remote.qr_hint").to_string(),
                running.then_some(CardAction::Stop),
            ),
            QrState::Scanned => (
                tr("remote.scan_wait"),
                tr("remote.bound_scanned"),
                crate::appearance::theme::WARN,
                tr("remote.qr_confirm").to_string(),
                running.then_some(CardAction::Stop),
            ),
            QrState::Done { .. } => (
                tr("remote.qr_done"),
                tr("remote.st_bound"),
                crate::appearance::theme::UNREAD,
                tr("remote.qr_help").to_string(),
                running.then_some(CardAction::Stop),
            ),
            QrState::Expired => (
                tr("remote.qr_title"),
                tr("remote.st_expired"),
                t.danger,
                tr("remote.qr_expired").to_string(),
                None,
            ),
            QrState::Error(m) => (
                tr("remote.qr_title"),
                tr("remote.st_failed"),
                t.danger,
                m.clone(),
                None,
            ),
            QrState::Idle => match running {
                true => (
                    tr("remote.scan_wait"),
                    tr("remote.status_running"),
                    crate::appearance::theme::UNREAD,
                    tr("remote.scan_wait_hint").to_string(),
                    Some(CardAction::Stop),
                ),
                false if has_token => (
                    tr("remote.scan_stopped"),
                    tr("remote.st_disabled"),
                    t.text_faint,
                    tr("remote.scan_stopped_hint").to_string(),
                    Some(CardAction::Start),
                ),
                false => (
                    tr("remote.qr_title"),
                    tr("remote.bound_not_scanned"),
                    crate::appearance::theme::WARN,
                    tr("remote.scan_begin_hint").to_string(),
                    None,
                ),
            },
        };
    let action_btn = action.map(|a| match a {
        CardAction::Stop => kit::config_button(
            "wx-channel-stop",
            weak,
            &tr("remote.scan_stop"),
            Btn::Secondary,
            true,
            false,
            |c, cx| {
                c.aux.remote.set_enabled(false);
                cx.notify();
            },
        ),
        CardAction::Start => kit::config_button(
            "wx-channel-start",
            weak,
            &tr("remote.scan_start"),
            Btn::Secondary,
            true,
            false,
            |c, cx| {
                c.aux.remote.set_enabled(true);
                cx.notify();
            },
        ),
    });
    let title_el = div()
        .text_size(crate::appearance::ui_size(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text))
        .child(SharedString::from(title.to_string()));
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(title_el)
        .child(kit::state_badge(badge, dot))
        .child(div().flex_1());
    if let Some(b) = action_btn {
        row = row.child(b);
    }
    kit::card()
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(row)
        .child(kit::note(&hint))
        .into_any_element()
}

/// 左栏二维码区：虚线框（R_CARD 8 + dashed），内容随 `QrState` 变化。
/// 取码由泵自动驱动（页在屏上即出码，用户零点击），Idle 与 Loading 同显示。
fn qr_area(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let mut inner: Vec<gpui::AnyElement> = Vec::new();
    match &chat.aux.remote.qr {
        QrState::Ready { url } => inner.push(qr_white_card(url)),
        QrState::Scanned => {
            // 码保持可见（qr_url 缓存）：确认期间手机端可能要重扫
            if let Some(url) = &chat.aux.remote.qr_url {
                inner.push(qr_white_card(url));
            }
            inner.push(kit::note(&tr("remote.qr_confirm")));
        }
        QrState::Idle | QrState::Loading => inner.push(kit::note(&tr("remote.qr_loading"))),
        QrState::Done { bot_id } => {
            let mut lines = vec![tr("remote.qr_done").to_string()];
            if let Some(b) = bot_id {
                lines.push(format!("bot_id = {b}"));
            }
            lines.push(tr("remote.qr_help").into());
            inner.push(mono_lines(lines, 11., true));
        }
        // 过期后泵下一拍自动续码，无需用户操作
        QrState::Expired => inner.push(kit::error_note(&tr("remote.qr_expired"))),
        QrState::Error(m) => {
            inner.push(kit::error_note(m));
            inner.push(qr_refresh_button(weak));
        }
    }
    div()
        .w_full()
        .min_h(px(440.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .border_dashed()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .p(px(16.))
        .children(inner)
        .into_any_element()
}

/// 「重新获取」钮（Error 专用；`begin_qr` 幂等——Expired 走泵自动续码）。
fn qr_refresh_button(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    kit::config_button(
        "wx-qr-refresh",
        weak,
        &tr("remote.qr_refresh"),
        Btn::Secondary,
        false,
        false,
        |c, cx| {
            c.aux.remote.begin_qr();
            cx.notify();
        },
    )
}

/// 二维码白卡：白色圆角衬底（R_SHEET 12，截图样式），内装实心方块码。
fn qr_white_card(url: &str) -> gpui::AnyElement {
    div()
        .rounded(px(12.))
        .bg(rgb(0xffffff))
        .p(px(12.))
        .child(qr_grid(url))
        .into_any_element()
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
            return kit::error_note(&crate::i18n::tf(
                "remote.qr_gen_failed",
                &[("e", e.to_string())],
            ));
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
            row = row.child(div().w(px(len as f32 * MOD)).h(px(MOD)).bg(rgb(DARK)));
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

/// 右栏微信渠道卡：品牌 icon + 说明。启停动作只在左栏状态卡。
fn wechat_card() -> gpui::AnyElement {
    let t = T();
    let title_el = div()
        .text_size(crate::appearance::ui_size(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text))
        .child(SharedString::from(tr("remote.wx_name").to_string()));
    kit::card()
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(crate::ui::icon("wechat", 20., WX_GREEN))
                .child(title_el),
        )
        .child(kit::note(&tr("remote.wx_desc")))
        .into_any_element()
}

/// 页面主体：两栏（左「扫码连接」弹性 + 右「Bot 渠道」`LIST_W` 定宽），
/// 纵向滚动。页头即两栏栏头（tab 条已有「微信通信」标题，无全局控制栏）。
pub(crate) fn page(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let running = chat.aux.remote.is_running();
    let token = wxprobe::state::load();
    let has_token = wxprobe::transport::require_token(&token).is_ok();

    let left = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(col_header("smartphone", &tr("remote.scan_title")))
        .child(scan_status_card(chat, weak, t, running, has_token))
        .child(qr_area(chat, weak));

    let right = div()
        .w(px(kit::LIST_W))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(col_header("bot", &tr("remote.bot_title")))
        .child(wechat_card());

    div()
        .id("wx-panel")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(16.))
                .px(px(20.))
                .pt(px(16.))
                .pb(px(20.))
                .child(left)
                .child(right),
        )
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
