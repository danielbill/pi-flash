use super::install::{INSTALL_PREPARE, run_install_prepare};
use super::*;

/// 钩子接线：注入的函数必须收到**解压后的插件目录**，成功/失败都要透传。
/// （wasm 预编译本身在 app 侧 `install_prepare`，这里只验 pi-link 的接线）
#[test]
fn install_prepare_hook_wiring() {
    fn ok_hook(dir: &Path) -> Result<(), String> {
        std::fs::write(dir.join(".prepared"), b"1").map_err(|e| e.to_string())
    }
    fn fail_hook(_: &Path) -> Result<(), String> {
        Err("预编译失败: boom".into())
    }
    let dir = std::env::temp_dir().join(format!("pf-prep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    install_prepare_hook(ok_hook);
    assert!(run_install_prepare(&dir).is_ok());
    assert!(dir.join(".prepared").is_file(), "钩子必须收到插件目录");

    install_prepare_hook(fail_hook);
    let err = run_install_prepare(&dir).unwrap_err();
    assert!(err.contains("boom"), "失败原因必须透传给安装方：{err}");

    // 未注入 = 不做事（pi-link 单测/CLI 场景）
    if let Ok(mut c) = INSTALL_PREPARE.write() {
        *c = None;
    }
    assert!(run_install_prepare(&dir).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 编译产物缓存绑在插件安装目录内（卸载即清、整目录拷走跟着走）。
#[test]
fn cache_dir_lives_inside_plugin_dir() {
    let p = plugin_cache_for(Path::new("/x/plugins/@s__plug/1.2.3"));
    assert!(p.ends_with(".cache"));
    assert!(p.starts_with("/x/plugins/@s__plug/1.2.3"));
}
