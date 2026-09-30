//! topbar 两段 (v54): 左段在 panel-col 内（仅收放钮 + 拖拽区），右段在
//! content-col 内（内容区 term/md tabs + 设置 + 窗口控制钮）。整条是客户
//! 区自绘标题栏——空段挂 `WindowControlArea::Drag` 命中盒，三个窗口钮注册
//! Min/Max/Close 命中盒（zed platform_title_bar parity）。收起态面板全隐，
//! 收放钮跳到右段起点（竖线镜像位）。

use gpui::{MouseButton, SharedString, Window, div, prelude::*, px, rgb};
use gpui::WindowControlArea;

use crate::Chat;
use crate::ContentView;
use crate::i18n::tr;
use crate::theme::theme as T;

/// topbar 高（两段同高；窗口控制钮高度跟随）。
pub(crate) const HEIGHT: f32 = 36.;

/// Caption-button glyph font (Win11; MDL2 covers Win10).
pub const CAPTION_FONT: &str = "Segoe Fluent Icons";

/// 左段：收放钮（贴左 5px）+ 拖拽填充。
pub(crate) fn topbar_l(_chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();
    div()
        .id("topbar-l")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(t.chrome))
        .child(div().ml(px(5.)).child(icon_btn(
            "panes-toggle",
            "panel-left",
            tr("收起 / 展开侧栏"),
            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                this.toggle_panes(cx);
            }),
        )))
        // drag filler: the empty middle is the drag region (HTCAPTION —
        // platform handles move + double-click-zoom)
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
}

/// 右段：内容区 tabs + 设置 + 竖线 + 窗口控制。
pub(crate) fn topbar_r(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let maximized = window.is_maximized();
    // Glyphs: 0xE921 min, 0xE922 max, 0xE923 restore, 0xE8BB close.
    let max_glyph = if maximized { "\u{E923}" } else { "\u{E922}" };
    let max_tip = if maximized { tr("向下还原") } else { tr("最大化") };
    let _ = max_tip;

    let mut bar = div()
        .id("topbar-r")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .relative()
        .flex()
        .items_center()
        .bg(rgb(t.chrome))
        .border_b_1()
        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x73)));

    // 收起态：收放钮跳到右段起点（4px 等距，竖线镜像位）+ 内容区 tabs
    // 都住在 items_end 的 tabs host 里（激活 tab 连体贴底需要）
    // 收起态：收放钮在 bar 主层（垂直居中），tabs host 只装内容区 tabs
    if chat.panes_hidden {
        bar = bar
            .child(
                div()
                    .ml(px(4.))
                    .mr(px(2.))
                    .child(icon_btn(
                        "panes-toggle-r",
                        "panel-left",
                        tr("展开侧栏"),
                        cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                            this.toggle_panes(cx);
                        }),
                    )),
            )
            .child(div().w(px(1.)).h(px(18.)).bg(gpui::rgba(crate::theme::border_alpha(t, 0x8c))).mx(px(4.)));
    }
    // topbar 状态与内容区绑定：会话视图=左对齐会话标题（≤15 字）；
    // 浏览操作区=终端/文件 tabs（切回会话视图 tabs 即消失）
    if chat.content_view == ContentView::Chat {
        let title: SharedString = chat.session_title(cx).into();
        bar = bar.child(
            div()
                .h_full()
                .flex()
                .items_center()
                .pl(px(12.))
                .min_w_0()
                .max_w(px(320.))
                .overflow_hidden()
                .text_size(px(12.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(title),
        );
    } else {
        bar = bar.child(browse_tabs(chat, cx));
    }

    bar = bar.child(
        div()
            .flex_1()
            .h_full()
            .window_control_area(WindowControlArea::Drag),
    );
    // 设置（sliders-horizontal）
    bar = bar.child(
        div().mx(px(6.)).child(icon_btn(
            "topbar-settings",
            "sliders-horizontal",
            tr("设置"),
            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                this.open_settings(0, cx);
            }),
        )),
    );
    bar = bar.child(div().w(px(1.)).h(px(18.)).bg(gpui::rgba(crate::theme::border_alpha(t, 0x8c))).mx(px(6.)));

    for (area, glyph) in [
        (WindowControlArea::Min, "\u{E921}"),
        (WindowControlArea::Max, max_glyph),
    ] {
        bar = bar.child(caption_button(area, glyph, t));
    }
    bar.child(caption_button(
        WindowControlArea::Close,
        "\u{E8BB}",
        t,
    ))
}

/// 30×30 图标钮（topbar 通用；hover chrome-hover）。
fn icon_btn(
    id: &'static str,
    icon_name: &'static str,
    _tip: &'static str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    let t = T();
    div()
        .id(id)
        .size(px(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, handler)
        .child(crate::ui::icon(icon_name, 18., t.text_muted))
}

/// 内容区 tab（Obsidian 式）：激活 = 凸起卡片（bg 色、顶圆角、压底线、×
/// 可见）；非激活 = 平铺文字。`ml` = 距分隔线/前一 tab 的间距。


/// 浏览操作区的 topbar tabs（终端 + 文件）。
fn browse_tabs(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> gpui::Stateful<gpui::Div> {
    let mut tabs_host = div().id("topbar-tabs").h_full().flex().items_end();
    for (ix, tab) in chat.panel_tabs.iter().enumerate() {
        let (label, is_file): (SharedString, bool) = match tab {
            crate::PanelTab::Term(id) => {
                let title = chat
                    .terminals
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| {
                        let dir = t
                            .cwd
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        format!("bash — {dir}")
                    })
                    .unwrap_or_else(|| "bash".into());
                (title.into(), false)
            }
            crate::PanelTab::File(p) => (
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into())
                    .into(),
                true,
            ),
        };
        let active = chat.content_view
            == if is_file { ContentView::File } else { ContentView::Term }
            && chat.active_panel_tab == Some(ix);
        tabs_host = tabs_host.child(content_tab(
            if is_file { "ctab-file" } else { "ctab-term" },
            ix,
            label,
            active,
            if ix == 0 { 10. } else { 4. },
            is_file,
            cx,
        ));
    }
    tabs_host
}

#[allow(clippy::too_many_arguments)]
fn content_tab(
    id: &'static str,
    ix: usize,
    label: SharedString,
    active: bool,
    ml: f32,
    is_file: bool,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let close = cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
        this.close_panel_tab(ix, cx);
        // 关掉当前 tab 后内容区回退：终端→浏览区遗留→chat
        if this.content_view
            == if is_file { ContentView::File } else { ContentView::Term }
            && this.active_panel_tab.is_none()
        {
            let v = if this.panel_tabs.is_empty() {
                ContentView::Chat
            } else {
                this.browse_last
            };
            this.set_content_view(v);
            if this.content_view == ContentView::Term {
                this.active_panel_tab = Some(this.panel_tabs.len() - 1);
            }
        }
        cx.notify();
    });
    let switch = cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
        this.active_panel_tab = Some(ix);
        let v = if is_file { ContentView::File } else { ContentView::Term };
        this.set_content_view(v);
        cx.notify();
    });
    tab_shell(id, label, active, ml, t, switch, Some(close))
}

#[allow(clippy::too_many_arguments)]
fn tab_shell(
    id: &'static str,
    label: SharedString,
    active: bool,
    ml: f32,
    t: &'static crate::theme::Theme,
    switch: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
    close: Option<impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static>,
) -> impl gpui::IntoElement {
    let mut tab = div()
        .id(id)
        .ml(px(ml))
        .flex()
        .items_center()
        .gap(px(9.))
        .text_size(px(12.))
        .cursor_pointer()
        .when(active, |d| {
            // 连体态：33px 高、bg 填充、压住底线（host 已 items_end 贴底）
            d.h(px(HEIGHT - 3.))
                .mb(px(-1.))
                .bg(rgb(t.bg))
                .border_1()
                .border_b_0()
                .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x8c)))
                .rounded_tl(px(9.))
                .rounded_tr(px(9.))
                .pl(px(12.))
                .pr(px(5.))
                .text_color(rgb(t.text))
        })
        .when(!active, |d| {
            d.h(px(HEIGHT))
                .pl(px(12.))
                .pr(px(12.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.text_color(rgb(t.text)))
        })
        .on_mouse_down(MouseButton::Left, switch);
    tab = tab
        .child(label)
        .children(active.then(|| {
            let mut x = div()
                .id(SharedString::from(format!("{id}-x")))
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .text_color(rgb(t.text_dim))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)));
            if let Some(close) = close {
                x = x.on_mouse_down(MouseButton::Left, move |ev, w, cx| {
                    cx.stop_propagation();
                    close(ev, w, cx);
                });
            }
            x.child(crate::ui::icon("x", 11., t.text_dim))
        }));
    tab
}

fn caption_button(
    area: WindowControlArea,
    glyph: &'static str,
    t: &crate::theme::Theme,
) -> impl gpui::IntoElement {
    let hover_bg = if area == WindowControlArea::Close {
        rgb(t.danger_hover) // v54 关闭悬停红（随主题 danger 系；mist 即设计稿 #d8626a）
    } else {
        rgb(t.bg_hover)
    };
    let hover_fg = if area == WindowControlArea::Close {
        rgb(0xffffff)
    } else {
        rgb(t.text)
    };
    div()
        .id(SharedString::from(format!("wb-{}", area as u8)))
        .w(px(42.))
        .h(px(HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .font_family(CAPTION_FONT)
        .text_sm()
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .window_control_area(area)
        .child(
            div()
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(glyph)),
        )
}
