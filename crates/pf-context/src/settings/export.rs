//! 跨进程投影 `ext-context.json`（001-2）——**S3 落地，本切片不落任何投影文件**。
//!
//! S3 形态（预留契约，勿提前实现）：`exportable()` 为真的 section 按白名单
//! 导出为投影段（带 `generated_at` + `version`），写入走同一条原子写管线、
//! 同内容跳过、半文件不写入；扩展读取契约与 legacy fallback 链见 001-2 §2/§8。
//! S1 过渡期扩展仍读 `agents/config.json`（双写由 app 侧 subagent_prefs 承担）。

use super::Snapshot;

/// 投影落盘（S3 实现）。S1 为无操作占位，保持调用形状稳定。
#[allow(dead_code)] // S3 接线（beads pi-flash-nndc.3）
pub(crate) fn export(_snap: &Snapshot) {}
