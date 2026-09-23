//! pi-flash spike: GPUI chat window bridged to a real `pi --mode rpc` child process.
//!
//! - stdin/stdout JSONL protocol per pi docs/rpc.md
//! - assistant text streams live via `message_update` / `assistantMessageEvent.text_delta`
//! - type a prompt, press Enter; Esc aborts the run; Ctrl+Q quits

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
};

use futures::{
    StreamExt,
    channel::mpsc::{UnboundedReceiver, unbounded},
};
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyDownEvent, ListAlignment, ListState,
    ParentElement, Render, SharedString, Styled, WindowOptions, div, list, prelude::*, px, rgb,
};
use serde_json::json;

// ---------------------------------------------------------------------------
// pi child process plumbing
// ---------------------------------------------------------------------------

struct PiProcess {
    child: Child,
    /// commands to write to pi's stdin (one JSONL record per line)
    cmd_tx: mpsc::Sender<String>,
}

impl Drop for PiProcess {
    fn drop(&mut self) {
        // closing stdin requests orderly shutdown (per rpc.md);
        // kill is the hard fallback for the spike
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_pi() -> std::io::Result<(PiProcess, UnboundedReceiver<String>)> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "pi", "--mode", "rpc", "--no-session"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    // writer thread: owns child stdin
    let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut stdin = stdin;
        for line in cmd_rx {
            if stdin.write_all(line.as_bytes()).is_err() {
                break;
            }
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
    });

    // reader thread: stdout lines -> async channel consumed by the UI task
    let (event_tx, event_rx) = unbounded::<String>();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if event_tx.unbounded_send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok((PiProcess { child, cmd_tx }, event_rx))
}

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
    pi: Option<PiProcess>,
    status: String,
    req_id: u64,
}
impl Chat {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();

        let (pi, event_rx) = match spawn_pi() {
            Ok(pair) => (Some(pair.0), pair.1),
            Err(e) => {
                eprintln!("failed to spawn pi: {e}");
                (None, unbounded().1)
            }
        };

        let connected = pi.is_some();
        cx.spawn(async move |this, cx| {
            consume_pi_events(this, cx, event_rx).await;
        })
        .detach();

        let mut list_state = ListState::new(0, ListAlignment::Bottom, px(1000.));
        list_state.reset(0);

        Self {
            focus,
            input: String::new(),
            messages: Vec::new(),
            list: list_state,
            pi,
            status: if connected {
                "pi starting…".into()
            } else {
                "pi spawn FAILED".into()
            },
            req_id: 0,
        }
    }

    fn push_message(&mut self, role: Role) -> usize {
        self.messages.push(Msg {
            role,
            text: String::new(),
        });
        let ix = self.messages.len() - 1;
        self.list.reset(self.messages.len());
        ix
    }

    fn append_to(&mut self, ix: usize, delta: &str) {
        if let Some(m) = self.messages.get_mut(ix) {
            m.text.push_str(delta);
        }
        // ListState tracks counts only; text growth needs a repaint nudge
        self.list.reset(self.messages.len());
    }

    fn send_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(pi) = &self.pi else {
            self.status = "not connected".into();
            cx.notify();
            return;
        };
        self.req_id += 1;
        let cmd = json!({
            "id": format!("req-{}", self.req_id),
            "type": "prompt",
            "message": text,
        });
        if pi.cmd_tx.send(cmd.to_string()).is_ok() {
            self.input.clear();
            self.status = "waiting for pi…".into();
        } else {
            self.status = "pi stdin closed".into();
        }
        cx.notify();
    }

    fn abort(&mut self, cx: &mut Context<Self>) {
        if let Some(pi) = &self.pi {
            let _ = pi.cmd_tx.send(r#"{"type":"abort"}"#.into());
            self.status = "aborting…".into();
            cx.notify();
        }
    }

    fn on_line(&mut self, line: String, cx: &mut Context<Self>) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            return;
        };
        match v["type"].as_str() {
            Some("response") => {
                let cmd = v["command"].as_str().unwrap_or("?");
                let ok = v["success"].as_bool().unwrap_or(false);
                self.status = if ok {
                    format!("{cmd} ok")
                } else {
                    let err = v["error"].as_str().unwrap_or("unknown error");
                    format!("{cmd} FAILED: {err}")
                };
            }
            Some("message_start") => {
                eprintln!("[dbg] msg_start user_content={:?}", v["message"]["content"]);
                let role = v["message"]["role"].as_str().unwrap_or("");
                match role {
                    "user" => {
                        let text = match &v["message"]["content"] {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Array(blocks) => blocks
                                .iter()
                                .filter_map(|b| b["text"].as_str())
                                .collect::<Vec<_>>()
                                .join(""),
                            _ => String::new(),
                        };
                        let ix = self.push_message(Role::User);
                        self.append_to(ix, &text);
                    }
                    "assistant" => {
                        self.push_message(Role::Assistant);
                    }
                    _ => {}
                }
            }
            Some("message_update") => {
                let ame = &v["assistantMessageEvent"];
                if ame["type"] == "text_delta" {
                    if let Some(delta) = ame["delta"].as_str() {
                        // current assistant message is the last one
                        if let Some(last) = self.messages.last() {
                            let ix = self.messages.len() - 1;
                            if last.role == Role::Assistant {
                                self.append_to(ix, delta);
                            }
                        }
                    }
                }
            }
            Some("message_end") => {
                // authoritative final content could replace accumulated text here;
                // spike keeps the streamed accumulation
                if let Some(content) = v["message"]["content"].as_str() {
                    if let Some(last) = self.messages.last_mut() {
                        if last.role == Role::Assistant && last.text.is_empty() {
                            last.text = content.to_string();
                        }
                    }
                }
            }
            Some("agent_settled") => {
                self.status = "idle".into();
                for (i, m) in self.messages.iter().enumerate() {
                    eprintln!("[dbg] settled msg[{i}] {:?} = {:?}", m.role, m.text);
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

async fn consume_pi_events(
    this: gpui::WeakEntity<Chat>,
    cx: &mut gpui::AsyncApp,
    mut rx: UnboundedReceiver<String>,
) {
    while let Some(line) = rx.next().await {
        let Ok(_) = this.update(cx, |chat, cx| chat.on_line(line, cx)) else {
            break;
        };
    }
    let _ = this.update(cx, |chat, cx| {
        chat.status = "pi exited".into();
        cx.notify();
    });
}

impl Focusable for Chat {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

const COL_BG: u32 = 0x1a1b1e;
const COL_PANEL: u32 = 0x232428;
const COL_TEXT: u32 = 0xd7dadd;
const COL_USER: u32 = 0x8ab4f8;
const COL_ASSISTANT: u32 = 0x81c995;
const COL_STATUS: u32 = 0x9aa0a6;

fn render_msg(m: &Msg, _ix: usize) -> impl IntoElement {
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
        .child(
            div()
                .text_color(rgb(COL_TEXT))
                .child(SharedString::from(m.text.clone())),
        )
}

impl Render for Chat {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("pi-flash spike"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(COL_STATUS))
                            .child(format!("pi rpc | {status}")),
                    ),
            )
            // message list (bottom-aligned: sticks to the newest message)
            .child(
                list(self.list.clone(), move |ix, _window, cx| {
                    let chat = entity.read(cx);
                    match chat.messages.get(ix) {
                        Some(m) => div().w_full().child(render_msg(m, ix)).into_any_element(),
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
                    .gap_2()
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
                                        let printable =
                                            k.chars().count() == 1 && k.chars().next().is_some()
                                                && !ev.keystroke.modifiers.control
                                                && !ev.keystroke.modifiers.alt;
                                        if printable {
                                            this.input.push(k.chars().next().unwrap());
                                            cx.notify();
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

// ---------------------------------------------------------------------------
// app entry
// ---------------------------------------------------------------------------

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = gpui::Bounds::centered(None, gpui::size(px(720.), px(520.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("pi-flash spike".into()),
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
