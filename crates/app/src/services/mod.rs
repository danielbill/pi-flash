//! Service layer (pi-web lib/ parity): UI-free helpers owned by name —
//! workspace memory, git, formatting.

pub(crate) mod asset_evict;
pub mod at_file;
pub mod custom_models_store;
pub mod file_icons;
pub mod file_index;
pub mod file_tree;
pub mod format;
pub mod git;
pub(crate) mod mj_doc_cache;
pub(crate) mod models_config;
pub mod paths_sort;
pub(crate) mod pf_state;
pub(crate) mod pfctx_ops;
pub mod pi_extension_updates;
pub(crate) mod script_exec;
pub(crate) mod session_clean;
pub(crate) mod shell_open;
pub mod sound;
pub mod token_probe;
pub mod updater;
pub mod watcher;
#[cfg(windows)]
pub(crate) mod win_pin;
pub mod workspace;
