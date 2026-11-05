//! 测试共用夹具（原 sessions.rs 内嵌 testing 模块 / temp_root / BODY，
//! itlq.6 外移集中）。recents 等兄弟模块的测试也经
//! `crate::sessions::testing` 引用（pub(crate) 口径不变）。

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::info::group_name_for_cwd;

/// 标准四条目会话正文（两轮消息 + 改名条目）。
pub(crate) const BODY: &str = concat!(
    "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"hello 世界\"}}\n",
    "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
    "{\"type\":\"session_info\",\"name\":\"renamed\"}\n",
    "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"again\"}}\n",
);

/// 唯一临时目录（进程号 + 纳秒防撞）。
pub(crate) fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pi-link-sess-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 写一个最小会话文件（header + body），返回路径。
pub fn write_session(root: &Path, cwd: &str, id: &str, body: &str) -> PathBuf {
    let group = root.join(group_name_for_cwd(cwd));
    std::fs::create_dir_all(&group).unwrap();
    let header = format!(
        "{{\"type\":\"session\",\"version\":3,\"id\":\"{id}\",\"cwd\":\"{}\"}}",
        cwd.replace('\\', "\\\\")
    );
    let path = group.join(format!("2026-09-26T00-00-00-000Z_{id}.jsonl"));
    std::fs::write(&path, format!("{header}\n{body}")).unwrap();
    path
}
