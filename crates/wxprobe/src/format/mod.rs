//! 界面层 B（P2）：纯字符串排版，无 IO、无状态。
//!
//! 逐行对齐 ZCode `packages/services/src/bots/` 的
//! `replyFormatter.ts` / `messages.ts` / `statusFormatting.ts` / `commandParser.ts`。
//! 数据来源（pi 会话状态）在 P3 的 `pipeline.rs` 接入。

pub mod messages;
pub mod reply;
pub mod status;
pub mod summary;
