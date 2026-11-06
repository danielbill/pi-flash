//! 文件树状态持久化（`__tree`：{ ws_key: { expanded: [相对路径…], pin: bool } }）。
//! 自 ws_state.rs 切出（itlq.9 超规拆分，纯结构搬移）。

use super::*;
use serde_json::Value;
use std::path::Path;
use std::path::PathBuf;

//
// 对齐 Zed：project panel 的展开始终是 **per-worktree 内存态**（
// `expanded_dir_ids: HashMap<WorktreeId, Vec<ProjectEntryId>>`，entry id 每次
// 重建都变，天然不跨会话），切项目不丢、打开项目默认展开根。本仓在此之上
// 加一层磁盘持久化（Zed 不做），让「我展开了 src/crates」跨重启还在。
//
// 存**相对项目根**的路径（"" = 根）：项目搬迁 / 盘符大小写变化时不废，体积也小。

/// 每项目展开条目上限（防御大仓库无限增长；超出丢弃最深项）。
pub const TREE_EXPANDED_CAP: usize = 300;

/// `__tree`：文件树状态（展开集合 + Pin 收藏夹视图开关）。
const WS_TREE_KEY: &str = "__tree";

/// 一个项目的文件树状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeState {
    /// 展开目录（相对项目根；`""` = 根）
    pub expanded: Vec<String>,
    /// 关闭前停在 Pin 收藏夹视图（重启恢复该视图）
    pub pin: bool,
}

/// 纯内核：从 map 读一个项目的文件树状态（无记录 = None）。
pub(crate) fn tree_state_from(map: &serde_json::Map<String, Value>, cwd: &str) -> Option<TreeState> {
    let v = map.get(WS_TREE_KEY)?.get(&ws_key(cwd))?.clone();
    let expanded = v
        .get("expanded")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let pin = v.get("pin").and_then(|b| b.as_bool()).unwrap_or(false);
    Some(TreeState { expanded, pin })
}

/// 读取一个项目的文件树状态（无记录 = None，调用方落默认「只展开根」）。
pub fn tree_state_for(cwd: &str) -> Option<TreeState> {
    tree_state_from(&memory(), cwd)
}

/// 纯内核：把一个项目的文件树状态写进传入 map（默认态 = 移除槽位）。
/// **绝不碰全局缓存/落盘**——落盘职责归调用方（测试用内核，不写任何盘）。
pub(crate) fn tree_state_into_map(map: &mut serde_json::Map<String, Value>, cwd: &str, state: &TreeState) {
    let key = ws_key(cwd);
    let entry = map
        .entry(WS_TREE_KEY.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(serde_json::Map::new());
    }
    let is_default = !state.pin && state.expanded.iter().all(|p| p.is_empty());
    if let Some(obj) = entry.as_object_mut() {
        if is_default {
            obj.remove(&key);
        } else {
            obj.insert(
                key,
                serde_json::json!({
                    "expanded": state.expanded,
                    "pin": state.pin,
                }),
            );
        }
    }
}

/// 整体改写一个项目的文件树状态；完全默认（只展开根且不在 Pin 视图）= 移除槽位。
pub fn save_tree_state(cwd: &str, state: &TreeState) {
    let mut map = memory();
    tree_state_into_map(&mut map, cwd, state);
    set_memory(&map);
}

/// 绝对展开集合 → 存储形态：相对项目根（`""` = 根）、项目外路径丢弃、
/// 稳定排序后截断 [`TREE_EXPANDED_CAP`]（先保根，再按深度/字母）。
pub fn tree_rel_paths(cwd: &Path, expanded: &std::collections::HashSet<PathBuf>) -> Vec<String> {
    let mut rel: Vec<String> = expanded
        .iter()
        .filter(|p| is_under(p, cwd))
        .map(|p| {
            p.strip_prefix(cwd)
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .collect();
    rel.sort_by(|a, b| {
        let depth = |s: &str| s.matches(['\\', '/']).count();
        depth(a).cmp(&depth(b)).then_with(|| a.cmp(b))
    });
    rel.dedup();
    // 根（空串）优先保留
    rel.sort_by_key(|s| !s.is_empty());
    rel.truncate(TREE_EXPANDED_CAP);
    rel
}

/// 存储形态 → 绝对展开集合：解析到 cwd 下，**丢弃项目外 / 已不存在**的条目
/// （磁盘目录已删、旧版绝对路径残留），恒含根，超上限截断。
pub fn tree_abs_paths(cwd: &Path, rel: &[String]) -> std::collections::HashSet<PathBuf> {
    let mut out: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    out.insert(cwd.to_path_buf());
    for r in rel.iter().take(TREE_EXPANDED_CAP) {
        if r.is_empty() {
            continue;
        }
        // 非法条目直接丢：绝对路径 / 含 `..`（`cwd.join("..")` 的字符串
        // 前缀仍是 cwd，单靠 is_under 拦不住）
        let rel_path = Path::new(r);
        if rel_path.is_absolute()
            || rel_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let p = cwd.join(r);
        if p.is_dir() && is_under(&p, cwd) {
            out.insert(p);
        }
    }
    out
}
