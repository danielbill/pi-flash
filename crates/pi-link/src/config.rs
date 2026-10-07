//! pi config-file access for the Models panel (pi-web parity).
//!
//! pi-web's ModelsConfig talks to the SDK in-process: `settings.json`
//! (enabledModels whitelist), `auth.json` (per-provider credentials) and
//! `models.json` (custom providers), all under `getAgentDir()` —
//! `PI_CODING_AGENT_DIR` env or `~/.pi/agent`. Reads tolerate BOM / `//`
//! comments / trailing commas (mirrors pi's loader); writes preserve every
//! unrelated key.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// `getAgentDir()` parity: `PI_CODING_AGENT_DIR` else `~/.pi/agent`.
pub fn agent_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PI_CODING_AGENT_DIR") {
        return PathBuf::from(shelonix::expand_tilde(&dir));
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    Path::new(&home).join(".pi").join("agent")
}

pub fn settings_path() -> PathBuf {
    agent_dir().join("settings.json")
}

pub fn auth_path() -> PathBuf {
    agent_dir().join("auth.json")
}

/// `<cwd>/.pi/settings.json` — project scope; read-only from the panel
/// (pi-web: project settings shadow the global value).
pub fn project_settings_path(cwd: &Path) -> PathBuf {
    cwd.join(".pi").join("settings.json")
}

/// Parse JSON the way pi does: tolerate a BOM, `//` line comments and
/// trailing commas so a file pi accepts never reads as broken here.
pub fn parse_lenient(text: &str) -> Result<Value, String> {
    let text = text.trim_start_matches('\u{feff}');
    let stripped = sanitize(text);
    let value: Value = serde_json::from_str(&stripped).map_err(|e| e.to_string())?;
    Ok(value)
}

/// Strip `//` comments and commas before closing braces/brackets.
fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for rest in chars.by_ref() {
                    if rest == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '}' | ']' => {
                // drop a trailing comma (with the whitespace around it)
                let trimmed = out.trim_end().to_string();
                if trimmed.ends_with(',') {
                    out = trimmed[..trimmed.len() - 1].to_string();
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Read a JSON file (lenient), empty object when missing.
pub fn read_json(path: &Path) -> Result<Value, String> {
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Object(Default::default())),
        Err(e) => return Err(e.to_string()),
    };
    parse_lenient(&text)
}

/// Atomic write (tmp + rename), parent dirs created.
pub fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// enabledModels (settings.json)
// ---------------------------------------------------------------------------

/// `settingsManager.getEnabledModels()`: `None` = unset (every model enabled).
pub fn read_enabled_models(path: &Path) -> Result<Option<Vec<String>>, String> {
    let value = read_json(path)?;
    Ok(value
        .get("enabledModels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        }))
}

/// `settingsManager.setEnabledModels(patterns)`: `None` removes the key.
pub fn write_enabled_models(path: &Path, patterns: Option<Vec<String>>) -> Result<(), String> {
    let mut value = read_json(path)?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "settings.json is not an object".to_string())?;
    match patterns {
        Some(list) => {
            obj.insert("enabledModels".into(), Value::Array(list.into_iter().map(Value::String).collect()));
        }
        None => {
            obj.remove("enabledModels");
        }
    }
    write_json(path, &value)
}

// ---------------------------------------------------------------------------
// auth.json (per-provider credentials)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    ApiKey,
    OAuth,
}

/// Credential types per provider (`CredentialInfo[]` parity, no secrets).
pub fn read_credential_kinds(path: &Path) -> Result<Vec<(String, CredentialKind)>, String> {
    let value = read_json(path)?;
    let mut out = Vec::new();
    if let Some(obj) = value.as_object() {
        for (provider, cred) in obj {
            let kind = if cred.get("type").and_then(|t| t.as_str()) == Some("oauth") {
                CredentialKind::OAuth
            } else {
                CredentialKind::ApiKey
            };
            out.push((provider.clone(), kind));
        }
    }
    Ok(out)
}

/// Store an API key for a provider: `{ "<provider>": { type: "api_key", key } }`.
pub fn set_api_key(path: &Path, provider: &str, key: &str) -> Result<(), String> {
    let mut value = read_json(path)?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "auth.json is not an object".to_string())?;
    obj.insert(
        provider.to_string(),
        serde_json::json!({ "type": "api_key", "key": key }),
    );
    write_json(path, &value)
}

/// DELETE /api/auth/api-key/[provider] parity: refuse to drop an OAuth
/// credential ("is authenticated with OAuth, not an API key").
pub fn remove_credential_if_api_key(path: &Path, provider: &str) -> Result<bool, String> {
    let mut value = read_json(path)?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "auth.json is not an object".to_string())?;
    let is_api_key = obj
        .get(provider)
        .and_then(|c| c.get("type").and_then(|t| t.as_str()))
        != Some("oauth");
    if !is_api_key {
        return Err(format!("{provider} is authenticated with OAuth, not an API key"));
    }
    let removed = obj.remove(provider).is_some();
    if removed {
        write_json(path, &value)?;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// tilde expansion (no external crate)
// ---------------------------------------------------------------------------

mod shelonix {
    /// Expand a leading `~` / `~/` to the user's home directory.
    pub fn expand_tilde(path: &str) -> String {
        if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".into());
            return format!("{home}{}", &path[1..]);
        }
        path.to_string()
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpfile(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-flash-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn lenient_parse_tolerates_bom_comments_trailing_commas() {
        let v = parse_lenient("\u{feff}{\n  // comment\n  \"a\": 1,\n}").unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn enabled_models_roundtrip_and_remove() {
        let path = tmpfile("settings.json");
        std::fs::write(&path, r#"{"theme":"dark","enabledModels":["deepseek/*"]}"#).unwrap();
        let models = read_enabled_models(&path).unwrap();
        assert_eq!(models, Some(vec!["deepseek/*".to_string()]));
        // remove restores a settings file without the key, keeping other keys
        write_enabled_models(&path, None).unwrap();
        assert_eq!(read_enabled_models(&path).unwrap(), None);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"theme\""));
        assert!(!raw.contains("enabledModels"));
        // write back
        write_enabled_models(&path, Some(vec!["openai/*".into()])).unwrap();
        assert_eq!(read_enabled_models(&path).unwrap(), Some(vec!["openai/*".into()]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_model_thinking_and_per_model_levels_parse() {
        let path = tmpfile("settings-defaults.json");
        std::fs::write(
            &path,
            r#"{"defaultProvider":"openrouter","defaultModel":"stealth/space-bunny-alpha",
                "defaultThinkingLevel":"high",
                "modelThinkingLevels":{"glm/glm-5.3-flash":"high","deepseek/deepseek-flash":"low"}}"#,
        )
        .unwrap();
        assert_eq!(
            read_default_model(&path),
            Some(("openrouter".into(), "stealth/space-bunny-alpha".into()))
        );
        assert_eq!(read_default_thinking_level(&path), Some("high".into()));
        let levels = read_model_thinking_levels(&path);
        assert_eq!(levels.len(), 2);
        assert!(levels.contains(&("glm/glm-5.3-flash".into(), "high".into())));
        assert!(levels.contains(&("deepseek/deepseek-flash".into(), "low".into())));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_model_none_when_either_key_missing() {
        let path = tmpfile("settings-partial.json");
        std::fs::write(&path, r#"{"defaultModel":"gpt-5"}"#).unwrap();
        assert_eq!(read_default_model(&path), None);
        assert_eq!(read_default_thinking_level(&path), None);
        assert!(read_model_thinking_levels(&path).is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_settings_file_reads_as_unset() {
        let path = tmpfile("settings-missing.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(read_enabled_models(&path).unwrap(), None);
    }

    #[test]
    fn api_key_crud_and_oauth_guard() {
        let path = tmpfile("auth.json");
        set_api_key(&path, "deepseek", "sk-test").unwrap();
        set_api_key(&path, "openrouter", "sk-or").unwrap();
        let kinds = read_credential_kinds(&path).unwrap();
        assert!(kinds.contains(&("deepseek".into(), CredentialKind::ApiKey)));
        // oauth entries are preserved and refuse deletion via the key path
        std::fs::write(&path, r#"{"github-copilot":{"type":"oauth","refresh":"r","access":"a","expires":1}}"#).unwrap();
        let kinds = read_credential_kinds(&path).unwrap();
        assert_eq!(kinds[0].1, CredentialKind::OAuth);
        let err = remove_credential_if_api_key(&path, "github-copilot").unwrap_err();
        assert!(err.contains("OAuth"), "{err}");
        // api-key delete works
        set_api_key(&path, "deepseek", "sk-test").unwrap();
        assert!(remove_credential_if_api_key(&path, "deepseek").unwrap());
        let kinds = read_credential_kinds(&path).unwrap();
        assert!(kinds.iter().all(|(p, _)| p != "deepseek"));
        let _ = std::fs::remove_file(&path);
    }
}

// ---------------------------------------------------------------------------
// packages + defaultTools (settings.json; plugins/tools panels)
// ---------------------------------------------------------------------------

/// `settingsManager.getGlobalSettings().packages` (string | object entries).
pub fn read_packages(path: &Path) -> Result<Vec<Value>, String> {
    let value = read_json(path)?;
    Ok(value
        .get("packages")
        .and_then(|v| v.as_array())
        .map(|a| a.clone())
        .unwrap_or_default())
}

/// Replace the `packages` array in one scope's settings.json.
pub fn write_packages(path: &Path, packages: Vec<Value>) -> Result<(), String> {
    let mut value = read_json(path)?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "settings.json is not an object".to_string())?;
    obj.insert("packages".into(), Value::Array(packages));
    write_json(path, &value)
}

/// `settings.defaultTools` — the tool allowlist (None = unset, pi default).
pub fn read_default_tools(path: &Path) -> Result<Option<Vec<String>>, String> {
    let value = read_json(path)?;
    Ok(value
        .get("defaultTools")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()))
}

// ---------------------------------------------------------------------------
// builtin extensions (settings.json `extensions`; full+plugins spawn recipe)
// ---------------------------------------------------------------------------

/// pi 的内置扩展（settings.md「The built-in extensions are named
/// `builtin:mcp`, `builtin:llama.cpp`, `builtin:codemode`, and
/// `builtin:tool-search` … They load by default; `-builtin:mcp` disables one」）。
pub const BUILTIN_EXTENSIONS: [&str; 4] = [
    "builtin:mcp",
    "builtin:llama.cpp",
    "builtin:codemode",
    "builtin:tool-search",
];

/// 生效的内置扩展清单（稳定按 [`BUILTIN_EXTENSIONS`] 顺序）。
///
/// full+plugins 档必须发 `-ne`（精确插件集），而 `-ne` 会连内置扩展一起关，
/// 只能逐条 `-e builtin:<name>` 加回来 —— 清单必须跟着 settings 走：用户
/// 用 `-builtin:mcp` 关掉的（本项目用户就用 pi-mcp-adapter 顶替内置 MCP）
/// 不能被他档的配方强行打开，codemode/tool-search 也不能丢。
///
/// 规则同 pi：默认四个全开；`extensions` 数组里 `builtin:x` / `+builtin:x`
/// 开、`-builtin:x` 关；项目 settings 的条目覆盖用户 settings。
pub fn enabled_builtin_extensions(cwd: &Path) -> Vec<String> {
    let user = read_json(&settings_path()).ok();
    let project = read_json(&project_settings_path(cwd)).ok();
    enabled_builtin_extensions_from(user.as_ref(), project.as_ref())
}

/// [`enabled_builtin_extensions`] 的纯函数内核（显式注入两侧 settings，
/// 便于测试且不碰进程环境）。
pub fn enabled_builtin_extensions_from(
    user: Option<&Value>,
    project: Option<&Value>,
) -> Vec<String> {
    let mut on: Vec<String> = BUILTIN_EXTENSIONS.iter().map(|s| s.to_string()).collect();
    for value in [user, project].into_iter().flatten() {
        let Some(entries) = value.get("extensions").and_then(Value::as_array) else {
            continue;
        };
        for raw in entries.iter().filter_map(Value::as_str) {
            let (enable, name) = match raw.strip_prefix('-') {
                Some(rest) => (false, rest),
                None => (true, raw.strip_prefix('+').unwrap_or(raw)),
            };
            if !BUILTIN_EXTENSIONS.contains(&name) {
                continue;
            }
            if enable {
                if !on.iter().any(|x| x == name) {
                    on.push(name.to_string());
                }
            } else {
                on.retain(|x| x != name);
            }
        }
    }
    BUILTIN_EXTENSIONS
        .iter()
        .filter(|b| on.iter().any(|x| x == *b))
        .map(|b| b.to_string())
        .collect()
}

/// `settings.defaultProvider` + `settings.defaultModel` — the model a new
/// session starts with (pi-web /api/models `defaultModel`, consumed by
/// selectInitialModelScope parity on the app side). Both keys must be present.
pub fn read_default_model(path: &Path) -> Option<(String, String)> {
    let value = read_json(path).ok()?;
    let provider = value.get("defaultProvider")?.as_str()?.to_string();
    let model = value.get("defaultModel")?.as_str()?.to_string();
    Some((provider, model))
}

/// `settings.defaultThinkingLevel` — global fallback thinking level for new
/// sessions (pi SettingsManager.getDefaultThinkingLevel).
pub fn read_default_thinking_level(path: &Path) -> Option<String> {
    read_json(path)
        .ok()?
        .get("defaultThinkingLevel")?
        .as_str()
        .map(str::to_string)
}

/// `settings.modelThinkingLevels` — per-`provider/modelId` thinking level pi
/// records when the user picks one (SettingsManager.getModelThinkingLevel).
pub fn read_model_thinking_levels(path: &Path) -> Vec<(String, String)> {
    read_json(path)
        .ok()
        .and_then(|v| v.get("modelThinkingLevels")?.as_object().cloned())
        .map(|obj| {
            obj.into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod builtin_extension_tests {
    use super::*;
    use serde_json::json;

    /// 期望值 = 全部内置扩展（默认口径）
    fn all_builtins() -> Vec<String> {
        BUILTIN_EXTENSIONS.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn defaults_to_every_builtin_extension() {
        let on = enabled_builtin_extensions_from(None, None);
        assert_eq!(on, all_builtins());
    }

    #[test]
    fn user_settings_can_disable_and_enable_builtins() {
        let user = json!({ "extensions": ["-builtin:mcp", "+builtin:codemode"] });
        let on = enabled_builtin_extensions_from(Some(&user), None);
        assert!(!on.iter().any(|x| x == "builtin:mcp"), "-builtin:mcp 应关掉 MCP");
        assert!(on.iter().any(|x| x == "builtin:codemode"));
        // 其它内置扩展不受影响
        assert!(on.iter().any(|x| x == "builtin:tool-search"));
        assert!(on.iter().any(|x| x == "builtin:llama.cpp"));
    }

    #[test]
    fn non_builtin_entries_are_ignored() {
        let user = json!({ "extensions": ["npm:pi-web-access", "./local.ts", "-builtin:nope"] });
        let on = enabled_builtin_extensions_from(Some(&user), None);
        assert_eq!(on, all_builtins());
    }

    #[test]
    fn project_settings_override_user_settings() {
        let user = json!({ "extensions": ["-builtin:mcp", "-builtin:tool-search"] });
        let project = json!({ "extensions": ["builtin:mcp"] });
        let on = enabled_builtin_extensions_from(Some(&user), Some(&project));
        assert!(on.iter().any(|x| x == "builtin:mcp"), "项目 settings 覆盖用户");
        assert!(!on.iter().any(|x| x == "builtin:tool-search"), "项目没覆盖的保持用户口径");
    }
}
