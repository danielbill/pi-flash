//! wxprobe 库 —— 微信 iLink 渠道层 + 界面层
//! （`docs/模块设计/060-远程控制-微信.md`）。
//!
//! 最初是 P0 的独立探针 crate，P3 起由 `crates/app` 依赖复用。
//! 这是同文档 §7 **决策 7 的修订**：原先倾向"整体搬进 app"，实际做法是
//! **保留为 lib + bin**，理由两条：
//!
//! 1. lib 侧 `pub` 项不触发 `dead_code`，`scripts/check_arch.sh` §4 的
//!    `app warnings: 0` 自然满足 —— 不必为"尚未被消费"的模块打补丁；
//! 2. bin 侧保留 `wxprobe qr|scan|recv|loop|send|state|reset` 真机工具与
//!    `wxprobe parity` 对拍 harness（P2 验收 42/42），搬走就都得另找宿主。
//!
//! 模块：
//! * [`wire`]     iLink HTTP 封装（出站白名单 4 域名，响应原样落盘）
//! * [`register`] 扫码两步（`get_bot_qrcode` / `get_qrcode_status`）
//! * [`poller`]   长轮询收发（`getupdates` 90s / `sendmessage`）
//! * [`state`]    状态文件与 `~/.pi-flash/` 路径
//! * [`lock`]     跨进程租约锁（10s 心跳 / 30s 租约）
//! * [`command`]  微信入站命令解析
//! * [`format`]   界面层文案与排版（与 ZCode 源码逐字节对拍）
//! * [`parity`]   对拍 harness（Node 24 type-stripping 跑 ZCode 真实 TS）

pub mod command;
pub mod format;
pub mod lock;
pub mod parity;
pub mod poller;
pub mod register;
pub mod state;
pub mod wire;
