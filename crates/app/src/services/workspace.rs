//! Workspace state + app settings persistence.
//!
//! Three-layer config boundary (ARCHITECTURE.md): pi's own `settings.json`
//! (models/credentials/tools — pi's domain) / `pi-flash-app-settings.json`
//! (app appearance: theme, icon theme, fonts, plus lang/sound preferences,
//! 006 界面设置) / `pi-flash-workspace.json` (runtime state: last-open
//! session per workspace, globally-last workspace, window/dock layout,
//! per-project tab strip `__tabs`（启动恢复 tab 带）).
//!
//! IO rules: every write is atomic (tmp + rename via pi_link::config),
//! each file is read once per process (in-process cache), and saves that
//! would write identical content are skipped.

//!
//! 布局（090 重构拆分）：[`store`] 唯一 IO 层 / [`ws_state`] 运行态 /
//! [`app_settings`] 外观设置；本文件只做 re-export，**全部旧路径
//! `crate::services::workspace::X` 保持不变**。

mod app_settings;
mod store;
mod ws_state;

pub use app_settings::*;
pub use store::{TABS_SAVE_CAP, file_sig, is_under, project_name, same_path, same_ws, same_ws_key};
pub use ws_state::*;

pub(crate) use store::{
    WS_LANG_KEY, WS_LAST_KEY, WS_SOUND_KEY, WS_TABS_KEY, app_owned_cwd, load_map_from,
    load_plain_map_from, memory, save_map_to, set_memory, ws_key,
};
