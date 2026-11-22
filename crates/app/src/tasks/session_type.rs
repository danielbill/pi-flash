//! 会话类型标记扩展的分发（046；机制照 mode/mod.rs plan_mode.ts 同款：
//! include_str! + 三级路径解析 + spawn `-e` 挂载，自身按 env 无值零动作）。
//! 事实源与过滤口径见 session_type.ts 头注与 046 设计文档 §2.3。

use std::path::{Path, PathBuf};

pub(crate) const SESSION_TYPE_TS: &str = include_str!("session_type.ts");

/// session_type.ts 的磁盘绝对路径 —— `-e` 只吃真实文件路径，不吃内存。
///
/// 解析顺序（与 `crate::mode::script_path` 同机制）：
/// 1. `PI_FLASH_SESSION_TYPE_TS` 显式覆盖（探针/调试）；
/// 2. 开发工作区源文件（编译期路径，改完即生效、无需重建）；
/// 3. 把内置内容落到 `~/.pi-flash/session_type.ts`（内容变了才写）——
///    release 包不带源码目录，靠这一步保证 `-e` 有路径可用。
pub(crate) fn script_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PI_FLASH_SESSION_TYPE_TS") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("tasks")
        .join("session_type.ts");
    if dev.is_file() {
        return Some(dev);
    }
    let dir = pi_link::paths::ensure_dir()?;
    let path = dir.join("session_type.ts");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SESSION_TYPE_TS) {
        std::fs::write(&path, SESSION_TYPE_TS).ok()?;
    }
    Some(path)
}

/// 全档位追加的挂载参数：`-e <session_type.ts>`（无 env 的会话零动作）。
pub(crate) fn mount_args() -> Vec<String> {
    match script_path() {
        Some(p) => vec!["-e".into(), p.to_string_lossy().into_owned()],
        None => {
            eprintln!(
                "session-type: session_type.ts 解析失败（无 dev 资产且 ~/.pi-flash 不可写）\
                 ——任务会话将按 user 显形"
            );
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_args_pair() {
        if std::env::var("PI_FLASH_SESSION_TYPE_TS").is_ok() {
            return; // 显式覆盖环境跳过精确断言，只验形状
        }
        let args = mount_args();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "-e");
        assert!(args[1].ends_with("session_type.ts"));
        assert!(Path::new(&args[1]).is_file(), "挂载路径必须真实存在");
    }

    #[test]
    fn ts_contract() {
        // 扩展契约自检（mode/mod.rs 同款）：改键名/事件名先改这里
        assert!(SESSION_TYPE_TS.contains("PF_SESSION_TYPE"));
        assert!(SESSION_TYPE_TS.contains("pf/session-type"));
        assert!(SESSION_TYPE_TS.contains("appendCustomEntry"));
        assert!(SESSION_TYPE_TS.contains("session_start"));
    }
}
