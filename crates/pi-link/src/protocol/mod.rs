//! Typed pi RPC protocol (JSONL over stdio).
//!
//! Reference: vendored pi `docs/rpc.md`, `docs/json.md`, `docs/rpc-commands.md`.
//! Unknown record shapes are preserved as [`Event::Other`] / raw JSON so a pi
//! upgrade never silently drops data — conformance tests fail loudly instead.
//!
//! 按类型族分文件（超规重构 itlq.6，只搬不改 wire 行为）：`command` 出站
//! 命令序列化、`blocks` 消息内容块、`events` 入站事件与扩展 UI、`session`
//! 会话状态/树/条目、`wire` JSONL 行解析入口与文本工具。

mod blocks;
mod command;
mod events;
mod session;
mod wire;

pub use blocks::{Block, ImageData, content_blocks};
pub use command::Command;
pub use events::{AssistantEvent, Event, ExtUiMethod, ExtensionUiRequest, SlashCommand, Usage};
pub use session::{
    BashResult, CostInfo, ModelInfo, SessionEntry, SessionState, SessionStats, TreeNode,
    active_user_entry_ids, parse_entries, parse_model_info, parse_model_list, parse_tree,
};
pub use wire::{parse_line, parse_record, prompt_disposition, strip_ansi};

#[cfg(test)]
mod tests;
