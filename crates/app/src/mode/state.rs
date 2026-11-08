//! 031-1 计划模式：Chat 侧动作（工具菜单 toggle / 弹窗三动作）。
//!
//! 状态纪律（§2 裁决 1）：扩展是唯一事实源，本文件的乐观翻转只是**菜单
//! 响应速度**；真值永远以 setStatus("pf-plan") 广播为准（防哑弹见
//! runtime/response.rs 的 plan_cmd_inflight 分支）。

use crate::*;
use gpui::Context;
use pi_link::protocol::Command;

impl Chat {
    /// 工具菜单「计划模式」行（P2）：乐观翻转 + `/plan` 发送（流式中也
    /// 立即执行，pi 原子判定）。在途回执记入 plan_cmd_inflight 供防哑弹。
    pub(crate) fn mc_toggle_plan(&mut self, cx: &mut Context<Self>) {
        let (id, prev) = {
            let rt = self.rt();
            let r = rt.read(cx);
            let prev = r.plan_enabled;
            let id = r.rpc_id(&Command::Prompt {
                message: "/plan".into(),
                images: Vec::new(),
                streaming_behavior: None,
            });
            (id, prev)
        };
        let Some(id) = id else {
            // 无进程（新会话草稿/断线）：记住意图，spawn 后由 spawn_attached
            // 补发 `/plan on|off` 同步扩展（先于用户首条消息，闸门首轮生效）。
            // 同步窗口期广播里相反的值被忽略（plan_sync_pending，裁决 1）。
            let rt = self.rt();
            rt.update(cx, |r, _| {
                r.plan_enabled = !prev;
                r.plan_sync_pending = true;
            });
            cx.notify();
            return;
        };
        let rt = self.rt();
        rt.update(cx, |r, _| {
            r.plan_enabled = !prev;
            r.plan_cmd_inflight = Some((id, prev));
        });
        cx.notify();
    }

    /// 弹窗【批准】（裁决 5）：`/plan-approve` 放行挂起裁决——扩展关计划
    /// （工具集未动过，无需恢复）+ resolve present_plan，agent 同轮继续。
    pub(crate) fn mc_plan_approve(&mut self, cx: &mut Context<Self>) {
        self.rt().read(cx).rpc(&Command::Prompt {
            message: "/plan-approve".into(),
            images: Vec::new(),
            streaming_behavior: None,
        });
        self.plan_close_modal(cx);
        cx.notify();
    }

    /// 弹窗【否决/取消 + Esc】（裁决 5）：关弹窗 + Abort+ClearQueue 配对
    /// （铁律：abort 不清队列）；present_plan 随 abort 取消（signal race），
    /// 计划模式保持。
    pub(crate) fn mc_plan_reject_abort(&mut self, cx: &mut Context<Self>) {
        self.plan_close_modal(cx);
        self.rt().update(cx, |r, cx| r.abort_stream(cx));
        cx.notify();
    }

    /// 弹窗【输入其他想法 + 回车】（裁决 5）：先 `/plan-reject` 放行本轮
    /// （resolve 为「用户否决」，run 干净收尾），再发正文——pi 原子排队
    /// 接住，计划模式保持，agent 重新计划。乐观上屏 + ↑ 历史与 send_input
    /// 同口径。
    pub(crate) fn mc_plan_feedback(&mut self, text: String, cx: &mut Context<Self>) {
        self.rt().read(cx).rpc(&Command::Prompt {
            message: "/plan-reject".into(),
            images: Vec::new(),
            streaming_behavior: None,
        });
        self.plan_close_modal(cx);
        if text.is_empty() {
            cx.notify();
            return;
        }
        let rt = self.rt();
        rt.update(cx, |r, cx| {
            let cmd = Command::Prompt {
                message: text.clone(),
                images: Vec::new(),
                streaming_behavior: None,
            };
            match r.agent.session.as_ref().map(|s| s.send(&cmd)) {
                Some(Ok(_)) => {
                    if r.history.last().map(|h| h != &text).unwrap_or(true) {
                        r.remember_input(text.clone());
                    }
                    r.history_ix = None;
                    r.status = "running".into();
                    r.optimistic_send(text, true, cx);
                }
                Some(Err(e)) => r.status = e,
                None => {}
            }
        });
        cx.notify();
    }

    /// 关弹窗 + 清输入（三动作共用的收场）。
    fn plan_close_modal(&mut self, cx: &mut Context<Self>) {
        self.dlg.plan.approval = None;
        self.dlg.plan.input.update(cx, |ti, cx| {
            ti.set_value(String::new(), cx);
        });
    }
}
