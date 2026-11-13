//! 测试共享工具（cfg(test)）：临时目录 + store 构造 + JSON 读回。

use std::path::{Path, PathBuf};

/// 测试用临时目录：Drop 时自动删除（委托 `tempfile::TempDir`）。
///
/// 注意：`TestDir` 的 Drop 是“尽力删”。如果测试里还持有指向目录内
/// 文件的句柄（如 SQLite 连接），Windows 上 `remove_dir_all` 会失败
/// （文件被占用），`tempfile` 会静默吞掉错误——残留目录极小且不再
/// 增长。测试应做到：连接先 drop，目录后 drop（声明顺序反序）。
///
/// 替代旧的 `temp_dir().join(format!("pf-context-{tag}-{pid}-{nanos}"))`
/// + `create_dir_all` 实现——旧做法测试结束后从不清理，会在 `%TEMP%`
/// 里持续堆积 `pf-context-*` 目录（Windows 不会主动回收用户临时目录）。
/// 这里 `Deref<Target = Path>`，调用方可继续传 `&dir` / `dir.join(..)`。
pub(crate) struct TestDir {
    /// 持有 TempDir 以保证目录存活期覆盖整个测试体。
    _inner: tempfile::TempDir,
    path: PathBuf,
}

impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl TestDir {
    /// 目录路径的独立 `PathBuf` 副本（按值传参 / 交给 store 时用）。
    pub(crate) fn to_path_buf(&self) -> PathBuf {
        self.path.clone()
    }
}

impl std::fmt::Debug for TestDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestDir").field("path", &self.path).finish()
    }
}

/// 创建测试临时目录：`%TEMP%/pf-context-<tag>-XXXXXX`，Drop 时自动删除。
pub(crate) fn tmp_dir(tag: &str) -> TestDir {
    let inner = tempfile::Builder::new()
        .prefix(&format!("pf-context-{tag}-"))
        .tempdir()
        .unwrap_or_else(|e| panic!("创建测试临时目录失败（tag={tag}）：{e}"));
    let path = inner.path().to_path_buf();
    TestDir {
        _inner: inner,
        path,
    }
}

pub(crate) fn store_at(dir: &Path) -> crate::settings::SettingsStore {
    crate::settings::SettingsStore::open(Some(dir.join("app-settings.json")))
}

pub(crate) fn read_json_file(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
