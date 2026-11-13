//! 测试共享工具（cfg(test)）：临时目录 + store 构造 + JSON 读回。

use std::path::{Path, PathBuf};

/// 测试用临时目录：Drop 时自动删除（委托 `tempfile::TempDir`）。
///
/// 两处配套（缺一个就会在 %TEMP% 留残留）：
/// 1. 写路径「父目录不存在 → 不重建目录树」（settings/doc.rs 的
///    `write_json_no_mkdir`、settings/export.rs、log.rs）——否则收尾后那一次
///    迟到落盘会把刚删的目录整个建回来；
/// 2. Drop 里的重试删除——Windows 上后台线程恰在枚举后新建文件会让
///    `remove_dir_all` 报 os error 145（目录不是空的）。
///
/// 替代旧的 `temp_dir().join(format!("pf-context-{tag}-{pid}-{nanos}"))`
/// + `create_dir_all` 实现——旧做法测试结束后从不清理，会在 `%TEMP%`
/// 里持续堆积 `pf-context-*` 目录（Windows 不会主动回收用户临时目录）。
/// 这里 `Deref<Target = Path>`，调用方可继续传 `&dir` / `dir.join(..)`。
pub(crate) struct TestDir {
    /// 持有 TempDir 以保证目录存活期覆盖整个测试体。
    _inner: Option<tempfile::TempDir>,
    path: PathBuf,
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // TempDir 自己删一次（忽略错误），随后带重试地补删：Windows 上
        // remove_dir_all 是「枚举 + 逐个删」，store 的后台线程（写者 / 初次
        // 投影）恰好在枚举之后又写了一个文件，目录就变成「不是空的」
        // （os error 145）而留在 %TEMP%。后台写是有限次的，等一拍再删即可。
        drop(self._inner.take());
        for _ in 0..20 {
            if !self.path.exists() {
                return;
            }
            let _ = std::fs::remove_dir_all(&self.path);
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
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
        _inner: Some(inner),
        path,
    }
}

pub(crate) fn store_at(dir: &Path) -> crate::settings::SettingsStore {
    crate::settings::SettingsStore::open(Some(dir.join("app-settings.json")))
}

pub(crate) fn read_json_file(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
