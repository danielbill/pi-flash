//! 发送路径：composer 发送/引导/排队、`!` bash 三件套、远程发送、
//! 模型/思考/压缩/工具预设。

use gpui::Context;
use pi_link::protocol::{Block, Command};

use crate::i18n::{tf, tr};
use crate::session::messages::{BashInfo, Msg, Role};

use super::*;

/// 远程（微信）投递结果：排队 ≠ 失败——只有 Failed 才向微信发出失败回执。
pub(crate) enum RemoteSend {
    Sent,
    Queued,
    Failed,
}

impl SessionRuntime {
    /// 发送后进 ↑ 历史：**上限截断**。此前无上限——长会话里每次发送的全文
    /// （含粘贴的大段日志）都永久留底，内存随消息条数线性涨，且
    /// `with_active_editor` 每次收发还要整体 clone 两份（per-send O(历史)）。
    /// 100 条足够 ↑ 回溯；完整记录在会话文件里，这里只是输入历史。
    /// 去重/空文本判定由调用方负责（各发送路径口径不同）。
    pub(super) fn remember_input(&mut self, text: String) {
        const INPUT_HISTORY_CAP: usize = 100;
        self.history.push(text);
        if self.history.len() > INPUT_HISTORY_CAP {
            let excess = self.history.len() - INPUT_HISTORY_CAP;
            self.history.drain(..excess);
        }
    }

    /// RPC 门面：session/runtime/ 域外的文件一律经此发送，不直摸
    /// `agent.session`——进程在场与否、wire 细节都收在 runtime 内部。
    /// 返回 false = 无进程（草稿/已断），调用方按需降级。
    pub(crate) fn rpc(&self, cmd: &Command) -> bool {
        match &self.agent.session {
            Some(s) => s.send(cmd).is_ok(),
            None => false,
        }
    }

    /// steer（中断当前运行并立即注入）。090 起两个入口：设置「插话」模式下
    /// composer 的 Enter，与 queue 模式下卡片「立即」/Ctrl+Enter 的显式插话
    /// （pif-ui `session.steer` 自动化也走这里）。乐观上屏与翻页和 prompt/
    /// 排队发送一致——发送帧气泡钉视口顶，不等回显。
    /// 走 prompt + streamingBehavior:"steer"（pi-web onPromptWithStreamingBehavior
    /// parity）：排队与否由 pi 原子判定；裸 steer 落在空闲 agent 的 steering
    /// 队列里同样会滞留到下一轮中途才插话。
    /// 插话模式 Enter / queue 模式 Ctrl+Enter / pif-ui `session.steer`——
    /// **打断投递**（2026-10-20 用户定稿规则 3：立即 = 中止当前 agent 轮次、
    /// 重新下达指令）。运行中 abort 当前回复、本条立刻作为新一轮发出；空闲
    /// 直接开跑。机制见 queue.rs `interrupt_deliver`。
    pub(crate) fn steer_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        self.interrupt_from_composer(cx);
    }

    /// 乐观上屏（prompt/排队共用，pi-web optimistic user message parity）：
    /// 气泡立即显示，RPC 回显经 pending_echo 去重就地升级。`pin` = 是否发言
    /// 钉顶（page_turn）——**空闲普通发送**钉顶；打断投递传 false：不翻页，
    /// 消息跟在流后面、视图保持贴底跟随（2026-10-20 用户定稿「打断消息
    /// 不要刷屏，跟在流后面」）。`pin` 记进 pending_echo 随条目走：回显到
    /// 达（message_start user）按同一决定行事，不再无条件补一次 page_turn。
    pub(super) fn optimistic_send(&mut self, text: String, pin: bool, cx: &mut Context<Self>) {
        // 伪标签：还没绑定会话文件的 draft，乐观发送的同一帧就在侧栏挂标
        // 签（真标签要等 pi 落盘 + 磁盘扫描，进程冷启 1-3s）。prompt/排队/
        // 微信远程三个入口共用本漏斗；已绑定会话被 file 守卫挡掉。
        if self.file.is_none() {
            let preview = text
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or_default()
                .to_string();
            cx.emit(SessionEvent::PendingTab(Some(preview)));
        }
        self.pending_echoes.push(PendingEcho {
            text: text.clone(),
            pin,
        });
        self.messages.push(Msg {
            role: Role::User,
            bash: None,
            blocks: vec![Block::Text {
                content_index: 0,
                text,
            }],
            usage: None,
            entry_id: None,
            ts: Some(crate::services::format::now_ms()),
            end_ts: None,
            stop_reason: None,
            error_message: None,
            custom_type: None,
            custom_display: true,
            details: None,
            model: None,
        });
        self.phase_waiting = true;
        // 钉顶：新用户消息立即钉视口顶、历史滚出屏，不等回显不等 bounds
        // （page_turn 内部完成 sync 落账）。不钉顶（打断投递）：notify_list
        // 做结构同步即可，视口停在贴底跟随位，消息自然跟在流尾部
        if pin {
            let ix = self.messages.len() - 1;
            let msgs = self.messages.len();
            let phase = self.phase_row_visible();
            self.pager.page_turn(ix, msgs, phase);
        } else {
            self.notify_list(cx);
        }
        cx.notify();
    }

    /// 排队消息（**路收窄**：仅 pif-ui 自动化；composer 走 send_input）。命令
    /// 与 send_input 同一付：prompt + streamingBehavior:"followUp"——排队与否
    /// 由 pi 原子判定（见 protocol.rs Prompt 注释的滞留队列竞态）。
    ///
    /// 三条硬要求（上一版踩过的坑）：
    /// 1. **先上屏再发**——pi 的 `followUp` 只入队 + 发 queue_update，等到
    ///    下一轮 turn 边界才 emit message_start(user)；不上屏就是“打字被
    ///    吞了”的观感；
    /// 2. **发成功才清输入**——上一版 `let _ = session.send(..)` 丢弃返回 +
    ///    无条件 `input.clear()`，pi 侧拒绝（`/ext-cmd` 扩展命令不允许排队、
    ///    输入被扩展 handler 消费、进程不在）时内容静默蒸发；
    /// 3. **失败保留草稿**并写 status（同 send_input 的 Err 口径）。
    pub(crate) fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return;
        };
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image", "data": img.data_b64, "mimeType": img.mime
                })
            })
            .collect();
        match session.send(&Command::Prompt {
            message: text.clone(),
            images,
            streaming_behavior: Some("followUp".into()),
        }) {
            Ok(_) => {
                if !text.is_empty() && self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.remember_input(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
                self.pending_images.clear();
                self.status = tr("status.queued").into();
                // 技能命令不做乐观回显（pi-web parity）：RPC 回显的是展开信封
                // 文本，乐观插入裸 "/skill:xxx" 会多出一条重复气泡
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, true, cx);
                }
            }
            Err(e) => {
                // 排队被拒：草稿与附件原样留着，用户改完能重发
                self.status = e;
            }
        }
        cx.notify();
    }

    /// 停止（rpc abort + clear_queue 配对）。
    pub(crate) fn abort_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Abort);
            // **abort 必须配对 clear_queue**：pi 的 abort 只掐当前 run，不清
            // followUp/steering 队列——已排队未投递的消息留在队列里，下一轮
            // 跑完时被 would-stop drain 冲出来当新轮执行（「停止后消息又自己
            // 执行了一遍」，2026-10-19 会话 01a1564b 实证）。clear_queue 把
            // 它们撤下，response 回传原文（{steering,followUp}），由
            // clear_queue response 臂删乐观气泡 + 回填输入框。
            let _ = session.send(&Command::ClearQueue);
        }
        // 090 定批：**本地队列全部退回编辑器**（文本不丢，改完可再发）——
        // 与 pi 侧 clear_queue 撤回同一「回填编辑器」口径。queue 模式下
        // composer 消息不再进 pi 队列，这里撤的是本地扣住的条目；两条撤回
        // 通路（clear_queue response 异步 / 本地同步）互不重叠。
        let (recalled, images) = self.restore_queue_to_editor();
        if !recalled.is_empty() || !images.is_empty() {
            cx.emit(SessionEvent::RecallInput {
                text: recalled,
                images,
            });
        }
        self.stream_started = None;
        // 与 interrupt_deliver 同款在途竞态：Esc 停止后旧流残包仍在管道里，
        // 紧接着的发送（乐观气泡）后面不得长出旧内容（stream_cut 字段注释）
        self.stream_cut = true;
        // 中止：等待行立即收起（不等 agent_settled）
        self.phase_waiting = false;
        // pending_echoes **不在此清**：撤回的条目由 clear_queue response 臂
        // 随气泡一起摘（response 异步回来）；已被投递的条目回显时正常消费。
        // 中止即「不在跑」：状态槽转圈 / composer 停止按钮都读 agent_running，
        // 若 pi 的终止事件迟到或压根不来（进程被掐），不就地复位就会永久显示
        // 运行中——同一条「立即收起」语义
        self.agent_running = false;
        if let Some(s) = self.state.as_mut() {
            s.is_streaming = false;
        }
        self.notify_list(cx);
        cx.notify();
    }

    /// 「回到最新」按钮（pi-web scrollToBottom）：钉顶期回锚点顶，跟尾期交还
    /// 贴底胶水（目标选取见 [`ChatList::jump_to_bottom`]，那里有跟尾期不能用
    /// 锚点顶的完整因果）。
    pub(crate) fn scroll_to_bottom(&mut self) {
        self.pager.jump_to_bottom();
    }

    /// 整表重读（`get_messages` / 会话文件重读）之后恢复锚点，**不要**无条件
    /// `release`：工具预设换绑就是一次 `GetMessages`，退役锚点会让「等待模型
    /// 响应」等短内容连同刚上翻的历史一起掉回屏底（用户实测）。锚点仍指着一条
    /// 用户消息就原样保留；否则按「最后一条用户消息」重锚；确实找不到才退役。
    pub(super) fn restore_anchor_after_reload(&mut self) {
        if !self.pager.anchor_active() {
            return;
        }
        let msgs = self.messages.len();
        let phase = self.phase_row_visible();
        let valid = self
            .pager
            .anchor_ix()
            .filter(|ix| self.messages.get(*ix).is_some_and(|m| m.role == Role::User));
        if let Some(ix) = valid {
            self.pager.reanchor(ix, msgs, phase);
        } else if let Some(ix) = self.messages.iter().rposition(|m| m.role == Role::User) {
            self.pager.reanchor(ix, msgs, phase);
        } else {
            self.pager.release();
        }
    }

    /// `!` / `!!` shell 命令发送（031 对齐 pi-web executeBash）：立即执行、
    /// 不进模型；乐观卡片流式回显输出，response 回填终态。发出后
    /// `bash_running` = true（调用方据此清空输入）。
    fn send_bash(&mut self, text: &str, cx: &mut Context<Self>) {
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

    /// 040/043：picker/MCP 运行中确认的清单，空闲发送（或 090 flush）开始前
    /// 生效——换包集必须重绑进程（RPC 无运行时切换），`--session` 复接不丢
    /// 消息。返回 false = 重绑 spawn 失败，调用方放弃本次发送。
    pub(super) fn rebind_pending(&mut self, cx: &mut Context<Self>) -> bool {
        if self.agent_running {
            return true; // 运行中不重绑（下一轮空闲时再生效）
        }
        let pending_ext = self.pending_ext_sources.take();
        if pending_ext.is_none() && !self.pending_mcp_reload {
            return true;
        }
        self.pending_mcp_reload = false;
        if let Some(list) = pending_ext {
            self.ext_sources = list;
            if let Some(p) = pi_link::session_ext::store_path() {
                let _ = pi_link::session_ext::write_for(&p, &self.key, &self.ext_sources);
            }
        }
        if !self.spawn_attached(cx) {
            return false;
        }
        if let Some(s) = &self.agent.session {
            let _ = s.send(&Command::GetMessages);
        }
        self.refresh_state();
        true
    }

    pub(crate) fn send_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        // 空文本+图片可发送（pi-web handleSend：!msg && !images 才拦）
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        // 040/043：空闲发送前生效的扩展/MCP 重绑（抽取件，flush 也走）
        if !self.rebind_pending(cx) {
            return;
        }
        if !self.ensure_spawned(cx) {
            return;
        }
        if self.agent.session.is_none() {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return;
        }
        // `!` / `!!` shell 命令（031）：不经 prompt/插话，走 rpc bash。
        // 带图片时 ! 只当普通文本（pi-web bashMode 需无附件）。
        if self.pending_images.is_empty() && text.starts_with('!') {
            self.send_bash(&text, cx);
            if self.bash_running {
                // 与普通消息同口径：进 ↑ 历史、清输入
                if self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.remember_input(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
            }
            cx.notify();
            return;
        }
        // 规则 1+2（2026-10-20 终稿）：仅**运行中**才进缓冲（轮次结束自动
        // 发出；缓冲非空时 enqueue 内部拒绝）。空闲 = 直接发送，不经过缓冲
        // ——「前面没话」时缓冲毫无意义。
        if self.queue_mode() {
            if self.agent_running {
                self.enqueue_from_composer(text, cx);
                cx.notify();
                return;
            }
            let images: Vec<serde_json::Value> = self
                .pending_images
                .iter()
                .map(|img| {
                    serde_json::json!({
                        "type": "image",
                        "data": img.data_b64,
                        "mimeType": img.mime
                    })
                })
                .collect();
            let cmd = Command::Prompt {
                message: text.clone(),
                images,
                // pi 原子判定：恰有 run 溜进（AgentEnd..Settled 续跑窗）时
                // pi 侧 followUp 接住，与本任务同源
                streaming_behavior: Some("followUp".into()),
            };
            match self.agent.session.as_ref().map(|s| s.send(&cmd)) {
                Some(Ok(_)) => {
                    if !text.is_empty() && self.history.last().map(|h| h != &text).unwrap_or(true) {
                        self.remember_input(text.clone());
                    }
                    self.history_ix = None;
                    self.input.clear();
                    self.pending_images.clear();
                    self.status = "running".into();
                    // 技能命令不做乐观回显（pi-web parity）
                    if !text.is_empty() && !text.starts_with("/skill:") {
                        self.optimistic_send(text, true, cx);
                    }
                }
                Some(Err(e)) => self.status = e,
                None => {}
            }
            cx.notify();
            return;
        }
        // steer 模式（规则 3）：显式插入 = **打断**——中止当前轮次，草稿立刻
        // 作为新一轮指令发出（空闲同样直接开跑），不再走 pi steer 注入通道。
        self.interrupt_from_composer(cx);
        cx.notify();
    }

    /// 远程（微信）来的一条文本 —— 与 [`send_input`] **同口径**，但不碰
    /// `self.input` / `pending_images`（那两个是 composer 专属的）。
    ///
    /// 关键：草稿**还没有 pi 进程**时必须先 `spawn()` + `attach_pump`，
    /// 否则 `agent.session` 是 `None`、命令直接丢掉 —— 真机表现就是
    /// 「发消息没有任何应答，重启后才有人回」。
    ///
    /// 远程（微信）来的一条文本 —— 与 [`send_input`] **同口径**，但不碰
    /// `self.input` / `pending_images`（那两个是 composer 专属的）。
    ///
    /// 关键：草稿**还没有 pi 进程**时必须先 `spawn()` + `attach_pump`，
    /// 否则 `agent.session` 是 `None`、命令直接丢掉 —— 真机表现就是
    /// 「发消息没有任何应答，重启后才有人回」。
    ///
    /// 返回三态：微信回执要区分「排队中」（正常，等当前轮结束）与
    /// 「失败」（必须出声），不能都按 bool 的 false 静默吞掉。
    pub(crate) fn send_remote_text(
        &mut self,
        text: String,
        cx: &mut Context<Self>,
    ) -> RemoteSend {
        if self.compacting {
            return RemoteSend::Failed;
        }
        if !self.ensure_spawned(cx) {
            return RemoteSend::Failed;
        }
        if self.agent.session.is_none() {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return RemoteSend::Failed;
        }
        // 规则 1+2：仅**运行中**才入缓冲（空闲直接发送，不经过缓冲）。
        if self.queue_mode() && self.agent_running {
            self.enqueue_remote(text.clone(), cx);
            cx.notify();
            return RemoteSend::Queued;
        }
        // steer 模式运行中（规则 3）：打断投递——中止当前轮，本条作为新一轮。
        if !self.queue_mode() && self.agent_running {
            if self.history.last().map(|h| h != &text).unwrap_or(true) {
                self.remember_input(text.clone());
            }
            self.history_ix = None;
            self.interrupt_deliver(
                QueuedInput {
                    text,
                    images: Vec::new(),
                    remote: true,
                },
                cx,
            );
            return RemoteSend::Sent;
        }
        // 空闲（两模式同条）：直接发送，pi 原子判定。
        let cmd = Command::Prompt {
            message: text.clone(),
            images: Vec::new(),
            streaming_behavior: Some("followUp".into()),
        };
        let sent = self.rpc(&cmd);
        if sent {
            if self.history.last().map(|h| h != &text).unwrap_or(true) {
                self.remember_input(text.clone());
            }
            self.history_ix = None;
            self.status = "running".into();
            // 让桌面端也看到这条消息（微信发的同样属于这轮对话）
            if !text.starts_with("/skill:") {
                self.optimistic_send(text, true, cx);
            }
        } else {
            self.status = tr("status.send_failed").into();
        }
        cx.notify();
        if sent {
            RemoteSend::Sent
        } else {
            RemoteSend::Failed
        }
    }

    /// 远程来的非文本命令（`/停止`、ExtUi 回填）：**不 spawn** ——
    /// 没有进程就明确返回失败，由调用方出声。
    pub(crate) fn send_remote_command(&mut self, cmd: &Command, cx: &mut Context<Self>) -> bool {
        if self.compacting {
            return false;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("status.disconnected").into();
            cx.notify();
            return false;
        };
        let ok = session.send(cmd).is_ok();
        if !ok {
            self.status = tr("status.send_failed").into();
        }
        cx.notify();
        ok
    }

    /// 思考档换挡（胶囊菜单 / 微信菜单同源）。
    ///
    /// `level` 是 pi 的档位键。pi 会按模型能力**截断**（deepseek-flash 只有
    /// off/low/high/max），真值随 `thinking_level_changed` 事件回来，本地这
    /// 里先乐观回填状态槽（不然胶囊要等下一次 `get_state` 才动，看起来像
    /// 「没换过去」）。
    pub(crate) fn set_thinking_level(&mut self, level: &str, cx: &mut Context<Self>) {
        self.thinking_override = Some(level.to_string());
        self.push_thinking_level(level);
        cx.notify();
    }

    /// 「使用 pi 默认设置」（菜单 auto）：override 清空（打勾留在 auto），但
    /// pi 端**没有** auto 档——只清本地标志等于点了没反应（真机：换到 low
    /// 再选 auto，pi 仍停在 low、胶囊也不动）。所以由调用方按 pi 设置解出
    /// 默认档（[`pi_link::config::PI_DEFAULT_THINKING_LEVEL`] 兜底）显式下发。
    pub(crate) fn set_thinking_auto(&mut self, default: &str, cx: &mut Context<Self>) {
        self.thinking_override = None;
        self.push_thinking_level(default);
        cx.notify();
    }

    /// 乐观回填状态槽 + 下发 rpc。没有进程（空闲回收/草稿）时就只落本地：
    /// 下次 spawn 用 `--thinking <override>` 带上。
    fn push_thinking_level(&mut self, level: &str) {
        if let Some(st) = self.state.as_mut() {
            st.thinking_level = Some(level.to_string());
        }
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetThinkingLevel {
                level: level.to_string(),
            });
        }
    }

    pub(crate) fn select_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetModel {
                provider,
                model: id,
            });
        } else {
            // draft with no process: remember the pick, spawn carries it as
            // --model (pi-web newSessionModel parity)
            self.pending_model = Some((provider, id));
        }
        // no immediate refresh_state here: pi 1.0 completes the swap
        // asynchronously, so an eager get_state reports the PREVIOUS model —
        // the label updates from the set_model response instead (see the
        // set_model response arm)
        cx.notify();
    }

    /// 压缩按钮可用性：agent 跑动中不可（compact 会中止当前 turn）、
    /// 压缩中防连点、未连接不可。除此之外不设门槛——用不用由用户决定。
    pub(crate) fn can_compact(&self) -> bool {
        !self.compacting && !self.agent_running && self.agent.session.is_some()
    }

    /// 圆环弹窗底部手动压缩：rpc compact。`compacting` 期间输入面板锁死
    /// （placeholder 提示 + 变更丢弃）；完成后 pi 回 compact response，
    /// 由其分支清标志并刷新 stats/messages。
    pub(crate) fn compact(&mut self, cx: &mut Context<Self>) {
        if !self.can_compact() {
            return;
        }
        match self
            .agent
            .session
            .as_ref()
            .map(|s| s.send(&Command::Compact))
        {
            Some(Ok(_)) => {
                self.compacting = true;
                self.status = tr("status.compacting").into();
            }
            Some(Err(e)) => self.status = e,
            None => self.status = tr("status.disconnected").into(),
        }
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

    pub(crate) fn tool_preset_key(&self) -> &str {
        &self.tools_preset
    }

    pub(crate) fn tool_preset_label(&self) -> String {
        let key = self.tool_preset_key();
        if key.is_empty() {
            return "configured".into();
        }
        if key == "custom" {
            // 档名 full+；扩展数量由右边的「扩展(n)」按钮承载，不重复
            return crate::i18n::tr("extensions.full").to_string();
        }
        key.to_string()
    }
}
