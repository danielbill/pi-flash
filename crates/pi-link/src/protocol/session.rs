//! Session snapshot types: models, branch tree/flat entries, `get_state` /
//! `get_session_stats` payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Parse `get_available_models` data: {"models": [...]}
pub fn parse_model_list(data: &Value) -> Vec<ModelInfo> {
    data["models"]
        .as_array()
        .map(|arr| arr.iter().filter_map(ModelInfo::parse).collect())
        .unwrap_or_default()
}

/// Parse a single model object (the `set_model` response carries the
/// swapped-in model).
pub fn parse_model_info(v: &Value) -> Option<ModelInfo> {
    ModelInfo::parse(v)
}

/// A node of the session branch tree (`get_tree` response).
/// Mirrors pi's SessionTreeNode: entry identity + role/text for message
/// entries, plus recursive children. Text is truncated to 80 chars because
/// the UI only ever shows a bounded preview.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub entry_type: String,
    /// "user" | "assistant" | "toolResult" | "system" for message entries
    pub role: Option<String>,
    /// bounded message text preview (80 chars)
    pub text: Option<String>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    fn parse(v: &Value) -> Option<TreeNode> {
        let entry = &v["entry"];
        let mut node = TreeNode {
            id: entry["id"].as_str()?.to_string(),
            parent_id: entry["parentId"].as_str().map(str::to_string),
            entry_type: entry["type"].as_str().unwrap_or("").to_string(),
            role: entry["message"]["role"].as_str().map(str::to_string),
            text: None,
            children: v["children"]
                .as_array()
                .map(|a| a.iter().filter_map(TreeNode::parse).collect())
                .unwrap_or_default(),
        };
        if node.entry_type == "message" {
            node.text = Some(extract_message_preview(&entry["message"]["content"]));
        }
        Some(node)
    }

    /// First user-message entry id on this node or its single-child chain
    /// (pi fork position "before" requires a user message entry).
    pub fn forkable_entry_id(&self) -> Option<&str> {
        let mut cur = Some(self);
        while let Some(n) = cur {
            if n.role.as_deref() == Some("user") {
                return Some(&n.id);
            }
            cur = n.children.first();
        }
        None
    }
}

/// Extract a bounded text preview from message content (string or blocks).
fn extract_message_preview(content: &Value) -> String {
    let mut text = String::new();
    if let Some(s) = content.as_str() {
        text = s.to_string();
    } else if let Some(arr) = content.as_array() {
        for b in arr {
            if b["type"].as_str() == Some("text") {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(b["text"].as_str().unwrap_or(""));
            }
        }
    }
    text.chars().take(80).collect()
}

/// Parse a `get_tree` response: {"tree": [...], "leafId": "..."|null}
pub fn parse_tree(data: &Value) -> (Vec<TreeNode>, Option<String>) {
    let tree = data["tree"]
        .as_array()
        .map(|a| a.iter().filter_map(TreeNode::parse).collect())
        .unwrap_or_default();
    let leaf_id = data["leafId"].as_str().map(str::to_string);
    (tree, leaf_id)
}

/// Final result of the rpc `bash` command (pi `BashResult`, bash-executor.d.ts).
/// Streaming deltas arrive earlier as [`Event::BashExecutionUpdate`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BashResult {
    /// Combined stdout+stderr output (sanitized, possibly truncated)
    pub output: String,
    /// Process exit code (None if killed/cancelled)
    pub exit_code: Option<i64>,
    pub cancelled: bool,
    pub truncated: bool,
    /// Temp file with the full output (set when output exceeded pi's cap)
    pub full_output_path: Option<String>,
}

impl BashResult {
    pub fn parse(v: &Value) -> BashResult {
        BashResult {
            output: v["output"].as_str().unwrap_or("").to_string(),
            exit_code: v["exitCode"].as_i64(),
            cancelled: v["cancelled"].as_bool().unwrap_or(false),
            truncated: v["truncated"].as_bool().unwrap_or(false),
            full_output_path: v["fullOutputPath"].as_str().map(str::to_string),
        }
    }
}

/// One session entry as `get_entries` returns it: flat, parentId-linked.
/// Only the fields the client acts on are lifted out (fork anchors need
/// id/parentId/type/role); the whole entry stays in `raw` for callers that
/// want the original (nav panel previews).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub entry_type: String,
    /// "user" | "assistant" | "toolResult" | "system" for message entries
    pub role: Option<String>,
    pub timestamp: Option<String>,
    pub raw: Value,
}

impl SessionEntry {
    fn parse(v: &Value) -> Option<SessionEntry> {
        Some(SessionEntry {
            id: v["id"].as_str()?.to_string(),
            parent_id: v["parentId"].as_str().map(str::to_string),
            entry_type: v["type"].as_str().unwrap_or("").to_string(),
            role: v["message"]["role"].as_str().map(str::to_string),
            timestamp: v["timestamp"].as_str().map(str::to_string),
            raw: v.clone(),
        })
    }
}

/// Parse a `get_entries` response: {"entries": [...], "leafId": "..."|null}
pub fn parse_entries(data: &Value) -> (Vec<SessionEntry>, Option<String>) {
    let entries = data["entries"]
        .as_array()
        .map(|a| a.iter().filter_map(SessionEntry::parse).collect())
        .unwrap_or_default();
    let leaf_id = data["leafId"].as_str().map(str::to_string);
    (entries, leaf_id)
}

/// Entry ids of the user messages on the active chain (leaf → root walk,
/// returned oldest → newest).
///
/// pi-web parity: `sliceActiveBranch` (lib/session-reader.ts) walks parentId
/// over the flat entry list for exactly this purpose (fork anchors +
/// navigation targets). This replaces the get_tree version, which cannot
/// survive a long session: pi answers deep trees with "Maximum call stack
/// size exceeded", i.e. no anchors at all.
pub fn active_user_entry_ids(entries: &[SessionEntry], leaf_id: Option<&str>) -> Vec<String> {
    let Some(leaf) = leaf_id else {
        return Vec::new();
    };
    let mut by_id: std::collections::HashMap<&str, &SessionEntry> =
        std::collections::HashMap::with_capacity(entries.len());
    for e in entries {
        by_id.insert(e.id.as_str(), e);
    }
    let mut chain: Vec<&SessionEntry> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut cur = by_id.get(leaf).copied();
    // a corrupt parentId cycle must never hang the UI thread (and must not
    // duplicate the entries it loops over)
    while let Some(e) = cur {
        if !seen.insert(e.id.as_str()) {
            break;
        }
        chain.push(e);
        cur = e.parent_id.as_deref().and_then(|p| by_id.get(p).copied());
    }
    chain.reverse();
    chain
        .into_iter()
        .filter(|e| e.entry_type == "message" && e.role.as_deref() == Some("user"))
        .map(|e| e.id.clone())
        .collect()
}

/// Snapshot of `get_state` response data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionState {
    pub model: Option<ModelInfo>,
    pub thinking_level: Option<String>,
    pub is_streaming: bool,
    pub session_name: Option<String>,
    /// pi 当前绑定的会话文件（rpc-types.d.ts RpcSessionState.sessionFile）。
    /// draft 首条 prompt 落盘、fork/clone 换分支都会让它变——shell 的身份
    /// （pool key / 侧栏高亮 / 重开目标）全挂在它上面，runtime 靠它跟随
    /// pi 的进程内重绑定。
    pub session_file: Option<String>,
    pub session_id: Option<String>,
    pub message_count: u64,
    pub pending_message_count: u64,
}

/// 价格四键（$/MTok，042-2）：内置模型详情回填用。缺键 = None（未知）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct CostInfo {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default, rename = "cacheRead")]
    pub cache_read: Option<f64>,
    #[serde(default, rename = "cacheWrite")]
    pub cache_write: Option<f64>,
}

impl CostInfo {
    pub fn parse(v: &Value) -> Option<CostInfo> {
        Some(CostInfo {
            input: v.get("input").and_then(Value::as_f64),
            output: v.get("output").and_then(Value::as_f64),
            cache_read: v.get("cacheRead").and_then(Value::as_f64),
            cache_write: v.get("cacheWrite").and_then(Value::as_f64),
        })
    }
}

/// serde 形状对齐 pi RPC 的 camelCase（`contextWindow`）——registry dump
/// 直接以 `Vec<ModelInfo>` 落 `registry-cache.json`（042 静态目录保底）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default, rename = "contextWindow")]
    pub context_window: Option<u64>,
    /// 042-1：模型详情弹窗要「显示当前生效的参数」，故保留 pi 的完整定义
    /// （RPC 的 get_available_models 与 models-store 都带这几键）
    #[serde(default, rename = "maxTokens")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub input: Option<Vec<String>>,
    /// 价格四键（042-2）：pi 模型定义自带，内置模型详情直接回显
    #[serde(default)]
    pub cost: Option<CostInfo>,
}

impl ModelInfo {
    fn parse(v: &Value) -> Option<ModelInfo> {
        Some(ModelInfo {
            id: v["id"].as_str()?.to_string(),
            name: v["name"].as_str().unwrap_or("").to_string(),
            provider: v["provider"].as_str().unwrap_or("").to_string(),
            context_window: v["contextWindow"].as_u64(),
            max_tokens: v["maxTokens"].as_u64(),
            reasoning: v["reasoning"].as_bool(),
            input: v["input"].as_array().map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            }),
            cost: v.get("cost").and_then(CostInfo::parse),
        })
    }

    pub fn label(&self) -> String {
        if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        }
    }
}

impl SessionState {
    pub fn parse(data: &Value) -> SessionState {
        SessionState {
            model: ModelInfo::parse(&data["model"]),
            thinking_level: data["thinkingLevel"].as_str().map(str::to_string),
            is_streaming: data["isStreaming"].as_bool().unwrap_or(false),
            session_name: data["sessionName"].as_str().map(str::to_string),
            session_file: data["sessionFile"].as_str().map(str::to_string),
            session_id: data["sessionId"].as_str().map(str::to_string),
            message_count: data["messageCount"].as_u64().unwrap_or(0),
            pending_message_count: data["pendingMessageCount"].as_u64().unwrap_or(0),
        }
    }

    pub fn model_label(&self) -> Option<String> {
        self.model.as_ref().map(|m| m.label())
    }
}

/// Snapshot of `get_session_stats` response data (tokens/cost/context).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionStats {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens_total: u64,
    pub cost: f64,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
}

impl SessionStats {
    pub fn parse(data: &Value) -> SessionStats {
        SessionStats {
            input: data["tokens"]["input"].as_u64().unwrap_or(0),
            output: data["tokens"]["output"].as_u64().unwrap_or(0),
            cache_read: data["tokens"]["cacheRead"].as_u64().unwrap_or(0),
            cache_write: data["tokens"]["cacheWrite"].as_u64().unwrap_or(0),
            tokens_total: data["tokens"]["total"].as_u64().unwrap_or(0),
            cost: data["cost"].as_f64().unwrap_or(0.0),
            context_tokens: data["contextUsage"]["tokens"].as_u64(),
            context_window: data["contextUsage"]["contextWindow"].as_u64(),
            context_percent: data["contextUsage"]["percent"].as_f64(),
        }
    }

    /// "ctx 30% - $0.45" (context part omitted when unavailable)
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = self.context_percent {
            parts.push(format!("ctx {p:.0}%"));
        }
        parts.push(format!("${:.2}", self.cost));
        parts.join(" \u{b7} ")
    }

    /// 缓存命中率 = cacheRead / (input + cacheWrite + cacheRead)（分母覆盖
    /// 全部输入类 token；pi-web session-info-popover 同式）。
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let denom = self.input + self.cache_write + self.cache_read;
        (denom > 0).then(|| self.cache_read as f64 / denom as f64)
    }
}
