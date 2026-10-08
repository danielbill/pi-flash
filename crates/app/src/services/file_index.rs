//! @ 检索的文件清单来源（pi-web `app/api/file-index/route.ts` 移植）：
//! git 仓库走 `git ls-files --cached --others --exclude-standard`（尊重
//! .gitignore，对齐 pi TUI 的 fd 行为），非 git 回退 BFS 目录遍历。
//!
//! 调用方缓存见 Chat::at_index（每 cwd 一份、后台线程构建；TTL/容量读
//! app-settings.json 的 at_index_ttl_secs / at_index_max_projects）。

use std::path::Path;

/// git 硬上限（route.ts GIT_HARD_CAP）
const GIT_HARD_CAP: usize = 200_000;
/// 非 git 回退遍历的上限/深度（route.ts WALK_HARD_CAP / MAX_WALK_DEPTH）
const WALK_HARD_CAP: usize = 50_000;
const MAX_WALK_DEPTH: usize = 8;

#[derive(Debug, Clone)]
pub struct FileListing {
    pub files: Vec<String>,
    /// 连硬上限都被截断（pi-web FileListing.hardTruncated 对齐字段；当前
    /// 全量打分不消费它，留给「结果可能不全」提示用）
    #[allow(dead_code)]
    pub hard_truncated: bool,
}

/// git ls-files：`-z` 分隔防路径带空格被拆；LC_ALL=C 稳定排序（route.ts
/// 同参）。git 不存在 / 非 git 仓库 → None（调用方回退 walk）。
pub fn list_with_git(cwd: &Path) -> Option<FileListing> {
    let out = std::process::Command::new("git")
        .args(["-C", &cwd.to_string_lossy(), "ls-files", "--cached", "--others", "--exclude-standard", "-z"])
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let all: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if all.len() > GIT_HARD_CAP {
        return Some(FileListing {
            files: all.into_iter().take(GIT_HARD_CAP).collect(),
            hard_truncated: true,
        });
    }
    Some(FileListing { files: all, hard_truncated: false })
}

/// BFS 目录遍历（浅层文件优先占上限）；跳过隐藏项与常见构建目录。仅在
/// 非 git 仓库生效——git 仓库靠 .gitignore（route.ts isHiddenOutsideGit 注）。
pub fn list_with_walk(cwd: &Path) -> FileListing {
    let mut files: Vec<String> = Vec::new();
    let mut queue: std::collections::VecDeque<(std::path::PathBuf, String, usize)> =
        std::collections::VecDeque::new();
    queue.push_back((cwd.to_path_buf(), String::new(), 0));
    while let Some((abs, rel, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&abs) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if is_hidden_outside_git(&name) {
                continue;
            }
            let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            match e.file_type() {
                Ok(ft) if ft.is_dir() => {
                    if depth + 1 <= MAX_WALK_DEPTH {
                        queue.push_back((abs.join(&name), child_rel, depth + 1));
                    }
                }
                Ok(ft) if ft.is_file() => {
                    if files.len() >= WALK_HARD_CAP {
                        return FileListing { files, hard_truncated: true };
                    }
                    files.push(child_rel);
                }
                _ => {}
            }
        }
    }
    FileListing { files, hard_truncated: false }
}

/// 非 git 回退的名字黑名单：隐藏项 + 依赖/构建目录（pi-web
/// file-tree-visibility 的同职责清单；与 walk_files 的黑名单一致）。
fn is_hidden_outside_git(name: &str) -> bool {
    name.starts_with('.')
        || name == "node_modules"
        || name == "target"
        || name == "dist"
}

/// 装载一个 cwd 的清单（git 优先，回退 BFS）。后台线程调用。
pub fn load_listing(cwd: &Path) -> FileListing {
    list_with_git(cwd).unwrap_or_else(|| list_with_walk(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_lists_relative_forward_slash_paths_and_skips_hidden() {
        let dir = std::env::temp_dir().join(format!("pif-at-walk-{}", std::process::id()));
        let sub = dir.join("sub");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("top.txt"), "x").unwrap();
        std::fs::write(dir.join(".hidden"), "x").unwrap();
        std::fs::write(sub.join("deep.rs"), "x").unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("node_modules/junk.js"), "x").unwrap();
        let listing = list_with_walk(&dir);
        assert_eq!(listing.files, vec!["top.txt", "sub/deep.rs"]);
        assert!(!listing.hard_truncated);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_git_dir_falls_back_to_walk() {
        let dir = std::env::temp_dir().join(format!("pif-at-nogit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        // 非 git 目录：git ls-files 失败 → 回退 walk
        assert!(list_with_git(&dir).is_none());
        assert_eq!(load_listing(&dir).files, vec!["a.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
