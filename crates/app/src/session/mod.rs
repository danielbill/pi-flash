//! sessionView (030): sessionMessagePanel (032) + inputPanel (031).

pub(crate) mod input;
pub(crate) mod messages;

use gpui::{Animation, AnimationExt, MouseButton, SharedString, div, list, prelude::*, pulsating_between, px, rgb};
use pi_link::protocol::Block;

use self::messages::{Role, render_msg};
use crate::ext_ui::render_ext_widget;

use crate::Chat;
use crate::PillMenu;
use crate::TopPanel;
use crate::services::branch::*;
use crate::i18n::tr;
use crate::services::format::*;
use crate::theme::theme as T;
use crate::ui::{icon, pill};

// ---------------------------------------------------------------------------
// sessionView assembly (030): toolbar + message list + composer + status
// (free function over Chat state; entity split is a G+ upgrade)
// ---------------------------------------------------------------------------

pub(crate) fn main_column(
    chat: &mut Chat,
    entity: gpui::Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> gpui::Div {
    let t = T();
    let status: SharedString = chat.status.clone().into();
    let streaming = chat.state.as_ref().is_some_and(|st| st.is_streaming);
    let model_label: SharedString = chat
        .state
        .as_ref()
        .and_then(|s| s.model_label())
        .unwrap_or_else(|| tr("选择模型").into())
        .into();
    let thinking_label: SharedString = chat
        .state
        .as_ref()
        .and_then(|s| s.thinking_level.clone())
        .unwrap_or_else(|| "medium".into())
        .into();
    let input_focused = chat.focus.is_focused(window);
    chat.input_focused = input_focused;
    let caret_on = chat.caret_on;
    let this_input: SharedString = chat.input.clone().into();
    let thinking_menu_open = chat.pill_menu == Some(PillMenu::Thinking);
    let tools_menu_open = chat.pill_menu == Some(PillMenu::Tools);
    let tools_label = chat.tool_preset_label();
    let stats_right: SharedString = if let Some(st) = chat.stats.as_ref() {
        format!(
            "↑{} ↓{} ⟳{} ${:.2}  {}% / {}",
            fmt_compact(st.input),
            fmt_compact(st.output),
            fmt_compact(st.cache_read),
            st.cost,
            st.context_percent.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            st.context_window.map(fmt_compact).unwrap_or_else(|| "-".into())
        )
        .into()
    } else {
        SharedString::from("")
    };
    let chat_entity = entity.clone();

    let weak_for_msg = weak.clone();

    let main_col = div()

        .flex_1()

        .min_w_0()

        .h_full()

        .flex()

        .flex_col()

        .bg(rgb(t.assistant_bg))

        .text_color(rgb(t.text))

        .font_family("Segoe UI")

        // top toolbar

        .child(

            div()

                .flex()

                .items_center()

                .gap_1p5()

                .px_3()

                .py_1p5()

                .border_b_1()

                .border_color(rgb(t.border))

                .child(pill("tb-sidebar", "panel-left", SharedString::from("")))

                .child(pill(

                    "tb-history",

                    "history",

                    SharedString::from(tr("完整历史")),

                ))

                .child(

                    div()

                        .id("tb-branch")

                        .px_2()

                        .py_1()

                        .rounded_md()

                        .border_1()

                        .border_color(rgb(t.border))

                        .flex()

                        .items_center()

                        .gap_1p5()

                        .text_xs()

                        .text_color(rgb(t.text_muted))

                        .cursor_pointer()

                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))

                        .on_mouse_down(MouseButton::Left, cx.listener(

                            |this, _: &gpui::MouseDownEvent, _w, cx| {

                                this.open_branch_tree(cx);

                            },

                        ))

                        .child(icon(

                            "git-branch",

                            12.,

                            if chat

                                .branch_tree

                                .as_ref()

                                .is_some_and(|(tr, _)| tree_has_branches(tr))

                            {

                                t.accent

                            } else {

                                t.text_muted

                            },

                        ))

                        .child(SharedString::from(tr("分支"))),

                )

                .child(

                    div()

                        .id("tb-title")

                        .px_2()

                        .py_1()

                        .rounded_md()

                        .border_1()

                        .border_color(rgb(t.border))

                        .flex()

                        .items_center()

                        .gap_1p5()

                        .text_xs()

                        .text_color(rgb(t.text_muted))

                        .cursor_pointer()

                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))

                        .on_mouse_down(MouseButton::Left, cx.listener(

                            |this, _: &gpui::MouseDownEvent, _w, cx| {

                                this.auto_title(cx);

                            },

                        ))

                        .child(icon("pencil", 12., t.text_muted))

                        .child(SharedString::from(tr("生成标题"))),

                )

                .child(

                    div()

                        .id("tb-system")

                        .px_2()

                        .py_1()

                        .rounded_md()

                        .border_1()

                        .border_color(rgb(if chat.top_panel == Some(TopPanel::System) { t.accent } else { t.border }))

                        .flex()

                        .items_center()

                        .gap_1p5()

                        .text_xs()

                        .text_color(rgb(if chat.top_panel == Some(TopPanel::System) { t.accent } else { t.text_muted }))

                        .cursor_pointer()

                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))

                        .on_mouse_down(MouseButton::Left, cx.listener(

                            |this, _: &gpui::MouseDownEvent, _w, cx| {

                                this.top_panel = match this.top_panel {

                                    Some(TopPanel::System) => None,

                                    _ => Some(TopPanel::System),

                                };

                                this.request_system_info(cx);

                            },

                        ))

                        .child(icon(

                            "file-text",

                            12.,

                            if chat.sys_prompt.is_some() { t.accent } else { t.text_muted },

                        ))

                        .child(SharedString::from(tr("系统"))),

                )

                .child(

                    div()

                        .id("tb-tools")

                        .px_2()

                        .py_1()

                        .rounded_md()

                        .border_1()

                        .border_color(rgb(if chat.top_panel == Some(TopPanel::Tools) { t.accent } else { t.border }))

                        .flex()

                        .items_center()

                        .gap_1p5()

                        .text_xs()

                        .text_color(rgb(if chat.top_panel == Some(TopPanel::Tools) { t.accent } else { t.text_muted }))

                        .cursor_pointer()

                        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))

                        .on_mouse_down(MouseButton::Left, cx.listener(

                            |this, _: &gpui::MouseDownEvent, _w, cx| {

                                this.top_panel = match this.top_panel {

                                    Some(TopPanel::Tools) => None,

                                    _ => Some(TopPanel::Tools),

                                };

                                this.request_system_info(cx);

                            },

                        ))

                        .child(icon(

                            "wrench",

                            12.,

                            if chat.session_tools.is_some() { t.accent } else { t.text_muted },

                        ))

                        .child(SharedString::from(tr("工具"))),

                )

                .child(

                    div()

                        .flex_1()

                        .text_right()

                        .text_xs()

                        .text_color(rgb(t.text_muted))

                        .child(stats_right),

                ),

        )

        // message list (820px centered column, ChatWindow parity)

        .child(

            list(chat.list.clone(), move |ix, _window, cx| {

                let chat = chat_entity.read(cx);

                let weak = weak_for_msg.clone();

                match chat.messages.get(ix) {

                    Some(m) => div()

                        .w_full()

                        .flex()

                        .justify_center()

                        .child(

                            div()

                                .w_full()

                                .max_w(px(820.))

                                .child(render_msg(

                                    m,

                                    ix,

                                    &weak,

                                    &chat.collapsed,

                                    t,

                                    &chat.model_label_text(),

                                    {

                                        let streaming = chat

                                            .state

                                            .as_ref()

                                            .is_some_and(|s| s.is_streaming);

                                        let is_last_assistant = streaming

                                            && Some(ix) == chat.messages.len().checked_sub(1)

                                            && m.role == Role::Assistant;

                                        if !is_last_assistant {

                                            None

                                        } else {

                                            let text: String = m

                                                .blocks

                                                .iter()

                                                .map(|b| match b {

                                                    Block::Text { text, .. }

                                                    | Block::Thinking { text, .. } => {

                                                        text.as_str()

                                                    }

                                                    _ => "",

                                                })

                                                .collect();

                                            let est = estimate_tokens(&text);

                                            let tps = chat.stream_started.and_then(

                                                |start| {

                                                    let secs =

                                                        start.elapsed().as_secs_f32();

                                                    (secs > 0.5 && est > 0)

                                                        .then(|| est as f32 / secs)

                                                },

                                            );

                                            Some((est, tps))

                                        }

                                    },

                                )),

                        )

                        .into_any_element(),

                    None => {

                        // pi-web ChatWindow phase label: pulsing text under

                        // the list while running with no streamed content

                        // yet (animate-[pulse_1.5s_infinite])

                        if chat.phase_row_visible() {

                            div()

                                .w_full()

                                .flex()

                                .justify_center()

                                .child(

                                    div()

                                        .w_full()

                                        .max_w(px(820.))

                                        .py_2()

                                        .text_size(px(13.))

                                        .text_color(rgb(t.text_muted))

                                        .child(SharedString::from(tr("正在等待模型...")))

                                        .with_animation(

                                            "phase-pulse",

                                            Animation::new(std::time::Duration::from_millis(

                                                1500,

                                            ))

                                            .repeat()

                                            .with_easing(pulsating_between(0.5, 1.0)),

                                            |label, delta| label.opacity(delta),

                                        ),

                                )

                                .into_any_element()

                        } else {

                            div().w_full().into_any_element()

                        }

                    }

                }

            })

            .flex_1()

            .min_h_0()

            .py_2(),

        )

        // empty new-session hero (pi-web ChatWindow isEmptyNew): logo row

        // directly above the editor, flex spacer below centers the pair

        .children((chat.messages.is_empty()

            && !chat

                .state

                .as_ref()

                .is_some_and(|s| s.is_streaming))

        .then(|| {

            div()

                .w_full()

                .mb_3()

                .px(px(16.))

                .child(

                    div()

                        .max_w(px(820.))

                        .mx_auto()

                        .flex()

                        .items_center()

                        .justify_between()

                        .gap_3()

                        .font_family("Consolas")

                        .child(

                            div()

                                .flex()

                                .items_center()

                                .gap_2p5()

                                .min_w_0()

                                .child(

                                    div()

                                        .size(px(32.))

                                        .rounded(px(8.))

                                        .bg(rgb(t.accent))

                                        .flex()

                                        .items_center()

                                        .justify_center()

                                        .text_size(px(20.))

                                        .font_weight(gpui::FontWeight::BOLD)

                                        .text_color(rgb(t.accent_contrast))

                                        .child("\u{3c0}"),

                                )

                                .child(

                                    div()

                                        .text_size(px(22.))

                                        .font_weight(gpui::FontWeight::BOLD)

                                        .text_color(rgb(t.text))

                                        .child("pi-flash"),

                                ),

                        )

                        .child(

                            div()

                                .flex()

                                .flex_col()

                                .items_end()

                                .gap(px(2.))

                                .child(

                                    div()

                                        .text_size(px(11.))

                                        .text_color(rgb(t.text_muted))

                                        .child(SharedString::from(format!(

                                            "app v{}",

                                            env!("CARGO_PKG_VERSION")

                                        ))),

                                )

                                .child(

                                    div()

                                        .text_size(px(11.))

                                        .text_color(rgb(t.text_muted))

                                        .child(SharedString::from(format!(

                                            "pi v{}",

                                            pi_link::vendor::vendored_version()

                                                .unwrap_or_default()

                                        ))),

                                ),

                        ),

                )

                .into_any_element()

        }))

        // extension widgets above the editor (setWidget aboveEditor)

        .children((!chat.ext_widgets.is_empty()).then(|| {

            let rows: Vec<gpui::AnyElement> = chat

                .ext_widgets

                .iter()

                .filter(|(_, _, above)| *above)

                .map(|(_, lines, _)| render_ext_widget(lines, t))

                .collect();

            (!rows.is_empty()).then(|| div().px_4().flex().flex_col().gap_1().children(rows).into_any_element())

        }).flatten())

        .child(input::input_area(chat, entity.clone(), &weak, streaming, input_focused, caret_on, this_input, model_label, thinking_menu_open, tools_menu_open, thinking_label, tools_label, cx))

        // extension widgets below the editor (setWidget belowEditor)

        .children((!chat.ext_widgets.is_empty()).then(|| {

            let rows: Vec<gpui::AnyElement> = chat

                .ext_widgets

                .iter()

                .filter(|(_, _, above)| !above)

                .map(|(_, lines, _)| render_ext_widget(lines, t))

                .collect();

            (!rows.is_empty()).then(|| div().px_4().pb_1().flex().flex_col().gap_1().children(rows).into_any_element())

        }).flatten())

        .children((chat.messages.is_empty()

            && !chat

                .state

                .as_ref()

                .is_some_and(|s| s.is_streaming))

        .then(|| div().flex_1().into_any_element()))

        // status bar (+ extension status items)

        .child(

            div()

                .px_3()

                .py_1()

                .border_t_1()

                .border_color(rgb(t.border))

                .bg(rgb(t.bg_panel))

                .text_xs()

                .text_color(rgb(t.text_muted))

                .flex()

                .items_center()

                .gap_3()

                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(status))

                .children(chat.ext_status.iter().map(|(k, text)| {

                    div()

                        .flex_shrink_0()

                        .font_family("Consolas")

                        .text_size(px(10.))

                        .text_color(rgb(t.text_dim))

                        .child(SharedString::from(format!("{}: {}", k, text)))

                })),

        );





    // ---- right panel: file + terminal tabs (pi-web AppShell panelTabs

    //      merge; fixed dark terminal surface in every theme) -----------

    main_col
}
