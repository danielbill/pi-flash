//! Response 命令回执臂：`on_event` 的 `Event::Response` 分支按命令拆成
//! 本文件的私有 fn 链（prompt/flush 关联、get_state/stats、fork/clone、
//! get_messages 投影、compact、bash、clear_queue 撤回……）。
//! 返回 false = 管道内提前收场（leaf 修复在飞），调用方跳过 on_event 尾部。

use std::path::PathBuf;

use gpui::Context;
use pi_link::protocol::{Command, SessionState, SessionStats, SlashCommand, parse_tree};

use crate::i18n::tr;
use crate::services::format::status_line;

use super::*;

impl SessionRuntime {
    /// 摘除「无回显替身」乐观气泡：扩展命令吃掉 prompt 时 pi 不回显同文
    /// user 消息；**会话已落盘**时 pi-web 的 `prompt_done → loadSession` 会用
    /// 磁盘真相覆盖列表（盘上没有这条命令消息），这里做同一口径的即时版。
    /// 未落盘（纯 UI 命令 / 新草稿）不调本函数——pi-web 的 loadSession 走
    /// 404 分支，**不动消息列表**。
    /// 文本匹配与回显臂同款（len 预筛 + plain_text 全量比对），echo 按
    /// FIFO 摘一条——连发同文命令时每个 response 各摘一对。
    pub(super) fn retire_stale_echo(&mut self) -> bool {
        let Some(text) = self.pending_echoes.last().map(|p| p.text.clone()) else {
            return false;
        };
        let Some(ix) = self.messages.iter().rposition(|m| {
            m.role == Role::User && m.text_len() == text.len() && m.plain_text().as_ref() == text
        }) else {
            return false;
        };
        self.messages.remove(ix);
        if let Some(p) = self.pending_echoes.iter().position(|p| p.text == text) {
            self.pending_echoes.remove(p);
        }
        true
    }

    /// 命令相位收场（只由 pi 的 `prompt` handled 回执驱动——rpc-mode 在
    /// `await command.handler()` 之后才发，等价 pi-web 的 prompt_done）：
    /// 命令相位（文案）**无条件**收场。`agent_running` 可能来自**上一轮**
    /// （用户在 agent 跑着时发的命令），拿它挡住会把等待行永远卡在
    /// 「执行扩展命令中」；真有 run 在跑时 `phase_waiting` 保持（那是那条
    /// 真轮在等，pi-web `if (!sdkAgentActive) settleUiStage()` 同款）。
    /// 消息处置按 pi-web `loadSession` 的磁盘真相：会话已落盘 → 盘上没有
    /// 这条命令消息，摘乐观气泡；**未落盘** → pi-web 走 404 分支不动列表，
    /// 气泡与侧栏 transient 行（PF 伪标签）都保留。
    pub(super) fn settle_command_phase(&mut self, cx: &mut Context<Self>) {
        self.phase_command = false;
        if !self.agent_running {
            self.phase_waiting = false;
            self.status = status_line(true, "idle");
        }
        // 顺手补绑定（文件已落盘就换真身）；绑定成功 = 盘上有这个会话 =
        // pi-web loadSession 拿到 200，磁盘真相赢。未绑定（草稿 / 纯 UI
        // 命令压根没落盘）保留气泡与伪标签。
        self.try_bind_reported_file(cx);
        if self.file.is_some() && self.retire_stale_echo() {
            self.pager.release();
        }
        cx.notify();
    }

    pub(super) fn on_response(
        &mut self,
        id: String,
        command: &str,
        success: bool,
        error: Option<String>,
        data: Option<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> bool {
        // 031-1 裁决 7 防哑弹：/plan 类扩展命令的在途回执。disposition 非
        // handled（-e 装载失败 → pi 把它当普通消息发给模型）= 回滚乐观位 +
        // toast；handled 时真值由 setStatus("pf-plan") 广播驱动（裁决 1），
        // 这里无需动作。/plan 不走命令相位机制（没设 phase_command），提前
        // 收场不落下面的 prompt 臂。
        if command == "prompt"
            && self
                .plan_cmd_inflight
                .as_ref()
                .is_some_and(|(pid, _)| pid == &id)
        {
            let (_, prev) = self.plan_cmd_inflight.take().unwrap();
            let handled = success
                && data
                    .as_ref()
                    .and_then(pi_link::protocol::prompt_disposition)
                    == Some("handled");
            if !handled {
                self.plan_enabled = prev;
                self.notify_toast(tr("plan.ext_unavailable").to_string(), cx);
            }
            cx.notify();
            return true;
        }
        if command == "prompt" && self.flush_in_flight.as_ref().is_some_and(|f| f.id == id) {
            // 090 flush 关联（queue.rs flush_local_queue）：队首/打断槽
            // 消息以 Prompt{None} 发出后的回执。success = pi 接受并开跑
            // （普通队列 flush 摘掉队首——回显经 message_start 正常入
            // 流；打断槽条目已出槽，无动作）；failure = pi 实际在忙
            // （AgentEnd..Settled 续跑窗的猜测误差）**响亮拒绝**——
            // 队首仍在队列、打断条目放回槽，等下一次 settled 重试，
            // 不静默丢、更不会插进当前轮。
            let in_flight = self.flush_in_flight.take();
            // 排队的斜杠命令（agent 跑着时用户发 /usage 之类）也走这条
            // 链：pi 对扩展命令**立即执行且不开 run**，回执是
            // `disposition:"handled"` 而不是普通轮。命令相位必须在这里
            // 收场——等 settled 永远等不到（真机回归：等待行永久
            // 「执行扩展命令中」，弹窗关不掉/关掉也不退）。
            if success
                && data
                    .as_ref()
                    .and_then(pi_link::protocol::prompt_disposition)
                    == Some("handled")
            {
                self.settle_command_phase(cx);
            }
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
                // leaf-repair 只增强、不闸门（pi-flash-9eck）：needle 计数有
                // 假阳性，先把 RPC 权威投影上屏放行（空尾窗的老洪泛会话不能
                // 再等一轮全文件重读才看到内容）；后台磁盘链更长才整表换入，
                // 落败无需动作——权威投影已在屏上。
                let short = rpc_msgs.len() < self.disk_msg_count && self.file.is_some();
                let rpc_len = rpc_msgs.len();
                self.project_rpc_messages(rpc_msgs, cx);
                if short {
                    self.spawn_leaf_repair(rpc_len, None, cx);
                }
            } else {
                // 快照缺 data：没有可投影的内容，加载页放行（真空会话语义，
                // 2026-11-07）——不能让打开永远钉在「会话载入中…」
                self.messages_loaded = true;
            }
            self.status = status_line(true, "resumed");
        } else if command == "get_messages" {
            // 失败响应：加载页放行（回退空会话页 + 状态条报错，不无限加载）
            self.messages_loaded = true;
        } else if command == "compact" {
            // 手动压缩（圆环弹窗按钮/斜杠命令）：response 在摘要 LLM
            // 完成后才回（带 summary/usage）——清防连点标志并重拉
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
                // 压缩前后 token 对比（response data 有 tokensBefore/
                // estimatedTokensAfter，pi agent-session.js compactResult）
                let detail = data.as_ref().map(|d| {
                    let before = d["tokensBefore"].as_u64();
                    let after = d["estimatedTokensAfter"].as_u64();
                    match (before, after) {
                        (Some(b), Some(a)) if b > 0 => {
                            format!("（{b} → {a} tokens）")
                        }
                        _ => String::new(),
                    }
                });
                let msg = format!("{}{}", tr("slash.compact_done"), detail.unwrap_or_default());
                self.notify_toast(msg, cx);
            } else {
                // pi 拒绝场景要有出口：会话太小（Nothing to compact）/
                // 已压缩过（Already compacted）/摘要 LLM 出错——此前
                // 写进无渲染死字段，用户视角就是「压缩毫无反应」
                let msg = format!(
                    "{}: {}",
                    tr("slash.compact_failed"),
                    error.unwrap_or_default()
                );
                self.notify_toast(msg, cx);
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
                    if let Some(ix) = self.pending_echoes.iter().position(|p| p.text == *text) {
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
                    self.status =
                        crate::i18n::tf("status.steer_dropped", &[("count", dropped.to_string())]);
                    cx.notify();
                }
            }
        } else if command == "prompt"
            && success
            && data
                .as_ref()
                .and_then(pi_link::protocol::prompt_disposition)
                == Some("handled")
        {
            // 扩展命令 / 输入 handler 吃掉了 prompt（#7「装了扩展，
            // 使用的时候会话页面卡住了」）：pi 对这类输入**不开 run**
            // ——没有 agent_start / 同文回显 / agent_settled 可等，
            // wire 上只有命令自己的 UI 事件（extension_ui_request/
            // extension_error，ext_ui 已渲染）+ 本 response。
            // handled 无条件收场命令相位：`agent_running` 为 true 可能
            // 只是**上一轮**还在跑（用户在 agent 跑着时发的命令），
            // 拿它挡住 = 行永远卡在「执行扩展命令中」（真机回归）。
            // handler 自己 sendMessage 引出真轮的情形：AgentStart 先于
            // 本 response 到达（同管道序），`phase_waiting` 由真轮接管。
            self.settle_command_phase(cx);
        } else if success {
            self.status = format!("{command} ok");
        } else if command == "prompt" {
            self.status = format!("prompt failed: {}", error.unwrap_or_default());
            // 发送失败：等待行不能一直转下去；锚点也无从跟随
            self.phase_waiting = false;
            self.phase_command = false;
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
        true
    }
}
