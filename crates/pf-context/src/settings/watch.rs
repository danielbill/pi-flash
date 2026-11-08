//! 外部改动监听（001-0 §5）：`notify` 非递归盯 PF 数据目录 + 内容哈希。
//!
//! - 外部手改：重载并广播（快照换新，UI 经订阅标记下一拍重绘）；
//! - 自写回声：写盘成功后登记**预期哈希窗口**，窗口内的同哈希事件直接吃掉，
//!   避免回环（编辑器一次保存常触发一串事件，另配去抖静默期）；
//! - 同内容重写（touch）：哈希与 `last_seen` 一致 → 忽略；
//! - 多实例：文件是仲裁者，last-writer-wins；检测到对方写入且本进程有未落盘
//!   改动时提示一次——S1 只打日志，状态栏提示留接口（见 `check_external_change`）。

use super::StoreInner;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// 外部事件去抖静默期（一次保存的一串事件合并成一次判定）。
const DEBOUNCE: Duration = Duration::from_millis(150);

/// 启动监听线程（Hub 全局生命周期，不 join 不停机；失败降级为无监听——
/// 外部改动要等下次本进程写盘才会被看见）。
pub(crate) fn spawn(inner: std::sync::Arc<StoreInner>, dir: PathBuf, watch_path: PathBuf) {
    let thread_inner = std::sync::Arc::clone(&inner);
    let handle = std::thread::Builder::new()
        .name("pf-settings-watch".into())
        .spawn(move || watch_loop(thread_inner, dir, watch_path));
    if let Ok(mut w) = inner.watch.lock() {
        w.handle = handle.ok();
    }
}

fn watch_loop(inner: std::sync::Arc<StoreInner>, dir: PathBuf, watch_path: PathBuf) {
    use notify::Watcher as _;

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let Ok(mut watcher) = notify::recommended_watcher(tx) else {
        eprintln!("[settings] notify 初始化失败，外部改动不再自动重载");
        inner
            .logger
            .log("watch", "notify 初始化失败，外部改动不再自动重载");
        return;
    };
    if let Err(e) = watcher.watch(&dir, notify::RecursiveMode::NonRecursive) {
        eprintln!("[settings] 监听 {dir:?} 失败（{e}），外部改动不再自动重载");
        inner
            .logger
            .log("watch", &format!("监听失败（{e}），外部改动不再自动重载"));
        return;
    }
    // 监听器必须活满进程：存进 WatchState（Mutex 内不 poll，只持有）
    if let Ok(mut w) = inner.watch.lock() {
        w.watcher = Some(watcher);
    }
    loop {
        match rx.recv() {
            Ok(Ok(ev)) => {
                // 只关心设置文件本身；无路径事件保守当作命中（判定端有哈希闸）
                let hit = ev.paths.is_empty() || ev.paths.iter().any(|p| p == &watch_path);
                if !hit {
                    continue;
                }
                // 去抖：抽干积压 → 静默期 → 再抽干 → 统一判定一次
                while rx.try_recv().is_ok() {}
                std::thread::sleep(DEBOUNCE);
                while rx.try_recv().is_ok() {}
                inner.check_external_change();
            }
            Ok(Err(e)) => {
                eprintln!("[settings] watch 事件错误（停止监听）: {e}");
                return;
            }
            Err(_) => return, // 通道关闭（发送端已亡）
        }
    }
}
