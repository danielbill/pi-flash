//! 测试专用：进程级 env 的互斥锁（仅测试构建编译进测试二进制）。
//!
//! `PI_FLASH_DIR` 等 env 是**进程级**的：一个测试 set_var 的窗口里，并行的
//! 任何读 env 的测试（如 drafts::file_for → paths::dir）都会读到别人的
//! 目录。CI runner 线程调度更散、窗口命中率高——v0.5.1 后首例实录：
//! `drafts::file_name_stable_and_distinct` 撞上 dev_flags 测试的
//! `devflags-all` DirGuard（第一次调用读到默认 `pi-flash-dev`、第三次读到
//! 别人的临时目录，`a == a2` 必炸）。
//!
//! 约定：**动 env 的测试**与**受 env 影响的读测试**一律持有
//! [`env_lock`]（守卫/引用持有到测试体结束）。目录能显式注入的测试照旧
//! 注入（paths/agents/automation 既有做法），不碰 env 就无须加锁。

use std::sync::{Mutex, MutexGuard, PoisonError};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 持有返回的 guard 直到测试体结束；中毒不阻断（并发测试 panic 不许放大成
/// 全套测试连环 panic）。
pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}
