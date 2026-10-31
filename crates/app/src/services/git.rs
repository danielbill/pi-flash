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
    /// index (X) column non-blank — the change is staged
    pub staged: bool,
}

/// One history entry (022 History list; simplified — no graph).
#[derive(Debug, Clone)]
pub struct GitCommit {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

/// `git` 是 console 程序：GUI 宿主裸 spawn 会给它分配控制台，Windows
/// Terminal 立刻弹一个窗口（tag 名=git）。一律 CREATE_NO_WINDOW 静默跑。
#[cfg(windows)]
fn hide_console(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_cmd: &mut std::process::Command) {}

/// 裸 `git` 命令基座（无参数）：统一 CREATE_NO_WINDOW（GUI 宿主裸 spawn
/// 会弹 Windows Terminal 窗口）+ `GIT_TERMINAL_PROMPT=0`（GUI 无 tty，
/// 凭据缺失时宁可立刻失败也不许挂起等输入）。域内其他自建 git 命令
/// （file_index / pi_extension_updates）一律从这里起步。
pub(crate) fn git_command() -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    hide_console(&mut cmd);
    cmd
}

/// 带 CREATE_NO_WINDOW 的 `git -C <cwd> <args>`。
pub(crate) fn git_cmd(cwd: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = git_command();
    cmd.arg("-C").arg(cwd).args(args);
    cmd
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = git_cmd(cwd, args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// `git status --porcelain=v1 -z --untracked-files=all -- .` parsed to files.
///
/// cwd 可能是仓库的子目录：无 pathspec 时 git 扫整个仓库，`-uall` 还会
/// 递归展开全部未跟踪目录（家目录遗留仓库 + Windows junction 实测
/// 270 万条 / 414MB / 99.9s）；`-- .` 把扫描钉在 cwd 内。
pub fn git_status_files(cwd: &Path) -> Vec<GitFile> {
    let Some(out) = run_git(
        cwd,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ],
    ) else {
        return Vec::new();
    };
    // porcelain 输出仓库根相对路径；重定基到 cwd，与文件树 read_dir 生成
    // 的路径同形态，flatten 的 git_map 才能命中。
    let repo_root = run_git(cwd, &["rev-parse", "--show-toplevel"])
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or_else(|| cwd.to_path_buf());
    let rel_cwd = cwd.strip_prefix(&repo_root).ok();
    let mut files = Vec::new();
    // -z: 记录 NUL 分隔；rename/copy 是两条记录——新路径带 "XY " 前缀，
    // 旧路径是紧跟的无前缀记录（没有 TAB 可切）。
    let mut records = out.split('\0');
    while let Some(rec) = records.next() {
        if rec.len() < 4 {
            continue;
        }
        let xy = &rec[..2];
        let rest = &rec[3..];
        let x = xy.as_bytes()[0];
        let y = xy.as_bytes()[1];
        let staged = !matches!(x, b' ' | b'?');
        let (status, file_path) = match xy {
            "??" => (GitStatus::Untracked, rest),
            _ => {
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
                (st, rest)
            }
        };
        let full = match rel_cwd.and_then(|prefix| Path::new(file_path).strip_prefix(prefix).ok()) {
            Some(sub) => cwd.join(sub),
            None => repo_root.join(file_path),
        };
        files.push(GitFile {
            path: full,
            status,
            staged,
        });
        if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            records.next(); // 旧路径记录不是独立条目
        }
    }
    files
}

/// Total (+, -) line counts of tracked changes (numstat HEAD summary,
/// scoped to cwd — 子目录项目不计项目外的改动).
pub fn git_numstat(cwd: &Path) -> (u64, u64) {
    let Some(out) = run_git(
        cwd,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--numstat",
            "HEAD",
            "--",
            ".",
        ],
    ) else {
        return (0, 0);
    };
    let (mut add, mut del) = (0u64, 0u64);
    for line in out.lines() {
        let mut it = line.split('\t');
        let (Some(a), Some(d)) = (it.next(), it.next()) else {
            continue;
        };
        if let Ok(n) = a.parse::<u64>() {
            add += n;
        }
        if let Ok(n) = d.parse::<u64>() {
            del += n;
        }
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
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--",
            &path.to_string_lossy(),
        ],
    )
    .unwrap_or_default()
}

/// run_git with error surface (commit/push report failure text).
fn run_git_checked(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_cmd(cwd, args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Stage one change (`git add -- <path>`; also removes deletions from the
/// index).
pub fn git_stage(cwd: &Path, path: &Path) -> Result<(), String> {
    run_git_checked(cwd, &["add", "--", &path.to_string_lossy()]).map(|_| ())
}

/// Stage every change incl. untracked (`git add -A`; v54 Stage All).
pub fn git_stage_all(cwd: &Path) -> Result<(), String> {
    run_git_checked(cwd, &["add", "-A"]).map(|_| ())
}

/// Undo the last commit, keeping changes staged (`git reset --soft HEAD~1`).
pub fn git_uncommit(cwd: &Path) -> Result<(), String> {
    run_git_checked(cwd, &["reset", "--soft", "HEAD~1"]).map(|_| ())
}

/// Commits the local branch is ahead of its upstream (v54 `↑N Push` badge).
pub fn git_ahead_count(cwd: &Path) -> usize {
    run_git(cwd, &["rev-list", "--count", "@{upstream}..HEAD"])
        .and_then(|out| out.trim().parse().ok())
        .unwrap_or(0)
}

/// Unstage one change (`git reset -q HEAD -- <path>`; a repo without a
/// first commit reports an error, which the panel surfaces verbatim).
pub fn git_unstage(cwd: &Path, path: &Path) -> Result<(), String> {
    run_git_checked(cwd, &["reset", "-q", "HEAD", "--", &path.to_string_lossy()]).map(|_| ())
}

/// Commit the staged changes with a validated message.
pub fn git_commit(cwd: &Path, message: &str) -> Result<(), String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err("commit message is empty".into());
    }
    run_git_checked(cwd, &["commit", "-m", msg]).map(|_| ())
}

/// Push the current branch; returns remote output on success.
pub fn git_push(cwd: &Path) -> Result<String, String> {
    // stderr carries the progress/ref output even on success
    let out = git_cmd(cwd, &["push", "--verbose"])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}

/// Recent commits, newest first (`git log` field-separated by  so
/// subjects containing `|` stay intact).
pub fn git_log(cwd: &Path, max: usize) -> Vec<GitCommit> {
    let Some(out) = run_git(
        cwd,
        &[
            "log",
            &format!("--max-count={}", max),
            "--date=format:%Y-%m-%d %H:%M",
            "--pretty=format:%h%an%ad%s",
        ],
    ) else {
        return Vec::new();
    };
    parse_git_log(&out)
}

fn parse_git_log(out: &str) -> Vec<GitCommit> {
    out.lines()
        .filter_map(|line| {
            let mut it = line.split('');
            Some(GitCommit {
                hash: it.next()?.to_string(),
                author: it.next()?.to_string(),
                date: it.next()?.to_string(),
                subject: it.next().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod panel_tests {
    use super::*;

    #[test]
    fn parses_log_fields() {
        let out = "abc1234Alice2026-09-26 10:00feat: add | pipe
beef00Bob2026-09-25 09:00fix";
        let log = parse_git_log(out);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].hash, "abc1234");
        assert_eq!(log[0].author, "Alice");
        assert_eq!(log[0].subject, "feat: add | pipe");
        assert_eq!(log[1].subject, "fix");
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;
    use std::collections::HashMap;

    /// temp git 仓库，Drop 清理；tag+pid 后缀防测试间碰撞。
    struct TempRepo {
        root: PathBuf,
    }

    impl TempRepo {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("pif-git-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self::git(&root, &["init"]);
            Self { root }
        }

        fn git(repo: &Path, args: &[&str]) {
            // 复用 git_cmd（CREATE_NO_WINDOW）：裸 spawn 控制台程序在并发
            // 测试下会撞 conhost 分配竞态（0xC0000142 STATUS_DLL_INIT_FAILED）
            let out = super::git_cmd(repo, args)
                .output()
                .expect("git 必须在 PATH 里");
            assert!(
                out.status.success(),
                "git {args:?} in {repo:?}: code={:?}, stdout={:?}, stderr={:?}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }

        fn commit_all(&self, msg: &str) {
            Self::git(&self.root, &["add", "-A"]);
            Self::git(
                &self.root,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "-m",
                    msg,
                ],
            );
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn non_git_dir_returns_empty() {
        let dir = std::env::temp_dir().join(format!("pif-git-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(git_status_files(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 项目是大仓库子目录时：不扫 sibling，路径重定基成 cwd 形态。
    #[test]
    fn status_scopes_to_subdir_and_rebases_paths_onto_cwd() {
        let t = TempRepo::new("scope");
        let sub = t.root.join("sub");
        let sibling = t.root.join("sibling");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sub.join("tracked.txt"), "a\n").unwrap();
        std::fs::write(sibling.join("noise.txt"), "n\n").unwrap();
        t.commit_all("init");

        // sub 内一改一增；sibling 塞 300 个未跟踪文件，混进结果即失败
        std::fs::write(sub.join("tracked.txt"), "b\n").unwrap();
        std::fs::write(sub.join("new.txt"), "x\n").unwrap();
        for i in 0..300 {
            std::fs::write(sibling.join(format!("n{i}.txt")), "y\n").unwrap();
        }

        let files = git_status_files(&sub);
        assert!(!files.is_empty());
        for f in &files {
            assert!(
                f.path.starts_with(&sub),
                "path outside cwd: {}",
                f.path.display()
            );
        }
        // 与 read_dir 同形态（flatten 的 git_map 命中的前提）
        let map: HashMap<PathBuf, GitStatus> =
            files.iter().map(|f| (f.path.clone(), f.status)).collect();
        assert_eq!(
            map.get(&sub.join("tracked.txt")),
            Some(&GitStatus::Modified)
        );
        assert_eq!(map.get(&sub.join("new.txt")), Some(&GitStatus::Untracked));
    }

    /// 真实消费方回归：子目录仓库里徽标要能落到树行上。
    #[test]
    fn flatten_badge_hits_real_tree_path_in_subdir_repo() {
        let t = TempRepo::new("badge");
        let sub = t.root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("mod.txt"), "a\n").unwrap();
        t.commit_all("init");
        std::fs::write(sub.join("mod.txt"), "b\n").unwrap();

        let files = git_status_files(&sub);
        let mut expanded = std::collections::HashSet::new();
        expanded.insert(sub.clone());
        let rows = crate::services::file_tree::flatten(&sub, &expanded, &files);
        let row = rows
            .iter()
            .find(|r| r.path == sub.join("mod.txt"))
            .expect("modified file row missing");
        assert_eq!(row.git, Some(GitStatus::Modified));
    }

    /// -z 下 rename 是 `R  new\0old\0` 两条记录：只留新路径一条 Renamed。
    #[test]
    fn rename_in_z_mode_consumes_old_path_record() {
        let t = TempRepo::new("rename");
        let sub = t.root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("old.txt"), "r\n").unwrap();
        t.commit_all("init");
        TempRepo::git(&t.root, &["mv", "sub/old.txt", "sub/renamed.txt"]);

        let files = git_status_files(&sub);
        let renamed: Vec<&GitFile> = files
            .iter()
            .filter(|f| f.status == GitStatus::Renamed)
            .collect();
        assert_eq!(renamed.len(), 1, "all: {files:?}");
        assert_eq!(renamed[0].path, sub.join("renamed.txt"));
        assert!(
            files.iter().all(|f| f.path != sub.join("old.txt")),
            "old path leaked as separate entry: {files:?}"
        );
    }

    #[test]
    fn numstat_scopes_to_cwd() {
        let t = TempRepo::new("numstat");
        let sub = t.root.join("sub");
        let sibling = t.root.join("sibling");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sub.join("a.txt"), "one\n").unwrap();
        std::fs::write(sibling.join("b.txt"), "x\n").unwrap();
        t.commit_all("init");
        std::fs::write(sub.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(sibling.join("b.txt"), "x\ny\nz\nw\n").unwrap();
        assert_eq!(git_numstat(&sub), (2, 0));
    }
}
