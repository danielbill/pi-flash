//! 文件树数据层：展平后的可见行缓存 + gitignore 过滤。
//!
//! - [`IgnoreStack`] 逐字搬自 Zed `crates/worktree/src/ignore.rs`（依托
//!   `ignore` crate 的 Gitignore 匹配，与 Zed 同一实现）；本仓暂不接
//!   global gitignore（Zed 的 global_ignore_root 来自其 fs 层，后续补）。
//! - [`flatten`] 把「根目录 + 展开集合」变成排序好的 [`TreeRow`] 扁平表：
//!   每目录读一次盘、按 Zed 语义排序（`paths_sort`）、过滤 dot 文件与
//!   ignore 条目、回填 git 徽标与「子树有变更」目录圆点。渲染层
//!   （function_panel::file_tree）只消费这个缓存，不再碰磁盘。

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::git::{GitFile, GitStatus};
use super::paths_sort::{SortMode, SortOrder, compare_entry_names};

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
            } => match ignore.matched(abs_path.strip_prefix(abs_base_path).unwrap(), is_dir) {
                ignore::Match::None => IgnoreStack {
                    top: prev.clone(),
                }
                .is_abs_path_ignored(abs_path, is_dir),
                ignore::Match::Ignore(_) => true,
                ignore::Match::Whitelist(_) => false,
            },
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

/// Flatten the root project row (depth 0, Zed 单根 worktree 的项目行) plus
/// every directory in `expanded` into rows. Sorting/filtering follow Zed
/// semantics: dot files hidden, `.git` and gitignored entries skipped,
/// directories first, natural sort.
///
/// 根行的展开态同样读 `expanded`（含 cwd 即展开）；Chat 侧在新建/切项目
/// 时把 cwd 塞进集合，保证默认展开。
pub fn flatten(
    cwd: &Path,
    expanded: &HashSet<PathBuf>,
    git_files: &[GitFile],
) -> Vec<TreeRow> {
    let git_map: HashMap<PathBuf, GitStatus> = git_files
        .iter()
        .map(|f| (f.path.clone(), f.status))
        .collect();
    let mut changed_dirs = changed_ancestor_dirs(cwd, git_files);
    // 根行圆点：任何变更存在即点亮
    if !git_files.is_empty() {
        changed_dirs.insert(cwd.to_path_buf());
    }

    // 仓库根的排除文件（.git/info/exclude）压栈底，再压根 .gitignore；
    // 子目录的 .gitignore 在下钻时继续压栈（深层覆盖浅层，同 git 语义）。
    let mut stack = match build_gitignore_at(cwd, ".git/info/exclude") {
        Some(gi) => IgnoreStack::none().append(IgnoreKind::RepoExclude, Arc::new(gi)),
        None => IgnoreStack::none(),
    };
    if let Some(gi) = build_gitignore_at(cwd, ".gitignore") {
        stack = stack.append(IgnoreKind::Gitignore(Arc::from(cwd)), Arc::new(gi));
    }

    let root_name = cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| cwd.to_string_lossy().to_string());
    let mut out = Vec::new();
    out.push(TreeRow {
        path: cwd.to_path_buf(),
        name: root_name,
        depth: 0,
        is_dir: true,
        expanded: expanded.contains(cwd),
        git: None,
        changed_dot: changed_dirs.contains(cwd),
    });
    if expanded.contains(cwd) {
        walk(cwd, 1, expanded, &git_map, &changed_dirs, &stack, &mut out);
    }
    out
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

fn walk(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    git_map: &HashMap<PathBuf, GitStatus>,
    changed_dirs: &HashSet<PathBuf>,
    stack: &IgnoreStack,
    out: &mut Vec<TreeRow>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(PathBuf, String, bool)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        // pi-web 语义：dot 文件不显示（Zed 的 hidden_files 设置后续再接）
        if name.starts_with('.') {
            continue;
        }
        let Ok(ty) = e.file_type() else { continue };
        let is_dir = ty.is_dir();
        let path = e.path();
        if stack.is_abs_path_ignored(&path, is_dir) {
            continue;
        }
        entries.push((path, name, is_dir));
    }

    // Zed 排序语义：目录先、stem/扩展名两级键、自然排序（DirectoriesFirst
    // + Default；sort_mode 设置后续再接）。par_sort 换 sort：单目录条目量级
    // 下差异可忽略，保持稳定序。
    entries.sort_by(|(pa, na, fa), (pb, nb, fb)| {
        let ord = compare_entry_names((na, !*fa), (nb, !*fb), SortMode::DirectoriesFirst, SortOrder::Default);
        if ord == Ordering::Equal {
            pa.cmp(pb)
        } else {
            ord
        }
    });

    for (path, name, is_dir) in entries {
        if is_dir {
            // Zed auto_fold_dirs：只含一个子目录（且无其他子项）的目录不占
            // 行，链式折叠成「a/b/c」一行；手动展开过的目录断链；chevron
            // 逐级展开（展开集合里是真实目录路径）。根行在 flatten 已豁免。
            let mut chain_names = vec![name];
            let mut chain_paths = vec![path.clone()];
            let mut cur = path.clone();
            while !expanded.contains(&cur) {
                let Some((child_path, child_name)) = sole_child(&cur) else {
                    break;
                };
                if !child_path.is_dir() {
                    break;
                }
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
            });
            if open {
                let mut child_stack = stack.clone();
                if let Some(gi) = build_gitignore_at(&cur, ".gitignore") {
                    child_stack = child_stack.append(
                        IgnoreKind::Gitignore(Arc::from(cur.as_path())),
                        Arc::new(gi),
                    );
                }
                walk(&cur, depth + 1, expanded, git_map, changed_dirs, &child_stack, out);
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

// ---------------------------------------------------------------------------
// 测试：临时树 + gitignore + 排序语义（std::env::temp_dir，无新依赖）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTree(PathBuf);
    impl TempTree {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "piflash-tree-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            Self(base)
        }
        fn dir(&self, rel: &str) -> PathBuf {
            std::fs::create_dir_all(self.0.join(rel)).unwrap();
            self.0.join(rel)
        }
        fn file(&self, rel: &str) -> PathBuf {
            if let Some(p) = self.0.join(rel).parent() {
                std::fs::create_dir_all(p).unwrap();
            }
            std::fs::write(self.0.join(rel), "x").unwrap();
            self.0.join(rel)
        }
    }
    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn flatten_orders_and_lazily_expands() {
        let t = TempTree::new("flat");
        t.file("src/main.rs");
        t.file("a.txt");
        t.file("b.txt");
        t.file("z.md");
        t.dir("node_modules/pkg"); // 未展开的目录不应下钻
        let root_name = t.0.file_name().unwrap().to_string_lossy().to_string();

        // 未展开任何目录：只有根项目行
        let rows = flatten(&t.0, &HashSet::new(), &[]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_dir && !rows[0].expanded);
        assert_eq!(rows[0].path, t.0);
        assert_eq!(rows[0].name, root_name);

        // 展开根 + node_modules：根行 + 目录先 + 自然排序，pkg 深度 2
        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        expanded.insert(t.0.join("node_modules"));
        let rows = flatten(&t.0, &expanded, &[]);
        // 先序遍历：pkg 紧跟 node_modules（树形显示顺序）
        let names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                root_name,
                "node_modules".to_string(),
                "pkg".to_string(),
                "src".to_string(),
                "a.txt".to_string(),
                "b.txt".to_string(),
                "z.md".to_string(),
            ]
        );
        assert!(rows[0].expanded);
        let src = rows.iter().find(|r| r.name == "src").expect("src row");
        assert!(!src.expanded, "unexpanded dir stays collapsed");
        let pkg = rows.iter().find(|r| r.name == "pkg").expect("pkg row");
        assert_eq!(pkg.depth, 2);
        assert_eq!(pkg.path, t.0.join("node_modules").join("pkg"));
    }

    #[test]
    fn flatten_applies_gitignore_and_repo_exclude() {
        let t = TempTree::new("ig");
        std::fs::write(t.0.join(".gitignore"), "build/\n*.log\n!keep.log\n").unwrap();
        t.file("build/out.js");
        t.file("debug.log");
        t.file("keep.log");
        t.file("src/lib.rs");

        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        let rows = flatten(&t.0, &expanded, &[]);
        let names: Vec<&str> = rows.iter().skip(1).map(|r| r.name.as_str()).collect();
        assert!(!names.contains(&"build"), "gitignored dir hidden");
        assert!(names.contains(&"keep.log"), "whitelist survives");
        // src/ 未展开不显示子文件，但目录本身在
        assert_eq!(names, vec!["src", "keep.log"]);

        // .git/info/exclude 也生效
        let git_dir = t.0.join(".git");
        std::fs::create_dir_all(git_dir.join("info")).unwrap();
        std::fs::write(git_dir.join("info").join("exclude"), "secret.txt\n").unwrap();
        t.file("secret.txt");
        let rows = flatten(&t.0, &expanded, &[]);
        let names: Vec<&str> = rows.iter().skip(1).map(|r| r.name.as_str()).collect();
        assert!(!names.contains(&"secret.txt"));
    }

    #[test]
    fn auto_folds_single_child_dir_chains() {
        let t = TempTree::new("fold");
        t.file("a/x/y/f.txt");
        t.file("r.txt");

        // 全折叠：a/x/y 折成一行（Zed auto_fold_dirs 语义）
        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        let rows = flatten(&t.0, &expanded, &[]);
        let names: Vec<&str> = rows.iter().skip(1).map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a/x/y", "r.txt"]);
        assert_eq!(rows[1].path, t.0.join("a").join("x").join("y"));
        assert!(!rows[1].expanded);

        // 展开链尾：行名不变、行内打开，f.txt 深度 2
        expanded.insert(t.0.join("a").join("x").join("y"));
        let rows = flatten(&t.0, &expanded, &[]);
        assert_eq!(rows[1].name, "a/x/y");
        assert!(rows[1].expanded);
        assert_eq!(rows[2].name, "f.txt");
        assert_eq!(rows[2].depth, 2);

        // 展开中段：断链成 a/x + y 两行（y 仍保持展开，f.txt 随其下钻）
        expanded.insert(t.0.join("a").join("x"));
        let rows = flatten(&t.0, &expanded, &[]);
        assert_eq!(rows[1].name, "a/x");
        assert!(rows[1].expanded);
        assert_eq!(rows[2].name, "y");
        assert_eq!(rows[2].depth, 2);
        assert!(rows[2].expanded);
        assert_eq!(rows[3].name, "f.txt");
        assert_eq!(rows[3].depth, 3);
    }

    #[test]
    fn changed_dot_covers_deep_ancestors() {
        let t = TempTree::new("dot");
        let deep = t.dir("a/b/c");
        let changed = t.file("a/b/c/mod.rs");
        let git_files = vec![GitFile {
            path: changed,
            status: GitStatus::Modified,
            staged: false,
        }];
        let mut expanded = HashSet::new();
        expanded.insert(t.0.clone());
        expanded.insert(t.0.join("a"));
        expanded.insert(t.0.join("a").join("b"));
        expanded.insert(deep);

        let rows = flatten(&t.0, &expanded, &git_files);
        // 根行：有任何变更即点亮
        assert!(rows[0].changed_dot);
        let a = rows.iter().find(|r| r.name == "a").unwrap();
        assert!(a.changed_dot, "ancestor dir a gets the dot");
        let c = rows.iter().find(|r| r.name == "c").unwrap();
        assert!(c.changed_dot);
        let m = rows.iter().find(|r| r.name == "mod.rs").unwrap();
        assert!(matches!(m.git, Some(GitStatus::Modified)));
        assert!(!m.changed_dot);
    }
}
