use super::install::remove_tree_keeping_cache;

/// 卸载两铁律：①台账先摘（目录删不掉也不能显示已装）；②留 `.cache`
/// 不算失败（cwasm 被运行中实例 mmap 时 Windows 会拒绝删除）。
#[test]
fn uninstall_clears_ledger_and_survives_busy_cache() {
    let root = std::env::temp_dir().join(format!("pf-uninst-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let base = root.join("@s__plug").join("1.0.0");
    std::fs::create_dir_all(base.join(".cache")).unwrap();
    std::fs::create_dir_all(base.join("assets")).unwrap();
    std::fs::write(base.join("pf-plugin.toml"), "schema = 2\n").unwrap();
    std::fs::write(base.join("plugin.wasm"), b"x").unwrap();
    std::fs::write(base.join(".cache/a.cwasm"), b"y").unwrap();
    std::fs::write(base.join("assets/a.svg"), b"z").unwrap();

    // 只验目录清理规则（台账/路径走全局数据目录，单测里不碰）
    remove_tree_keeping_cache(&base).unwrap();
    assert!(!base.join("pf-plugin.toml").exists(), "清单必须被删");
    assert!(!base.join("plugin.wasm").exists(), "wasm 必须被删");
    assert!(!base.join("assets").exists(), "资产目录必须被清");
    assert!(base.join(".cache/a.cwasm").exists(), "缓存留给启动清扫");
    let _ = std::fs::remove_dir_all(&root);
}
