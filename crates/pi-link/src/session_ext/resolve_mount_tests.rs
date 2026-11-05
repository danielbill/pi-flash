//! 挂载源自愈单测（090-10 版本层换代；真机 2026-11-03 重启哑火根因）。

use super::*;

fn sandbox() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pi-flash-mountheal-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn plant(pkg_dir: &Path, ver: Option<&str>, rel: &str) -> PathBuf {
    let base = match ver {
        Some(v) => pkg_dir.join(v),
        None => pkg_dir.to_path_buf(),
    };
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("pf-plugin.toml"), "schema = 5\n").unwrap();
    let f = base.join(rel);
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "export function activate() {}").unwrap();
    f
}

/// 真机形态：台账里是旧版本目录的绝对路径，磁盘上只剩新版本目录。
#[test]
fn stale_version_dir_heals_to_current() {
    let root = sandbox();
    let pkg = root.join("plugins").join("@pi-flash-plugins__pf-wxbot");
    let live = plant(&pkg, Some("0.2.0"), "pi/index.mjs");
    // 旧版本目录已不在（市场安装换代时被清掉）
    let stale = pkg.join("0.1.4").join("pi/index.mjs");
    let srcs = vec![
        "npm:pi-freeflow".to_string(),
        stale.to_string_lossy().into_owned(),
    ];
    let (healed, changed) = resolve_sources(&srcs);
    assert!(changed, "版本层重定位必须报变动（调用方要回写台账）");
    assert_eq!(healed[0], "npm:pi-freeflow", "scheme 源原样保留");
    assert!(
        same_source(&healed[1], &live.to_string_lossy()),
        "指向当前版本目录"
    );
    assert!(Path::new(&healed[1]).is_file(), "自愈结果必须是活文件");
    let _ = std::fs::remove_dir_all(&root);
}

/// 平铺 dev 布局（无版本层）路径本来就对：不自愈、不报变动。
#[test]
fn flat_layout_is_left_alone() {
    let root = sandbox();
    let pkg = root.join("plugins").join("pf-wxbot");
    let live = plant(&pkg, None, "pi/index.mjs");
    let srcs = vec![live.to_string_lossy().into_owned()];
    let (healed, changed) = resolve_sources(&srcs);
    assert!(!changed);
    assert_eq!(healed, srcs);
    let _ = std::fs::remove_dir_all(&root);
}

/// 包内文件改名/包已卸载：条目没救了 → 摘掉（绝不能把死路径喂给 pi，
/// 一条死 `-e` 会让 pi 整个退出）。
#[test]
fn dead_plugin_entry_is_dropped() {
    let root = sandbox();
    let pkg = root.join("plugins").join("@x__gone");
    plant(&pkg, Some("0.2.0"), "pi/index.mjs");
    let srcs = vec![
        pkg.join("0.1.0")
            .join("pi/old-entry.mjs")
            .to_string_lossy()
            .into_owned(),
    ];
    let (healed, changed) = resolve_sources(&srcs);
    assert!(changed);
    assert!(healed.is_empty(), "死条目必须摘除");
    let _ = std::fs::remove_dir_all(&root);
}

/// 非插件包的用户自有绝对路径：PF 不替它判生死（原样留着）。
#[test]
fn foreign_absolute_path_is_preserved() {
    let root = sandbox();
    let other = root.join("my-ext").join("index.mjs");
    let srcs = vec![other.to_string_lossy().into_owned()];
    let (healed, changed) = resolve_sources(&srcs);
    assert!(!changed);
    assert_eq!(healed, srcs);
    let _ = std::fs::remove_dir_all(&root);
}

/// 同一文件两条写法（旧版本 + 新版本）：去重成一条（-e 挂两次 = ext 双开）。
#[test]
fn duplicates_collapse() {
    let root = sandbox();
    let pkg = root.join("plugins").join("@x__p");
    let live = plant(&pkg, Some("0.2.0"), "pi/index.mjs");
    let srcs = vec![
        live.to_string_lossy().into_owned(),
        pkg.join("0.1.4")
            .join("pi/index.mjs")
            .to_string_lossy()
            .into_owned(),
    ];
    let (healed, changed) = resolve_sources(&srcs);
    assert!(changed);
    assert_eq!(healed.len(), 1);
    assert!(same_source(&healed[0], &live.to_string_lossy()));
    let _ = std::fs::remove_dir_all(&root);
}
