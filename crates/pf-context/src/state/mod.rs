//! state 子集（001-0 §2）：运行期状态——当前会话 / 面板 / 进程 / 在跑子代理。
//!
//! **内存为主，至多写缓存；不投影。丢了大不了重建。** S4 落地（beads
//! pi-flash-nndc.4：当前会话 / 面板 / 运行中子代理计数，UI 不再各自扫全局）；
//! 本切片只占位同生命周期，保证 [`crate::Hub`] 形状稳定。
#[derive(Default)]
pub struct StateStore;
