//! 子代理派发账本（**内存态**，符合 `pf-context::state` 契约：内存为主、
//! 不落盘、丢了大不了重建）。
//!
//! 为什么需要这一层：派发入口只有一个（扩展 pf-subagents 的 `agent` 工具
//! → `SubagentManager`），但调用它的人有两种——模型直接调，或 codemode
//! 脚本里的 `tools.agent`。后者在 transcript 里只留一个 `codemode` 块
//! （内层调用仅存进父块的 `nestedCalls`，且有损），**扫消息永远看不见**：
//! 2026-11-08 实测一轮派 3 个、点阵 0 个；而更根本的病是渲染层在每次重建
//! 时全量重扫 transcript 反推状态，没有会话状态层。
//!
//! 真源是 pi 进程里的 `SubagentManager.descriptors()`；宿主侧只保留它的
//! 内存副本，靠 pi 的事件流增量喂（两条 `tool_execution_*` 对两种调用者
//! 一视同仁）：
//! - `tool_execution_start{toolCallId, toolName, parentToolCallId}` —— 派发
//! - `tool_execution_end{result.details: child_id / stop_reason}` —— 终态
//! - `subagent-settled` 条目 / `subagent-completion` 消息 —— 按 child_id
//!   补终态（后台派发的 toolResult 只是 ack）
//!
//! 会话重开（`project_rpc_messages` 整表替换）时用 [`SubagentLedger::reset_and_seed`]
//! 从快照一次性重建：历史轮次只能从 transcript 的 `agent` 块恢复（嵌套
//! 调用留不下块，历史点无法回放——本会话内实时路径不受影响）。

use std::collections::HashMap;

use crate::session::messages::Msg;
use crate::session::subagent_panel::DotTone;

/// 扩展 pf-subagents 注册的派发工具名（`extensions/pf-subagents/src/index.ts` `TOOL_NAME`）。
pub(crate) const AGENT_TOOL: &str = "agent";

/// 子会话终态（与扩展 `SubagentResult.stopReason` 同口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildStatus {
    Running,
    Completed,
    Aborted,
    MaxTurns,
    Error,
    /// 被拒（并发满/参数/深度）：**根本没派出去**，点阵不画（点 = 子代理
    /// 数，不是调用数；`CAPACITY_REACHED` 此前会被画成红点）。
    Rejected,
}

impl ChildStatus {
    pub(crate) fn tone(self) -> DotTone {
        match self {
            ChildStatus::Running => DotTone::Muted,
            ChildStatus::Completed => DotTone::Notice,
            ChildStatus::Aborted | ChildStatus::MaxTurns => DotTone::Warn,
            ChildStatus::Error => DotTone::Danger,
            // 被 [`ChildStatus::drawn`] 挡掉，走到这里只作兜底
            ChildStatus::Rejected => DotTone::Muted,
        }
    }

    /// 是否占一个点（被拒不占）。
    fn drawn(self) -> bool {
        !matches!(self, ChildStatus::Rejected)
    }

    pub(crate) fn from_stop(stop: &str) -> Self {
        match stop {
            "completed" => ChildStatus::Completed,
            "aborted" => ChildStatus::Aborted,
            "max_turns" => ChildStatus::MaxTurns,
            // 未知终态按异常红处理（与会话列表状态槽同口径）
            _ => ChildStatus::Error,
        }
    }

    fn from_tone(tone: DotTone) -> Self {
        match tone {
            DotTone::Muted => ChildStatus::Running,
            DotTone::Notice => ChildStatus::Completed,
            DotTone::Warn => ChildStatus::Aborted,
            DotTone::Danger => ChildStatus::Error,
        }
    }
}

/// 一次派发的记录。`tool_call_id` 是 pi 的调用 id：直调 = `agent` 调用 id，
/// 嵌套 = `"<codemode 调用 id>/N"`（`NestedToolCallRunner` 的构造法）。
#[derive(Debug, Clone)]
pub(crate) struct ChildRecord {
    /// 嵌套派发时指向发起它的工具调用（codemode）；`None` = 模型直调。
    pub parent_tool_call_id: Option<String>,
    /// 派发时刻的轮序（第 n 条 User 消息 = 第 n 轮，0 基）——**定格**在派发
    /// 那一刻，不随之后到达的消息改写（旧投影实现的差一错根源）。
    pub turn: usize,
    pub label: Option<String>,
    pub profile: Option<String>,
    pub child_id: Option<String>,
    pub status: ChildStatus,
}

#[derive(Debug, Default)]
pub(crate) struct SubagentLedger {
    /// 派发顺序（去重后的 `tool_call_id`）；点阵按它取记录
    order: Vec<String>,
    by_call: HashMap<String, ChildRecord>,
}

impl SubagentLedger {
    /// 派发（`tool_execution_start`）。非派发工具直接忽略，返回 false。
    pub(crate) fn start(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        parent_tool_call_id: Option<&str>,
        turn: usize,
        args: Option<&serde_json::Value>,
    ) -> bool {
        if tool_name != AGENT_TOOL || tool_call_id.is_empty() {
            return false;
        }
        if let Some(rec) = self.by_call.get_mut(tool_call_id) {
            // 重放 / 与快照种子撞车：只补派发面（轮序以种子为准）
            rec.parent_tool_call_id = parent_tool_call_id.map(str::to_string);
            rec.label = args
                .and_then(|a| a.get("label"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            rec.profile = args
                .and_then(|a| a.get("profile"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            return true;
        }
        self.order.push(tool_call_id.to_string());
        self.by_call.insert(
            tool_call_id.to_string(),
            ChildRecord {
                parent_tool_call_id: parent_tool_call_id.map(str::to_string),
                turn,
                label: args
                    .and_then(|a| a.get("label"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                profile: args
                    .and_then(|a| a.get("profile"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                child_id: None,
                status: ChildStatus::Running,
            },
        );
        true
    }

    /// 收尾（`tool_execution_end`）。`fallback_turn` 用于没有派发记录就收到
    /// 终态的情形（快照重建后仍在飞的子代理）。
    pub(crate) fn finish(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        is_error: bool,
        details: Option<&serde_json::Value>,
        fallback_turn: usize,
    ) -> bool {
        if tool_name != AGENT_TOOL || tool_call_id.is_empty() {
            return false;
        }
        let child_id = details
            .and_then(|d| d.get("child_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let stop = details
            .and_then(|d| d.get("stop_reason"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let detail_status = details
            .and_then(|d| d.get("status"))
            .and_then(|v| v.as_str());
        let status = match (stop.as_deref(), is_error, detail_status) {
            (Some(s), _, _) => ChildStatus::from_stop(s),
            // 后台派发（`delivery: parent`）的 toolResult 只是回执：
            // `status=started/running` 不是终态，终态另走 subagent-settled /
            // completion 消息（按 child_id 归位）
            (None, _, Some("started" | "running")) => ChildStatus::Running,
            // 没有终态字段的失败且没拿到 child_id = 准入就被拒（capacity /
            // 参数 / 深度），子代理从未存在
            (None, true, _) if child_id.is_none() => ChildStatus::Rejected,
            (None, true, _) => ChildStatus::Error,
            // 有的后端/前台派发只回结果不带 stop_reason：按成功算
            (None, false, _) => ChildStatus::Completed,
        };
        match self.by_call.get_mut(tool_call_id) {
            Some(rec) => {
                if child_id.is_some() {
                    rec.child_id = child_id;
                }
                rec.status = status;
            }
            None => {
                self.order.push(tool_call_id.to_string());
                self.by_call.insert(
                    tool_call_id.to_string(),
                    ChildRecord {
                        parent_tool_call_id: None,
                        turn: fallback_turn,
                        label: None,
                        profile: None,
                        child_id,
                        status,
                    },
                );
            }
        }
        true
    }

    /// 运行中状态回填（`tool_execution_update.partialResult.details`，前台
    /// 派发先给 `status=running` + `child_id`）——只补 child_id，不动终态。
    pub(crate) fn touch(&mut self, tool_call_id: &str, details: &serde_json::Value) -> bool {
        let Some(cid) = details.get("child_id").and_then(|v| v.as_str()) else {
            return false;
        };
        let Some(rec) = self.by_call.get_mut(tool_call_id) else {
            return false;
        };
        if rec.child_id.as_deref() == Some(cid) {
            return false;
        }
        rec.child_id = Some(cid.to_string());
        true
    }

    /// 按 `child_id` 补终态（`subagent-settled` 条目 / `subagent-completion`
    /// 消息）：后台派发的 toolResult 只是 ack，终态走这两条。
    pub(crate) fn settle(&mut self, child_id: &str, stop: Option<&str>) -> bool {
        let Some(stop) = stop else { return false };
        let status = ChildStatus::from_stop(stop);
        for rec in self.by_call.values_mut() {
            if rec.child_id.as_deref() == Some(child_id) {
                if rec.status == status {
                    return false;
                }
                rec.status = status;
                return true;
            }
        }
        false
    }

    /// 快照重建：清空后从 transcript 的 `agent` 块回放（历史轮次）。嵌套
    /// 派发在 transcript 里没有块，历史点无法回放（本会话实时路径不受影响）。
    pub(crate) fn reset_and_seed(&mut self, msgs: &[Msg]) {
        self.order.clear();
        self.by_call.clear();
        for (turn, calls) in crate::session::subagent_panel::project_turn_agent_calls(msgs)
            .into_iter()
            .enumerate()
        {
            for (tool_call_id, tone) in calls {
                if tool_call_id.is_empty() {
                    continue;
                }
                self.order.push(tool_call_id.clone());
                self.by_call.insert(
                    tool_call_id.clone(),
                    ChildRecord {
                        parent_tool_call_id: None,
                        turn,
                        label: None,
                        profile: None,
                        child_id: None,
                        status: ChildStatus::from_tone(tone),
                    },
                );
            }
        }
    }

    /// 按轮投影：第 n 项 = 第 n 轮的点（被拒的调用不占点；末轮无点不补空槽）。
    pub(crate) fn turn_tones(&self) -> Vec<Vec<DotTone>> {
        let mut out: Vec<Vec<DotTone>> = Vec::new();
        for id in &self.order {
            let Some(rec) = self.by_call.get(id) else {
                continue;
            };
            if !rec.status.drawn() {
                continue;
            }
            while out.len() <= rec.turn {
                out.push(Vec::new());
            }
            out[rec.turn].push(rec.status.tone());
        }
        out
    }

    /// 运行中的子代理数（`pf-context::state::StateSnapshot::running_subagents`
    /// 的喂值点；state store 目前尚未接入 app，接线时从这里取）。
    #[allow(dead_code)]
    pub(crate) fn running_count(&self) -> u32 {
        self.by_call
            .values()
            .filter(|r| matches!(r.status, ChildStatus::Running))
            .count() as u32
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.order.len()
    }
}

#[cfg(test)]
#[path = "subagents_tests.rs"]
mod tests;
