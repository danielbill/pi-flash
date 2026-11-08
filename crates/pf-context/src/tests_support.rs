//! 测试共享工具（cfg(test)）：临时目录 + store 构造 + JSON 读回。

use std::path::{Path, PathBuf};

pub(crate) fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pf-context-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) fn store_at(dir: &Path) -> crate::settings::SettingsStore {
    crate::settings::SettingsStore::open(Some(dir.join("app-settings.json")))
}

pub(crate) fn read_json_file(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
