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

/// 功能总开关（2026-10-19 发布前拍板：subagent 是开发中功能，**默认关**，
/// 发布包即关态）。机制在 [`crate::dev_flags`]——env `PI_FLASH_SUBAGENTS`
/// 强开/强关 > dev-flags.json `"subagents"` 键 > 关，改动重启生效。
pub fn dev_enabled() -> bool {
    crate::dev_flags::enabled("subagents")
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
