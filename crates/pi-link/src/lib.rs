//! pi-link: typed bridge to a vendored, version-pinned `pi` coding agent.
//!
//! Contract (see PORT_PLAN.md 「内置 pi」):
//! - the ONLY supported runtime is the vendored pi under `vendor/pi`
//!   (`vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js`),
//!   spawned as `node <cli> --mode rpc ...` — never resolved from PATH.
//! - bumping the pin (vendor/pi/package.json + VERSION) is a deliberate release
//!   act gated by the protocol conformance tests in this crate.

pub mod agents;
pub mod automation;
pub mod catalog;
pub mod client;
pub mod config;
pub mod credentials;
pub mod dev_flags;
pub mod estimate;
pub mod extensions;
pub mod hidden_sessions;
pub mod json;
pub mod market;
pub mod mcp;
pub mod model_test;
pub mod models_json;
pub mod models_dev;
pub mod openrouter;
pub mod paths;
pub mod pf_auth;
pub mod pf_plugins;
pub mod pf_providers;
pub mod probe;
pub mod projects;
pub mod protocol;
pub mod recents;
pub mod registry;
pub mod session_ext;
pub mod sessions;
pub mod skills;
pub mod subagent_catalog;
pub mod subagents;
pub mod transcript;
pub mod vendor;

/// Vendored pi version this crate's types/tests are written against.
/// Single source of truth is `vendor/pi/VERSION`; keep both in lockstep.
pub const PI_VENDOR_VERSION: &str = "1.4.0";
