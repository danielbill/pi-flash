//! 文件编辑器模块域：编辑编排（文件打开/缓冲区/双态切换，现散于
//! `content.rs` 的 `ensure_file_editor` / `file_editor_body`，后置收拢）
//! 与编辑能力子模块。
//!
//! 当前子模块：view（023 fileView 视图，自 content.rs 拆入）+ wysiwyg
//! （Markdown 所见即所得）；vendor 编辑底座见
//! `vendor/gpui-component/src/input/`（rope/光标/undo/IME），本域不碰内核。

pub mod view;
pub mod wysiwyg;
