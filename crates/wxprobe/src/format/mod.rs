//! 界面层 B（P2）+ P3 菜单/权限选项：纯字符串排版，无 IO、无状态。
//!
//! 逐行对齐 ZCode `packages/services/src/bots/` 的
//! `replyFormatter.ts` / `messages.ts` / `statusFormatting.ts` /
//! `commandParser.ts`，以及 P3 的 `botsService.ts`（[`menu`] 的纯文本菜单与
//! 权限选项语义）。数据来源（pi 会话状态）在 P3 的 `pipeline.rs` 接入。

pub mod menu;
pub mod messages;
pub mod permission;
pub mod reply;
pub mod status;
pub mod summary;
