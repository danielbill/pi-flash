//! Markdown 所见即所得（Live Preview）：Obsidian 式单视图编辑——正文即渲染、
//! 语法字符隐藏（零宽折叠）、光标进入哪段哪段语法显现。
//!
//! 铁律：**rope 源码是文档本体，唯一真相；本模块只产视图装饰，从不改写
//! 文档**。复制/撤销/持久化天然正确。
//!
//! 数据流（每帧 prepaint，O(可见行)，纯函数无状态）：
//!
//! ```text
//! rope → parse.rs (pulldown-cmark) → LineModel[]（行样式 + span + folds）
//!   → fold.rs FoldSet（doc↔vis 映射 + atomic + reveal）
//!   → vendor 三缝：layout_lines（折叠后 shaping）/ highlight_lines（样式 run）
//!     / layout_cursor + movement（光标经 FoldSet 换算）
//! ```
//!
//! 设计文档：`docs/模块设计/024-Markdown所见即所得.md`（分期 P0-P3、
//! vendor 补丁点、编辑语义决策表、风险全在彼处）。

pub mod fold;
pub mod parse;
pub mod style;
pub mod widget;
