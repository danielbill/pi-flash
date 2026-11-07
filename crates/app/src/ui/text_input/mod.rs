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

use gpui::{App, Context, FocusHandle, SharedString, Window, prelude::*};
use gpui_component::input::{InputEvent, InputState};

use std::ops::Range;

mod render;

/// fires after every user value mutation; receives the new value so callers
/// never need to re-read the entity (re-entrant reads from inside the
/// input's own event handling would panic)
type Changed = Box<dyn Fn(&str, &mut App)>;
/// Enter (single-line input); receives the current value
type Submitted = Box<dyn Fn(&str, &mut App)>;
/// Escape (propagates out of the inner widget; IME-cancel keeps priority)
type Escaped = Box<dyn Fn(&mut App)>;
/// focus left the input; receives the current value (rename-in-row commits
/// on blur — the value is passed through so callbacks never re-read the
/// entity, same rule as Changed/Submitted)
type Blurred = Box<dyn Fn(&str, &mut App)>;

pub struct TextInput {
    fallback_focus: FocusHandle,
    state: Option<gpui::Entity<InputState>>,
    // config (buffered until the inner state exists)
    placeholder: Option<SharedString>,
    masked: bool,
    numeric: bool,
    multiline: bool,
    /// 多行固定行数档（auto_grow(n, n)，内件自算高）：None = 普通 multiline
    ///（容器 170px 固定高）。
    lines: Option<usize>,
    /// 紧凑档（22px 高 + XSmall 内距）：文件树行内改名用，塞进 24px 行
    compact: bool,
    /// 平面档（无组件底色/边框）：行内编辑的 chrome（边框/底色）由所在
    /// 行提供，Zed project_panel 编辑行同款——行 = 唯一的框
    flat: bool,
    /// 设计稿表单档（040-2 pi.dev 目录页）：44px 高 + 等宽 15px + 零圆角。
    /// 只改「高度/字号/族」，chrome 仍由宿主给（配 flat 用）。
    form: bool,
    /// 光标色覆盖（None = 跟 gpui-component 主题 `caret`）。深肤页（040-2
    /// 原生市场）需要浅色光标，而主题 token 是按 PF 主题算的。
    caret_color: Option<u32>,
    /// 占位符色覆盖（None = 主题 `muted_foreground`）：同理，深肤页要用页内灰。
    placeholder_color: Option<u32>,
    select_all_on_focus: bool,
    /// 只读态（042-1：内置模型参数由 pi 定义，PF 只看不改）：内件 disabled +
    /// 半透明；焦点/点击/输入全部由 gpui-component 拦下
    disabled: bool,
    select_all_done: bool,
    want_focus: bool,
    // dirty buffers applied on render (inner setters need &mut Window)
    pending_value: Option<String>,
    pending_placeholder: Option<SharedString>,
    pending_masked: Option<bool>,
    /// 选区在 set_value 之后落（内件建好才有 text 可选）；Zed project_panel
    /// 行内改名选 stem 用
    pending_select: Option<Range<usize>>,
    want_select_all: bool,
    /// mirrored inner value so `value()` works without cx (call-site compat)
    value: String,
    on_change: Option<Changed>,
    on_submit: Option<Submitted>,
    on_escape: Option<Escaped>,
    on_blur: Option<Blurred>,
}

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            fallback_focus: cx.focus_handle(),
            state: None,
            placeholder: None,
            masked: false,
            numeric: false,
            multiline: false,
            lines: None,
            compact: false,
            flat: false,
            form: false,
            caret_color: None,
            placeholder_color: None,
            select_all_on_focus: false,
            disabled: false,
            select_all_done: false,
            want_focus: false,
            pending_value: None,
            pending_placeholder: None,
            pending_masked: None,
            pending_select: None,
            want_select_all: false,
            value: String::new(),
            on_change: None,
            on_submit: None,
            on_escape: None,
            on_blur: None,
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

    /// 多行模式（Enter 换行不提交；渲染高度约 8 行，MCP 粘贴框用）。
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// 多行固定 n 行高（auto_grow(n, n)：内件自算高 = n × 行高，超行内滚）。
    /// 隐含 multiline。会话 hover 卡改名框（三行高）用。
    pub fn lines(mut self, n: usize) -> Self {
        self.multiline = true;
        self.lines = Some(n.max(1));
        self
    }

    /// 紧凑单行档：20px 高（与 XSmall 内件同高零富余）+ XSmall 内距 +
    /// ui(11) 字号（随面板字号缩放，与宿主行同号），文件树行内改名用。
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// 平面档：内件不出底色/边框，由宿主行提供 chrome（行内编辑用）。
    pub fn flat(mut self) -> Self {
        self.flat = true;
        self
    }

    /// 设计稿表单档（040-2）：44px 高、等宽字体 15px、零圆角。**只**用于
    /// 照抄设计稿的表单行；配 `flat()` 让宿主（设计稿的 44px 框）出 chrome。
    pub fn form(mut self) -> Self {
        self.form = true;
        self
    }

    /// 光标色覆盖（None = 跟 gpui-component 主题 `caret`）：深肤页（040-2
    /// 原生市场）需浅色光标，而主题 token 是按 PF 主题算的。
    pub fn caret_color(mut self, rgb24: u32) -> Self {
        self.caret_color = Some(rgb24);
        self
    }

    /// 占位符色覆盖（None = 主题 `muted_foreground`）：深肤页用页内灰。
    pub fn placeholder_color(mut self, rgb24: u32) -> Self {
        self.placeholder_color = Some(rgb24);
        self
    }

    /// pi-web rename parity: when focus first lands, the whole value is
    /// selected so typing replaces it（光标锚在选区**起点**——头部可见，
    /// vendor `set_selected_range_reversed` 缝，2026-10-13 用户定夺）.
    pub fn select_all_on_focus(mut self) -> Self {
        self.select_all_on_focus = true;
        self
    }

    /// Focus the inner input on the next render: the request is honored
    /// after the inner state is created (otherwise the focused handle would
    /// not be in that frame's element tree — the frame's focus path empties
    /// and the previous owner blurs for nothing). 一次性首发聚焦用：popup
    /// 过滤框；文件树行内编辑 = Zed project_panel `focus_filename_editor`
    /// 请求同位（打开当帧请求、内件就绪即兑现）。
    pub fn focus_soon(&mut self, _cx: &mut Context<Self>) {
        self.want_focus = true;
        _cx.notify();
    }

    pub fn on_change(mut self, cb: Changed) -> Self {
        self.on_change = Some(cb);
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

    pub fn set_on_blur(&mut self, cb: Blurred) {
        self.on_blur = Some(cb);
    }

    /// 下拉字段用：设置占位符（下次 render 应用）。
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

    /// 只读开关：禁用后内件不吃焦点/输入（值仍可由 set_value 回填）。
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    pub fn set_value(&mut self, v: String, cx: &mut Context<Self>) {
        self.value = v.clone();
        self.pending_value = Some(v);
        cx.notify();
    }

    /// Select a byte range of the value (applied after the pending value
    /// lands — Zed project_panel rename parity: stem selection).
    pub fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.pending_select = Some(range);
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

    /// 内件是否已创建（焦点级联的判据：未建时不能把 fallback 句柄推上
    /// 焦点位——它不在元素树里，本帧焦点路径变空，会白白触发一轮 blur）。
    pub fn has_inner(&self) -> bool {
        self.state.is_some()
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
            if self.multiline {
                st = match self.lines {
                    // auto_grow(n, n)：内件自算高（min = n × 行高），容器不设高
                    Some(n) => st.auto_grow(n, n),
                    None => st.multi_line(),
                };
            }
            st
        });
        cx.subscribe(&state, |this, entity, event: &InputEvent, cx| {
            this.on_inner_event(entity, event, cx);
        })
        .detach();
        // pre-init 值（prefill）不在这里灌：统一走 render 的 pending_value
        // 应用——那里带 select_all_on_focus 的建态即选（vendor prepaint 的
        // is_selected_all 门闸依赖它抢在首次布局前落位）。ensure_state 只被
        // render 调用，应用时机不变。
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
            InputEvent::Blur => {
                if let Some(cb) = self.on_blur.take() {
                    let v = self.value.clone();
                    cb(&v, cx);
                    self.on_blur = Some(cb);
                }
            }
        }
    }
}

