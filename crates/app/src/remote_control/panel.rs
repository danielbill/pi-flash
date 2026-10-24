//! 微信通信（060）：「微信线会话」已升格为**普通会话 tab**（桌面与微信
//! 同权同视图，2026-10-24 定夺），本文件只剩三件套——
//! 1. [`fn_bar_status`]：会话操作栏右侧信息/控制簇（仅活跃会话是微信线时）
//! 2. [`wx_qr_panel`]：二维码弹窗（Dialog::WxQr，dialogs.rs 调用）
//! 3. [`Chat::open_wx`]：入口（确保微信线会话存在并激活；未创建自动弹码）
//!
//! UI 语义收敛于 `pi-flash-plugins/docs/插件设计/002-微信通信.md`：
//!
//! - 页 = 复用对话页单独一个标签页，标签名【微信通话】(`remote.tab_title`)。
//! - 操作栏右侧从左到右三件：**微信 icon**（灰=未创建 / 绿=已创建并启用 /
//!   黄=已创建但停用，点击开弹窗）、**开关 + 说明文字**（启用/停用微信通话）、
//!   **刷新二维码钮**（refresh icon + 重新绑定）。
//! - 二维码弹窗：展示区三态（出码 / 已创建空框 / 超时蒙白，提示一律写在
//!   码方框内）+ 固定三步提示区。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{Theme, UNREAD, WARN, theme as T};
use crate::i18n::tr;
use crate::ui::kit;

use super::QrState;

/// 二维码方框边长：码 49 模块 × 8px = 392 + 白衬 12×2 = 416。三态共用同一
/// 方框，状态切换时弹窗不跳尺寸（002「所有提示信息显示在二维码方框中」）。
const QR_BOX: f32 = 416.;
/// 方框恒为白色 QR 衬底（与 [`qr_grid`] 的深色模块同一套固定色）。白底上的
/// 提示文字**不能跟主题色走**——深色主题里 `t.text` 是浅色，落在白底上直接
/// 消失，故提示/链接用固定深色。
const QR_SLOT_BG: u32 = 0xff_ffff;
const QR_SLOT_TEXT: u32 = 0x37_41_51;
const QR_SLOT_LINK: u32 = 0x1d_4e_d8;
const QR_SLOT_LINK_HOVER: u32 = 0x1e_40_af;

/// 会话操作栏右侧信息/控制簇（仅活跃会话是微信线时显示，`session_fn_bar`
/// 调用）。顺序按 002 定死，从左到右：
/// 1. 微信 icon —— 灰（未创建）/ 绿（已创建且启用）/ 黄（已创建但停用），
///    点击开二维码弹窗（未创建时顺带取码）；
/// 2. 开关 + 说明文字 —— 启用/停用微信通话（未创建时禁用，只能扫码创建）；
/// 3. 刷新二维码 —— refresh icon +「重新绑定」，强制换一张新码。
pub(crate) fn fn_bar_status(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> Option<gpui::AnyElement> {
    let is_wx_line = chat.aux.remote.wx_session.as_deref() == Some(chat.pool.active_key.as_str());
    if !is_wx_line {
        return None;
    }
    let t = T();
    let has_token = wxprobe::transport::require_token(&wxprobe::state::load()).is_ok();
    let running = chat.aux.remote.is_running();
    let weak = cx.entity().downgrade();

    // 微信 icon 三色（002 操作栏第 1 件）：灰=状态①未创建；绿=创建成功；
    // 黄=已创建但关停（on/off 的颜色出口，与开关同源）。
    let icon_color = match (has_token, running) {
        (false, _) => t.text_faint,
        (true, true) => UNREAD,
        (true, false) => WARN,
    };
    let icon_btn = {
        let weak = weak.clone();
        div()
            .id("sfbar-wx-icon")
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(
                MouseButton::Left,
                move |_: &gpui::MouseDownEvent, _w, cx| {
                    let has_token =
                        wxprobe::transport::require_token(&wxprobe::state::load()).is_ok();
                    let _ = weak.update(cx, |this, cx| {
                        // 未创建：自动取码（002 状态①）；已创建：只开弹窗，
                        // 框内给「重新绑定」提示，换码归第 3 件刷新钮。
                        if !has_token {
                            this.aux.remote.begin_qr();
                        }
                        this.dlg.dialog = Some(crate::Dialog::WxQr);
                        cx.notify();
                    });
                },
            )
            .child(crate::ui::icon("wechat", 16., icon_color))
            .into_any_element()
    };

    // 开关 + 说明文字（第 2 件）：未创建时禁用（唯一出路是扫码创建）。
    let on = has_token && running;
    let sw = kit::config_switch("wx-enable", &weak, on, !has_token, |c, cx| {
        let on = !c.aux.remote.is_running();
        c.aux.remote.set_enabled(on);
        cx.notify();
    });
    let label = if on {
        tr("remote.toggle_on")
    } else {
        tr("remote.toggle_off")
    };
    let sw_group = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(sw)
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(label)),
        );

    // 刷新二维码（第 3 件）：强制换新码 + 开弹窗看码（002「重新绑定」）。
    let refresh_btn = {
        let weak = weak.clone();
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
                move |_: &gpui::MouseDownEvent, _w, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.aux.remote.refresh_qr();
                        this.dlg.dialog = Some(crate::Dialog::WxQr);
                        cx.notify();
                    });
                },
            )
            .child(crate::ui::icon(
                "refresh",
                crate::ui::tokens::icon::MD,
                t.text_muted,
            ))
            .child(SharedString::from(tr("remote.rebind").to_string()))
            .into_any_element()
    };

    Some(
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .child(icon_btn)
            .child(sw_group)
            .child(refresh_btn)
            .into_any_element(),
    )
}

/// 二维码弹窗面板（`Dialog::WxQr`，dialogs.rs 用 dialog_shell 包壳）：
/// 紧凑居中小卡——header（标题+关闭）+ 二维码方框 + 底部固定三步提示。
///
/// 方框三态（002 §二维码弹窗）：
/// 1. 未创建（或已取到新码）→ 只显示二维码，框内**无**提示信息；
/// 2. 已创建 → 不给码，框内居中提示「【刷新二维码】/ 重新绑定PF微信机器人」；
/// 3. 超时（附扫码出错）→ 白蒙层遮掩旧码，框内「【刷新二维码】/ 扫描超时…」。
pub(crate) fn wx_qr_panel(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &Theme) -> gpui::Div {
    let has_token = wxprobe::transport::require_token(&wxprobe::state::load()).is_ok();
    // 在手的活性二维码：Ready 带 url；Scanned 不带（靠 qr_url 续命），
    // 手机确认前码必须留在屏幕上。
    let live = match &chat.aux.remote.qr {
        QrState::Ready { url } => Some(url.clone()),
        QrState::Scanned => chat.aux.remote.qr_url.clone(),
        _ => None,
    };
    let content: gpui::AnyElement = match &chat.aux.remote.qr {
        // 状态 1：出码，框内不放任何提示
        QrState::Ready { url } => qr_white_card(url),
        // 手机确认中的码留在屏幕上（`Scanned` 不带 url，靠 `qr_url` 续命）
        QrState::Scanned => match live.as_deref() {
            Some(url) => qr_white_card(url),
            None if has_token => qr_prompt_box(None, tr("remote.rebind_desc"), weak),
            None => qr_note_box(&tr("remote.qr_confirm")),
        },
        // 状态 3：超时 → 旧码上蒙白（已建过的机器人重建超时同样走这里：
        // 用户刚扫完一轮，最要紧的信息是「码过期了」，不是「已创建」）
        QrState::Expired => {
            qr_prompt_box(chat.aux.remote.qr_url.as_deref(), tr("remote.qr_timeout"), weak)
        }
        // 微信侧出错（②）：同蒙白口径，文案换成行动指引
        QrState::Error(m) => {
            let msg = friendly_qr_error(m);
            qr_prompt_box(chat.aux.remote.qr_url.as_deref(), &msg, weak)
        }
        // ④ 微信已返回但我方没写进状态：按出错蒙白处理
        QrState::Done { .. } if !has_token => {
            let msg =
                tr("remote.hint_create_failed").replace("{err}", &tr("remote.st_write_failed"));
            qr_prompt_box(None, &msg, weak)
        }
        // 状态 2：已创建（已建成功 / 刚开弹窗还没取新码）→ 空框 + 重新绑定提示
        QrState::Done { .. } | QrState::Idle | QrState::Loading if has_token => {
            qr_prompt_box(None, tr("remote.rebind_desc"), weak)
        }
        QrState::Done { .. } | QrState::Idle | QrState::Loading => {
            qr_note_box(&tr("remote.qr_loading"))
        }
    };
    // 紧凑小卡：header（标题 + 关闭）+ 居中 body（R_MODAL 10）
    let weak_close = weak.clone();
    let header = div()
        .h(px(36.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .pl(px(12.))
        .pr(px(6.))
        .bg(rgb(t.chrome))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x73)))
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.5))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(tr("remote.panel_title").to_string())),
        )
        .child(div().flex_1())
        .child(
            div()
                .id("wx-qr-close")
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(
                    MouseButton::Left,
                    move |_: &gpui::MouseDownEvent, _w, cx| {
                        let _ = weak_close.update(cx, |c, cx| {
                            c.dlg.dialog = None;
                            cx.notify();
                        });
                    },
                )
                .child(crate::ui::icon("close", 14., t.text_dim)),
        );
    let body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .p(px(16.))
        .child(content)
        // 底部三步提示（固定文案，左对齐，每行一条）
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(tr("remote.qr_steps").lines().map(kit::note)),
        );
    div()
        .w(px(460.))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(10.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .child(header)
        .child(body)
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

/// 码方框内的【刷新二维码】按钮（002 三态文案的第一行，方框内唯一可点击
/// 件）：点了换一张新码。画成实心边框按钮而不是裸链接——用户定的形制
///（白底框 + 蓝字蓝边，与码方框的白衬同一套固定色，不跟主题走）。
fn qr_refresh_link(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let weak = weak.clone();
    div()
        .id("wx-qr-refresh")
        .flex()
        .items_center()
        .justify_center()
        .h(px(28.))
        .px(px(12.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(QR_SLOT_LINK))
        .cursor_pointer()
        .text_size(crate::appearance::ui_size(12.5))
        .text_color(rgb(QR_SLOT_LINK))
        .hover(|s| s.bg(rgb(0xef_f6_ff)).text_color(rgb(QR_SLOT_LINK_HOVER)))
        .on_mouse_down(
            MouseButton::Left,
            move |_: &gpui::MouseDownEvent, _w, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.aux.remote.refresh_qr();
                    cx.notify();
                });
            },
        )
        .child(SharedString::from(format!(
            "【{}】",
            tr("remote.refresh_qr")
        )))
        .into_any_element()
}

/// 提示态方框：白衬（`old` 有值就先铺旧码）+ 白蒙层 + 居中【刷新二维码】
/// 链接与消息（002：所有提示信息显示在二维码方框中）。
fn qr_prompt_box(old: Option<&str>, message: &str, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let mut wrap = div().relative().w(px(QR_BOX)).h(px(QR_BOX));
    if let Some(url) = old {
        wrap = wrap.child(qr_white_card(url));
    }
    let overlay = div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .rounded(px(12.))
        .bg(rgb(QR_SLOT_BG))
        .child(qr_refresh_link(weak))
        .child(
            div()
                .max_w(px(QR_BOX - 48.))
                .text_center()
                .text_size(crate::appearance::ui_size(12.5))
                .text_color(rgb(QR_SLOT_TEXT))
                .child(SharedString::from(message.to_string())),
        );
    wrap.child(overlay).into_any_element()
}

/// 无提示的等码方框（Loading / 已扫码待确认）：同尺寸白框，居中一行说明。
fn qr_note_box(note: &str) -> gpui::AnyElement {
    div()
        .w(px(QR_BOX))
        .h(px(QR_BOX))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(12.))
        .bg(rgb(QR_SLOT_BG))
        .text_size(crate::appearance::ui_size(12.5))
        .text_color(rgb(QR_SLOT_TEXT))
        .child(SharedString::from(note.to_string()))
        .into_any_element()
}

/// 二维码白卡：白色圆角衬底（R_SHEET 12），内装实心方块码。
fn qr_white_card(url: &str) -> gpui::AnyElement {
    div()
        .rounded(px(12.))
        .bg(rgb(QR_SLOT_BG))
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
        // switch_to 池内 runtime）。进微信线前把桌面活跃记入 desktop_key：
        // 微信是单开 tab，topbar 第一格继续显示桌面会话（2026-10-24 定夺）
        if let Some(key) = self.aux.remote.wx_session.clone() {
            if self.pool.active_key != key {
                self.aux.remote.desktop_key = Some(self.pool.active_key.clone());
            }
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
