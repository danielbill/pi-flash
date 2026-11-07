//! 运行时事件分发：on_event 主分发器（Agent 轮次臂 + 压缩/重试/扩展等
//! 小臂）、轮结算与 phase/通知小件。Response 命令回执臂在 response.rs，
//! 消息流三臂在 stream.rs，RPC 泵（consume_runtime_events）epoch 守卫在
//! pump.rs。

use gpui::Context;
use pi_link::protocol::Block;

use crate::i18n::tr;
use crate::session::messages::Role;

use super::*;

mod on_event;

impl SessionRuntime {
    /// 轮末提示音（「其他」页开关）。`stream_started` 兼作「本轮在飞」的闩：
    /// AgentSettled 与 AgentEnd 是同一轮的两个终止事件、都会到达，所以在这里
    /// take 掉它就恰好响一次；abort_stream() 会清掉闩，用户中止的一轮静音。
    pub(super) fn settle_turn(&mut self) {
        if self.stream_started.take().is_some() && self.sound_on {
            crate::services::workspace::play_notify_sound();
        }
    }

    /// 进程死亡收尾：最后一条 assistant 消息里未回结果的工具卡就地落
    /// 错误态（红框 + 中断文案），不再永远转圈。其余块不动。
    pub(super) fn fail_dangling_tool_calls(&mut self) {
        let text = tr("messages.tool_interrupted").to_string();
        if let Some(m) = self.messages.last_mut() {
            if m.role == Role::Assistant {
                for b in m.blocks.iter_mut() {
                    if let Block::ToolCall {
                        result,
                        is_error,
                        args_partial,
                        result_arrived,
                        ..
                    } = b
                    {
                        if !*result_arrived {
                            *result_arrived = true;
                            *is_error = true;
                            *args_partial = false;
                            if result.is_empty() {
                                *result = text.clone();
                            }
                        }
                    }
                }
            }
        }
    }

    /// 重放 [`Self::system_msgs`] 刷新 `sys`（transcript 无 system 消息时保留
    /// 旧值，维持「尚未加载」语义）；变了就发 Changed，让开着的信息面板当场
    /// 点亮。
    pub(super) fn replay_system(&mut self, cx: &mut Context<Self>) {
        if let Some(sys) = pi_link::transcript::transcript_system(&self.system_msgs) {
            let changed = match &self.sys {
                Some(prev) => prev.prompt != sys.prompt || prev.tools != sys.tools,
                None => true,
            };
            self.sys = Some(sys);
            if changed {
                cx.emit(SessionEvent::Changed);
            }
        }
    }

    /// 思考档对账：pi 的 `thinkingLevel` 是唯一真值（请求档会被模型能力
    /// 截断——deepseek-flash 只有 off/low/high/max，请求 `minimal` 实际是
    /// `low`）。override 只在不是 auto（None）时归一过来：它要跟 pi 同档，
    /// 否则菜单打勾与胶囊各说各话，respawn 的 `--thinking` 还会带着一个 pi
    /// 根本不支持的档。
    pub(super) fn reconcile_thinking_override(&mut self) {
        let Some(real) = self.state.as_ref().and_then(|s| s.thinking_level.clone()) else {
            return;
        };
        if self
            .thinking_override
            .as_deref()
            .is_some_and(|ov| ov != real)
        {
            self.thinking_override = Some(real);
        }
    }
}

impl SessionRuntime {
    /// pi-web ChatWindow agentRunning && !hasStreamingContent parity: the
    /// waiting/running row shows while no content is streaming — before the
    /// first assistant event (waiting) and during tool execution (running).
    pub(crate) fn phase_row_visible(&self) -> bool {
        (self.phase_waiting || self.agent_running) && !self.streaming_content
    }

    /// pi-web phaseLabel parity: running tool names derive from the last
    /// assistant message's unanswered tool calls; none pending = waiting.
    pub(crate) fn phase_label(&self) -> String {
        let tools: Vec<String> = self
            .messages
            .last()
            .filter(|m| m.role == Role::Assistant)
            .map(|m| {
                m.blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::ToolCall { name, result, .. }
                            if !name.is_empty() && result.is_empty() =>
                        {
                            Some(name.to_string())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        match tools.len() {
            0 => {
                if self.phase_command {
                    // 斜杠命令在途（handler 还在跑：阻塞菜单/查询都算）：
                    // 语义与模型无关，不能谎报「正在等待模型」（#7）
                    tr("status.running_command").to_string()
                } else {
                    tr("status.waiting_model").to_string()
                }
            }
            1 => crate::i18n::tf("status.running_tool", &[("name", tools[0].clone())]),
            n if n <= 3 => crate::i18n::tf("status.running_tools", &[("names", tools.join(", "))]),
            n => crate::i18n::tf(
                "status.running_tools_with_extra",
                &[
                    ("names", tools[..2].join(", ")),
                    ("count", (n - 2).to_string()),
                ],
            ),
        }
    }

    /// 列表同步：结构手术（splice/reset）与翻页垫片结算全在 ChatList
    /// （chat_list.rs，pi-web useAgentSession 滚屏层的移植）。
    ///
    /// 带 33ms 节流门（2026-11-06 流式卡死修复 A2）：列内容类拍（pump 事
    /// 件 / bash 输出 / 外部追加观察 / 帧补账）统一最小间隔 ≈30fps 上限——
    /// 快流模型 100+ delta/s 时每条全窗失效会把前台队列塞满、输入饿死。
    /// 间隔未到置 pending，trailing timer 补一拍（事件停了最后一拍也能落
    /// 地）；pager.sync 按当前 messages 全量对账且幂等，推迟到补拍执行安
    /// 全。终端态（轮终/错误/进程死亡）不经本函数，照旧直拍。
    pub(crate) fn notify_list(&mut self, cx: &mut Context<Self>) {
        const NOTIFY_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(33);
        let now = std::time::Instant::now();
        if let Some(t) = self.notify_last {
            if now.duration_since(t) < NOTIFY_MIN_INTERVAL {
                if !self.notify_pending {
                    self.notify_pending = true;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(NOTIFY_MIN_INTERVAL).await;
                        let _ = this.update(cx, |rt, cx| {
                            rt.notify_pending = false;
                            rt.notify_last = Some(std::time::Instant::now());
                            rt.pager.sync(rt.messages.len(), rt.phase_row_visible());
                            rt.nav_dirty = true;
                            cx.notify();
                        });
                    })
                    .detach();
                }
                return;
            }
        }
        self.notify_last = Some(now);
        self.pager
            .sync(self.messages.len(), self.phase_row_visible());
        // 消息变化（含流式 delta 路径）→ 导航摘要置脏，渲染帧惰性重建
        self.nav_dirty = true;
        cx.notify();
    }

    /// 清除复制反馈（032 恢复）：点亮 ~1.5s 后把 copy_flash 复位并重绘
    /// （pi-web copied-reset parity）。
    pub(crate) fn spawn_flash_clear(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |r, cx| {
                r.copy_flash = None;
                cx.notify();
            });
        })
        .detach();
    }
}
