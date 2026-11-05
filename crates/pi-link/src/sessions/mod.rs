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
//!
//! 按关注点分文件（超规重构 itlq.6，只搬不改）：`info` 会话身份/路径归一、
//! `scan` 指纹索引扫描器、`read` 有界读与公开读 API、`search` 内容搜索、
//! `testing` 测试夹具。

mod info;
mod read;
mod scan;
mod search;

#[cfg(test)]
pub(crate) mod testing;

pub use info::{
    SessionInfo, canonical_session_path, canonical_session_path_in, group_name_for_cwd,
    is_subagent_session, sessions_root,
};
pub use read::{
    count_message_entries, list_sessions, list_sessions_excluding, list_sessions_for_cwd,
    read_leaf_messages, read_tail_messages, scan_diagnostics, sessions_for_paths,
    stale_session_files,
};
pub use scan::Scanner;
pub use search::{
    SEARCH_MAX_RESULTS, SearchHit, SearchResponse, search_all_in_root, search_sessions_all,
    search_sessions_for_cwd, search_sessions_in_dir,
};
