//! state 子集（001-0 §2）：运行期状态——当前会话 / 面板 / 进程 / 在跑子代理。
//!
//! **内存为主，至多写缓存；不投影、不落盘。丢了大不了重建**（进程退出即失）。
//! 真源仍在宿主实体（app `Chat`），各模块变动时把关键值同步进来，读点经
//! [`StateStore::get`] 拿 `Arc` 快照（O(1)、不持锁跨帧安全），UI 不再各自扫
//! 全局。S4 最小集：当前会话 key / 当前 cwd / 面板可见性（dock 键）/ 运行中
//! 子代理计数 / 窗口尺寸快照。

use std::sync::{Arc, RwLock};

/// 运行期状态快照（可重建）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateSnapshot {
    /// 当前 dock 面板键（宿主口径："sessions" / "git" / "plugin:<ix>"）；
    /// `None` = 宿主尚未发布（读点自行回落缺省面板）
    pub dock: Option<String>,
    /// 当前会话键（会话池 active_key 口径）
    pub active_session: Option<String>,
    /// 当前工作目录 / 项目
    pub active_cwd: Option<String>,
    /// 运行中子代理计数（供状态栏/面板；044 面板接线后生效）
    pub running_subagents: u32,
    /// 窗口尺寸快照（退出序列发布）
    pub window: Option<WindowSnapshot>,
}

/// 窗口尺寸快照（与宿主 WindowState 字段同形）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowSnapshot {
    pub w: f64,
    pub h: f64,
    pub maximized: bool,
}

/// state 门面：`Arc` 快照读 + `update` 闭包写。
#[derive(Default)]
pub struct StateStore {
    snap: RwLock<Arc<StateSnapshot>>,
}

impl StateStore {
    /// O(1) 读：Arc 克隆，零锁面跨帧安全。
    pub fn get(&self) -> Arc<StateSnapshot> {
        Arc::clone(&self.snap.read().unwrap())
    }

    /// 更新：闭包改克隆 → 换 `Arc`。同帧多次调用各自成快照（state 高频
    /// 小写，无 coalesce 必要——不落盘，最后一次写即最终值）。
    pub fn update(&self, f: impl FnOnce(&mut StateSnapshot)) {
        let mut guard = self.snap.write().unwrap();
        let mut s = (**guard).clone();
        f(&mut s);
        *guard = Arc::new(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_then_get_snapshot_semantics() {
        let st = StateStore::default();
        assert_eq!(st.get().dock, None);
        st.update(|s| s.dock = Some("git".into()));
        let snap = st.get();
        assert_eq!(snap.dock.as_deref(), Some("git"));
        // Arc 快照不可变：后续 update 不影响已取出的快照
        st.update(|s| s.dock = Some("sessions".into()));
        assert_eq!(snap.dock.as_deref(), Some("git"));
        assert_eq!(st.get().dock.as_deref(), Some("sessions"));
    }

    #[test]
    fn concurrent_get_during_update() {
        let st = Arc::new(StateStore::default());
        let r1 = Arc::clone(&st);
        let reader = std::thread::spawn(move || {
            for _ in 0..2000 {
                let s = r1.get();
                assert!(s.running_subagents <= 100);
            }
        });
        for i in 0..500u32 {
            st.update(move |s| s.running_subagents = i % 10);
        }
        reader.join().unwrap();
        assert_eq!(st.get().running_subagents, 499 % 10);
    }
}
