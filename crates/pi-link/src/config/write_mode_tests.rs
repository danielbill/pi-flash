//! write_json_private lands 0600 on Unix (auth.json / models.json parity
//! with pi's auth-storage). Unix-only: on Windows mode bits are inert.

use super::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("pf-write-mode-{}-{}", tag, std::process::id()))
}

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn private_write_lands_0600_and_dir_0700() {
    let dir = tmp_dir("fresh");
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join("auth.json");
    write_json_private(&path, &json!({ "glm": { "type": "api_key", "key": "k" } })).unwrap();
    assert_eq!(mode_of(&path), 0o600);
    assert_eq!(mode_of(&dir), 0o700);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn private_write_tightens_existing_world_readable() {
    let dir = tmp_dir("heal");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("auth.json");
    // simulate a file widened by an earlier non-private write
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    write_json_private(&path, &json!({})).unwrap();
    assert_eq!(mode_of(&path), 0o600);
    // pre-existing dir mode is left alone (admin-managed), like pi
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn plain_write_stays_default() {
    let dir = tmp_dir("plain");
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join("settings.json");
    write_json(&path, &json!({})).unwrap();
    // umask-independent: same perms as a plain std::fs::write control
    let control = dir.join("control.json");
    std::fs::write(&control, "{}").unwrap();
    assert_eq!(mode_of(&path), mode_of(&control));
    std::fs::remove_dir_all(&dir).ok();
}
