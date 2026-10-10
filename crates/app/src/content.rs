//! 内容区 (v54): topbar-r 之下的一切。状态机 chat / term / md —— 终端与
//! markdown 预览以 topbar tab 打开（Obsidian 式），聊天为默认视图。内容
//! 区直通窗口底（statusbar 只在面板段）。

use gpui::{Context, Entity, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::ContentView;
use crate::i18n::tr;
use crate::theme::theme as T;

/// 内容主视图：按 content_view 切换（flex-1，占满 topbar-r 以下）。
pub(crate) fn content_main(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let view: gpui::AnyElement = match chat.content_view {
        ContentView::Chat => crate::session::main_column(chat, entity, weak, window, cx)
            .into_any_element(),
        ContentView::Term => term_view(chat, weak, window, cx)
            .map(|d| d.into_any_element())
            .unwrap_or_else(|| empty_hint(tr("暂无终端会话"), t)),
        ContentView::File => crate::editor::view::file_view(chat, weak, window, cx).into_any_element(),
    };
    div()
        .id("content-main")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(view)
        .into_any_element()
}

pub(crate) fn empty_hint(text: &str, t: &'static crate::theme::Theme) -> gpui::AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        // §2：空态提示 = 辅助说明档（随界面字号缩放，原 text_xs 裸字号）
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 终端视图：纯暗面（tab 在 topbar-r；失败/退出横幅保留；无 38px 头）
// ---------------------------------------------------------------------------

pub(crate) fn term_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let _t = T();
    let body: Option<gpui::AnyElement> = chat
        .active_panel_tab
        .and_then(|ix| chat.panel_tabs.get(ix).cloned())
        .map(|tab| match tab {
            crate::PanelTab::File(_) => div().into_any_element(),
            crate::PanelTab::Term(id) => {
                let tix = chat.terminals.iter().position(|t| t.id == id);
                let Some(tix) = tix else {
                    return div().into_any_element();
                };
                let tab = &chat.terminals[tix];
                let mut col = div().flex_1().min_h_0().flex().flex_col();
                match &tab.status {
                    crate::terminal::TermStatus::Exited(code) => {
                        let code_text = code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| tr("unknown").to_string());
                        col = col.child(
                            div()
                                .py(px(7.))
                                .px(px(12.))
                                .text_size(crate::appearance::ui_size(11.))
                                .font_family(crate::terminal::FONT_FAMILY)
                                .text_color(rgb(0x6e8a7d))
                                .child(SharedString::from(crate::i18n::tf(
                                    "Process exited with code {code_text}",
                                    &[("code_text", code_text)],
                                ))),
                        );
                    }
                    crate::terminal::TermStatus::Ready => {}
                }
                col.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .bg(rgb(0x141a17))
                        .child(
                            crate::terminal::TerminalElement::new(tab, weak.clone())
                                .blink_on(chat.term_cursor_on)
                                .track_focus(&tab.focus)
                                .flex_1()
                                .h_full()
                                .on_mouse_down(MouseButton::Left, {
                                    let f = tab.focus.clone();
                                    move |_, window, _cx| {
                                        window.focus(&f);
                                    }
                                })
                                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                                    this.terminal_key(ev, cx);
                                })),
                        ),
                )
                .into_any_element()
            }
        });

    body.map(|b| {
        div()
            .id("term-view")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(0x141a17))
            .child(b)
            .into_any_element()
    })
}

