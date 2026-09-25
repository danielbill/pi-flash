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

/// Node binary resolution (bundled node next to the exe > PI_FLASH_NODE >
/// PATH), shared by the RPC spawner and one-off CLI operations. The bundled
/// lookup accepts `node.exe` (Windows) and `node` (macOS/Linux) so the app
/// bundle is self-contained even when Finder gives it an empty PATH.
pub fn node_bin() -> String {
    std::env::var("PI_FLASH_NODE").ok().or_else(|| {
        std::env::current_exe().ok().and_then(|d| {
            let dir = d.parent()?;
            #[cfg(windows)]
            let candidate = dir.join("node.exe");
            #[cfg(not(windows))]
            let candidate = dir.join("node");
            candidate.is_file().then(|| candidate.to_string_lossy().to_string())
        })
    })
    .unwrap_or_else(|| "node".to_string())
}

/// Run a one-off vendored pi CLI command (`pi install/remove/list ...`) and
/// return combined output. Synchronous — call from a background thread.
pub fn run_cli(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let cli = cli_path().ok_or_else(|| "vendored pi not found".to_string())?;
    let mut cmd = std::process::Command::new(node_bin());
    cmd.arg(&cli).args(args).current_dir(cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.trim().is_empty() {
            format!("pi {} failed ({})", args.join(" "), out.status)
        } else {
            text
        })
    }
}

/// Run a one-off CLI command capturing **stdout only** (stderr reported on
/// failure). Used for `--print` one-shots whose stdout is the payload.
pub fn run_cli_stdout(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let cli = cli_path().ok_or_else(|| "vendored pi not found".to_string())?;
    let mut cmd = std::process::Command::new(node_bin());
    cmd.arg(&cli).args(args).current_dir(cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(if err.trim().is_empty() {
            format!("pi {} failed ({})", args.join(" "), out.status)
        } else {
            err.into_owned()
        })
    }
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
