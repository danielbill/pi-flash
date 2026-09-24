//! Session discovery over `~/.pi/agent/sessions/` (docs/sessions.md).
//!
//! Layout: `<sessions>/--<cwd-with-separators-dashed>--/<timestamp>_<id>.jsonl`,
//! first line is a header `{"type":"session","version":3,"id":...,"cwd":...}`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub id: String,
    pub cwd: String,
    pub modified: SystemTime,
    /// first user message text (used as the row label, like pi-web)
    pub preview: String,
    pub message_count: u64,
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

/// List most-recently-modified sessions, newest first.
pub fn list_sessions(max: usize) -> Vec<SessionInfo> {
    let Some(root) = sessions_root() else { return Vec::new() };
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    let Ok(groups) = std::fs::read_dir(&root) else { return Vec::new() };
    for group in groups.flatten() {
        let Ok(entries) = std::fs::read_dir(group.path()) else { continue };
        for f in entries.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(meta) = f.metadata() {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((mtime, path));
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(max);

    let mut out = Vec::new();
    for (modified, path) in files {
        if let Some(info) = read_session(&path, modified) {
            out.push(info);
        }
    }
    out
}

fn read_session(path: &Path, modified: SystemTime) -> Option<SessionInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut id = String::new();
    let mut cwd = String::new();
    let mut preview = String::new();
    let message_count = (u64::from(content.contains("\"type\":\"message\"")))
        * content.matches("\"type\":\"message\"").count() as u64;
    // header + first user message are near the top; read a bounded prefix
    for line in content.lines().take(40) {
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
                                (b["type"] == "text").then(|| b["text"].as_str().unwrap_or("").to_string())
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
    Some(SessionInfo {
        path: path.to_path_buf(),
        id,
        cwd,
        modified,
        preview,
        message_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_real_sessions_when_present() {
        // machine-dependent smoke test: only asserts invariants, never fails
        // when no sessions exist (e.g. fresh CI machine)
        let sessions = list_sessions(50);
        for s in &sessions {
            assert!(!s.id.is_empty());
            assert!(s.path.exists());
        }
    }
}
