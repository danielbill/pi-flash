//! TextInput — the app-wide text input component (pi-web parity note:
//! pi-web has no shared input component because HTML `<input>` natively
//! provides focus, caret and IME; GPUI has no such builtin, so this module
//! IS that builtin). Every text field in the app must render this instead
//! of hand-rolled `on_key_down` char matching (see ARCHITECTURE.md).
//!
//! Owns per-instance: focus handle, value, IME marked range, blinking
//! caret, placeholder/masking/numeric filtering and change/submit/escape
//! callbacks.

use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels,
    prelude::*, Render, SharedString, Styled, Window, div,
};

use crate::theme::theme as T;

/// Instance counter for unique element ids (multiple inputs can be mounted
/// in one frame, e.g. settings forms).
static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// fires after every user value mutation (typing, IME commit, backspace);
/// receives the new value so callers never need to re-read the entity
/// (re-entrant reads from inside the input's own listener would panic)
type Changed = Box<dyn Fn(&str, &mut App)>;
/// Enter (single-line input); receives the current value — see Changed
type Submitted = Box<dyn Fn(&str, &mut App)>;
/// Escape
type Escaped = Box<dyn Fn(&mut App)>;

pub struct TextInput {
    focus: FocusHandle,
    id: SharedString,
    /// utf16 range of the active IME composition
    marked: Option<std::ops::Range<usize>>,
    caret_on: bool,
    /// refreshed every render; the blink pump only notifies while focused
    focused_hint: bool,
    numeric: bool,
    masked: bool,
    /// next focus (and IME query) reports the whole value selected
    select_all: bool,
    placeholder: Option<SharedString>,
    /// fires after every value mutation (typing, IME commit, programmatic)
    on_change: Option<Changed>,
    /// Enter (single-line input)
    on_submit: Option<Submitted>,
    on_escape: Option<Escaped>,
    /// the field value (public read via `value()`; mutate via `set_value`)
    pub(crate) value: String,
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let input = Self {
            focus: cx.focus_handle(),
            id: SharedString::from(format!("ti-{id}")),
            marked: None,
            caret_on: true,
            focused_hint: false,
            numeric: false,
            masked: false,
            select_all: false,
            placeholder: None,
            on_change: None,
            on_submit: None,
            on_escape: None,
            value: String::new(),
        };
        // caret blink pump (530ms, mirrors the chat editor's cadence);
        // repaints only while this input owns focus
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(530))
                .await;
            let Ok(()) = this.update(cx, |ti, cx| {
                if ti.focused_hint {
                    ti.caret_on = !ti.caret_on;
                    cx.notify();
                } else {
                    ti.caret_on = true;
                }
            }) else {
                return;
            };
        })
        .detach();
        input
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

    /// Mount-selected mode (pi-web rename input parity: autoFocus + select;
    /// the first typed character replaces the whole value).
    pub fn select_all_on_focus(mut self) -> Self {
        self.select_all = true;
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
        self.placeholder = ph;
    }

    pub fn set_masked(&mut self, masked: bool) {
        self.masked = masked;
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, v: String, cx: &mut Context<Self>) {
        self.value = v;
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    /// utf16 offset -> char offset for `value`
    fn utf16_to_char_offset(&self, u16_offset: usize) -> usize {
        let mut u16_count = 0usize;
        for (char_ix, ch) in self.value.chars().enumerate() {
            if u16_count >= u16_offset {
                return char_ix;
            }
            u16_count += ch.len_utf16();
        }
        self.value.chars().count()
    }

    fn fire(cb: &Option<Changed>, value: &str, cx: &mut App) {
        if let Some(cb) = cb {
            cb(value, cx);
        }
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = T();
        let focused = self.focus.is_focused(window);
        self.focused_hint = focused;
        let composing = self.marked.is_some();
        let empty = self.value.is_empty() && !composing;

        let shown: SharedString = if empty {
            self.placeholder.clone().unwrap_or_default()
        } else if self.masked && !composing {
            "\u{2022}".repeat(self.value.chars().count()).into()
        } else {
            self.value.clone().into()
        };

        let field_focus = self.focus.clone();
        let show_caret = focused && self.caret_on && !composing;
        let entity = cx.entity();

        div()
            .id(self.id.clone())
            .track_focus(&self.focus)
            // click anywhere in the field focuses it (fixes "点击没有 focus")
            .on_mouse_down(MouseButton::Left, move |_, window, _cx| {
                window.focus(&field_focus);
            })
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                if this.marked.is_some() {
                    return; // IME owns the composition keys
                }
                match ev.keystroke.key.as_str() {
                    "enter" => {
                        let v = this.value.clone();
                        if let Some(cb) = this.on_submit.as_ref() {
                            cb(&v, cx);
                        }
                    }
                    "escape" => {
                        if let Some(cb) = this.on_escape.as_ref() {
                            cb(cx);
                        }
                    }
                    "backspace" => {
                        if this.select_all && !this.value.is_empty() {
                            this.value.clear(); // select-all + backspace = clear
                        } else {
                            this.value.pop();
                        }
                        this.select_all = false;
                        let v = this.value.clone();
                        TextInput::fire(&this.on_change, &v, cx);
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .flex()
            .items_center()
            .min_h(gpui::px(30.))
            .px(gpui::px(9.))
            .rounded(gpui::px(5.))
            .border_1()
            .border_color(gpui::rgb(if focused { t.accent } else { t.border }))
            .bg(gpui::rgb(t.bg_panel))
            .overflow_hidden()
            .font_family("Consolas")
            .text_size(gpui::px(12.))
            .line_height(gpui::relative(1.4))
            .text_color(gpui::rgb(if empty { t.text_dim } else { t.text }))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(shown),
            )
            .when(show_caret, |d| {
                d.child(
                    div()
                        .w(gpui::px(1.))
                        .h(gpui::px(14.))
                        .flex_shrink_0()
                        .bg(gpui::rgb(t.text)),
                )
            })
            // paint-phase IME registration (absolute overlay, no layout)
            .child(
                TextInputElement::new(entity, self.focus.clone())
                    .absolute()
                    .inset_0(),
            )
    }
}

impl gpui::EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text: Vec<u16> = self.value.encode_utf16().collect();
        let slice: String = text
            .get(range.start..range.end)?
            .iter()
            .map(|&u| char::from_u32(u as u32).unwrap_or('\u{fffd}'))
            .collect();
        adjusted_range.replace(range);
        Some(slice)
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        let end = self.value.encode_utf16().count();
        // select_all_on_focus: report the whole value selected until the
        // user edits (platform IMEs replace the selection on input)
        if self.select_all && end > 0 {
            return Some(gpui::UTF16Selection { range: 0..end, reversed: false });
        }
        // single-line field: caret at end, empty selection
        Some(gpui::UTF16Selection { range: end..end, reversed: false })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.marked.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = if self.numeric {
            text.chars().filter(|c| c.is_ascii_digit()).collect::<String>()
        } else {
            text.to_string()
        };
        if text.is_empty() && range.is_none() && !self.numeric {
            return;
        }
        match range.or_else(|| self.marked.clone()) {
            Some(r) => {
                let start = self.utf16_to_char_offset(r.start);
                let end = self.utf16_to_char_offset(r.end);
                self.value.replace_range(start..end, &text);
            }
            None => self.value.push_str(&text),
        }
        self.marked = None;
        self.select_all = false;
        let v = self.value.clone();
        TextInput::fire(&self.on_change, &v, cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // composition update: swap the marked span for the new composition
        // string, then re-mark it
        let range = range.or_else(|| self.marked.clone());
        let start = match &range {
            Some(r) => self.utf16_to_char_offset(r.start),
            None => self.value.chars().count(),
        };
        let end = range
            .as_ref()
            .map(|r| self.utf16_to_char_offset(r.end))
            .unwrap_or(start);
        self.value.replace_range(start..end, new_text);
        let start_u16 = self.value.chars().take(start).map(char::len_utf16).sum();
        let new_len = new_text.encode_utf16().count();
        self.marked = Some(start_u16..start_u16 + new_len);
    }

    fn bounds_for_range(
        &mut self,
        _range: std::ops::Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // IME candidate window anchors to the field
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// Invisible paint-phase element that registers the input as the window's
/// InputHandler when focused (required for Windows IME); mirrors the chat
/// editor's EditorInputElement.
pub struct TextInputElement {
    focus: FocusHandle,
    view: gpui::Entity<TextInput>,
    interactivity: gpui::Interactivity,
}

impl TextInputElement {
    pub fn new(view: gpui::Entity<TextInput>, focus: FocusHandle) -> Self {
        Self {
            focus,
            view,
            interactivity: gpui::Interactivity::new(),
        }
    }
}

impl IntoElement for TextInputElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for TextInputElement {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl gpui::Element for TextInputElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| window.request_layout(style, None, cx),
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.handle_input(
            &self.focus,
            gpui::ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
    }
}
