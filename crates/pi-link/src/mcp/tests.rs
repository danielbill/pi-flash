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
    std::fs::write(
        &global,
        r#"{"mcpServers":{"fs":{"command":"npx"},"docs":{"url":"https://x/mcp"}}}"#,
    )
    .unwrap();
    std::fs::write(
        &project,
        r#"{"mcpServers":{"fs":{"command":"bunx","enabled":false}}}"#,
    )
    .unwrap();
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
fn raw_entry_keeps_values_and_unknown_fields() {
    let path = tmpfile("mcp-raw.json");
    let _ = std::fs::remove_file(&path);
    add(
        &path,
        "fs",
        json!({"command":"npx","env":{"TOKEN":"secret"},"custom":1}),
    )
    .unwrap();
    let raw = raw_entry(&path, "fs").unwrap().unwrap();
    assert_eq!(raw["env"]["TOKEN"], "secret");
    assert_eq!(raw["custom"], 1);
    assert!(raw_entry(&path, "nope").unwrap().is_none());
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
