//! 文件系统监听 —— notify（Zed `crates/fs` 同款 watcher crate）的轻封装：
//! 递归 watch 一个根目录，相关事件折叠成 `()` 打进 channel；防抖合批在
//! 消费端（startup::spawn_fs_watch_pump，Zed 的 FS_WATCH_LATENCY + 批内
//! 一次抽干语义在那一侧）。
//!
//! `.git` 内部噪音（COMMIT_EDITMSG / objects / hooks 等）在回调里就丢弃，
//! 对齐 Zed worktree `process_events` 的过滤段——不滤会造成 git 面板与树
//! 互相触发刷新死循环。

use std::path::Path;
use std::sync::mpsc::Sender;
use notify::Watcher;

pub type FsWatcher = notify::RecommendedWatcher;

/// 递归 watch `root`；返回的句柄 drop 即停止监听。
pub fn watch(root: &Path, tx: Sender<()>) -> notify::Result<FsWatcher> {
    watch_with(root, notify::RecursiveMode::Recursive, tx)
}

/// 非递归 watch 单个目录：**workspace 之外**打开文件的「单文件监听」。
///
/// Zed 对不在任何 worktree 里的文件会建 single-file worktree（只监听该
/// 文件所在目录），这里是同一语义的轻量版：只为「这个目录里有东西动了」
/// 产生信号，具体哪个文件变了交给 `check_external_file_changes` 按
/// disk_sig 复查（事件类型/原子替换/临时文件都不必区分）。
pub fn watch_dir(dir: &Path, tx: Sender<()>) -> notify::Result<FsWatcher> {
    watch_with(dir, notify::RecursiveMode::NonRecursive, tx)
}

fn watch_with(
    root: &Path,
    mode: notify::RecursiveMode,
    tx: Sender<()>,
) -> notify::Result<FsWatcher> {
    let mut watcher = notify::recommended_watcher(
        move |res: Result<notify::Event, notify::Error>| {
            let Ok(ev) = res else { return };
            let relevant = matches!(
                ev.kind,
                notify::EventKind::Create(_)
                    | notify::EventKind::Modify(_)
                    | notify::EventKind::Remove(_)
            );
            if !relevant {
                return;
            }
            // .git 内部与编辑器临时文件不触发刷新（Zed process_events 过滤段同款）
            let interesting = ev.paths.iter().any(|p| {
                !p.components().any(|c| c.as_os_str() == ".git")
                    && !p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with('~') || (n.starts_with('#') && n.ends_with('#')))
            });
            if interesting {
                // 批内事件只当一次「有变化」信号，消费端负责合批
                let _ = tx.send(());
            }
        },
    )?;
    watcher.watch(root, mode)?;
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows 上 recommended_watcher 对不存在路径会报错——契约测试：
    /// 存在的目录可挂 watch，drop 后不 panic。
    #[test]
    fn watch_roundtrip() {
        let dir = std::env::temp_dir().join(format!("piflash-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = watch(&dir, tx).expect("watch tmp dir");
        std::fs::write(dir.join("probe.txt"), "x").unwrap();
        // 轮询等待事件（watcher 延迟与平台相关）
        let mut got = false;
        for _ in 0..100 {
            if rx.recv_timeout(std::time::Duration::from_millis(50)).is_ok() {
                got = true;
                break;
            }
        }
        assert!(got, "write under watched root should produce a signal");
        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 非递归目录 watch（cwd 外打开文件的单文件监听）：目录内任意文件的
    /// 写入都要出信号——信号只用来触发 disk_sig 复查，不区分具体文件。
    #[test]
    fn watch_dir_signals_on_child_write() {
        let dir = std::env::temp_dir().join(format!("piflash-watchdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 先落地一个兄弟文件：信号是目录级的，兄弟文件动也要报
        let sibling = dir.join("sibling.md");
        std::fs::write(&sibling, "a").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = watch_dir(&dir, tx).expect("watch tmp dir non-recursive");
        std::fs::write(&sibling, "bb").unwrap();
        let mut got = false;
        for _ in 0..100 {
            if rx.recv_timeout(std::time::Duration::from_millis(50)).is_ok() {
                got = true;
                break;
            }
        }
        assert!(got, "write inside watched dir should produce a signal");
        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
