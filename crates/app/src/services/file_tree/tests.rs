//! file_tree 数据层测试（itlq.9 自 file_tree.rs 外移，纯结构搬移）：临时树
//! + gitignore + 排序语义（std::env::temp_dir，无新依赖）。

use super::*;

struct TempTree(PathBuf);
impl TempTree {
    fn new(tag: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("piflash-tree-{tag}-{}", std::process::id()));
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

    // 未展开任何目录：空树（根行已由操作栏承载，021 二改）
    let rows = flatten(&t.0, &HashSet::new(), &[]);
    assert!(rows.is_empty());

    // 展开根 + node_modules：目录先 + 自然排序，根子项 depth 0 起
    let mut expanded = HashSet::new();
    expanded.insert(t.0.clone());
    expanded.insert(t.0.join("node_modules"));
    let rows = flatten(&t.0, &expanded, &[]);
    // 先序遍历：pkg 紧跟 node_modules（树形显示顺序）
    let names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
    assert_eq!(
        names,
        vec![
            "node_modules".to_string(),
            "pkg".to_string(),
            "src".to_string(),
            "a.txt".to_string(),
            "b.txt".to_string(),
            "z.md".to_string(),
        ]
    );
    let node_modules = rows.first().expect("node_modules row");
    assert!(node_modules.is_dir && node_modules.expanded);
    assert_eq!(node_modules.depth, 0);
    let src = rows.iter().find(|r| r.name == "src").expect("src row");
    assert!(!src.expanded, "unexpanded dir stays collapsed");
    let pkg = rows.iter().find(|r| r.name == "pkg").expect("pkg row");
    assert_eq!(pkg.depth, 1);
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
    let by_name = |n: &str| {
        rows.iter()
            .find(|r| r.name == n)
            .unwrap_or_else(|| panic!("row {n}"))
    };
    // v60 起 ignored 条目显示但置灰标记（Zed parity），不再隐藏
    let build = by_name("build");
    assert!(build.ignored, "gitignored dir shown dimmed");
    let debug = by_name("debug.log");
    assert!(debug.ignored, "gitignored file shown dimmed");
    let keep = by_name("keep.log");
    assert!(!keep.ignored, "whitelist survives as normal");
    assert!(!by_name("src").ignored, "normal dir unmarked");
    // v61 起 dot 文件可见（Zed parity）：.gitignore 本身也是普通行
    assert!(
        !by_name(".gitignore").ignored,
        "dot file shown as normal row"
    );
    // src/ 未展开不显示子文件，但目录本身在
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["build", "src", ".gitignore", "debug.log", "keep.log"]
    );

    // .git/info/exclude 也生效（同样显示但标记）；.git 目录本身不进树
    let git_dir = t.0.join(".git");
    std::fs::create_dir_all(git_dir.join("info")).unwrap();
    std::fs::write(git_dir.join("info").join("exclude"), "secret.txt\n").unwrap();
    t.file("secret.txt");
    let rows = flatten(&t.0, &expanded, &[]);
    let secret = rows
        .iter()
        .find(|r| r.name == "secret.txt")
        .expect("excluded file shown");
    assert!(secret.ignored, "repo-excluded file shown dimmed");
    assert!(rows.iter().all(|r| r.name != ".git"), ".git scanned out");
}

#[test]
fn dot_files_visible_but_scan_exclusions_hidden() {
    let t = TempTree::new("dotvis");
    std::fs::write(t.0.join(".gitignore"), ".beads/\n").unwrap();
    t.file(".gitattributes");
    t.dir(".beads");
    t.file(".beads/issues.jsonl");
    std::fs::create_dir_all(t.0.join(".git").join("objects")).unwrap();
    t.file(".DS_Store");

    let mut expanded = HashSet::new();
    expanded.insert(t.0.clone());
    expanded.insert(t.0.join(".beads"));
    let rows = flatten(&t.0, &expanded, &[]);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // 目录先（.beads 展开随后子行），dot 文件按普通条目参与排序
    assert_eq!(
        names,
        vec![".beads", "issues.jsonl", ".gitattributes", ".gitignore"]
    );
    assert!(rows[0].ignored, "gitignored dot dir shown dimmed");
    assert!(rows[1].ignored, "ignored dir children inherit the mark");
    assert!(
        !rows[2].ignored && !rows[3].ignored,
        "plain dot files normal"
    );
    // Zed 默认 file_scan_exclusions：.git / .DS_Store 整个不进树
    assert!(names.iter().all(|n| *n != ".git" && *n != ".DS_Store"));
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
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["a/x/y", "r.txt"]);
    assert_eq!(rows[0].path, t.0.join("a").join("x").join("y"));
    assert!(!rows[0].expanded);

    // 展开链尾：行名不变、行内打开，f.txt 深度 1
    expanded.insert(t.0.join("a").join("x").join("y"));
    let rows = flatten(&t.0, &expanded, &[]);
    assert_eq!(rows[0].name, "a/x/y");
    assert!(rows[0].expanded);
    assert_eq!(rows[1].name, "f.txt");
    assert_eq!(rows[1].depth, 1);

    // 展开中段：断链成 a/x + y 两行（y 仍保持展开，f.txt 随其下钻）
    expanded.insert(t.0.join("a").join("x"));
    let rows = flatten(&t.0, &expanded, &[]);
    assert_eq!(rows[0].name, "a/x");
    assert!(rows[0].expanded);
    assert_eq!(rows[1].name, "y");
    assert_eq!(rows[1].depth, 1);
    assert!(rows[1].expanded);
    assert_eq!(rows[2].name, "f.txt");
    assert_eq!(rows[2].depth, 2);
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
    let a = rows.iter().find(|r| r.name == "a").unwrap();
    assert!(a.changed_dot, "ancestor dir a gets the dot");
    let c = rows.iter().find(|r| r.name == "c").unwrap();
    assert!(c.changed_dot);
    let m = rows.iter().find(|r| r.name == "mod.rs").unwrap();
    assert!(matches!(m.git, Some(GitStatus::Modified)));
    assert!(!m.changed_dot);
}

#[test]
fn flatten_changes_is_flat_relative_paths() {
    let t = TempTree::new("chg");
    let m = t.file("a/b/mod.rs");
    let r = t.file("root.txt");
    let u = t.file("src/new.txt");
    let git_files = vec![
        GitFile {
            path: u,
            status: GitStatus::Untracked,
            staged: false,
        },
        GitFile {
            path: r,
            status: GitStatus::Modified,
            staged: false,
        },
        GitFile {
            path: m,
            status: GitStatus::Modified,
            staged: false,
        },
    ];
    let rows = flatten_changes(&t.0, &git_files);
    // 扁平（depth 恒 0）+ 路径字典序 + '/' 分隔相对路径 + 徽标回填
    let rows: Vec<(String, usize, Option<GitStatus>)> =
        rows.into_iter().map(|r| (r.name, r.depth, r.git)).collect();
    assert_eq!(
        rows,
        vec![
            ("a/b/mod.rs".to_string(), 0, Some(GitStatus::Modified)),
            ("root.txt".to_string(), 0, Some(GitStatus::Modified)),
            ("src/new.txt".to_string(), 0, Some(GitStatus::Untracked)),
        ]
    );

    // 无变更 = 空表（渲染层出「暂无变更」空态）
    assert!(flatten_changes(&t.0, &[]).is_empty());
}

#[test]
fn flatten_pins_roots_sorted_children_via_walk() {
    let t = TempTree::new("pins");
    t.file("src/deep/lib.rs");
    t.file("docs/guide.md");
    std::fs::write(t.0.join("docs/.gitignore"), "draft.md\n").unwrap();
    t.file("docs/draft.md");
    t.file("beta.txt");

    // pin 序故意乱：展示序 = 名称字母序；src 未展开 = 折叠根行
    let pins = vec![t.0.join("src"), t.0.join("beta.txt"), t.0.join("docs")];
    let rows = flatten_pins(&t.0, &pins, &HashSet::new(), &[]);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["beta.txt", "docs", "src"]);
    assert!(rows.iter().all(|r| r.depth == 0), "未展开只有三根行");

    // 展开 docs：根行 depth 0 + 子行 depth 1（gitignore 在根目录内生效；
    // .gitignore 自身是普通可见行，v61 dot 文件语义）
    let mut expanded = HashSet::new();
    expanded.insert(t.0.join("docs"));
    let rows = flatten_pins(&t.0, &pins, &expanded, &[]);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "beta.txt",
            "docs",
            ".gitignore",
            "draft.md",
            "guide.md",
            "src"
        ]
    );
    let docs = &rows[1];
    assert!(docs.is_dir && docs.expanded && docs.depth == 0);
    let draft = &rows[3];
    assert_eq!(draft.depth, 1);
    assert!(draft.ignored, "根内 .gitignore 命中的子行置灰");
    assert!(!rows[4].ignored, "guide.md 是普通行");

    // 失效条目（外部已删）兜底跳过
    let stale = vec![t.0.join("nope"), t.0.join("beta.txt")];
    let rows = flatten_pins(&t.0, &stale, &HashSet::new(), &[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "beta.txt");
}

/// 回归（用户 2026-10-17 崩溃）：Pin 收藏夹收到**别的项目**的绝对路
/// 径时，gitignore 匹配不得 strip_prefix 失败就 panic。旧实现
/// `abs_path.strip_prefix(abs_base_path).unwrap()` 直接炸，且 panic
/// 发生在鼠标事件回调里（非 unwind 边界）→ 整个 app 瞬间消失。
#[test]
fn flatten_pins_survives_out_of_project_pin() {
    let a = TempTree::new("pins-other-a");
    let b = TempTree::new("pins-other-b");
    std::fs::write(a.0.join(".gitignore"), "secret.txt\n").unwrap();
    let secret = a.file("secret.txt"); // 另一个项目里被 .gitignore 命中的文件
    let keep = b.file("keep.txt");

    // 视图 cwd = b，pin 里混着 a 的绝对路径：以前当场 panic
    let pins = vec![secret.clone(), keep.clone()];
    let rows = flatten_pins(&b.0, &pins, &HashSet::new(), &[]);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["keep.txt", "secret.txt"]);
    let secret_row = rows.iter().find(|r| r.name == "secret.txt").unwrap();
    assert!(
        !secret_row.ignored,
        "外项目路径不归本项目 .gitignore 管（跳过本层交上层）"
    );

    // 本项目内的命中仍照旧置灰（守卫没把语义一起放掉）
    std::fs::write(b.0.join(".gitignore"), "hidden.txt\n").unwrap();
    let hidden = b.file("hidden.txt");
    let rows = flatten_pins(&b.0, &[hidden], &HashSet::new(), &[]);
    assert!(rows[0].ignored, "本项目 .gitignore 命中仍置灰");
}
