//! `~/.pi/agent/models.json` — custom provider/model overrides (pi-web
//! `/api/models-config` parity). The app buffers the whole document in memory
//! and rewrites the file on Save; the helpers here are the pure edits.
//!
//! ```json
//! { "providers": { "glm": { "baseUrl": "...", "api": "openai-completions",
//!     "apiKey": "ENV_OR_KEY", "models": [ { "id": "glm-5.3", "reasoning": true } ] } } }
//! ```

use std::path::PathBuf;

use serde_json::Value;

use crate::config::{parse_lenient, read_json, write_json_private};

pub fn path() -> PathBuf {
    crate::config::agent_dir().join("models.json")
}

pub fn read() -> Result<Value, String> {
    read_json(&path())
}

pub fn write(value: &Value) -> Result<(), String> {
    // providers can carry `apiKey` — same 0600-on-Unix treatment as auth.json
    write_json_private(&path(), value)
}

pub fn providers(value: &Value) -> Vec<(String, &Value)> {
    value
        .get("providers")
        .and_then(|p| p.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v)).collect())
        .unwrap_or_default()
}

pub fn provider_entry<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.get("providers")?.get(name)
}

pub fn provider_models<'a>(entry: &'a Value) -> Vec<&'a Value> {
    entry
        .get("models")
        .and_then(|m| m.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// Ensure `providers` exists and return the provider object for `name`,
/// creating an empty entry when missing.
fn provider_entry_mut<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    let obj = value
        .as_object_mut()
        .expect("models.json document is an object");
    let providers = obj
        .entry("providers")
        .or_insert_with(|| Value::Object(Default::default()));
    providers
        .as_object_mut()
        .expect("providers is an object")
        .entry(name.to_string())
        .or_insert_with(|| Value::Object(Default::default()))
}

fn remove_provider_entry(value: &mut Value, name: &str) -> bool {
    value
        .get_mut("providers")
        .and_then(|p| p.as_object_mut())
        .map(|obj| obj.remove(name).is_some())
        .unwrap_or(false)
}

/// Create or replace a whole provider entry (custom-provider create / field
/// edits that hand back a full object). Returns true when it replaced one.
pub fn upsert_provider(value: &mut Value, name: &str, entry: Value) -> bool {
    let replaced = value.get("providers").and_then(|p| p.get(name)).is_some();
    let slot = provider_entry_mut(value, name);
    *slot = entry;
    replaced
}

/// Rename a provider key, preserving its position in the object.
pub fn rename_provider(value: &mut Value, from: &str, to: &str) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    let obj = value
        .get_mut("providers")
        .and_then(|p| p.as_object_mut())
        .ok_or_else(|| "models.json has no providers".to_string())?;
    if !obj.contains_key(from) {
        return Err(format!("no provider named {from}"));
    }
    if obj.contains_key(to) {
        return Err(format!("provider {to} already exists"));
    }
    // serde_json objects preserve insertion order; rebuild the map in the
    // original order with the key swapped in place.
    let entries: Vec<(String, Value)> = obj
        .iter()
        .map(|(k, v)| {
            let key = if k == from { to.to_string() } else { k.clone() };
            (key, v.clone())
        })
        .collect();
    *obj = entries.into_iter().collect();
    Ok(())
}

pub fn remove_provider(value: &mut Value, name: &str) -> bool {
    remove_provider_entry(value, name)
}

/// Append an empty model entry (`{ "id": "" }` — the sidebar shows it as
/// "新模型") and return its index.
pub fn add_model(value: &mut Value, provider: &str) -> usize {
    let entry = provider_entry_mut(value, provider);
    let obj = entry.as_object_mut().expect("provider entry is an object");
    let models = obj
        .entry("models")
        .or_insert_with(|| Value::Array(Vec::new()));
    let arr = models.as_array_mut().expect("models is an array");
    arr.push(serde_json::json!({ "id": "" }));
    arr.len() - 1
}

/// Overwrite one model entry (Save applies the editor fields).
pub fn update_model(value: &mut Value, provider: &str, index: usize, model: Value) {
    let entry = provider_entry_mut(value, provider);
    if let Some(arr) = entry
        .as_object_mut()
        .and_then(|o| o.get_mut("models"))
        .and_then(|m| m.as_array_mut())
    {
        if index < arr.len() {
            arr[index] = model;
        }
    }
}

pub fn remove_model(value: &mut Value, provider: &str, index: usize) -> bool {
    let entry = provider_entry_mut(value, provider);
    entry
        .as_object_mut()
        .and_then(|o| o.get_mut("models"))
        .and_then(|m| m.as_array_mut())
        .map(|arr| (index < arr.len()).then(|| arr.remove(index)).is_some())
        .unwrap_or(false)
}

/// Lenient re-parse used when loading the file into the editor buffer.
pub fn parse(text: &str) -> Result<Value, String> {
    parse_lenient(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn upsert_add_and_update_models() {
        let mut doc = json!({ "providers": {} });
        assert!(!upsert_provider(
            &mut doc,
            "glm",
            json!({ "baseUrl": "https://x/v1", "api": "openai-completions" })
        ));
        assert_eq!(add_model(&mut doc, "glm"), 0);
        assert_eq!(add_model(&mut doc, "glm"), 1);
        update_model(&mut doc, "glm", 1, json!({ "id": "glm-5.3", "reasoning": true }));
        let entry = provider_entry(&doc, "glm").unwrap();
        assert_eq!(provider_models(entry).len(), 2);
        assert_eq!(entry["models"][1]["reasoning"], true);
        // replace flags
        assert!(upsert_provider(&mut doc, "glm", json!({ "baseUrl": "https://y" })));
        assert!(provider_entry(&doc, "glm").unwrap().get("models").is_none());
    }

    #[test]
    fn rename_keeps_order_and_rejects_collision() {
        let mut doc = json!({ "providers": {
            "aaa": { "baseUrl": "a" }, "glm": { "baseUrl": "g" }, "zzz": { "baseUrl": "z" }
        }});
        rename_provider(&mut doc, "glm", "deepseek").unwrap();
        let names: Vec<String> = providers(&doc).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["aaa", "deepseek", "zzz"]);
        let err = rename_provider(&mut doc, "aaa", "zzz").unwrap_err();
        assert!(err.contains("already exists"));
        assert!(remove_provider(&mut doc, "deepseek"));
        assert!(!remove_provider(&mut doc, "deepseek"));
    }

    #[test]
    fn remove_model_and_missing_provider_are_safe() {
        let mut doc = json!({ "providers": { "p": { "models": [ { "id": "a" } ] } } });
        assert!(remove_model(&mut doc, "p", 0));
        assert!(!remove_model(&mut doc, "p", 0));
        assert!(!remove_model(&mut doc, "nope", 0));
        add_model(&mut doc, "fresh");
        assert_eq!(
            provider_entry(&doc, "fresh").unwrap()["models"][0]["id"],
            ""
        );
    }
}
