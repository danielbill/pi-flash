//! 微信通信页（060；`ContentView::Wx` 内容区全屏页，三横菜单唯一入口）。
//! 2026-10-24 按 `pi-flash-plugins/docs/插件设计/002-微信通信.md` 重绘：
//! 上下两区（**状态说明区** + **二维码生成展示区**），单列布局为插件化
//! （pf-wxbot）铺路。状态判定沿用七态生命周期（①未创建 ②微信侧出错
//! ③更新重建 ④我方创建出错 ⑤创建成功 ⑥启用中 ⑦关停中）。

use gpui::{FontWeight, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::ui::kit::{self, Btn};

use super::QrState;

/// 微信品牌绿（simple-icons wechat 主色，链路行微信 icon 着色）。
const WX_GREEN: u32 = 0x07c160;

/// 状态说明区（002 §状态说明区）：第一行 `Pi-Flash ⟷ 微信` 链路标识 +
/// 启停开关（仅已创建显示），第二行固定说明 + 随七态变化的提示。
fn status_area(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    running: bool,
    has_token: bool,
) -> gpui::AnyElement {
    let t = T();
    let name = |s: &str| {
        div()
            .text_size(crate::appearance::ui_size(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(t.text))
            .child(SharedString::from(s.to_string()))
    };
    // linked/unlink = 是否扫码创建成功（状态①为 unlink，其余为 link）
    let link = if has_token {
        crate::ui::icon("link-2", 14., crate::appearance::theme::UNREAD)
    } else {
        crate::ui::icon("link-2-off", 14., t.text_faint)
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(6.))
        // logo-marks 的字形只占画布 ~2/3，放大到 20 与 14px lucide 邻居对齐
        .child(crate::ui::icon_path("icons/logo-marks.svg", 20., t.text))
        .child(name("Pi-Flash"))
        .child(link)
        .child(crate::ui::icon("wechat", 16., WX_GREEN))
        .child(name("微信"));
    // phone icon 仅已创建显示：accent = 启用中，faint = 停用
    if has_token {
        row = row.child(crate::ui::icon(
            "smartphone",
            14.,
            if running { t.accent } else { t.text_faint },
        ));
    }
    row = row.child(div().flex_1());
    // switch 仅已创建显示（002：linked 状态下才显示）
    if has_token {
        row = row.child(kit::config_switch(
            "wx-enable",
            weak,
            running,
            false,
            |c, cx| {
                let on = !c.aux.remote.is_running();
                c.aux.remote.set_enabled(on);
                cx.notify();
            },
        ));
    }
    kit::card()
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(row)
        .child(kit::note(&tr("remote.wx_desc")))
        .child(kit::note(&state_hint(chat, running, has_token)))
        .into_any_element()
}

/// 第二行提示：随七态生命周期变化（002 §状态说明区）。
fn state_hint(chat: &Chat, running: bool, has_token: bool) -> String {
    match &chat.aux.remote.qr {
        // ② 微信侧出错
        QrState::Error(_) => tr("remote.hint_wx_error").to_string(),
        // ③ 更新重建（重新扫码中，旧 bot 仍在服务，成功后自动替换重启）
        QrState::Loading | QrState::Ready { .. } | QrState::Scanned | QrState::Expired
            if has_token =>
        {
            tr("remote.hint_rebind").to_string()
        }
        // ④⑤⑥⑦ 微信已正常返回 → 看我方落地结果
        QrState::Done { .. } => {
            if !has_token {
                // ④ 状态写入失败
                tr("remote.hint_create_failed").replace("{err}", &tr("remote.st_write_failed"))
            } else if running {
                // ⑥ 启用中
                tr("remote.scan_wait_hint").to_string()
            } else if chat.aux.remote.wants_start {
                // ⑤ 创建成功，泵下一拍自动拉渠道（瞬态）
                tr("remote.hint_starting").to_string()
            } else if let Some(err) = chat.aux.remote.start_error.as_deref() {
                // ④ 我方创建出错（渠道没能起来）
                tr("remote.hint_create_failed").replace("{err}", err)
            } else {
                // ⑦ 关停中
                tr("remote.scan_stopped_hint").to_string()
            }
        }
        QrState::Idle => {
            if running {
                // ⑥ 启用中
                tr("remote.scan_wait_hint").to_string()
            } else if has_token {
                // ⑦ 关停中
                tr("remote.scan_stopped_hint").to_string()
            } else {
                // ① 未创建：pf-wxbot 不存在
                tr("remote.hint_absent").to_string()
            }
        }
        // ① 未创建：取码/扫码进行中（过期自动续码，同属创建流程）
        _ => tr("remote.hint_absent").to_string(),
    }
}

/// 二维码生成展示区（002 §二维码生成展示区）：三态——未创建/换绑中出码、
/// 已创建不展示、超时蒙白。虚线框（R_CARD 8 + dashed），取码由泵自动驱动。
fn qr_area(chat: &Chat, weak: &gpui::WeakEntity<Chat>, has_token: bool) -> gpui::AnyElement {
    let t = T();
    let mut inner: Vec<gpui::AnyElement> = Vec::new();
    match &chat.aux.remote.qr {
        // 已创建（且不在换绑流程中）：不展示二维码，给重建入口
        QrState::Idle | QrState::Done { .. } if has_token => {
            inner.push(kit::note(&tr("remote.qr_created")));
            inner.push(qr_regen_button(weak));
        }
        // ④ 微信已返回但我方没写进状态：按出错蒙白处理
        QrState::Done { .. } => inner.push(qr_masked(
            chat,
            weak,
            tr("remote.hint_create_failed").replace("{err}", &tr("remote.st_write_failed")),
        )),
        // 扫码超时：白蒙旧码（002：『扫描超时，请点击重新扫码』）
        QrState::Expired => inner.push(qr_masked(chat, weak, tr("remote.qr_timeout").to_string())),
        QrState::Error(m) => inner.push(qr_masked(chat, weak, friendly_qr_error(m))),
        // 未创建/换绑中：二维码本体（Scanned 保持可见，确认期间可重扫）
        QrState::Ready { url } => inner.push(qr_white_card(url)),
        QrState::Scanned => {
            if let Some(url) = &chat.aux.remote.qr_url {
                inner.push(qr_white_card(url));
            }
            inner.push(kit::note(&tr("remote.qr_confirm")));
        }
        QrState::Idle | QrState::Loading => inner.push(kit::note(&tr("remote.qr_loading"))),
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

/// 用户可读的扫码错误：原始诊断（HTTP 细节/字段名）只进 stderr 与 dump，
/// 界面上只留行动指引（2026-10-24 用户定夺「对用户没有帮助的信息不展示」）。
fn friendly_qr_error(m: &str) -> String {
    let low = m.to_lowercase();
    if low.contains("timeout") || m.contains("超时") {
        // 超时按 002 的超时口径显示
        tr("remote.qr_timeout").to_string()
    } else if m.contains("未安装") || m.contains("未识别的扫码状态") {
        // worker 拼好的行动指引，保留
        m.to_string()
    } else {
        tr("remote.qr_retry").to_string()
    }
}

/// 码上蒙白：二维码（有缓存就画）+ 不透明白色蒙层 + 居中提示与「重新生成
/// 二维码」。没有缓存（取码就失败）时用同尺寸白底占位，文案不变。
fn qr_masked(chat: &Chat, weak: &gpui::WeakEntity<Chat>, message: String) -> gpui::AnyElement {
    let t = T();
    let mut wrap = div().relative();
    match &chat.aux.remote.qr_url {
        Some(url) => wrap = wrap.child(qr_white_card(url)),
        None => {
            wrap = wrap
                .w(px(416.))
                .h(px(416.))
                .bg(rgb(0xffffff))
                .rounded(px(12.))
        }
    }
    let message_el = div()
        .text_size(crate::appearance::ui_size(12.5))
        .text_color(rgb(t.danger))
        .child(SharedString::from(message));
    let overlay = div()
        .absolute()
        .inset_0()
        .bg(rgb(0xffffff))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .child(message_el)
        .child(qr_regen_button(weak));
    wrap.child(overlay).into_any_element()
}

/// 「重新生成二维码」钮（重建机器人；`begin_qr` 幂等）。
fn qr_regen_button(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    kit::config_button(
        "wx-qr-refresh",
        weak,
        &tr("remote.qr_regen"),
        Btn::Secondary,
        false,
        false,
        |c, cx| {
            c.aux.remote.begin_qr();
            cx.notify();
        },
    )
}

/// 二维码白卡：白色圆角衬底（R_SHEET 12），内装实心方块码。
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

/// 页面主体：单列上下两区（002 §UI设计）——上「状态说明区」、下「二维码
/// 生成展示区」，纵向滚动，为插件化（pf-wxbot）铺路。
pub(crate) fn page(chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let running = chat.aux.remote.is_running();
    let has_token = wxprobe::transport::require_token(&wxprobe::state::load()).is_ok();
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
                .flex_col()
                .gap(px(12.))
                .px(px(20.))
                .pt(px(16.))
                .pb(px(20.))
                .child(status_area(chat, weak, running, has_token))
                .child(qr_area(chat, weak, has_token)),
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
