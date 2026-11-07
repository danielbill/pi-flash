//! 单文件有界读（前缀/尾窗/流式计数）+ 面向调用方的读 API 与进程级
//! 全局持久化扫描器。

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use super::info::{SessionInfo, sessions_root};
use super::scan::Scanner;

/// Degraded preview chars grabbed from a prefix-truncated line (see
/// `preview_from_partial_line`). Deliberately shorter than the 120-char
/// full-parse cap: it's a best-effort salvage, not a real preview.
pub(super) const PARTIAL_PREVIEW_CHARS: usize = 50;

// ---------------------------------------------------------------------------
// single-file scan (bounded prefix + bounded tail + streaming count)
// ---------------------------------------------------------------------------

pub(super) fn read_prefix(path: &Path, bytes: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buf = vec![0u8; bytes as usize];
    let mut filled = 0usize;
    while filled < buf.len() {
        match f.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return String::new(),
        }
    }
    buf.truncate(filled);
    String::from_utf8_lossy(&buf).into_owned()
}

fn read_tail(path: &Path, bytes: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let window = len.min(bytes);
    if f.seek(SeekFrom::End(-(window as i64))).is_err() {
        return String::new();
    }
    let mut buf = Vec::with_capacity(window as usize);
    if f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

const NEEDLE_MESSAGE: &str = "\"type\":\"message\"";
const NEEDLE_SESSION_INFO: &str = "\"type\":\"session_info\"";

/// Streaming message count + latest session name — one pass, no
/// whole-file allocation.
pub(super) fn count_messages(
    path: &Path,
    start_byte: u64,
    base: u64,
) -> (u64, u64, Option<String>) {
    // 返回 (总数, 已覆盖字节, 最新会话名)。只统计 '\n' 结尾的完整行；文件尾
    // 未落盘完的半行不计、不推进——半行既可能漏 needle 也可能把 needle 拆在
    // 边界，留给下次扫描重数。start_byte 之前的字节默认已计过（pi 会话文件
    // 只追加）：活跃会话每轮重扫从全量流读退化为 O(新增量)。
    // 名字与 pi getSessionName 同口径：最后一个 session_info 条目生效。流式
    // 倒序做不到，正序扫到"最后一个"等价。needle 只是粗筛——嵌套结构里也
    // 可能撞上子串，解析校验 type 后才认。
    use std::io::{BufRead, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return (base, start_byte, None);
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = start_byte.min(len);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return (base, start, None);
    }
    let mut reader = std::io::BufReader::with_capacity(128 * 1024, f);
    let mut line = String::new();
    let mut count = base;
    let mut consumed = start;
    let mut name: Option<String> = None;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if !line.ends_with('\n') {
                    break; // 尾部半行：留给下次
                }
                consumed += n as u64;
                if line.contains(NEEDLE_MESSAGE) {
                    count += 1;
                }
                if line.contains(NEEDLE_SESSION_INFO) {
                    if let Ok(v) = crate::json::parse_value(line.trim_end()) {
                        if v["type"].as_str() == Some("session_info") {
                            name = v["name"].as_str().map(str::to_string);
                        }
                    }
                }
            }
        }
    }
    (count, consumed, name)
}

/// Salvage a preview from a prefix-truncated line. The 256KB prefix cuts
/// the first user message mid-line exactly when it matters — a huge first
/// paste (system-prompt line alone can be 40-100KB, so a 300KB+ first
/// message starts near the boundary). Full JSON parsing then fails on the
/// one entry the preview needs and the row degrades to its "(empty)"
/// fallback title. Best-effort: locate the user role marker (it must sit
/// before `"content"` — a truncated *assistant* line can quote the literal
/// inside its text) and decode the first string's leading characters.
pub(super) fn preview_from_partial_line(line: &str) -> Option<String> {
    const CONTENT_MARKER: &str = "\"content\"";
    const TEXT_MARKER: &str = "\"text\":\"";
    if !line.starts_with('{') {
        return None;
    }
    let role = line.find("\"role\":\"user\"")?;
    let content = line.find(CONTENT_MARKER)?;
    if role > content {
        return None; // role marker occurs inside message text, not the header
    }
    let rest = line[content + CONTENT_MARKER.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    // string content (`"content":"…"`) vs block array (`"content":[…]`)
    let raw = match rest.strip_prefix('"') {
        Some(s) => s,
        None => {
            let at = rest.find(TEXT_MARKER)? + TEXT_MARKER.len();
            &rest[at..]
        }
    };
    let mut out = String::new();
    let mut chars = raw.chars();
    while out.chars().count() < PARTIAL_PREVIEW_CHARS {
        match chars.next() {
            None | Some('"') => break, // end of raw or closing quote
            Some('\\') => match chars.next() {
                None => break,
                Some('n' | 'r' | 't' | 'b' | 'f') => out.push(' '),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(c);
                    }
                }
                Some(esc) => out.push(esc), // \" \\ \/ … escape lands literally
            },
            Some(c) => out.push(c),
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Map one session-file entry to its renderable message shape (pi-web
/// session-reader.ts entryToMessage parity): `message` entries pass through;
/// `compaction` / `custom_message` map to synthetic role="custom" envelopes;
/// `branch_summary` maps to a custom message too (pi-web renders it as a user
/// bubble, but a synthetic User here would consume a fork-anchor id — kept
/// custom so entry-id alignment stays intact).
fn renderable_message(v: &Value) -> Option<Value> {
    let ts = v["timestamp"].clone();
    let envelope = |message: Value| {
        Some(json!({
            "id": v["id"].clone(),
            "parentId": v["parentId"].clone(),
            "type": "message",
            "timestamp": ts,
            "message": message,
        }))
    };
    match v["type"].as_str()? {
        "message" => Some(v.clone()),
        "compaction" => envelope(json!({
            "role": "custom",
            "customType": "compaction",
            "content": v["summary"].as_str().unwrap_or(""),
            "display": true,
            "details": {
                "tokensBefore": v["tokensBefore"].clone(),
                "firstKeptEntryId": v["firstKeptEntryId"].clone(),
            },
        })),
        "custom_message" => envelope(json!({
            "role": "custom",
            "customType": v["customType"].as_str().unwrap_or(""),
            "content": v["content"].clone(),
            "display": v["display"].as_bool().unwrap_or(true),
            "details": v["details"].clone(),
        })),
        "branch_summary" if v["summary"].as_str().is_some_and(|s| !s.is_empty()) => {
            envelope(json!({
                "role": "custom",
                "customType": "branch_summary",
                "content": v["summary"].as_str().unwrap_or(""),
                "display": true,
            }))
        }
        _ => None,
    }
}

/// 尾窗抗洪泛上限（pi-flash-9eck）：老洪泛会话尾部几十万条不可渲染的
/// custom 条目，256KB 窗口极易全部落空 → 打开卡「载入中」等 RPC。窗口
/// 倍增到此封顶（2MB 解析 ~10ms 量级，只在落空时才付）。
const TAIL_WINDOW_MAX: u64 = 2 * 1024 * 1024;
/// 倍增判定阈值：窗口里 renderable 条目低于此数就继续扩窗。
const TAIL_MIN_RENDERABLE: usize = 10;

/// Parse the trailing whole message entries of a session file — the
/// disk-direct render path (agent_session renders the last conversation
/// before the RPC session is up). Returns up to `max` parsed
/// `{"type":"message",...}` values, oldest→newest within the window.
///
/// 尾窗内 renderable 条目不足时倍增窗口重读（至 [`TAIL_WINDOW_MAX`]）：
/// descriptor 洪泛时代的老会话尾段全是垃圾行，固定窗口会读空。
pub fn read_tail_messages(path: &Path, tail_bytes: u64, max: usize) -> Vec<Value> {
    let mut window = tail_bytes;
    let mut messages = read_tail_window(path, window, max);
    // 窗口字节数单调递增 → renderable 计数单调不减；循环由上限封顶，
    // 中间窗口仍读空也继续扩（垃圾段可能超过初窗数倍）。
    while messages.len() < TAIL_MIN_RENDERABLE && window < TAIL_WINDOW_MAX {
        window = (window * 2).min(TAIL_WINDOW_MAX);
        messages = read_tail_window(path, window, max);
    }
    messages
}

/// 固定字节数的尾窗读取（[`read_tail_messages`] 的单窗实现）。
fn read_tail_window(path: &Path, tail_bytes: u64, max: usize) -> Vec<Value> {
    let tail = read_tail(path, tail_bytes);
    // only a seeked window starts mid-line; a whole-file read doesn't
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let body = if len > tail_bytes {
        let start = tail.find('\n').map(|i| i + 1).unwrap_or(0);
        &tail[start..]
    } else {
        &tail[..]
    };
    let mut messages: Vec<Value> = Vec::new();
    for line in body.lines() {
        let Ok(v) = crate::json::parse_value(line) else {
            continue;
        };
        if let Some(m) = renderable_message(&v) {
            messages.push(m);
        }
    }
    if messages.len() > max {
        messages.drain(..messages.len() - max);
    }
    messages
}

/// Byte-level probe: occurrences of the compact `"type":"message"` marker.
/// Message *text* can contain the literal (false positives), so this is only
/// a cheap upper bound for the leaf-chain integrity check — a false positive
/// costs one full `read_leaf_messages` pass, never correctness.
pub fn count_message_entries(path: &Path) -> u64 {
    let Ok(data) = std::fs::read(path) else {
        return 0;
    };
    const NEEDLE: &[u8] = b"\"type\":\"message\"";
    data.windows(NEEDLE.len()).filter(|&w| w == NEEDLE).count() as u64
}

/// Full leaf-chain read anchored at the LAST `type=message` entry.
///
/// pi's own restore (`session-manager _buildIndex`) anchors the leaf at the
/// last entry of ANY type — a mis-parented non-message entry (real case:
/// `plan-mode-state` written with a stale parentId) strands whole turns off
/// the default chain, and `get_messages` then returns a truncated
/// conversation. Anchoring at the last MESSAGE entry and backtracking
/// parentId repairs the display side. Returns chain messages oldest→newest
/// (file order = chain order: entries always append below their parent),
/// up to `max`.
pub fn read_leaf_messages(path: &Path, max: usize) -> Vec<Value> {
    use std::io::BufRead;
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut parent_of: HashMap<String, String> = HashMap::new();
    let mut messages: Vec<Value> = Vec::new();
    let mut anchor: Option<String> = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = crate::json::parse_value(&line) else {
            continue;
        };
        if v["type"].as_str() == Some("session") {
            continue;
        }
        if let Some(id) = v["id"].as_str() {
            parent_of.insert(
                id.to_string(),
                v["parentId"].as_str().unwrap_or("").to_string(),
            );
            if let Some(m) = renderable_message(&v) {
                anchor = Some(id.to_string());
                messages.push(m);
            }
        }
    }
    let Some(anchor) = anchor else {
        return Vec::new();
    };
    let mut on_chain: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cur = Some(anchor);
    while let Some(id) = cur {
        if !on_chain.insert(id.clone()) {
            break; // cycle guard
        }
        cur = parent_of.get(&id).filter(|p| !p.is_empty()).cloned();
    }
    messages.retain(|v| v["id"].as_str().is_some_and(|id| on_chain.contains(id)));
    if messages.len() > max {
        messages.drain(..messages.len() - max);
    }
    messages
}

// ---------------------------------------------------------------------------
// global convenience (process-wide persisted scanner)
// ---------------------------------------------------------------------------

fn with_scanner<T>(f: impl FnOnce(&mut Scanner) -> T) -> T {
    static SCANNER: Mutex<Option<Scanner>> = Mutex::new(None);
    let mut guard = SCANNER.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let scanner = match sessions_root() {
            Some(root) => {
                // 索引落 pi-flash 自有目录（010-启动.md §4）
                let index = crate::paths::session_index_file()
                    .unwrap_or_else(|| crate::config::agent_dir().join("session-index.json"));
                Scanner::open_persisted(&root, &index)
            }
            None => Scanner::open(Path::new("")),
        };
        *guard = Some(scanner);
    }
    f(guard.as_mut().expect("scanner just initialized"))
}

/// List most-recently-modified sessions across all projects, newest first.
pub fn list_sessions(max: usize) -> Vec<SessionInfo> {
    with_scanner(|s| s.list(max))
}

/// [`list_sessions`] minus the excluded files (recents poll's live-runtime
/// skip).
pub fn list_sessions_excluding(
    max: usize,
    exclude: &std::collections::HashSet<PathBuf>,
) -> Vec<SessionInfo> {
    with_scanner(|s| s.list_excluding(max, exclude))
}

/// Summaries for an explicit path list (recents top-N startup), input order
/// preserved, dead paths dropped. Fingerprint-served like every scanner
/// read; a stale entry is rescanned on the spot.
pub fn sessions_for_paths(paths: &[PathBuf]) -> Vec<SessionInfo> {
    with_scanner(|s| {
        paths
            .iter()
            .filter_map(|p| {
                let meta = std::fs::metadata(p).ok()?;
                let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
                s.info_for(p, &mtime)
            })
            .collect()
    })
}

/// List one project's sessions without touching other groups' files.
/// (Header-cwd semantic filtering stays with the caller.)
pub fn list_sessions_for_cwd(cwd: &str, max: usize) -> Vec<SessionInfo> {
    with_scanner(|s| s.list_for_cwd(cwd, max))
}

/// 052 会话自动清理候选：root 下所有 `<group>/*.jsonl` 里 mtime 早于
/// `cutoff` 的文件路径（最后活动 = mtime，与 recents/load window 同一口径）。
/// 只 read_dir + stat，不解析内容——清理是后排任务，不走 Scanner 那套
/// 头尾解析/计数重活。root 不存在（无 sessions 目录）→ 空。
pub fn stale_session_files(root: &Path, cutoff: SystemTime) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(groups) = std::fs::read_dir(root) else {
        return out;
    };
    for group in groups.flatten() {
        let Ok(files) = std::fs::read_dir(group.path()) else {
            continue;
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Ok(md) = f.metadata() else { continue };
            if md.is_file() && md.modified().is_ok_and(|m| m < cutoff) {
                out.push(p);
            }
        }
    }
    out
}

/// Process-wide scanner diagnostics: `(files scanned, index rows)`.
/// Startup measurements read this after the first list (ARCHITECTURE.md
/// startup budget).
pub fn scan_diagnostics() -> (u64, usize) {
    with_scanner(|s| (s.scans, s.entries.len()))
}

#[cfg(test)]
mod tests;
