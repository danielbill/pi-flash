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
//! preview come from a bounded prefix, the name and the message count from a
//! single streaming pass (the name rides the count scan: renames can sit
//! anywhere in a megabytes-long file, so no fixed window is safe — the old
//! 1MB tail cap silently dropped renames and the sidebar title "reverted").
//! `(mtime, size)` fingerprints
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
/// Lines of the prefix considered when looking for the preview message.
const PREVIEW_LINES: usize = 40;
/// Degraded preview chars grabbed from a prefix-truncated line (see
/// `preview_from_partial_line`). Deliberately shorter than the 120-char
/// full-parse cap: it's a best-effort salvage, not a real preview.
const PARTIAL_PREVIEW_CHARS: usize = 50;

/// 子代理子会话的专属标记（custom entry 的 customType，由扩展在子会话
/// session_start 时落盘）。与父会话里的 `subagent/descriptor` 是两个东西：
/// descriptor 记录委派事实（落父会话），marker 标记"这个会话是子代理"（落
/// 子会话自己）。侧栏过滤只认后者。
const SUBAGENT_CHILD_MARKER: &str = "subagent/child";

/// 子会话头部判定窗口（[`is_subagent_session`]）：marker 由扩展在子会话
/// session_start 时 appendEntry，位置紧随 header，必在头部几十行内——
/// 按前 64KB 有界扫即可判，免整文件读（面板逐行点击热路径）。
const SUBAGENT_HEAD_SCAN: u64 = 64 * 1024;

/// 该会话文件是否是子代理子会话（044-0 只读观察窗的打开守卫）：头部窗口
/// 内出现 `subagent/child` custom entry 即是。header 的 parentSession 不可
/// 用作判定——pi 原生 fork 的分支同样带它。文件不存在/读不了 → false
/// （按普通会话开，行为不变）。
pub fn is_subagent_session(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let take = len.min(SUBAGENT_HEAD_SCAN) as usize;
    let mut buf = vec![0u8; take];
    if f.read_exact(&mut buf).is_err() {
        return false;
    }
    let Ok(text) = String::from_utf8(buf) else {
        return false;
    };
    text.split_inclusive('\n').any(|line| {
        serde_json::from_str::<Value>(line.trim())
            .ok()
            .is_some_and(|v| {
                v["type"].as_str() == Some("custom")
                    && v["customType"].as_str() == Some(SUBAGENT_CHILD_MARKER)
            })
    })
}

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

/// group 目录的 on-disk 形态：root 下按大小写不敏感找同名目录，返回
/// read_dir 报告的名字。找不到（未建 / 自定义 session-dir 布局）→ 原样。
///
/// Windows 的 read_dir 保留**调用方传入**的父目录大小写：`list_for_cwd`
/// 用 ws.cwd 拼 group（projects.json 里可能是小写 `d:`），整列行的 path
/// 就跟着那个形态走——与 pi 自报 / 其他入口的形态互相失配。
fn canonical_group_dir(root: &Path, group: &Path) -> PathBuf {
    let Some(name) = group.file_name() else {
        return group.to_path_buf();
    };
    let name_lc = name.to_string_lossy().to_lowercase();
    let Ok(rd) = std::fs::read_dir(root) else {
        return group.to_path_buf();
    };
    for entry in rd.flatten() {
        let n = entry.file_name();
        if n.to_string_lossy().to_lowercase() == name_lc {
            return root.join(n);
        }
    }
    group.to_path_buf()
}

/// 归一到「扫描器形态」的会话文件路径：sessions root + group 目录都取
/// on-disk 大小写。pi 会按**进程 cwd** 自拼 session 目录（`--d--…`），
/// 磁盘扫描拿到的是 on-disk 形态（`--D--…`）——NTFS 同一个目录、两串
/// 不同的字符串，而 `Path` 相等比较区分大小写：原样绑定 pi 自报路径的
/// 话，r.file / pool key / 通知点集合与侧栏行永远对不上（行转圈、完成
/// 点、池命中全失灵）。所有「接受会话文件路径」的入口（follow_session_file /
/// open_session）都先过这里。root 匹配与 group 名匹配均大小写不敏感
/// （Windows 口径）；不在 root 下 / 结构不符 / group 不在盘上 → 原样返回。
pub fn canonical_session_path_in(root: &Path, p: &Path) -> PathBuf {
    let p_str = p.to_string_lossy();
    let root_str = root.to_string_lossy();
    // 大小写不敏感的前缀匹配：剩余段从**原串**切（保留原大小写）；
    // get() 兜住极端 to_lowercase 变长字符的非边界切分，失败即原样
    let rest = p_str
        .strip_prefix(&*root_str)
        .or_else(|| {
            p_str.get(..root_str.len()).and_then(|head| {
                head.to_lowercase()
                    .eq(&root_str.to_lowercase())
                    .then(|| p_str.get(root_str.len()..))
                    .flatten()
            })
        })
        .map(|r| r.trim_start_matches(['\\', '/']))
        .filter(|r| !r.is_empty());
    let Some(rest) = rest else {
        return p.to_path_buf();
    };
    let Some((group, file)) = rest.split_once(['\\', '/']) else {
        return p.to_path_buf();
    };
    if group.is_empty() || file.is_empty() {
        return p.to_path_buf();
    }
    canonical_group_dir(root, &root.join(group)).join(file)
}

/// [`canonical_session_path_in`] over the default sessions root; root
/// unavailable → as-is.
pub fn canonical_session_path(p: &Path) -> PathBuf {
    match sessions_root() {
        Some(root) => canonical_session_path_in(&root, p),
        None => p.to_path_buf(),
    }
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

/// v2: `scan_file` salvages previews from prefix-truncated first user
/// lines (huge first paste ⇒ "(empty)" rows); v1 caches hold the old
/// empty previews on dormant files, so they are dropped once on load.
/// v4: 会话名不再来自尾窗（改名条目被后续聊天推出旧 1MB 上限后，侧栏
/// 标题会"改回去"）；v3 缓存里这些文件的 name=None 是错的 → 丢一次
/// 缓存全量重扫，从只追加的会话文件里找回真名。
const INDEX_VERSION: u64 = 4;

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
    /// 计数已覆盖的字节前缀（pi 会话文件只追加）：增量计数从此字节续读。
    /// 旧版索引文件无此字段 → 0（下次指纹失效时全量重数一次后落盘）。
    #[serde(default)]
    count_bytes: u64,
    /// 子代理专属标记：子会话文件里的 `subagent/child` custom entry（由扩展
    /// 在子会话 session_start 时落盘）。**不能**改用 header 的 `parentSession`
    /// 判定——pi 原生 fork 的分支会话同样带它，那是要正常上侧栏的用户会话。
    #[serde(default)]
    child_marker: bool,
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

    /// Subagent session: listed only through its parent, never as a peer row.
    fn is_child(&self) -> bool {
        self.child_marker
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
    pub(crate) scans: u64,
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
        let Some(path) = self.index_path.clone() else {
            return;
        };
        let Ok(Value::Object(obj)) = crate::config::read_json(&path) else {
            return;
        };
        // Schema bump → discard the whole cache once (entries keep their
        // (mtime, size) fingerprint forever on dormant files, so a changed
        // scan algorithm would never see those files again otherwise).
        if obj.get("version").and_then(|v| v.as_u64()) != Some(INDEX_VERSION) {
            self.dirty = true;
            return;
        }
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
        let value = serde_json::json!({ "version": INDEX_VERSION, "entries": entries });
        // 自有目录可能还没建（首启 / 迁移后）——写前确保存在
        let _ = crate::paths::ensure_dir();
        let _ = crate::config::write_json(path, &value);
    }

    /// Most-recently-modified sessions across all groups, newest first.
    pub fn list(&mut self, max: usize) -> Vec<SessionInfo> {
        self.list_excluding(max, &std::collections::HashSet::new())
    }

    /// [`list`](Self::list) minus the excluded files — the recents poll
    /// passes live runtimes' files here so it never rescans a file pi is
    /// actively appending to (their recency arrives via the event path).
    pub fn list_excluding(
        &mut self,
        max: usize,
        exclude: &std::collections::HashSet<PathBuf>,
    ) -> Vec<SessionInfo> {
        let groups = self.group_dirs();
        self.list_groups(&groups, max, exclude)
    }

    /// Sessions of one project only: reads the cwd's group directory,
    /// skipping every other group's IO (semantic cwd filtering stays with
    /// the caller — the header `cwd` is authoritative, not the dir name).
    /// group 段先归一到 on-disk 名：产出的 SessionInfo.path 恒为扫描器
    /// 形态，不随调用方 cwd 字符串的大小写漂移。
    pub fn list_for_cwd(&mut self, cwd: &str, max: usize) -> Vec<SessionInfo> {
        let group = self.root.join(group_name_for_cwd(cwd));
        if group.is_dir() {
            let group = canonical_group_dir(&self.root, &group);
            self.list_groups(&[group], max, &std::collections::HashSet::new())
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

    fn list_groups(
        &mut self,
        groups: &[PathBuf],
        max: usize,
        exclude: &std::collections::HashSet<PathBuf>,
    ) -> Vec<SessionInfo> {
        let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
        for group in groups {
            let Ok(rd) = std::fs::read_dir(group) else {
                continue;
            };
            for f in rd.flatten() {
                let path = f.path();
                if exclude.contains(&path) {
                    continue;
                }
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

        let mut out = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();
        for (mtime, path) in files {
            if out.len() >= max {
                break;
            }
            seen.push(path.clone());
            if let Some(info) = self.info_for(&path, &mtime) {
                // 子代理会话不占侧栏行：它们只从父会话进入（044 跳转），
                // 否则每次委派都在列表里多出一行子代理的首条 prompt。
                let is_child = self.entries.get(&path).is_some_and(IndexEntry::is_child);
                if !is_child {
                    out.push(info);
                }
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
        // 活跃会话文件每轮都在变（用户每发一条消息 ListDirty 就重扫
        // 一次），把旧条目传下去让计数只读新增后缀——全量流读几百 MB
        // 的会话文件发生在主线程就是「发送一条消息窗口就冻死」的实绩。
        let prev = self.entries.get(path).cloned();
        let entry = scan_file(path, *mtime, size, prev.as_ref())?;
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
    if f.seek(SeekFrom::End(-(window as i64))).is_err() {
        return String::new();
    }
    let mut buf = Vec::with_capacity(window as usize);
    if f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Streaming message count + latest session name — one pass, no
/// whole-file allocation.
fn count_messages(path: &Path, start_byte: u64, base: u64) -> (u64, u64, Option<String>) {
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
fn preview_from_partial_line(line: &str) -> Option<String> {
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

/// One bounded scan of a session file: header + preview from the prefix,
/// name + count from one streaming pass. Never reads the whole file into
/// memory at once.
///
/// `prev` = 旧索引条目（指纹失效前的版本）：pi 会话文件只追加，计数从
/// `count_bytes` 续读新增后缀即可；文件缩水（外部改写，非 pi 行为）才
/// 全量重数。活跃会话文件每轮都在变，这里的增量化直接决定发送消息时
/// 主线程重扫的卡顿。
fn scan_file(
    path: &Path,
    modified: SystemTime,
    size: u64,
    prev: Option<&IndexEntry>,
) -> Option<IndexEntry> {
    let prefix = read_prefix(path, PREFIX_BYTES);
    let mut id = String::new();
    let mut cwd = String::new();
    let mut child_marker = false;
    let mut preview = String::new();
    for line in prefix.lines().take(PREVIEW_LINES) {
        let Ok(v) = crate::json::parse_value(line) else {
            // Prefix-truncated line: salvage a 50-char preview from the
            // raw text (only ever fills preview while it's still empty).
            if preview.is_empty() {
                if let Some(p) = preview_from_partial_line(line) {
                    preview = p;
                }
            }
            continue;
        };
        match v["type"].as_str() {
            Some("session") => {
                id = v["id"].as_str().unwrap_or("").to_string();
                cwd = v["cwd"].as_str().unwrap_or("").to_string();
            }
            Some("custom") => {
                // 子代理标记（扩展在子会话 session_start 时 appendEntry，位置
                // 紧随 header，必在 prefix 窗口内）。header 的 parentSession
                // 不可用作判定：pi 原生 fork 的分支同样带它。
                if v["customType"].as_str() == Some(SUBAGENT_CHILD_MARKER) {
                    child_marker = true;
                }
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
    // 名字与计数同一次流式扫描产出（口径同 pi getSessionName：最后一个
    // session_info 生效）。改名条目会被后续聊天推出任何固定窗口——旧版
    // 1MB 尾窗上限把改名"改回去"就是根因——所以不再独立找名字。
    let incremental = prev.is_some_and(|p| p.count_bytes > 0 && size >= p.count_bytes);
    let (message_count, count_bytes, scanned_name) = if incremental {
        // 只追加：从旧计数终点续读。count_bytes == 0 的旧版索引条目不算——
        // 那时的 message_count 是旧文件的全量计数，从 0 续读会把旧行重复
        // 计入，必须全量重数一次后落盘
        let p = prev.expect("incremental implies prev");
        count_messages(path, p.count_bytes, p.message_count)
    } else {
        // 文件缩水 / 无可靠基线 = 外部改写，全量重数（名字同样以全量为
        // 准，旧条目名字不可信）
        count_messages(path, 0, 0)
    };
    let name = if incremental {
        // 增量后缀里出现新改名就用新的，否则沿用旧条目的名字
        scanned_name.or_else(|| prev.and_then(|p| p.name.clone()))
    } else {
        scanned_name
    };
    Some(IndexEntry {
        mtime_sec: dur.as_secs(),
        mtime_nsec: dur.subsec_nanos(),
        size,
        id,
        cwd,
        preview,
        name,
        message_count,
        count_bytes,
        child_marker,
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
    /// 该会话所属项目的 cwd（会话文件首行 `type=="session"` 的 `cwd`；
    /// 老文件缺失时回落 group 目录名解码）。跨项目全量搜索时上屏用。
    pub project: Option<String>,
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
///
/// **全量**：所有项目 group 目录一起搜（2026-10-21 用户定案——会话查询不能
/// 只看当前项目），结果按会话文件 mtime 倒序排（跨项目混排、最近会话靠前），
/// 同一会话的命中仍然连在一起。
pub fn search_sessions_all(query: &str) -> SearchResponse {
    match sessions_root() {
        Some(root) => search_all_in_root(&root, query),
        None => SearchResponse {
            hits: Vec::new(),
            truncated: true,
        },
    }
}

/// 全量搜索的可测入口（root 由调用方给，测试不碰真实 sessions 目录）。
pub fn search_all_in_root(root: &Path, query: &str) -> SearchResponse {
    let Ok(rd) = std::fs::read_dir(root) else {
        return SearchResponse::default();
    };
    let groups: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    search_in_groups(&groups, query)
}

/// 单项目（按 cwd 找 group 目录）——pi-web 原范围，保留给调用方/测试。
pub fn search_sessions_for_cwd(cwd: &str, query: &str) -> SearchResponse {
    match sessions_root() {
        Some(root) => search_sessions_in_dir(&root.join(group_name_for_cwd(cwd)), query),
        None => SearchResponse {
            hits: Vec::new(),
            truncated: true,
        },
    }
}

/// The scan itself, over one project group directory (testable without
/// touching the real sessions root).
pub fn search_sessions_in_dir(group: &Path, query: &str) -> SearchResponse {
    search_in_groups(std::slice::from_ref(&group.to_path_buf()), query)
}

/// 多 group 扫描：收集所有会话文件 → 全局按 mtime 倒序 → 逐文件扫。
fn search_in_groups(groups: &[PathBuf], query: &str) -> SearchResponse {
    let mut resp = SearchResponse::default();
    let needle = query.trim();
    if needle.is_empty() {
        return resp;
    }
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for group in groups {
        let Ok(rd) = std::fs::read_dir(group) else {
            continue;
        };
        for f in rd.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = f.metadata() else { continue };
            files.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), path));
        }
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
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let mut session_name: Option<String> = None;
        // 项目 cwd：会话文件首行 `{"type":"session","cwd":...}`；老文件/截断
        // 时回落 group 目录名（`--D--ai_workspace-pi-flash--` → 去掉 `--` 外壳）
        let mut project: Option<String> = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().trim_matches('-').to_string())
            .filter(|s| !s.is_empty());
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
            let Ok(entry) = crate::json::parse_value(line) else {
                continue;
            };
            // pi 落盘的 session_info：name 在顶层（不是 content.name——
            // 旧写法永远取不到，搜索结果标题从来只显示预览）
            if entry["type"] == "session" {
                if let Some(cwd) = entry["cwd"].as_str() {
                    project = Some(cwd.to_string());
                }
                continue;
            }
            if entry["type"] == "session_info" {
                session_name = entry["name"].as_str().map(str::to_string).or(session_name);
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
                project: project.clone(),
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
    let ms = if b.len() >= 23 && b[19] == b'.' {
        num(20..23)?
    } else {
        0
    };
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

    #[test]
    fn is_subagent_session_keys_on_head_marker() {
        let root = temp_root("subagent-marker");
        // 子会话形状：header + marker + 正文（扩展 session_start 即写 marker）
        let child = write_session(
            &root,
            "D:\\proj-c",
            "child1",
            &format!(
                "{{\"type\":\"custom\",\"customType\":\"subagent/child\",\"id\":\"m1\",\"data\":{{\"provider\":\"spawn\",\"depth\":1}}}}\n{BODY}"
            ),
        );
        assert!(is_subagent_session(&child));
        // 普通会话：无 marker → false（即使带 parentSession header——pi 原生
        // fork 同样有它，不可作判定）
        let normal = write_session(&root, "D:\\proj-c", "normal1", BODY);
        assert!(!is_subagent_session(&normal));
        let forked = write_session(
            &root,
            "D:\\proj-c",
            "fork1",
            &format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"fk\",\"cwd\":\"D:\\proj-c\",\"parentSession\":\"D:\\proj.jsonl\"}}\n{BODY}"
            ),
        );
        assert!(
            !is_subagent_session(&forked),
            "parentSession header 不是判定据"
        );
        // 不存在的文件 → false（按普通会话开，行为不变）
        assert!(!is_subagent_session(&root.join("missing.jsonl")));
        let _ = std::fs::remove_dir_all(&root);
    }

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
    fn all_projects_search_spans_groups() {
        // 2026-10-21 用户定案：会话查询搜**全部**会话，不只看当前项目
        let root = temp_root("all");
        let body = concat!(
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"timestamp\":1790454123456,",
            "\"content\":[{\"type\":\"text\",\"text\":\"跨项目 hello 世界\"}]}}\n"
        );
        let a = write_session(&root, r"D:\proj-a", "a1", body);
        let b = write_session(&root, r"D:\proj-b", "b1", body);
        // 让 proj-b 的会话更新一些，验证跨项目按 mtime 倒序
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&a)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let resp = search_all_in_root(&root, "世界");
        assert!(!resp.truncated, "两个 group 都在预算内");
        assert_eq!(resp.hits.len(), 2, "两个项目的会话都要命中");
        assert_eq!(resp.hits[0].session_path, b, "更新的会话（proj-b）排在前");
        assert_eq!(resp.hits[0].project.as_deref(), Some(r"D:\proj-b"));
        assert_eq!(resp.hits[1].session_path, a);
        assert_eq!(resp.hits[1].project.as_deref(), Some(r"D:\proj-a"));
        // 单项目入口仍然只搜自己的 group
        let only_a = search_sessions_in_dir(&root.join(group_name_for_cwd(r"D:\proj-a")), "世界");
        assert_eq!(only_a.hits.len(), 1);
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
    fn stale_session_files_picks_old_jsonl_only() {
        // 052：只按 mtime 挑超期候选——老 jsonl 入选，新会话 / 非 jsonl /
        // 直接落在 root 的散文件都不入选
        let root = temp_root("stale");
        let old = write_session(&root, "D:\\proj-a", "old1", BODY);
        let fresh = write_session(&root, "D:\\proj-a", "new1", BODY);
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86_400);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&old)
            .unwrap()
            .set_modified(past)
            .unwrap();
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86_400);
        let hits = stale_session_files(&root, cutoff);
        assert!(
            hits.iter().any(|p| p == &old),
            "mtime 早于 cutoff 的会话入选"
        );
        assert!(!hits.iter().any(|p| p == &fresh), "新会话不入选");
        let group = root.join(group_name_for_cwd("D:\\proj-a"));
        std::fs::write(group.join("notes.txt"), "x").unwrap();
        std::fs::write(root.join("loose.jsonl"), "{}").unwrap();
        let hits2 = stale_session_files(&root, cutoff);
        assert_eq!(hits2.len(), hits.len(), "散文件/非 jsonl 不入选");
        assert!(
            hits2
                .iter()
                .all(|p| p.extension().is_some_and(|e| e == "jsonl")),
            "结果全是 jsonl"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_parses_summary_name_and_count() {
        let root = temp_root("scan");
        let path = write_session(&root, "D:\\proj", "abc", BODY);
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let e = scan_file(&path, mtime, 42, None).expect("scan ok");
        assert_eq!(e.id, "abc");
        assert_eq!(e.cwd, "D:\\proj");
        assert_eq!(e.preview, "hello 世界");
        assert_eq!(e.name.as_deref(), Some("renamed"));
        assert_eq!(e.message_count, 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_survives_growth_beyond_old_tail_window() {
        // 回归：改名条目被 1MB+ 后续聊天推出旧版尾窗上限后，侧栏标题会
        // "改回去"（回落首条用户消息预览）。名字现随计数流全量扫描，
        // 与条目在文件中的位置无关。
        let root = temp_root("rename-growth");
        let pad = json!({"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"padpadpadpadpadpadpadpadpadpad"}]}})
            .to_string();
        let mut body = String::from(BODY);
        for _ in 0..12_000 {
            body.push_str(&pad);
            body.push('\n');
        }
        assert!(body.len() > 1_048_576, "必须真超过旧 1MB 尾窗上限");
        let path = write_session(&root, "D:\\proj-g", "g1", &body);
        let meta = std::fs::metadata(&path).unwrap();
        let e = scan_file(&path, meta.modified().unwrap(), meta.len(), None).expect("scan ok");
        assert_eq!(e.name.as_deref(), Some("renamed"));
        assert_eq!(e.message_count, 3 + 12_000);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn incremental_rescan_tracks_new_rename_and_keeps_old_name() {
        let root = temp_root("rename-incr");
        let path = write_session(&root, "D:\\proj-r", "r1", BODY);
        let meta = std::fs::metadata(&path).unwrap();
        let e1 = scan_file(&path, meta.modified().unwrap(), meta.len(), None).expect("scan ok");
        assert_eq!(e1.name.as_deref(), Some("renamed"));
        use std::io::Write;
        // 追加新改名 + 一条消息 → 增量续读后缀，名字更新
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(
                concat!(
                    "{\"type\":\"session_info\",\"name\":\"renamed-2\"}\n",
                    "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"more\"}}\n"
                )
                .as_bytes(),
            )
            .unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let e2 =
            scan_file(&path, meta.modified().unwrap(), meta.len(), Some(&e1)).expect("scan ok");
        assert_eq!(e2.name.as_deref(), Some("renamed-2"));
        assert_eq!(e2.message_count, e1.message_count + 1);
        // 再追加一条普通消息（无改名）→ 沿用旧名，不回落预览
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(
                b"{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"even more\"}}\n",
            )
            .unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let e3 =
            scan_file(&path, meta.modified().unwrap(), meta.len(), Some(&e2)).expect("scan ok");
        assert_eq!(e3.name.as_deref(), Some("renamed-2"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn needle_substring_in_other_entries_never_shadows_rename() {
        // 改名之后的行里出现 `"type":"session_info"` 裸子串（嵌套结构，
        // 比如讨论本仓源码的会话），不得冒充改名条目——解析校验 type 后
        // 才认。旧尾窗 rfind 版会把子串所在行当条目行，名字丢失。
        let root = temp_root("rename-shadow");
        let body = concat!(
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
            "{\"type\":\"session_info\",\"name\":\"real name\"}\n",
            "{\"type\":\"custom\",\"customType\":\"x\",\"details\":{\"type\":\"session_info\"}}\n",
        );
        let path = write_session(&root, "D:\\proj-s2", "s2", body);
        let meta = std::fs::metadata(&path).unwrap();
        let e = scan_file(&path, meta.modified().unwrap(), meta.len(), None).expect("scan ok");
        assert_eq!(e.name.as_deref(), Some("real name"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_salvages_preview_when_first_user_line_truncated() {
        // Real-world shape: small header/meta lines, a fat system-prompt
        // line, then a first user message so big the 256KB prefix cuts its
        // line — full parse fails, the degraded path must still surface
        // the leading text (capped at PARTIAL_PREVIEW_CHARS).
        let root = temp_root("scan-trunc");
        let big = "甲".repeat(200_000); // 600KB utf8 >> PREFIX_BYTES
        let body = format!(
            concat!(
                "{{\"type\":\"model_change\",\"id\":\"m\"}}\n",
                "{{\"type\":\"message\",\"id\":\"s\",\"message\":{{\"role\":\"system\",\"content\":\"{}\"}}}}\n",
                "{{\"type\":\"message\",\"id\":\"u1\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}}}\n",
            ),
            "sys ".repeat(30_000), // ~120KB system line
            big,
        );
        let path = write_session(&root, "D:\\proj-trunc", "trunc1", &body);
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let e = scan_file(&path, mtime, body.len() as u64, None).expect("scan ok");
        let want: String = "甲".repeat(PARTIAL_PREVIEW_CHARS.min(40)); // 50 chars = 150B > 120B display cap → re-truncated with …
        assert_eq!(e.preview, format!("{want}…"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn partial_line_preview_rejects_assistant_and_bad_anchors() {
        // assistant line quoting the literal must not yield a preview
        let a = "{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":\"see \\\"role\\\":\\\"user\\\" docs and \\\"text\\\":\\\"spoiled\"";
        assert_eq!(preview_from_partial_line(a), None);
        // role marker after content (inside quoted text) is rejected
        let late = "{\"message\":{\"content\":\"junk\"},\"role\":\"user\"}";
        assert_eq!(preview_from_partial_line(late), None);
        // proper string-content user line, escapes decoded, \n folded;
        // result capped at exactly 50 chars
        let ok = "{\"message\":{\"role\":\"user\",\"content\":\"line1\\nline2 \\\"quoted\\\" rest of a fairly long first message";
        assert_eq!(
            preview_from_partial_line(ok).as_deref(),
            Some("line1 line2 \"quoted\" rest of a fairly long first m")
        );
        // empty text → None (caller keeps scanning later lines)
        let empty = "{\"message\":{\"role\":\"user\",\"content\":\"\"}";
        assert_eq!(preview_from_partial_line(empty), None);
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

    /// 行路径恒为 on-disk 形态：调用方 cwd 大小写漂移（projects.json 里的
    /// 小写 `d:` vs 盘上的 `D:`）不得带进 SessionInfo.path——否则与 pi 自报
    /// 路径 / 通知点集合互相失配（行转圈、完成点全哑的真机回归根因）。
    #[test]
    fn list_for_cwd_paths_are_on_disk_form() {
        let root = temp_root("canon-list");
        write_session(&root, "D:\\proj-canon", "c1", BODY);
        let mut s = Scanner::open(&root);
        let rows = s.list_for_cwd("d:\\proj-canon", 10);
        assert_eq!(rows.len(), 1);
        // write_session 用大写 cwd 建 group；小写 cwd 拉列表也必须产大写形态
        assert_eq!(
            rows[0].path,
            root.join("--D--proj-canon--")
                .join(rows[0].path.file_name().unwrap())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// pi 按 cwd 自拼的 group 形态（小写段）→ on-disk 形态；幂等；
    /// root 外 / 无 group 段 / group 不在盘上 → 原样返回。
    #[test]
    fn canonical_session_path_maps_reported_form_to_on_disk_group() {
        let root = temp_root("canon");
        let on_disk = root.join("--D--proj--");
        std::fs::create_dir_all(&on_disk).unwrap();
        let name = "2026-10-19T00-00-00-000Z_x.jsonl";
        std::fs::write(on_disk.join(name), "{}\n").unwrap();
        let reported = root.join("--d--proj--").join(name);
        assert_eq!(
            canonical_session_path_in(&root, &reported),
            on_disk.join(name)
        );
        // 已是 on-disk 形态：幂等
        assert_eq!(
            canonical_session_path_in(&root, &on_disk.join(name)),
            on_disk.join(name)
        );
        // root 外：原样
        let outside = temp_root("canon-out").join("a.jsonl");
        std::fs::write(&outside, "{}\n").unwrap();
        assert_eq!(canonical_session_path_in(&root, &outside), outside);
        let _ = std::fs::remove_dir_all(outside.parent().unwrap());
        // root 本身 / 只有 group 段：原样
        assert_eq!(canonical_session_path_in(&root, &root), root);
        assert_eq!(
            canonical_session_path_in(&root, &root.join("--D--proj--")),
            root.join("--D--proj--")
        );
        // group 不在盘上：原样
        let ghost = root.join("--e--nope--").join(name);
        assert_eq!(canonical_session_path_in(&root, &ghost), ghost);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_skips_child_sessions_but_keeps_forks() {
        // 045：子代理会话靠自体内的 `subagent/child` marker 排除；pi 原生 fork
        // 分支（header 带 parentSession 但无 marker）是用户会话，必须上行。
        // 回归背景：曾用 parentSession 判定，把用户的 fork 置顶会话从侧栏全杀。
        let root = temp_root("children");
        let parent = write_session(&root, "D:\\proj-c", "parent1", BODY);
        let group = parent.parent().unwrap().to_path_buf();

        // fork 分支：header 带 parentSession，无 marker。
        let fork = group.join("2026-09-26T00-00-02-000Z_fork1.jsonl");
        std::fs::write(
            &fork,
            format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"fork1\",\"cwd\":\"D:\\\\proj-c\",\"parentSession\":\"{}\"}}\n{}",
                parent.display().to_string().replace('\\', "\\\\"),
                BODY
            ),
        )
        .unwrap();

        // subagent 子会话：header 带 parentSession + 自体 marker entry。
        let child = group.join("2026-09-26T00-00-01-000Z_child1.jsonl");
        std::fs::write(
            &child,
            format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"child1\",\"cwd\":\"D:\\\\proj-c\",\"parentSession\":\"{}\"}}\n{{\"type\":\"custom\",\"customType\":\"subagent/child\",\"id\":\"m1\",\"data\":{{\"provider\":\"spawn\",\"depth\":1}}}}\n{}",
                parent.display().to_string().replace('\\', "\\\\"),
                BODY
            ),
        )
        .unwrap();

        let mut s = Scanner::open(&root);
        let mut listed = s.list_for_cwd("D:\\proj-c", 10);
        listed.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(
            listed.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["fork1", "parent1"],
            "fork 上行、子代理被排除: {listed:?}"
        );

        // 子会话文件本身仍可解析，且 marker 被识别。
        let mtime = std::fs::metadata(&child).unwrap().modified().unwrap();
        let entry = scan_file(
            &child,
            mtime,
            std::fs::metadata(&child).unwrap().len(),
            None,
        )
        .expect("child scan ok");
        assert!(entry.is_child(), "marker 应被识别为子会话");
        let fmtime = std::fs::metadata(&fork).unwrap().modified().unwrap();
        let fork_entry = scan_file(&fork, fmtime, std::fs::metadata(&fork).unwrap().len(), None)
            .expect("fork scan ok");
        assert!(!fork_entry.is_child(), "fork 无 marker，不是子会话");
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
    fn incremental_count_survives_append_and_partial_tail() {
        // 052 后活跃会话文件每轮重扫（用户每发一条消息 ListDirty 一次），
        // 计数必须只读新增后缀——全量流读几百 MB 会话文件发生在主线程就是
        // 「发送一条消息窗口就冻死」的实绩。半行（pi 正在写）不计、不推进。
        use std::io::Write as _;
        let root = temp_root("inccount");
        let path = write_session(&root, r"D:\proj", "ic1", BODY);
        let mut s = Scanner::open(&root);
        let first = s.list(10);
        let base_count = first[0].message_count;
        let base_bytes = s.entries.get(&path).unwrap().count_bytes;
        assert_eq!(base_count, 3);
        assert!(base_bytes > 0, "首扫即记录计数终点");

        // 追加 2 条 + 一条未写完的半行（无换行结尾）
        let mut full = String::new();
        for i in 0..2 {
            full.push_str(&format!(
                "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"m{i}\"}}}}
"
            ));
        }
        let partial = "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"half";
        let app = format!("{full}{partial}");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(app.as_bytes())
            .unwrap();

        let second = s.list(10);
        assert_eq!(
            second[0].message_count,
            base_count + 2,
            "完整行计入，半行不计"
        );
        let mid_bytes = s.entries.get(&path).unwrap().count_bytes;
        assert_eq!(
            mid_bytes,
            base_bytes + full.len() as u64,
            "计数终点停在半行之前"
        );

        // 半行写完 → 下次扫描补计这一条
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(
                "\"}}
"
                .as_bytes(),
            )
            .unwrap();
        let third = s.list(10);
        assert_eq!(third[0].message_count, base_count + 3, "补齐的行下次补计");
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
    fn index_version_bump_forces_full_rescan() {
        let root = temp_root("idx-ver");
        let path = write_session(&root, "D:\\proj", "v1", BODY);
        let idx = root.join("index.json");
        // hand-written v1 index (previous schema) with a stale empty preview
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let dur = mtime.duration_since(UNIX_EPOCH).unwrap();
        let stale = serde_json::json!({
            "version": 1,
            "entries": {
                path.to_string_lossy(): {
                    "mtime_sec": dur.as_secs(), "mtime_nsec": dur.subsec_nanos(),
                    "size": std::fs::metadata(&path).unwrap().len(),
                    "id": "v1", "cwd": "D:\\proj", "preview": "",
                    "name": null, "message_count": 3,
                }
            }
        });
        crate::config::write_json(&idx, &stale).unwrap();
        let mut b = Scanner::open_persisted(&root, &idx);
        let list = b.list(10);
        assert_eq!(b.scans, 1, "v1 cache discarded → file rescanned");
        assert_eq!(list[0].preview, "hello 世界", "fresh scan, not stale cache");
        b.save_index();
        let saved = crate::config::read_json(&idx).unwrap();
        assert_eq!(saved["version"].as_u64(), Some(INDEX_VERSION));
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
        let first = msgs.first().unwrap()["message"]["content"]
            .as_str()
            .unwrap();
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
        let e = scan_file(&p, meta.modified().unwrap(), meta.len(), None).expect("session parsed");
        eprintln!(
            "parsed name={:?} preview={} count={}",
            e.name, e.preview, e.message_count
        );
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
            .map(|v| {
                v["message"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        assert_eq!(texts, vec!["hi", "first", "again", "stranded tail"]);

        // max trims from the head, keeps the newest
        let trimmed = read_leaf_messages(&path, 2);
        let texts: Vec<String> = trimmed
            .iter()
            .map(|v| {
                v["message"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        assert_eq!(texts, vec!["again", "stranded tail"]);

        std::fs::remove_file(&path).ok();
    }
}
