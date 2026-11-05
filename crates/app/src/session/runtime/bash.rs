//! `!` / `!!` shell 命令三件套（031 对齐 pi-web executeBash）：发送 +
//! 流式增量消费（events.rs BashExecutionUpdate 臂）+ 终态回填 + Esc 取消。

use gpui::Context;
use pi_link::protocol::Command;

use crate::i18n::{tf, tr};
use crate::session::messages::{BashInfo, Msg, Role};

use super::*;

impl SessionRuntime {
    /// `!` / `!!` shell 命令发送（031 对齐 pi-web executeBash）：立即执行、
    /// 不进模型；乐观卡片流式回显输出，response 回填终态。发出后
    /// `bash_running` = true（调用方据此清空输入）。
    pub(super) fn send_bash(&mut self, text: &str, cx: &mut Context<Self>) {
        let trimmed = text.trim_start();
        let excluded = trimmed.starts_with("!!");
        let command = trimmed[if excluded { 2 } else { 1 }..].trim();
        if command.is_empty() {
            self.status = tr("composer.shell_empty").to_string();
            cx.notify();
            return;
        }
        // pi 侧拒绝条件（rpc-mode bash：busy 即 error）——提前拦，输入保留
        if self.bash_running || self.agent_running || self.compacting {
            self.status = tr("composer.shell_busy").to_string();
            cx.notify();
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return;
        };
        let cmd = Command::Bash {
            command: command.to_string(),
            exclude_from_context: excluded,
        };
        match session.send(&cmd) {
            Ok(_) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .ok();
                self.pending_bash = Some(self.messages.len());
                self.messages.push(Msg {
                    role: Role::Bash,
                    bash: Some(BashInfo {
                        command: command.to_string(),
                        excluded,
                        running: true,
                        ..Default::default()
                    }),
                    blocks: Vec::new(),
                    usage: None,
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                });
                self.bash_running = true;
                self.status = if excluded {
                    tr("composer.shell_local_only").into()
                } else {
                    tr("composer.shell_to_model").into()
                };
                self.notify_list(cx);
            }
            Err(e) => self.status = e,
        }
    }

    /// bash response 回填终态（pi BashResult）。乐观卡片按 pending_bash 定位，
    /// 索引失效（快照重建/换档重读）时补一张独立完成卡。
    pub(super) fn finalize_bash(
        &mut self,
        result: pi_link::protocol::BashResult,
        cx: &mut Context<Self>,
    ) {
        self.bash_running = false;
        let end_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .ok();
        let applied = self
            .pending_bash
            .take()
            .and_then(|ix| self.messages.get_mut(ix))
            .filter(|m| m.role == Role::Bash)
            .map(|m| {
                if let Some(b) = m.bash.as_mut() {
                    b.output = result.output.clone();
                    b.exit_code = result.exit_code;
                    b.cancelled = result.cancelled;
                    b.truncated = result.truncated;
                    b.full_output_path = result.full_output_path.clone();
                    b.running = false;
                }
                m.end_ts = end_ts;
            })
            .is_some();
        if !applied {
            // 卡片丢了：补一张完整卡（命令名无从找回时留空——正常链路
            // pending_bash 不会失效）
            self.messages.push(Msg {
                role: Role::Bash,
                bash: Some(BashInfo {
                    output: result.output,
                    exit_code: result.exit_code,
                    cancelled: result.cancelled,
                    truncated: result.truncated,
                    full_output_path: result.full_output_path,
                    ..Default::default()
                }),
                blocks: Vec::new(),
                usage: None,
                entry_id: None,
                ts: None,
                end_ts,
                stop_reason: None,
                error_message: None,
                custom_type: None,
                custom_display: true,
                details: None,
                model: None,
            });
        }
        self.status = if result.cancelled {
            tr("send.shell_aborted").into()
        } else {
            match result.exit_code {
                Some(0) => tr("send.shell_done").into(),
                Some(c) => tf("send.shell_exit", &[("code", c.to_string())]),
                None => tr("send.shell_done").into(),
            }
        };
        self.notify_list(cx);
    }

    /// Esc / 停止按钮的 bash 分支（rpc abort_bash；pi 会把已启动的命令
    /// 取消掉，response 带 cancelled=true 收尾卡片）
    pub(crate) fn abort_bash(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::AbortBash);
        }
        cx.notify();
    }
}
