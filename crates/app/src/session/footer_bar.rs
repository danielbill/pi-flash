//! 034 会话区底部状态栏（模块代码 sessionStatusBar）：content-col 最底部
//! 28px chrome 带，承载 pi 扩展 `ctx.ui.setStatus(key, text)` 申请的状态项。
//!
//! - 数据面：`SessionRuntime::ext_status`（per-session；pi 换会话/重载会清，
//!   放 Chat 上会串味）。
//! - 可点项点击 = **执行命令**（如 `/usage`），走与 composer 手打完全相同的
//!   发送漏斗（`SessionRuntime::send_command`）——不新写一套发送逻辑。
//! - 动作表在 PF 侧维护（pi 的 RPC `setStatus` 只有 key + text，没有动作字段），
//!   新增动作要改 `action_of` 并回写 `docs/模块设计/034-会话区底部状态栏.md` §4.2。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::{border_alpha, theme as T};
use crate::ui::tooltip::WithTip;

/// 状态带高度（034 §3.1：inputpanel 正下方一条 28px）。
pub(crate) const BAR_H: f32 = 28.;

/// 项内左右内衬（SP6）+ 内容行左右留白（SP6）。
const PAD_X: f32 = 12.;

/// 状态项动作（034 §4.2）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusAction {
    /// 执行一条斜杠命令（与 composer 手打同一漏斗）
    Command(&'static str),
    /// 打开设置页（tab 索引见 `settings::TAB_*`）
    Settings(u8),
}

impl StatusAction {
    /// 快照出口用的稳定标签（pif-ui 断言）。
    pub(crate) fn tag(self) -> &'static str {
        match self {
            StatusAction::Command(cmd) => cmd,
            StatusAction::Settings(_) => "settings",
        }
    }
}

/// key → 动作的唯一映射（静态 match，渲染期零分配）。
///
/// 未登记的键（`mcp-auth` / `freeflow` / `goal` / `cline-login`…）一律纯文本：
/// 我们不知道点下去该干什么，就不给点击语。
pub(crate) fn action_of(key: &str) -> Option<StatusAction> {
    match key {
        "usage" => Some(StatusAction::Command("/usage")),
        "mcp" => Some(StatusAction::Settings(crate::settings::TAB_MCP)),
        _ => None,
    }
}

/// 可点项的悬停提示（与 `action_of` 同表维护）。
fn tip_key_of(key: &str) -> &'static str {
    match key {
        "usage" => "footerbar.tip.usage",
        "mcp" => "footerbar.tip.mcp",
        _ => "",
    }
}

// 本次要补上：旧的 ext_status_row 已删，本模块声明提前（路径同下）。
// 034 状态带可见高度：渲染侧与 composer 底距共用同一个判定，避免两套
// “到底有没有条”的真相（switch 开 + 有非空文本项）。
pub(crate) fn will_show(chat: &Chat, cx: &gpui::App) -> bool {
    if !crate::services::workspace::session_status_bar_on() {
        return false;
    }
    chat.pool
        .runtimes
        .get(&chat.pool.active_key)
        .map(|rt| {
            rt.read(cx)
                .ext_status
                .iter()
                .any(|(_, text)| !text.is_empty())
        })
        .unwrap_or(false)
}

/// 状态带本体。None = 整条不渲染（不占 28px，也不建 element）。
///
/// 显隐四档（034 §3.4）：开关关 / 无项 / 全部文本空 / 新会话页（调用方不挂）。
pub(crate) fn bar(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> Option<gpui::AnyElement> {
    if !crate::services::workspace::session_status_bar_on() {
        return None;
    }
    let t = T();
    // 读一次运行态：items（key 字母序）+ agent 是否在跑（命令类禁用判定）
    let (items, running) = {
        let r = chat.rt().read(cx);
        if r.ext_status.is_empty() {
            return None;
        }
        let mut items: Vec<(String, String)> = r
            .ext_status
            .iter()
            .filter(|(_, text)| !text.is_empty())
            .cloned()
            .collect();
        // pi footer 同口径：按 key 字母序（同一组项跨会话顺序稳定）
        items.sort_by(|a, b| a.0.cmp(&b.0));
        (items, r.agent_running)
    };
    if items.is_empty() {
        return None;
    }

    let mut row = div().flex().items_center().gap(px(PAD_X));
    for (key, text) in items {
        let action = action_of(&key);
        let blocked = running && matches!(action, Some(StatusAction::Command(_)));
        let color = match action {
            None => t.text_faint,
            Some(_) if blocked => t.text_faint,
            Some(_) => t.text_muted,
        };
        let mut el = div()
            .id(SharedString::from(format!("ext-status-{key}")))
            .flex()
            .items_center()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(5.)) // R_INLINE：被塞进行里的散件
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(color))
            .child(SharedString::from(text));
        match action {
            // 禁用：opacity 0.5 + 无 pointer / 无 hover（§8），提示为什么
            Some(_) if blocked => {
                el = el.opacity(0.5).with_tip("footerbar.tip.busy");
            }
            Some(act) => {
                let key_tip = tip_key_of(&key);
                if !key_tip.is_empty() {
                    el = el.with_tip(key_tip);
                }
                el = el
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text_muted)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                            this.footer_status_act(act, cx);
                        }),
                    );
            }
            None => {}
        }
        row = row.child(el);
    }

    Some(
        div()
            .id("session-status-bar")
            .h(px(BAR_H))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .bg(rgb(t.chrome))
            // 上沿细线：§5.12 分隔线唯一口径（常驻 1px）
            .border_t_1()
            .border_color(gpui::rgba(border_alpha(t, 0x99)))
            .child(
                div()
                    .w_full()
                    .px(px(PAD_X))
                    .flex()
                    .items_center()
                    // 会话槽（本期空）+ 扩展槽右对齐（034 §3.3）
                    .child(div().flex_1())
                    .child(row),
            )
            .into_any_element(),
    )
}

impl Chat {
    /// 状态项点击（034 §5.1）：命令类走与 composer 同一条发送漏斗；
    /// 设置类走既有设置页打开链路（纯 UI 跳转，不动会话状态）。
    pub(crate) fn footer_status_act(&mut self, action: StatusAction, cx: &mut gpui::Context<Chat>) {
        match action {
            StatusAction::Command(cmd) => {
                self.rt().update(cx, |r, cx| r.send_command(cmd, cx));
            }
            StatusAction::Settings(tab) => self.open_settings(tab, cx),
        }
    }

    /// 显示开关翻转（034 §2）：落盘 ui_state 后整条重建。
    pub(crate) fn toggle_session_status_bar(&mut self, cx: &mut gpui::Context<Chat>) {
        let on = !crate::services::workspace::session_status_bar_on();
        crate::services::workspace::set_session_status_bar(on);
        cx.notify();
    }
}
