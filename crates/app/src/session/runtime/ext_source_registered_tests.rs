//! ext 挂载源注册过滤（tc6）的口径锁：pkg 取自**磁盘目录名**
//! （market `@scope__slug` / dev 平铺短名），台账 name 是 scope 名
//! （`@scope/slug`）——只按 dir_aliases 单向比会永不命中，market 插件
//! ext 全被过滤（真机 2026-11-02：wxbot 出码无人执行）。

use super::*;
use std::path::Path;

fn ledger_entry(name: &str, disabled: bool) -> pi_link::pf_plugins::LedgerEntry {
    pi_link::pf_plugins::LedgerEntry {
        name: name.into(),
        version: "0.1.3".into(),
        integrity: String::new(),
        installed_at: String::new(),
        source: "market".into(),
        disabled,
    }
}

fn seed_versioned_pkg(root: &Path, pkg_dir_name: &str) -> PathBuf {
    let pkg = root.join("plugins").join(pkg_dir_name);
    let ver = pkg.join("0.1.3");
    std::fs::create_dir_all(&ver).unwrap();
    std::fs::write(ver.join("pf-plugin.toml"), "schema = 5\n").unwrap();
    pkg
}

#[test]
fn market_dir_name_matches_scope_ledger_name() {
    let root = std::env::temp_dir().join(format!("pf-extsrc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    seed_versioned_pkg(&root, "@x__wxbot");
    let src = root
        .join("plugins")
        .join("@x__wxbot")
        .join("0.1.3")
        .join("pi")
        .join("index.mjs")
        .to_string_lossy()
        .replace('\\', "/");
    let ledger = vec![ledger_entry("@x/wxbot", false)];
    assert!(
        spawn::ext_source_registered(&src, &ledger),
        "目录名 ↔ scope 名须命中"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dev_flat_short_name_matches_scope_ledger_name() {
    let root = std::env::temp_dir().join(format!("pf-extsrc-flat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    seed_versioned_pkg(&root, "wxbot");
    let src = root
        .join("plugins")
        .join("wxbot")
        .join("0.1.3")
        .join("pi")
        .join("index.mjs")
        .to_string_lossy()
        .replace('\\', "/");
    let ledger = vec![ledger_entry("@x/wxbot", false)];
    assert!(
        spawn::ext_source_registered(&src, &ledger),
        "dev 平铺短名须命中"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn disabled_or_uninstalled_is_filtered() {
    let root = std::env::temp_dir().join(format!("pf-extsrc-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let pkg = seed_versioned_pkg(&root, "@x__wxbot");
    let src = pkg
        .join("0.1.3")
        .join("pi")
        .join("index.mjs")
        .to_string_lossy()
        .replace('\\', "/");
    assert!(
        !spawn::ext_source_registered(&src, &vec![ledger_entry("@x/wxbot", true)]),
        "停用须过滤"
    );
    std::fs::remove_file(pkg.join("0.1.3").join("pf-plugin.toml")).unwrap();
    assert!(
        !spawn::ext_source_registered(&src, &vec![ledger_entry("@x/wxbot", false)]),
        "清单没了（已卸载）须过滤"
    );
    let _ = std::fs::remove_dir_all(&root);
}
