//! ComposerInput — the chat composer's multi-line text input.
//!
//! 与 ui/text_input.rs 同源：门面适配 **gpui-component 的 InputState**
//! （真选区/点击定位/方向键/IME/剪贴板/undo），区别只在多行模式：
//! AutoGrow(min,max) 行数自增长、超限出纵向滚动条、Enter 由事件层接管。
//!
//! 按键契约（对照 gpui-component 0.2.0 源码 input/state.rs）：
//! - Enter：多行模式组件先插入 "\n" 再发 `PressEnter { secondary:false }`
//!   ——门面在事件里把该 "\n"（cursor 前 1 字节）剥掉后回调 on_submit，
//!   上层发完清空即可，无需还原。
//! - Shift+Enter：组件无此绑定 → 按键事件冒泡到 wrapper 的 on_key_down
//!   （composer 里决定换行还是排队 follow-up）。
//! - IME 组合中：gpui Windows 平台在 marked text 期间不派发按键，回车
//!   不会误发送（此前手搓 InputHandler 的 marked range 实现坏了才漏）。
//! - ↑/↓/Tab：被 main.rs 注册的同上下文("Input")覆盖绑定截获（菜单导航/
//!   历史/补全），非空多行时由 composer 重新派发 MoveUp/MoveDown。

use std::rc::Rc;

use gpui::{App, Context, Entity, FocusHandle, ParentElement, Render, SharedString, Styled, Window, div, prelude::*};
use gpui_component::input::{InputEvent, InputState, TextInput as GpInput};

use crate::theme::theme as T;
use gpui::{px, rgb};

/// fires after every user value mutation (typing, paste, IME commit)
pub type Changed = Rc<dyn Fn(&str, &mut App)>;
/// plain Enter (IME 组合期不会到达)；参数为剥掉组件自插 "\n" 后的文本
pub type Submitted = Rc<dyn Fn(&str, &mut App)>;

pub struct ComposerInput {
    fallback_focus: FocusHandle,
    state: Option<Entity<InputState>>,
    placeholder: Option<SharedString>,
    min_rows: usize,
    max_rows: usize,
    // config buffered until the inner state exists (needs &mut Window)
    pending_value: Option<String>,
    pending_placeholder: Option<SharedString>,
    /// mirrored inner value so `value()` works without cx
    value: String,
    on_change: Option<Changed>,
    on_submit: Option<Submitted>,
}

impl ComposerInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            fallback_focus: cx.focus_handle(),
            state: None,
            placeholder: None,
            min_rows: 3,
            max_rows: 10,
            pending_value: None,
            pending_placeholder: None,
            value: String::new(),
            on_change: None,
            on_submit: None,
        }
    }

    pub fn set_on_change(&mut self, cb: Changed) {
        self.on_change = Some(cb);
    }

    pub fn set_on_submit(&mut self, cb: Submitted) {
        self.on_submit = Some(cb);
    }

    pub fn set_placeholder(&mut self, ph: Option<SharedString>) {
        self.placeholder = ph.clone();
        if self.state.is_some() {
            self.pending_placeholder = ph;
        }
    }

    /// 外部写入（草稿切换/历史/清空）。与镜像同值时跳过，避免回环。
    pub fn set_value(&mut self, v: String, cx: &mut Context<Self>) {
        if self.value == v {
            return;
        }
        self.value = v.clone();
        self.pending_value = Some(v);
        cx.notify();
    }

    /// The inner widget's focus handle (composer 边框焦点态/程序聚焦用)。
    pub fn focus_handle_in(&self, cx: &App) -> FocusHandle {
        use gpui::Focusable as _;
        self.state
            .as_ref()
            .map(|s| s.read(cx).focus_handle(cx))
            .unwrap_or_else(|| self.fallback_focus.clone())
    }

    /// Shift+Enter 换行（组件对 shift-enter 无绑定，由 composer wrapper 调）。
    pub fn insert_newline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(st) = &self.state {
            st.update(cx, |s, scx| s.insert("\n", window, scx));
        }
    }

    fn ensure_state(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(state) = &self.state {
            return state.clone();
        }
        let placeholder = self.placeholder.clone().unwrap_or_default();
        let (min, max) = (self.min_rows, self.max_rows);
        let state = cx.new(|scx| {
            InputState::new(window, scx)
                .auto_grow(min, max)
                .placeholder(placeholder)
        });
        cx.subscribe(&state, |this, entity, event: &InputEvent, cx| {
            this.on_inner_event(entity, event, cx);
        })
        .detach();
        if let Some(v) = self.pending_value.take() {
            self.value = v.clone();
            state.update(cx, |st, scx| st.set_value(v, window, scx));
        }
        self.state = Some(state.clone());
        state
    }

    fn on_inner_event(
        &mut self,
        entity: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let v = entity.read(cx).value().to_string();
                self.value = v.clone();
                if let Some(cb) = self.on_change.clone() {
                    cb(&v, cx);
                }
            }
            InputEvent::PressEnter { secondary: false } => {                // 多行模式组件已在光标处自插 "\n"（cursor 停在其后）——剥掉
                // 再交给上层发送；上层随后清空，无需写回组件
                let sent = {
                    let state = entity.read(cx);
                    let v = state.value().to_string();
                    let c = state.cursor();
                    if c >= 1 && v.as_bytes().get(c - 1) == Some(&b'\n') {
                        format!("{}{}", &v[..c - 1], &v[c..])
                    } else {
                        v
                    }
                };                // 事件派发期间本实体处于租用中——回调若同步再 composer
                // .update()（发送清空走 Chat::set_input）即双重租约 panic
                //（0xc0000409，已实测）。defer 到本租约结束后执行。
                if let Some(cb) = self.on_submit.clone() {
                    cx.defer(move |cx| cb(&sent, cx));
                }
            }
            // Ctrl+Enter（secondary）：composer 未使用
            InputEvent::PressEnter { secondary: true } => {}
            InputEvent::Focus | InputEvent::Blur => {}
        }
    }
}

impl Render for ComposerInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.ensure_state(window, cx);
        if let Some(v) = self.pending_value.take() {
            self.value = v.clone();
            state.update(cx, |st, scx| st.set_value(v, window, scx));
        }
        if let Some(ph) = self.pending_placeholder.take() {
            state.update(cx, |st, scx| st.set_placeholder(ph, window, scx));
        }

        let t = T();
        div()
            .id("composer-input")
            .w_full()
            // 字号/颜色从 wrapper 继承进组件的文本塑形
            .text_size(px(13.5))
            .text_color(rgb(t.text))
            .child(GpInput::new(&state).appearance(false).bordered(false))
    }
}
