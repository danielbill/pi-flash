//! 消息摄入：ingest_message 落表、assistant 槽位寻址（流式分片
//! 写入点，事件臂共用）。

use gpui::Context;
use pi_link::protocol::{Block, Usage, content_blocks};

use crate::session::messages::{BashInfo, Msg, Role, UsageLine, merge_tool_result, result_payload};

use super::*;

impl SessionRuntime {
    /// Convert one wire message (get_messages snapshot shape) into render
    /// state. Mirrors msgs_from_tail semantics exactly: user/assistant push,
    /// toolResult merges into the paired tool call, other roles skipped.
    pub(super) fn ingest_message(
        &mut self,
        msg: &serde_json::Value,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        entry_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let role = msg["role"].as_str().unwrap_or("");
        let ts = msg["timestamp"].as_i64();
        match role {
            "user" => {
                // 快照重建路径（打开会话/刷新循环调用）：不做清屏，
                // reset 后 Bottom 对齐保持贴底；清屏只属活事件路径
                self.messages.push(Msg {
                    role: Role::User,
                    bash: None,
                    blocks,
                    usage: None,
                    entry_id,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                });
            }
            "assistant" => {
                self.messages.push(Msg {
                    role: Role::Assistant,
                    bash: None,
                    blocks,
                    usage: usage.map(|u| UsageLine {
                        input: u.input,
                        output: u.output,
                        cache_read: u.cache_read,
                        cache_write: u.cache_write,
                        cost: u.cost,
                    }),
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: msg["stopReason"].as_str().map(str::to_string),
                    error_message: msg["errorMessage"].as_str().map(str::to_string),
                    model: msg["model"].as_str().map(str::to_string),
                    custom_type: None,
                    custom_display: true,
                    details: None,
                });
            }
            "toolResult" => {
                let (text, images) = result_payload(&blocks);
                let is_error = msg["isError"].as_bool().unwrap_or(false);
                let tcid = msg["toolCallId"].as_str();
                let details = msg["details"].as_object().map(|_| msg["details"].clone());
                if let Some(m) = self.messages.last_mut() {
                    merge_tool_result(m, tcid, is_error, &text, images, details, ts);
                }
            }
            "custom" => {
                self.messages.push(Msg {
                    role: Role::Custom,
                    bash: None,
                    blocks,
                    usage: None,
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: Some(msg["customType"].as_str().unwrap_or("").to_string()),
                    custom_display: msg["display"].as_bool().unwrap_or(true),
                    details: msg["details"].as_object().map(|_| msg["details"].clone()),
                    model: None,
                });
            }
            // `!` 执行记录：get_messages 快照（agent 投影）不携带，快照里
            // 出现只可能是未来 pi 行为——照磁盘口径转成 Role::Bash
            "bashExecution" => {
                self.messages.push(Msg {
                    role: Role::Bash,
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
                    bash: Some(BashInfo {
                        command: msg["command"].as_str().unwrap_or("").to_string(),
                        output: msg["output"].as_str().unwrap_or("").to_string(),
                        exit_code: msg["exitCode"].as_i64(),
                        cancelled: msg["cancelled"].as_bool().unwrap_or(false),
                        truncated: msg["truncated"].as_bool().unwrap_or(false),
                        full_output_path: msg["fullOutputPath"].as_str().map(str::to_string),
                        excluded: msg["excludeFromContext"].as_bool().unwrap_or(false),
                        running: false,
                    }),
                });
            }
            _ => {}
        }
        self.notify_list(cx);
    }

    fn last_assistant(&mut self) -> &mut Msg {
        if !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant) {
            // 流已切断（abort 在途）：管道残包属于**旧轮**，最后一条消息往往
            // 已是乐观追加的打断气泡——照常兜底推壳会让旧内容长在气泡后面，
            // 气泡视觉上跳到 agent 输出上方。并入最后一条既有 assistant
            // （TextEnd/ThinkingEnd 携带全文，正好把被截断的旧回复补完整）；
            // 一个 assistant 都没有（理论边缘）才退回推壳。
            if self.stream_cut
                && let Some(ix) = self
                    .messages
                    .iter()
                    .rposition(|m| m.role == Role::Assistant)
            {
                return &mut self.messages[ix];
            }
            self.messages.push(Msg {
                role: Role::Assistant,
                bash: None,
                blocks: Vec::new(),
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
            });
        }
        self.messages.last_mut().expect("just pushed")
    }

    pub(super) fn assistant_slot(&mut self, content_index: usize, fresh: Block) -> &mut Block {
        let msg = self.last_assistant();
        while msg.blocks.len() <= content_index {
            let pad = msg.blocks.len();
            msg.blocks.push(Block::Text {
                content_index: pad,
                text: String::new(),
            });
        }
        let same_kind = match (&msg.blocks[content_index], &fresh) {
            (Block::Text { .. }, Block::Text { .. })
            | (Block::Thinking { .. }, Block::Thinking { .. })
            | (Block::ToolCall { .. }, Block::ToolCall { .. }) => true,
            _ => false,
        };
        if !same_kind {
            msg.blocks[content_index] = fresh;
        }
        &mut msg.blocks[content_index]
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
}
