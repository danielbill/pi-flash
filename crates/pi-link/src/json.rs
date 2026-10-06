//! JSON entry point for everything pi hands us.
//!
//! serde_json's default recursion limit is 128 nesting levels, and `pi` nests
//! per *session entry*: `get_tree` returns the whole conversation as
//! `{entry:{…},children:[{entry:{…},children:[…]}]}`, i.e. ~4–5 JSON levels
//! per turn. Measured on an 85-message session: tree depth 95 → ~400 JSON
//! levels → `recursion limit exceeded`, the entire response line was dropped
//! by `parse_line` and `get_tree` never arrived (so every user message ended
//! up without an entry id, which silently killed the 「新分支」 fork button).
//!
//! pi-web parses the same payload in JS, which has no such ceiling — hence
//! parity means lifting the limit here, not truncating the tree.

use serde::Deserialize;
use serde_json::Value;

/// `serde_json::from_str` without the recursion ceiling.
pub fn parse_value(text: &str) -> Result<Value, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_str(text);
    de.disable_recursion_limit();
    let v = Value::deserialize(&mut de)?;
    de.end()?;
    Ok(v)
}
