//! Skill discovery + SKILL.md frontmatter editing (pi-web SkillsConfig
//! parity). Discovery mirrors `DefaultResourceLoader`'s skill directories
//! subset: project `.pi/skills` / `.agents/skills`, global agent-dir skills,
//! `~/.agents/skills`, plus explicit `skills` paths from settings.json.
//!
//! The visible/hidden toggle persists in the SKILL.md frontmatter
//! (`disable-model-invocation`), exactly like pi-web's PATCH /api/skills.

use std::path::{Path, PathBuf};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    Project,
    Global,
}

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    /// SKILL.md path (frontmatter edit target)
    pub path: PathBuf,
    pub scope: SkillScope,
    /// `disable-model-invocation: true` — hidden from the model prompt
    pub disable_invocation: bool,
}

/// Discover skills for a workspace. Order: project dirs, global dirs, then
/// settings-declared paths; each dir contributes its immediate subdirs that
/// contain a SKILL.md.
pub fn discover_skills(
    cwd: &Path,
    agent_dir: &Path,
    home_agents_dir: &Path,
    settings: &serde_json::Value,
) -> Vec<SkillEntry> {
    let mut dirs: Vec<(PathBuf, SkillScope)> = vec![
        (cwd.join(".pi").join("skills"), SkillScope::Project),
        (cwd.join(".agents").join("skills"), SkillScope::Project),
        (agent_dir.join("skills"), SkillScope::Global),
        (home_agents_dir.to_path_buf(), SkillScope::Global),
    ];
    if let Some(extra) = settings.get("skills").and_then(|v| v.as_array()) {
        for p in extra.iter().filter_map(|v| v.as_str()) {
            dirs.push((resolve_setting_path(cwd, agent_dir, p), SkillScope::Project));
        }
    }
    let mut out: Vec<SkillEntry> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for (dir, scope) in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            let skill_md = sub.join("SKILL.md");
            if !seen.insert(skill_md.clone()) || !skill_md.is_file() {
                continue;
            }
            if let Some(entry) = parse_skill(&skill_md, scope) {
                out.push(entry);
            }
        }
    }
    out
}

fn resolve_setting_path(cwd: &Path, agent_dir: &Path, p: &str) -> PathBuf {
    if p.starts_with('~') {
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
        return Path::new(&home).join(p.trim_start_matches("~/").trim_start_matches("~\\"));
    }
    let path = Path::new(p);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if p.starts_with("./") || p.starts_with(".\\") {
        return cwd.join(p.trim_start_matches("./").trim_start_matches(".\\"));
    }
    // bare relative path: pi resolves settings skill paths against the agent
    // dir when the file lives there, else the cwd
    let via_agent = agent_dir.join(p);
    if via_agent.exists() {
        via_agent
    } else {
        cwd.join(p)
    }
}

/// Parse one SKILL.md: frontmatter name/description/disable-model-invocation.
fn parse_skill(skill_md: &Path, scope: SkillScope) -> Option<SkillEntry> {
    let text = std::fs::read_to_string(skill_md).ok()?;
    let fm = frontmatter(&text).unwrap_or_default();
    let name = fm
        .iter()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| {
            skill_md
                .parent()
                .and_then(|d| d.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });
    let description = fm
        .iter()
        .find(|(k, _)| k == "description")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let disable_invocation = fm
        .iter()
        .find(|(k, _)| k == "disable-model-invocation")
        .map(|(_, v)| v == "true")
        .unwrap_or(false);
    Some(SkillEntry { name, description, path: skill_md.to_path_buf(), scope, disable_invocation })
}

/// Key/value pairs of the leading `---` frontmatter block (flat YAML subset).
pub fn frontmatter(text: &str) -> Option<Vec<(String, String)>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut out = Vec::new();
    for line in lines {
        let line = line.trim_end();
        if line.trim() == "---" {
            return Some(out);
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_string();
            let value = line[colon + 1..].trim().trim_matches('"').to_string();
            if !key.is_empty() {
                out.push((key, value));
            }
        }
    }
    None
}

/// PATCH /api/skills parity: set `disable-model-invocation` in the SKILL.md
/// frontmatter (insert into an existing block, create the block when missing,
/// drop the key when enabling). Returns the new file content length on success.
pub fn set_disable_invocation(path: &Path, disable: bool) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let updated = edit_frontmatter(&text, disable);
    std::fs::write(path, updated).map_err(|e| e.to_string())
}

/// pi 系统提示词里单个可见技能的注入块（`formatSkillsForPrompt` 的逐条
/// 格式，vendor pi core/skills.js）——「说明大小」的计算基准：技能只有
/// name/description/location 三行进上下文，正文要在调用时才由 read 装载。
pub fn prompt_block(skill: &SkillEntry) -> String {
    // 与 pi 的 escapeXml 同款（& < > " ' 五实体）
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    format!(
        "  <skill>\n    <name>{}</name>\n    <description>{}</description>\n    <location>{}</location>\n  </skill>",
        esc(&skill.name),
        esc(&skill.description),
        esc(&skill.path.to_string_lossy())
    )
}

/// 该技能对模型上下文的说明开销（pi 的 `<available_skills>` 逐条块；
/// 休眠技能不进提示词，计 0）。共享段头（引导语 + 标签壳）不摊入。
pub fn prompt_tokens(skill: &SkillEntry) -> u64 {
    if skill.disable_invocation {
        return 0;
    }
    crate::estimate::estimate_tokens(&prompt_block(skill))
}

/// SKILL.md 全文估算 —— 技能被调用时由 read 装载的一次性开销（进对话
/// 记录、随压缩回收，非系统提示词常驻）。读不到（文件被删等）按 0。
pub fn body_tokens(skill: &SkillEntry) -> u64 {
    match std::fs::read_to_string(&skill.path) {
        Ok(text) => crate::estimate::estimate_tokens(&text),
        Err(_) => 0,
    }
}

fn edit_frontmatter(text: &str, disable: bool) -> String {
    let ends_fm = text.starts_with("---") && text[3..].contains("\n---");
    if !ends_fm {
        if !disable {
            return text.to_string();
        }
        let mut out = String::from("---\ndisable-model-invocation: true\n---\n\n");
        out.push_str(text);
        return out;
    }
    let close = text[3..].find("\n---").unwrap() + 3;
    let (block, rest) = text.split_at(close);
    let block = &block[3..]; // strip leading ---
    let kept: Vec<&str> = block
        .lines()
        .skip(1) // leading --- line
        .filter(|l| !l.trim_start().starts_with("disable-model-invocation:"))
        .collect();
    let mut out = String::from("---\n");
    for line in &kept {
        out.push_str(line);
        out.push('\n');
    }
    if disable {
        out.push_str("disable-model-invocation: true\n");
    }
    out.push_str("---");
    out.push_str(rest);
    out
}

/// Packages (`packages` settings key) helpers for the plugins panel.
pub fn entry_source(entry: &serde_json::Value) -> String {
    match entry {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => o
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

/// pi-web disables a package by zeroing its resource arrays; an object entry
/// with all four arrays present and empty is the disabled marker.
pub fn entry_disabled(entry: &serde_json::Value) -> bool {
    match entry {
        serde_json::Value::Object(o) => {
            ["extensions", "skills", "prompts", "themes"].iter().all(|k| {
                o.get(*k).and_then(|v| v.as_array()).map(|a| a.is_empty()).unwrap_or(false)
            })
        }
        _ => false,
    }
}

/// Resource counts for the detail footer (`ext · skills · prompts · themes`).
pub fn entry_resource_counts(entry: &serde_json::Value) -> (usize, usize, usize, usize) {
    match entry {
        serde_json::Value::Object(o) => (
            o.get("extensions").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
            o.get("skills").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
            o.get("prompts").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
            o.get("themes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
        ),
        _ => (0, 0, 0, 0),
    }
}

/// Normalize a pasted source the way pi-web AddPluginPanel does.
pub fn normalize_source(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for prefix in ["$ pi install ", "pi install "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
        }
    }
    s
}

/// Installed package dir, best-effort（pi CLI 约定，040）：全局
/// `~/.pi/agent/npm/node_modules/<pkg>`（scoped 名含 `/` 直接 join）、
/// `~/.pi/agent/git/<host>/<path>`（https / ssh / git@ 三种写法归一）、
/// 本地包原地。
pub fn package_install_dir_in(agent_dir: &Path, source: &str) -> PathBuf {
    if let Some(pkg) = source.strip_prefix("npm:") {
        return agent_dir.join("npm").join("node_modules").join(pkg);
    }
    if let Some(url) = source.strip_prefix("git:") {
        let rest = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("ssh://")
            .trim_start_matches("git@")
            .trim_end_matches(".git");
        return agent_dir.join("git").join(rest.replace(':', "/"));
    }
    PathBuf::from(source)
}

/// [`package_install_dir_in`] at the real agent dir.
pub fn package_install_dir(source: &str) -> PathBuf {
    package_install_dir_in(&crate::config::agent_dir(), source)
}

/// 包内 package.json 的 `description`（040 详情「说明」行）。best-effort：
/// 未安装 / 无字段 / 空串返回 None。
pub fn package_description_in(agent_dir: &Path, source: &str) -> Option<String> {
    let text = std::fs::read_to_string(package_install_dir_in(agent_dir, source).join("package.json"))
        .ok()?;
    let v = crate::config::parse_lenient(&text).ok()?;
    v.get("description")
        .and_then(|d| d.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// [`package_description_in`] at the real agent dir.
pub fn package_description(source: &str) -> Option<String> {
    package_description_in(&crate::config::agent_dir(), source)
}

/// 列表展示名：去掉 `npm:` 前缀（040），其余来源原样。
pub fn display_source(source: &str) -> &str {
    source.strip_prefix("npm:").unwrap_or(source)
}

/// settings.json `packages` 里「启用中」的包来源（040：基础扩展集——
/// custom 档挂载与勾选面板的持续生效底座；禁用 = 对象条目四数组置空）。
pub fn enabled_package_sources_in(settings: &Path) -> Vec<String> {
    crate::config::read_packages(settings)
        .unwrap_or_default()
        .iter()
        .filter(|e| !entry_disabled(e))
        .map(entry_source)
        .filter(|s| !s.is_empty())
        .collect()
}

/// [`enabled_package_sources_in`] at the real global settings.json.
pub fn enabled_package_sources() -> Vec<String> {
    enabled_package_sources_in(&crate::config::settings_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_lenient;

    fn write_skill(dir: &Path, name: &str, fm: Option<&str>) -> PathBuf {
        let sub = dir.join(name);
        std::fs::create_dir_all(&sub).unwrap();
        let body = match fm {
            Some(f) => format!("{f}\n\n# {name}\n"),
            None => format!("# {name}\nbody without frontmatter\n"),
        };
        let p = sub.join("SKILL.md");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn discovers_project_and_global_skills() {
        let base = std::env::temp_dir().join(format!("skills-{}", std::process::id()));
        let proj = base.join("proj");
        let glob = base.join("glob");
        write_skill(&proj.join(".pi").join("skills"), "alpha", Some("---\nname: alpha\ndescription: A alpha\n---"));
        write_skill(&glob.join("skills"), "beta", Some("---\nname: beta\ndescription: B beta\ndisable-model-invocation: true\n---"));
        let settings = parse_lenient("{}").unwrap();
        let skills = discover_skills(&proj, &glob, &base.join("home-agents"), &settings);
        assert_eq!(skills.len(), 2);
        let alpha = skills.iter().find(|s| s.name == "alpha").unwrap();
        assert_eq!(alpha.scope, SkillScope::Project);
        assert!(!alpha.disable_invocation);
        let beta = skills.iter().find(|s| s.name == "beta").unwrap();
        assert_eq!(beta.scope, SkillScope::Global);
        assert!(beta.disable_invocation);
    }

    #[test]
    fn skill_without_frontmatter_falls_back_to_dir_name() {
        let base = std::env::temp_dir().join(format!("skills-nofm-{}", std::process::id()));
        let proj = base.join("proj");
        write_skill(&proj.join(".agents").join("skills"), "plain-skill", None);
        let settings = parse_lenient("{}").unwrap();
        let skills = discover_skills(&proj, &base.join("nope"), &base.join("home-agents"), &settings);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "plain-skill");
        assert_eq!(skills[0].description, "");
    }

    #[test]
    fn package_description_reads_installed_package_json() {
        let base = std::env::temp_dir().join(format!("pkgdesc-{}", std::process::id()));
        let pkg_dir = base.join("npm").join("node_modules").join("pi-demo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name":"pi-demo","description":"  Demo pkg  "}"#,
        )
        .unwrap();
        assert_eq!(package_description_in(&base, "npm:pi-demo").as_deref(), Some("Demo pkg"));
        // scoped 名：node_modules/@scope/pkg
        let scoped = base.join("npm").join("node_modules").join("@scope").join("pi-x");
        std::fs::create_dir_all(&scoped).unwrap();
        std::fs::write(scoped.join("package.json"), r#"{"description":"scoped"}"#).unwrap();
        assert_eq!(package_description_in(&base, "npm:@scope/pi-x").as_deref(), Some("scoped"));
        assert_eq!(package_description_in(&base, "npm:pi-missing"), None);
        // git 三种写法归一到 git/<host>/<path>
        let git_dir = base.join("git").join("github.com").join("user").join("repo");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(git_dir.join("package.json"), r#"{"description":"git pkg"}"#).unwrap();
        for src in [
            "git:https://github.com/user/repo",
            "git:git@github.com:user/repo.git",
        ] {
            assert_eq!(package_description_in(&base, src).as_deref(), Some("git pkg"));
        }
    }

    #[test]
    fn display_source_strips_npm_and_enabled_sources_filter_disabled() {
        assert_eq!(display_source("npm:pi-web-access"), "pi-web-access");
        assert_eq!(display_source("git:https://github.com/u/r"), "git:https://github.com/u/r");
        let base = std::env::temp_dir().join(format!("pkgsrc-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("settings.json"),
            r#"{"packages": [
            "npm:pi-on",
            {"source": "npm:pi-off", "extensions": [], "skills": [], "prompts": [], "themes": []},
            {"source": "npm:pi-filtered", "extensions": ["a.ts"]}
        ]}"#,
        )
        .unwrap();
        assert_eq!(
            enabled_package_sources_in(&base.join("settings.json")),
            vec!["npm:pi-on".to_string(), "npm:pi-filtered".to_string()]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn frontmatter_toggle_roundtrip() {
        let base = std::env::temp_dir().join(format!("skills-fm-{}", std::process::id()));
        let p = write_skill(&base, "gamma", Some("---\nname: gamma\ndescription: G\n---"));

        set_disable_invocation(&p, true).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("disable-model-invocation: true"));
        assert!(text.starts_with("---\n"));
        assert!(text.contains("name: gamma"), "existing keys preserved");
        let fm = frontmatter(&text).unwrap();
        assert!(fm.iter().any(|(k, v)| k == "disable-model-invocation" && v == "true"));

        // enabling removes the key
        set_disable_invocation(&p, false).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("disable-model-invocation"));
        assert!(text.contains("name: gamma"));

        // file without frontmatter gets a block created on disable
        let p2 = write_skill(&base, "delta", None);
        set_disable_invocation(&p2, true).unwrap();
        let text = std::fs::read_to_string(&p2).unwrap();
        assert!(text.starts_with("---\ndisable-model-invocation: true\n---\n"));
        assert!(text.contains("# delta"));

        // replace an existing false value
        let p3 = write_skill(&base, "eps", Some("---\nname: eps\ndisable-model-invocation: false\n---"));
        set_disable_invocation(&p3, true).unwrap();
        let text = std::fs::read_to_string(&p3).unwrap();
        assert_eq!(text.matches("disable-model-invocation").count(), 1);
        assert!(text.contains("disable-model-invocation: true"));
    }

    #[test]
    fn package_entry_helpers() {
        let plain = serde_json::json!("npm:pi-web-access");
        assert_eq!(entry_source(&plain), "npm:pi-web-access");
        assert!(!entry_disabled(&plain));
        let disabled = serde_json::json!({"source": "npm:x", "extensions": [], "skills": [], "prompts": [], "themes": []});
        assert!(entry_disabled(&disabled));
        assert_eq!(entry_resource_counts(&disabled), (0, 0, 0, 0));
        let filter = serde_json::json!({"source": "npm:y", "skills": ["a"]});
        assert!(!entry_disabled(&filter));
        assert_eq!(normalize_source("$ pi install npm:foo"), "npm:foo");
        assert_eq!(normalize_source("pi install git:https://x"), "git:https://x");
        assert_eq!(normalize_source(" npm:bar "), "npm:bar");
    }

    #[test]
    fn prompt_block_matches_pi_format() {
        let sk = SkillEntry {
            name: "demo".into(),
            description: "Does <things> & more".into(),
            path: PathBuf::from("C:/x/.pi/skills/demo/SKILL.md"),
            scope: SkillScope::Project,
            disable_invocation: false,
        };
        assert_eq!(
            prompt_block(&sk),
            "  <skill>\n    <name>demo</name>\n    <description>Does &lt;things&gt; &amp; more</description>\n    <location>C:/x/.pi/skills/demo/SKILL.md</location>\n  </skill>"
        );
        assert!(prompt_tokens(&sk) > 0);

        // 正文 = 全文估算：真实文件非零；文件不存在按 0
        let real = write_skill(&std::env::temp_dir(), "tok-body", None);
        let on_disk = SkillEntry { path: real, ..sk.clone() };
        assert!(body_tokens(&on_disk) > 0);
        let missing = SkillEntry { path: PathBuf::from("Z:/no/such/SKILL.md"), ..sk.clone() };
        assert_eq!(body_tokens(&missing), 0);

        // 休眠技能不进提示词 → 0
        let dormant = SkillEntry { disable_invocation: true, ..sk };
        assert_eq!(prompt_tokens(&dormant), 0);
    }
}
