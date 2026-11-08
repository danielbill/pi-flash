//! 消息模型层：Role/Msg/MsgMeta 类型、toolResult 合并、tail 窗口解码
//! （msgs_from_tail）与元数据计算。纯数据 + 纯函数，无渲染。

use pi_link::protocol::{Block, Usage, content_blocks};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Role {
    User,
    Assistant,
    /// pi CustomMessage (compaction summary, extension messages,
    /// branch summaries) — v56-6 renders the card; skipped until then
    Custom,
    /// 用户 `!` / `!!` 发起的 shell 命令执行（pi `role:"bashExecution"`）。
    /// 独立角色而非 assistant 消息：不进轮分组、不计 nav 统计；pi 侧它
    /// 由 convertToLlm 在下一次 prompt 时折叠为 user 文本
    Bash,
}

/// 一条 shell 命令执行（Msg.bash；pi BashExecutionMessage 同名字段）。
/// 流式阶段 output 逐步追加（bash_execution_update），final 由 bash response
/// 回填（exit_code/cancelled/truncated/full_output_path）。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct BashInfo {
    pub(crate) command: String,
    pub(crate) output: String,
    pub(crate) exit_code: Option<i64>,
    pub(crate) cancelled: bool,
    pub(crate) truncated: bool,
    pub(crate) full_output_path: Option<String>,
    /// `!!` 前缀：会话有记录、模型看不到（pi excludeFromContext）
    pub(crate) excluded: bool,
    /// true = 乐观插入的执行中卡片（response 回填后翻 false）
    pub(crate) running: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct UsageLine {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
    pub(crate) cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Msg {
    pub(crate) role: Role,
    pub(crate) blocks: Vec<Block>,
    pub(crate) usage: Option<UsageLine>,
    /// session entry id for user messages (fork anchor), filled from get_entries
    pub(crate) entry_id: Option<String>,
    /// wall-clock stamp, epoch ms (032 发送时间/回复时间): user = send time,
    /// assistant = generation end; from pi message payloads / session entries
    pub(crate) ts: Option<i64>,
    /// completion stamp (回复用时 anchor): pi's message-object timestamp is
    /// the START of generation (≈ the user's send time — real files show a
    /// ~300ms delta), so duration needs the entry write-time (tails) or the
    /// MessageEnd arrival (live). None in get_messages snapshots.
    pub(crate) end_ts: Option<i64>,
    /// AssistantMessage.stopReason (v56 error/length alert boxes)
    pub(crate) stop_reason: Option<String>,
    /// AssistantMessage.errorMessage — set when stop_reason == "error"
    pub(crate) error_message: Option<String>,
    /// CustomMessage.customType (None for user/assistant)
    pub(crate) custom_type: Option<String>,
    /// CustomMessage.display — false = hidden extension payload
    pub(crate) custom_display: bool,
    /// CustomMessage.details (compaction: tokensBefore/firstKeptEntryId)
    pub(crate) details: Option<serde_json::Value>,
    /// AssistantMessage.model — per-message label (pi-web getModelDisplayName
    /// source); None on live-streamed messages until MessageEnd
    pub(crate) model: Option<String>,
    /// Role::Bash 专属（其余角色恒 None）
    pub(crate) bash: Option<BashInfo>,
}

/// Per-message display metadata computed by the list owner (needs whole-list
/// lookarounds): pi-web showTimestamp rule + the turn's user stamp for
/// 回复用时.
#[derive(Clone, Copy, Default)]
pub(crate) struct MsgMeta {
    pub(crate) show_ts: bool,
    pub(crate) turn_user_ts: Option<i64>,
}

/// Decoded image payload from a toolResult content array (v56-0 c3).
pub(crate) fn result_payload(blocks: &[Block]) -> (String, Vec<pi_link::protocol::ImageData>) {
    use pi_link::protocol::ImageData;
    let mut text = String::new();
    let mut images = Vec::new();
    for b in blocks {
        match b {
            Block::Text { text: t, .. } => text.push_str(t),
            Block::Image { mime, data, .. } => images.push(ImageData {
                mime: mime.clone(),
                data: data.clone(),
            }),
            _ => {}
        }
    }
    (text.trim_end().to_string(), images)
}

/// `agent` 工具的 details 是否已到终态。
///
/// 前台派发（`delivery: result`）的终态**只在** toolResult 的 details 里：
/// `stop_reason`（completed / max_turns / aborted）或 `error_code`（派发失败）。
/// 后台派发的 ack 形如 `{child_id, status: "started", delivery: "parent"}`，
/// 终态另走 completion 消息 / `subagent-settled` 条目——ack 不能算「结果已到」，
/// 否则卡片会在两个通道之间闪一下红色「执行异常」。
fn agent_details_terminal(d: Option<&serde_json::Value>) -> bool {
    let Some(d) = d else { return false };
    d.get("stop_reason").is_some()
        || d.get("error_code").is_some()
        || matches!(
            d.get("status").and_then(|v| v.as_str()),
            Some("failed" | "completed" | "done" | "aborted" | "error")
        )
}

/// Merge a toolResult payload into the paired ToolCall of `msg` (must be the
/// last assistant message). Correlates by toolCallId; blind-fallback to the
/// last call keeps old snapshots without ids working.
pub(crate) fn merge_tool_result(
    msg: &mut Msg,
    tool_call_id: Option<&str>,
    is_error: bool,
    text: &str,
    images: Vec<pi_link::protocol::ImageData>,
    details: Option<serde_json::Value>,
    result_ts: Option<i64>,
) {
    if msg.role != Role::Assistant {
        return;
    }
    // two-pass by index: rposition's immutable borrow ends before get_mut
    let ix = msg
        .blocks
        .iter()
        .rposition(|b| matches!(b, Block::ToolCall { id, .. } if tool_call_id == Some(id.as_str())))
        .or_else(|| {
            msg.blocks
                .iter()
                .rposition(|b| matches!(b, Block::ToolCall { .. }))
        });
    // pi-web {n}s duration = result arrival − message start (rounded s)
    let duration_s = match (result_ts, msg.ts) {
        (Some(r), Some(start)) => Some(((r - start).max(0)) / 1000),
        _ => None,
    };
    if let Some(ix) = ix {
        if let Some(Block::ToolCall {
            name,
            result,
            is_error: err,
            images: imgs,
            duration_s: dur,
            details: det,
            result_arrived,
            ..
        }) = msg.blocks.get_mut(ix)
        {
            result.push_str(text);
            *err = is_error;
            imgs.extend(images);
            *det = details;
            *dur = duration_s;
            // 结果到达 = 卡片离开「运行中」（2026-11-08 用户报障：前台子代理
            // 完成/轮次用尽后卡片永远转圈、点阵却已上色——因为点阵读 details、
            // 卡片读本标记，而后台通道之外的 toolResult 从来没人置它）。
            // `agent` 卡只认 details 契约（或错误）；其余工具的结果即终态。
            if !*result_arrived
                && (is_error || *name != "agent" || agent_details_terminal(det.as_ref()))
            {
                *result_arrived = true;
            }
        }
    }
}

/// Merge a tool_execution_update partial into the paired ToolCall of `msg`
/// （044-0 B3②：委派被拒的失败 details 即时上卡，不等整轮 toolResult）。
/// 只替换 details、不动 result/result_arrived——正文仍走 toolResult 主通道，
/// 终态 details 由 merge_tool_result 覆盖。按 id 精确匹配，匹配不到就丢弃
/// （update 不做盲回退：写错块比丢一条分片糟）。
pub(crate) fn merge_tool_update(msg: &mut Msg, tool_call_id: &str, details: serde_json::Value) {
    if msg.role != Role::Assistant {
        return;
    }
    if let Some(ix) = msg
        .blocks
        .iter()
        .rposition(|b| matches!(b, Block::ToolCall { id, .. } if id == tool_call_id))
    {
        if let Some(Block::ToolCall { details: det, .. }) = msg.blocks.get_mut(ix) {
            *det = Some(details);
        }
    }
}

/// 044-0 B4：子代理终态来源归一——completion custom 消息与 settled 自定义
/// 条目（扩展 `SETTLED_ENTRY_TYPE`）都带同一套扁平 details（childId /
/// stopReason / errorCode? / session_file? / durationMs?）；这里折成「卡片
/// details 的终态合并键」。返回 `(child_id, 终态字段)`。
pub(crate) fn settled_patch_fields(
    details: &serde_json::Value,
) -> Option<(String, serde_json::Value)> {
    let child_id = details.get("childId").and_then(|v| v.as_str())?;
    let stop = details.get("stopReason").and_then(|v| v.as_str())?;
    let mut merged = serde_json::Map::new();
    merged.insert("status".into(), stop.into());
    merged.insert("stop_reason".into(), stop.into());
    if let Some(v) = details.get("errorCode") {
        merged.insert("error_code".into(), v.clone());
    }
    if let Some(v) = details.get("session_file") {
        merged.insert("session_file".into(), v.clone());
    }
    Some((child_id.to_string(), serde_json::Value::Object(merged)))
}

/// 把终态字段并入 `details.child_id` 匹配的派发 ToolCall 并置
/// `result_arrived`——卡片从「运行中」即刻切到任务完成/执行异常。整表扫描：
/// 一次结算只发生一次，数百条消息也是微秒级（无需 panel 索引）。幂等：
/// 重复应用值相同返回 false（调用方据此决定是否 notify）。
pub(crate) fn apply_child_settled(
    msgs: &mut [Msg],
    child_id: &str,
    fields: &serde_json::Value,
) -> bool {
    let serde_json::Value::Object(fields) = fields else {
        return false;
    };
    let mut changed = false;
    for m in msgs.iter_mut() {
        for b in m.blocks.iter_mut() {
            let Block::ToolCall {
                name,
                details: slot,
                result_arrived,
                ..
            } = b
            else {
                continue;
            };
            if name != "agent" {
                continue;
            }
            let Some(serde_json::Value::Object(cur)) = slot.as_ref() else {
                continue;
            };
            if cur.get("child_id").and_then(|v| v.as_str()) != Some(child_id) {
                continue;
            }
            let map = slot.get_or_insert_with(|| serde_json::Value::Object(Default::default()));
            if let serde_json::Value::Object(map) = map {
                for (k, v) in fields {
                    if map.get(k) != Some(v) {
                        map.insert(k.clone(), v.clone());
                        changed = true;
                    }
                }
            }
            if !*result_arrived {
                *result_arrived = true;
                changed = true;
            }
        }
    }
    changed
}

/// 一并归一磁盘上全部 `subagent-completion` 终态（装载 / 即到即用）。
///
/// 设计判据（2026-11-08 用户定夺）：**一个子会话只有一个状态**——终态在
/// 消息流里只此一份（派发卡 details 落盘永远停在 started），所以进内存时
/// 一次性折进卡片，渲染层（卡片 spinner、导航第三行点）看到的从来是同一
/// 个状态，不存在「先运行中、后补终态」的两段式画面。幂等：重复应用零
/// 变更、零 notify。
///
/// 只认 `customType == subagent-completion`；`settled` 自定义条目走
/// [`apply_child_settled`] 的事件路径，不在此重复（两者字段可重叠）。
pub(crate) fn apply_completion_messages(msgs: &mut [Msg]) -> bool {
    let patches: Vec<(String, serde_json::Value)> = msgs
        .iter()
        .filter(|m| m.role == Role::Custom && m.custom_type.as_deref() == Some(COMPLETION_CUSTOM))
        .filter_map(|m| m.details.as_ref().and_then(settled_patch_fields))
        .collect();
    let mut changed = false;
    for (cid, fields) in &patches {
        changed |= apply_child_settled(msgs, cid, fields);
    }
    changed
}

/// 子代理终态 custom 消息类型（扩展 completion.ts 落盘）。
pub(crate) const COMPLETION_CUSTOM: &str = "subagent-completion";

impl Msg {
    /// 返回 Cow：单文本块（绝大多数用户消息）零拷贝借用。每帧每个可见
    /// 用户行都要调（气泡 markdown/技能折叠判定），长粘贴消息的全量 String
    /// 拷贝是白付的打字延迟。
    pub(crate) fn plain_text(&self) -> std::borrow::Cow<'_, str> {
        match self.blocks.as_slice() {
            [Block::Text { text, .. }] => std::borrow::Cow::Borrowed(text.as_str()),
            blocks => {
                let joined = blocks
                    .iter()
                    .map(|b| match b {
                        Block::Text { text, .. } => text.as_str(),
                        _ => "",
                    })
                    .collect::<Vec<_>>()
                    .join("");
                std::borrow::Cow::Owned(joined)
            }
        }
    }

    /// 文本总字节数（Text 块长度和）：回显去重的廉价预筛——先比长度再
    /// 全量 plain_text，避免在几百条消息上逐条全量拼 String（大粘贴会话
    /// 里这是发送路径上秒级的主线程冻结源）。
    pub(crate) fn text_len(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Text { text, .. } => text.len(),
                _ => 0,
            })
            .sum()
    }

    /// 文本前缀（导航面板摘要专用）：拼够 max_chars 个字符即停，避免为
    /// 截断摘要把整条长回复全量拼接一遍（nav 每帧都取，长程任务会话里
    /// 全量 join 的成本随回复长度线性涨）。换行拍平成空格：摘要靠
    /// line_clamp(3) 限三行，而 gpui line_clamp 只限软换行，硬 \n 会
    /// 绕过钳制（slash 菜单 desc 同款处理）
    pub(crate) fn plain_text_prefix(&self, max_chars: usize) -> String {
        let mut out = String::new();
        let mut taken = 0usize;
        for b in &self.blocks {
            if let Block::Text { text, .. } = b {
                for ch in text.chars() {
                    if taken >= max_chars {
                        return out;
                    }
                    out.push(if ch == '\n' || ch == '\r' { ' ' } else { ch });
                    taken += 1;
                }
            }
        }
        out
    }
}

/// Per-message display metadata (pi-web ChatWindow renderMessage parity):
/// assistant rows show the reply-time meta only on the last one of an
/// exchange (no later assistant before the next user); every row's
/// 回复用时 anchor is the nearest prior user stamp.
pub(crate) fn compute_meta(messages: &[Msg], ix: usize) -> MsgMeta {
    let mut meta = MsgMeta::default();
    let Some(m) = messages.get(ix) else {
        return meta;
    };
    meta.show_ts = if m.role == Role::User {
        true
    } else {
        messages[ix + 1..]
            .iter()
            .all(|later| later.role != Role::Assistant)
    };
    for prev in messages[..ix].iter().rev() {
        if prev.role == Role::User {
            meta.turn_user_ts = prev.ts;
            break;
        }
    }
    meta
}

/// Convert raw session-file tail entries (`{"type":"message","message":{…}}`,
/// as produced by `pi_link::sessions::read_tail_messages`) into renderable
/// `Msg`s. Mirrors `Chat::ingest_message` semantics exactly: user/assistant
/// push, toolResult merges into the last assistant's tool call, other roles
/// skipped. Timestamps: the message object's epoch-ms `timestamp`, falling
/// back to the entry's ISO `timestamp` (032 发送时间).
/// pi 冷启动横幅（「🔌 MCP: 1 server enabled」）：进程每次冷启动都会作为
/// **独立 assistant 消息**发进会话流并落盘，渲染成聊天气泡毫无意义
/// （2026-10-25 定夺：识别口径收紧——「含 mcp: 且 enabled 结尾」，
/// 避免误杀真在聊 MCP 的长回复）。
pub(crate) fn is_mcp_banner(s: &str) -> bool {
    let low = s.trim().to_lowercase();
    low.contains("mcp:") && low.ends_with("enabled")
}

/// assistant 消息是否纯横幅（所有非空 text 块都是横幅、且无别的块型）。
pub(crate) fn msg_is_mcp_banner(m: &Msg) -> bool {
    let mut saw_text = false;
    for b in &m.blocks {
        match b {
            Block::Text { text, .. } if !text.trim().is_empty() => {
                if !is_mcp_banner(text) {
                    return false;
                }
                saw_text = true;
            }
            Block::Text { .. } => {}
            _ => return false,
        }
    }
    saw_text
}

pub(crate) fn msgs_from_tail(values: Vec<serde_json::Value>) -> Vec<Msg> {
    let mut out: Vec<Msg> = Vec::new();
    for v in values {
        let m = &v["message"];
        let role = m["role"].as_str().unwrap_or("");
        let blocks = content_blocks(&m["content"]);
        let usage = Usage::parse(&m["usage"]);
        let ts = m["timestamp"]
            .as_i64()
            .or_else(|| parse_iso_ms(&v["timestamp"]));
        let entry_ts = parse_iso_ms(&v["timestamp"]);
        match role {
            "user" => out.push(Msg {
                role: Role::User,
                bash: None,
                blocks,
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
            }),
            "assistant" => out.push(Msg {
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
                end_ts: entry_ts.or(ts),
                stop_reason: m["stopReason"].as_str().map(str::to_string),
                error_message: m["errorMessage"].as_str().map(str::to_string),
                model: m["model"].as_str().map(str::to_string),
                custom_type: None,
                custom_display: true,
                details: None,
            }),
            "custom" if m["customType"].as_str() == Some(crate::mode::CONTEXT_CUSTOM_TYPE) => {
                // 031-1：计划提醒是内部指令，磁盘尾读也不进 transcript
                //（ingest/stream 两路径同款过滤）
            }
            "custom" => out.push(Msg {
                role: Role::Custom,
                bash: None,
                blocks,
                usage: None,
                entry_id: None,
                ts,
                end_ts: None,
                stop_reason: None,
                error_message: None,
                custom_type: Some(m["customType"].as_str().unwrap_or("").to_string()),
                custom_display: m["display"].as_bool().unwrap_or(true),
                details: m["details"].as_object().map(|_| m["details"].clone()),
                model: None,
            }),
            // `!` 执行记录（pi role:"bashExecution"；会话文件里的 message 条目。
            // get_messages 快照不带它——磁盘重读是唯一来源）
            "bashExecution" => out.push(Msg {
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
                    command: m["command"].as_str().unwrap_or("").to_string(),
                    output: m["output"].as_str().unwrap_or("").to_string(),
                    exit_code: m["exitCode"].as_i64(),
                    cancelled: m["cancelled"].as_bool().unwrap_or(false),
                    truncated: m["truncated"].as_bool().unwrap_or(false),
                    full_output_path: m["fullOutputPath"].as_str().map(str::to_string),
                    excluded: m["excludeFromContext"].as_bool().unwrap_or(false),
                    running: false,
                }),
            }),
            "toolResult" => {
                let (text, images) = result_payload(&blocks);
                let is_error = m["isError"].as_bool().unwrap_or(false);
                let tcid = m["toolCallId"].as_str();
                let details = m["details"].as_object().map(|_| m["details"].clone());
                if let Some(last) = out.last_mut() {
                    merge_tool_result(last, tcid, is_error, &text, images, details, ts);
                }
            }
            _ => {}
        }
    }
    // 历史里的纯横幅 assistant 消息（pi 每次冷启动都会落盘一条）不渲染
    out.retain(|m| !(m.role == Role::Assistant && msg_is_mcp_banner(m)));
    // 磁盘口径一次性归一：派发卡落盘永远停在 started，终态只在
    // `subagent-completion` 消息里——所有磁盘链路（秒开 tail、观察窗、
    // 外部追加对账、leaf 修复）都在这里收口，首帧即终态，没有两段式。
    apply_completion_messages(&mut out);
    out
}

/// RFC3339 entry stamp -> epoch ms (session entries carry ISO timestamps).
fn parse_iso_ms(v: &serde_json::Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tail_tests;

#[cfg(test)]
mod tool_update_tests;

#[cfg(test)]
mod settled_tests;
