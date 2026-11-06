//! Pin 收藏夹展平（021 三改：Pin = 与「只看变更」同级的独立视图，本质是
//! 文件树过滤器）。自 file_tree.rs 切出（itlq.9 超规拆分，纯结构搬移）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::services::git::{GitFile, GitStatus};

use super::{
    IgnoreKind, TreeRow, build_gitignore_at, changed_ancestor_dirs, root_ignore_stack, walk,
};

/// Pin 收藏夹视图（021 三改：Pin = 与「只看变更」同级的独立视图，本质是
/// 文件树过滤器）——每个收藏条目是一棵根树：根行 depth 0（name =
/// file_name，**不链式折叠**——pin 的是显式节点），展开的目录从该路径
/// `walk(depth 1)` 下钻，子级排序/gitignore/链式折叠全复用主树语义。
/// 渲染层以 `depth == 0` 判定收藏根行（子行恒 depth ≥ 1）。
/// 根行失效（外部已删）跳过；调用方（toggle）负责清失效条目，这里只兜底。
pub fn flatten_pins(
    cwd: &Path,
    pins: &[PathBuf],
    expanded: &HashSet<PathBuf>,
    git_files: &[GitFile],
) -> Vec<TreeRow> {
    let git_map: HashMap<PathBuf, GitStatus> = git_files
        .iter()
        .map(|f| (f.path.clone(), f.status))
        .collect();
    // 无根行，cwd 不进 changed_dirs（主树 flatten 的「根圆点」语义不适用）
    let changed_dirs = changed_ancestor_dirs(cwd, git_files);
    let stack = root_ignore_stack(cwd);

    // 展示序：名称字母序（大小写不敏感），同名按完整路径（Windows 盘内
    // 路径大小写不敏感，键用小写化）。非绝对路径不进视图——gitignore 匹配
    // （is_abs_path_ignored）按绝对路径 strip_prefix，收相对路径必炸；正常
    // 入口（tree_pin_toggle）已归一，这里兜历史脏数据。
    let mut roots: Vec<&PathBuf> = pins
        .iter()
        .filter(|p| p.exists() && p.is_absolute())
        .collect();
    roots.sort_by(|a, b| {
        let ka = a
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| a.to_string_lossy().to_lowercase());
        let kb = b
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| b.to_string_lossy().to_lowercase());
        ka.cmp(&kb)
            .then_with(|| a.to_string_lossy().cmp(&b.to_string_lossy()))
    });

    let mut out = Vec::new();
    for p in roots {
        let is_dir = p.is_dir();
        let expanded_root = is_dir && expanded.contains(p);
        let ignored = stack.is_abs_path_ignored(p, is_dir);
        out.push(TreeRow {
            path: p.clone(),
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string()),
            depth: 0,
            is_dir,
            expanded: expanded_root,
            git: git_map.get(p).copied(),
            changed_dot: changed_dirs.contains(p),
            ignored,
        });
        if expanded_root {
            // 根已 ignored → 子级整体继承（walk 的 parent_ignored 同语义）；
            // 否则压入根目录自己的 .gitignore 再下钻
            let child_stack = if ignored {
                stack.clone()
            } else {
                let mut s2 = stack.clone();
                if let Some(gi) = build_gitignore_at(p, ".gitignore") {
                    s2 = s2.append(IgnoreKind::Gitignore(Arc::from(p.as_path())), Arc::new(gi));
                }
                s2
            };
            walk(
                p,
                1,
                expanded,
                &git_map,
                &changed_dirs,
                &child_stack,
                ignored,
                &mut out,
            );
        }
    }
    out
}
