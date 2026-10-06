//! MCP server configuration — `~/.pi/agent/mcp.json` (global) and
//! `<cwd>/.pi/mcp.json` (project) with the shared `mcpServers` shape
//! (vendored pi `extensions/mcp/config` parity). Project entries replace
//! global ones with the same name.
//!
//! ```json
//! { "mcpServers": {
//!     "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
//!     "docs": { "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer ${DOCS_TOKEN}" } }
//! } }
//! ```
//!
//! Every write keeps the rest of the file intact (`enabled: true` and the
//! default exposure are expressed by *removing* the key, like pi does).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::read_json;

pub fn global_path() -> PathBuf {
    crate::config::agent_dir().join("mcp.json")
}

pub fn project_path(cwd: &Path) -> PathBuf {
    cwd.join(".pi").join("mcp.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    Project,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::Global => "全局",
            Scope::Project => "项目",
        }
    }
}

/// One server as the panel lists it (project shadows global by name).
#[derive(Debug, Clone, PartialEq)]
pub struct ServerEntry {
    pub name: String,
    pub scope: Scope,
    /// File that defines this server (writes go there).
    pub file: PathBuf,
    pub enabled: bool,
    pub exposure: Option<String>,
    pub description: Option<String>,
    /// `stdio` command + args, or `http` url.
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    /// Env / header *names* only — values never reach the UI.
    pub env_names: Vec<String>,
    pub header_names: Vec<String>,
    pub cwd: Option<String>,
}

impl ServerEntry {
    pub fn transport(&self) -> &'static str {
        if self.url.is_some() { "HTTP" } else { "stdio" }
    }
}

fn parse_entry(name: String, config: &Value, scope: Scope, file: PathBuf) -> ServerEntry {
    let str_val = |k: &str| config.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let names = |k: &str| -> Vec<String> {
        config
            .get(k)
            .and_then(|v| v.as_object())
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default()
    };
    let args = config
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    ServerEntry {
        enabled: config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        exposure: str_val("exposure"),
        description: str_val("description"),
        command: str_val("command"),
        args,
        url: str_val("url"),
        env_names: names("env"),
        header_names: names("headers"),
        cwd: str_val("cwd"),
        name,
        scope,
        file,
    }
}

/// List global + project servers (read-only; project wins by exact name).
pub fn load(cwd: Option<&Path>) -> (Vec<ServerEntry>, Vec<String>) {
    load_with_paths(&global_path(), cwd.map(project_path).as_deref())
}

/// Same, with explicit files (tests; also lets callers pre-resolve paths).
pub fn load_with_paths(global: &Path, project: Option<&Path>) -> (Vec<ServerEntry>, Vec<String>) {
    let mut errors = Vec::new();
    let mut out: Vec<ServerEntry> = Vec::new();
    for (scope, path) in [(Scope::Global, global), (Scope::Project, project.unwrap_or(global))] {
        if scope == Scope::Project && project.is_none() {
            continue;
        }
        if !path.exists() {
            continue;
        }
        match read_json(&path) {
            Ok(value) => {
            if let Some(servers) = value.get("mcpServers").and_then(|s| s.as_object()) {
                for (name, config) in servers {
                    // project entries replace global ones with the same name
                    out.retain(|e| e.name != *name);
                    out.push(parse_entry(name.clone(), config, scope, path.to_path_buf()));
                }
            }
        }
            Err(e) => errors.push(format!("{}: {}", path.display(), e)),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    (out, errors)
}

/// Serialize a server config object back from an entry (add/edit writes).
pub fn config_object(entry: &ServerEntry) -> Value {
    let mut obj = serde_json::Map::new();
    if let Some(cmd) = &entry.command {
        obj.insert("command".into(), Value::String(cmd.clone()));
        if !entry.args.is_empty() {
            obj.insert(
                "args".into(),
                Value::Array(entry.args.iter().cloned().map(Value::String).collect()),
            );
        }
    }
    if let Some(url) = &entry.url {
        obj.insert("url".into(), Value::String(url.clone()));
    }
    if let Some(cwd) = &entry.cwd {
        obj.insert("cwd".into(), Value::String(cwd.clone()));
    }
    if let Some(desc) = &entry.description {
        obj.insert("description".into(), Value::String(desc.clone()));
    }
    if let Some(exposure) = &entry.exposure {
        if exposure != "codemode" {
            obj.insert("exposure".into(), Value::String(exposure.clone()));
        }
    }
    if !entry.enabled {
        obj.insert("enabled".into(), Value::Bool(false));
    }
    Value::Object(obj)
}

fn servers_mut<'a>(value: &'a mut Value) -> &'a mut serde_json::Map<String, Value> {
    let obj = value
        .as_object_mut()
        .expect("mcp.json document is an object");
    obj.entry("mcpServers")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .expect("mcpServers is an object")
}

/// Add (or replace) a server in one mcp.json, creating the file when missing.
/// Returns true when an entry was replaced.
pub fn add(path: &Path, name: &str, config: Value) -> Result<bool, String> {
    let mut value = read_json(path)?;
    let servers = servers_mut(&mut value);
    let replaced = servers.insert(name.to_string(), config).is_some();
    crate::config::write_json(path, &value)?;
    Ok(replaced)
}

/// Remove a server from the mcp.json that defines it.
pub fn remove(path: &Path, name: &str) -> Result<bool, String> {
    let mut value = read_json(path)?;
    let removed = servers_mut(&mut value).remove(name).is_some();
    if removed {
        crate::config::write_json(path, &value)?;
    }
    Ok(removed)
}

/// Flip one server's `enabled` key (`true` removes the key — pi stores only
/// deviations from the default).
pub fn set_enabled(path: &Path, name: &str, enabled: bool) -> Result<(), String> {
    patch(path, name, |cfg| {
        if enabled {
            cfg.as_object_mut().ok_or("server entry is not an object")?.remove("enabled");
        } else {
            cfg.as_object_mut()
                .ok_or("server entry is not an object")?
                .insert("enabled".into(), Value::Bool(false));
        }
        Ok(())
    })
}

/// Set the tool exposure (`codemode` removes the key — it is the default).
pub fn set_exposure(path: &Path, name: &str, exposure: &str) -> Result<(), String> {
    patch(path, name, |cfg| {
        let obj = cfg.as_object_mut().ok_or("server entry is not an object")?;
        if exposure == "codemode" {
            obj.remove("exposure");
        } else {
            obj.insert("exposure".into(), Value::String(exposure.to_string()));
        }
        Ok(())
    })
}

/// Read-modify-write one server entry in place.
fn patch(
    path: &Path,
    name: &str,
    f: impl FnOnce(&mut Value) -> Result<(), String>,
) -> Result<(), String> {
    let mut value = read_json(path)?;
    let mut cfg = servers_mut(&mut value)
        .get_mut(name)
        .cloned()
        .ok_or_else(|| format!("no server named {name}"))?;
    f(&mut cfg)?;
    servers_mut(&mut value).insert(name.to_string(), cfg);
    crate::config::write_json(path, &value)
}

/// Valid server name (pi: letters, digits, `_`, `-`).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ---------------------------------------------------------------------------
// paste parsing (pi-web mcp-add-helpers subset)
// ---------------------------------------------------------------------------

/// Split a command line respecting single/double quotes.
fn shellwords(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => quote = Some(c),
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                c => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Suggested server name from a URL host or command tail.
fn suggest_name(text: &str) -> String {
    if let Ok(url) = url_host(text) {
        let host = url.trim_start_matches("www.");
        let name: String = host
            .split('.')
            .next()
            .unwrap_or(host)
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        if valid_name(&name) {
            return name;
        }
    }
    "my-server".into()
}

fn url_host(text: &str) -> Result<String, ()> {
    let rest = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .ok_or(())?;
    Ok(rest
        .split(['/', '?', ':'])
        .next()
        .unwrap_or_default()
        .to_string())
}

/// What the paste box was recognized as (shown in the add panel preview).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedSource {
    Json,
    Url,
    CommandLine,
}

/// Parse pasted content into `(suggested name, config, source)`.
///
/// Accepts: a `mcpServers` JSON document (first server), a single server JSON
/// object (`command` or `url` key), a bare http(s) URL, or a command line —
/// optionally prefixed with `pi|claude|codex|gemini mcp add`.
pub fn parse_server_input(text: &str) -> Result<(String, Value, ParsedSource), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("内容为空".into());
    }
    if let Some(cfg) = parse_json_blob(text)? {
        return Ok(cfg);
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return Ok((
            suggest_name(text),
            serde_json::json!({ "url": text }),
            ParsedSource::Url,
        ));
    }
    // command line — strip the `<cli> mcp add` prefixes other clients use
    let mut words = shellwords(text);
    for prefix in [("pi", 3), ("claude", 3), ("codex", 3), ("gemini", 3)] {
        if words.len() > prefix.1
            && words[0].eq_ignore_ascii_case(prefix.0)
            && words[1] == "mcp"
            && words[2] == "add"
        {
            words.drain(0..3);
            break;
        }
    }
    // flags: -s/--scope <v>, -e/--env KEY=VAL (claude); other flags dropped.
    // Only flags *before* the command are stripped — after it, everything is
    // an argument (`npx -y @x/server` must keep `-y`).
    let mut command: Option<String> = None;
    let mut args: Vec<String> = Vec::new();
    let mut env: serde_json::Map<String, Value> = Default::default();
    let mut ix = 0;
    while ix < words.len() {
        if command.is_some() {
            args.extend(words[ix..].iter().cloned());
            break;
        }
        let w = words[ix].clone();
        match w.as_str() {
            "-s" | "--scope" => ix += 2,
            "-e" | "--env" => {
                if let Some(kv) = words.get(ix + 1) {
                    if let Some((k, v)) = kv.split_once('=') {
                        env.insert(k.to_string(), Value::String(v.to_string()));
                    }
                }
                ix += 2;
            }
            f if f.starts_with('-') => ix += 1,
            _ => {
                command = Some(w);
                ix += 1;
            }
        }
    }
    let Some(command) = command else {
        return Err("无法识别：粘贴 JSON、http(s) URL 或命令行".into());
    };
    let mut cfg = serde_json::json!({ "command": command });
    if !args.is_empty() {
        cfg["args"] = Value::Array(args.into_iter().map(Value::String).collect());
    }
    if !env.is_empty() {
        cfg["env"] = Value::Object(env);
    }
    let name = std::path::Path::new(&command)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-server".into());
    let name = if valid_name(&name) { name } else { "my-server".into() };
    Ok((name, cfg, ParsedSource::CommandLine))
}

/// JSON paste: `{"mcpServers": {...}}` (first server) or one server object.
fn parse_json_blob(text: &str) -> Result<Option<(String, Value, ParsedSource)>, String> {
    if !text.starts_with('{') {
        return Ok(None);
    }
    let value = crate::config::parse_lenient(text)
        .map_err(|e| format!("JSON 解析失败: {e}"))?;
    if let Some(servers) = value.get("mcpServers").and_then(|s| s.as_object()) {
        if servers.is_empty() {
            return Err("mcpServers 里没有服务器".into());
        }
        let servers: Vec<(&String, &Value)> = servers.iter().collect();
        // Multi-server pastes add one at a time (pi-web asks to pick; the
        // panel shows the first and the user re-pastes for the rest).
        let (name, cfg) = servers[0];
        return Ok(Some((name.clone(), cfg.clone(), ParsedSource::Json)));
    }
    if value.get("command").is_some() || value.get("url").is_some() {
        let name = value
            .get("command")
            .and_then(|c| c.as_str())
            .and_then(|c| {
                std::path::Path::new(c)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .filter(|n| valid_name(n))
            .unwrap_or_else(|| "my-server".into());
        return Ok(Some((name, value, ParsedSource::Json)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmpfile(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-flash-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn load_merges_project_over_global() {
        let base = std::env::temp_dir().join(format!("pi-flash-mcp-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let cwd = base.join("proj");
        std::fs::create_dir_all(cwd.join(".pi")).unwrap();
        let global = base.join("mcp.json");
        let project = cwd.join(".pi").join("mcp.json");
        std::fs::write(&global, r#"{"mcpServers":{"fs":{"command":"npx"},"docs":{"url":"https://x/mcp"}}}"#).unwrap();
        std::fs::write(&project, r#"{"mcpServers":{"fs":{"command":"bunx","enabled":false}}}"#).unwrap();
        let (servers, errors) = load_with_paths(&global, Some(&project));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(servers.len(), 2); // project `fs` replaced global `fs`
        let fs = servers.iter().find(|s| s.name == "fs").unwrap();
        assert_eq!(fs.scope, Scope::Project);
        assert_eq!(fs.command.as_deref(), Some("bunx"));
        assert!(!fs.enabled);
        assert_eq!(fs.file, project);
        let docs = servers.iter().find(|s| s.name == "docs").unwrap();
        assert_eq!(docs.transport(), "HTTP");
        assert_eq!(docs.scope, Scope::Global);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn add_remove_patch_roundtrip() {
        let path = tmpfile("mcp.json");
        let _ = std::fs::remove_file(&path);
        assert!(!add(&path, "fs", json!({"command":"npx"})).unwrap());
        assert!(add(&path, "fs", json!({"command":"bunx"})).unwrap());
        set_enabled(&path, "fs", false).unwrap();
        set_exposure(&path, "fs", "direct").unwrap();
        let (servers, _) = load_with_paths(&path, None);
        let fs = servers.iter().find(|s| s.name == "fs").unwrap();
        assert!(!fs.enabled);
        assert_eq!(fs.exposure.as_deref(), Some("direct"));
        // enabling removes the key, keeping the rest
        set_enabled(&path, "fs", true).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("exposure"));
        assert!(!raw.contains("\"enabled\""));
        assert!(remove(&path, "fs").unwrap());
        assert!(!remove(&path, "fs").unwrap());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parse_url_command_and_json() {
        let (name, cfg, src) = parse_server_input("https://mcp.example.com/sse").unwrap();
        assert_eq!(name, "mcp");
        assert_eq!(cfg["url"], "https://mcp.example.com/sse");
        assert_eq!(src, ParsedSource::Url);

        let (name, cfg, src) =
            parse_server_input("claude mcp add -s user -e KEY=1 npx -y @x/server").unwrap();
        assert_eq!(name, "npx");
        assert_eq!(src, ParsedSource::CommandLine);
        assert_eq!(cfg["command"], "npx");
        assert_eq!(cfg["args"][0], "-y");
        assert_eq!(cfg["env"]["KEY"], "1");

        let (name, cfg, src) =
            parse_server_input(r#"{"mcpServers":{"docs":{"url":"https://a/mcp"}}}"#).unwrap();
        assert_eq!(name, "docs");
        assert_eq!(cfg["url"], "https://a/mcp");
        assert_eq!(src, ParsedSource::Json);

        let (name, cfg, _) = parse_server_input(r#"{"command":"uvx","args":["mcp-fetch"]}"#).unwrap();
        assert_eq!(name, "uvx");
        assert_eq!(cfg["args"][0], "mcp-fetch");

        assert!(parse_server_input("   ").is_err());
        assert!(parse_server_input(r#"{"mcpServers":{}}"#).is_err());
        // flags consume the whole line — no command left to run
        assert!(parse_server_input("-s user").is_err());
    }

    #[test]
    fn shellwords_respect_quotes_and_name_rules() {
        assert_eq!(
            shellwords(r#"uvx --from "a b" pkg"#),
            vec!["uvx", "--from", "a b", "pkg"]
        );
        assert!(valid_name("fs-server_1"));
        assert!(!valid_name("a b"));
        assert!(!valid_name(""));
    }
}
