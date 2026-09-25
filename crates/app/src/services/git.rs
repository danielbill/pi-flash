//! git status/diff helpers (pi-web lib/git-changes.ts parity).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl GitStatus {
    pub fn badge(&self) -> &'static str {
        match self {
            GitStatus::Modified => "M",
            GitStatus::Added => "A",
            GitStatus::Deleted => "D",
            GitStatus::Renamed => "R",
            GitStatus::Untracked => "U",
            GitStatus::Conflict => "C",
        }
    }

    pub fn color(&self) -> u32 {
        match self {
            GitStatus::Modified => 0xd6a84b,
            GitStatus::Added | GitStatus::Untracked => 0x4ade80,
            GitStatus::Deleted | GitStatus::Conflict => 0xf87171,
            GitStatus::Renamed => 0x60a5fa,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitFile {
    pub path: PathBuf,
    pub status: GitStatus,
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// `git status --porcelain=v1 -z --untracked-files=all` parsed to files.
pub fn git_status_files(cwd: &Path) -> Vec<GitFile> {
    let Some(out) = run_git(cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=all"])
    else {
        return Vec::new();
    };
    let mut files = Vec::new();
    // -z: NUL-separated records; rename records embed a TAB then the new name
    for rec in out.split('\0') {
        if rec.len() < 4 {
            continue;
        }
        let xy = &rec[..2];
        let rest = &rec[3..];
        let (status, file_path) = match xy {
            "??" => (GitStatus::Untracked, rest),
            _ => {
                let x = xy.as_bytes()[0];
                let y = xy.as_bytes()[1];
                let st = if x == b'D' || y == b'D' {
                    GitStatus::Deleted
                } else if x == b'A' {
                    GitStatus::Added
                } else if x == b'R' || y == b'R' {
                    GitStatus::Renamed
                } else if x == b'U' || y == b'U' || (x == b'A' && y == b'A') {
                    GitStatus::Conflict
                } else {
                    GitStatus::Modified
                };
                // renames: "orig	new" — track the new name
                let p = match rest.split_once('\t') {
                    Some((_, new)) => new,
                    None => rest,
                };
                (st, p)
            }
        };
        let full = cwd.join(file_path);
        files.push(GitFile { path: full, status });
    }
    files
}

/// Total (+, -) line counts of tracked changes (numstat HEAD summary).
pub fn git_numstat(cwd: &Path) -> (u64, u64) {
    let Some(out) = run_git(
        cwd,
        &["diff", "--no-color", "--no-ext-diff", "--numstat", "HEAD"],
    ) else {
        return (0, 0);
    };
    let (mut add, mut del) = (0u64, 0u64);
    for line in out.lines() {
        let mut it = line.split('\t');
        let (Some(a), Some(d)) = (it.next(), it.next()) else { continue };
        if let Ok(n) = a.parse::<u64>() { add += n; }
        if let Ok(n) = d.parse::<u64>() { del += n; }
    }
    (add, del)
}

/// Unified diff for one file; untracked files render as all-added content.
pub fn git_file_diff(cwd: &Path, path: &Path, untracked: bool) -> String {
    if untracked {
        if let Ok(text) = std::fs::read_to_string(path) {
            let rel = path
                .strip_prefix(cwd)
                .unwrap_or(path)
                .to_string_lossy()
                .replace("\\", "/");
            let mut out = format!("@@ -0,0 +1,L @@\n");
            for line in text.lines() {
                out.push('+');
                out.push_str(line);
                out.push('\n');
            }
            let _ = rel;
            return out;
        }
        return String::new();
    }
    run_git(
        cwd,
        &["diff", "--no-color", "--no-ext-diff", "--", &path.to_string_lossy()],
    )
    .unwrap_or_default()
}
