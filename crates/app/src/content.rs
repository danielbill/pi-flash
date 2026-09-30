//! 内容区 (v54): topbar-r 之下的一切。状态机 chat / term / md —— 终端与
//! markdown 预览以 topbar tab 打开（Obsidian 式），聊天为默认视图。内容
//! 区直通窗口底（statusbar 只在面板段）。

use gpui::{Context, Entity, Image, KeyDownEvent, MouseButton, SharedString, div, img, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::ContentView;
use crate::i18n::tr;
use crate::theme::theme as T;
use crate::ui::icon;

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
        ContentView::File => file_view(chat).into_any_element(),
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

fn empty_hint(text: &str, t: &'static crate::theme::Theme) -> gpui::AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
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
                let (dot, _status) = match &tab.status {
                    crate::terminal::TermStatus::Ready => (0x4ade80, ""),
                    crate::terminal::TermStatus::Exited(_) | crate::terminal::TermStatus::Failed(_) => {
                        (0xf87171, "")
                    }
                };
                let cwd_text: SharedString = tab.cwd.to_string_lossy().to_string().into();
                let weak_restart = weak.clone();
                let mut col = div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(32.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pl(px(13.))
                            .pr(px(10.))
                            .bg(rgb(0x141a17))
                            .child(
                                div()
                                    .size(px(7.))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(rgb(dot)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(11.))
                                    .font_family(crate::terminal::FONT_FAMILY)
                                    .text_color(rgb(0x6e8a7d))
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .child(cwd_text),
                            )
                            .child(
                                div()
                                    .id("term-restart")
                                    .h(px(24.))
                                    .w(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.))
                                    .text_color(rgb(0x6e8a7d))
                                    .cursor_pointer()
                                    .hover(|s| {
                                        s.bg(rgb(0x242932)).text_color(rgb(0xe5e7eb))
                                    })
                                    .on_mouse_down(MouseButton::Left, {
                                        let rix = tix;
                                        move |_, _, cx| {
                                            let _ = weak_restart.update(cx, |c, cx| {
                                                c.restart_terminal(rix, cx);
                                            });
                                        }
                                    })
                                    .child(icon("refresh", 12., 0x6e8a7d)),
                            ),
                    );
                match &tab.status {
                    crate::terminal::TermStatus::Failed(e) => {
                        col = col.child(
                            div()
                                .py(px(7.))
                                .px(px(12.))
                                .bg(rgb(0x321b1b))
                                .border_b_1()
                                .border_color(rgb(0x5f2424))
                                .text_size(px(11.))
                                .font_family(crate::terminal::FONT_FAMILY)
                                .text_color(rgb(0xfca5a5))
                                .child(SharedString::from(e.clone())),
                        );
                    }
                    crate::terminal::TermStatus::Exited(code) => {
                        let code_text = code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| tr("unknown").to_string());
                        col = col.child(
                            div()
                                .py(px(7.))
                                .px(px(12.))
                                .text_size(px(11.))
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

// ---------------------------------------------------------------------------
// 文件查看：md/html 渲染预览（pi-web rendered-first parity）、其余源码
// （markdown 渲染器带语法高亮）、大文件/二进制提示
// ---------------------------------------------------------------------------

fn file_view(chat: &mut Chat) -> gpui::AnyElement {
    let t = T();
    let path = chat
        .active_panel_tab
        .and_then(|ix| chat.panel_tabs.get(ix).cloned())
        .and_then(|tab| match tab {
            crate::PanelTab::File(p) => Some(p),
            _ => None,
        });
    let Some(path) = path else {
        return empty_hint("no file", t);
    };
    let content = chat
        .file_cache
        .get(&path)
        .map(|f| f.content.clone())
        .unwrap_or_default();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let meta = Chat::file_meta(&path, &content);
    let md_font = crate::appearance::markdown_font();

    let body: gpui::AnyElement = match ext.as_str() {
        // md：渲染（复用 agent 正文的 markdown 渲染器，自带语法高亮）
        "md" | "markdown" => div()
            .id("fv-md")
            .relative()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&chat.file_scroll)
            .bg(rgb(t.bg))
            .font_family(md_font.family.clone())
            .overflow_x_hidden()
            .child(
                div()
                    .max_w(px(760.))
                    .mx_auto()
                    .w_full()
                    .pt(px(26.))
                    .px(px(34.))
                    .pb(px(40.))
                    .overflow_hidden()
                    .child(crate::markdown::render_themed(&content)),
            )
            .child(
                gpui_component::scroll::Scrollbar::vertical(&chat.file_scrollbar, &chat.file_scroll),
            )
            .into_any_element(),
        // 图片：gpui img() 真渲染（最佳查看方式）
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => {
            let format = match ext.as_str() {
                "png" => Some(gpui::ImageFormat::Png),
                "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
                "gif" => Some(gpui::ImageFormat::Gif),
                "bmp" => Some(gpui::ImageFormat::Bmp),
                "svg" => Some(gpui::ImageFormat::Svg),
                _ => Some(gpui::ImageFormat::Webp),
            };
            match std::fs::read(&path)
                .ok()
                .zip(format)
                .map(|(bytes, f)| std::sync::Arc::new(Image::from_bytes(f, bytes)))
            {
                Some(image) => div()
                    .id("fv-img")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .bg(rgb(t.bg))
                    .p(px(20.))
                    .flex()
                    .justify_center()
                    .items_start()
                    .child(
                        img(image).max_w_full(),
                    )
                    .into_any_element(),
                None => empty_hint("图片读取失败", t),
            }
        }
        // 其余：源码。单 text 块渲染整个文件（逐行 div 在千行级文件上
        // 会拖垮帧率——无虚拟化）。行号以内嵌右对齐数字拼接。
        _ => {
            let numbered: String = content
                .lines()
                .enumerate()
                .map(|(i, line)| {
                    // 行内 tab 展开为 4 空格（等宽对齐）
                    let line = line.replace("	", "    ");
                    format!("{:>4} │ {}", i + 1, line)
                })
                .collect::<Vec<_>>()
                .join("
");
            div()
                .id("fv-src")
                .relative()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .track_scroll(&chat.file_scroll)
                .bg(rgb(t.bg))
                .overflow_x_hidden()
                .child(
                    div()
                        .font_family("Consolas")
                        .text_size(px(12.5))
                        .line_height(relative(1.5))
                        .text_color(rgb(t.text))
                        .overflow_hidden()
                        .child(SharedString::from(numbered)),
                )
                .child(
                    gpui_component::scroll::Scrollbar::vertical(&chat.file_scrollbar, &chat.file_scroll),
                )
                .into_any_element()
        }
    };

    div()
        .id("file-view")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 头部：文件名 + 语言/行数/大小
        .child(
            div()
                .h(px(34.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .border_b_1()
                .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x66)))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(
                            path.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default(),
                        )),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(t.text_faint))
                        .child(SharedString::from(meta)),
                ),
        )
        .child(body)
        .into_any_element()
}
