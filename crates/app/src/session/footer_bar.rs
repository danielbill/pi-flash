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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
/// **同时是渲染白名单**：只有这里登记的键才上屏（`bar()` 用 `continue` 过滤）。
/// 扩展的 `setStatus` 会塞进各种给 pi TUI footer 画的东西（`mcp` 的服务器计数、
/// `mcp-auth` 登录码、`freeflow` relay 状态、`goal` 进度…），对 PF 都是无落位的
/// 噪音；噪声源在扩展侧，PF 只能靠这张表守门。新增/恢复某个键 = 改本表 + 本条
/// 注释 + 文档 §4.2。
pub(crate) fn action_of(key: &str) -> Option<StatusAction> {
    match key {
        // pi-usage 的额度/余额行（点击 = /usage 菜单卡）
        "usage" => Some(StatusAction::Command("/usage")),
        // `mcp`：**故意不上屏**。pi-mcp-adapter 的 "0 servers enabled (1 disabled)"
        // 对 PF 是纯噪音——composer 功能栏已有 MCP 胶囊（server 图标 + `[n]` 已启用
        // 数，点开勾选菜单），比这行更准也更可点。
        _ => None,
    }
}

/// 可点项的悬停提示（与 `action_of` 同表维护）。
fn tip_key_of(key: &str) -> &'static str {
    match key {
        "usage" => "footerbar.tip.usage",
        _ => "",
    }
}

/// 034 状态带可见高度：渲染侧与 composer 底距共用同一个判定，避免两套
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
    // 读一次运行态：白名单项（key 字母序）+ agent 是否在跑（命令类禁用判定）。
    // 非白名单键（mcp / mcp-auth / freeflow / goal…）在这里就被滤掉 —— 带子上
    // 只出现白名单键；如果剩下的全是噪音，整条不占位（零高度）。
    let (items, running) = {
        let r = chat.rt().read(cx);
        let mut items: Vec<(String, String, StatusAction)> = r
            .ext_status
            .iter()
            .filter(|(_, text)| !text.is_empty())
            .filter_map(|(key, text)| action_of(key).map(|a| (key.clone(), text.clone(), a)))
            .collect();
        // pi footer 同口径：按 key 字母序（同一组项跨会话顺序稳定）
        items.sort_by(|a, b| a.0.cmp(&b.0));
        (items, r.agent_running)
    };
    if items.is_empty() {
        return None;
    }

    let mut row = div().flex().items_center().gap(px(PAD_X));
    for (key, text, action) in items {
        let blocked = running && matches!(action, StatusAction::Command(_));
        let color = if blocked { t.text_faint } else { t.text_muted };
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
        if blocked {
            // 禁用：opacity 0.5 + 无 pointer / 无 hover（§8），提示为什么
            el = el.opacity(0.5).with_tip("footerbar.tip.busy");
        } else {
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
                        this.footer_status_act(action, cx);
                    }),
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 渲染白名单就是动作表：没登记的键一律不上屏（mcp 是用户点名的噪音源）。
    #[test]
    fn whitelist_only_admits_action_keys() {
        assert!(matches!(
            action_of("usage"),
            Some(StatusAction::Command("/usage"))
        ));
        assert_eq!(action_of("mcp"), None, "MCP 计数行是噪音，不进带子");
        for noisy in ["mcp-auth", "freeflow", "goal", "cline-login", ""] {
            assert_eq!(action_of(noisy), None, "{noisy} 不该上屏");
        }
    }
}
