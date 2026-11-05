//! 会话身份与路径：group 目录编码、大小写归一（on-disk 形态）、子代理
//! 标记判定、sessions root 定位。

use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 子代理子会话的专属标记（custom entry 的 customType，由扩展在子会话
/// session_start 时落盘）。与父会话里的 `subagent/descriptor` 是两个东西：
/// descriptor 记录委派事实（落父会话），marker 标记"这个会话是子代理"（落
/// 子会话自己）。侧栏过滤只认后者。
pub(super) const SUBAGENT_CHILD_MARKER: &str = "subagent/child";

/// 子会话头部判定窗口（[`is_subagent_session`]）：marker 由扩展在子会话
/// session_start 时 appendEntry，位置紧随 header，必在头部几十行内——
/// 按前 64KB 有界扫即可判，免整文件读（面板逐行点击热路径）。
const SUBAGENT_HEAD_SCAN: u64 = 64 * 1024;

/// 该会话文件是否是子代理子会话（044-0 只读观察窗的打开守卫）：头部窗口
/// 内出现 `subagent/child` custom entry 即是。header 的 parentSession 不可
/// 用作判定——pi 原生 fork 的分支同样带它。文件不存在/读不了 → false
/// （按普通会话开，行为不变）。
pub fn is_subagent_session(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let take = len.min(SUBAGENT_HEAD_SCAN) as usize;
    let mut buf = vec![0u8; take];
    if f.read_exact(&mut buf).is_err() {
        return false;
    }
    let Ok(text) = String::from_utf8(buf) else {
        return false;
    };
    text.split_inclusive('\n').any(|line| {
        serde_json::from_str::<Value>(line.trim())
            .ok()
            .is_some_and(|v| {
                v["type"].as_str() == Some("custom")
                    && v["customType"].as_str() == Some(SUBAGENT_CHILD_MARKER)
            })
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub id: String,
    pub cwd: String,
    pub modified: SystemTime,
    /// first user message text (used as the row label, like pi-web)
    pub preview: String,
    pub message_count: u64,
    /// session name (pi `session_info` entry; pi-web `session.name` — the
    /// row label prefers it over `preview` when set)
    pub name: Option<String>,
}

/// Group directory name for a cwd: separators dashed, wrapped in `--`.
/// Encoding direction only (a `-` in the original path is indistinguishable,
/// so decoding needs the session header's `cwd`).
pub fn group_name_for_cwd(cwd: &str) -> String {
    let encoded: String = cwd
        .chars()
        .map(|c| match c {
            ':' | '\\' | '/' => '-',
            _ => c,
        })
        .collect();
    format!("--{encoded}--")
}

/// group 目录的 on-disk 形态：root 下按大小写不敏感找同名目录，返回
/// read_dir 报告的名字。找不到（未建 / 自定义 session-dir 布局）→ 原样。
///
/// Windows 的 read_dir 保留**调用方传入**的父目录大小写：`list_for_cwd`
/// 用 ws.cwd 拼 group（projects.json 里可能是小写 `d:`），整列行的 path
/// 就跟着那个形态走——与 pi 自报 / 其他入口的形态互相失配。
pub(super) fn canonical_group_dir(root: &Path, group: &Path) -> PathBuf {
    let Some(name) = group.file_name() else {
        return group.to_path_buf();
    };
    let name_lc = name.to_string_lossy().to_lowercase();
    let Ok(rd) = std::fs::read_dir(root) else {
        return group.to_path_buf();
    };
    for entry in rd.flatten() {
        let n = entry.file_name();
        if n.to_string_lossy().to_lowercase() == name_lc {
            return root.join(n);
        }
    }
    group.to_path_buf()
}

/// 归一到「扫描器形态」的会话文件路径：sessions root + group 目录都取
/// on-disk 大小写。pi 会按**进程 cwd** 自拼 session 目录（`--d--…`），
/// 磁盘扫描拿到的是 on-disk 形态（`--D--…`）——NTFS 同一个目录、两串
/// 不同的字符串，而 `Path` 相等比较区分大小写：原样绑定 pi 自报路径的
/// 话，r.file / pool key / 通知点集合与侧栏行永远对不上（行转圈、完成
/// 点、池命中全失灵）。所有「接受会话文件路径」的入口（follow_session_file /
/// open_session）都先过这里。root 匹配与 group 名匹配均大小写不敏感
/// （Windows 口径）；不在 root 下 / 结构不符 / group 不在盘上 → 原样返回。
pub fn canonical_session_path_in(root: &Path, p: &Path) -> PathBuf {
    let p_str = p.to_string_lossy();
    let root_str = root.to_string_lossy();
    // 大小写不敏感的前缀匹配：剩余段从**原串**切（保留原大小写）；
    // get() 兜住极端 to_lowercase 变长字符的非边界切分，失败即原样
    let rest = p_str
        .strip_prefix(&*root_str)
        .or_else(|| {
            p_str.get(..root_str.len()).and_then(|head| {
                head.to_lowercase()
                    .eq(&root_str.to_lowercase())
                    .then(|| p_str.get(root_str.len()..))
                    .flatten()
            })
        })
        .map(|r| r.trim_start_matches(['\\', '/']))
        .filter(|r| !r.is_empty());
    let Some(rest) = rest else {
        return p.to_path_buf();
    };
    let Some((group, file)) = rest.split_once(['\\', '/']) else {
        return p.to_path_buf();
    };
    if group.is_empty() || file.is_empty() {
        return p.to_path_buf();
    }
    canonical_group_dir(root, &root.join(group)).join(file)
}

/// [`canonical_session_path_in`] over the default sessions root; root
/// unavailable → as-is.
pub fn canonical_session_path(p: &Path) -> PathBuf {
    match sessions_root() {
        Some(root) => canonical_session_path_in(&root, p),
        None => p.to_path_buf(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testing::{BODY, temp_root, write_session};

    #[test]
    fn is_subagent_session_keys_on_head_marker() {
        let root = temp_root("subagent-marker");
        // 子会话形状：header + marker + 正文（扩展 session_start 即写 marker）
        let child = write_session(
            &root,
            "D:\\proj-c",
            "child1",
            &format!(
                "{{\"type\":\"custom\",\"customType\":\"subagent/child\",\"id\":\"m1\",\"data\":{{\"provider\":\"spawn\",\"depth\":1}}}}\n{BODY}"
            ),
        );
        assert!(is_subagent_session(&child));
        // 普通会话：无 marker → false（即使带 parentSession header——pi 原生
        // fork 同样有它，不可作判定）
        let normal = write_session(&root, "D:\\proj-c", "normal1", BODY);
        assert!(!is_subagent_session(&normal));
        let forked = write_session(
            &root,
            "D:\\proj-c",
            "fork1",
            &format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"fk\",\"cwd\":\"D:\\proj-c\",\"parentSession\":\"D:\\proj.jsonl\"}}\n{BODY}"
            ),
        );
        assert!(
            !is_subagent_session(&forked),
            "parentSession header 不是判定据"
        );
        // 不存在的文件 → false（按普通会话开，行为不变）
        assert!(!is_subagent_session(&root.join("missing.jsonl")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn group_name_encoding() {
        assert_eq!(
            group_name_for_cwd("D:\\ai_workspace\\pi-flash"),
            "--D--ai_workspace-pi-flash--"
        );
        assert_eq!(group_name_for_cwd("/home/u/x"), "---home-u-x--");
    }

    /// pi 按 cwd 自拼的 group 形态（小写段）→ on-disk 形态；幂等；
    /// root 外 / 无 group 段 / group 不在盘上 → 原样返回。
    #[test]
    fn canonical_session_path_maps_reported_form_to_on_disk_group() {
        let root = temp_root("canon");
        let on_disk = root.join("--D--proj--");
        std::fs::create_dir_all(&on_disk).unwrap();
        let name = "2026-10-19T00-00-00-000Z_x.jsonl";
        std::fs::write(on_disk.join(name), "{}\n").unwrap();
        let reported = root.join("--d--proj--").join(name);
        assert_eq!(
            canonical_session_path_in(&root, &reported),
            on_disk.join(name)
        );
        // 已是 on-disk 形态：幂等
        assert_eq!(
            canonical_session_path_in(&root, &on_disk.join(name)),
            on_disk.join(name)
        );
        // root 外：原样
        let outside = temp_root("canon-out").join("a.jsonl");
        std::fs::write(&outside, "{}\n").unwrap();
        assert_eq!(canonical_session_path_in(&root, &outside), outside);
        let _ = std::fs::remove_dir_all(outside.parent().unwrap());
        // root 本身 / 只有 group 段：原样
        assert_eq!(canonical_session_path_in(&root, &root), root);
        assert_eq!(
            canonical_session_path_in(&root, &root.join("--D--proj--")),
            root.join("--D--proj--")
        );
        // group 不在盘上：原样
        let ghost = root.join("--e--nope--").join(name);
        assert_eq!(canonical_session_path_in(&root, &ghost), ghost);
        let _ = std::fs::remove_dir_all(&root);
    }
}
