use super::*;

fn fresh_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pf-migr-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn ledger(name: &str, version: &str) -> Vec<LedgerEntry> {
    vec![LedgerEntry {
        name: name.into(),
        version: version.into(),
        integrity: String::new(),
        installed_at: String::new(),
        source: "market".into(),
        disabled: false,
    }]
}

fn seed_data(dir: &Path, file: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(file), body).unwrap();
}

/// 主场景（090-10 迁移体验）：被本版拒载的 schema 2 旧包，其 `<ver>/data`
/// 必须搬到 `<pkg>/data`——不看清单能否解析，只看台账。
#[test]
fn versioned_data_moves_to_pkg_dir() {
    let root = fresh_root("main");
    let pkg = root.join("@s__quick");
    seed_data(&pkg.join("0.1.4").join("data"), "pins.lst", "a\nb\n");
    std::fs::write(pkg.join("0.1.4").join("pf-plugin.toml"), "schema = 2\n").unwrap();

    let moved = migrate_data_dirs(&root, &ledger("@s/quick", "0.1.4"));
    assert_eq!(moved, vec!["@s/quick".to_string()]);
    assert_eq!(
        std::fs::read_to_string(pkg.join("data/pins.lst")).unwrap(),
        "a\nb\n",
        "数据内容原样到位"
    );
    assert!(!pkg.join("0.1.4/data").exists(), "源目录整体搬走（rename）");
    let _ = std::fs::remove_dir_all(&root);
}

/// 幂等：`<pkg>/data` 已存在就不动——第二次启动、或更新后新版本目录下又
/// 长出 data，都不能覆盖已有数据。
#[test]
fn existing_pkg_data_is_kept() {
    let root = fresh_root("keep");
    let pkg = root.join("@s__quick");
    seed_data(&pkg.join("data"), "pins.lst", "new\n");
    seed_data(&pkg.join("0.1.4").join("data"), "pins.lst", "old\n");

    let moved = migrate_data_dirs(&root, &ledger("@s/quick", "0.1.4"));
    assert!(moved.is_empty());
    assert_eq!(
        std::fs::read_to_string(pkg.join("data/pins.lst")).unwrap(),
        "new\n"
    );
    assert!(pkg.join("0.1.4/data/pins.lst").exists(), "旧数据不被动");
    let _ = std::fs::remove_dir_all(&root);
}

/// 平铺 dev 布局：`<pkg>/` 即清单目录，data 本就在 `<pkg>/data`，天然 no-op；
/// `assets/` 之类非版本形子目录绝不被当版本层。
#[test]
fn flat_layout_is_noop() {
    let root = fresh_root("flat");
    let pkg = root.join("quick");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("pf-plugin.toml"), "schema = 5\n").unwrap();
    seed_data(&pkg.join("assets").join("data"), "x", "x"); // 诱饵：非版本形目录

    let moved = migrate_data_dirs(&root, &ledger("quick", "0.1.5"));
    assert!(moved.is_empty());
    assert!(!pkg.join("data").exists(), "平铺布局不该凭空造 data");
    assert!(pkg.join("assets/data/x").exists(), "诱饵目录不动");
    let _ = std::fs::remove_dir_all(&root);
}

/// 台账版本目录没有 data（或为空）→ 回落到 mtime 最新且非空的版本目录。
#[test]
fn falls_back_to_newest_non_empty_version() {
    let root = fresh_root("fallback");
    let pkg = root.join("@s__quick");
    seed_data(&pkg.join("0.1.2").join("data"), "pins.lst", "v2\n");
    std::fs::create_dir_all(pkg.join("0.1.4").join("data")).unwrap(); // 台账版本：空 data
    std::thread::sleep(std::time::Duration::from_millis(20));
    seed_data(&pkg.join("0.1.3").join("data"), "pins.lst", "v3\n"); // 更晚写入

    let moved = migrate_data_dirs(&root, &ledger("@s/quick", "0.1.4"));
    assert_eq!(moved.len(), 1);
    assert_eq!(
        std::fs::read_to_string(pkg.join("data/pins.lst")).unwrap(),
        "v3\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 哪个版本目录都没有 data → 什么都不做（宿主装载时自己建空目录）。
#[test]
fn nothing_to_migrate_is_noop() {
    let root = fresh_root("none");
    let pkg = root.join("@s__quick").join("0.1.4");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("pf-plugin.toml"), "schema = 2\n").unwrap();

    let moved = migrate_data_dirs(&root, &ledger("@s/quick", "0.1.4"));
    assert!(moved.is_empty());
    assert!(!root.join("@s__quick/data").exists());
    let _ = std::fs::remove_dir_all(&root);
}

// ---- prune_old_versions（mda8：更新后旧版本目录残留 → 双装载）----

/// 只删非台账版本的版本形目录；台账版本、`<pkg>/data`、`.cache`/非版本目录不动。
#[test]
fn prune_removes_only_other_version_dirs() {
    let root = fresh_root("prune");
    let pkg = root.join("@s__quick");
    seed_data(&pkg.join("data"), "pins.lst", "live\n");
    seed_data(&pkg.join("0.1.4"), "pf-plugin.toml", "schema = 2\n");
    seed_data(&pkg.join("0.1.4").join(".cache"), "a.cwasm", "x");
    seed_data(&pkg.join("0.1.5"), "pf-plugin.toml", "schema = 5\n");
    seed_data(&pkg.join("assets"), "bolt.svg", "<svg/>"); // 非版本形目录：诱饵

    let n = prune_old_versions(&pkg, "0.1.5");
    assert_eq!(n, 1);
    assert!(!pkg.join("0.1.4").exists(), "旧版本整目录（含 .cache）删掉");
    assert!(pkg.join("0.1.5/pf-plugin.toml").exists(), "台账版本不动");
    assert_eq!(
        std::fs::read_to_string(pkg.join("data/pins.lst")).unwrap(),
        "live\n"
    );
    assert!(pkg.join("assets/bolt.svg").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// 上版 PF 没迁过：旧版本目录里还躺着唯一一份 data → 先救到包层再删，
/// 绝不丢数据（prune 永远不能跑赢迁移）。
#[test]
fn prune_rescues_unmigrated_data_before_removing() {
    let root = fresh_root("prune-rescue");
    let pkg = root.join("@s__quick");
    seed_data(&pkg.join("0.1.4").join("data"), "pins.lst", "only-copy\n");
    seed_data(&pkg.join("0.1.5"), "pf-plugin.toml", "schema = 5\n");

    assert_eq!(prune_old_versions(&pkg, "0.1.5"), 1);
    assert_eq!(
        std::fs::read_to_string(pkg.join("data/pins.lst")).unwrap(),
        "only-copy\n"
    );
    assert!(!pkg.join("0.1.4").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// 全量版按台账遍历；台账版本目录不存在也不会误删别的（只删「非台账版本」）。
#[test]
fn prune_stale_versions_walks_ledger() {
    let root = fresh_root("prune-all");
    let a = root.join("@s__a");
    seed_data(&a.join("1.0.0"), "pf-plugin.toml", "schema = 5\n");
    seed_data(&a.join("0.9.0"), "pf-plugin.toml", "schema = 5\n");
    let b = root.join("@s__b");
    seed_data(&b.join("2.0.0"), "pf-plugin.toml", "schema = 5\n"); // 台账版本，唯一目录
    let wild = root.join("@s__wild"); // 台账没有：不碰
    seed_data(&wild.join("0.0.1"), "pf-plugin.toml", "schema = 5\n");

    let mut entries = ledger("@s/a", "1.0.0");
    entries.extend(ledger("@s/b", "2.0.0"));
    let out = prune_stale_versions(&root, &entries);
    assert_eq!(out, vec![("@s/a".to_string(), 1)]);
    assert!(!a.join("0.9.0").exists() && a.join("1.0.0").exists());
    assert!(b.join("2.0.0").exists());
    assert!(
        wild.join("0.0.1").exists(),
        "台账外的包不碰（归 sweep_orphans 管）"
    );
    let _ = std::fs::remove_dir_all(&root);
}
