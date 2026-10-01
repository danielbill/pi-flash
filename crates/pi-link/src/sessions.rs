//! Session discovery over `~/.pi/agent/sessions/` — indexed scanner
//! (docs/sessions.md + ARCHITECTURE.md startup budget).
//!
//! Layout: `<sessions>/<group>/<timestamp>_<id>.jsonl`, where `<group>` is
//! `--<cwd with ':' / '\' / '/' dashed>--`; the first line is the session
//! header `{"type":"session","version":3,"id":...,"cwd":...}`, the first
//! user message follows near the top, and renames append `session_info`
//! entries (latest wins) near the end.
//!
//! Startup-speed design: summaries never read whole files — the header and
//! preview come from a bounded prefix, the name from a bounded tail, the
//! message count from a single streaming pass. `(mtime, size)` fingerprints
//! index results in memory and on disk, so a warm start stats files only
//! (pi-web's scanner-index parity). The window between group directories
//! (which encode the cwd) lets project lists read one directory instead of
//! every session on disk.

use std::collections::HashMap;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::Serialize;
use serde_json::{Value, json};

/// Bounded prefix parsed for header + first user message (preview).
const PREFIX_BYTES: u64 = 256 * 1024;
/// Bounded tail parsed for the latest `session_info` rename.
const TAIL_BYTES: u64 = 64 * 1024;
/// Lines of the prefix considered when looking for the preview message.
const PREVIEW_LINES: usize = 40;

const NEEDLE_MESSAGE: &str = "\"type\":\"message\"";
const NEEDLE_SESSION_INFO: &str = "\"type\":\"session_info\"";

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub id: String,
    pub cwd: String,
    pub modified: SystemTime,
    /// first user message text (used as the row label, like pi-web)
    pub preview: String,
    pub message_count: u64,
    /// session name (pi `session_info` entry; pi-web `session.name` — the
    /// row label prefers it over `preview` when set)
    pub name: Option<String>,
}

/// Group directory name for a cwd: separators dashed, wrapped in `--`.
/// Encoding direction only (a `-` in the original path is indistinguishable,
/// so decoding needs the session header's `cwd`).
pub fn group_name_for_cwd(cwd: &str) -> String {
    let encoded: String = cwd
        .chars()
        .map(|c| match c {
            ':' | '\\' | '/' => '-',
            _ => c,
        })
        .collect();
    format!("--{encoded}--")
}

/// Home-relative sessions root (`~/.pi/agent/sessions`), honoring
/// `PI_CODING_AGENT_SESSION_DIR` / `--session-dir` is left to callers that
/// spawn pi; discovery here covers the default location.
pub fn sessions_root() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("PI_CODING_AGENT_SESSION_DIR") {
        let p = PathBuf::from(d);
        return p.is_dir().then_some(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let p = Path::new(&home).join(".pi").join("agent").join("sessions");
    p.is_dir().then_some(p)
}

// ---------------------------------------------------------------------------
// fingerprint index entry (persisted)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct IndexEntry {
    mtime_sec: u64,
    mtime_nsec: u32,
    size: u64,
    id: String,
    cwd: String,
    preview: String,
    name: Option<String>,
    message_count: u64,
}

impl IndexEntry {
    fn fingerprint_of(mtime: &SystemTime, size: u64) -> (u64, u32, u64) {
        let dur = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
        (dur.as_secs(), dur.subsec_nanos(), size)
    }

    fn matches(&self, mtime: &SystemTime, size: u64) -> bool {
        Self::fingerprint_of(mtime, size) == (self.mtime_sec, self.mtime_nsec, self.size)
    }

    fn to_session_info(&self, path: &Path, modified: SystemTime) -> SessionInfo {
        SessionInfo {
            path: path.to_path_buf(),
            id: self.id.clone(),
            cwd: self.cwd.clone(),
            modified,
            preview: self.preview.clone(),
            message_count: self.message_count,
            name: self.name.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// scanner
// ---------------------------------------------------------------------------

/// Indexing scanner over one sessions root. Fingerprints files by
/// `(mtime, size)`; uncached files get one bounded-prefix + bounded-tail +
/// streaming-count scan, results are cached in memory and (when an index
/// path is set) persisted atomically.
pub struct Scanner {
    root: PathBuf,
    index_path: Option<PathBuf>,
    entries: HashMap<PathBuf, IndexEntry>,
    dirty: bool,
    /// files actually scanned since `Scanner` creation (diagnostics/tests)
    scans: u64,
}

impl Scanner {
    /// Volatile scanner (no persistence) — tests and one-shot use.
    pub fn open(root: &Path) -> Scanner {
        Scanner {
            root: root.to_path_buf(),
            index_path: None,
            entries: HashMap::new(),
            dirty: false,
            scans: 0,
        }
    }

    /// Scanner with a persisted index loaded up front; call
    /// [`save_index`](Self::save_index) to flush (also implicit after scans).
    pub fn open_persisted(root: &Path, index_path: &Path) -> Scanner {
        let mut s = Scanner::open(root);
        s.index_path = Some(index_path.to_path_buf());
        s.load_index();
        s
    }

    fn load_index(&mut self) {
        let Some(path) = self.index_path.clone() else { return };
        let Ok(Value::Object(obj)) = crate::config::read_json(&path) else {
            return;
        };
        let Some(entries) = obj.get("entries").and_then(|v| v.as_object()) else {
            return;
        };
        for (k, v) in entries {
            if let Ok(e) = serde_json::from_value::<IndexEntry>(v.clone()) {
                self.entries.insert(PathBuf::from(k), e);
            }
        }
    }

    /// Atomic index flush (tmp + rename via config::write_json).
    pub fn save_index(&self) {
        let Some(path) = &self.index_path else { return };
        let entries: serde_json::Map<String, Value> = self
            .entries
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    serde_json::to_value(v).unwrap_or(Value::Null),
                )
            })
            .collect();
        let value = serde_json::json!({ "version": 1, "entries": entries });
        let _ = crate::config::write_json(path, &value);
    }

    /// Most-recently-modified sessions across all groups, newest first.
    pub fn list(&mut self, max: usize) -> Vec<SessionInfo> {
        let groups = self.group_dirs();
        self.list_groups(&groups, max)
    }

    /// Sessions of one project only: reads the cwd's group directory,
    /// skipping every other group's IO (semantic cwd filtering stays with
    /// the caller — the header `cwd` is authoritative, not the dir name).
    pub fn list_for_cwd(&mut self, cwd: &str, max: usize) -> Vec<SessionInfo> {
        let group = self.root.join(group_name_for_cwd(cwd));
        if group.is_dir() {
            self.list_groups(&[group], max)
        } else {
            Vec::new()
        }
    }

    fn group_dirs(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        rd.flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect()
    }

    fn list_groups(&mut self, groups: &[PathBuf], max: usize) -> Vec<SessionInfo> {
        let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
        for group in groups {
            let Ok(rd) = std::fs::read_dir(group) else { continue };
            for f in rd.flatten() {
                let path = f.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Ok(meta) = f.metadata() {
                    let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
                    files.push((mtime, path));
                }
            }
        }
        files.sort_by(|a, b| b.0.cmp(&a.0));
        files.truncate(max);

        let mut out = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for (mtime, path) in files {
            seen.push(path.clone());
            if let Some(info) = self.info_for(&path, &mtime) {
                out.push(info);
            }
        }
        self.prune_missing(&seen, groups);
        if self.dirty {
            self.save_index();
            self.dirty = false;
        }
        out
    }

    /// Cached-or-scan lookup keyed by the `(mtime, size)` fingerprint.
    fn info_for(&mut self, path: &Path, mtime: &SystemTime) -> Option<SessionInfo> {
        let size = std::fs::metadata(path).ok()?.len();
        if let Some(entry) = self.entries.get(path) {
            if entry.matches(mtime, size) {
                return Some(entry.to_session_info(path, *mtime));
            }
        }
        let entry = scan_file(path, *mtime, size)?;
        self.scans += 1;
        self.dirty = true;
        self.entries.insert(path.to_path_buf(), entry.clone());
        Some(entry.to_session_info(path, *mtime))
    }

    /// Drop index rows for files that no longer exist under the scanned
    /// scopes, so the persisted index doesn't accumulate dead paths.
    fn prune_missing(&mut self, seen: &[PathBuf], groups: &[PathBuf]) {
        let before = self.entries.len();
        self.entries.retain(|path, _| {
            if seen.iter().any(|s| s == path) {
                return true;
            }
            // prune only within the scopes we just enumerated: keep
            // out-of-scope rows and files that still exist
            !(groups.iter().any(|g| path.starts_with(g)) && !path.is_file())
        });
        if self.entries.len() != before {
            self.dirty = true;
        }
    }
}

// ---------------------------------------------------------------------------
// single-file scan (bounded prefix + bounded tail + streaming count)
// ---------------------------------------------------------------------------

fn read_prefix(path: &Path, bytes: u64) -> String {
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
    if f
        .seek(SeekFrom::End(-(window as i64)))
        .is_err()
    {
        return String::new();
    }
    let mut buf = Vec::with_capacity(window as usize);
    if f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Streaming message count — one pass, no whole-file allocation.
fn count_messages(path: &Path) -> u64 {
    use std::io::BufRead;
    let Ok(f) = std::fs::File::open(path) else { return 0 };
    let mut reader = std::io::BufReader::with_capacity(128 * 1024, f);
    let mut line = String::new();
    let mut count = 0u64;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if line.contains(NEEDLE_MESSAGE) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// One bounded scan of a session file: header + preview from the prefix,
/// name from the tail, count streamed. Never reads the whole file into
/// memory at once.
fn scan_file(path: &Path, modified: SystemTime, size: u64) -> Option<IndexEntry> {
    let prefix = read_prefix(path, PREFIX_BYTES);
    let mut id = String::new();
    let mut cwd = String::new();
    let mut preview = String::new();
    for line in prefix.lines().take(PREVIEW_LINES) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match v["type"].as_str() {
            Some("session") => {
                id = v["id"].as_str().unwrap_or("").to_string();
                cwd = v["cwd"].as_str().unwrap_or("").to_string();
            }
            Some("message") => {
                if v["message"]["role"].as_str() == Some("user") && preview.is_empty() {
                    let c = &v["message"]["content"];
                    preview = match c {
                        Value::String(s) => s.clone(),
                        Value::Array(items) => items
                            .iter()
                            .filter_map(|b| {
                                (b["type"] == "text")
                                    .then(|| b["text"].as_str().unwrap_or("").to_string())
                            })
                            .collect::<Vec<_>>()
                            .join(""),
                        _ => String::new(),
                    };
                    if !preview.is_empty() {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if id.is_empty() {
        return None;
    }
    // renames append `session_info` entries; the latest wins (tail window)
    let mut name = None;
    let tail = read_tail(path, TAIL_BYTES);
    if let Some(pos) = tail.rfind(NEEDLE_SESSION_INFO) {
        // the needle sits inside the JSON line — back up to its `{`
        let line_start = tail[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let tail_rest = &tail[line_start..];
        let line_end = tail_rest.find('\n').unwrap_or(tail_rest.len());
        if let Ok(v) = serde_json::from_str::<Value>(&tail_rest[..line_end]) {
            if let Some(n) = v["name"].as_str() {
                name = Some(n.to_string());
            }
        }
    }
    if preview.len() > 120 {
        // truncate at a char boundary
        let mut cut = 120;
        while !preview.is_char_boundary(cut) {
            cut -= 1;
        }
        preview.truncate(cut);
        preview.push('…');
    }
    preview = preview.replace('\n', " ");
    let dur = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    Some(IndexEntry {
        mtime_sec: dur.as_secs(),
        mtime_nsec: dur.subsec_nanos(),
        size,
        id,
        cwd,
        preview,
        name,
        message_count: count_messages(path),
    })
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

/// Parse the trailing whole message entries of a session file — the
/// disk-direct render path (agent_session renders the last conversation
/// before the RPC session is up). Returns up to `max` parsed
/// `{"type":"message",...}` values, oldest→newest within the window.
pub fn read_tail_messages(path: &Path, tail_bytes: u64, max: usize) -> Vec<Value> {
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
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
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
    let Ok(data) = std::fs::read(path) else { return 0 };
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
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let mut parent_of: HashMap<String, String> = HashMap::new();
    let mut messages: Vec<Value> = Vec::new();
    let mut anchor: Option<String> = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
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
    let Some(anchor) = anchor else { return Vec::new() };
    let mut on_chain: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cur = Some(anchor);
    while let Some(id) = cur {
        if !on_chain.insert(id.clone()) {
            break; // cycle guard
        }
        cur = parent_of.get(&id).filter(|p| !p.is_empty()).cloned();
    }
    messages.retain(|v| {
        v["id"]
            .as_str()
            .is_some_and(|id| on_chain.contains(id))
    });
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
                let index = crate::config::agent_dir().join("pi-flash-session-index.json");
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

/// List one project's sessions without touching other groups' files.
/// (Header-cwd semantic filtering stays with the caller.)
pub fn list_sessions_for_cwd(cwd: &str, max: usize) -> Vec<SessionInfo> {
    with_scanner(|s| s.list_for_cwd(cwd, max))
}

/// Process-wide scanner diagnostics: `(files scanned, index rows)`.
/// Startup measurements read this after the first list (ARCHITECTURE.md
/// startup budget).
pub fn scan_diagnostics() -> (u64, usize) {
    with_scanner(|s| (s.scans, s.entries.len()))
}

// ---------------------------------------------------------------------------
// session content search (013; pi-web lib/session-search.ts parity)
// ---------------------------------------------------------------------------

/// Total hit cap across the scan.
pub const SEARCH_MAX_RESULTS: usize = 30;
const SEARCH_MAX_FILES: usize = 500;
const SEARCH_MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const SEARCH_MAX_LINE_BYTES: usize = 1024 * 1024;
const SEARCH_BUDGET_MS: u64 = 3000;
const SEARCH_CONTEXT_CHARS: usize = 80;
const SEARCH_MAX_PER_SESSION: usize = 5;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub session_path: PathBuf,
    pub session_name: Option<String>,
    pub preview: String,
    pub modified: SystemTime,
    /// matched message's payload timestamp (epoch ms) — row locator
    pub ts: Option<i64>,
    pub role: String,
    pub before: String,
    pub match_text: String,
    pub after: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    /// budget/result/file cap reached — the UI hints at partial results
    pub truncated: bool,
}

/// Literal case-insensitive content search across one project's session
/// files, newest first. Hits are capped per session so the grouped view
/// stays readable; user/assistant text blocks only (pi-web scope).
pub fn search_sessions_for_cwd(cwd: &str, query: &str) -> SearchResponse {
    match sessions_root() {
        Some(root) => search_sessions_in_dir(&root.join(group_name_for_cwd(cwd)), query),
        None => SearchResponse { hits: Vec::new(), truncated: true },
    }
}

/// The scan itself, over one project group directory (testable without
/// touching the real sessions root).
pub fn search_sessions_in_dir(group: &Path, query: &str) -> SearchResponse {
    let mut resp = SearchResponse::default();
    let needle = query.trim();
    if needle.is_empty() {
        return resp;
    }
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    let Ok(rd) = std::fs::read_dir(&group) else {
        return resp;
    };
    for f in rd.flatten() {
        let path = f.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = f.metadata() else { continue };
        files.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), path));
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(SEARCH_BUDGET_MS);
    let needle_lc: Vec<char> = needle.to_lowercase().chars().collect();
    let mut scanned = 0usize;
    'files: for (mtime, path) in files {
        if resp.hits.len() >= SEARCH_MAX_RESULTS
            || scanned >= SEARCH_MAX_FILES
            || std::time::Instant::now() >= deadline
        {
            resp.truncated = true;
            break;
        }
        scanned += 1;
        let Ok(bytes) = std::fs::read(&path) else {
            resp.truncated = true;
            continue;
        };
        if bytes.len() as u64 > SEARCH_MAX_FILE_BYTES {
            resp.truncated = true;
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else { continue };
        let mut session_name: Option<String> = None;
        let mut preview = String::new();
        // (ts, role, before, match, after) gathered per file, assembled below
        let mut local: Vec<(Option<i64>, String, String, String, String)> = Vec::new();
        for line in text.lines() {
            if std::time::Instant::now() >= deadline {
                resp.truncated = true;
                break 'files;
            }
            if line.len() > SEARCH_MAX_LINE_BYTES {
                resp.truncated = true;
                continue;
            }
            let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if entry["type"] == "session_info" {
                session_name = entry["content"]["name"]
                    .as_str()
                    .map(str::to_string)
                    .or(session_name);
                continue;
            }
            if entry["type"] != "message" {
                continue;
            }
            let role = entry["message"]["role"].as_str().unwrap_or("");
            if role != "user" && role != "assistant" {
                continue;
            }
            let blocks: Vec<String> = entry["message"]["content"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|b| b["type"] == "text")
                        .filter_map(|b| b["text"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if blocks.is_empty() {
                continue;
            }
            if preview.is_empty() && role == "user" {
                let t: String = blocks
                    .first()
                    .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
                    .unwrap_or_default();
                preview = t.chars().take(120).collect();
            }
            let joined = blocks.join("\n");
            let orig: Vec<char> = joined.chars().collect();
            let lower: Vec<char> = orig
                .iter()
                .map(|c| c.to_lowercase().next().unwrap_or(*c))
                .collect();
            let Some(pos) = find_sub(&lower, &needle_lc) else {
                continue;
            };
            let ts = entry["message"]["timestamp"]
                .as_i64()
                .or_else(|| entry["timestamp"].as_str().and_then(parse_iso_ts));
            let squash = |range: std::ops::Range<usize>| -> String {
                orig[range]
                    .iter()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let start = pos.saturating_sub(SEARCH_CONTEXT_CHARS);
            let end = (pos + needle_lc.len() + SEARCH_CONTEXT_CHARS).min(orig.len());
            let mut before = squash(start..pos);
            if pos > SEARCH_CONTEXT_CHARS {
                before = format!("...{before}");
            }
            let match_text = orig[pos..pos + needle_lc.len()].iter().collect::<String>();
            let mut after = squash(end..orig.len().min(end + SEARCH_CONTEXT_CHARS));
            if end + SEARCH_CONTEXT_CHARS < orig.len() {
                after.push_str("...");
            }
            local.push((ts, role.to_string(), before, match_text, after));
            if local.len() >= SEARCH_MAX_PER_SESSION {
                break;
            }
        }
        for (ts, role, before, match_text, after) in local {
            if resp.hits.len() >= SEARCH_MAX_RESULTS {
                resp.truncated = true;
                break 'files;
            }
            resp.hits.push(SearchHit {
                session_path: path.clone(),
                session_name: session_name.clone(),
                preview: preview.clone(),
                modified: mtime,
                ts,
                role,
                before,
                match_text,
                after,
            });
        }
    }
    resp
}

fn find_sub(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Fixed-format RFC3339 UTC -> epoch ms ("2026-09-25T08:09:47.902Z", the pi
/// writer's shape). Hand-parsed: pi-link stays date-crate-free.
fn parse_iso_ts(v: &str) -> Option<i64> {
    let b = v.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || (b[10] != b'T' && b[10] != b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { v.get(r)?.parse().ok() };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let ms = if b.len() >= 23 && b[19] == b'.' { num(20..23)? } else { 0 };
    // days since 1970-01-01 (Howard Hinnant's days_from_civil)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 24 + h) * 60 + mi) * 60 * 1000 + se * 1000 + ms)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub fn write_session(root: &Path, cwd: &str, id: &str, body: &str) -> PathBuf {
        let group = root.join(group_name_for_cwd(cwd));
        std::fs::create_dir_all(&group).unwrap();
        let header = format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"{id}\",\"cwd\":\"{}\"}}",
            cwd.replace('\\', "\\\\")
        );
        let path = group.join(format!("2026-09-26T00-00-00-000Z_{id}.jsonl"));
        std::fs::write(&path, format!("{header}\n{body}")).unwrap();
        path
    }
}

#[cfg(test)]
mod tests {
    use super::testing::write_session;
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-link-sess-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const BODY: &str = concat!(
        "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"hello 世界\"}}\n",
        "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
        "{\"type\":\"session_info\",\"name\":\"renamed\"}\n",
        "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"again\"}}\n",
    );

    #[test]
    fn content_search_finds_matches_with_context() {
        let root = temp_root("search");
        let body = concat!(
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"timestamp\":1790454123456,\"content\":[{\"type\":\"text\",\"text\":\"help me refactor the parser code\"}]}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"timestamp\":1790454125000,\"content\":[{\"type\":\"text\",\"text\":\"sure, the PARSER module is ready\"}]}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"content\":\"ignored\"}}\n"
        );
        write_session(&root, "D:\\proj-s", "s1", body);
        let group = root.join(group_name_for_cwd("D:\\proj-s"));
        let resp = search_sessions_in_dir(&group, "PARSER");
        assert!(!resp.truncated);
        assert_eq!(resp.hits.len(), 2, "user + assistant both match");
        assert_eq!(resp.hits[0].ts, Some(1790454123456));
        assert_eq!(resp.hits[0].match_text, "parser"); // lowercased needle len
        assert!(resp.hits[0].before.starts_with("help me"));
        assert!(resp.hits[1].match_text.eq_ignore_ascii_case("parser"));
        // empty query: no hits, no truncation
        let empty = search_sessions_in_dir(&group, "   ");
        assert!(empty.hits.is_empty() && !empty.truncated);
        // no match: clean empty response
        let miss = search_sessions_in_dir(&group, "quantum");
        assert!(miss.hits.is_empty() && !miss.truncated);
    }

    #[test]
    fn group_name_encoding() {
        assert_eq!(
            group_name_for_cwd("D:\\ai_workspace\\pi-flash"),
            "--D--ai_workspace-pi-flash--"
        );
        assert_eq!(group_name_for_cwd("/home/u/x"), "---home-u-x--");
    }

    #[test]
    fn scan_parses_summary_name_and_count() {
        let root = temp_root("scan");
        let path = write_session(&root, "D:\\proj", "abc", BODY);
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let e = scan_file(&path, mtime, 42).expect("scan ok");
        assert_eq!(e.id, "abc");
        assert_eq!(e.cwd, "D:\\proj");
        assert_eq!(e.preview, "hello 世界");
        assert_eq!(e.name.as_deref(), Some("renamed"));
        assert_eq!(e.message_count, 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_for_cwd_targets_one_group() {
        let root = temp_root("groups");
        write_session(&root, "D:\\proj-a", "a1", BODY);
        write_session(&root, "D:\\proj-b", "b1", BODY);
        let mut s = Scanner::open(&root);
        let a = s.list_for_cwd("D:\\proj-a", 10);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].id, "a1");
        assert_eq!(s.list_for_cwd("D:\\missing", 10).len(), 0);
        assert_eq!(s.list(10).len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_index_hits_and_invalidates() {
        let root = temp_root("index");
        let path = write_session(&root, "D:\\proj", "x1", BODY);
        let mut s = Scanner::open(&root);
        assert_eq!(s.list(10).len(), 1);
        assert_eq!(s.scans, 1, "first list scans the file");
        assert_eq!(s.list(10).len(), 1);
        assert_eq!(s.scans, 1, "second list is index-served");

        // fingerprint change (size differs) forces a rescan — append keeps
        // the header line intact
        let mut content = std::fs::read_to_string(&path).unwrap();
        content
            .push_str("{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"z\"}}\n");
        std::fs::write(&path, content).unwrap();
        let infos = s.list(10);
        assert_eq!(s.scans, 2, "appended line invalidates the entry");
        assert_eq!(infos[0].message_count, 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn persisted_index_roundtrip() {
        let root = temp_root("persist");
        write_session(&root, "D:\\proj", "p1", BODY);
        let idx = root.join("index.json");
        {
            let mut a = Scanner::open_persisted(&root, &idx);
            a.list(10);
            a.save_index();
            assert_eq!(a.scans, 1);
        }
        let mut b = Scanner::open_persisted(&root, &idx);
        assert_eq!(b.list(10).len(), 1);
        assert_eq!(b.scans, 0, "warm scanner stats only, no file scans");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_drops_deleted_files() {
        let root = temp_root("prune");
        let path = write_session(&root, "D:\\proj", "d1", BODY);
        let mut s = Scanner::open(&root);
        assert_eq!(s.list(10).len(), 1);
        assert_eq!(s.entries.len(), 1);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(s.list(10).len(), 0);
        assert!(s.entries.is_empty(), "dead row pruned from the index");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tail_messages_window_and_cap() {
        let root = temp_root("tail");
        let mut body = String::new();
        for i in 0..5 {
            body.push_str(&format!(
                "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"m{i}\"}}}}\n"
            ));
        }
        let path = write_session(&root, "D:\\proj", "t1", &body);
        let msgs = read_tail_messages(&path, 1024 * 1024, 3);
        assert_eq!(msgs.len(), 3, "capped at max");
        let last = msgs.last().unwrap()["message"]["content"].as_str().unwrap();
        assert_eq!(last, "m4");
        let first = msgs.first().unwrap()["message"]["content"].as_str().unwrap();
        assert_eq!(first, "m2", "oldest of the window dropped first");

        // tiny window starting mid-line: first partial line discarded
        let small = read_tail_messages(&path, 200, 10);
        assert!(small.len() <= 5);
        for v in &small {
            assert!(v["message"]["content"].is_string(), "no partial JSON rows");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_real_sessions_when_present() {
        // machine-dependent smoke test: only asserts invariants, never fails
        // when no sessions exist (e.g. fresh CI machine). Prints the scan
        // timing + index diagnostics used by the startup budget record.
        let t0 = std::time::Instant::now();
        let sessions = list_sessions(50);
        let elapsed = t0.elapsed();
        let (scans, rows) = scan_diagnostics();
        eprintln!(
            "scan: {} sessions in {elapsed:?} (files scanned: {scans}, index rows: {rows})",
            sessions.len()
        );
        for s in &sessions {
            assert!(!s.id.is_empty());
            assert!(s.path.exists());
        }
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn parses_session_info_name_from_real_file_tail() {
        // regression for the rfind mid-line bug; machine-dependent by
        // design (the renamed session lives on the dev machine) — CI and
        // fresh checkouts skip silently. Fixture coverage for the same
        // parse path lives in scan_parses_summary_name_and_count.
        let home = match std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            Ok(h) => h,
            Err(_) => {
                eprintln!("no home dir, skipping");
                return;
            }
        };
        let p = Path::new(&home)
            .join(".pi/agent/sessions/--D--ai_workspace-pi_work--")
            .join("2026-09-25T06-12-27-503Z_01a0d731-9aee-71ef-982e-cfb077b412de.jsonl");
        let Ok(meta) = std::fs::metadata(&p) else {
            eprintln!("file missing, skipping");
            return;
        };
        let e = scan_file(&p, meta.modified().unwrap(), meta.len()).expect("session parsed");
        eprintln!("parsed name={:?} preview={} count={}", e.name, e.preview, e.message_count);
        assert!(e.name.is_some(), "session_info name must be parsed");
    }
}

#[cfg(test)]
mod leaf_chain_tests {
    use super::*;

    /// Synthetic session: linear messages, then a mis-parented custom entry
    /// (plan-mode-state shape) that pi's own restore anchors the leaf at —
    /// stranding the final turn. read_leaf_messages must anchor at the last
    /// MESSAGE instead and recover the stranded turn.
    #[test]
    fn leaf_anchored_at_last_message_recovers_stranded_turn() {
        let dir = std::env::temp_dir().join("pi-flash-leaf-chain-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("leaf-chain.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"session","version":3,"id":"hdr","cwd":"/w"}"#, "\n",
                r#"{"type":"message","id":"m1","parentId":"hdr","timestamp":1,"message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#, "\n",
                r#"{"type":"message","id":"m2","parentId":"m1","timestamp":2,"message":{"role":"assistant","content":[{"type":"text","text":"first"}]}}"#, "\n",
                r#"{"type":"message","id":"m3","parentId":"m2","timestamp":3,"message":{"role":"user","content":[{"type":"text","text":"again"}]}}"#, "\n",
                // mis-parented: pi's default leaf lands here (parent = m2, not m3)
                r#"{"type":"custom","customType":"plan-mode-state","id":"c1","parentId":"m2","timestamp":4,"data":{"enabled":false}}"#, "\n",
                // stranded turn
                r#"{"type":"message","id":"m4","parentId":"m3","timestamp":5,"message":{"role":"assistant","content":[{"type":"text","text":"stranded tail"}]}}"#, "\n",
            ),
        )
        .unwrap();

        assert_eq!(count_message_entries(&path), 4);
        let msgs = read_leaf_messages(&path, 100);
        let texts: Vec<String> = msgs
            .iter()
            .map(|v| v["message"]["content"][0]["text"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(texts, vec!["hi", "first", "again", "stranded tail"]);

        // max trims from the head, keeps the newest
        let trimmed = read_leaf_messages(&path, 2);
        let texts: Vec<String> = trimmed
            .iter()
            .map(|v| v["message"]["content"][0]["text"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(texts, vec!["again", "stranded tail"]);

        std::fs::remove_file(&path).ok();
    }
}
