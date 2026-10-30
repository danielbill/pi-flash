//! 运行时事件分发：RPC 事件泵消费、on_event 各事件臂、轮结算与
//! phase/通知小件。pump（consume_runtime_events）epoch 守卫在此。

use gpui::Context;
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, Usage,
    content_blocks, parse_tree,
};

use crate::i18n::tr;
use crate::session::messages::{
    Msg, Role, UsageLine, merge_tool_result, msg_is_mcp_banner, result_payload,
};

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
    fn replay_system(&mut self, cx: &mut Context<Self>) {
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
    fn reconcile_thinking_override(&mut self) {
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

    pub(super) fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
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
                if command == "prompt" && self.flush_in_flight.as_ref().is_some_and(|f| f.id == id)
                {
                    // 090 flush 关联（queue.rs flush_local_queue）：队首/打断槽
                    // 消息以 Prompt{None} 发出后的回执。success = pi 接受并开跑
                    // （普通队列 flush 摘掉队首——回显经 message_start 正常入
                    // 流；打断槽条目已出槽，无动作）；failure = pi 实际在忙
                    // （AgentEnd..Settled 续跑窗的猜测误差）**响亮拒绝**——
                    // 队首仍在队列、打断条目放回槽，等下一次 settled 重试，
                    // 不静默丢、更不会插进当前轮。
                    let in_flight = self.flush_in_flight.take();
                    if success {
                        let from_interrupt = in_flight.is_some_and(|f| f.interrupt.is_some());
                        if !from_interrupt && !self.queued_inputs.is_empty() {
                            self.queued_inputs.remove(0);
                        }
                    } else {
                        if let Some(entry) = in_flight.and_then(|f| f.interrupt) {
                            self.interrupt_pending = Some(entry);
                        }
                        self.status = tr("queue.pushed_back").into();
                    }
                    cx.notify();
                } else if command == "set_thinking_level" && success {
                    // 请求档可能被 pi 按模型能力**截断**，而 pi 只在真变了才
                    // 广播 thinking_level_changed（请求 minimal 落在当前档时
                    // 一声不响）——回执补一次 get_state 把 override 归一到
                    // 真值，菜单打勾才与 pi 同档（否则 override 会留着一个
                    // pi 根本不支持的档，respawn 的 --thinking 也带着它）。
                    self.rpc(&Command::GetState);
                } else if command == "get_state" && success {
                    if let Some(data) = &data {
                        let st = SessionState::parse(data);
                        // pi 换了绑定的会话文件（draft 首条 prompt 落盘 /
                        // fork / clone 换分支）→ shell 身份必须跟着换
                        let rebound = st.session_file.clone().map(PathBuf::from);
                        self.state = Some(st);
                        // 换模型/轮末/思考档回执都会走到这里：状态槽是
                        // 标签真值源，override 相形之下只是草稿意图
                        self.reconcile_thinking_override();
                        if let Some(f) = rebound {
                            self.follow_session_file(f, cx);
                        }
                    }
                    if self.pending_rename {
                        self.pending_rename = false;
                        let prefill = self
                            .state
                            .as_ref()
                            .and_then(|s| s.session_name.clone())
                            .or_else(|| {
                                self.messages
                                    .iter()
                                    .find(|m| matches!(m.role, Role::User))
                                    .map(|m| m.plain_text().into_owned())
                            })
                            .map(|v| v.chars().take(50).collect::<String>())
                            .unwrap_or_default();
                        cx.emit(SessionEvent::RenameReady(prefill));
                    }
                } else if command == "get_session_stats" && success {
                    if let Some(data) = &data {
                        self.stats = Some(SessionStats::parse(data));
                        if let Some(f) = data["sessionFile"].as_str().map(PathBuf::from) {
                            // same follow-the-rebind path as get_state: a draft
                            // persisted by its first prompt, or a fork/clone
                            // that moved pi onto the branched file
                            self.follow_session_file(f, cx);
                        }
                    }
                } else if command == "set_session_name" && success {
                    // pi flushed the name to the session file — reload the
                    // sidebar labels (pi-web onRenamed → loadSessions), with
                    // one delayed pass to cover flush lag
                    cx.emit(SessionEvent::ListDirty);
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(500))
                            .await;
                        let _ = this.update(cx, |_c, cx| {
                            cx.emit(SessionEvent::ListDirty);
                            cx.notify();
                        });
                    })
                    .detach();
                } else if command == "get_commands" && success {
                    if let Some(data) = &data {
                        let commands = SlashCommand::parse_list(data);
                        // 转发给 Chat：并进全局清单 + 回写自有缓存（010-启动.md §3）
                        cx.emit(SessionEvent::Commands(commands.clone()));
                        self.commands = commands;
                    }
                } else if command == "get_available_models" && success {
                    if let Some(data) = &data {
                        // project-level shared catalog: forward, don't store
                        cx.emit(SessionEvent::Models(pi_link::protocol::parse_model_list(
                            data,
                        )));
                    }
                } else if command == "set_model" && success {
                    // pi 1.0 swaps the model asynchronously — a get_state sent
                    // right after the command still reports the PREVIOUS model
                    // (measured), which made the pill lag one click behind.
                    // The set_model response carries the swapped model: apply
                    // it immediately, then re-sync state (thinking level now
                    // reflects the new model, e.g. "off" for non-reasoning).
                    if let Some(data) = &data {
                        if let Some(st) = self.state.as_mut() {
                            st.model = pi_link::protocol::parse_model_info(data);
                        }
                    }
                    self.refresh_state();
                } else if command == "get_entries" && success {
                    // fork 锚点的唯一可靠来源：扁平 entries + parentId 回溯。
                    // get_tree 在长会话上会被 pi 自己拒绝（见 GetEntries 注释），
                    // 033 导航面板因此降级为空，但「新分支」必须一直可用。
                    if let Some(data) = &data {
                        let (entries, leaf) = pi_link::protocol::parse_entries(data);
                        self.active_user_entry_ids =
                            pi_link::protocol::active_user_entry_ids(&entries, leaf.as_deref());
                        self.apply_entry_ids();
                    }
                } else if command == "get_tree" && success {
                    if let Some(data) = &data {
                        let (tree, leaf) = parse_tree(data);
                        // 树只是 033 导航面板的数据源；锚点以 get_entries 为准
                        // （同一条链、同一顺序），这里同步一份作为短会话的兜底
                        self.active_user_entry_ids =
                            super::fork::collect_path_user_ids(&tree, leaf.as_deref());
                        self.apply_entry_ids();
                        self.branch_tree = Some((tree, leaf));
                    }
                } else if command == "fork" || command == "clone" {
                    // clone = 整段复制（rpc clone → fork at leaf），与 fork
                    // 的落地动作完全一样
                    self.on_fork_response(success, error.as_deref(), cx);
                } else if command == "get_messages" && success {
                    if let Some(data) = &data {
                        let rpc_msgs = data["messages"].as_array().cloned().unwrap_or_default();
                        // 系统提示词 + 工具声明：pi 0.86+ 把它们写进 transcript
                        // 的 system 消息（sections / toolsAdded），replay 是唯一读法
                        // （pi-ai transcript.js；pi-web 的 System/Tools 面板同源）。
                        // 快照是权威源：system 消息整表替换后重放（live 流式追加
                        // 的增量以快照为准对齐）。在叶子链修复之前算——那份回退只
                        // 喂显示用的消息。
                        self.system_msgs = rpc_msgs
                            .iter()
                            .map(|e| e.get("message").unwrap_or(e))
                            .filter(|m| m["role"].as_str() == Some("system"))
                            .cloned()
                            .collect();
                        self.replay_system(cx);
                        // leaf-chain repair: pi anchors its restored leaf at the
                        // last entry of ANY type; a mis-parented custom entry
                        // (plan-mode-state 实测) strands whole turns off the
                        // chain and get_messages comes back short. Rebuild from
                        // the file anchored at the last message entry instead.
                        // 秒开深度优化（pi-flash-2o92）：全文重读是 O(会话大小)
                        // 的主线程冻结源（20MB 上限下 ~300ms+）——改后台线程
                        // 执行，期间维持现有显示（tail 预渲染），落地后磁盘链
                        // 赢则换入、落败则执行权威投影（同守卫下无陈旧回退）。
                        self.last_rpc_msg_count = Some(rpc_msgs.len());
                        if rpc_msgs.len() < self.disk_msg_count && self.file.is_some() {
                            let rpc_len = rpc_msgs.len();
                            self.spawn_leaf_repair(rpc_len, Some(rpc_msgs), cx);
                            return;
                        }
                        self.project_rpc_messages(rpc_msgs, cx);
                    }
                    self.status = status_line(true, "resumed");
                } else if command == "compact" {
                    // 手动压缩（圆环弹窗按钮）：response 在摘要 LLM 完成后
                    // 才回（带 summary/usage）——清防连点标志并重拉
                    // stats/messages（compaction 卡片 + 上下文环跟着变）
                    self.compacting = false;
                    if success {
                        self.refresh_state();
                        if let Some(s) = self.agent.session.as_ref() {
                            let _ = s.send(&Command::GetMessages);
                        }
                        // 090：压缩期间扣住的本地队列（flush 复核 compacting
                        // 会跳过），压缩收尾这里补发队首
                        self.flush_local_queue(cx);
                    } else {
                        self.status = format!("compact failed: {}", error.unwrap_or_default());
                    }
                } else if command == "bash" {
                    // `!` shell 命令最终结果（流式增量已就地追加；这里回填
                    // 退出码/截断/取消并点亮完成态）。失败响应（会话忙被 pi
                    // 拒等）同样要收尾乐观卡片，否则「执行中」永不消失。
                    let result = if success {
                        data.map(|d| pi_link::protocol::BashResult::parse(&d))
                            .unwrap_or_default()
                    } else {
                        pi_link::protocol::BashResult {
                            output: error.clone().unwrap_or_default(),
                            exit_code: Some(1),
                            ..Default::default()
                        }
                    };
                    self.finalize_bash(result, cx);
                } else if command == "clear_queue" && success {
                    // abort 的配对撤回（根因见 abort_stream 注释）：response
                    // data 携带被撤下的未投递原文 {steering:[..], followUp:[..]}。
                    // 分流（2026-10-20 验收修订）：
                    // - **已乐观上屏的**（pending_echoes 命中）= 用户视角
                    //   「已发送」——气泡**保留**、文本不回编辑器（回输入框
                    //   会把发出去的话原样弹回，真机验收否掉；转排队卡同理）。
                    //   它确实没进模型上下文，快照重读后气泡自然消失；↑ 历史
                    //   里仍有原文可捞回。
                    // - 从未上屏的（090 后只剩 pif-ui 自动化残留）：删乐观
                    //   气泡兜底 + 文本回填输入框（14e6059 语义）。
                    let d = data.unwrap_or(serde_json::Value::Null);
                    let mut recalled: Vec<String> = Vec::new();
                    for key in ["followUp", "steering"] {
                        if let Some(arr) = d.get(key).and_then(|v| v.as_array()) {
                            for t in arr {
                                if let Some(s) = t.as_str() {
                                    if !s.is_empty() {
                                        recalled.push(s.to_string());
                                    }
                                }
                            }
                        }
                    }
                    if !recalled.is_empty() {
                        let mut dropped = 0usize;
                        let mut editor_recalled: Vec<String> = Vec::new();
                        for text in &recalled {
                            if let Some(ix) =
                                self.pending_echoes.iter().position(|p| p.text == *text)
                            {
                                // 已上屏：摘 pending_echoes（不会再回显），气泡留
                                self.pending_echoes.remove(ix);
                                dropped += 1;
                            } else {
                                // 未上屏：删乐观气泡兜底（同文匹配口径与回显臂
                                // 一致）+ 回编辑器
                                if let Some(ix) = self.messages.iter().rposition(|m| {
                                    m.role == Role::User
                                        && m.text_len() == text.len()
                                        && m.plain_text().as_ref() == text
                                }) {
                                    self.messages.remove(ix);
                                }
                                editor_recalled.push(text.clone());
                            }
                        }
                        if !editor_recalled.is_empty() {
                            // 锚点可能指着被删的气泡：按「重读后」口径重锚
                            self.restore_anchor_after_reload();
                            self.status = crate::i18n::tf(
                                "status.recalled_queued",
                                &[("count", editor_recalled.len().to_string())],
                            );
                            // pi 侧撤回的条目不带附件（附件随消息入队即消费），
                            // images 恒空；本地队列的撤回（090 abort）才有附件
                            cx.emit(SessionEvent::RecallInput {
                                text: editor_recalled.join("\n\n"),
                                images: Vec::new(),
                            });
                        }
                        if dropped > 0 {
                            self.status = crate::i18n::tf(
                                "status.steer_dropped",
                                &[("count", dropped.to_string())],
                            );
                            cx.notify();
                        }
                    }
                } else if success {
                    self.status = format!("{command} ok");
                } else if command == "prompt" {
                    self.status = format!("prompt failed: {}", error.unwrap_or_default());
                    // 发送失败：等待行不能一直转下去；锚点也无从跟随
                    self.phase_waiting = false;
                    // **不清 pending_echoes**：prompt 被拒（鉴权/无模型）时 pi
                    // 侧队列原封不动——之前排队的消息仍会在下一轮跑完时回
                    // 显，届时还要靠 pending_echoes 去重（清了就是「排队气泡
                    // 几分钟后又冒出一条」的重复气泡）。
                    self.pager.release();
                    // 发送失败 = 本轮未跑成，红点
                    self.turn_error = true;
                    // preflight 拒绝（无鉴权/无模型）= 什么都没落盘，撤伪标签
                    if !self.try_bind_reported_file(cx) {
                        cx.emit(SessionEvent::PendingTab(None));
                    }
                } else if command == "follow_up" || command == "steer" {
                    // 排队/插话被 pi 拒（扩展命令不允许排队、输入被
                    // handler 消费掉、无可用模型…）：`PiSession::send` 只写 stdin
                    // 不等应答，拒信是**异步**回来的，所以不能在发送帧上
                    // 就判死。乐观气泡**留着**（用户还看得见自己写了什么，
                    // 不算静默丢失），这里只负责出声原因。
                    self.status = format!(
                        "{}: {}",
                        tr("status.queued"),
                        error.unwrap_or_else(|| "—".into())
                    );
                } else {
                    self.status = format!("{command} failed: {}", error.unwrap_or_default());
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
                // is_error 只出现在 toolResult 上（协议口径：非 toolResult 恒
                // false），而工具报错是 agent 工作过程的一部分——它经常一边
                // 试错一边把事情做完（grep 未命中、命令非零退出…），**不是**
                // 这一轮的「外部错误/崩溃」。红点只认轮末终值：助手消息
                // stopReason=="error"（见 MessageEnd）与 prompt 发送失败。
                match role.as_str() {
                    "user" => {
                        // 轮次边界：结局旗标在用户消息处归零，由随后的每条
                        // assistant 消息按 stopReason **覆盖**（见 MessageEnd）。
                        // 不在 AgentStart/AgentEnd 清——自动重试同样会发
                        // AgentStart（会把上一跳的错误结局抹掉），而
                        // AgentEnd(willRetry) 是 pi 的乐观预判：先报 true，再看
                        // `_prepareRetry` 决定是否真的重试；退避期间被中止（用户
                        // 停止/新消息）或重试未跑起来时，清掉的错误旗再也没机会
                        // 置回来——「错误轮次报青蓝」的根因。
                        self.turn_error = false;
                        self.turn_interrupted = false;
                        // upgrade the optimistic send bubble in place instead
                        // of pushing a duplicate echo (pi-web
                        // optimisticUserMessageKey parity); image blocks and
                        // entry ids ride in with the echo
                        let echo_text = blocks
                            .iter()
                            .map(|b| match b {
                                Block::Text { text, .. } => text.as_str(),
                                _ => "",
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        // 去重口径（2026-10-16）：只看 pending_echoes 里的同文
                        // 条目，命中就**按位置**就地升级那条乐观气泡，不再要求
                        // 它是 messages.last()——排队发送的回显前面必然堆着本轮
                        // 的 assistant 消息，last() 口径下必然重复成两条气泡。
                        // 不命中则照常 push（pi 侧自行产生的 user 消息）。
                        // 回显到达 ≠ agent 已开始应答，等待行（spark + 正在思考…
                        // + shimmer）必须活到第一个 assistant 事件
                        if let Some(ix) =
                            self.pending_echoes.iter().position(|p| p.text == echo_text)
                        {
                            // 发送帧的钉顶决定随条目走（PendingEcho.pin）：打断
                            // 投递（立即/Ctrl+Enter）上屏时不钉顶，回显升级同样
                            // 不许翻页——回显再补一次 page_turn 就是「立即刷屏」
                            // 的根因（2026-10-20 三次验收）
                            let echo_pin = self.pending_echoes.remove(ix).pin;
                            // 保障幅度：死信条目（拒绝误判 / 会话切换等）永不回显时只会占位，
                            // 不能无限堆。轮终不清（见上），这里补上限。
                            while self.pending_echoes.len() > 64 {
                                self.pending_echoes.remove(0);
                            }
                            if let Some(ix) = self.messages.iter().rposition(|m| {
                                // 长度预筛：全量 plain_text 只留给等长候选
                                //（大粘贴会话里全量拼几百条 = 秒级冻结）
                                m.role == Role::User
                                    && m.text_len() == echo_text.len()
                                    && m.plain_text().as_ref() == echo_text
                            }) {
                                let m = &mut self.messages[ix];
                                m.blocks = blocks;
                            }
                            user_arrived = Some(echo_pin);
                        } else {
                            self.messages.push(Msg {
                                role: Role::User,
                                bash: None,
                                blocks,
                                usage: None,
                                entry_id: None,
                                ts: timestamp,
                                end_ts: None,
                                stop_reason: None,
                                error_message: None,
                                custom_type: None,
                                custom_display: true,
                                details: None,
                                model: None,
                            });
                            // pi 侧自行产生的 user 消息（分支/压缩重放等）：无
                            // 发送帧可言，沿用原行为钉顶
                            user_arrived = Some(true);
                        }
                    }
                    "assistant" => {
                        self.phase_waiting = false;
                        // 新轮 assistant 消息 = 合法新流开始：此后的 delta 属于
                        // 它本身，切断标记使命结束（残包窗口只覆盖 abort..settle）
                        self.stream_cut = false;
                        // 不清 pending_echoes：排队发送的回显在**下一轮 turn 边界**
                        // 才到，中间会先流过本轮的一堆 assistant 事件；这里
                        // 清了就是“排队气泡回显时又被当成新消息推了一条”
                        // （2026-10-16）。收口放到轮终（AgentEnd/AgentSettled/发送失败/
                        // 整表重读）。
                        self.streaming_content = false;
                        self.messages.push(Msg {
                            role: Role::Assistant,
                            bash: None,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type: None,
                            custom_display: true,
                            details: None,
                            model: None,
                        });
                    }
                    "toolResult" => {
                        let (text, images) = result_payload(&blocks);
                        if let Some(m) = self.messages.last_mut() {
                            merge_tool_result(
                                m,
                                tool_call_id.as_deref(),
                                is_error,
                                &text,
                                images,
                                details.clone(),
                                timestamp,
                            );
                        }
                    }
                    "custom" => {
                        let _ = &details;
                        self.messages.push(Msg {
                            role: Role::Custom,
                            bash: None,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type: None,
                            custom_display: true,
                            details: None,
                            model: None,
                        });
                    }
                    "system" => {
                        // pi 每个 run 都会先追加一条 role:"system"（content /
                        // sections / toolsAdded 补丁），RPC message_start 全量
                        // 携带——收进 transcript 重放缓存，面板实时点亮
                        // （新会话第一轮就有，不必等下一次 get_messages）。
                        // message_end 不重复处理（start 已带全量）。
                        if let Some(raw) = raw_system {
                            self.system_msgs.push(raw);
                            self.replay_system(cx);
                        }
                    }
                    _ => {}
                }
                let _ = timestamp;
            }
            Event::MessageUpdate(assistant_event) => {
                self.phase_waiting = false;
                self.streaming_content = true;
                match assistant_event {
                    AssistantEvent::TextDelta {
                        content_index,
                        delta,
                    } => {
                        if let Block::Text { text, .. } = self.assistant_slot(
                            content_index,
                            Block::Text {
                                content_index,
                                text: String::new(),
                            },
                        ) {
                            text.push_str(&delta);
                        }
                    }
                    AssistantEvent::TextEnd {
                        content_index,
                        content,
                    } => {
                        if let Block::Text { text, .. } = self.assistant_slot(
                            content_index,
                            Block::Text {
                                content_index,
                                text: String::new(),
                            },
                        ) {
                            *text = content;
                        }
                    }
                    AssistantEvent::ThinkingStart { content_index } => {
                        self.assistant_slot(
                            content_index,
                            Block::Thinking {
                                content_index,
                                text: String::new(),
                            },
                        );
                    }
                    AssistantEvent::ThinkingDelta {
                        content_index,
                        delta,
                    } => {
                        if let Block::Thinking { text, .. } = self.assistant_slot(
                            content_index,
                            Block::Thinking {
                                content_index,
                                text: String::new(),
                            },
                        ) {
                            text.push_str(&delta);
                        }
                    }
                    AssistantEvent::ThinkingEnd {
                        content_index,
                        content,
                    } => {
                        if let Block::Thinking { text, .. } = self.assistant_slot(
                            content_index,
                            Block::Thinking {
                                content_index,
                                text: String::new(),
                            },
                        ) {
                            *text = content;
                            // pi-web parity: collapse thinking once it completes
                            let msg_ix = self.messages.len().saturating_sub(1);
                            self.collapsed.insert((msg_ix, content_index), false);
                        }
                    }
                    AssistantEvent::ToolCallStart {
                        content_index,
                        id,
                        tool_name,
                    } => {
                        self.assistant_slot(
                            content_index,
                            Block::ToolCall {
                                content_index,
                                id,
                                name: tool_name,
                                args: String::new(),
                                result: String::new(),
                                is_error: false,
                                images: Vec::new(),
                                duration_s: None,
                                details: None,
                                args_partial: true,
                                result_arrived: false,
                            },
                        );
                    }
                    AssistantEvent::ToolCallDelta {
                        content_index,
                        delta,
                    } => {
                        if let Block::ToolCall { args, .. } = self.assistant_slot(
                            content_index,
                            Block::ToolCall {
                                content_index,
                                id: String::new(),
                                name: String::new(),
                                args: String::new(),
                                result: String::new(),
                                is_error: false,
                                images: Vec::new(),
                                duration_s: None,
                                details: None,
                                args_partial: true,
                                result_arrived: false,
                            },
                        ) {
                            args.push_str(&delta);
                        }
                    }
                    AssistantEvent::ToolCallEnd {
                        content_index,
                        tool_call,
                    } => {
                        let name = tool_call["toolName"]
                            .as_str()
                            .or_else(|| tool_call["name"].as_str())
                            .unwrap_or("")
                            .to_string();
                        let args = tool_call["arguments"]
                            .as_object()
                            .filter(|o| !o.is_empty())
                            .map(|o| serde_json::Value::Object(o.clone()).to_string())
                            .unwrap_or_default();
                        let name_c = name;
                        let args_c = args;
                        if let Block::ToolCall { name, args, .. } = self.assistant_slot(
                            content_index,
                            Block::ToolCall {
                                content_index,
                                id: String::new(),
                                name: name_c.clone(),
                                args: args_c.clone(),
                                result: String::new(),
                                is_error: false,
                                images: Vec::new(),
                                duration_s: None,
                                details: None,
                                args_partial: false,
                                result_arrived: false,
                            },
                        ) {
                            *name = name_c;
                            *args = args_c;
                        }
                    }
                    AssistantEvent::Other(_) => {}
                }
            }
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
                if role == "assistant" {
                    // pi 冷启动横幅（🔌 MCP: N server(s) enabled）作为独立
                    // assistant 消息到达（也会落盘，下次 tail 解码同样滤），
                    // 渲染成气泡毫无意义（2026-10-25 定夺：桌面侧消化）
                    let banner_probe = Msg {
                        role: Role::Assistant,
                        bash: None,
                        blocks: blocks.clone(),
                        usage: None,
                        entry_id: None,
                        ts: None,
                        end_ts: None,
                        stop_reason: None,
                        error_message: None,
                        custom_type: None,
                        custom_display: true,
                        details: None,
                        model: None,
                    };
                    if msg_is_mcp_banner(&banner_probe) {
                        if let Some(m) = self.messages.last() {
                            if m.role == Role::Assistant && msg_is_mcp_banner(m) {
                                self.messages.pop();
                                self.streaming_content = false;
                                cx.emit(SessionEvent::Changed);
                                return;
                            }
                        }
                    }
                    if let Some(m) = self.messages.last_mut() {
                        if m.role == Role::Assistant {
                            m.blocks = blocks;
                            m.usage = usage.map(|u| UsageLine {
                                input: u.input,
                                output: u.output,
                                cache_read: u.cache_read,
                                cache_write: u.cache_write,
                                cost: u.cost,
                            });
                            m.ts = timestamp.or(m.ts);
                            m.end_ts = Some(crate::services::format::now_ms());
                            m.stop_reason = stop_reason;
                            m.error_message = error_message;
                            m.model = model;
                            self.streaming_content = false;
                            // 通知点三色 = **最后一条 assistant 消息的 stopReason**
                            // （与转写区 final_error/final_truncated 同一口径）：
                            // error → 红（外部错误/崩溃）；length/aborted → 黄
                            // （内部中断）；其余 = 正常完成青蓝。这里**覆盖**（不是
                            // 只置位）：pi 自动重试会产出多条 assistant 消息，只有
                            // 最后一条是这轮的结局（重试成功也要能从红回到正常）。
                            self.turn_error = m.stop_reason.as_deref() == Some("error");
                            self.turn_interrupted =
                                matches!(m.stop_reason.as_deref(), Some("length" | "aborted"));
                        }
                    }
                } else if role == "user" {
                    // 新会话首条消息落盘（pi session-manager `_persist`：会话有
                    // user/assistant 消息那一刻才建文件，见 `_hasConversation`）。
                    // 此刻文件刚出现在磁盘上，但 shell 侧的会话清单此前只在轮末
                    // （AgentEnd 的 ListDirty）重扫——新会话标签要等整轮回复跑完
                    // 才出现。首条用户消息一到就刷一次侧栏（pi-web
                    // 「running 会话未列入 → loadSessions」parity），再补一次
                    // 300ms 后的刷新兜住写盘 / 指纹扫描的滞后。
                    if self
                        .messages
                        .iter()
                        .filter(|m| m.role == Role::User)
                        .count()
                        == 1
                    {
                        cx.emit(SessionEvent::ListDirty);
                        cx.notify();
                        // 绑定对账：此刻文件已在盘上（pi 先 appendMessage 落
                        // 盘再发事件，存在性守卫此刻放行）→ FileBound 迁移
                        // pool key。不等 AgentSettled 的 refresh_state：否则
                        // 「真标签已上侧栏、pool key 还是 draft-N」的窗口里
                        // 点标签会 spawn 出 duplicate runtime。
                        self.try_bind_reported_file(cx);
                        cx.spawn(async move |this, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(300))
                                .await;
                            let _ = this.update(cx, |_c, cx| {
                                cx.emit(SessionEvent::ListDirty);
                                cx.notify();
                            });
                        })
                        .detach();
                    }
                }
            }
            Event::AgentStart => {
                // 不动 turn_error/turn_interrupted：自动重试也会走这里，结局由
                // assistant 消息（MessageEnd）覆盖、由用户消息（MessageStart）
                // 归零（见那两处注释）。
                self.agent_running = true;
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
            Event::WireEvent { .. } => {}
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
            0 => tr("status.waiting_model").to_string(),
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
    pub(crate) fn notify_list(&mut self, cx: &mut Context<Self>) {
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

    /// get_messages 的权威投影（整表替换——fixes doubled rows after the tail
    /// pre-render）：ingest 全表 + fork 锚点映射 + 尾部时间戳回填 + 定位/
    /// 钉顶/重绘。同步路径与后台 leaf 修复的落败分支共用（秒开深度优化
    /// pi-flash-2o92 把修复改后台后，落败分支在这里补投影）。
    pub(crate) fn project_rpc_messages(
        &mut self,
        rpc_msgs: Vec<serde_json::Value>,
        cx: &mut Context<Self>,
    ) {
        self.messages.clear();
        self.pending_echoes.clear();
        // 整表已按权威快照归一：切断窗口随之作废（残包即使再到也不能并入）
        self.stream_cut = false;
        for msg in &rpc_msgs {
            let blocks = content_blocks(&msg["content"]);
            let usage = Usage::parse(&msg["usage"]);
            self.ingest_message(msg, blocks, usage, None, cx);
        }
        // map user messages to active-path entry ids (fork anchors)
        let mut ids = self.active_user_entry_ids.iter();
        for m in self.messages.iter_mut() {
            if m.role == Role::User {
                m.entry_id = ids.next().cloned();
            }
        }
        // get_messages payloads carry generation-START stamps only;
        // completion stamps (回复用时) come from the session file's entry
        // write-times
        self.merge_tail_stamps();
        self.apply_pending_locate(cx);
        // 同一会话的重读保住钉顶（换工具预设/压缩后重拉都走这里）
        self.restore_anchor_after_reload();
        self.notify_list(cx);
    }

    // ---- 033 会话导航面板 ----
}
