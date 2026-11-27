//! all-sessions 动态挂载（契约 v5.1 pi_extensions[].scope）的口径锁：
//! 启用中 + scope=all-sessions 才注入；停用（disabled 位）/卸载（清单
/// 消失）/bound 缺省 = 跳过。路径解析吃两档布局（版本层/平铺）。
use super::*;
use std::path::Path;

fn ledger_entry(name: &str, disabled: bool) -> pi_link::pf_plugins::LedgerEntry {
    pi_link::pf_plugins::LedgerEntry {
        name: name.into(),
        version: "0.1.0".into(),
        integrity: String::new(),
        installed_at: String::new(),
        source: "market".into(),
        disabled,
    }
}

/// 版本层布局假包：`<root>/plugins/<pkg_dir>/<ver>/` + 清单 + ext 文件。
/// 返回清单所在目录（版本目录）。
fn seed_pkg(root: &Path, pkg_dir_name: &str, scope: Option<&str>) -> std::path::PathBuf {
    let ver = root.join(pkg_dir_name).join("0.1.0");
    std::fs::create_dir_all(ver.join("pi")).unwrap();
    let scope_line = scope
        .map(|s| format!("scope = \"{s}\"\n"))
        .unwrap_or_default();
    std::fs::write(
        ver.join("pf-plugin.toml"),
        format!(
            "schema = 5\nid = \"{id}\"\ntitle = {{ zh = \"测试\" }}\nversion = \"0.1.0\"\n\n[[contributes.pi_extensions]]\npath = \"pi/index.mjs\"\n{scope_line}",
            id = pkg_dir_name.rsplit("__").next().unwrap_or(pkg_dir_name),
        ),
    )
    .unwrap();
    std::fs::write(ver.join("pi").join("index.mjs"), "export {}").unwrap();
    ver
}

#[test]
fn all_sessions_mount_injected_when_enabled() {
    let root = std::env::temp_dir().join(format!("pf-allmount-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let ver = seed_pkg(&root, "@x__hooky", Some("all-sessions"));
    let out = SessionRuntime::all_sessions_mounts(&[ledger_entry("@x/hooky", false)], &root);
    assert_eq!(out.len(), 1, "启用 + all-sessions 须注入一条");
    let want = ver
        .join("pi")
        .join("index.mjs")
        .to_string_lossy()
        .replace('\\', "/");
    assert_eq!(out[0].replace('\\', "/"), want, "路径须指版本目录内的 ext");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bound_default_and_disabled_and_uninstalled_are_skipped() {
    let root = std::env::temp_dir().join(format!("pf-allmount2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    seed_pkg(&root, "@x__bound", None); // 无 scope = bound 缺省
    seed_pkg(&root, "@x__off", Some("all-sessions")); // 启用侧停用
    // @x__gone 不落盘 = 已卸载
    let ledger = vec![
        ledger_entry("@x/bound", false),
        ledger_entry("@x/off", true),
        ledger_entry("@x/gone", false),
    ];
    let out = SessionRuntime::all_sessions_mounts(&ledger, &root);
    assert!(out.is_empty(), "bound/停用/卸载都不注入，得到 {out:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_bound_is_not_injected() {
    let root = std::env::temp_dir().join(format!("pf-allmount3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    seed_pkg(&root, "@x__explicit", Some("bound"));
    let out = SessionRuntime::all_sessions_mounts(&[ledger_entry("@x/explicit", false)], &root);
    assert!(out.is_empty(), "显式 bound = 绑定语义，动态注入不认领");
    let _ = std::fs::remove_dir_all(&root);
}
