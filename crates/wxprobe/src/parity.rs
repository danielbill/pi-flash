//! P2 验收：与 ZCode 真实 TS 源码做**逐字节对拍**。
//!
//! 流程：
//! 1. `node parity/run.ts > parity/golden.jsonl` —— 跑 ZCode 原文产出黄金结果
//!    （`parity/*.ts` 是从 ZCode 拷来、只改 import 说明符的副本，函数体逐字保留）
//! 2. `wxprobe parity` —— 用 Rust 实现跑同一份 `parity/cases.json`，逐条比对
//!
//! 任一条不一致即非零退出。用 `--update` 刷新黄金文件（改了 cases 之后）。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::format::messages::Lang;
use crate::format::permission::{get_permission_request_preview, PermissionRequest};
use crate::format::reply::{
    extract_bot_assistant_response_messages, format_bot_assistant_reply_blocks,
    format_bot_permission_request_summary, format_bot_tool_call_reply,
    format_bot_tool_call_summary_line, is_bot_tool_call_reply_terminal, BotAssistantReplyBlock,
    BotReplyToolCallState, ChangeSummary, FileChange, ToolStatus,
};
use crate::format::status::{status_task_line, task_running_duration};
use crate::format::summary::{get_compact_tool_call_summary, ToolCallSummarySource};

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("parity")
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .filter(|x| !x.is_null())
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn lang_of(v: &Value) -> Lang {
    match v.get("locale").and_then(Value::as_str) {
        Some("en-US") => Lang::En,
        _ => Lang::ZhCn,
    }
}

fn status_of(v: &Value) -> Option<ToolStatus> {
    match v.as_str() {
        None => None,
        Some("pending") => Some(ToolStatus::Pending),
        Some("in_progress") => Some(ToolStatus::InProgress),
        Some("completed") => Some(ToolStatus::Completed),
        Some("failed") => Some(ToolStatus::Failed),
        Some("denied") => Some(ToolStatus::Denied),
        Some("stopped") => Some(ToolStatus::Stopped),
        Some(other) => panic!("未知 status: {other}"),
    }
}

/// 与 ZCode `BotReplyToolCallState` 对齐（toolId 不参与格式化）。
fn tool_call_of(c: &Value) -> BotReplyToolCallState {
    BotReplyToolCallState {
        tool_id: c["id"].as_str().unwrap_or_default().to_string(),
        title: opt_str(c, "title"),
        kind: opt_str(c, "toolKind"),
        input: c.get("input").cloned().unwrap_or(Value::Null),
        output: c.get("output").filter(|v| !v.is_null()).cloned(),
        status: status_of(&c["status"]),
        error: opt_str(c, "error"),
        raw: c.get("raw").filter(|v| !v.is_null()).cloned(),
    }
}

fn request_of(c: &Value) -> PermissionRequest {
    PermissionRequest {
        title: opt_str(c, "title"),
        description: opt_str(c, "description").unwrap_or_default(),
        kind: opt_str(c, "permissionKind").unwrap_or_default(),
        raw: c.get("raw").cloned().unwrap_or(Value::Null),
    }
}

/// 摘要对象序列化：键序与 ZCode 对象字面量一致，
/// `undefined` 字段不出现（JSON.stringify 同语义）。
fn summary_to_value(summary: &crate::format::summary::ToolCallSummary) -> Value {
    let mut map = Map::new();
    map.insert("primaryText".into(), json!(summary.primary_text));
    if let Some(secondary) = &summary.secondary_text {
        map.insert("secondaryText".into(), json!(secondary));
    }
    if let Some(stat) = summary.change_stat {
        let mut s = Map::new();
        s.insert("added".into(), json!(stat.added));
        s.insert("removed".into(), json!(stat.removed));
        map.insert("changeStat".into(), Value::Object(s));
    }
    Value::Object(map)
}

/// 预览对象序列化：键序 = title, command, filePaths, scope, fileChange, fileChanges。
/// `command` / `fileChange` 是 `T | null`，**null 要保留**（不是 undefined）。
fn preview_to_value(preview: &crate::format::permission::PermissionRequestPreview) -> Value {
    let change = |c: &crate::format::permission::PermissionFileChange| {
        json!({
            "path": c.path,
            "type": match c.kind {
                crate::format::permission::ChangeKind::Add => "add",
                crate::format::permission::ChangeKind::Update => "update",
            },
        })
    };
    json!({
        "title": preview.title,
        "command": preview.command,
        "filePaths": preview.file_paths,
        "scope": match preview.scope {
            crate::format::permission::Scope::Command => "command",
            crate::format::permission::Scope::File => "file",
            crate::format::permission::Scope::Generic => "generic",
        },
        "fileChange": preview.file_change.as_ref().map(change),
        "fileChanges": preview.file_changes.iter().map(change).collect::<Vec<_>>(),
    })
}

fn blocks_of(c: &Value) -> Vec<BotAssistantReplyBlock> {
    let mut out = Vec::new();
    for block in c["blocks"].as_array().expect("blocks 必须是数组") {
        match block["type"].as_str().expect("block.type") {
            "content" => out.push(BotAssistantReplyBlock::Content {
                content: block["content"].as_str().unwrap_or_default().to_string(),
            }),
            "tool-call" => out.push(BotAssistantReplyBlock::ToolCall {
                tool_call: tool_call_of(&block["toolCall"]),
            }),
            "change-summary" => {
                let cs = &block["changeSummary"];
                let files = cs["files"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .map(|f| FileChange {
                                path: f["path"].as_str().unwrap_or_default().to_string(),
                                added: f["added"].as_u64().unwrap_or(0) as u32,
                                removed: f["removed"].as_u64().unwrap_or(0) as u32,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(BotAssistantReplyBlock::ChangeSummary {
                    change_summary: ChangeSummary {
                        file_count: cs["fileCount"].as_u64().unwrap_or(0) as u32,
                        added: cs["added"].as_u64().unwrap_or(0) as u32,
                        removed: cs["removed"].as_u64().unwrap_or(0) as u32,
                        files,
                    },
                });
            }
            other => panic!("未知 block.type: {other}"),
        }
    }
    out
}

fn compute(c: &Value) -> Value {
    let kind = c["kind"].as_str().expect("case.kind");
    let lang = lang_of(c);
    let workspace = c.get("workspacePath").and_then(Value::as_str);

    match kind {
        "tool_line" => Value::String(format_bot_tool_call_summary_line(
            &tool_call_of(c),
            lang,
            workspace,
        )),
        "tool_reply" => Value::String(format_bot_tool_call_reply(
            &tool_call_of(c),
            lang,
            workspace,
        )),
        "blocks" => Value::Array(
            format_bot_assistant_reply_blocks(&blocks_of(c), lang, workspace)
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
        "perm_summary" => Value::String(format_bot_permission_request_summary(
            &request_of(c),
            lang,
            workspace,
        )),
        "perm_preview" => preview_to_value(&get_permission_request_preview(&request_of(c))),
        "flush" => {
            let buffer = c["buffer"].as_str().unwrap_or_default();
            let force = c["force"].as_bool().unwrap_or(false);
            let (messages, rest) = extract_bot_assistant_response_messages(buffer, force);
            json!({ "messages": messages, "rest": rest })
        }
        "terminal" => Value::Bool(is_bot_tool_call_reply_terminal(status_of(&c["status"]))),
        "duration" => Value::String(task_running_duration(c["ms"].as_u64().unwrap_or(0))),
        "task_line" => Value::String(status_task_line(
            c["label"].as_str().unwrap_or_default(),
            c["title"].as_str().unwrap_or_default(),
            c["taskId"].as_str().unwrap_or_default(),
        )),
        "compact_summary" => {
            let title = opt_str(c, "title");
            let input = c.get("input").cloned().unwrap_or(Value::Null);
            let source = ToolCallSummarySource {
                title: title.as_deref(),
                kind: c["toolKind"].as_str().unwrap_or("tool"),
                input: &input,
                output: c.get("output").filter(|v| !v.is_null()),
                raw: c.get("raw").filter(|v| !v.is_null()),
            };
            summary_to_value(&get_compact_tool_call_summary(source))
        }
        other => panic!("未知 case kind: {other}"),
    }
}

fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("读 {} 失败: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

/// `wxprobe parity [--update]`
pub fn run(update: bool) -> Result<(), String> {
    let dir = parity_dir();
    let cases_path = dir.join("cases.json");
    let golden_path = dir.join("golden.jsonl");

    let cases: Vec<Value> = serde_json::from_str(&fs::read_to_string(&cases_path).map_err(|e| {
        format!(
            "读 {} 失败: {e}（先 `python prep_parity.py` 生成，或确认 ZCode 仓库在位）",
            cases_path.display()
        )
    })?)
    .map_err(|e| format!("cases.json 解析失败: {e}"))?;

    let mut outputs: Vec<Value> = Vec::with_capacity(cases.len());
    for case in &cases {
        let id = case["id"].as_str().ok_or("case 缺 id")?.to_string();
        outputs.push(json!({ "id": id, "out": compute(case) }));
    }

    if update {
        let body = outputs
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&golden_path, format!("{body}\n")).map_err(|e| e.to_string())?;
        println!("已刷新 {}（{} 条）", golden_path.display(), outputs.len());
        return Ok(());
    }

    let golden: Vec<Value> = read_lines(&golden_path)
        .into_iter()
        .map(|l| serde_json::from_str(&l).map_err(|e| format!("golden 行解析失败: {e}")))
        .collect::<Result<_, _>>()?;
    let golden_by_id: std::collections::HashMap<String, Value> = golden
        .into_iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?.to_string();
            Some((id, v.get("out")?.clone()))
        })
        .collect();

    let mut mismatch = 0usize;
    for mine in &outputs {
        let id = mine["id"].as_str().unwrap_or_default();
        let mine_out = &mine["out"];
        match golden_by_id.get(id) {
            None => {
                println!("MISS  {id}（黄金文件里没有这条 case）");
                mismatch += 1;
            }
            Some(want) => {
                let a = serde_json::to_string(mine_out).unwrap();
                let b = serde_json::to_string(want).unwrap();
                if a != b {
                    println!("DIFF  {id}");
                    println!("      rust : {a}");
                    println!("      zcode: {b}");
                    mismatch += 1;
                }
            }
        }
    }
    if golden_by_id.len() != outputs.len() {
        println!(
            "MISS  黄金文件多出 {} 条（cases.json 需 --update）",
            golden_by_id.len().saturating_sub(outputs.len())
        );
        mismatch += 1;
    }

    if mismatch == 0 {
        println!(
            "PARITY OK：{} 条 case 与 ZCode TS 输出逐字节一致",
            outputs.len()
        );
        Ok(())
    } else {
        Err(format!(
            "对拍失败：{mismatch}/{} 条不一致",
            outputs.len()
        ))
    }
}
