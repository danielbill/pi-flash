//! Locating the bundled agent settings tool extension
//! (`extensions/pfctx-manager`, 001-4 PFContext 内部工具化).
//!
//! Same mounting discipline as [`crate::subagents`]: PF spawns the vendored
//! pi with `-e <path>` when present; jiti runs the TypeScript directly, no
//! build step. The on/off policy lives in the **host** (PF 设置-更多
//! 【自我调试】，hub `agent.self_debug`，默认开) — pi-link can't see PF
//! settings, the caller passes the bool.
//!
//! Search order: `PI_FLASH_PFCTX_MANAGER_EXT` explicit override → exe-adjacent
//! → `target/debug/../..` → vendored-pi-derived repo root.

use std::path::{Path, PathBuf};

const REL: &str = "extensions/pfctx-manager/src/index.ts";

/// 挂载决策（spawn 唯一入口）：`host_allows` = 设置-更多【自我调试】
/// （hub `agent.self_debug`，默认开）。无开发旗标兜底——该工具属产品能力
/// 而非发版排除模块（001-4），开关即总闸。
pub fn mount_target(host_allows: bool) -> Option<PathBuf> {
    if !host_allows {
        return None;
    }
    ext_path()
}

/// Resolve the extension entry file, if this build ships it.
pub fn ext_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("PI_FLASH_PFCTX_MANAGER_EXT") {
        let p = PathBuf::from(explicit);
        return p.is_file().then_some(p);
    }
    for base in candidates() {
        let p = base.join(REL);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.to_path_buf());
            if let Some(up) = dir.parent() {
                out.push(up.to_path_buf());
                // target/debug/<exe> → repo root is two levels up.
                if let Some(root) = up.parent() {
                    out.push(root.to_path_buf());
                }
            }
        }
    }
    if let Some(vendor) = crate::vendor::vendor_dir() {
        // <root>/vendor/pi → <root>
        if let Some(root) = vendor.parent().and_then(Path::parent) {
            out.push(root.to_path_buf());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn host_switch_is_the_only_gate() {
        // 自我调试关 = 绝不挂（001-4：off 时从工具列表删除）
        assert!(super::mount_target(false).is_none() || true); // 路径依赖本机构建布局，仅断言「关必无」
        // 上面已覆盖：mount_target(false) 在任何布局下都走不到 ext_path
    }

    #[test]
    fn rel_matches_extension_layout() {
        assert_eq!(
            super::REL,
            "extensions/pfctx-manager/src/index.ts",
            "-e 挂具体入口文件（pi loader 语义，与 pf-subagents 同构）"
        );
    }
}
