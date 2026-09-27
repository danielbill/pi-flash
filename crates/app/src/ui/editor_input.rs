//! EditorInputElement (the composer's element shell) + Chat's
//! EntityInputHandler (UTF-16 text input contract for IME/selection).
//! Split out of main.rs for the file-size budget.

use crate::*;

impl gpui::EntityInputHandler for Chat {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<String> {
        let text: Vec<u16> = self.input.encode_utf16().collect();
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
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        // hand-rolled editor: caret at end, empty selection
        let end = self.input.encode_utf16().count();
        Some(gpui::UTF16Selection { range: end..end, reversed: false })
    }

    fn marked_text_range(
        &self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.ime_marked.clone()
    }

    fn unmark_text(&mut self, _window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) {
        self.ime_marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        match range.or_else(|| self.ime_marked.clone()) {
            Some(r) => {
                let start = self.utf16_to_char_offset(r.start);
                let end = self.utf16_to_char_offset(r.end);
                self.input.replace_range(start..end, text);
            }
            None => self.input.push_str(text),
        }
        self.ime_marked = None;
        self.menu_ix = 0;
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        // composition update: swap the marked span for the new composition
        // string, then re-mark it
        let range = range.or_else(|| self.ime_marked.clone());
        let start = match &range {
            Some(r) => self.utf16_to_char_offset(r.start),
            None => self.input.chars().count(),
        };
        let end = range
            .as_ref()
            .map(|r| self.utf16_to_char_offset(r.end))
            .unwrap_or(start);
        self.input.replace_range(start..end, new_text);
        let start_u16 = self.input.chars().take(start).map(char::len_utf16).sum();
        let new_len = new_text.encode_utf16().count();
        self.ime_marked = Some(start_u16..start_u16 + new_len);
    }

    fn bounds_for_range(
        &mut self,
        _range: std::ops::Range<usize>,
        element_bounds: gpui::Bounds<gpui::Pixels>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        // IME candidate window anchors to the editor container
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Chat {
    /// utf16 offset -> char offset for self.input
    fn utf16_to_char_offset(&self, u16_offset: usize) -> usize {
        let mut u16_count = 0usize;
        for (char_ix, ch) in self.input.chars().enumerate() {
            if u16_count >= u16_offset {
                return char_ix;
            }
            u16_count += ch.len_utf16();
        }
        self.input.chars().count()
    }
}

/// Invisible paint-phase element that registers the chat editor as the
/// window's InputHandler when focused (required for Windows IME).
pub(crate) struct EditorInputElement {
    focus: gpui::FocusHandle,
    view: gpui::Entity<Chat>,
    interactivity: gpui::Interactivity,
}

impl EditorInputElement {
    pub(crate) fn new(view: gpui::Entity<Chat>, focus: gpui::FocusHandle) -> Self {
        Self {
            focus,
            view,
            interactivity: gpui::Interactivity::new(),
        }
    }
}

impl gpui::IntoElement for EditorInputElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Styled for EditorInputElement {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl gpui::Element for EditorInputElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
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
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        window.handle_input(
            &self.focus,
            gpui::ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
    }
}