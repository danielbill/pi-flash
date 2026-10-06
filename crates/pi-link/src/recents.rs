//! Recent-activity session list (docs/模块设计/003-session管理.md).
//!
//! The recents layer answers "which sessions did the user touch lately",
//! where an activity is EITHER a pf-side open (touch — always wins, set to
//! now) OR a file mtime bump from any writer (pi subprocess, pi-web, cli —
//! record, max semantics so an older mtime never rewinds a fresher open).
//! Entries carry `(path, last_active)` only; summaries (preview/name/count)
//! stay in the fingerprint index and are looked up at render time.
//!
//! Startup: the persisted list is the tail-preload source — the shell reads
//! the top-N paths, stats them and serves tails straight from disk. A first
//! run (empty list) seeds itself from the scanner's mtime ordering, which
//! unifies the fresh-machine and pre-existing-sessions cases into one path.
//!
//! A 30s poll (app side) keeps the list fresh against external writers;
//! files owned by live runtimes are excluded there — their recency arrives
//! through the event path (open / message callbacks), and polling them
//! would re-scan the file every tick while pi keeps appending to it.
//! Dirty state is flushed at most every `SAVE_THROTTLE`, with the poll
//! acting as the periodic fallback (worst case ~30s of recency loss).

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;

/// 清单容量:按活动时间排序的 top-500(30 天窗口内重度使用的硬上限)
pub const RECENTS_CAPACITY: usize = 500;
/// 轮询扫描窗口 = 容量 + 余量,给外部新文件留挤进清单的空间
pub const POLL_SCAN_WINDOW: usize = RECENTS_CAPACITY + 50;
/// 落盘节流:touch 只进内存,由窗口期满后的首次 maybe_save 落盘
const SAVE_THROTTLE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecentEntry {
    path: PathBuf,
    last_active_ms: i64,
}

#[derive(Debug)]
pub struct Recents {
    file: PathBuf,
    /// 按 last_active_ms 降序,容量 ≤ RECENTS_CAPACITY
    entries: Vec<RecentEntry>,
    dirty: bool,
    last_save: Instant,
}

impl Recents {
    /// Load from disk; a missing/corrupt file is just an empty list (first
    /// run) — seeding happens lazily in [`recent_preload_paths`]. Legacy
    /// cross-writer duplicates (same file, two path strings) are merged
    /// here, keeping the freshest activity per file.
    pub fn load(file: &Path) -> Recents {
        let mut entries = crate::config::read_json(file)
            .ok()
            .and_then(|v| serde_json::from_value::<Vec<RecentEntry>>(v["entries"].clone()).ok())
            .unwrap_or_default();
        entries.sort_by(|a, b| b.last_active_ms.cmp(&a.last_active_ms));
        let mut seen: HashSet<String> = HashSet::new();
        let before = entries.len();
        entries.retain(|e| seen.insert(path_key(&e.path)));
        let dirty = entries.len() != before;
        Recents {
            file: file.to_path_buf(),
            entries,
            dirty,
            last_save: Instant::now(),
        }
    }

    fn save(&self) {
        let value = serde_json::json!({ "version": 1, "entries": self.entries });
        // 自有目录可能还没建（首启 / 迁移后）——写前确保存在
        let _ = crate::paths::ensure_dir();
        let _ = crate::config::write_json(&self.file, &value);
    }

    /// Throttled flush: writes only when dirty and the window has elapsed.
    /// The 30s poll calls this every tick as the periodic fallback.
    pub fn maybe_save(&mut self) {
        if self.dirty && self.last_save.elapsed() >= SAVE_THROTTLE {
            self.save();
            self.dirty = false;
            self.last_save = Instant::now();
        }
    }

    /// Force a flush (bypasses the throttle) — seeding and explicit callers.
    pub fn flush(&mut self) {
        if self.dirty {
            self.save();
            self.dirty = false;
        }
        self.last_save = Instant::now();
    }

    /// Insert/promote one entry: identity by [`path_key`], activity time is
    /// max(existing, given) (touch passes now — always fresh; record passes
    /// an mtime — never rewinds a fresher open), and the freshest path
    /// string wins so cross-writer spellings of the same file converge.
    /// Returns whether anything materially changed.
    fn promote(&mut self, path: PathBuf, last_active_ms: i64) -> bool {
        let key = path_key(&path);
        let ix = self
            .entries
            .iter()
            .position(|e| path_key(&e.path) == key);
        let (prev_ms, same_string) = match ix {
            Some(i) => (self.entries[i].last_active_ms, self.entries[i].path == path),
            None => (0, false),
        };
        let ms = last_active_ms.max(prev_ms);
        let changed = ix.is_none() || ms != prev_ms || !same_string;
        if let Some(i) = ix {
            self.entries.remove(i);
        }
        let at = self
            .entries
            .partition_point(|e| e.last_active_ms > ms);
        self.entries.insert(at, RecentEntry { path, last_active_ms: ms });
        if self.entries.len() > RECENTS_CAPACITY {
            self.entries.truncate(RECENTS_CAPACITY);
        }
        if changed {
            self.dirty = true;
        }
        changed
    }

    /// pf-side open/message activity: the entry moves to the top, stamped
    /// `now`. Always wins over any recorded mtime.
    pub fn touch(&mut self, path: &Path) {
        self.promote(path.to_path_buf(), now_ms());
    }

    /// External activity (file mtime): max semantics — never rewinds a
    /// fresher open. Returns whether anything changed.
    pub fn record(&mut self, path: &Path, active_ms: i64) -> bool {
        if self
            .entries
            .iter()
            .any(|e| e.path == path && e.last_active_ms >= active_ms)
        {
            return false;
        }
        self.promote(path.to_path_buf(), active_ms)
    }

    /// Drop dead files from the list (skips excluded/live files — they are
    /// owned by the event path). Returns whether anything changed.
    pub fn prune_missing(&mut self, exclude: &HashSet<PathBuf>) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|e| exclude.contains(&e.path) || e.path.is_file());
        if self.entries.len() != before {
            self.dirty = true;
            return true;
        }
        false
    }

    /// Drop one entry explicitly (session deleted).
    pub fn remove(&mut self, path: &Path) {
        let before = self.entries.len();
        self.entries.retain(|e| e.path != path);
        if self.entries.len() != before {
            self.dirty = true;
        }
    }

    /// The newest `n` active paths, newest first.
    pub fn top_paths(&self, n: usize) -> Vec<PathBuf> {
        self.entries
            .iter()
            .take(n)
            .map(|e| e.path.clone())
            .collect()
    }

    /// Paths active within the window (recency order). The list capacity is
    /// the hard upper bound — a 30-day window on a heavy setup can outgrow
    /// it, and the psp list then simply ends at the capacity.
    pub fn window_paths(&self, window_days: u64) -> Vec<PathBuf> {
        let cutoff = now_ms() - (window_days as i64) * 86_400_000;
        self.entries
            .iter()
            .filter(|e| e.last_active_ms >= cutoff)
            .map(|e| e.path.clone())
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn now_ms() -> i64 {
    systemtime_to_ms(SystemTime::now())
}

fn systemtime_to_ms(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Path identity key. Windows paths are case-insensitive, and cross-writer
/// representations of the SAME file do occur in practice: pi (Node) hands
/// back `--d--…` group paths while the Rust scanner enumerates the stored
/// directory name `--D--…` — both strings stat the same file, and treating
/// them as two entries duplicated sessions in the list. Dedup on the
/// lowercased string; exact match elsewhere.
#[cfg(windows)]
fn path_key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

#[cfg(not(windows))]
fn path_key(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// process-wide convenience (mirrors the sessions scanner singleton)
// ---------------------------------------------------------------------------

fn with_recents<T>(f: impl FnOnce(&mut Recents) -> T) -> T {
    static RECENTS: Mutex<Option<Recents>> = Mutex::new(None);
    let mut guard = RECENTS.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        // 清单落 pi-flash 自有目录（010-启动.md §4）
        let file = crate::paths::session_recents_file()
            .unwrap_or_else(|| crate::config::agent_dir().join("session-recents.json"));
        *guard = Some(Recents::load(&file));
    }
    f(guard.as_mut().expect("recents just initialized"))
}

/// pf-side open/message activity: promote to the top, stamped now.
pub fn touch_recent(path: &Path) {
    with_recents(|r| r.touch(path));
}

/// Session deleted: drop the entry.
pub fn remove_recent(path: &Path) {
    with_recents(|r| r.remove(path));
}

/// 首启种子：清单为空（新机器，或路径迁移后新目录还空着）→ 按扫描器的
/// mtime 序记录并落盘。startup 启动时**显式**调一次（顺序可预期、失败可见）；
/// `recent_load_paths` 保留同样的兜底（幂等）。
pub fn ensure_seeded() {
    with_recents(seed_if_empty);
}

fn seed_if_empty(r: &mut Recents) {
    if r.is_empty() {
        for s in crate::sessions::list_sessions(RECENTS_CAPACITY) {
            r.record(&s.path, systemtime_to_ms(s.modified));
        }
        r.flush();
    }
}

/// Load-window source: every path active within `window_days`, recency
/// order. An empty list (first run on a machine with existing sessions)
/// seeds itself from the scanner's mtime ordering and persists, so the next
/// start is list-served. Callers page the result for display and may take
/// a small prefix for tail preload.
pub fn recent_load_paths(window_days: u64) -> Vec<PathBuf> {
    with_recents(|r| {
        seed_if_empty(r);
        r.window_paths(window_days)
    })
}

/// 30s poll: external-writer reconciliation over the sessions root.
/// Enumerates the tree (metadata only), records mtime bumps with max
/// semantics, prunes dead entries and saves. Files in `exclude` (live
/// runtimes' files) are skipped entirely — the event path owns their
/// recency, and scanning them while pi appends would rescan every tick.
/// Returns whether the list changed (callers may refresh UI).
pub fn poll_recent_sessions(exclude: &[PathBuf]) -> bool {
    let exclude: HashSet<PathBuf> = exclude.iter().cloned().collect();
    let infos = crate::sessions::list_sessions_excluding(POLL_SCAN_WINDOW, &exclude);
    with_recents(|r| {
        let mut changed = false;
        for s in &infos {
            changed |= r.record(&s.path, systemtime_to_ms(s.modified));
        }
        changed |= r.prune_missing(&exclude);
        r.maybe_save();
        changed
    })
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pi-link-recents-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("recents.json")
    }

    fn p(tag: &str) -> PathBuf {
        PathBuf::from(format!("C:\\tmp\\{tag}.jsonl"))
    }

    #[test]
    fn touch_promotes_and_caps() {
        let mut r = Recents::load(&temp_file("cap"));
        let now = now_ms();
        // inject descending timestamps via promote (500 real touches would
        // need monotonic sleeps); newest first
        for i in 0..(RECENTS_CAPACITY + 10) {
            r.promote(p(&format!("s{i}")), now - i as i64);
        }
        assert_eq!(r.entries.len(), RECENTS_CAPACITY);
        assert_eq!(r.top_paths(1)[0], p("s0"), "newest entry first");
        assert!(r.top_paths(RECENTS_CAPACITY + 5).len() == RECENTS_CAPACITY);
    }

    #[test]
    fn window_filters_by_recency() {
        let mut r = Recents::load(&temp_file("win"));
        let now = now_ms();
        let day = 86_400_000i64;
        r.promote(p("fresh"), now - 3 * day);
        r.promote(p("mid"), now - 10 * day);
        r.promote(p("old"), now - 40 * day);
        let w7: Vec<_> = r.window_paths(7);
        assert_eq!(w7, vec![p("fresh")]);
        let w14: Vec<_> = r.window_paths(14);
        assert_eq!(w14, vec![p("fresh"), p("mid")], "recency order");
        let w30: Vec<_> = r.window_paths(30);
        assert_eq!(w30, vec![p("fresh"), p("mid")]);
    }

    #[test]
    fn record_max_semantics() {
        let mut r = Recents::load(&temp_file("max"));
        r.touch(&p("open")); // opened now (large timestamp)
        let opened_top = r.top_paths(1)[0].clone();
        // an old external mtime must not rewind the fresher open
        assert!(!r.record(&p("open"), 1_000));
        assert_eq!(r.top_paths(1)[0], opened_top);
        // a fresh external mtime on another file takes the top
        assert!(r.record(&p("ext"), 9_999_999_999_999));
        assert_eq!(r.top_paths(1)[0], p("ext"));
        // re-recording the same stamp is a no-op
        assert!(!r.record(&p("ext"), 9_999_999_999_999));
    }

    #[test]
    fn prune_and_remove_drop_dead_paths() {
        let mut r = Recents::load(&temp_file("prune"));
        r.touch(&p("a"));
        r.touch(&p("b"));
        // "a" doesn't exist on disk, "b" does
        let real = std::env::temp_dir().join("pi-link-recents-real.jsonl");
        std::fs::write(&real, b"{}").unwrap();
        r.entries[0].path = real.clone();
        let mut exclude = HashSet::new();
        assert!(r.prune_missing(&exclude));
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].path, real);
        // excluded paths survive a failed stat (live runtime owns them)
        exclude.insert(p("ghost"));
        r.entries.insert(0, RecentEntry { path: p("ghost"), last_active_ms: 5 });
        r.prune_missing(&exclude);
        assert_eq!(r.entries.len(), 2);
        r.remove(&real);
        assert_eq!(r.entries.len(), 1);
        let _ = std::fs::remove_file(&real);
    }

    #[test]
    fn persisted_roundtrip() {
        let file = temp_file("round");
        {
            let mut r = Recents::load(&file);
            r.touch(&p("x1"));
            r.flush();
        }
        let r = Recents::load(&file);
        assert_eq!(r.top_paths(1), vec![p("x1")]);
        let _ = std::fs::remove_file(&file);
    }

    /// Real-case regression: pi (Node) returns `--d--…` group paths while
    /// the Rust scanner enumerates the stored name `--D--…` — Win32 stats
    /// the same file for both strings, and the list showed the session
    /// twice. Identity must be case-insensitive on Windows, and the
    /// freshest spelling must win.
    #[cfg(windows)]
    #[test]
    fn case_insensitive_identity_converges_cross_writer_paths() {
        let node_spelling =
            PathBuf::from("C:\\u\\.pi\\agent\\sessions\\--d--ai_workspace-pi_work--\\s1.jsonl");
        let scan_spelling =
            PathBuf::from("C:\\u\\.pi\\agent\\sessions\\--D--ai_workspace-pi_work--\\s1.jsonl");
        let mut r = Recents::load(&temp_file("case"));
        r.touch(&node_spelling);
        r.touch(&scan_spelling);
        assert_eq!(r.entries.len(), 1, "same file, one entry");
        assert_eq!(r.entries[0].path, scan_spelling, "freshest spelling wins");

        // legacy on-disk duplicates merge at load and get flushed clean
        let file = temp_file("case2");
        {
            let mut r = Recents::load(&file);
            r.touch(&node_spelling);
            std::thread::sleep(std::time::Duration::from_millis(2));
            r.touch(&scan_spelling);
            r.flush();
        }
        let r = Recents::load(&file);
        assert_eq!(r.entries.len(), 1, "stored duplicates merged");
        assert_eq!(r.top_paths(1), vec![scan_spelling]);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn scanner_excluding_skips_excluded_files() {
        let dir = std::env::temp_dir().join(format!(
            "pi-link-recents-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let a = crate::sessions::testing::write_session(&dir, "D:\\proj-r", "ra", "{}\n");
        let b = crate::sessions::testing::write_session(&dir, "D:\\proj-r", "rb", "{}\n");
        let mut s = crate::sessions::Scanner::open(&dir);
        let mut exclude = HashSet::new();
        exclude.insert(b.clone());
        let out = s.list_excluding(10, &exclude);
        assert_eq!(out.len(), 1, "excluded file not listed");
        assert_eq!(out[0].path, a);
        assert_eq!(s.scans, 1, "excluded file never scanned");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
