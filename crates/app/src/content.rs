//! 内容区 (v54): topbar-r 之下的一切。状态机 chat / term / md —— 终端与
//! markdown 预览以 topbar tab 打开（Obsidian 式），聊天为默认视图。内容
//! 区直通窗口底（statusbar 只在面板段）。

use gpui::{Context, Entity, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

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
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(rgb(t.bg))
            .font_family(md_font.family.clone())
            .child(
                div()
                    .max_w(px(760.))
                    .mx_auto()
                    .w_full()
                    .pt(px(26.))
                    .px(px(34.))
                    .pb(px(40.))
                    .child(crate::markdown::render_themed(&content)),
            )
            .into_any_element(),
        // html：无法安全执行脚本（无 webview），展示带样式的只读提示 +
        // 源码；正文 markdown 亦按渲染处理。pi-web 用 iframe srcdoc——
        // 桌面端后续接 webview 时替换
        "html" | "htm" => div()
            .id("fv-html")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(rgb(t.bg))
            .child(
                div()
                    .max_w(px(900.))
                    .mx_auto()
                    .w_full()
                    .pt(px(18.))
                    .px(px(30.))
                    .pb(px(30.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg_panel))
                            .text_size(px(12.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(tr(
                                "HTML 预览（静态渲染）：脚本未执行；需要交互请用浏览器打开",
                            ))),
                    )
                    .child(crate::markdown::render_themed(&html_to_md(&content))),
            )
            .into_any_element(),
        // 图片（桌面端暂无 image 元素支持，提示）
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" => empty_hint(
            "图片预览即将支持——请在资源管理器中查看",
            t,
        ),
        // 其余：源码
        _ => div()
            .id("fv-src")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(rgb(t.bg))
            .font_family("Consolas")
            .child(
                div()
                    .w_full()
                    .pt(px(14.))
                    .px(px(22.))
                    .pb(px(30.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .children(content.lines().enumerate().map(|(i, line)| {
                                div()
                                    .flex()
                                    .text_size(px(12.5))
                                    .line_height(relative(1.55))
                                    .child(
                                        div()
                                            .w(px(44.))
                                            .flex_shrink_0()
                                            .text_right()
                                            .pr(px(12.))
                                            .text_color(rgb(t.text_faint))
                                            .child(SharedString::from(format!(
                                                "{}",
                                                i + 1
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .whitespace_nowrap()
                                            .text_color(rgb(t.text))
                                            .child(SharedString::from(
                                                line.to_string(),
                                            )),
                                    )
                            })),
                    ),
            )
            .into_any_element(),
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
                .border_color(gpui::rgba(0xafc4ba66))
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

/// 极简 HTML→文本转换（去标签保结构），供无 webview 场景的静态阅读。
fn html_to_md(html: &str) -> String {
    // 块级标签换行
    let mut s = html
        .replace("</p>", "\n\n")
        .replace("</div>", "\n")
        .replace("</h1>", "\n\n")
        .replace("</h2>", "\n\n")
        .replace("</h3>", "\n\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</li>", "\n");
    // 去掉 script/style 块
    if let Some(a) = s.find("<script") {
        if let Some(b) = s[a..].find("</script>") {
            s = format!("{}{}", &s[..a], &s[a + b + 9..]);
        }
    }
    if let Some(a) = s.find("<style") {
        if let Some(b) = s[a..].find("</style>") {
            s = format!("{}{}", &s[..a], &s[a + b + 8..]);
        }
    }
    // 剥其余标签
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    // 折叠空行
    let lines: Vec<&str> = out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let mut prev_blank = false;
    let mut cleaned = String::new();
    for l in lines {
        let blank = l.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        cleaned.push_str(l);
        cleaned.push('\n');
        prev_blank = blank;
    }
    cleaned
}
