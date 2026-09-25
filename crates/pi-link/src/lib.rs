//! pi-link: typed bridge to a vendored, version-pinned `pi` coding agent.
//!
//! Contract (see PORT_PLAN.md 「内置 pi」):
//! - the ONLY supported runtime is the vendored pi under `vendor/pi`
//!   (`vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js`),
//!   spawned as `node <cli> --mode rpc ...` — never resolved from PATH.
//! - bumping the pin (vendor/pi/package.json + VERSION) is a deliberate release
//!   act gated by the protocol conformance tests in this crate.

pub mod client;
pub mod config;
pub mod protocol;
pub mod sessions;
pub mod skills;
pub mod subagents;
pub mod vendor;

/// Vendored pi version this crate's types/tests are written against.
/// Single source of truth is `vendor/pi/VERSION`; keep both in lockstep.
pub const PI_VENDOR_VERSION: &str = "0.87.1";
