//! 微信通信（060）：「微信线会话」已升格为**普通会话 tab**（桌面与微信
//! 同权同视图，2026-10-24 定夺）。会话页**不复制**——它走 `session::template`
//! 模版页，本文件以 [`WX_SKIN`] 注入微信线的全部差异：
//! 1. [`wx_fn_bar`]：操作栏右侧注入簇（微信 icon 三态 / 启停开关 + 说明 /
//!    新建会话 / 重新绑定）
//! 2. [`wx_qr_layer`] / [`wx_qr_panel`]：绑定层（Dialog::WxQr，render.rs
//!    挂 content-col）与二维码卡
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

use gpui::{MouseButton, Rgba, SharedString, div, prelude::*, px, rgb, rgba};

use pi_link::protocol::Command;

use std::path::PathBuf;

use crate::Chat;
use crate::appearance::theme::{Theme, UNREAD, WARN, theme as T};
use crate::i18n::tr;
use crate::ui::kit;

use super::{QrState, TASK_RUNNING, wx_lang};

/// 操作栏启停开关是否展示（2026-10-25 用户定夺：**隐藏**）。开关语义 =
/// 开/关【微信通话】标签（002：开 = on，关 = off），功能不删：自动化
/// `wx.enable` / 微信端 `/启` `/停` 走同一条开合标签的路。
const SHOW_WX_SWITCH: bool = false;

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

/// 【微信通话】功能页（ContentView::Wx，002 §1–§3）：从管理到内部自管
/// 会话的完整页，按放行条件分段：
/// - **Bind**（无 token）：绑定页 = 白底内容区 + 二维码卡（W2 定夺视觉
///   原样：去标题栏、尺寸不变、背景加白覆盖至 topbar）；泵见 Done 自动
///   切下一段
/// - **Setup**（有 token、项目/模型未齐）：新会话页 wx 变体（欢迎语 +
///   选项目/模型；齐了由泵自动放行切进会话）
/// - **Chat**：不在本页——就绪即切回 Chat 视图的微信线会话（open_wx/泵），
///   wx_tab 依旧高亮
pub(crate) fn page(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let has_token = chat.aux.remote.has_token();
    // 002：服务侧会话过期 → 自动返回绑定页（与未绑定同段，码卡下带标注）
    if !has_token || chat.aux.remote.health().expired() {
        // Bind 段 = 绑定票面（与强制模态层同一个构造器，002「背景覆盖至
        // topbar」，颜色跟主题）
        return wx_bind_surface(chat, weak, t);
    }
    // Setup 段：走会话页模版的新会话页皮肤（微信线：插件欢迎语 + 选项目/
    // 模型，不挂扩展更新卡）。composer 焦点态同 main_column 口径
    let input_focused = chat
        .composer
        .composer
        .as_ref()
        .map(|c| c.read(cx).focus_handle_in(cx).is_focused(window))
        .unwrap_or(false);
    chat.composer.input_focused = input_focused;
    crate::session::new_session::page(
        chat,
        weak,
        &WX_SKIN,
        false,
        input_focused,
        f32::from(window.viewport_size().height),
        cx,
    )
    .into_any_element()
}

/// 微信线皮肤（会话页模版注入面，见 `session::template`）：
/// - 空会话仍留在会话页（设置页由功能页 ContentView::Wx 承担）；
/// - 新会话页欢迎语换插件注入语、不挂 Pi 扩展更新卡；
/// - 操作栏右侧注入微信专属簇（[`wx_fn_bar`]）。
pub(crate) static WX_SKIN: crate::session::template::SessionPageSkin =
    crate::session::template::SessionPageSkin {
        keep_on_empty: true,
        greeting_key: "remote.page_greeting",
        plugin_updates: false,
        fn_bar: Some(wx_fn_bar),
    };

/// 微信线操作栏簇（模版注入点 ①，002 定序，从左到右）：
/// 1. 微信 icon —— 灰（未创建）/ 绿（已创建并启用）/ 黄（已创建但停用），
///    点击开二维码弹窗（未创建时顺带取码）；
/// 2. 开关 + 说明文字 —— 启用/停用微信通话（未创建时禁用，只能扫码创建）；
/// 3. 新建会话 —— 丢弃当前会话重建（= 微信端 `/新建` 同语义）；
/// 4. 刷新二维码 —— refresh icon +「重新绑定」，强制换一张新码。
pub(crate) fn wx_fn_bar(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    // 镜像读（泵一拍刷一次）：渲染路径不碰盘
    let has_token = chat.aux.remote.has_token();
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
        div() // ui-raw: ok 002 微信操作栏图标钮固定形制（060 发版隐藏模块，尺寸随设计稿钉死）
            .id("sfbar-wx-icon")
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded(px(6.))
            // 微信 icon 单独再让 10px：它无 px8 内边距，与右侧钮的
            // 视觉间隙 = gap4 + 钮px8 = 12，补 10 → 22
            .mr(px(10.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(
                MouseButton::Left,
                move |_: &gpui::MouseDownEvent, _w, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        let has_token = this.aux.remote.has_token();
                        // 未创建：自动取码 + 绑定弹窗（002 状态①）；已创建：
                        // 回微信通话页（标签关闭后的重开入口，2026-10-25
                        // 用户定夺「标签必须可关」）；换码归第 4 件刷新钮。
                        if !has_token {
                            this.aux.remote.begin_qr();
                            this.dlg.dialog = Some(crate::Dialog::WxQr);
                        } else {
                            this.open_wx(cx);
                        }
                        cx.notify();
                    });
                },
            )
            .child(crate::ui::icon(
                "wechat",
                22.,
                icon_color,
            ))
            .into_any_element()
    };

    // 开关 + 说明文字（第 2 件，002 §1）：动作式文案——on 时点之则关停
    // 服务，off 时点之则启用；未创建时禁用（只能扫码创建）。
    //
    // **暂不展示**（2026-10-25 用户定夺）：开关 = 标签开合，功能不删。
    // 恢复展示 = `SHOW_WX_SWITCH` 改 true（隐藏态零渲染成本）。
    let sw_group = SHOW_WX_SWITCH.then(|| {
        let on = has_token && running;
        let sw = kit::config_switch("wx-enable", &weak, on, !has_token, |c, cx| {
            // 开关 = 标签开合（002：开 = on，关 = off）
            if c.wx_tab_open() {
                c.close_wx_tab(cx);
            } else {
                c.open_wx(cx);
            }
            cx.notify();
        });
        let label = if on {
            tr("remote.toggle_on")
        } else {
            tr("remote.toggle_off")
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(sw)
            .child(
                div()
                    .text_size(crate::appearance::ui_size(11.5))
                    .text_color(rgb(t.text_muted))
                    .child(SharedString::from(label)),
            )
    });
    // 新建会话（第 3 件，W1）：002 §3——弹确认框「丢弃当前会话创建新会话？」，
    // 确认后走 wx_rotate_session（= 微信 /新建 同语义）。运行中不开确认框，
    // 回执与微信命令同文案（防串线，ZCode botsService.ts:6039 同款）。
    let new_session_btn = {
        let weak = weak.clone();
        let running = chat.aux.remote.active_running;
        crate::session::fn_bar_btn(
            "sfbar-wx-new",
            "bot-message-square",
            tr("remote.new_session"),
            false,
            move |_: &gpui::MouseDownEvent, _w, cx: &mut gpui::App| {
                let _ = weak.update(cx, |this, cx| {
                    if running {
                        let lang = wx_lang();
                        this.aux
                            .remote
                            .send(&wxprobe::format::messages::t(lang, TASK_RUNNING));
                    } else {
                        this.dlg.dialog = Some(crate::Dialog::WxNewSession);
                    }
                    cx.notify();
                });
            },
        )
    };

    // 重新绑定（第 4 件）：强制换新码 + 开弹窗看码（002 模块跳转 ④ →
    // view：bind_wxbot，即 Dialog::WxQr 白底模态）。
    let refresh_btn = {
        let weak = weak.clone();
        crate::session::fn_bar_btn(
            "sfbar-wx-qr",
            "refresh",
            tr("remote.rebind"),
            false,
            move |_: &gpui::MouseDownEvent, _w, cx: &mut gpui::App| {
                let _ = weak.update(cx, |this, cx| {
                    this.aux.remote.refresh_qr();
                    this.dlg.dialog = Some(crate::Dialog::WxQr);
                    cx.notify();
                });
            },
        )
    };

    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_end()
        // 簇内间距 20px 视觉。**账要算上钮自带的 px8**：视觉间隙 =
        // gap + 8 + 8，故 gap = 20-16 = 4（不是 20，写 20 会得到 36 的
        // 视觉间距——已踩过）
        .gap(px(4.))
        // 重新绑定视觉右边距 20px = 整行 pr8 + 钮 px8 + 这里 4
        .pr(px(4.))
        .child(icon_btn)
        .children(sw_group)
        .child(new_session_btn)
        .child(refresh_btn)
        .into_any_element()
}

/// 二维码卡（[`wx_qr_layer`] 的居中内容）：无标题栏——二维码方框 + 底部
/// 固定三步提示。方框三态（002 §二维码弹窗）：
/// 1. 未创建（或已取到新码）→ 只显示二维码，框内**无**提示信息；
/// 2. 已创建 → 不给码，框内居中提示「【刷新二维码】/ 重新绑定PF微信机器人」；
/// 3. 超时（附扫码出错）→ 白蒙层遮掩旧码，框内「【刷新二维码】/ 扫描超时…」。

/// 绑定层蒙层色（主题跟随）：主题底色 + 91% 不透明度。浅色主题 ≈ 白蒙层
/// （002 原稿），dark 主题下是暗蒙层——底稿的白色在 dark 下刺眼（2026-10-25
/// 用户定夺「背景色要跟着主题走」）。二维码卡自身保持纯白（扫码需要）。
fn veil(t: &Theme) -> Rgba {
    rgba((t.bg << 8) | 0xE8)
}

pub(crate) fn wx_qr_panel(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &Theme) -> gpui::Div {
    let has_token = chat.aux.remote.has_token();
    let qr = &chat.aux.remote.qr;
    let content: gpui::AnyElement = match qr {
        // 状态 1：出码，框内不放任何提示
        QrState::Ready { url } => qr_white_card(url),
        // 手机确认中：码留在屏幕上（url 就在状态里，不靠旁路字段续命）
        QrState::Scanned { url } => qr_white_card(url),
        // 状态 3：超时 → 旧码上蒙白（已建过的机器人重建超时同样走这里：
        // 用户刚扫完一轮，最要紧的信息是「码过期了」，不是「已创建」）
        QrState::Expired { url } => qr_prompt_box(url.as_deref(), tr("remote.qr_timeout"), weak),
        // 微信侧出错（②）：同蒙白口径，文案换成行动指引
        QrState::Error { msg, url } => {
            let msg = friendly_qr_error(msg);
            qr_prompt_box(url.as_deref(), &msg, weak)
        }
        // ④ 微信已返回但我方没写进状态：按出错蒙白处理
        QrState::Done { .. } if !has_token => {
            let msg =
                tr("remote.hint_create_failed").replace("{err}", &tr("remote.st_write_failed"));
            qr_prompt_box(None, &msg, weak)
        }
        // 状态 2：已创建（已建成功 / 刚开弹窗还没取新码）→ 空框 + 重新绑定提示
        QrState::Done { .. } | QrState::Idle | QrState::Loading if has_token => {
            qr_prompt_box(None, rebind_hint(chat), weak)
        }
        QrState::Done { .. } | QrState::Idle | QrState::Loading => {
            qr_note_box(&tr("remote.qr_loading"))
        }
    };
    let body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .p(px(16.))
        .child(content)
        // 002：服务侧会话过期 → 二维码下方标注解释
        .children(chat.aux.remote.health().expired().then(|| {
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(t.danger))
                .child(SharedString::from(tr("remote.wx_expired").to_string()))
        }))
        // 底部提示（固定文案，左对齐，每行一条）
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
        .child(body)
}

/// 002-微信通信 绑定层（W2 定夺）：白蒙层覆盖内容区（topbar 以下、含会话
/// 操作栏），二维码卡原尺寸居中。**无标题栏、无关闭钮、外点/ESC 均不关**
/// ——不扫码绑定就不能进行下一步操作；唯一撤层路径是扫码成功（泵见
/// `QrState::Done` 自动清 `dlg.dialog`）。
pub(crate) fn wx_qr_layer(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &Theme,
) -> gpui::Div {
    crate::ui::overlay::layer_blocking(&chat.dlg.dialog_focus)
        // topbar 不盖住（用户定夺「覆盖至 topbar」）；content-col 是
        // relative 容器，40px = titlebar::HEIGHT
        .top(px(crate::titlebar::HEIGHT))
        .bg(veil(t)) // 蒙层跟主题（002 原稿白蒙层仅浅色主题适用）
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::stop_click(wx_qr_panel(chat, weak, t)))
}

/// 绑定票面：蒙层（跟主题）+ 居中二维码卡。**一份**——功能页 Bind 段（内容区
/// 内）与强制模态层（Dialog::WxQr，挂 content-col）都用它，别各画一份布局。
fn wx_bind_surface(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &Theme,
) -> gpui::AnyElement {
    div()
        .id("wx-bind-surface")
        .size_full()
        .bg(veil(t))
        .flex()
        .items_center()
        .justify_center()
        .child(wx_qr_panel(chat, weak, t))
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

/// 状态 2 的框内说明：已绑定时提示「重新绑定 PF 微信机器人」。
/// （-14 游标失配由 wxprobe 后台自愈，**不在这里报**——本模块无「过期」概念）
fn rebind_hint(_chat: &Chat) -> &'static str {
    tr("remote.rebind_desc")
}

/// 码方框内的【刷新二维码】按钮（002 三态文案的第一行，方框内唯一可点击
/// 件）：点了换一张新码。画成实心边框按钮而不是裸链接——用户定的形制
///（白底框 + 蓝字蓝边，与码方框的白衬同一套固定色，不跟主题走）。
fn qr_refresh_link(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let weak = weak.clone();
    div() // ui-raw: ok 002 码方框内【刷新二维码】边框钮——用户定案白底蓝字蓝边不跟主题
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
fn qr_prompt_box(
    old: Option<&str>,
    message: &str,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
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
    /// 微信通话入口（三横菜单 / topbar 标签 / 操作栏 icon / 自动化 / 启动恢复
    /// 共用）。只做两件事：**把入口摆出来**（幂等展开标签）+ **把内容导航到
    /// 微信自己的位置**（未绑定 → 绑定页；已绑定 → 微信线会话）。渠道起停
    /// 与它无关——那是泵里的服务状态机（开关 on 且窗口激活才在跑）。
    pub(crate) fn open_wx(&mut self, cx: &mut gpui::Context<Self>) {
        // 发版排除总开关（dev-flag `wx` 默认关）：入口全隐时自动化兜底也
        // 在这里吃掉
        if !crate::services::workspace::wx_enabled() {
            return;
        }
        self.dlg.market_menu = None;
        self.wx_open_tab();
        self.wx_goto_front(cx);
    }

    /// 微信前台落地（002 模块跳转）：未绑定 / 服务侧会话过期 → 绑定页（自动
    /// 出码）；否则微信线会话页；线建不出来（独立工作目录未定）→ 设置会话页。
    fn wx_goto_front(&mut self, cx: &mut gpui::Context<Self>) {
        if !self.aux.remote.snap.has_token() || self.aux.remote.health().expired() {
            self.aux.remote.begin_qr();
            self.set_content_view(crate::ContentView::Wx);
            cx.notify();
            return;
        }
        self.wx_ensure_session(cx);
        let Some(key) = self.aux.remote.line.key.clone() else {
            self.set_content_view(crate::ContentView::Wx);
            cx.notify();
            return;
        };
        if std::path::Path::new(&key).is_file() {
            // **必须用 `_with(wx_cwd)`**：影子会话被过滤清单藏在会话列表外，
            // 通用 `open_session_detached` 的 cwd 兜底会落到桌面 cwd——微信
            // 线就「工作在错误项目里」（2026-10-25 报障：一直在 pi_work）。
            let path = std::path::PathBuf::from(&key);
            let opened = match self.aux.remote.wx_cwd().cloned() {
                Some(cwd) => self.open_session_detached_with(path, cwd, cx),
                None => self.open_session_detached(path, cx),
            };
            if let Some(rt) = opened {
                self.wx_adopt_line_cwd(&rt, cx);
                self.switch_to(rt, cx);
            }
        } else if let Some(rt) = self.pool.runtimes.get(&key).cloned() {
            self.switch_to(rt, cx);
        }
        self.set_content_view(crate::ContentView::Chat);
        cx.notify();
    }

    /// 关【微信通话】标签（× / 中键）：收起标签、收前台、收专属模态、落盘。
    /// 服务与影子会话**不动**——关的只是这个「窗口」。
    ///
    /// 关标签 = off：微信内容（功能页 / 微信会话）必须一起收掉，不留幻影。
    pub(crate) fn close_wx_tab(&mut self, cx: &mut gpui::Context<Self>) {
        let front = self.wx_front_active();
        self.aux.remote.tab_open = false;
        // 标签与它的弹窗一体（2026-10-25 定夺）：微信专属模态（二维码 /
        // 新建会话确认）归微信线上下文，标签一关就跟着收——否则二维码盖在
        // 桌面会话上、功能入口却没了（真机踩到）
        if matches!(
            self.dlg.dialog,
            Some(crate::Dialog::WxQr) | Some(crate::Dialog::WxNewSession)
        ) {
            self.dlg.dialog = None;
        }
        if front {
            self.wx_leave_front(cx); // 内含 set_content_view + persist_tabs
        } else {
            self.persist_tabs();
        }
        cx.notify();
    }

    /// 关闭微信内容时收回前台（标签与内容一体，不许「标签关了、微信会话页
    /// 还挂着」）。去处 = 当前项目「最后打开的会话」；没有会话/文件没了 →
    /// [`Chat::landing_session`] 保底。微信会话与服务本身不动。
    pub(crate) fn wx_leave_front(&mut self, cx: &mut gpui::Context<Self>) {
        let target = self
            .ws
            .cwd
            .as_ref()
            .and_then(|cwd| crate::services::workspace::get_last_open(&cwd.to_string_lossy()))
            .and_then(|k| {
            if let Some(rt) = self.pool.runtimes.get(&k).cloned() {
                Some(rt)
            } else if std::path::Path::new(&k).is_file() {
                self.open_session_detached(std::path::PathBuf::from(&k), cx)
            } else {
                None
            }
        });
        // 落地保底：桌面会话不在了也要有地方去（否则活跃位留在微信线 =
        // 「新会话」标题 + 微信内容的幻影页）
        let rt = match target {
            Some(rt) => Some(rt),
            None => self.landing_session(cx),
        };
        if let Some(rt) = rt {
            self.switch_to(rt, cx);
        }
        // 无条件落盘：`switch_to` 只在跨视图时才经 set_content_view 顺手
        // persist——「Chat 视图 + 微信线会话」关标签时前台视图没变，
        // 不补这一笔，标签开合态就没进过盘（重启必复开，真机踩坑）
        self.persist_tabs();
    }

    /// 微信线会话名：`pf-wxbot-` + 15 位时间戳（`YYMMDDHHMMSS` + 3 位毫秒）。
    pub(crate) fn wx_session_name() -> String {
        format!(
            "pf-wxbot-{}",
            chrono::Local::now().format("%y%m%d%H%M%S%3f")
        )
    }

    /// 渠道（重新）连上：老影子会话下线 + 删除，新开一条并命名、注入过滤清单。
    /// 桌面会话与项目完全不受影响（新会话是 detached 草稿）。
    pub(crate) fn wx_rotate_session(&mut self, cx: &mut gpui::Context<Self>) {
        let old = self.aux.remote.line.key.clone();
        let old_active = old.as_deref() == Some(self.pool.active_key.as_str());
        if let Some(key) = old {
            self.forget_session(&key, cx);
        }
        let Some(new_key) = self.wx_new_session(cx) else {
            return; // 无项目态：等有工作区再建（open_wx 也会兜底）
        };
        // 老会话正被看着：焦点挪到新会话（active_key 已指向被删的旧 runtime，
        // 不挪的话下一次 rt() 会拿到空）
        if old_active {
            if let Some(rt) = self.pool.runtimes.get(&new_key).cloned() {
                self.switch_to(rt, cx);
            }
        }
        self.refresh_sessions();
        cx.notify();
    }

    /// 002 §6 [新建会话]（确认后）：**清空 `wx_cwd` + 清微信线指针（含状态
    /// 文件里的 `session_key`，防重启复活）**，跳转[设置会话]页。不现场建
    /// 会话——用户重选项目/模型后由 `wx_after_setup_change` 建新线。
    /// 旧影子会话下线不删文件（口径同轮换，前缀过滤挡住）。
    pub(crate) fn wx_new_session_reset(&mut self, cx: &mut gpui::Context<Self>) {
        let old = self.aux.remote.line.key.clone();
        let old_active = old.as_deref() == Some(self.pool.active_key.as_str());
        if let Some(key) = old {
            self.forget_session(&key, cx);
        }
        self.aux.remote.clear_wx_line();
        self.aux.remote.set_wx_cwd(None); // 持久化一并清（重启不再恢复旧目录）
        // 焦点在被删的影子会话上：挪到落地会话，避免 rt() 悬空
        if old_active {
            if let Some(rt) = self.landing_session(cx) {
                self.switch_to(rt, cx);
            }
        }
        self.aux.remote.tab_open = true;
        // 走 set_content_view：顺带 persist_tabs——裸写字段不落盘，重启后
        // tab 恢复读到 switch_to 刚落的 Chat，就会「蹿回桌面会话对话页」
        self.set_content_view(crate::ContentView::Wx); // [设置会话] 页
        self.refresh_sessions();
        cx.notify();
    }

    /// 建一条新的影子会话（`pf-wxbot-<ts>`）：detached 草稿 + pi 侧命名 +
    /// 过滤清单注入。返回 key；无工作区时 `None`。
    /// 设置会话页选项目（002 §2，用户定夺）：只写微信**独立工作目录**
    /// `wx_cwd`，禁止联动桌面 `cwd`；目录变了 → 轮换新影子会话（同目录
    /// 只落归属不打断当前会话），Setup 页上下文把活跃会话切到微信线
    /// （composer/模型 pill 才编辑微信线），模型承接不再丢失。
    pub(crate) fn wx_pick_project(&mut self, path: PathBuf, cx: &mut gpui::Context<Self>) {
        let inherit = self.rt().read(cx).default_model.clone();
        self.aux.remote.set_wx_cwd(Some(path.clone()));
        let cur = self.aux.remote.line.key.as_ref().and_then(|key| {
            self.pool.runtimes.get(key).map(|rt| rt.read(cx).cwd.clone())
        });
        let changed = !cur
            .as_ref()
            .is_some_and(|c| crate::services::workspace::same_path(c, &path));
        if changed {
            self.wx_rotate_session(cx);
        }
        if let Some(key) = self.aux.remote.line.key.clone() {
            let in_setup = self.layout.content_view == crate::ContentView::Wx;
            if in_setup {
                if let Some(rt) = self.pool.runtimes.get(&key).cloned() {
                    // switch_to 会把内容区带回 Chat 视图：微信线空草稿渲染
                    // 的正是欢迎（设置会话）页，视觉不变、归属已切到微信线
                    self.switch_to(rt, cx);
                }
            }
            if let Some(m) = inherit {
                if let Some(rt) = self.pool.runtimes.get(&key) {
                    rt.update(cx, |r, _| r.default_model = Some(m));
                }
            }
        }
        // 设置齐（wx_cwd + 模型）→ 立即建会话并跳聊天页（不靠桌面消息驱动）
        self.wx_after_setup_change(cx);
        cx.notify();
    }

    pub(crate) fn wx_new_session(&mut self, cx: &mut gpui::Context<Self>) -> Option<String> {
        let name = Self::wx_session_name();
        let key = self.new_draft_detached(cx)?;
        if let Some(rt) = self.pool.runtimes.get(&key) {
            rt.read(cx)
                .rpc(&Command::SetSessionName { name: name.clone() });
        }
        self.aux.remote.line.name = Some(name.clone());
        self.aux.remote.set_wx_line(key.clone());
        pi_link::hidden_sessions::set_owned(
            pi_link::hidden_sessions::OWNER_WX,
            Some(&name),
            None,
            Some("pf-wxbot-"),
        );
        Some(key)
    }
}
