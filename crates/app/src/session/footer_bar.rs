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
use crate::appearance::theme::theme as T;
use crate::ui::tooltip::WithTip;

/// 状态带高度（034 §3.1：inputpanel 正下方一条 28px）。
pub(crate) const BAR_H: f32 = 28.;

/// 状态项动作（034 §4.2）。当前只有「执行命令」一类；将来若加面板类
/// （如某个扩展设置页），在这里补 variant 并同步 `footer_status_act`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StatusAction {
    /// 执行一条斜杠命令（与 composer 手打同一漏斗）
    Command(&'static str),
}

impl StatusAction {
    /// 快照出口用的稳定标签（pif-ui 断言）。
    pub(crate) fn tag(self) -> &'static str {
        match self {
            StatusAction::Command(cmd) => cmd,
        }
    }
}

/// key → 动作的映射（静态 match，渲染期零分配）。
///
/// **只是"可不可点"**，不管准入：状态带默认显示**全部**扩展状态项（Pi 老用户
/// 默认需求），无动作的键 = 灰字纯文本。想藏某一行是个体偏好，走逐键静音
/// （`ui_state.ext_status_muted`，如 `["mcp"]`），不是这里。
pub(crate) fn action_of(key: &str) -> Option<StatusAction> {
    match key {
        // pi-usage 的额度/余额行（点击 = /usage 菜单卡）
        "usage" => Some(StatusAction::Command("/usage")),
        // `mcp`：composer 功能栏已有 MCP 胶囊（server 图标 + `[n]` 已启用数），
        // 这行不做第二入口；嫌吵可在 ext_status_muted 里静音
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

/// 状态带本体。None = 整条不渲染（不占 28px，也不建 element）。
///
/// 显隐四档（034 §3.4）：开关关 / 无项 / 全部文本空 / 新会话页（调用方不挂）。
pub(crate) fn bar(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> Option<gpui::AnyElement> {
    if !crate::services::workspace::session_status_bar_on() {
        return None;
    }
    let t = T();
    // 读一次运行态：全部非空状态项（key 字母序）+ agent 是否在跑（命令类禁用
    // 判定）。准入 = 全显示 − 用户静音键（`ext_status_muted`）；动作表只决定
    // 可点性，不决定显示。
    let muted = crate::services::workspace::ext_status_muted_keys();
    let (items, running) = {
        let r = chat.rt().read(cx);
        let mut items: Vec<(String, String, Option<StatusAction>)> = r
            .ext_status
            .iter()
            .filter(|(key, text)| !text.is_empty() && !muted.iter().any(|m| m == key))
            .map(|(key, text)| (key.clone(), text.clone(), action_of(key)))
            .collect();
        // pi footer 同口径：按 key 字母序（同一组项跨会话顺序稳定）
        items.sort_by(|a, b| a.0.cmp(&b.0));
        (items, r.agent_running)
    };
    if items.is_empty() {
        return None;
    }

    let mut row = div().flex().items_center();
    let mut first = true;
    for (key, text, action) in items {
        // 多条信息用 `|` 分隔（用户定夺，2026-11-04）：项间 gap 由分隔符 +
        // 两侧 SP2 = 4 构成，不再用 ITEM_GAP
        if !first {
            row = row.child(
                div()
                    .mx(px(4.))
                    .text_size(crate::appearance::ui_size(10.))
                    .font_family(crate::appearance::panel_font().family)
                    .text_color(rgb(t.text_faint))
                    .child("|"),
            );
        }
        first = false;

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
            // 034：字体族与左侧面板同族（psp 行同源 panel_font），字号 = 面板 −2
            .font_family(crate::appearance::panel_font().family)
            .text_size(crate::appearance::ui_size(10.))
            .text_color(rgb(color))
            .child(SharedString::from(text));
        match action {
            Some(_) if blocked => {
                // 禁用：opacity 0.5 + 无 pointer / 无 hover（§8），提示为什么
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
            // 纯文本：无 pointer、无 hover（不可点语义）
            None => {}
        }
        row = row.child(el);
    }

    // 条 = chat-wrap 底缘的 absolute 覆盖层（不参与 flex 流）：显隐都不推挤
    // Inputpanel（composer 底距恒为 BAR_H，开闭之间不动）。PAD_BOTTOM 已把
    // 这 28px 算进让位，消息不会被压在带子底下。
    // 不画上沿线：左邻是 dock 的 tab 栏，横线一拉就对不齐（用户定夺）。
    // 内容行与消息列/composer 同心：px15 + 内容列宽封顶，项在列内**居中**
    //（用户口径：右侧已有模型信息，项再右对齐会形成两条贴右竖排文字，碍眼）。
    let content_w = crate::services::workspace::chat_content_width();
    Some(
        div()
            .id("session-status-bar")
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(BAR_H))
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(t.chrome))
            .child(
                div().w_full().px(px(15.)).flex().justify_center().child(
                    div()
                        .w_full()
                        .max_w(px(content_w))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(row),
                ),
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

    /// 动作表只管可点性，不管准入：usage 可点，其余键 None（但仍会上屏）。
    #[test]
    fn action_table_is_not_admission() {
        assert!(matches!(
            action_of("usage"),
            Some(StatusAction::Command("/usage"))
        ));
        // mcp / mcp-auth / freeflow… = None = 纯文本（显示，但不可点）
        for key in ["mcp", "mcp-auth", "freeflow", "goal", ""] {
            assert_eq!(action_of(key), None, "{key} 无动作但不应被隐藏");
        }
    }
}
