//! Subagent profiles (pi-web subagents.ts parity): markdown files with YAML
//! frontmatter, three built-in profiles, and the global agents settings
//! (`<agentDir>/agents/settings.json`).
//!
//! Scope order + shadowing: builtin < global (`<agentDir>/agents/*.md`) <
//! workspace (`<cwd>/.agents/agents/*.md`) < project (`<cwd>/.pi/agents/*.md`);
//! a same-name file (case-insensitive) replaces anything from a lower scope.

use std::path::{Path, PathBuf};

use crate::config::parse_lenient;

pub const TOOL_OPTIONS: [&str; 7] = ["read", "bash", "edit", "write", "grep", "find", "ls"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentScope {
    Builtin,
    Global,
    Workspace,
    Project,
}

impl SubagentScope {
    pub fn label(self) -> &'static str {
        match self {
            SubagentScope::Builtin => "内置",
            SubagentScope::Global => "全局",
            SubagentScope::Workspace => "工作区",
            SubagentScope::Project => "项目",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubagentProfile {
    pub name: String,
    pub display_name: String,
    pub description: String,
    /// body of the markdown file = system prompt
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub load_skills: bool,
    pub load_extensions: bool,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub max_turns: Option<u32>,
    pub inherit_context: bool,
    pub run_in_background: bool,
    pub enabled: bool,
    pub scope: SubagentScope,
    /// file path (None for built-ins)
    pub file_path: Option<PathBuf>,
    /// true when a higher scope file shadows this profile's name
    pub overridden: bool,
}

/// The three built-in profiles (pi-web lib/subagents.ts constants).
pub fn builtin_profiles() -> Vec<SubagentProfile> {
    let all: Vec<&str> = TOOL_OPTIONS.iter().copied().collect();
    let read_only: Vec<&str> = vec!["read", "grep", "find", "ls"];
    let mk = |name: &str, display: &str, desc: &str, tools: Vec<&str>| SubagentProfile {
        name: name.to_string(),
        display_name: display.to_string(),
        description: desc.to_string(),
        system_prompt: String::new(),
        tools: tools.iter().map(|s| s.to_string()).collect(),
        load_skills: true,
        load_extensions: true,
        model: None,
        thinking: None,
        max_turns: None,
        inherit_context: true,
        run_in_background: false,
        enabled: true,
        scope: SubagentScope::Builtin,
        file_path: None,
        overridden: false,
    };
    vec![
        mk(
            "general-purpose",
            "通用",
            "有全部工具的通用子代理（内置）",
            all,
        ),
        mk("explore", "探索", "只读检索/探索（内置）", read_only.clone()),
        mk("plan", "规划", "只读方案规划（内置）", read_only),
    ]
}

/// `<agentDir>/agents/settings.json`: `{ version, builtInEnabled,
/// disabledBuiltIns, maxConcurrent }` (pi-web subagent-settings.ts).
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentSettings {
    pub builtin_enabled: bool,
    pub disabled_built_ins: Vec<String>,
    pub max_concurrent: u32,
}

impl Default for SubagentSettings {
    fn default() -> Self {
        Self { builtin_enabled: true, disabled_built_ins: Vec::new(), max_concurrent: 10 }
    }
}

pub fn settings_path(agent_dir: &Path) -> PathBuf {
    agent_dir.join("agents").join("settings.json")
}

pub fn read_settings(agent_dir: &Path) -> SubagentSettings {
    let value = match std::fs::read_to_string(settings_path(agent_dir)) {
        Ok(text) => parse_lenient(&text).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };
    SubagentSettings {
        builtin_enabled: value["builtInEnabled"].as_bool().unwrap_or(true),
        disabled_built_ins: value["disabledBuiltIns"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        max_concurrent: value["maxConcurrent"].as_u64().unwrap_or(10).clamp(1, 32) as u32,
    }
}

pub fn write_settings(agent_dir: &Path, settings: &SubagentSettings) -> Result<(), String> {
    let path = settings_path(agent_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let value = serde_json::json!({
        "version": 1,
        "builtInEnabled": settings.builtin_enabled,
        "disabledBuiltIns": settings.disabled_built_ins,
        "maxConcurrent": settings.max_concurrent,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// Profile directories in shadowing order (lowest precedence first).
pub fn profile_directories(cwd: &Path, agent_dir: &Path) -> Vec<(PathBuf, SubagentScope)> {
    vec![
        (agent_dir.join("agents"), SubagentScope::Global),
        (cwd.join(".agents").join("agents"), SubagentScope::Workspace),
        (cwd.join(".pi").join("agents"), SubagentScope::Project),
    ]
}

fn is_md(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|e| e == "md").unwrap_or(false)
}

/// Parse one profile markdown file.
pub fn read_profile_file(path: &Path, scope: SubagentScope) -> Option<SubagentProfile> {
    let text = std::fs::read_to_string(path).ok()?;
    let (fm, body) = split_frontmatter(&text);
    let name = fm
        .iter()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.clone())
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()))?;
    let get_bool = |key: &str, default: bool| -> bool {
        fm.iter().find(|(k, _)| k == key).map(|(_, v)| v == "true").unwrap_or(default)
    };
    Some(SubagentProfile {
        display_name: fm
            .iter()
            .find(|(k, _)| k == "display_name")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| name.clone()),
        description: fm
            .iter()
            .find(|(k, _)| k == "description")
            .map(|(_, v)| v.clone())
            .unwrap_or_default(),
        system_prompt: body.trim().to_string(),
        tools: fm
            .iter()
            .find(|(k, _)| k == "tools")
            .map(|(_, v)| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_else(|| TOOL_OPTIONS.iter().map(|s| s.to_string()).collect()),
        load_skills: get_bool("load_skills", true),
        load_extensions: get_bool("load_extensions", true),
        model: fm.iter().find(|(k, _)| k == "model").map(|(_, v)| v.clone()).filter(|v| !v.is_empty()),
        thinking: fm
            .iter()
            .find(|(k, _)| k == "thinking")
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty()),
        max_turns: fm
            .iter()
            .find(|(k, _)| k == "max_turns")
            .and_then(|(_, v)| v.parse().ok()),
        inherit_context: get_bool("inherit_context", true),
        run_in_background: get_bool("run_in_background", false),
        enabled: get_bool("enabled", true),
        scope,
        file_path: Some(path.to_path_buf()),
        overridden: false,
        name,
    })
}

/// Split a leading `---` frontmatter block into key/value pairs + body.
fn split_frontmatter(text: &str) -> (Vec<(String, String)>, String) {
    if text.starts_with("---") {
        if let Some(close) = text[3..].find("\n---") {
            let block = &text[3..3 + close];
            let body = &text[3 + close + 4..];
            let fm = block
                .lines()
                .filter_map(|line| {
                    let colon = line.find(':')?;
                    let key = line[..colon].trim().to_string();
                    let value = line[colon + 1..].trim().trim_matches('"').to_string();
                    (!key.is_empty()).then_some((key, value))
                })
                .collect();
            return (fm, body.to_string());
        }
    }
    (Vec::new(), text.to_string())
}

/// Serialize a profile to markdown (frontmatter snake_case, body = prompt).
pub fn profile_to_markdown(p: &SubagentProfile) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", p.name));
    out.push_str(&format!("display_name: {}\n", p.display_name));
    if !p.description.is_empty() {
        out.push_str(&format!("description: {}\n", p.description));
    }
    if !p.tools.is_empty() {
        out.push_str(&format!("tools: {}\n", p.tools.join(",")));
    }
    out.push_str(&format!("load_skills: {}\n", p.load_skills));
    out.push_str(&format!("load_extensions: {}\n", p.load_extensions));
    if let Some(model) = &p.model {
        if !model.is_empty() {
            out.push_str(&format!("model: {model}\n"));
        }
    }
    if let Some(thinking) = &p.thinking {
        if !thinking.is_empty() {
            out.push_str(&format!("thinking: {thinking}\n"));
        }
    }
    if let Some(turns) = p.max_turns {
        out.push_str(&format!("max_turns: {turns}\n"));
    }
    out.push_str(&format!("inherit_context: {}\n", p.inherit_context));
    out.push_str(&format!("run_in_background: {}\n", p.run_in_background));
    out.push_str(&format!("enabled: {}\n", p.enabled));
    out.push_str("---\n\n");
    out.push_str(&p.system_prompt);
    out.push('\n');
    out
}

/// Write a profile file (creating parent dirs).
pub fn write_profile_file(path: &Path, p: &SubagentProfile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, profile_to_markdown(p)).map_err(|e| e.to_string())
}

/// Merged profile list (last-write-wins by lowercased name), built-ins first.
/// Built-ins are suppressed when the global feature switch is off or the name
/// is in `disabled_built_ins`.
pub fn list_profiles(
    cwd: &Path,
    agent_dir: &Path,
    settings: &SubagentSettings,
) -> Vec<SubagentProfile> {
    let mut out: Vec<SubagentProfile> = Vec::new();
    if settings.builtin_enabled {
        for mut p in builtin_profiles() {
            p.enabled = !settings.disabled_built_ins.iter().any(|n| n == &p.name);
            out.push(p);
        }
    }
    for (dir, scope) in profile_directories(cwd, agent_dir) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| is_md(p)).collect();
        files.sort();
        for f in files {
            if let Some(p) = read_profile_file(&f, scope) {
                // last-write-wins by lowercased name
                out.retain(|e| e.name.to_lowercase() != p.name.to_lowercase());
                out.push(p);
            }
        }
    }
    // mark shadowed entries (same lowercased name can no longer occur, but a
    // builtin may be replaced by a file — flag those)
    for p in &mut out {
        if p.scope == SubagentScope::Builtin {
            p.overridden = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subagents-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn builtins_present_and_disablable() {
        let mut settings = SubagentSettings::default();
        let cwd = tmpdir("b1");
        let profiles = list_profiles(&cwd, &cwd, &settings);
        assert_eq!(profiles.len(), 3);
        assert!(profiles.iter().all(|p| p.scope == SubagentScope::Builtin));
        let gp = profiles.iter().find(|p| p.name == "general-purpose").unwrap();
        assert!(gp.tools.contains(&"bash".to_string()));
        // feature off -> no builtins at all
        settings.builtin_enabled = false;
        assert!(list_profiles(&cwd, &cwd, &settings).is_empty());
        // per-builtin disable
        settings.builtin_enabled = true;
        settings.disabled_built_ins = vec!["plan".into()];
        let profiles = list_profiles(&cwd, &cwd, &settings);
        // disabled builtins stay listed with enabled=false (ADR 0005)
        assert_eq!(profiles.len(), 3);
        let plan = profiles.iter().find(|p| p.name == "plan").unwrap();
        assert!(!plan.enabled);
    }

    #[test]
    fn file_roundtrip_and_shadowing() {
        let base = tmpdir("b2");
        let agent_dir = base.join("agent");
        let cwd = base.join("proj");
        std::fs::create_dir_all(agent_dir.join("agents")).unwrap();
        std::fs::create_dir_all(cwd.join(".pi").join("agents")).unwrap();

        let global_profile = SubagentProfile {
            name: "reviewer".into(),
            display_name: "Reviewer".into(),
            description: "reviews code".into(),
            system_prompt: "You review code.".into(),
            tools: vec!["read".into(), "grep".into()],
            load_skills: false,
            load_extensions: true,
            model: Some("openai/gpt-5".into()),
            thinking: Some("high".into()),
            max_turns: Some(12),
            inherit_context: false,
            run_in_background: true,
            enabled: true,
            scope: SubagentScope::Global,
            file_path: Some(agent_dir.join("agents").join("reviewer.md")),
            overridden: false,
        };
        write_profile_file(global_profile.file_path.as_ref().unwrap(), &global_profile).unwrap();

        let settings = SubagentSettings::default();
        let profiles = list_profiles(&cwd, &agent_dir, &settings);
        assert_eq!(profiles.len(), 4); // 3 builtins + reviewer
        let parsed = profiles.iter().find(|p| p.name == "reviewer").unwrap();
        assert_eq!(parsed.display_name, "Reviewer");
        assert_eq!(parsed.system_prompt, "You review code.");
        assert_eq!(parsed.tools, vec!["read".to_string(), "grep".to_string()]);
        assert!(!parsed.load_skills);
        assert_eq!(parsed.model.as_deref(), Some("openai/gpt-5"));
        assert_eq!(parsed.thinking.as_deref(), Some("high"));
        assert_eq!(parsed.max_turns, Some(12));
        assert!(!parsed.inherit_context);
        assert!(parsed.run_in_background);
        assert_eq!(parsed.scope, SubagentScope::Global);

        // project file with the same name shadows the global one
        let mut project_profile = global_profile.clone();
        project_profile.scope = SubagentScope::Project;
        project_profile.display_name = "Local Reviewer".into();
        project_profile.file_path = Some(cwd.join(".pi").join("agents").join("reviewer.md"));
        write_profile_file(project_profile.file_path.as_ref().unwrap(), &project_profile).unwrap();
        let profiles = list_profiles(&cwd, &agent_dir, &settings);
        let reviewer: Vec<_> = profiles.iter().filter(|p| p.name == "reviewer").collect();
        assert_eq!(reviewer.len(), 1);
        assert_eq!(reviewer[0].scope, SubagentScope::Project);
        assert_eq!(reviewer[0].display_name, "Local Reviewer");
    }

    #[test]
    fn settings_roundtrip() {
        let base = tmpdir("b3");
        let agent_dir = base.join("agent");
        let settings = SubagentSettings {
            builtin_enabled: false,
            disabled_built_ins: vec!["explore".into()],
            max_concurrent: 17,
        };
        write_settings(&agent_dir, &settings).unwrap();
        let read = read_settings(&agent_dir);
        assert_eq!(read, settings);
        // missing file -> defaults
        let read = read_settings(&base.join("nope"));
        assert_eq!(read, SubagentSettings::default());
    }

    #[test]
    fn frontmatter_split_tolerates_missing_block() {
        let (fm, body) = split_frontmatter("just a prompt, no frontmatter");
        assert!(fm.is_empty());
        assert_eq!(body, "just a prompt, no frontmatter");
    }
}
