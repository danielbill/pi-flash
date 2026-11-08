//! 消息流三臂：`on_event` 的 message_start / message_update / message_end
//! 分支（乐观气泡回显升级、流式分片写入、权威整表终态 + 首条消息落盘
//! 刷侧栏）。`on_event` 其余分支在 events.rs / response.rs。

use gpui::Context;
use pi_link::protocol::{AssistantEvent, Block, Usage};

use crate::session::messages::{
    COMPLETION_CUSTOM, Msg, Role, UsageLine, merge_tool_result, msg_is_mcp_banner, result_payload,
};

use super::*;

impl SessionRuntime {
    /// Returns the 发送帧钉顶决定（`user_arrived`）——仅 user 回显臂产生。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_message_start(
        &mut self,
        role: String,
        blocks: Vec<Block>,
        timestamp: Option<i64>,
        is_error: bool,
        tool_call_id: Option<String>,
        custom_type: Option<String>,
        custom_display: bool,
        details: Option<serde_json::Value>,
        raw_system: Option<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let mut user_arrived: Option<bool> = None;
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
                if let Some(ix) = self.pending_echoes.iter().position(|p| p.text == echo_text) {
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
                // 031-1：计划提醒是内部指令（pf-plan-context），live 流不进
                // transcript（重放路径 ingest.rs 同款过滤）
                if custom_type.as_deref() == Some(crate::mode::CONTEXT_CUSTOM_TYPE) {
                    return None;
                }
                // customType/details 必须落表（pi-flash-9eck）：background
                // 子代理的终态按 `custom_type=="subagent-completion"` +
                // `details.childId` 折进派发卡片——此前 live 事件臂丢弃这
                // 两个字段，卡片转圈到永远（重开会话走磁盘重放才自愈）。
                let is_settled = custom_type.as_deref() == Some(COMPLETION_CUSTOM);
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
                    custom_type,
                    custom_display,
                    details,
                    model: None,
                });
                // 终态即到即归一（与装载路径同一函数）：一个子会话只有一个
                // 状态——卡片 spinner 与导航点此刻就落色，不等事后回灌。
                if is_settled {
                    crate::session::messages::apply_completion_messages(&mut self.messages);
                }
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
        user_arrived
    }

    pub(super) fn on_message_update(&mut self, assistant_event: AssistantEvent) {
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
                // toolCall 对象里带 id（vendored pi: toolcall_end 的 toolCall =
                // {type,toolCall,id,name,arguments}）——不补的话块 id 永远是
                // 空串，tool_execution_update 按 id 匹配全部落空，卡片收不到
                // 派发/状态回写。
                let id = tool_call["id"].as_str().unwrap_or("").to_string();
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
                let id_c = id;
                let name_c = name;
                let args_c = args;
                if let Block::ToolCall { id, name, args, .. } = self.assistant_slot(
                    content_index,
                    Block::ToolCall {
                        content_index,
                        id: id_c.clone(),
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
                    if !id_c.is_empty() {
                        *id = id_c;
                    }
                    *name = name_c;
                    *args = args_c;
                }
            }
            AssistantEvent::Other(_) => {}
        }
    }

    /// Returns false = 管道内提前收场（MCP 横幅气泡就地消化），调用方
    /// 跳过 on_event 尾部的翻页与 notify_list。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_message_end(
        &mut self,
        role: String,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        timestamp: Option<i64>,
        stop_reason: Option<String>,
        error_message: Option<String>,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
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
                        return false;
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
        true
    }
}
