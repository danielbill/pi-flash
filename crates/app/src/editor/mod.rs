//! 文件编辑器模块域：文件编辑视图 + Markdown 渲染与所见即所得。
//!
//! 子模块：
//! - view：023 fileView 视图（自 content.rs 拆入：file_view/
//!   file_editor_body/面包屑/冲突横幅）；文件打开/缓冲区/file_cache
//!   编排状态仍在 Chat（后置收拢）
//! - markdown：Markdown 渲染器（聊天正文 + md 预览），含 render/
//!   {html,math,mermaid} 富渲染组件
//!   └─ wysiwyg：Markdown 所见即所得（Live Preview，024 设计）
//!
//! vendor 编辑底座见 `vendor/gpui-component/src/input/`
//! （rope/光标/undo/IME），本域不碰内核。

pub mod markdown;
pub mod view;
