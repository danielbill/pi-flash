//! Locating the vendored pi runtime.

use std::path::{Path, PathBuf};

pub const VENDOR_DIR_NAME: &str = "vendor/pi";
pub const NODE_MODULES_REL: &str =
    "node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js";

/// Resolve the vendored pi CLI path.
///
/// Search order:
/// 1. `PI_FLASH_VENDOR_DIR` (explicit override; dev/testing)
/// 2. next to the executable (`<exe_dir>/vendor/pi`, release layout)
/// 3. one level up (`<exe_dir>/../vendor/pi`, `target/debug/` during dev)
/// 4. this crate's workspace layout (compile-time fallback)
pub fn vendor_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("PI_FLASH_VENDOR_DIR") {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return Some(p);
        }
    }
    for base in base_candidates() {
        let p = base.join(VENDOR_DIR_NAME);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// Full path to the vendored pi CLI entry (`dist/bundle/cli.js`).
pub fn cli_path() -> Option<PathBuf> {
    let dir = vendor_dir()?;
    let cli = dir.join(NODE_MODULES_REL);
    cli.is_file().then_some(cli)
}

/// The vendored pi version recorded in `vendor/pi/VERSION`.
pub fn vendored_version() -> Option<String> {
    let dir = vendor_dir()?;
    std::fs::read_to_string(dir.join("VERSION"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn base_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.to_path_buf());
            if let Some(p) = d.parent() {
                v.push(p.to_path_buf());
            }
        }
    }
    // compile-time fallback (dev): <workspace>/crates/pi-link -> <workspace>
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(ws) = manifest.parent().and_then(Path::parent) {
        v.push(ws.to_path_buf());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_resolves_and_version_matches_crate_pin() {
        let dir = vendor_dir().expect("vendor/pi not found — run `npm ci` in vendor/pi");
        assert!(dir.join(NODE_MODULES_REL).is_file(), "cli.js missing");
        let v = vendored_version().expect("vendor/pi/VERSION missing");
        assert_eq!(v, crate::PI_VENDOR_VERSION, "vendor/pi/VERSION out of sync");
    }
}
