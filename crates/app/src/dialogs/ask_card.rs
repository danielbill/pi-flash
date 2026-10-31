//! ask_user_question 整卷问卷卡（v2 重设计）。
//!
//! 链路：transcript 流里的 `Block::ToolCall{args}`（name = ask_user_question）
//! 携带完整 questions[]，先于包逐题发来的 `extension_ui_request` 可见 ——
//! 在此整卷解析成 tab 卡先行渲染；包的第 N 个阻塞请求到达时按序映射到
//! tab N，用户点「确认」后串行应答（或 submitted 态下静默自动应答）。
//! 回传契约与 @juicesharp/rpiv-ask-user-question 的 rpc-fallback 严格对齐：
//! 单选回 options 原行 / 自定义回纯文本；多选回 "N[,N…][; 文本]"，空 = 全不选。
//!
//! 交互结构移植自 my-mastra-bot AskCard.tsx：tab 自由切题、每题独立作答态、
//! tab ●→✓ 答完标记、确认 [n/N] 门控、Tab/Enter/数字键键盘模型。
//! 视觉 = tmp/ask-user-redesign.html 评审稿（MIST 主题逐值对译）。

use gpui::{
    FontWeight, MouseButton, SharedString, div, hsla, prelude::FluentBuilder as _, px, rgb,
};

use crate::*;
use pi_link::protocol::{Block, ExtUiMethod, ExtensionUiRequest};

// ---------------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct AskCard {
    pub questions: Vec<AskQ>,
    pub active: usize,
    /// 已映射的阻塞请求数（= 下一个请求对应的题目下标）
    pub next_request: usize,
    /// 确认后进入自动应答态：后续请求到达即按已答序列化回填
    pub submitted: bool,
    /// 卡片可见（首个请求到达时打开；确认后收起，等剩余请求流过）
    pub open: bool,
    /// 当前挂起的阻塞请求（包严格串行，至多一个）
    pub pending: Option<ExtensionUiRequest>,
}

#[derive(Debug, Clone)]
pub(crate) struct AskQ {
    pub header: String,
    pub question: String,
    pub multi: bool,
    pub options: Vec<AskOpt>,
    /// 与 options 等长的选中标记
    pub sel: Vec<bool>,
    /// 「自定义答案」行选中态（单选与选项互斥；多选并存）
    pub other_sel: bool,
    pub other_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AskOpt {
    pub label: String,
    pub desc: String,
    pub preview: Option<String>,
}

impl AskQ {
    fn answered(&self) -> bool {
        if self.sel.iter().any(|s| *s) {
            return true;
        }
        self.other_sel && !self.other_text.trim().is_empty()
    }

    /// 按请求类型序列化本题答案（与包 rpc-fallback 解析器对齐）：
    /// 单选回 options 原行 / 自定义回纯文本；多选回 "N[,N…][; 文本]"
    fn serialize_for(&self, req: &ExtensionUiRequest) -> String {
        if self.multi {
            return self.pick_text();
        }
        if self.other_sel {
            return self.other_text.trim().to_string();
        }
        if let Some(i) = self.sel.iter().position(|s| *s) {
            if let ExtUiMethod::Select { options, .. } = &req.method {
                if let Some(line) = options.get(i) {
                    return line.clone();
                }
            }
            return self.options[i].label.clone();
        }
        String::new()
    }

    /// 提交序列化预览（多选摘要条 / 调试用）；与 serialize() 同构
    fn pick_text(&self) -> String {
        if !self.multi {
            return self
                .sel
                .iter()
                .position(|s| *s)
                .map(|i| self.options[i].label.clone())
                .or_else(|| {
                    self.other_sel
                        .then(|| self.other_text.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or_default();
        }
        let nums: Vec<String> = self
            .sel
            .iter()
            .enumerate()
            .filter(|(_, s)| **s)
            .map(|(i, _)| (i + 1).to_string())
            .collect();
        let text = self.other_text.trim();
        match (!nums.is_empty(), !text.is_empty()) {
            (true, true) => format!("{}; {}", nums.join(","), text),
            (true, false) => nums.join(","),
            (false, true) => text.to_string(),
            _ => String::new(),
        }
    }

    fn has_preview(&self) -> bool {
        self.options.iter().any(|o| o.preview.is_some())
    }
}

// ---------------------------------------------------------------------------
// 解析：tool_call args → AskCard
// ---------------------------------------------------------------------------

/// 从 transcript 末尾向前找最近的 ask_user_question tool_call（args 已收全）。
pub(crate) fn parse_from_transcript(rt: &session::runtime::SessionRuntime) -> Option<AskCard> {
    for msg in rt.messages.iter().rev() {
        if !matches!(msg.role, session::messages::Role::Assistant) {
            continue;
        }
        for b in msg.blocks.iter().rev() {
            let Block::ToolCall {
                name,
                args,
                args_partial,
                ..
            } = b
            else {
                continue;
            };
            if name != "ask_user_question" || *args_partial {
                continue;
            }
            return parse_args(args);
        }
    }
    None
}

/// 手解 args JSON（app 不直接依赖 serde derive，pi-link 协议层用 Value 同款）
fn parse_args(args: &str) -> Option<AskCard> {
    let v: serde_json::Value = serde_json::from_str(args).ok()?;
    let qs = v.get("questions")?.as_array()?;
    if qs.is_empty() || qs.len() > 10 {
        return None;
    }
    let mut questions = Vec::with_capacity(qs.len());
    for q in qs {
        let question = str_field(q, "question");
        if question.trim().is_empty() {
            return None;
        }
        let header = str_field(q, "header");
        let multi = q
            .get("multiSelect")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let opts_v = q.get("options")?.as_array()?;
        if opts_v.is_empty() || opts_v.len() > 12 {
            return None;
        }
        let mut options = Vec::with_capacity(opts_v.len());
        for o in opts_v {
            let label = str_field(o, "label");
            if label.trim().is_empty() {
                return None;
            }
            let desc = str_field(o, "description");
            let preview = o
                .get("preview")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty());
            options.push(AskOpt {
                label,
                desc,
                preview,
            });
        }
        let sel = vec![false; options.len()];
        questions.push(AskQ {
            header,
            question,
            multi,
            options,
            sel,
            other_sel: false,
            other_text: String::new(),
        });
    }
    Some(AskCard {
        questions,
        active: 0,
        next_request: 0,
        submitted: false,
        open: false,
        pending: None,
    })
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// 请求 title 是否对应当前题（title = `[header] question` + 折叠 preview 块）
fn title_matches(req: &ExtensionUiRequest, q: &AskQ) -> bool {
    let title = match &req.method {
        ExtUiMethod::Select { title, .. } | ExtUiMethod::Input { title, .. } => title,
        _ => return false,
    };
    !q.question.is_empty() && title.contains(&q.question)
}

// ---------------------------------------------------------------------------
// 交互逻辑（impl Chat，与 ext_ui.rs 同风格）
// ---------------------------------------------------------------------------

impl Chat {
    /// 阻塞请求路由：命中问卷流 → true（卡内处理）；否则 false 走 legacy 弹窗。
    pub(crate) fn ask_route_request(
        &mut self,
        req: ExtensionUiRequest,
        cx: &mut Context<Self>,
    ) -> bool {
        let blocking = matches!(
            req.method,
            ExtUiMethod::Select { .. } | ExtUiMethod::Input { .. }
        );
        if !blocking {
            return false;
        }
        // 未布防：从 transcript 解析整卷，题目命中才接管
        if self.aux.ask.is_none() {
            let Some(mut card) = parse_from_transcript(&self.rt().read(cx)) else {
                return false;
            };
            let idx = card.next_request;
            if idx >= card.questions.len() || !title_matches(&req, &card.questions[idx]) {
                return false;
            }
            card.pending = Some(req);
            card.open = true;
            self.aux.ask = Some(card);
            cx.notify();
            return true;
        }
        // 已布防：请求必须对应下一题；未确认 → 挂起，已确认 → 静默自动应答
        let submitted = match self.aux.ask.as_ref() {
            Some(card) => {
                let idx = card.next_request;
                if idx >= card.questions.len() || !title_matches(&req, &card.questions[idx]) {
                    return false;
                }
                card.submitted
            }
            None => return false,
        };
        if !submitted {
            if let Some(card) = self.aux.ask.as_mut() {
                card.pending = Some(req);
                card.open = true;
            }
            cx.notify();
            return true;
        }
        // 已确认：静默自动应答，流完即清
        let value = match self.aux.ask.as_ref() {
            Some(card) => card.questions[card.next_request].serialize_for(&req),
            None => return false,
        };
        let finished = match self.aux.ask.as_mut() {
            Some(card) => {
                card.next_request += 1;
                let done = card.next_request >= card.questions.len();
                if done {
                    self.aux.ask = None;
                }
                done
            }
            None => true,
        };
        let _ = finished;
        self.ext_send(&req, Some(value), None, false, cx);
        true
    }

    /// tab 切换：把输入框现值存回旧题，再载入新题的自定义文本
    pub(crate) fn ask_switch_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_mut() else {
            return;
        };
        if i >= card.questions.len() || i == card.active {
            return;
        }
        let cur = card.active;
        let val = self.aux.ask_input.read(cx).value().to_string();
        card.questions[cur].other_text = val;
        let load = card.questions[i].other_text.clone();
        card.active = i;
        self.aux
            .ask_input
            .update(cx, |ti, cx| ti.set_value(load, cx));
        cx.notify();
    }

    /// 选项点选：单选互斥（可反选），多选切换；清空本题自定义态（AskCard 语义）
    pub(crate) fn ask_toggle_option(&mut self, o: usize, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_mut() else {
            return;
        };
        let q = &mut card.questions[card.active];
        if o >= q.options.len() {
            return;
        }
        if q.multi {
            q.sel[o] = !q.sel[o];
        } else {
            let was = q.sel[o];
            for s in q.sel.iter_mut() {
                *s = false;
            }
            q.sel[o] = !was;
            if q.sel[o] {
                q.other_sel = false;
                q.other_text.clear();
                self.aux
                    .ask_input
                    .update(cx, |ti, cx| ti.set_value(String::new(), cx));
            }
        }
        cx.notify();
    }

    /// 「自定义答案」行点选：单选清选项；聚焦输入框
    pub(crate) fn ask_other_click(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_mut() else {
            return;
        };
        let q = &mut card.questions[card.active];
        q.other_sel = true;
        if !q.multi {
            for s in q.sel.iter_mut() {
                *s = false;
            }
        }
        let fh = self.aux.ask_input.read(cx).focus_handle_in(cx);
        window.focus(&fh);
        cx.notify();
    }

    /// Tab 键：切下一题
    pub(crate) fn ask_tab_key(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_ref() else {
            return;
        };
        if card.active + 1 < card.questions.len() {
            self.ask_switch_tab(card.active + 1, cx);
        }
    }

    /// Enter：未答完跳到下一未答题；全部答完 = 确认提交（AskCard 同款）
    pub(crate) fn ask_enter(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_ref() else {
            return;
        };
        if card.questions.iter().all(|q| q.answered()) {
            self.ask_confirm(cx);
            return;
        }
        let n = card.questions.len();
        for off in 1..n {
            let i = (card.active + off) % n;
            if !card.questions[i].answered() {
                self.ask_switch_tab(i, cx);
                return;
            }
        }
    }

    /// 数字键 1-4 直选当前题选项
    pub(crate) fn ask_digit(&mut self, d: usize, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_ref() else {
            return;
        };
        let q = &card.questions[card.active];
        if d < q.options.len() {
            self.ask_toggle_option(d, cx);
        }
    }

    /// 确认：全答完才生效；应答挂起请求 → 收起卡 → 后续请求静默流过
    pub(crate) fn ask_confirm(&mut self, cx: &mut Context<Self>) {
        // ① 卡内状态收尾（不触碰 ext_send）
        let (resp, finished) = {
            let Some(card) = self.aux.ask.as_mut() else {
                return;
            };
            if !card.questions.iter().all(|q| q.answered()) {
                return;
            }
            let val = self.aux.ask_input.read(cx).value().to_string();
            let q = &mut card.questions[card.active];
            q.other_text = val;
            if q.other_sel && q.other_text.trim().is_empty() {
                q.other_sel = false;
            }
            let resp = card.pending.take().map(|req| {
                let value = card.questions[card.next_request].serialize_for(&req);
                (req, value)
            });
            if resp.is_some() {
                card.next_request += 1;
            }
            card.submitted = true;
            card.open = false;
            let finished = card.next_request >= card.questions.len();
            (resp, finished)
        };
        // ② 卡外：应答 + 清态
        if let Some((req, value)) = &resp {
            self.ext_send(req, Some(value.clone()), None, false, cx);
        }
        if finished {
            self.aux.ask = None;
        }
        cx.notify();
    }

    /// 取消整卷（Esc / 取消钮 / 点外）：挂起请求回 cancelled，包侧整卷 DECLINE
    pub(crate) fn ask_cancel(&mut self, cx: &mut Context<Self>) {
        let Some(mut card) = self.aux.ask.take() else {
            return;
        };
        if let Some(req) = card.pending.take() {
            self.ext_send(&req, None, None, true, cx);
        }
        cx.notify();
    }
}
// 渲染（评审稿 ask-user-redesign.html 的 gpui 直译）
// ---------------------------------------------------------------------------

/// MIST accent #1e6559 的 HSL（167°, 54%, 26%）派生透明色
const ACCENT_HUE: f32 = 167.0 / 360.0;
const ACCENT_SAT: f32 = 0.54;
const ACCENT_LIT: f32 = 0.26;

fn accent_tint(a: f32) -> gpui::Hsla {
    hsla(ACCENT_HUE, ACCENT_SAT, ACCENT_LIT, a)
}

pub(crate) fn render_ask_card(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let Some(card) = chat.aux.ask.clone() else {
        return div().into_any_element();
    };
    let t = crate::appearance::theme::theme();
    let n = card.questions.len();
    let done = card.questions.iter().filter(|q| q.answered()).count();
    let all_answered = done == n;
    let q = &card.questions[card.active];

    // ---- dismiss（点外 / Esc = 取消整卷） ----
    let weak_cancel = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_cancel.update(cx, |c, cx| c.ask_cancel(cx));
    };

    // ---- 键盘：Tab 切题 / Enter 跳题·提交 / 数字直选 ----
    let weak_keys = weak.clone();
    let keys = move |ev: &gpui::KeyDownEvent, _w: &mut gpui::Window, cx: &mut gpui::App| {
        let key = ev.keystroke.key.as_str();
        match key {
            "tab" => {
                let _ = weak_keys.update(cx, |c, cx| c.ask_tab_key(cx));
            }
            "enter" => {
                let _ = weak_keys.update(cx, |c, cx| c.ask_enter(cx));
            }
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => {
                let d = (k.as_bytes()[0] - b'0') as usize;
                if d >= 1 {
                    let _ = weak_keys.update(cx, |c, cx| c.ask_digit(d - 1, cx));
                }
            }
            _ => {}
        }
    };

    // ---- tab 栏（含右上角 ×；装配见 ask_tab_bar） ----
    let tabs = ask_tab_bar(&card, weak, t);

    // ---- 题面（qcol + 可选预览栏） ----
    let with_pv = q.has_preview();
    let mut qcol = div()
        .flex()
        .flex_col()
        .gap(px(11.))
        .map(|d| if with_pv { d.w(px(400.)) } else { d.w_full() });
    qcol = qcol.child(
        div()
            .text_size(crate::appearance::ui_size(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(t.text))
            .child(SharedString::from(q.question.clone())),
    );

    qcol = qcol.child(ask_option_rows(q, weak, t));

    // 自定义答案行（AskCard 常驻模式）：active 题内嵌输入框，其余处只读回显
    qcol = qcol.child(ask_other_row(chat, q, weak, t));

    let mut pane = div().flex().gap(px(0.)).child(qcol);

    // 预览侧栏（当前题任一选项带 preview 时出现；装配见 ask_preview_pane）
    if with_pv {
        pane = pane.child(ask_preview_pane(q, t));
    }

    // ---- 卡脚：提示 + 取消 + 确认 [n/N]（装配见 ask_footer） ----
    let foot = ask_footer(weak, t, done, n, all_answered);

    // ---- 装配（浮层由 session/mod.rs main_column 挂进 chat-wrap 中栏；
    //      窄窗口下卡片随中栏收缩，两侧留 24px 呼吸边距） ----
    let card_el = div()
        // 问卷卡与内容列同宽（设置-界面「会话内容宽度」），窄窗由 max_w 收
        .w(px(crate::services::workspace::chat_content_width()))
        .max_w(gpui::relative(1.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(12.))
        .p(px(18.))
        .flex()
        .flex_col()
        .gap(px(11.))
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(tabs)
        .child(pane)
        .child(foot);

    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss)
        .on_key_down(keys)
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .px(px(24.))
                .child(card_el),
        )
        .into_any_element()
}

/// tab 栏：题签（ answered 实心点 / active 高亮 / 挂起题呼吸边）+ 右上角 ×
/// 关闭钮。内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
fn ask_tab_bar(
    card: &AskCard,
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
) -> gpui::Div {
    let mut tabs = div()
        .flex()
        .gap(px(5.))
        .items_center()
        .pb(px(10.))
        .border_b_1()
        .border_color(rgb(t.bg_selected));
    for (i, tq) in card.questions.iter().enumerate() {
        let answered = tq.answered();
        let active = i == card.active;
        let label = if tq.header.trim().is_empty() {
            format!("{} {}", tr("ask.question"), i + 1)
        } else {
            tq.header.trim().to_string()
        };
        let weak_tab = weak.clone();
        let mut tab = div()
            .id(SharedString::from(format!("ask-tab-{i}")))
            .flex()
            .items_center()
            .gap(px(7.))
            .px(px(11.))
            .py(px(5.))
            .rounded(px(8.))
            .text_size(crate::appearance::ui_size(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .font_family(crate::editor::markdown::MONO_FAMILY)
            .cursor_pointer()
            .child(div().size(px(6.)).rounded_full().map(|d| {
                if answered {
                    if active {
                        d.bg(rgb(t.accent_contrast))
                    } else {
                        d.bg(rgb(t.accent))
                    }
                } else if active {
                    d.border_1().border_color(rgb(t.accent_contrast))
                } else {
                    d.border_1().border_color(rgb(t.text_faint))
                }
            }))
            .child(SharedString::from(label))
            .map(|d| {
                if active {
                    d.bg(rgb(t.accent))
                        .text_color(rgb(t.accent_contrast))
                        .shadow_sm()
                } else if answered {
                    d.bg(accent_tint(0.08))
                        .text_color(rgb(t.accent_hover))
                        .hover(|s| s.bg(accent_tint(0.14)))
                } else {
                    d.text_color(rgb(t.text_muted))
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_tab.update(cx, |c, cx| c.ask_switch_tab(i, cx));
            });
        // 挂起中的请求对应题给一个呼吸点提示（提交前正在等这题）
        if i == card.next_request && card.pending.is_some() && !card.submitted {
            tab = tab.border_1().border_color(accent_tint(0.35));
        }
        tabs = tabs.child(tab);
    }
    // 右上角 X 关闭钮（点 × = 取消整卷；快捷键说明全部沉底）
    let weak_close = weak.clone();
    tabs.child(
        div().flex_1().flex().justify_end().child(
            div()
                .id("ask-close")
                .size(px(34.))
                .mt(px(-2.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .text_size(crate::appearance::ui_size(20.))
                .text_color(rgb(t.text_faint))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .child(tr("action.close"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_close.update(cx, |c, cx| c.ask_cancel(cx));
                }),
        ),
    )
}

/// 选项行列表：序号徽标 + 标签/描述 + 选中标记，选中行 tint。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
fn ask_option_rows(
    q: &AskQ,
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
) -> gpui::Div {
    let mut opts = div().flex().flex_col().gap(px(6.));
    for (j, opt) in q.options.iter().enumerate() {
        let sel = q.sel[j];
        let weak_opt = weak.clone();
        let mark = option_mark(q.multi, sel, t);
        let mut row = div()
            .id(SharedString::from(format!("ask-opt-{j}")))
            .flex()
            .items_start()
            .gap(px(10.))
            .px(px(11.))
            .py(px(9.))
            .rounded(px(9.))
            .border_1()
            .cursor_pointer()
            .child(
                div()
                    .flex_none()
                    .min_w(px(18.))
                    .h(px(18.))
                    .mt(px(1.))
                    .rounded(px(5.))
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(SharedString::from(format!("{}", j + 1)))
                    .map(|d| {
                        if sel {
                            d.bg(rgb(t.accent)).text_color(rgb(t.accent_contrast))
                        } else {
                            d.border_1()
                                .border_color(rgb(t.bg_selected))
                                .bg(rgb(t.bg))
                                .text_color(rgb(t.text_faint))
                        }
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(SharedString::from(opt.label.clone())),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(12.))
                            .text_color(rgb(t.text_muted))
                            .mt(px(2.))
                            .child(SharedString::from(opt.desc.clone())),
                    ),
            )
            .child(mark)
            .map(|d| {
                if sel {
                    d.bg(accent_tint(0.08)).border_color(accent_tint(0.35))
                } else {
                    d.border_color(gpui::hsla(0., 0., 0., 0.))
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_opt.update(cx, |c, cx| c.ask_toggle_option(j, cx));
            });
        if !q.multi && q.other_sel {
            // 单选自定义态下的选项行降透明度提示互斥
            row = row.opacity(0.72);
        }
        opts = opts.child(row);
    }
    opts
}

/// 自定义答案行（AskCard 常驻模式）：active 题内嵌输入框，未选时只显示占位。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
fn ask_other_row(
    chat: &Chat,
    q: &AskQ,
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
) -> gpui::Stateful<gpui::Div> {
    let weak_other = weak.clone();
    let mut row = div()
        .id("ask-other")
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(11.))
        .py(px(7.))
        .rounded(px(9.))
        .border_1()
        .cursor_pointer()
        .child(option_mark(q.multi, q.other_sel, t))
        .map(|d| {
            if q.other_sel {
                d.bg(accent_tint(0.08)).border_color(accent_tint(0.35))
            } else {
                d.border_color(rgb(t.bg_selected))
                    .bg(rgb(t.bg))
                    .hover(|s| s.bg(rgb(t.bg_hover)))
            }
        })
        .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            let _ = ev;
            let _ = weak_other.update(cx, |c, cx| c.ask_other_click(window, cx));
        });
    if q.other_sel || !q.other_text.is_empty() {
        row = row.child(chat.aux.ask_input.clone().into_any_element());
    } else {
        row = row.child(
            div()
                .text_size(crate::appearance::ui_size(12.5))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(tr("ask.other_placeholder").to_string())),
        );
    }
    row
}

/// 预览侧栏：当前选中（或首个带 preview 的）选项的 PREVIEW 头 + mono 正文框。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
fn ask_preview_pane(q: &AskQ, t: &crate::appearance::theme::Theme) -> gpui::Div {
    let pv_idx = q
        .sel
        .iter()
        .position(|s| *s)
        .or_else(|| q.options.iter().position(|o| o.preview.is_some()))
        .unwrap_or(0);
    let pv_text = q.options[pv_idx]
        .preview
        .clone()
        .unwrap_or_else(|| tr("ask.no_preview").to_string());
    let pv_name = q.options[pv_idx].label.clone();
    div()
        .flex_1()
        .min_w_0()
        .pl(px(16.))
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .pb(px(8.))
                .border_b_1()
                .border_color(rgb(t.bg_selected))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(t.text_soft))
                .child(div().size(px(6.)).rounded_full().bg(rgb(t.accent)))
                .child(SharedString::from(format!("PREVIEW · {pv_name}"))),
        )
        .child(
            div()
                .flex_1()
                .mt(px(10.))
                .max_h(px(240.))
                .overflow_hidden()
                .bg(rgb(t.tool_bg))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(9.))
                .px(px(14.))
                .py(px(13.))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(pv_text)),
        )
}

/// 卡脚：快捷键提示行 + 取消 + 确认 [n/N]（全答完才亮起可点）。
/// 内切自 render_ask_card（check_arch 300 行上限，纯结构搬运）。
fn ask_footer(
    weak: &gpui::WeakEntity<Chat>,
    t: &crate::appearance::theme::Theme,
    done: usize,
    n: usize,
    all_answered: bool,
) -> gpui::Div {
    let weak_cancel2 = weak.clone();
    let weak_ok = weak.clone();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(hint_row(
            &[
                ("tab", tr("ask.next")),
                ("enter", tr("ask.advance_submit")),
                ("1-4", tr("ask.direct_pick")),
                ("esc", tr("ask.cancel_all")),
            ],
            t,
        ))
        .child(div().flex_1())
        .child(
            div()
                .id("ask-cancel")
                .px(px(16.))
                .py(px(7.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
                .child(SharedString::from(tr("action.cancel").to_string()))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_cancel2.update(cx, |c, cx| c.ask_cancel(cx));
                }),
        )
        .child({
            let mut b = div()
                .id("ask-confirm")
                .px(px(16.))
                .py(px(7.))
                .rounded(px(8.))
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .child(SharedString::from(format!(
                    "{} [{}/{}]",
                    tr("ask.confirm"),
                    done,
                    n
                )));
            if all_answered {
                b = b
                    .bg(rgb(t.accent))
                    .text_color(rgb(t.accent_contrast))
                    .shadow_sm()
                    .hover(|s| s.bg(rgb(t.accent_hover)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = weak_ok.update(cx, |c, cx| c.ask_confirm(cx));
                    });
            } else {
                b = b.bg(rgb(t.bg_selected)).text_color(rgb(t.text_faint));
            }
            b
        })
}

/// 选中标记：单选 = 圆（选中实心+白点），多选 = 方（选中 accent+白勾）
fn option_mark(multi: bool, sel: bool, t: &crate::appearance::theme::Theme) -> gpui::AnyElement {
    let base = div()
        .flex_none()
        .size(px(16.))
        .mt(px(2.))
        .flex()
        .items_center()
        .justify_center()
        .map(|d| {
            if multi {
                d.rounded(px(5.))
            } else {
                d.rounded_full()
            }
        });
    if sel {
        base.bg(rgb(t.accent))
            .border_1()
            .border_color(rgb(t.accent))
            .map(|d| {
                if multi {
                    // 白勾（无 45° 旋转原语，用字形）
                    d.text_color(rgb(t.accent_contrast))
                        .text_size(crate::appearance::ui_size(9.5))
                        .font_weight(FontWeight::BOLD)
                        .child(tr("action.confirm_mark"))
                } else {
                    d.child(div().size(px(6.)).rounded_full().bg(rgb(t.accent_contrast)))
                }
            })
            .into_any_element()
    } else {
        base.border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg))
            .into_any_element()
    }
}

/// kbd 提示组：小键帽 + 灰说明
fn hint_row(pairs: &[(&str, &str)], t: &crate::appearance::theme::Theme) -> gpui::AnyElement {
    let mut row = div().flex().items_center().gap(px(5.));
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            row = row.child(div().mx(px(2.)));
        }
        row = row
            .child(
                div()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(9.5))
                    .text_color(rgb(t.text_muted))
                    .border_1()
                    .border_color(rgb(t.border))
                    .border_b_2()
                    .rounded(px(4.))
                    .px(px(5.))
                    .py(px(1.))
                    .bg(rgb(t.bg))
                    .child(SharedString::from(key.to_string())),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_size(11.5))
                    .text_color(rgb(t.text_faint))
                    .child(SharedString::from(label.to_string())),
            );
    }
    row.into_any_element()
}
