use aho_corasick::AhoCorasick;
use rust_i18n::t;
use std::{ops::Range, rc::Rc};

use gpui::{
    actions, canvas, div, prelude::FluentBuilder as _, App, AnyElement, AppContext as _, Context,
    Empty, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyBinding,
    ParentElement as _, Pixels, Render, Styled, Subscription, Window,
};
use ropey::Rope;

use crate::{
    actions::SelectPrev,
    button::{Button, ButtonVariants},
    h_flex,
    input::{
        Enter, Escape, IndentInline, InputEvent, InputState, OutdentInline, RopeExt as _, Search,
        SearchReplace, TextInput,
    },
    label::Label,
    v_flex, ActiveTheme, Disableable, IconName, Selectable, Sizable,
};

const KEY_CONTEXT: &'static str = "SearchPanel";

actions!(input, [Tab]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys(vec![KeyBinding::new(
        "shift-enter",
        SelectPrev,
        Some(KEY_CONTEXT),
    )]);
}

#[derive(Debug, Clone)]
pub struct SearchMatcher {
    text: Rope,
    pub query: Option<AhoCorasick>,

    pub(super) matched_ranges: Rc<Vec<Range<usize>>>,
    pub(super) current_match_ix: usize,
    /// Is in replacing mode, if true, the next update will not reset the current match index.
    replacing: bool,
}

impl SearchMatcher {
    pub fn new() -> Self {
        Self {
            text: "".into(),
            query: None,
            matched_ranges: Rc::new(Vec::new()),
            current_match_ix: 0,
            replacing: false,
        }
    }

    /// Update source text and re-match
    pub(crate) fn update(&mut self, text: &Rope) {
        if self.text.eq(text) {
            return;
        }

        self.text = text.clone();
        self.update_matches();
    }

    fn update_matches(&mut self) {
        let mut new_ranges = Vec::new();
        if let Some(query) = &self.query {
            let text = self.text.to_string();
            // FIXME: Use stream find
            let matches = query.stream_find_iter(text.as_bytes());

            for query_match in matches.into_iter() {
                let query_match = query_match.expect("query match for select all action");
                new_ranges.push(query_match.range());
            }
        }
        self.matched_ranges = Rc::new(new_ranges);
        if !self.replacing {
            self.current_match_ix = 0;
            self.replacing = false;
        }
    }

    /// Update the search query and reset the current match index.
    pub fn update_query(&mut self, query: &str, case_insensitive: bool) {
        if query.len() > 0 {
            self.query = Some(
                AhoCorasick::builder()
                    .ascii_case_insensitive(case_insensitive)
                    .build(&[query.to_string()])
                    .expect("failed to build AhoCorasick query in SearchMatcher"),
            );
        } else {
            self.query = None;
        }
        self.update_matches();
    }

    /// Returns the number of matches found.
    #[allow(unused)]
    #[inline]
    fn len(&self) -> usize {
        self.matched_ranges.len()
    }

    fn peek(&self) -> Option<Range<usize>> {
        self.matched_ranges.get(self.current_match_ix + 1).cloned()
    }

    fn label(&self) -> String {
        if self.len() == 0 {
            return "0/0".to_string();
        }
        format!("{}/{}", self.current_match_ix + 1, self.len())
    }

    /// Update the current match index based on the given offset.
    fn update_cursor_by_offset(&mut self, offset: usize) {
        for (ix, range) in self.matched_ranges.iter().enumerate() {
            self.current_match_ix = ix;
            if range.contains(&offset) || range.end >= offset {
                return;
            }
        }
    }
}

impl Iterator for SearchMatcher {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.matched_ranges.is_empty() {
            return None;
        }

        if self.current_match_ix < self.matched_ranges.len().saturating_sub(1) {
            self.current_match_ix += 1;
        } else {
            self.current_match_ix = 0;
        }

        self.matched_ranges.get(self.current_match_ix).cloned()
    }
}

impl DoubleEndedIterator for SearchMatcher {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.matched_ranges.is_empty() {
            return None;
        }

        if self.current_match_ix == 0 {
            self.current_match_ix = self.matched_ranges.len();
        }

        self.current_match_ix -= 1;
        let item = self.matched_ranges[self.current_match_ix].clone();

        Some(item)
    }
}

pub(super) struct SearchPanel {
    editor: Entity<InputState>,
    search_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    case_insensitive: bool,
    replace_mode: bool,
    matcher: SearchMatcher,
    input_width: Pixels,

    open: bool,
    _subscriptions: Vec<Subscription>,
}

impl InputState {
    /// 编辑器搜索面板（Ctrl+F）的输入框当前是否持焦。宿主 app 的帧级焦点
    /// 仲裁用：面板输入框不是 InputState 自身的 focus_handle，不查它会被
    /// 「默认抢回主输入框」的仲裁每帧夺焦，面板一打开就打不了字。
    pub fn search_panel_focused(&self, window: &Window, cx: &App) -> bool {
        self.search_panel
            .as_ref()
            .is_some_and(|p| p.read(cx).inputs_focused(window, cx))
    }

    /// 搜索面板是否展开——宿主操作栏（pi-flash file_nav_bar）据此决定
    /// 是否渲染第二行。
    pub fn search_panel_open(&self, cx: &App) -> bool {
        self.search_panel
            .as_ref()
            .is_some_and(|p| p.read(cx).open)
    }

    /// 面板元素（Entity 本身即元素）：宿主把它作为操作栏第二行渲染——
    /// 此前挂在编辑器 TextInput 顶部（text_input.rs），浮在内容上方、
    /// 与操作栏两块皮；zed 的搜索框是操作栏（面包屑栏）的一部分。
    pub fn search_panel_element(&self) -> Option<AnyElement> {
        self.search_panel.clone().map(|p| p.into_any_element())
    }

    /// Update the search matcher when text changes.
    pub(super) fn update_search(&mut self, cx: &mut App) {
        let Some(search_panel) = self.search_panel.as_ref() else {
            return;
        };

        let text = self.text.clone();
        search_panel.update(cx, |this, _| {
            this.matcher.update(&text);
        });
    }

    pub(super) fn on_action_search(
        &mut self,
        _: &Search,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_search_panel(window, cx, false);
    }

    /// Ctrl+H：搜索 + 替换行展开（zed 同款）。
    pub(super) fn on_action_search_replace(
        &mut self,
        _: &SearchReplace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_search_panel(window, cx, true);
    }

    fn open_search_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        replace: bool,
    ) {
        if !self.searchable {
            return;
        }

        let search_panel = match self.search_panel.as_ref() {
            Some(panel) => panel.clone(),
            None => SearchPanel::new(cx.entity(), window, cx),
        };

        let text = self.text.clone();
        let editor = cx.entity();
        let selected_text = Rope::from(self.selected_text());
        search_panel.update(cx, |this, cx| {
            this.editor = editor;
            this.matcher.update(&text);
            this.show(&selected_text, window, cx, replace);
        });
        self.search_panel = Some(search_panel);
        cx.notify();
    }
}

impl SearchPanel {
    pub fn new(editor: Entity<InputState>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let replace_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("Input.Search Replace With")));

        cx.new(|cx| {
            let _subscriptions =
                vec![
                    cx.subscribe(&search_input, |this: &mut Self, _, ev: &InputEvent, cx| {
                        // Handle search input changes
                        match ev {
                            InputEvent::Change => {
                                this.update_search_query(cx);
                            }
                            _ => {}
                        }
                    }),
                ];

            Self {
                editor,
                search_input,
                replace_input,
                case_insensitive: true,
                replace_mode: false,
                matcher: SearchMatcher::new(),
                open: true,
                input_width: Pixels::ZERO,
                _subscriptions,
            }
        })
    }

    pub(super) fn show(
        &mut self,
        selected_text: &Rope,
        window: &mut Window,
        cx: &mut Context<Self>,
        replace: bool,
    ) {
        self.open = true;
        // ctrl+f 只出搜索行，ctrl+h 出搜索+替换（zed 同款）；面板已开时
        // 重复按键也按新意图归位。zed toggle_replace（buffer_search.rs
        // L1749-1761）：展开替换行时焦点自动落替换框，收起回查询框——
        // 用户无需 Tab 跳栏即可续填
        self.replace_mode = replace;
        if replace {
            self.replace_input.read(cx).focus_handle.focus(window);
        } else {
            self.search_input.read(cx).focus_handle.focus(window);
        }

        self.search_input.update(cx, |this, cx| {
            if selected_text.len() > 0 {
                // Set value will emit to update_search_query
                this.set_value(selected_text.to_string(), window, cx);
            }
            this.select_all(&super::SelectAll, window, cx);
        });
    }

    fn update_search_query(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value();
        let visible_range_offset = self
            .editor
            .read(cx)
            .last_layout
            .as_ref()
            .map(|l| l.visible_range_offset.clone());

        self.matcher
            .update_query(query.as_str(), self.case_insensitive);

        if let Some(visible_range_offset) = visible_range_offset {
            self.matcher
                .update_cursor_by_offset(visible_range_offset.start);
        }
        cx.notify();
    }

    pub(super) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.editor.read(cx).focus_handle.focus(window);
        cx.notify();
    }

    fn on_action_prev(&mut self, _: &SelectPrev, window: &mut Window, cx: &mut Context<Self>) {
        self.prev(window, cx);
    }

    fn on_action_next(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        self.next(window, cx);
    }

    fn on_action_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        self.hide(window, cx);
    }

    // zed buffer_search.rs（scroll 之外第二处字段焦点语义）tab/backtab/
    // cycle_field（L1661-1690）移植：Tab/shift-tab 在面板字段间轮转
    // （查询框 → 替换框 → 编辑器本体），与 zed 同款走 capture 阶段截获
    // 编辑器既有动作（PF：tab→IndentInline、shift-tab→OutdentInline，
    // input state.rs L145/146 绑定；zed：editor::Tab/editor::Backtab）——
    // 焦点在面板内时编辑器收不到（不误触 indent/outdent）；焦点落到编辑
    // 器后面板退出焦点链，两键恢复原语义。上游原 on_action_tab 一步甩编
    // 辑器（无替换框中转），与用户肌肉记忆相悖（2026-11-20 报障）。
    fn on_action_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_field(true, window, cx);
    }

    fn on_action_backtab(&mut self, _: &OutdentInline, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_field(false, window, cx);
    }

    fn cycle_field(&mut self, next: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut handles = vec![self.search_input.read(cx).focus_handle(cx)];
        if self.replace_mode {
            handles.push(self.replace_input.read(cx).focus_handle(cx));
        }
        handles.push(self.editor.focus_handle(cx));
        let Some(current) = handles.iter().position(|h| h.is_focused(window)) else {
            return;
        };
        let len = handles.len();
        let new_index = if next {
            (current + 1) % len
        } else if current == 0 {
            len - 1
        } else {
            current - 1
        };
        handles[new_index].focus(window);
        cx.stop_propagation();
    }

    fn prev(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(range) = self.matcher.next_back() {
            self.editor.update(cx, |state, cx| {
                state.scroll_to(range.start, cx);
            });
        }
    }

    fn next(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(range) = self.matcher.next() {
            self.editor.update(cx, |state, cx| {
                state.scroll_to(range.end, cx);
            });
        }
    }

    pub(super) fn matcher(&self) -> Option<&SearchMatcher> {
        if !self.open {
            return None;
        }

        Some(&self.matcher)
    }

    /// 搜索/替换输入框当前是否持焦（面板打开才算）。宿主 app 的帧级焦点
    /// 仲裁靠它判断「不回收」——面板输入框不属于 InputState 自身的
    /// focus_handle，漏判会被抢回默认输入框，面板一打开就打不了字。
    pub fn inputs_focused(&self, window: &Window, cx: &App) -> bool {
        self.open
            && (self
                .search_input
                .read(cx)
                .focus_handle
                .is_focused(window)
                || self
                    .replace_input
                    .read(cx)
                    .focus_handle
                    .is_focused(window))
    }

    fn replace_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new_text = self.replace_input.read(cx).value();
        self.matcher.replacing = true;
        if let Some(range) = self
            .matcher
            .matched_ranges
            .get(self.matcher.current_match_ix)
            .cloned()
        {
            let text_state = self.editor.clone();

            let next_range = self.matcher.peek().unwrap_or(range.clone());
            cx.spawn_in(window, async move |_, cx| {
                cx.update(|window, cx| {
                    text_state.update(cx, |state, cx| {
                        let range_utf16 = state.range_to_utf16(&range);
                        state.scroll_to(next_range.end, cx);
                        state.replace_text_in_range_silent(
                            Some(range_utf16),
                            new_text.as_str(),
                            window,
                            cx,
                        );
                    });
                })
            })
            .detach();
        }
    }

    fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new_text = self.replace_input.read(cx).value();
        self.matcher.replacing = true;
        let ranges = self.matcher.matched_ranges.clone();
        if ranges.is_empty() {
            return;
        }

        let editor = self.editor.clone();
        cx.spawn_in(window, async move |_, cx| {
            cx.update(|window, cx| {
                editor.update(cx, |state, cx| {
                    // Replace from the end to avoid messing up the ranges.
                    let mut rope = state.text.clone();
                    for range in ranges.iter().rev() {
                        rope.replace(range.clone(), new_text.as_str());
                    }
                    state.replace_text_in_range_silent(
                        Some(0..state.text.len()),
                        &rope.to_string(),
                        window,
                        cx,
                    );
                    state.scroll_to(0, cx);
                });
            })
        })
        .detach();
    }
}

impl Focusable for SearchPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search_input.read(cx).focus_handle.clone()
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return Empty.into_any_element();
        }

        let has_matches = self.matcher.len() > 0;

        v_flex()
            .id("search-panel")
            .occlude()
            .track_focus(&self.focus_handle(cx))
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_action_prev))
            .on_action(cx.listener(Self::on_action_next))
            .on_action(cx.listener(Self::on_action_escape))
            // zed 同款 capture 阶段截获（buffer_search.rs L462-463）：焦点
            // 在查询/替换框时先于 InputState 自身的 indent/outdent 处理
            .capture_action(cx.listener(Self::on_action_tab))
            .capture_action(cx.listener(Self::on_action_backtab))
            .font_family(".SystemUIFont")
            .items_center()
            // 内边距对齐操作栏行（pl 12 = 面包屑文字左缘，pr 8 = 右侧图标
            // 列），× 与上方图标列同缘
            .pl_3()
            .pr_2()
            .py_1p5()
            .w_full()
            .gap_1()
            // zed 同款：全宽平贴、背景与上方操作栏（面包屑栏）同色；
            // 不做浮卡（popover 底/圆角/下边框的卡片感已按用户口径撤掉），
            // 层次全靠输入框自己的边框
            .bg(cx.theme().editor_background())
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .gap_1()
                            .child(
                                TextInput::new(&self.search_input)
                                    .focus_bordered(false)
                                    .suffix(
                                        Button::new("case-insensitive")
                                            .selected(!self.case_insensitive)
                                            .small()
                                            .compact()
                                            .ghost()
                                            .tooltip(t!("Input.Search Match Case"))
                                            .icon(IconName::CaseSensitive)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.case_insensitive = !this.case_insensitive;
                                                this.update_search_query(cx);
                                                cx.notify();
                                            })),
                                    )
                                    .w_full()
                                    .shadow_none(),
                            )
                            .child(
                                canvas(
                                    {
                                        let view = cx.entity();
                                        move |bounds, _, cx| {
                                            view.update(cx, |r, _| {
                                                r.input_width = bounds.size.width
                                            })
                                        }
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            ),
                    )
                    .child(
                        Button::new("replace-mode")
                            .small()
                            .ghost()
                            .tooltip(t!("Input.Search Toggle Replace"))
                            .icon(IconName::Replace)
                            .selected(self.replace_mode)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.replace_mode = !this.replace_mode;
                                if this.replace_mode {
                                    this.replace_input.read(cx).focus_handle.focus(window);
                                } else {
                                    this.search_input.read(cx).focus_handle.focus(window);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("prev")
                            .small()
                            .ghost()
                            .tooltip(t!("Input.Search Previous Match"))
                            .icon(IconName::ChevronLeft)
                            .disabled(!has_matches)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prev(window, cx);
                            })),
                    )
                    .child(
                        Button::new("next")
                            .small()
                            .ghost()
                            .tooltip(t!("Input.Search Next Match"))
                            .icon(IconName::ChevronRight)
                            .disabled(!has_matches)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.next(window, cx);
                            })),
                    )
                    .child(
                        Label::new(self.matcher.label())
                            .when(!has_matches, |this| {
                                this.text_color(cx.theme().muted_foreground)
                            })
                            .text_left()
                            .min_w_16(),
                    )
                    .child(div().w_7())
                    .child(
                        Button::new("close")
                            .small()
                            .ghost()
                            .tooltip(t!("Input.Search Close"))
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_action_escape(&Escape, window, cx);
                            })),
                    ),
            )
            .when(self.replace_mode, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            TextInput::new(&self.replace_input)
                                .focus_bordered(false)
                                .w(self.input_width)
                                .shadow_none(),
                        )
                        .child(
                            Button::new("replace-one")
                                .small()
                                .ghost()
                                .tooltip(t!("Input.Replace"))
                                .icon(IconName::Replace)
                                .disabled(!has_matches)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_next(window, cx);
                                })),
                        )
                        .child(
                            Button::new("replace-all")
                                .small()
                                .ghost()
                                .tooltip(t!("Input.Replace All"))
                                .icon(IconName::ReplaceAll)
                                .disabled(!has_matches)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_all(window, cx);
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);

        assert_eq!(matcher.len(), 3);
        let mut matches = matcher.clone();
        assert_eq!(matches.current_match_ix, 0);
        assert_eq!(matches.next(), Some(18..20));
        assert_eq!(matches.next(), Some(23..25));
        assert_eq!(matches.current_match_ix, 2);
        assert_eq!(matches.next(), Some(15..17));
        assert_eq!(matches.current_match_ix, 0);
        assert_eq!(matches.next_back(), Some(23..25));
        assert_eq!(matches.current_match_ix, 2);
        assert_eq!(matches.next_back(), Some(18..20));
        assert_eq!(matches.current_match_ix, 1);
        assert_eq!(matches.next_back(), Some(15..17));
        assert_eq!(matches.current_match_ix, 0);
        assert_eq!(matches.next_back(), Some(23..25));

        matcher.update_query("IS", false);
        assert_eq!(matcher.len(), 0);
        assert_eq!(matcher.next(), None);
        assert_eq!(matcher.next_back(), None);
    }

    #[test]
    fn test_search_label() {
        let mut matcher = SearchMatcher::new();
        matcher.update(&Rope::from("Hello 世界 this is a Is test string."));
        matcher.update_query("Is", true);
        assert_eq!(matcher.label(), "1/3");
        matcher.next();
        assert_eq!(matcher.label(), "2/3");
        matcher.next();
        assert_eq!(matcher.label(), "3/3");
        matcher.next();
        assert_eq!(matcher.label(), "1/3");

        matcher.update_query("IS", false);
        assert_eq!(matcher.label(), "0/0");
    }

    #[test]
    fn test_select_range_start() {
        let mut matcher = SearchMatcher::new();
        matcher.matched_ranges = Rc::new(vec![5..10, 15..20, 25..30]);
        matcher.update_cursor_by_offset(0);
        assert_eq!(matcher.current_match_ix, 0);

        matcher.update_cursor_by_offset(5);
        assert_eq!(matcher.current_match_ix, 0);

        matcher.update_cursor_by_offset(12);
        assert_eq!(matcher.current_match_ix, 1);

        matcher.update_cursor_by_offset(16);
        assert_eq!(matcher.current_match_ix, 1);

        matcher.update_cursor_by_offset(30);
        assert_eq!(matcher.current_match_ix, 2);

        matcher.update_cursor_by_offset(31);
        assert_eq!(matcher.current_match_ix, 2);
    }
}
