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
use gpui_component::input::{InputEvent, InputState, TextInput as GpInput};

use std::ops::Range;

use crate::appearance::theme::theme as T;

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
    select_all_on_focus: bool,
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
            select_all_on_focus: false,
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

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.ensure_state(window, cx).expect("input state");
        // apply dirty buffers (inner setters need the window)
        if let Some(v) = self.pending_value.take() {
            self.value = v.clone();
            let select_all = self.select_all_on_focus;
            state.update(cx, |st, scx| {
                st.set_value(v.clone(), window, scx);
                // select_all_on_focus 的**建态即选**路径：抢在首个 prepaint
                // 之前把选区落成全选——vendor prepaint 的光标跟随滚动块被
                // is_selected_all 门闸整个跳过。否则 set_value 把光标甩到
                // 文本尾，首帧布局按尾光标滚，CJK 长行 position_for_index
                // 越界外推会把横向滚动推出屏外（改名框一打开空白的根因，
                // 2026-10-13 pif-ui 实测复现）。focus 晚于首帧的场景（弹层
                // 筛选框先建后聚焦）仍走 Focus 事件路径。
                if select_all && !v.is_empty() {
                    let len = st.value().len();
                    st.set_selected_range_reversed(0..len, scx);
                }
            });
            if select_all {
                self.select_all_done = true; // 免 Focus 事件路径重复全选
            }
        }
        if let Some(r) = self.pending_select.take() {
            state.update(cx, |st, scx| st.set_selected_range(r, scx));
        }
        if let Some(ph) = self.pending_placeholder.take() {
            state.update(cx, |st, scx| st.set_placeholder(ph, window, scx));
        }
        if let Some(m) = self.pending_masked.take() {
            state.update(cx, |st, scx| st.set_masked(m, window, scx));
        }
        if self.want_select_all {
            self.want_select_all = false;
            // 全选 + 光标锚起点（vendor selection_reversed）：头部可见，
            // 键入仍整段替换
            let len = state.read(cx).value().len();
            state.update(cx, |st, scx| st.set_selected_range_reversed(0..len, scx));
        }
        if self.want_focus {
            self.want_focus = false;
            // vendor InputState::focus：句柄聚焦 + 光标 blink 同帧启动。
            // 裸 window.focus 会踩 gpui 订阅激活时序：on_focus 监听在
            // SubscriberSet 里以 active=false 插入、cx.defer 才激活，
            // retain 派发跳过未激活者——「建内件当帧 focus」的首发路径
            // 必错过同帧 Focus 相位的 focus-in，blink.visible 恒 false，
            // 新建/改名行光标不闪、直到首个键入 pause() 强制可见
            //（pif-ui 三连 shot 字节不变复现，键入后即刻恢复）
            state.update(cx, |st, scx| st.focus(window, scx));
        }

        let t = T();
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        // 多行：容器给足高度，内件 h_full 撑满（单行保持固定 30px；紧凑
        // 档 20px = vendor XSmall 内件同高，**零富余**——任何一侧都不留
        // 白隙，行内垂直居中交给宿主行 items_center 链）；lines(n) =
        // auto_grow，内件自算高（min = n × 行高），容器不设高、内件不撑满
        let auto_h = self.multiline && self.lines.is_some();
        let (h, fill) = if auto_h {
            (gpui::px(0.), false)
        } else if self.multiline {
            (gpui::px(170.), true)
        } else if self.form {
            (gpui::px(44.), false)
        } else if self.compact {
            (gpui::px(20.), false)
        } else {
            (gpui::px(30.), false)
        };
        let mut input = GpInput::new(&state)
            .appearance(!self.flat)
            .bordered(!self.flat)
            .map(|input| {
                if self.compact {
                    use gpui_component::Sizable as _;
                    input.with_size(gpui_component::Size::XSmall)
                } else {
                    input
                }
            });
        if self.flat {
            // Zed 行内编辑零内距（project_panel 把 Editor 原样塞进 Label 位，
            // 文字起点由宿主行给）：flat 档宿主行已对好起点，内件 XSmall 的
            // input_px(4) 会让改名文字比行标签多缩进 4px（refine_style 在
            // input_px 之后生效，可覆盖）
            input = input.pl_0();
        }
        if !self.flat {
            input = input
                .bg(gpui::rgb(t.bg_panel))
                .border_color(gpui::rgb(if focused { t.accent } else { t.border }))
                .rounded(gpui::px(5.));
        }
        if fill {
            input = input.h_full();
        }
        div()
            .w_full()
            .when(!auto_h, |d| d.h(h))
            // compact：槽高与 XSmall 内件（vendor input_h 20px）同高、零富余
            //（曾 22px，stretch 对定高子件退化为 top-align，2px 全落底部把
            // 输入基线顶高 1px——改名行文字上跳根因）；items_center 兜底。
            // form（040-2）：44px 槽 + 30px 内件，同样必须居中，否则 placeholder
            // 贴着上边（用户实测「文字没垂直居中」）。
            .when(self.compact, |d| d.items_center())
            // form：items_center 对 vendor 内件不生效（内件 layout 高被
            // stretch 成槽高，文字仍按行盒顶对齐），直接用 padding 把 30px
            // 内件压到 44px 槽的垂直正中（(44-30)/2 = 7）
            .when(self.form, |d| d.pt(gpui::px(5.)).pb(gpui::px(5.)))
            // 文字轨 = Zed create_style 同款（editor.rs：SingleLine/AutoHeight →
            // ui_font + UI 字号，Full → buffer font）：单行走面板 UI 字体（文件树
            // 行内改名与行标签同族同号，Zed project_panel 对编辑器零覆盖的同款
            // 效果），多行走内置 mono（MCP 粘贴框等 buffer 轨）。字号 ui() 双轨
            //（§5.5 ui(12)，compact 档随宿主行 ui(11)），销债表 #2 的裸 px。
            .font_family(if self.multiline || self.form {
                SharedString::from(crate::editor::markdown::MONO_FAMILY)
            } else {
                crate::appearance::panel_font().family.clone().into()
            })
            .text_size(if self.form {
                gpui::px(15.)
            } else {
                crate::appearance::ui_size(if self.compact { 11. } else { 12. })
            })
            // form（040-2）：内件行高**取整行高**（默认 1.5em = 22.5 → 22.5px
            // 非"整"，44 槽居中后文字偏上 5px——用户实测「placeholder 没有垂直
            // 居中」）。指定 20px 行高后文字在 44px 槽里几何居中。
            .when(self.form, |d| d.line_height(gpui::px(20.)))
            // escape bubbles up from the inner widget (it only unmarks IME
            // composition and propagates) — turn it into the app callback
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                if ev.keystroke.key == "escape" {
                    if let Some(cb) = this.on_escape.take() {
                        cb(cx);
                        this.on_escape = Some(cb);
                        // 只在输入框自己消费 ESC（如收起弹层）时拦截；
                        // 否则放行给外层（弹窗 ESC 关闭）
                        cx.stop_propagation();
                    }
                }
            }))
            .child(input)
    }
}
