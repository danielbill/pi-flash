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
        // utf16 代理对必须整体重组：逐 u16 char::from_u32 会把中文/emoji
        // 拆成一串 U+FFFD（TSF 取标区文本时必踩）
        let slice: String = String::from_utf16_lossy(text.get(range.start..range.end)?);
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
        cx: &mut gpui::Context<Self>,
    ) {
        match range.or_else(|| self.ime_marked.clone()) {
            Some(r) => {
                let start = self.utf16_to_byte_offset(r.start);
                let end = self.utf16_to_byte_offset(r.end);
                self.safe_replace_range(start, end, text);
            }
            None => self.input.push_str(text),
        }
        self.ime_marked = None;
        self.menu_ix = 0;
        // 漏 notify = 文本进了 model 但界面不动
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        // composition update: swap the marked span for the new composition
        // string, then re-mark it
        let range = range.or_else(|| self.ime_marked.clone());
        let start = match &range {
            Some(r) => self.utf16_to_byte_offset(r.start),
            None => self.input.len(),
        };
        let end = range
            .as_ref()
            .map(|r| self.utf16_to_byte_offset(r.end))
            .unwrap_or(start);
        // 字节边界 + 夹取：平台给的 range 可能越界/反转，replace_range
        // panic 会直接带走进程（STATUS_STACK_BUFFER_OVERRUN）
        let used_start = self.safe_replace_range(start, end, new_text);
        let start_u16 = self.input[..used_start].encode_utf16().count();
        let new_len = new_text.encode_utf16().count();
        self.ime_marked = Some(start_u16..start_u16 + new_len);
        // 漏 notify = 组合中的拼音/候选词不显示
        cx.notify();
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
    /// utf16 offset -> **字节** offset（`String::replace_range` 的坐标）。
    /// 注意别用 char 下标：中文/emoji 一个 char 3~4 字节，
    /// 拿 char 下标去切会 panic `assertion failed: self.is_char_boundary(n)`。
    fn utf16_to_byte_offset(&self, u16_offset: usize) -> usize {
        let mut u16_count = 0usize;
        for (byte_ix, ch) in self.input.char_indices() {
            if u16_count >= u16_offset {
                return byte_ix;
            }
            u16_count += ch.len_utf16();
        }
        self.input.len()
    }

    /// 把 `start..end` 夹到合法字节区间后替换，返回实际使用的 start（字节）。
    /// IME/TSF 给的 range 不保证在界内、不保证 start <= end —— 这里不夹，
    /// `replace_range` 会 panic 并带走整个进程。
    fn safe_replace_range(&mut self, start: usize, end: usize, with: &str) -> usize {
        let len = self.input.len();
        let start = floor_char_boundary(&self.input, start.min(len));
        let end = ceil_char_boundary(&self.input, end.clamp(start, len));
        self.input.replace_range(start..end, with);
        start
    }
}

/// 向下取到最近字符边界（`str::floor_char_boundary` 未稳定前的等价物）。
fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 向上取到最近字符边界。
fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
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