//! 031-1 计划模式：Chat 侧动作（工具菜单 toggle / 弹窗两动作）。
//!
//! 状态纪律（§2 裁决 1）：扩展是唯一事实源，本文件的乐观翻转只是**菜单
//! 响应速度**；真值永远以 setStatus("pf-plan") 广播为准（防哑弹见
//! runtime/response.rs 的 plan_cmd_inflight 分支；草稿意图见
//! plan_sync_pending 的 spawn 同步）。

use crate::*;
use gpui::Context;
use pi_link::protocol::Command;

impl Chat {
    /// 工具菜单「计划模式」行（P2）：乐观翻转 + `/plan` 发送（流式中也
    /// 立即执行，pi 原子判定）。在途回执记入 plan_cmd_inflight 供防哑弹；
    /// 无进程（新会话草稿/断线）记为意图态，spawn 后由 spawn_attached
    /// 补发 `/plan on` 同步扩展（先于用户首条消息，闸门首轮生效）。
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
        let rt = self.rt();
        match id {
            Some(id) => rt.update(cx, |r, _| {
                r.plan_enabled = !prev;
                r.plan_cmd_inflight = Some((id, prev));
            }),
            None => rt.update(cx, |r, _| {
                // 031-1 草稿意图窗口（2026-11-08 bug 修复）：勾选被记住，
                // spawn 后同步；同窗口期广播里相反的值被忽略
                r.plan_enabled = !prev;
                r.plan_sync_pending = true;
            }),
        }
        cx.notify();
    }

    /// 弹窗【批准】（裁决 5）：`/plan-approve` 放行挂起裁决——扩展关计划
    /// （工具集未动过，无需恢复），resolve present_plan，agent 同轮继续。
    pub(crate) fn mc_plan_approve(&mut self, cx: &mut Context<Self>) {
        self.rt().read(cx).rpc(&Command::Prompt {
            message: "/plan-approve".into(),
            images: Vec::new(),
            streaming_behavior: None,
        });
        self.dlg.plan.approval = None;
        cx.notify();
    }

    /// 弹窗【否决/取消 + Esc + 标题 ×】（2026-11-08 二次修正）：关弹窗 +
    /// `/plan-reject` **优雅 resolve**——present_plan 正常返回（本轮干净
    /// 收尾，模型不再续跑），计划模式保持。**不走 Abort**：abort 会把
    /// 被掐的 present_plan 标成 "This operation was aborted" 错误卡
    /// （真机实锤）；resolve 通道零噪声。用户的新想法直接在 composer
    /// 输入（无弹窗内输入框，2026-11-08 简化裁决）。
    pub(crate) fn mc_plan_reject(&mut self, cx: &mut Context<Self>) {
        self.rt().read(cx).rpc(&Command::Prompt {
            message: "/plan-reject".into(),
            images: Vec::new(),
            streaming_behavior: None,
        });
        self.dlg.plan.approval = None;
        cx.notify();
    }
}
