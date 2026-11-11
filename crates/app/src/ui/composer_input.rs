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

use gpui::{
    App, Context, Entity, FocusHandle, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::*,
};
use gpui_component::input::{InputEvent, InputState, TextInput as GpInput};

use crate::appearance::theme::theme as T;
use gpui::{px, rgb};

/// chip 的两种形态（由 value 前缀派生，互斥）：命令/技能 `/名字 ` 与
/// 会话引用 `<全路径> `。编辑器只装前缀之后的参数部分，Change/Submit
/// 再把前缀拼回去——发送出去的仍是原始文本（会话 = 全路径，模型可读；
/// chip 只是视觉替身，ZCode parity：图标+会话名隐藏原始路径）。
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Command { name: String, skill: bool },
    Session { path: String, title: String },
}

impl Token {
    /// chip 前缀（含终止空格）——完整输入 = 前缀 + 编辑器参数部分
    fn prefix(&self) -> String {
        match self {
            Self::Command { name, .. } => format!("/{} ", name),
            Self::Session { path, .. } => format!("{} ", path),
        }
    }
}

/// 从完整输入中拆出命令/技能 chip：`/名字 ` 或 `/名字 参数...`（命令词
/// 已终止——尾随空格存在才成 chip，打字途中 "/llam" 仍是普通文本走菜单）。
fn split_command_token(value: &str, commands: &[String]) -> Option<Token> {
    let after = value.strip_prefix('/')?;
    let ix = after.find(' ')?;
    let word = &after[..ix];
    let name = commands.iter().find(|c| c.as_str() == word)?;
    Some(Token::Command {
        skill: name.starts_with("skill:"),
        name: name.clone(),
    })
}

/// 会话 chip 派生：value 以注册的会话全路径开头（恰好等于，或后随空格）
/// 即成 chip——@插入写入的就是 `path ` 形态，裸路径（用户删掉尾空格）
/// 也认，退格可整颗删除。token 由 Chat 每帧按 psp 清单派生下发。
fn split_session_token(value: &str, token: Option<&(String, String)>) -> Option<Token> {
    let (path, title) = token?;
    let hit = value.len() >= path.len()
        && value.starts_with(path.as_str())
        && (value.len() == path.len() || value.as_bytes()[path.len()] == b' ');
    hit.then(|| Token::Session {
        path: path.clone(),
        title: title.clone(),
    })
}

/// 向下取整到 UTF-8 字符边界（move_to 接受任意字节但语义要求边界）
fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// placeholder 是覆盖层（面板字号，与输入框正文的会话字号可以不同），
/// 但它必须落在组件**第一行的行框**里，才能和光标对齐。行框由三个量决定，
/// 三者都要与 gpui-component 内部一致（Size::Medium / text_input.rs）：
///
/// - 左：组件 padding.left = `input_px(Medium)` = 12
/// - 上：组件 padding.top  = `input_py(Medium)` = 5
/// - 行高：编辑器写死的 `LINE_HEIGHT = Rems(1.25)` = 20px（rem 16）
///
/// 行高最容易被漏掉：不写它，覆盖层会继承 gpui 默认行高 φ(1.618)，15px
/// 字号 → 24.3px，比组件的 20px 高一截；行内的字形按行框居中，于是整行
/// 下沉 (24.3−20)/2 ≈ 2.1px。此前又手调了一个 +2px「视觉补偿」，两者叠加
/// 就成了「placeholder 比光标低半个行距」的错位。
const PLACEHOLDER_LEFT: f32 = 12.;
const PLACEHOLDER_TOP: f32 = 5.;
/// 必须与 vendor/gpui-component/src/input/text_input.rs 的 LINE_HEIGHT 同值，
/// 字号（面板 vs 会话）不同时基线才不会跑。
const COMPOSER_LINE_HEIGHT_REMS: f32 = 1.25;

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
    /// 菜单确认后的光标落点（字节偏移；渲染帧 inner 值就位后应用——
    /// set_value 会把光标甩到末尾，@ 补全要求光标停在插入 token 之后）
    pending_cursor: Option<usize>,
    pending_commands: Option<Vec<String>>,
    /// mirrored inner value so `value()` works without cx
    value: String,
    /// 命令名镜像（渲染期判断 token 态用）
    commands: Vec<String>,
    /// 当前激活的命令/技能 chip（由 value 派生；编辑器只装参数部分）
    token: Option<Token>,
    /// 会话引用 token（path, title）——Chat 每帧按输入前缀从 psp 清单
    /// 派生下发（set_session_token 同值跳过）；split_session_token 拿它
    /// 与 value 对拼决定会话 chip
    session_token: Option<(String, String)>,
    on_chip_backspace: Option<std::rc::Rc<dyn Fn(&mut gpui::App)>>,
    on_change: Option<Changed>,
    on_submit: Option<Submitted>,
    /// Ctrl+Enter（secondary-enter 绑定）：090 反向快捷键——queue 模式下
    /// 「本条立即 steer 插话」，steer 模式下「显式入队」。参数与 on_submit
    /// 同口径（已剥组件自插的换行）。未设置时保持旧行为（只插换行）。
    on_submit_secondary: Option<Submitted>,
    /// 锁态（如上下文压缩中）：编辑器组件**不挂载**——无焦点、无 IME、
    /// 无粘贴、无点击定位，整块输入区替换为 placeholder 样式提示。
    /// 草稿仍留在 self.value / chat.composer.input，解锁即恢复。
    read_only: bool,
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
            pending_cursor: None,
            pending_commands: None,
            value: String::new(),
            commands: Vec::new(),
            token: None,
            session_token: None,
            on_chip_backspace: None,
            on_change: None,
            on_submit: None,
            on_submit_secondary: None,
            read_only: false,
        }
    }

    pub fn set_on_change(&mut self, cb: Changed) {
        self.on_change = Some(cb);
    }

    pub fn set_on_submit(&mut self, cb: Submitted) {
        self.on_submit = Some(cb);
    }

    /// Ctrl+Enter（secondary）提交回调（090 反向快捷键，见字段注释）。
    pub fn set_on_submit_secondary(&mut self, cb: Submitted) {
        self.on_submit_secondary = Some(cb);
    }

    /// 占位文案仅门面自绘（text_faint，设计规范 placeholder 专属色）——
    /// 组件内置占位用 muted_foreground(=text_dim) 过深，与正文难区分。
    pub fn set_placeholder(&mut self, ph: Option<SharedString>) {
        self.placeholder = ph;
    }

    /// 外部写入（草稿切换/历史/清空/菜单接受）。与镜像同值时跳过，避免回环。
    /// 收 &str：调用方（input_area 每帧镜像 chat.composer.input）不必先克隆全文，
    /// 同值跳过路径零分配。
    ///
    /// 短路只看 value：`token` 是 value × commands 的纯派生量，render 每帧
    /// 重算（见 Render 里 split_token），不必在此再算一遍——那曾是**每帧一次
    /// 全串扫描 + find(' ')**，哪怕什么都没变。
    pub fn set_value(&mut self, v: &str, cx: &mut Context<Self>) {
        if self.value == v {
            return;
        }
        let s = v.to_string();
        // pending_value 只在组件未创建时被 ensure_state 消费；组件已建后
        // 是死存储，不再每次覆写
        if self.state.is_none() {
            self.pending_value = Some(s.clone());
        }
        self.value = s;
        cx.notify();
    }

    /// 外部写入 + 光标落点（031 @ 补全：确认后光标停在插入 token 之后，
    /// 而不是 set_value 用的「文本末尾」）。cursor_byte 按新值字节偏移。
    pub fn set_value_with_cursor(&mut self, v: String, cursor_byte: usize, cx: &mut Context<Self>) {
        let clamped = floor_char_boundary(&v, cursor_byte.min(v.len()));
        self.set_value(&v, cx);
        // set_value 同值短路也要落光标（菜单接受常发生在同值场景外，但
        // 防御性覆盖）：无论哪条路，渲染帧统一应用
        self.pending_cursor = Some(clamped);
        cx.notify();
    }

    /// 当前光标字节偏移（inner 未建时 = 值末尾）。@ token 检测用。
    pub fn cursor(&self, cx: &App) -> usize {
        self.state
            .as_ref()
            .map(|s| s.read(cx).cursor())
            .unwrap_or(self.value.len())
    }

    /// The inner widget's focus handle (composer 边框焦点态/程序聚焦用)。
    pub fn focus_handle_in(&self, cx: &App) -> FocusHandle {
        use gpui::Focusable as _;
        self.state
            .as_ref()
            .map(|s| s.read(cx).focus_handle(cx))
            .unwrap_or_else(|| self.fallback_focus.clone())
    }

    /// 锁态开关（input_area 每帧按 runtime.compacting 同步）。
    pub fn set_read_only(&mut self, v: bool, cx: &mut Context<Self>) {
        if self.read_only == v {
            return;
        }
        self.read_only = v;
        cx.notify();
    }

    /// 注册命令名（不含 "/" 前缀；供 token 高亮/整体退格/类别图标）。
    /// input_area 每帧调用，仅在真变化时 notify——否则 Chat 每帧渲染都把
    /// ComposerInput 标脏，下一帧又连坐回 Chat，永不停歇。
    pub fn set_command_names(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        if self.commands == names {
            return;
        }
        self.commands = names;
        cx.notify();
    }

    /// 注册会话引用 token（Chat 每帧派生下发；同值跳过不 notify，否则
    /// Chat 每帧渲染连坐标脏，同 set_command_names 的节流约定）。
    pub fn set_session_token(&mut self, token: Option<(String, String)>, cx: &mut Context<Self>) {
        if self.session_token == token {
            return;
        }
        self.session_token = token;
        cx.notify();
    }

    /// 编辑器为空时退格 = 删除整个 chip（转发给宿主清空输入）。
    pub fn set_on_chip_backspace(&mut self, cb: std::rc::Rc<dyn Fn(&mut gpui::App)>) {
        self.on_chip_backspace = Some(cb);
    }

    /// Shift+Enter 换行（组件对 shift-enter 无绑定，由 composer wrapper 调）。
    pub fn insert_newline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(st) = &self.state {
            st.update(cx, |s, scx| s.insert("\n", window, scx));
        }
    }

    fn ensure_state(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(state) = &self.state {
            return state.clone();
        }
        let (min, max) = (self.min_rows, self.max_rows);
        let state = cx.new(|scx| InputState::new(window, scx).auto_grow(min, max));
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
                // 编辑器只装参数部分；拼回 chip 前缀才是完整输入（chip 化
                // 首帧编辑器被置空，此处若不带前缀会把输入清成 ""）
                let rest = entity.read(cx).value().to_string();
                let v = match &self.token {
                    Some(tok) => format!("{}{}", tok.prefix(), rest),
                    None => rest,
                };
                self.value = v.clone();
                if let Some(cb) = self.on_change.clone() {
                    cb(&v, cx);
                }
            }
            InputEvent::PressEnter { secondary: false } => {
                // 多行模式组件已在光标处自插换行（cursor 停在其后）——剥掉、
                // 拼回 chip 前缀，再交给上层发送
                let stripped = {
                    let state = entity.read(cx);
                    let v = state.value().to_string();
                    let c = state.cursor();
                    if c >= 1 && v.as_bytes().get(c - 1) == Some(&b'\n') {
                        format!("{}{}", &v[..c - 1], &v[c..])
                    } else {
                        v
                    }
                };
                let sent = match &self.token {
                    Some(tok) if stripped.is_empty() => tok.prefix().trim_end().to_string(),
                    Some(tok) => format!("{}{}", tok.prefix(), stripped),
                    None => stripped,
                };
                // 事件派发期间本实体处于租用中——回调若同步再 composer
                // .update()（发送清空走 Chat::set_input）即双重租约 panic
                //（0xc0000409，已实测）。defer 到本租约结束后执行。
                if let Some(cb) = self.on_submit.clone() {
                    cx.defer(move |cx| cb(&sent, cx));
                }
            }
            // Ctrl+Enter（secondary，090 反向快捷键）：与 primary 同款剥换行
            // （多行模式组件对 secondary-enter 也先自插 "\n"），交上层按
            // 「反向发送方式」处理
            InputEvent::PressEnter { secondary: true } => {
                let stripped = {
                    let state = entity.read(cx);
                    let v = state.value().to_string();
                    let c = state.cursor();
                    if c >= 1 && v.as_bytes().get(c - 1) == Some(&b'\n') {
                        format!("{}{}", &v[..c - 1], &v[c..])
                    } else {
                        v
                    }
                };
                let sent = match &self.token {
                    Some(tok) if stripped.is_empty() => tok.prefix().trim_end().to_string(),
                    Some(tok) => format!("{}{}", tok.prefix(), stripped),
                    None => stripped,
                };
                if let Some(cb) = self.on_submit_secondary.clone() {
                    // defer 同 on_submit：派发期实体租用中，回调内同步
                    // composer.update() 会双重租约（0xc0000409，已实测）
                    cx.defer(move |cx| cb(&sent, cx));
                }
            }
            InputEvent::Focus | InputEvent::Blur => {}
        }
    }
}

impl Render for ComposerInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.ensure_state(window, cx);
        if let Some(names) = self.pending_commands.take() {
            self.commands = names;
        }
        let t = T();
        // chip 派生：命令 `/名字 参数...` 优先，会话引用 `<path> 参数...`
        // 其次（两者前缀互斥，不可能同帧双命中）
        self.token = split_command_token(&self.value, &self.commands)
            .or_else(|| split_session_token(&self.value, self.session_token.as_ref()));
        // 编辑器值取借用切片：每帧与 Rope 的 memcmp 零分配，只有真变了
        // 才物化 String（长草稿下每帧一次全文拷贝就是打字延迟）。
        // 会话裸路径形态（value == path，无尾空格）编辑器为空
        let editor_value: &str = match &self.token {
            Some(tok) => {
                let pfx = tok.prefix();
                if self.value.len() >= pfx.len() {
                    &self.value[pfx.len()..]
                } else {
                    ""
                }
            }
            None => &self.value,
        };
        // Rope 直接与 &str 比对（ropey PartialEq，零分配 memcmp）
        if *state.read(cx).text() != editor_value {
            let ev = editor_value.to_string();
            state.update(cx, |st, scx| st.set_value(ev, window, scx));
        }
        // 菜单确认后的光标落点（inner 值就位后应用；set_cursor_offset 不需要 window）
        if let Some(cb) = self.pending_cursor.take() {
            state.update(cx, |st, scx| st.set_cursor_offset(cb, scx));
        }
        {
            let chip = self.token.is_some();
            let cb = self.on_chip_backspace.clone();
            state.update(cx, |st, _| {
                st.chip_active = chip;
                st.on_chip_backspace = cb;
            });
        }

        if self.read_only {
            // 锁态先把焦点赶走：编辑器马上不再挂载，留着焦点会让按键
            // 事件打进一个没有元素的 handle
            use gpui::Focusable as _;
            let handle = state.read(cx).focus_handle(cx);
            if handle.is_focused(window) {
                window.blur();
            }
        }

        let ph = self.placeholder.clone().unwrap_or_default();
        let empty = self.value.is_empty();
        let ph_lh = gpui::rems(COMPOSER_LINE_HEIGHT_REMS);
        // token chip（ZCode 原子节点 parity）：图标+裸名胶囊，顶格插在编辑器前。
        // 会话引用 = message-square-text + 会话名（原始全路径隐藏，超宽省略号）
        let chip_el = self.token.as_ref().map(|tok| {
            let (icon_name, label): (&'static str, String) = match tok {
                Token::Command { name, skill } => (
                    if *skill {
                        "wand"
                    } else {
                        "icon-terminal-solid"
                    },
                    name.strip_prefix("skill:").unwrap_or(name).to_string(),
                ),
                Token::Session { title, .. } => ("message-square-text", title.clone()),
            };
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .py(px(3.))
                .mt(px(1.))
                .rounded(px(6.))
                .border_1()
                .border_color(gpui::rgba(((t.accent as u64) << 8) as u32 | 0x4d))
                .bg(rgb(t.bg_selected))
                .child(crate::ui::icon(icon_name, 13., t.accent))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(14.))
                        .text_color(rgb(t.accent))
                        // 会话名可长（preview 兑底）：超宽省略号截断，不挤压编辑器
                        .max_w(px(280.))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(SharedString::from(label)),
                )
        });

        div()
            .id("composer-input")
            .w_full()
            .relative()
            // 输入文字跟随「会话字体」设置（发送后与气泡正文一致，所见即
            // 所得）；placeholder 单独按面板设置值（字体大小设置.md §1）
            .text_size(crate::appearance::sess_size(0.))
            .text_color(rgb(t.text))
            .flex()
            .items_start()
            .gap(px(6.))
            .children(if self.read_only { None } else { chip_el })
            .child(if self.read_only {
                // 锁态：整块换成 placeholder 样式的提示（草稿不丢，
                // 只是暂时不显示，解锁后原样回来）；提示前置转圈
                // spinner，表明系统在动（复用 ui::spinner，同侧栏会话行）
                div()
                    .flex_1()
                    .min_w_0()
                    // 与解锁态的同一条首行行框对齐（否则锁开/关提示会跳一行）
                    .pt(px(PLACEHOLDER_TOP))
                    // 同理补左对齐：解锁态 placeholder 以 PLACEHOLDER_LEFT
                    // 缩进对齐编辑器 input_px(Medium)=12，锁态编辑器不挂载、
                    // 提示直接落在容器左缘，少了这 12px 会比其他 placeholder
                    // 靠左一截（锁开/关切换时文案水平跳动）
                    .pl(px(PLACEHOLDER_LEFT))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(crate::ui::spark_spinner(12., t.text_faint))
                    .child(
                        div()
                            .line_height(ph_lh)
                            .text_size(crate::appearance::ui_size(12.))
                            .text_color(rgb(t.text_faint))
                            .child(ph),
                    )
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .when(empty && self.token.is_none() && !ph.is_empty(), |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(PLACEHOLDER_TOP))
                                .left(px(PLACEHOLDER_LEFT))
                                .line_height(ph_lh)
                                .text_size(crate::appearance::ui_size(12.))
                                .text_color(rgb(t.text_faint))
                                .child(ph),
                        )
                    })
                    .child(GpInput::new(&state).appearance(false).bordered(false))
            })
    }
}
