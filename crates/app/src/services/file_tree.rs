//! 文件树数据层：展平后的可见行缓存 + gitignore 过滤。
//!
//! - [`IgnoreStack`] 逐字搬自 Zed `crates/worktree/src/ignore.rs`（依托
//!   `ignore` crate 的 Gitignore 匹配，与 Zed 同一实现）；本仓暂不接
//!   global gitignore（Zed 的 global_ignore_root 来自其 fs 层，后续补）。
//! - [`flatten`] 把「根目录 + 展开集合」变成排序好的 [`TreeRow`] 扁平表：
//!   每目录读一次盘、按 Zed 语义排序（`paths_sort`）、跳过扫描排除项、
//!   标记 ignore 条目、回填 git 徽标与「子树有变更」目录圆点。渲染层
//!   （function_panel::file_tree）只消费这个缓存，不再碰磁盘。

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::git::{GitFile, GitStatus};
use super::paths_sort::{SortMode, SortOrder, compare_entry_names};

mod pins;

#[cfg(test)]
mod tests;

pub(crate) use pins::flatten_pins;

// ---------------------------------------------------------------------------
// IgnoreStack —— 逐字搬自 Zed crates/worktree/src/ignore.rs（去掉
// global_ignore_root / repo_root 字段相关的全局忽略分支）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct IgnoreStack {
    pub top: Arc<IgnoreStackEntry>,
}

#[derive(Debug)]
pub enum IgnoreStackEntry {
    None,
    RepoExclude {
        ignore: Arc<Gitignore>,
        parent: Arc<IgnoreStackEntry>,
    },
    Some {
        abs_base_path: Arc<Path>,
        ignore: Arc<Gitignore>,
        parent: Arc<IgnoreStackEntry>,
    },
    All,
}

#[derive(Debug)]
pub enum IgnoreKind {
    Gitignore(Arc<Path>),
    RepoExclude,
}

use ignore::gitignore::Gitignore;

impl IgnoreStack {
    pub fn none() -> Self {
        Self {
            top: Arc::new(IgnoreStackEntry::None),
        }
    }

    /// Zed 语义：明确无视 ignore 规则时全收（接「显示被忽略文件」设置时启用）
    #[allow(dead_code)]
    pub fn all() -> Self {
        Self {
            top: Arc::new(IgnoreStackEntry::All),
        }
    }

    pub fn append(self, kind: IgnoreKind, ignore: Arc<Gitignore>) -> Self {
        let top = match self.top.as_ref() {
            IgnoreStackEntry::All => self.top.clone(),
            _ => Arc::new(match kind {
                IgnoreKind::Gitignore(abs_base_path) => IgnoreStackEntry::Some {
                    abs_base_path,
                    ignore,
                    parent: self.top.clone(),
                },
                IgnoreKind::RepoExclude => IgnoreStackEntry::RepoExclude {
                    ignore,
                    parent: self.top.clone(),
                },
            }),
        };
        Self { top }
    }

    pub fn is_abs_path_ignored(&self, abs_path: &Path, is_dir: bool) -> bool {
        if is_dir && abs_path.file_name() == Some(std::ffi::OsStr::new(".git")) {
            return true;
        }

        match self.top.as_ref() {
            IgnoreStackEntry::None => false,
            IgnoreStackEntry::All => true,
            IgnoreStackEntry::RepoExclude { ignore, parent } => {
                // Ignore rules from a repository that does not contain this path.
                if !abs_path.starts_with(ignore.path()) {
                    return IgnoreStack {
                        top: parent.clone(),
                    }
                    .is_abs_path_ignored(abs_path, is_dir);
                }

                match ignore.matched(abs_path, is_dir) {
                    ignore::Match::None => IgnoreStack {
                        top: parent.clone(),
                    }
                    .is_abs_path_ignored(abs_path, is_dir),
                    ignore::Match::Ignore(_) => true,
                    ignore::Match::Whitelist(_) => false,
                }
            }
            IgnoreStackEntry::Some {
                abs_base_path,
                ignore,
                parent: prev,
            } => {
                // 规则来自不包含本路径的 base（Pin 收藏夹收到别项目的绝对
                // 路径、目录被移走过的历史脏数据等）——与 RepoExclude 分支
                // 同款处置：跳过本层、交上一层。原先直接 strip_prefix
                // .unwrap()，非前缀路径当场 panic（用户 2026-10-17 报崩：
                // 切项目时 Pin 清单没跟着换 → 崩在 flatten_pins）。
                if !abs_path.starts_with(abs_base_path) {
                    return IgnoreStack { top: prev.clone() }.is_abs_path_ignored(abs_path, is_dir);
                }
                match ignore.matched(abs_path.strip_prefix(abs_base_path).unwrap(), is_dir) {
                    ignore::Match::None => {
                        IgnoreStack { top: prev.clone() }.is_abs_path_ignored(abs_path, is_dir)
                    }
                    ignore::Match::Ignore(_) => true,
                    ignore::Match::Whitelist(_) => false,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TreeRow + flatten
// ---------------------------------------------------------------------------

/// One visible row of the flattened tree (render order == Vec order).
#[derive(Debug, Clone)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    /// git status of a file row (badge).
    pub git: Option<GitStatus>,
    /// directory row whose subtree contains changes (amber dot).
    pub changed_dot: bool,
    /// gitignored entry（Zed parity：显示但置灰；被忽略目录内部整体继承
    /// ignored，不再逐项匹配——git 语义下 negation 救不回被忽略目录内内容）
    pub ignored: bool,
}

/// git 变更文件的全部祖先目录（相对 cwd 的每一级），目录圆点上浮用。
fn changed_ancestor_dirs(cwd: &Path, files: &[GitFile]) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    for f in files {
        let mut dir = f.path.parent();
        while let Some(d) = dir {
            if d == cwd {
                break;
            }
            if d.starts_with(cwd) {
                out.insert(d.to_path_buf());
            }
            dir = d.parent();
        }
    }
    out
}

/// Flatten every directory in `expanded` into rows. Sorting/filtering
/// follow Zed semantics: dot files **visible**（v61 起对齐 Zed，此前误随
/// pi-web 藏点）, only the default scan exclusions (`.git` etc.) skipped,
/// gitignored entries shown dimmed (`ignored`), directories first, natural sort.
///
/// 021 定稿（二改）：**根行不再进树**——项目名由操作栏承载（树顶 40px
/// 固定行，名字居左、操作钮右对齐，右键弹根目录菜单），树体直接从根的
/// 子项排起（depth 0 起，不再有一层幻缩进）。
///
/// 「只看变更」不走这里——那是 [`flatten_changes`]（直接吃 git status
/// 结果，不扫盘；本函数扫盘展开，大仓库上量级完全不同）。
pub fn flatten(cwd: &Path, expanded: &HashSet<PathBuf>, git_files: &[GitFile]) -> Vec<TreeRow> {
    let git_map: HashMap<PathBuf, GitStatus> = git_files
        .iter()
        .map(|f| (f.path.clone(), f.status))
        .collect();
    let mut changed_dirs = changed_ancestor_dirs(cwd, git_files);
    // 根行圆点：任何变更存在即点亮
    if !git_files.is_empty() {
        changed_dirs.insert(cwd.to_path_buf());
    }

    let stack = root_ignore_stack(cwd);

    let mut out = Vec::new();
    if expanded.contains(cwd) {
        walk(
            cwd,
            0,
            expanded,
            &git_map,
            &changed_dirs,
            &stack,
            false,
            &mut out,
        );
    }
    out
}

/// 仓库根的 ignore 栈（`.git/info/exclude` 压栈底，再压根 `.gitignore`；
/// 子目录的 .gitignore 在 walk 下钻时继续压栈）。flatten / flatten_pins
/// 共用的初始化。
fn root_ignore_stack(cwd: &Path) -> IgnoreStack {
    let mut stack = match build_gitignore_at(cwd, ".git/info/exclude") {
        Some(gi) => IgnoreStack::none().append(IgnoreKind::RepoExclude, Arc::new(gi)),
        None => IgnoreStack::none(),
    };
    if let Some(gi) = build_gitignore_at(cwd, ".gitignore") {
        stack = stack.append(IgnoreKind::Gitignore(Arc::from(cwd)), Arc::new(gi));
    }
    stack
}

/// 「只看变更」的**扁平**行表（021 二改定稿：不要树状，如 pi-web 截图
/// ——头行 N 个文件 +x -y，行 = 文件名前置 + 目录段后置）。
/// 直接吃 `git status` 结果构造，**不读盘**：大仓库上毫秒级（此前
/// changes_only 全量下钻每切一次重扫整棵树，是「检索很慢」的根因）。
/// 行字段约定：`name` = 相对路径（'/' 分隔，渲染层拆目录段/文件名段），
/// depth 恒 0，`git` = 状态徽标，路径字典序。
pub fn flatten_changes(cwd: &Path, git_files: &[GitFile]) -> Vec<TreeRow> {
    let mut files: Vec<&GitFile> = git_files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
        .into_iter()
        .map(|f| {
            let name = f
                .path
                .strip_prefix(cwd)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .replace('\\', "/");
            TreeRow {
                path: f.path.clone(),
                name,
                depth: 0,
                is_dir: false,
                expanded: false,
                git: Some(f.status),
                changed_dot: false,
                ignored: false,
            }
        })
        .collect()
}

/// Per-directory `.gitignore`, or `None` when the directory has none.
fn build_gitignore_at(dir: &Path, rel: &str) -> Option<Gitignore> {
    let path = dir.join(rel);
    if !path.is_file() {
        return None;
    }
    let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
    let _ = builder.add(&path);
    Some(builder.build().unwrap_or_else(|_| Gitignore::empty()))
}

/// Zed 默认 `file_scan_exclusions`：任何层级匹配这些名字的条目整个不进树
/// （Zed project panel 同款名单；除此之外 dot 文件默认可见）。
const SCAN_EXCLUSIONS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "CVS",
    ".DS_Store",
    "Thumbs.db",
    ".classpath",
    ".settings",
];

#[allow(clippy::too_many_arguments)]
fn walk(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    git_map: &HashMap<PathBuf, GitStatus>,
    changed_dirs: &HashSet<PathBuf>,
    stack: &IgnoreStack,
    parent_ignored: bool,
    out: &mut Vec<TreeRow>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(PathBuf, String, bool, bool)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // Zed parity：dot 文件默认可见（此前「点开头全隐藏」是 pi-web 语义，
        // 与 Zed 不符）；仅扫描排除名单整个不进树。
        if SCAN_EXCLUSIONS.contains(&name.as_str()) {
            continue;
        }
        let Ok(ty) = e.file_type() else { continue };
        let is_dir = ty.is_dir();
        let path = e.path();
        let ignored = parent_ignored || stack.is_abs_path_ignored(&path, is_dir);
        entries.push((path, name, is_dir, ignored));
    }

    // Zed 排序语义：目录先、stem/扩展名两级键、自然排序（DirectoriesFirst
    // + Default；sort_mode 设置后续再接）。par_sort 换 sort：单目录条目量级
    // 下差异可忽略，保持稳定序。
    entries.sort_by(|(pa, na, fa, _), (pb, nb, fb, _)| {
        let ord = compare_entry_names(
            (na, !*fa),
            (nb, !*fb),
            SortMode::DirectoriesFirst,
            SortOrder::Default,
        );
        if ord == Ordering::Equal {
            pa.cmp(pb)
        } else {
            ord
        }
    });

    for (path, name, is_dir, ignored) in entries {
        if is_dir {
            // Zed auto_fold_dirs：只含一个子目录（且无其他子项）的目录不占
            // 行，链式折叠成「a/b/c」一行；手动展开过的目录断链；chevron
            // 逐级展开（展开集合里是真实目录路径）。根行在 flatten 已豁免。
            let mut chain_names = vec![name];
            let mut chain_paths = vec![path.clone()];
            let mut chain_ignored = ignored;
            let mut cur = path.clone();
            while !expanded.contains(&cur) {
                let Some((child_path, child_name)) = sole_child(&cur) else {
                    break;
                };
                if !child_path.is_dir() {
                    break;
                }
                chain_ignored |= stack.is_abs_path_ignored(&child_path, true);
                chain_names.push(child_name);
                chain_paths.push(child_path.clone());
                cur = child_path;
            }
            let open = expanded.contains(&cur);
            out.push(TreeRow {
                path: cur.clone(),
                name: chain_names.join("/"),
                depth,
                is_dir: true,
                expanded: open,
                git: None,
                changed_dot: chain_paths.iter().any(|p| changed_dirs.contains(p)),
                ignored: chain_ignored,
            });
            if open {
                // 被忽略目录内部整体继承 ignored，不再压子 .gitignore 也
                // 不再逐项匹配（git 语义：negation 救不回被忽略目录内内容）
                let child_stack = if chain_ignored {
                    stack.clone()
                } else {
                    let mut s2 = stack.clone();
                    if let Some(gi) = build_gitignore_at(&cur, ".gitignore") {
                        s2 = s2.append(
                            IgnoreKind::Gitignore(Arc::from(cur.as_path())),
                            Arc::new(gi),
                        );
                    }
                    s2
                };
                walk(
                    &cur,
                    depth + 1,
                    expanded,
                    git_map,
                    changed_dirs,
                    &child_stack,
                    chain_ignored,
                    out,
                );
            }
        } else {
            let open = false;
            out.push(TreeRow {
                path: path.clone(),
                name,
                depth,
                is_dir,
                expanded: open,
                git: git_map.get(&path).copied(),
                changed_dot: false,
                ignored,
            });
        }
    }
}

/// 目录的唯一子项（不过滤 dot/ignore——Zed 的 child_entries 同样是全量）；
/// 子项数 ≠ 1 或读失败返回 None。
fn sole_child(dir: &Path) -> Option<(PathBuf, String)> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut only: Option<(PathBuf, String)> = None;
    let mut count = 0;
    for e in rd.flatten() {
        count += 1;
        if count > 1 {
            return None;
        }
        only = Some((e.path(), e.file_name().to_string_lossy().to_string()));
    }
    only.filter(|_| count == 1)
}
