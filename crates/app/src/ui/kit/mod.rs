//! UI 套件（页面级组件库）——**页面只从这里取控件**，禁止就地 `gpui::div()` 手搓。
//!
//! 为什么有这层（2026-10-20 用户定夺）：组件原先长在 `settings/widgets.rs`，
//! 别的域要用得跨域引用，膈应 → agent 干脆自己拼按钮/开关/行，于是同一个页面
//! 里 gap 出现 4/6/8 三种、圆角 5/6 混用、等宽各算各的（040-1 返工一整天的
//! 直接原因）。现在统一上移到 UI 库，任何页面 `use crate::ui::kit::*;` 即可。
//!
//! 规矩：
//! - **新页面只许调这里的函数**；缺什么先加在这里（尺寸同时入
//!   `docs/UI设计/UI组件规范.md` §10），不要就地拼；
//! - 视觉/尺寸的唯一来源是 §10 实测表 + `ui::tokens`；
//! - settings 只在 `settings/widgets.rs` 留 `select_section` 这类与
//!   SettingsPanel 强绑的逻辑，公共件全部转发到这里。
//!
//! 目录：`button` 按钮 / `switch` 开关 / `toggle` 工具条开关钮 / `row` 行与
//! 文本 / `layout` 两栏骨架 / `badge` 徽标与状态点 / `checkbox` 勾选框 /
//! `slider` 连续量。

mod badge;
mod button;
mod checkbox;
mod layout;
mod row;
mod slider;
mod switch;
mod toggle;

pub(crate) use badge::*;
pub(crate) use button::*;
pub(crate) use checkbox::*;
pub(crate) use layout::*;
pub(crate) use row::*;
pub(crate) use slider::*;
pub(crate) use switch::*;
pub(crate) use toggle::*;
