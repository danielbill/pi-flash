//! 问卷卡交互逻辑（impl Chat，与 ext_ui.rs 同风格；自 ask_card.rs 外移）：
//! 阻塞请求路由 / Enter·数字键 / 选项点选 / 取消·确认。

use super::parse::{parse_from_transcript, title_matches};
use crate::*;
use pi_link::protocol::{ExtUiMethod, ExtensionUiRequest};

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
                // #问题卡不弹：transcript 里找不到可解析的 ask_user_question
                // toolCall（未送达 / args_partial / parse_args 拒收）→ 落 legacy
                pi_link::client::ext_trace("ask: no parseable tool_call in transcript → legacy");
                return false;
            };
            let idx = card.next_request;
            if idx >= card.questions.len() || !title_matches(&req, &card.questions[idx]) {
                pi_link::client::ext_trace(&format!("ask: title mismatch (idx={idx}) → legacy"));
                return false;
            }
            card.pending = Some(req);
            card.open = true;
            self.aux.ask = Some(card);
            // flat 输入框常驻自定义行：placeholder 由输入框自身显示，布防时
            // 现取 tr()（语言切换后下一张卡即刷新）；上一张卡残留文本一并
            // 清掉（新卷 = 空输入）
            let ph = tr("ask.other_placeholder").to_string();
            let weak = cx.entity().downgrade();
            self.aux.ask_input.update(cx, |ti, cx| {
                ti.set_placeholder(Some(ph.into()));
                ti.set_value(String::new(), cx);
                // 打字实时镜像进当前题 other_text：作答态判定 / 确认 [n/N]
                // 门控 / Enter 全答完判定读的都是这份镜像。不挂 on_change 时
                // 自定义文本只在 tab 切换才落账——单题卡没有「切走」这一步，
                // 确认恒 [0/1] 灰死（2026-11-09 用户报告）。回调在 TextInput
                // 自身 update 里发出：只许写 Chat 态，不许回头 read 输入框
                // 实体（重复租 = panic，gpui.md 双租节；值由参数传入）。
                ti.set_on_change(Box::new(move |v, cx| {
                    let v = v.to_string();
                    let _ = weak.update(cx, |c, cx| c.ask_input_mirror(&v, cx));
                }));
            });
            pi_link::client::ext_trace("ask: armed");
            cx.notify();
            return true;
        }
        // 已布防：请求必须对应下一题；未确认 → 挂起，已确认 → 静默自动应答
        let submitted = match self.aux.ask.as_ref() {
            Some(card) => {
                let idx = card.next_request;
                if idx >= card.questions.len() || !title_matches(&req, &card.questions[idx]) {
                    pi_link::client::ext_trace(&format!(
                        "ask: stale-card mismatch (idx={idx}) → legacy"
                    ));
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

    /// 「自定义答案」行体点选（输入区/行内任意处）：确保选中（幂等，再点
    /// 不取消——编辑意图），单选清选项；聚焦输入框
    pub(crate) fn ask_other_select(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_mut() else {
            return;
        };
        let q = &mut card.questions[card.active];
        if !q.other_sel {
            q.other_sel = true;
            if !q.multi {
                for s in q.sel.iter_mut() {
                    *s = false;
                }
            }
        }
        let fh = self.aux.ask_input.read(cx).focus_handle_in(cx);
        window.focus(&fh);
        cx.notify();
    }

    /// 「自定义答案」圆标点选：切换选中态（选中后再点 = 取消选中，用户定稿）；
    /// 单选在选中瞬间清选项。取消后文本保留（重选即恢复，作答口径只认
    /// other_sel && 非空文本，残留文本不误提交）
    pub(crate) fn ask_other_toggle(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_mut() else {
            return;
        };
        let q = &mut card.questions[card.active];
        q.other_sel = !q.other_sel;
        if q.other_sel && !q.multi {
            for s in q.sel.iter_mut() {
                *s = false;
            }
        }
        if q.other_sel {
            let fh = self.aux.ask_input.read(cx).focus_handle_in(cx);
            window.focus(&fh);
        }
        cx.notify();
    }

    /// 输入框现值镜像进当前题 other_text（布防时挂的 on_change 逐键驱动）：
    /// 作答口径仍由 answered() 把关（单选要 other_sel，文本非空才作数）。
    pub(crate) fn ask_input_mirror(&mut self, v: &str, cx: &mut Context<Self>) {
        let Some(card) = self.aux.ask.as_mut() else {
            return;
        };
        card.questions[card.active].other_text = v.to_string();
        cx.notify();
    }

    /// Enter：未答完跳到下一未答题；全部答完 = 确认提交（AskCard 同款）
    pub(crate) fn ask_enter(&mut self, cx: &mut Context<Self>) {
        // 活题自定义文本落账（on_change 已逐键同步，此处兜底 IME 迟发等
        // 场景；on_submit 经 defer 到此，输入框租约已释放，read 安全）
        if let Some(card) = self.aux.ask.as_mut() {
            let val = self.aux.ask_input.read(cx).value().to_string();
            card.questions[card.active].other_text = val;
        }
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
            let val = self.aux.ask_input.read(cx).value().to_string();
            {
                let q = &mut card.questions[card.active];
                q.other_text = val;
                if q.other_sel && q.other_text.trim().is_empty() {
                    q.other_sel = false;
                }
            }
            if !card.questions.iter().all(|q| q.answered()) {
                return;
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
