//! `on_event` 主分发器：pi wire 事件的全部会话级反应（轮次起止、压缩/
//! 重试/扩展臂、翻页与列表通知收尾）。拆出自 events.rs，逐字未动；
//! Response 回执臂 / 消息流三臂仍分别在 response.rs / stream.rs。

use gpui::Context;
use pi_link::protocol::Event;

use crate::i18n::tr;

use super::*;

/// 长错误文本截成单行状态条（按 chars 计，超长加省略号）
fn truncate_line(s: &str, cap: usize) -> String {
    let total = s.chars().count();
    let mut out: String = s.chars().take(cap).collect();
    if total > cap {
        out.push('…');
    }
    out
}

impl SessionRuntime {
    pub(in crate::session::runtime) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        // 用户消息到达携带发送帧的钉顶决定（None = 未到；Some(pin) = 到了，
        // 见函数尾 page_turn）
        let mut user_arrived: Option<bool> = None;
        match event {
            Event::Response {
                id,
                command,
                success,
                error,
                data,
                ..
            } => {
                if !self.on_response(id, &command, success, error, data, cx) {
                    return;
                }
            }
            Event::MessageStart {
                role,
                blocks,
                timestamp,
                is_error,
                tool_call_id,
                custom_type: _,
                custom_display: _,
                details,
                raw_system,
            } => {
                user_arrived = self.on_message_start(
                    role,
                    blocks,
                    timestamp,
                    is_error,
                    tool_call_id,
                    details,
                    raw_system,
                    cx,
                );
            }
            Event::MessageUpdate(assistant_event) => self.on_message_update(assistant_event),
            Event::MessageEnd {
                role,
                blocks,
                usage,
                timestamp,
                stop_reason,
                error_message,
                model,
                ..
            } => {
                if !self.on_message_end(
                    role,
                    blocks,
                    usage,
                    timestamp,
                    stop_reason,
                    error_message,
                    model,
                    cx,
                ) {
                    return;
                }
            }
            Event::AgentStart => {
                // 不动 turn_error/turn_interrupted：自动重试也会走这里，结局由
                // assistant 消息（MessageEnd）覆盖、由用户消息（MessageStart）
                // 归零（见那两处注释）。
                self.agent_running = true;
                // 真轮开跑：命令相位让位（扩展 handler 里 sendMessage 引出的
                // 轮次从这一刻起就是正经的模型等待语义）
                self.phase_command = false;
                self.status = "running".into();
                if self.stream_started.is_none() {
                    self.stream_started = Some(std::time::Instant::now());
                }
                // 跑动状态的订阅方（psp 状态槽的未读绿点）只挂在 Changed 上，
                // 而 Changed 只在进程退出 / 压缩 / 换工具预设时才发——轮次起止
                // 必须自己冒泡，否则后台会话跑起来也点不亮未读。
                cx.emit(SessionEvent::Changed);
            }
            Event::AgentSettled => {
                self.agent_running = false;
                self.phase_waiting = false;
                self.phase_command = false;
                self.streaming_content = false;
                // 轮次彻底结束：残包窗口（abort..settle）关闭的兜底清除——
                // 之后到达的流事件都属新轮，不得再并入旧 assistant
                self.stream_cut = false;
                // 一轮结束**不**退役锚点：pi-web 只是 promptAnchorActive=false
                // 让垫片收敛，容器 scrollTop 保持——短回复留在屏顶、长回复由
                // 胶水跟随尾部。锚点在此退役会把内容拽回屏底（消息从屏顶跳走，
                // 等于立刻撤销「发言钉顶」）。锚点留待用户滚轮 / 发送失败 /
                // 快照重建 / 会话切换退役。
                self.status = status_line(true, "idle");
                self.settle_turn();
                self.refresh_state();
                // 090：本轮**彻底**结束（settled 是唯一可靠信号——AgentEnd
                // 之后 pi 还可能 post-run continue/boundary 续跑），本地队列
                // 在这里发队首；有队列时状态行随即由 flush 拨回 running
                self.flush_local_queue(cx);
                // 轮末对账：文件落盘了就绑定换真身（首条消息处通常已绑，这里
                // 幂等）；确定没落盘（prompt 被扩展接管等，pi 没写过任何
                // user/assistant 消息）就撤伪标签——标签不能挂一个永远不会
                // 变真的会话。
                if !self.try_bind_reported_file(cx) {
                    cx.emit(SessionEvent::PendingTab(None));
                }
                // 轮次终点专用事件（turn_error 终值随行）：订阅方按「窗口
                // 没焦点」规则给点——走 Changed 的话 running 已复位，永远
                // 轮不到记点
                cx.emit(SessionEvent::TurnSettled);
            }
            Event::AgentEnd { will_retry } => {
                // willRetry 只说明 pi「打算」重试（乐观预判）——结局旗标不在这里
                // 清：重试真跑起来后由它的 assistant 消息覆盖，没跑起来就保留
                // 上一跳的错误结局（见 AgentStart / MessageEnd 注释）。
                if will_retry {
                    // 重试退避：轮**没**结束（pi 退避后重发 agent_start 接着跑），
                    // agent_running 必须保持 true——composer 停止按钮/输入框提示/
                    // 等待行都读它，就地复位会在退避的 2-8s 里把「停止」翻回
                    // 「发送」、界面假装空闲（pi 语义：isStreaming=_isAgentRunActive
                    // 持续到 settled，isIdle 明列 retry 不算 idle；pi-web 全程
                    // 显示运行中）。轮末收尾（响铃/落盘基线/列表/锚点）一并推迟
                    // 到真轮末；退避倒计时文案由 auto_retry_start 负责。用户此时
                    // 点停止仍走 abort_stream：pi abortRetry 取消退避，乐观复位
                    // 语义不变。
                    self.phase_waiting = false;
                    self.streaming_content = false;
                    cx.emit(SessionEvent::Changed);
                } else {
                    self.agent_running = false;
                    self.phase_waiting = false;
                    self.streaming_content = false;
                    // 同上：轮末不退役锚点（短回复继续钉在屏顶）
                    self.settle_turn();
                    // our own writer advanced the file — re-baseline so the
                    // external-append tick doesn't re-read our own turn
                    self.sync_disk_baseline();
                    // the session file exists now — make the new session show up
                    // in the sidebar (pi-web refreshKey-on-agent_end parity)
                    cx.emit(SessionEvent::ListDirty);
                    // refresh fork anchors so newly-sent user messages gain entry ids
                    self.refresh_anchors();
                    // agent may have written files: refresh git status
                }
            }
            Event::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message: _,
            } => {
                // 状态行（快照/远程端消费）报倒计时与次数；错误原文可能很长，
                // 不进一行状态。桌面端此时按钮保持停止态、等待行照常转。
                let secs = (delay_ms / 1000).max(1);
                self.status = crate::i18n::tf(
                    "status.auto_retry",
                    &[
                        ("secs", secs.to_string()),
                        ("attempt", attempt.to_string()),
                        ("max", max_attempts.to_string()),
                    ],
                );
                cx.emit(SessionEvent::Changed);
            }
            Event::AutoRetryEnd {
                success,
                attempt: _,
                final_error: _,
            } => {
                // success=true：重试尝试已通过，轮继续（随后的 AgentStart 会把
                // 状态行拨回 running）；false：耗尽/被中止，真轮末的
                // agent_end{willRetry:false}+agent_settled 马上到，收尾归它们。
                if success {
                    self.status = "running".into();
                }
            }
            // ---- 此前落 Unparsed 被静默丢弃的会话事件（见 pi-rpc.md）----
            Event::CompactionStart { .. } => {
                // 上下文自动压缩（threshold/overflow）在轮中发生：此前整个
                // 事件被丢，界面毫无说明地「卡住」
                self.status = tr("status.auto_compacting").into();
            }
            Event::CompactionEnd {
                aborted,
                will_retry,
                error_message,
                ..
            } => {
                // 定局失败（不重试、未中止）才报：其余情况真轮末事件马上到
                if let Some(err) = error_message.filter(|_| !aborted && !will_retry) {
                    self.status = crate::i18n::tf(
                        "status.auto_compact_failed",
                        &[("error", truncate_line(&err, 160))],
                    );
                }
            }
            Event::SummarizationRetryScheduled {
                attempt,
                max_attempts,
                delay_ms,
                error_message: _,
            } => {
                // 上下文溢出摘要重试：与 auto_retry 同构的退避窗口，轮未结束
                let secs = (delay_ms / 1000).max(1);
                self.status = crate::i18n::tf(
                    "status.summarization_retry",
                    &[
                        ("secs", secs.to_string()),
                        ("attempt", attempt.to_string()),
                        ("max", max_attempts.to_string()),
                    ],
                );
                cx.emit(SessionEvent::Changed);
            }
            Event::SummarizationRetryAttemptStart { .. } => {
                // 一次摘要重试尝试开跑（同 AutoRetryEnd success → running）
                self.status = "running".into();
            }
            Event::SummarizationRetryFinished => {}
            Event::ExtensionError { error, .. } => {
                // 扩展崩溃不终止轮，但要在状态行留痕（此前静默丢弃）
                self.status = crate::i18n::tf(
                    "status.extension_error",
                    &[("error", truncate_line(&error.unwrap_or_default(), 160))],
                );
            }
            Event::ProcessExited { code } => {
                // 合成事件：退出码先存，轮终语义由 pump 尾部（通道关闭后）
                // 统一补全——那里才能拿到 was_running 并一次收尾
                self.exit_code = code;
            }
            Event::WireEvent { kind, value } => {
                // 044-0 B3②：tool_execution_update 此前整体丢弃——委派被拒的
                // 失败 details 要等同帧其余工具全部收尾才随 toolResult 上屏，
                // 运行期完全不可见。这里把 partialResult.details 合入对应
                // ToolCall 块（先限定 agent 卡：running/failed 状态即时化的
                // 唯一消费者）；正文仍走 toolResult 主通道避免双写。
                if kind == "tool_execution_update" && value["toolName"].as_str() == Some("agent") {
                    if let (Some(tcid), Some(det)) = (
                        value["toolCallId"].as_str(),
                        value["partialResult"]["details"].as_object(),
                    ) {
                        if let Some(m) = self.messages.last_mut() {
                            crate::session::messages::merge_tool_update(
                                m,
                                tcid,
                                serde_json::Value::Object(det.clone()),
                            );
                            cx.notify();
                        }
                    }
                }
            }
            // 思考档真值（pi 按模型能力截断后广播）：状态槽是胶囊标签的
            // 首选来源，不跟着它走就永远停在 get_state 那一刻的旧值——真机
            // 症状「怎么换都是 high」（pi 早已是 low/minimal，标签不动）。
            Event::ThinkingLevelChanged { level } => {
                if let Some(st) = self.state.as_mut() {
                    st.thinking_level = Some(level.clone());
                }
                self.reconcile_thinking_override();
                cx.notify();
            }
            // pi 队列全量快照（入队/投递/清空都发）：镜像供「已排队 N 条」
            // 状态行；投递回显时 pi 已同步摘除队列项，镜像自动跟真
            Event::QueueUpdate {
                steering,
                follow_up,
            } => {
                self.queued_steering = steering;
                self.queued_followups = follow_up;
                // 090：计数并入 PF 本地队列（状态行口径 = queue_total）
                if self.agent_running && self.queue_total() > 0 {
                    self.status = crate::i18n::tf(
                        "status.queued_n",
                        &[("count", self.queue_total().to_string())],
                    );
                }
            }
            Event::ExtensionUi(req) => cx.emit(SessionEvent::ExtUi(req)),
            // `!` 执行的流式输出增量：就地追加进乐观卡片（pi-web 不消费这个
            // 事件、靠会话重载——这里直接流式渲染，严格更好）
            Event::BashExecutionUpdate { delta, .. } => {
                if let Some(ix) = self.pending_bash {
                    if let Some(m) = self.messages.get_mut(ix) {
                        if let Some(b) = m.bash.as_mut() {
                            b.output.push_str(&delta);
                        }
                    }
                    self.notify_list(cx);
                }
            }
            Event::Unparsed(v) => {
                // 未知 wire 事件不再完全静默：按 type 去重留痕（新 pi 版本
                // 加事件时至少 console 有一条）
                let kind = v["type"].as_str().unwrap_or("(no type)").to_string();
                if self.unparsed_types.insert(kind.clone()) {
                    eprintln!("[pi-flash] unparsed pi wire event type: {kind}");
                }
            }
        }
        // 用户消息到达 → 按**发送帧**的钉顶决定翻页：空闲普通发送钉视口
        // 顶，历史滚出屏（回显升级同锚点重入不重钉，见 page_turn）；打断
        // 投递（立即/Ctrl+Enter）发送帧不钉（queue.rs interrupt_deliver），
        // 回显到达同样不钉——否则乐观上屏的「不刷屏」在这里被补一刀，立
        // 即消息照旧把整屏顶出去（2026-10-20 三次验收）
        if user_arrived == Some(true) {
            let ix = self.messages.len() - 1;
            self.pager
                .page_turn(ix, self.messages.len(), self.phase_row_visible());
        }
        self.notify_list(cx);
    }
}
