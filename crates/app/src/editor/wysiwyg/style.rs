//! `MdStyle` → gpui `HighlightStyle`：**规格唯一来源是 `crate::markdown`
//! （渲染器的字号/字重/混色常量），引用不复制**——预览与编辑态观感一致。
//!
//! 注意 gpui `HighlightStyle` 无字号字段：标题字号走行级布局（LineStyle），
//! 不在行内 run 里解决。
