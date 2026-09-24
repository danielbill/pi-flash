//! pi-flash M1 shell: GPUI chat over the vendored pi (pi-link).
//!
//! UI note: interaction parity with pi-web comes first; visual polish last
//! (see PORT_PLAN.md). This is the M1 chat core, not the final look.

use std::path::PathBuf;

use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyDownEvent, ListAlignment, ListState,
    ParentElement, Render, SharedString, Styled, WindowOptions, div, list, prelude::*, px, rgb,
};
use pi_link::client::{PiSession, spawn as spawn_pi};
use pi_link::protocol::{AssistantEvent, Command, Event};

// ---------------------------------------------------------------------------
// palette (placeholder theme; visual polish is deliberately deferred)
// ---------------------------------------------------------------------------

const COL_BG: u32 = 0x1a1b1e;
const COL_PANEL: u32 = 0x232428;
const COL_TEXT: u32 = 0xd7dadd;
const COL_USER: u32 = 0x8ab4f8;
const COL_ASSISTANT: u32 = 0x81c995;
const COL_STATUS: u32 = 0x9aa0a6;

// ---------------------------------------------------------------------------
// chat state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    User,
    Assistant,
}

struct Msg {
    role: Role,
    text: String,
}

struct Chat {
    focus: FocusHandle,
    input: String,
    messages: Vec<Msg>,
    list: ListState,
    session: Option<PiSession>,
    status: String,
}

impl Chat {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();

        let cwd = std::env::var("PI_FLASH_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let (session, events) = match spawn_pi(&cwd, &[]) {
            Ok((s, ev)) => (Some(s), Some(ev)),
            Err(e) => {
                eprintln!("{e}");
                (None, None)
            }
        };

        let status = match &session {
            Some(_) => format!("pi {} | starting", pi_link::PI_VENDOR_VERSION),
            None => "pi not available (vendor missing)".to_string(),
        };

        if let Some(events) = events {
            cx.spawn(async move |this, cx| {
                consume_events(this, cx, events).await;
            })
            .detach();
        }

        let mut list = ListState::new(0, ListAlignment::Bottom, px(1000.));
        list.reset(0);

        Self {
            focus,
            input: String::new(),
            messages: Vec::new(),
            list,
            session,
            status,
        }
    }

    fn push_message(&mut self, role: Role, text: &str) {
        self.messages.push(Msg { role, text: text.to_string() });
        let count = self.messages.len();
        self.list.reset(count);
    }

    /// Append to the trailing assistant message, creating one if needed.
    fn append_assistant(&mut self, delta: &str) {
        match self.messages.last_mut() {
            Some(m) if m.role == Role::Assistant => m.text.push_str(delta),
            _ => self.push_message(Role::Assistant, delta),
        }
        let count = self.messages.len();
        self.list.reset(count);
    }

    fn send_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(session) = &self.session else {
            self.status = "not connected".into();
            cx.notify();
            return;
        };
        match session.send(&Command::Prompt { message: text }) {
            Ok(_) => {
                self.input.clear();
                self.status = "running".into();
            }
            Err(e) => self.status = e,
        }
        cx.notify();
    }

    fn abort(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            let _ = session.send(&Command::Abort);
            self.status = "aborting".into();
            cx.notify();
        }
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Response { command, success, error, .. } => {
                self.status = if success {
                    format!("{command} ok")
                } else {
                    format!("{command} failed: {}", error.unwrap_or_default())
                };
            }
            Event::MessageStart { role, text } => match role.as_str() {
                "user" => self.push_message(Role::User, &text),
                "assistant" => self.push_message(Role::Assistant, ""),
                _ => {}
            },
            Event::MessageUpdate(AssistantEvent::TextDelta { delta, .. }) => {
                self.append_assistant(&delta);
            }
            Event::MessageUpdate(AssistantEvent::ThinkingDelta { .. }) => {}
            Event::MessageUpdate(_) => {}
            Event::MessageEnd { role, text } => {
                if role == "assistant" && !text.is_empty() {
                    match self.messages.last_mut() {
                        Some(m) if m.role == Role::Assistant && m.text.is_empty() => m.text = text,
                        _ => {}
                    }
                }
            }
            Event::AgentStart => self.status = "running".into(),
            Event::AgentSettled => self.status = "idle".into(),
            Event::AgentEnd { .. } => {}
            Event::ExtensionUi(_) => {}
            Event::Unparsed(_) => {}
        }
        cx.notify();
    }
}

async fn consume_events(
    this: gpui::WeakEntity<Chat>,
    cx: &mut gpui::AsyncApp,
    mut rx: UnboundedReceiver<Event>,
) {
    while let Some(event) = rx.next().await {
        if this.update(cx, |chat, cx| chat.on_event(event, cx)).is_err() {
            return;
        }
    }
    let _ = this.update(cx, |chat, cx| {
        chat.status = "pi exited".into();
        cx.notify();
    });
}

impl Focusable for Chat {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

fn render_msg(m: &Msg) -> impl IntoElement {
    let (label, color) = match m.role {
        Role::User => ("you", rgb(COL_USER)),
        Role::Assistant => ("pi", rgb(COL_ASSISTANT)),
    };
    div()
        .w_full()
        .px_3()
        .py_1()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(label),
        )
        .child(div().text_color(rgb(COL_TEXT)).child(SharedString::from(m.text.clone())))
}

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.focus(&self.focus);

        let status: SharedString = self.status.clone().into();
        let input: SharedString = if self.input.is_empty() {
            "type a prompt, Enter to send, Esc to abort".into()
        } else {
            self.input.clone().into()
        };
        let input_empty = self.input.is_empty();
        let entity = cx.entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(COL_BG))
            .text_color(rgb(COL_TEXT))
            .font_family("Segoe UI")
            .text_sm()
            // header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .bg(rgb(COL_PANEL))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("pi-flash"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(COL_STATUS))
                            .child(status),
                    ),
            )
            // message list (bottom-aligned: sticks to the newest message)
            .child(
                list(self.list.clone(), move |ix, _window, cx| {
                    let chat = entity.read(cx);
                    match chat.messages.get(ix) {
                        Some(m) => div().w_full().child(render_msg(m)).into_any_element(),
                        None => div().w_full().into_any_element(),
                    }
                })
                .flex_1()
                .min_h_0()
                .py_2(),
            )
            // input row
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_3()
                    .py_2()
                    .bg(rgb(COL_PANEL))
                    .child(
                        div()
                            .id("input")
                            .track_focus(&self.focus)
                            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _w, cx| {
                                let key = ev.keystroke.key.as_str();
                                match key {
                                    "enter" => this.send_input(cx),
                                    "escape" => this.abort(cx),
                                    "backspace" => {
                                        if !ev.keystroke.modifiers.modified() {
                                            this.input.pop();
                                            cx.notify();
                                        }
                                    }
                                    "space" => {
                                        this.input.push(' ');
                                        cx.notify();
                                    }
                                    k => {
                                        let printable = k.chars().count() == 1
                                            && !ev.keystroke.modifiers.control
                                            && !ev.keystroke.modifiers.alt;
                                        if printable {
                                            if let Some(c) = k.chars().next() {
                                                this.input.push(c);
                                                cx.notify();
                                            }
                                        }
                                    }
                                }
                            }))
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(COL_BG))
                            .text_color(if input_empty {
                                rgb(COL_STATUS)
                            } else {
                                rgb(COL_TEXT)
                            })
                            .child(input),
                    ),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = gpui::Bounds::centered(None, gpui::size(px(720.), px(520.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("pi-flash".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(Chat::new),
        )
        .unwrap();
        cx.activate(true);
    });
}

