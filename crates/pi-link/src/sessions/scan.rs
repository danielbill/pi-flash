//! 指纹索引扫描器：`(mtime, size)` 指纹 + 有界前缀/流式计数，索引内存与
//! 磁盘双缓存（pi-web scanner-index parity）。

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::info::{SUBAGENT_CHILD_MARKER, SessionInfo, canonical_group_dir, group_name_for_cwd};
use super::read::{count_messages, preview_from_partial_line, read_prefix};

/// Bounded prefix parsed for header + first user message (preview).
const PREFIX_BYTES: u64 = 256 * 1024;
/// Lines of the prefix considered when looking for the preview message.
const PREVIEW_LINES: usize = 40;
/// 索引条目上限（047-内存削减 P1-5）：内存 + session-index.json 双份持有，
/// 每条 ≈300-600B——10 万会话 ≈ 30-60MB。超限按 mtime 升序截断保留最新
/// （正常用户离这个量级很远；上限只为多年累积的极端兜底）。
const INDEX_ENTRY_CAP: usize = 50_000;

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
pub(super) struct IndexEntry {
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
    pub(super) entries: HashMap<PathBuf, IndexEntry>,
    dirty: bool,
    /// files actually scanned since `Scanner` creation (diagnostics/tests)
    pub(crate) scans: u64,
    /// 启动后首次 list 已做过全库 prune（047-P1-5，一次性开关）
    full_pruned: bool,
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
            full_pruned: false,
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

        // 进程生命周期内首次 list：对整份索引做一次全库死行对账（纯
        // exists() 批量，无内容读）——prune_missing 只覆盖被枚举的 group，
        // 从未枚举过的 group 的死行永不清（047-P1-5）
        self.full_prune_once();

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
        self.cap_entries();
        if self.dirty {
            self.save_index();
            self.dirty = false;
        }
        out
    }

    /// Cached-or-scan lookup keyed by the `(mtime, size)` fingerprint.
    pub(super) fn info_for(&mut self, path: &Path, mtime: &SystemTime) -> Option<SessionInfo> {
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

    /// 全库死行对账（进程生命周期一次，047-P1-5）：prune_missing 只清被
    /// 枚举 group 范围内的死行，其余 group 的死行按旧口径永不清。这里对
    /// 整份 entries 做一次纯 `is_file()` 批量（5 万行 stat ≈ 百 ms 级，
    /// 每进程只付一次），死行全清。
    fn full_prune_once(&mut self) {
        if self.full_pruned {
            return;
        }
        self.full_pruned = true;
        let before = self.entries.len();
        self.entries.retain(|path, _| path.is_file());
        if self.entries.len() != before {
            self.dirty = true;
        }
    }

    /// 条目上限（047-P1-5）：超限按 (mtime_sec, mtime_nsec) 降序保留最新
    /// INDEX_ENTRY_CAP 条。只截索引缓存行，不碰磁盘文件。
    fn cap_entries(&mut self) {
        if self.entries.len() <= INDEX_ENTRY_CAP {
            return;
        }
        let mut rows: Vec<(u64, u32, PathBuf)> = self
            .entries
            .iter()
            .map(|(p, e)| (e.mtime_sec, e.mtime_nsec, p.clone()))
            .collect();
        rows.sort_unstable_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
        let keep: std::collections::HashSet<PathBuf> = rows
            .into_iter()
            .take(INDEX_ENTRY_CAP)
            .map(|(_, _, p)| p)
            .collect();
        let before = self.entries.len();
        self.entries.retain(|p, _| keep.contains(p));
        if self.entries.len() != before {
            self.dirty = true;
        }
    }
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

#[cfg(test)]
mod tests;
