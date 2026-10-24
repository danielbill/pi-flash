//! 微信通信（060）：「微信线会话」已升格为**普通会话 tab**（桌面与微信
//! 同权同视图，2026-10-24 定夺），本文件只剩三件套——
//! 1. [`fn_bar_status`]：会话操作栏右侧状态簇（仅活跃会话是微信线时）
//! 2. [`wx_qr_panel`]：二维码弹窗（Dialog::WxQr，dialogs.rs 调用）
//! 3. [`Chat::open_wx`]：入口（确保微信线会话存在并激活；未创建自动弹码）
//!
//! UI 语义收敛于 `pi-flash-plugins/docs/插件设计/002-微信通信.md`（提示行
//! 三态、展示区三态）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::kit::{self, Btn};

use super::QrState;

/// 会话操作栏右侧状态簇（仅活跃会话是微信线时显示，`session_fn_bar` 调
/// 用）：[链路 icon] [手机 icon] [重制二维码] [启停开关]，右对齐。
pub(crate) fn fn_bar_status(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> Option<gpui::AnyElement> {
    let is_wx_line = chat.aux.remote.wx_session.as_deref() == Some(chat.pool.active_key.as_str());
    if !is_wx_line {
        return None;
    }
    let t = T();
    let running = chat.aux.remote.is_running();
    let has_token = wxprobe::transport::require_token(&wxprobe::state::load()).is_ok();
    let weak = cx.entity().downgrade();
    // linked/unlink = 是否扫码创建成功
    let link = if has_token {
        crate::ui::icon("link-2", 16., crate::appearance::theme::UNREAD)
    } else {
        crate::ui::icon("link-2-off", 16., t.text_faint)
    };
    let mut cluster = div().flex().items_center().gap(px(6.)).child(link);
    // phone icon 仅已创建显示：accent = 启用中，faint = 停用
    if has_token {
        cluster = cluster.child(crate::ui::icon(
            "smartphone",
            16.,
            if running { t.accent } else { t.text_faint },
        ));
    }
    // 【重制二维码】：取码 + 开弹窗
    cluster = cluster.child(
        div()
            .id("sfbar-wx-qr")
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .text_size(crate::appearance::ui_size(11.5))
            .text_color(rgb(t.text_muted))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.aux.remote.begin_qr();
                    this.dlg.dialog = Some(crate::Dialog::WxQr);
                    cx.notify();
                }),
            )
            .child(crate::ui::icon(
                "refresh",
                crate::ui::tokens::icon::MD,
                t.text_muted,
            ))
            .child(SharedString::from(tr("remote.qr_reset").to_string())),
    );
    // 启停开关（仅已创建显示；关停即停长轮询线程，060 P4 验收项）
    if has_token {
        cluster = cluster.child(kit::config_switch(
            "wx-enable",
            &weak,
            running,
            false,
            |c, cx| {
                let on = !c.aux.remote.is_running();
                c.aux.remote.set_enabled(on);
                cx.notify();
            },
        ));
    }
    Some(
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .child(cluster)
            .into_any_element(),
    )
}

/// 二维码弹窗面板（`Dialog::WxQr`，dialogs.rs 用 dialog_shell 包壳）：
/// 三态（出码+安装提示 / 已创建 / 超时蒙白+重新生成）+ 底部三态提示行
/// （002 §状态说明区收敛于此）。
pub(crate) fn wx_qr_panel(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &Theme) -> gpui::Div {
    let has_token = wxprobe::transport::require_token(&wxprobe::state::load()).is_ok();
    let mut body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.))
        .py(px(8.));
    match &chat.aux.remote.qr {
        QrState::Ready { url } => {
            body = body
                .child(qr_white_card(url))
                .child(kit::note(&tr("remote.qr_install_hint")));
        }
        QrState::Scanned => {
            if let Some(url) = &chat.aux.remote.qr_url {
                body = body.child(qr_white_card(url));
            }
            body = body.child(kit::note(&tr("remote.qr_confirm")));
        }
        QrState::Expired => {
            body = body.child(qr_masked(chat, weak, tr("remote.qr_timeout").to_string()))
        }
        QrState::Error(m) => body = body.child(qr_masked(chat, weak, friendly_qr_error(m))),
        // ④ 微信已返回但我方没写进状态：按出错蒙白处理
        QrState::Done { .. } if !has_token => {
            body = body.child(qr_masked(
                chat,
                weak,
                tr("remote.hint_create_failed").replace("{err}", &tr("remote.st_write_failed")),
            ))
        }
        QrState::Done { .. } => body = body.child(kit::note(&tr("remote.qr_created"))),
        QrState::Idle | QrState::Loading => body = body.child(kit::note(&tr("remote.qr_loading"))),
    }
    // 底部提示行：三态（未创建 / 微信侧出错 / 已创建）
    body = body.child(kit::note(&state_hint(chat, has_token)));
    let weak_close = weak.clone();
    crate::ui::overlay::big_card(
        tr("remote.panel_title"),
        None,
        body.into_any_element(),
        t,
        move |_w, cx| {
            let _ = weak_close.update(cx, |c, cx| {
                c.dlg.dialog = None;
                cx.notify();
            });
        },
    )
}

/// 弹窗底部提示：只有三态（002 §状态说明区，2026-10-24 修订）——
/// ①未创建 / ②微信侧出错 / 已创建（③④⑤⑥⑦全部归位为「已创建」）。
fn state_hint(chat: &Chat, has_token: bool) -> String {
    match &chat.aux.remote.qr {
        QrState::Error(m) => {
            let low = m.to_lowercase();
            if low.contains("timeout") || m.contains("超时") {
                // 超时只是创建/重建流程被打断：有 bot 归「已创建」，没有归「未创建」
                if has_token {
                    tr("remote.qr_created").to_string()
                } else {
                    tr("remote.hint_absent").to_string()
                }
            } else {
                // ② 微信侧出错（未装插件等）
                tr("remote.hint_wx_error").to_string()
            }
        }
        // ③④⑤⑥⑦：bot 已存在（含更新重建中/启停中，一切已创建形态）
        _ if has_token => tr("remote.qr_created").to_string(),
        // ① 未创建：pf-wxbot 不存在
        _ => tr("remote.hint_absent").to_string(),
    }
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
        "wx-qr-regen",
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

/// 二维码**实心方块**绘制。不用字符画：字形抗锯齿会在模块之间留细缝，
/// 手机容易解不出来（用户实测扫不上）。按行把连续深色段合并成一个方块，
/// 49x49 的网格约 600 个元素，不是 2401 个。
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

impl Chat {
    /// 打开微信线会话（三横菜单唯一入口；automation `content.view wx` /
    /// `panel.dock wx` 也落到这里）：确保微信线会话存在并作为**普通会话
    /// tab** 激活——桌面与微信同权同视图（2026-10-24 定夺）。未创建时自动
    /// 弹出二维码（002「插件自动展示二维码」）。
    pub(crate) fn open_wx(&mut self, cx: &mut gpui::Context<Self>) {
        self.dlg.market_menu = None;
        let st = wxprobe::state::load();
        let has_token = wxprobe::transport::require_token(&st).is_ok();
        // 线不存在：有 token 认领绑定的会话文件；没有则开专属草稿
        //（2026-10-24 定夺：微信线不再认领桌面活跃会话）
        if self.aux.remote.wx_session.is_none() {
            if has_token {
                if let Some(key) = wxprobe::state::get_str(&st, "session_key") {
                    self.aux.remote.set_wx_session(key);
                }
            }
            if self.aux.remote.wx_session.is_none() {
                if let Some(key) = self.new_draft_detached(cx) {
                    self.aux.remote.set_wx_session(key);
                }
            }
        }
        // 激活为普通会话 tab（文件 → 后台唤醒 + switch_to；草稿 → 直接
        // switch_to 池内 runtime）
        if let Some(key) = self.aux.remote.wx_session.clone() {
            if std::path::Path::new(&key).is_file() {
                if let Some(rt) = self.open_session_detached(std::path::PathBuf::from(&key), cx) {
                    self.switch_to(rt, cx);
                }
            } else if let Some(rt) = self.pool.runtimes.get(&key).cloned() {
                self.switch_to(rt, cx);
            }
        }
        // 未创建：自动展示二维码
        if !has_token {
            self.aux.remote.begin_qr();
            self.dlg.dialog = Some(crate::Dialog::WxQr);
        }
        cx.notify();
    }
}
