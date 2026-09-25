//! TextInput — the app-wide single-line text input (pi-web parity note:
//! pi-web leans on HTML `<input>` natively providing focus, caret,
//! selection, clipboard and IME; GPUI has no such builtin, so this facade
//! adapts **gpui-component's InputState** (battle-tested widget: real
//! selection highlight, arrow/word movement, copy/cut/paste, undo, Windows
//! IME) behind the app's own constructor/config API so call sites stay
//! framework-agnostic (see ARCHITECTURE.md §4).
//!
//! The inner `InputState` needs `&mut Window` at construction, so it is
//! created lazily on first render; config set before that is buffered and
//! applied at creation / sync time.

use gpui::{
    App, Context, FocusHandle, Focusable as _, KeyDownEvent, ParentElement, Render, SharedString,
    Styled, Window, div, prelude::*,
};
use gpui_component::input::{InputEvent, InputState, SelectAll, TextInput as GpInput};

use crate::theme::theme as T;

/// fires after every user value mutation; receives the new value so callers
/// never need to re-read the entity (re-entrant reads from inside the
/// input's own event handling would panic)
type Changed = Box<dyn Fn(&str, &mut App)>;
/// Enter (single-line input); receives the current value
type Submitted = Box<dyn Fn(&str, &mut App)>;
/// Escape (propagates out of the inner widget; IME-cancel keeps priority)
type Escaped = Box<dyn Fn(&mut App)>;

pub struct TextInput {
    fallback_focus: FocusHandle,
    state: Option<gpui::Entity<InputState>>,
    // config (buffered until the inner state exists)
    placeholder: Option<SharedString>,
    masked: bool,
    numeric: bool,
    select_all_on_focus: bool,
    select_all_done: bool,
    // dirty buffers applied on render (inner setters need &mut Window)
    pending_value: Option<String>,
    pending_placeholder: Option<SharedString>,
    pending_masked: Option<bool>,
    want_select_all: bool,
    /// mirrored inner value so `value()` works without cx (call-site compat)
    value: String,
    on_change: Option<Changed>,
    on_submit: Option<Submitted>,
    on_escape: Option<Escaped>,
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            fallback_focus: cx.focus_handle(),
            state: None,
            placeholder: None,
            masked: false,
            numeric: false,
            select_all_on_focus: false,
            select_all_done: false,
            pending_value: None,
            pending_placeholder: None,
            pending_masked: None,
            want_select_all: false,
            value: String::new(),
            on_change: None,
            on_submit: None,
            on_escape: None,
        }
    }

    pub fn placeholder(mut self, ph: impl Into<SharedString>) -> Self {
        self.placeholder = Some(ph.into());
        self
    }

    /// Show `•` instead of the value while not composing (API keys).
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// Reject non-ASCII-digit input (numeric fields).
    pub fn numeric(mut self, numeric: bool) -> Self {
        self.numeric = numeric;
        self
    }

    /// pi-web rename parity: when focus first lands, the whole value is
    /// selected so typing replaces it (←/→ collapse, Backspace clears).
    pub fn select_all_on_focus(mut self) -> Self {
        self.select_all_on_focus = true;
        self
    }

    pub fn on_change(mut self, cb: Changed) -> Self {
        self.on_change = Some(cb);
        self
    }

    pub fn on_submit(mut self, cb: Submitted) -> Self {
        self.on_submit = Some(cb);
        self
    }

    pub fn on_escape(mut self, cb: Escaped) -> Self {
        self.on_escape = Some(cb);
        self
    }

    /// Post-construction callback wiring (for owners created after the
    /// input entity, e.g. the root entity wiring its search field).
    pub fn set_on_change(&mut self, cb: Changed) {
        self.on_change = Some(cb);
    }

    pub fn set_on_submit(&mut self, cb: Submitted) {
        self.on_submit = Some(cb);
    }

    pub fn set_on_escape(&mut self, cb: Escaped) {
        self.on_escape = Some(cb);
    }

    pub fn set_placeholder(&mut self, ph: Option<SharedString>) {
        self.placeholder = ph.clone();
        if self.state.is_some() {
            self.pending_placeholder = ph;
        }
    }

    pub fn set_masked(&mut self, masked: bool, _cx: &mut Context<Self>) {
        self.masked = masked;
        if self.state.is_some() {
            self.pending_masked = Some(masked);
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, v: String, cx: &mut Context<Self>) {
        self.value = v.clone();
        self.pending_value = Some(v);
        cx.notify();
    }

    /// Focus handle before the inner state exists (one-frame fallback).
    pub fn focus_handle(&self) -> FocusHandle {
        self.fallback_focus.clone()
    }

    /// The inner widget's focus handle once initialized.
    pub fn focus_handle_in(&self, cx: &App) -> FocusHandle {
        use gpui::Focusable as _;
        self.state
            .as_ref()
            .map(|s| s.read(cx).focus_handle(cx))
            .unwrap_or_else(|| self.fallback_focus.clone())
    }

    /// Create (once) + focus the inner state immediately — for callers that
    /// must focus programmatically (session search toggle).
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.ensure_state(window, cx);
        if let Some(state) = state {
            state.update(cx, |st, scx| st.focus(window, scx));
        }
        cx.notify();
    }

    fn ensure_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Entity<InputState>> {
        if let Some(state) = &self.state {
            return Some(state.clone());
        }
        let placeholder = self.placeholder.clone().unwrap_or_default();
        let masked = self.masked;
        let state = cx.new(|scx| {
            let mut st = InputState::new(window, scx).placeholder(placeholder);
            if masked {
                st = st.masked(true);
            }
            st
        });
        cx.subscribe(&state, |this, entity, event: &InputEvent, cx| {
            this.on_inner_event(entity, event, cx);
        })
        .detach();
        // apply a pre-init value (prefill) now that we have a window
        if let Some(v) = self.pending_value.take() {
            self.value = v.clone();
            state.update(cx, |st, scx| st.set_value(v, window, scx));
        }
        self.state = Some(state.clone());
        Some(state)
    }

    fn on_inner_event(
        &mut self,
        entity: gpui::Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let mut v = entity.read(cx).value().to_string();
                if self.numeric {
                    let digits: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
                    if digits != v {
                        // reject the rejected characters
                        self.pending_value = Some(digits.clone());
                    }
                    v = digits;
                }
                self.value = v.clone();
                let cb = self.on_change.take();
                if let Some(cb) = &cb {
                    cb(&v, cx);
                }
                self.on_change = cb;
            }
            InputEvent::PressEnter { .. } => {
                let v = self.value.clone();
                let cb = self.on_submit.take();
                if let Some(cb) = &cb {
                    cb(&v, cx);
                }
                self.on_submit = cb;
            }
            InputEvent::Focus => {
                if self.select_all_on_focus && !self.select_all_done && !self.value.is_empty() {
                    self.select_all_done = true;
                    self.want_select_all = true; // dispatched in render (needs window)
                    cx.notify();
                }
            }
            InputEvent::Blur => {}
        }
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.ensure_state(window, cx).expect("input state");
        // apply dirty buffers (inner setters need the window)
        if let Some(v) = self.pending_value.take() {
            self.value = v.clone();
            state.update(cx, |st, scx| st.set_value(v, window, scx));
        }
        if let Some(ph) = self.pending_placeholder.take() {
            state.update(cx, |st, scx| st.set_placeholder(ph, window, scx));
        }
        if let Some(m) = self.pending_masked.take() {
            state.update(cx, |st, scx| st.set_masked(m, window, scx));
        }
        if self.want_select_all {
            self.want_select_all = false;
            window.dispatch_action(Box::new(SelectAll), cx);
        }

        let t = T();
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        div()
            .w_full()
            .h(gpui::px(30.))
            // inherit into the inner widget's text shaping
            .font_family("Consolas")
            .text_size(gpui::px(12.))
            // escape bubbles up from the inner widget (it only unmarks IME
            // composition and propagates) — turn it into the app callback
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                if ev.keystroke.key == "escape" {
                    let cb = this.on_escape.take();
                    if let Some(cb) = &cb {
                        cb(cx);
                    }
                    this.on_escape = cb;
                    cx.stop_propagation();
                }
            }))
            .child(
                GpInput::new(&state)
                    .appearance(true)
                    .bordered(true)
                    .map(|input| {
                        input
                            .bg(gpui::rgb(t.bg_panel))
                            .border_color(gpui::rgb(if focused { t.accent } else { t.border }))
                            .rounded(gpui::px(5.))
                    }),
            )
    }
}
