//! 内容区 (v54): topbar-r 之下的一切。状态机 chat / term / md —— 终端与
//! markdown 预览以 topbar tab 打开（Obsidian 式），聊天为默认视图。内容
//! 区直通窗口底（statusbar 只在面板段）。

use gpui::{Context, Entity, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::ContentView;
use crate::i18n::tr;
use crate::theme::theme as T;
use crate::ui::ScrollAxisExt;

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
        // 081 更新日志 tab（新版本首启自动打开）
        ContentView::Changelog => changelog_view(chat).into_any_element(),
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

pub(crate) fn empty_hint(text: &str, t: &'static crate::theme::Theme) -> gpui::AnyElement {    div()
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
// 更新日志 tab（081）：markdown 渲染 release notes，阅读列居中
// ---------------------------------------------------------------------------

fn changelog_view(chat: &mut Chat) -> gpui::AnyElement {
    let t = T();
    let Some(page) = &chat.changelog else {
        return empty_hint(tr("暂无更新日志"), t);
    };
    let version = page.version.clone();
    let body = page.body.clone();
    let fetching = page.fetching;
    let mut col = div().flex_1().min_h_0().flex().flex_col();
    // 头：更新日志 + 版本号（滚动跟随外层，不参与内层滚动）
    col = col.child(
        div()
            .px(px(32.))
            .pt(px(20.))
            .flex()
            .items_baseline()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(17.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(tr("更新日志").to_string()),
            )
            .child(
                div()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_muted))
                    .child(SharedString::from(format!("v{version}"))),
            ),
    );
    if let Some(body) = body {
        col = col.child(
            div()
                .id("changelog-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&chat.changelog_scroll)
                .restrict_scroll_to_axis()
                .child(
                    div()
                        .max_w(px(820.))
                        .mx_auto()
                        .px(px(32.))
                        .pb(px(40.))
                        .child(crate::editor::markdown::render(&body, t, false)),
                ),
        );
    } else if fetching {
        col = col.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(crate::ui::spinner(14., t.text_dim))
                .child(tr("正在获取更新日志…").to_string()),
        );
    } else {
        col = col.child(empty_hint(tr("暂无更新日志"), t));
    }
    col.into_any_element()
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
            crate::PanelTab::Changelog => div().into_any_element(),
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

