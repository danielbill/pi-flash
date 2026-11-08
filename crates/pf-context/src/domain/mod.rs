//! domain 子集（001-0 §2）：域数据——任务（046）与管家（100）的定义与运行。
//!
//! **SQLite WAL（S5 落地，beads pi-flash-nndc.5）；事务失败即回滚；settings
//! 永不进 DB**（001-0 §4 规则 3：同一 key 不允许既在文件又在数据库）。
//! 本切片只留 trait 形状：DB 实现者后续实现 [`Source`]。

/// 域数据源 trait（S5 由 rusqlite WAL 实现；预留给任务/管家的 DB 装载点）。
#[allow(dead_code)] // S5 接线（beads pi-flash-nndc.5）前先占形状
pub trait Source {}

/// domain 门面（S5 填充）。
#[derive(Default)]
pub struct DomainStore;
