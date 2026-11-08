//! Locating the bundled subagent extension (`extensions/pf-subagents`).
//!
//! PF spawns the vendored pi with `-e <path>` when the extension is present, the
//! same way `pf_providers` mounts its provider extension (051). The extension is
//! plain TypeScript: pi loads it with jiti, so no build step is involved.
//!
//! Search order:
//! 1. `PI_FLASH_SUBAGENTS_EXT` (explicit override; the on/off policy lives in
//!    `crate::dev_flags`, not here)
//! 2. next to the executable (`<exe_dir>/extensions/pf-subagents`, release layout)
//! 3. one level up (`target/debug/../..`, `cargo run` during dev)
//! 4. derived from the vendored pi directory (`vendor/pi` → repo root), the
//!    layout used by `cargo run` and by the macOS `.app` bundle

use std::path::{Path, PathBuf};

const REL: &str = "extensions/pf-subagents/src/index.ts";

/// 功能总开关（2026-10-19 发布前拍板；044-0 起默认开、用户面另有模块
/// 开关）。机制在 [`crate::dev_flags`]——env `PI_FLASH_SUBAGENTS` 强开/强关
/// > dev-flags.json `"subagents"` 键 > 关，改动重启生效。
///
/// 注意语义分层：本旗标只是「本构建允许这个模块存在」；**用户**关不关它
/// 看 [`mount_target`] 的 `host_allows`（PF 设置页），两者相与。
pub fn dev_enabled() -> bool {
    crate::dev_flags::enabled("subagents")
}

/// 挂载决策（spawn 唯一入口）：宿主意愿 **且** 开发旗标，才回落到
/// [`ext_path`] 定位。宿主 = PF 设置页模块运行时开关
/// （`services::workspace::subagent_module_active`）；开发旗标是发版排除
/// 兜底，env `PI_FLASH_SUBAGENTS` 仍可强关。
///
/// 2026-11-08 真机 bug：旧实现只查开发旗标，设置页关掉开关后扩展照挂、
/// `agent` / `subagent_*` 四件工具照常注入。宿主意愿必须在这里 AND 进来
/// ——pi-link 看不到 PF 设置，决策由调用方传参。
pub fn mount_target(host_allows: bool) -> Option<PathBuf> {
    decide(host_allows, dev_enabled(), ext_path())
}

/// 纯决策核（测试友好：不读 env/磁盘）：宿主意愿与开发旗标同时成立才
/// 采用定位结果。
fn decide(host_allows: bool, dev_on: bool, path: Option<PathBuf>) -> Option<PathBuf> {
    if host_allows && dev_on { path } else { None }
}

/// Resolve the extension entry file, if this build ships it.
pub fn ext_path() -> Option<PathBuf> {
    // 开关政策归 dev_flags（消费点 gate），本函数只管定位。
    if let Ok(explicit) = std::env::var("PI_FLASH_SUBAGENTS_EXT") {
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
    use std::path::PathBuf;

    fn p() -> Option<PathBuf> {
        Some(PathBuf::from("/ext/index.ts"))
    }

    /// 宿主意愿是硬闸：设置-子代理页关掉就绝不挂（2026-11-08 真机 bug
    /// 的回归锚——旧实现只查 dev 旗标，开关形同虚设）。
    #[test]
    fn decide_requires_host_switch() {
        assert_eq!(super::decide(false, true, p()), None);
        assert_eq!(super::decide(true, true, p()), p());
    }

    /// 开发旗标仍是兜底门：发布包排除 subagents 后宿主意愿再开也不挂。
    #[test]
    fn decide_requires_dev_flag() {
        assert_eq!(super::decide(true, false, p()), None);
        assert_eq!(super::decide(false, false, p()), None);
    }
}
