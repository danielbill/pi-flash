//! 消息域（032）：模型层 / 图片 / 卡片 / 行轮渲染 / TextView 包装五件套。
//! 子模块条目经 glob re-export 保持 `messages::X` 旧路径全部可用。

mod compaction;
mod diff_view;
mod images;
mod model;
mod textview;
mod tool_card;
mod turn;
mod user_row;

pub(crate) use compaction::*;
pub(crate) use diff_view::*;
pub(crate) use images::*;
pub(crate) use model::*;
pub(crate) use textview::*;
pub(crate) use tool_card::*;
pub(crate) use turn::*;
pub(crate) use user_row::*;
