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
    write_skill(
        &proj.join(".pi").join("skills"),
        "alpha",
        Some("---\nname: alpha\ndescription: A alpha\n---"),
    );
    write_skill(
        &glob.join("skills"),
        "beta",
        Some("---\nname: beta\ndescription: B beta\ndisable-model-invocation: true\n---"),
    );
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
    let skills = discover_skills(
        &proj,
        &base.join("nope"),
        &base.join("home-agents"),
        &settings,
    );
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
    assert_eq!(
        package_description_in(&base, "npm:pi-demo").as_deref(),
        Some("Demo pkg")
    );
    // scoped 名：node_modules/@scope/pkg
    let scoped = base
        .join("npm")
        .join("node_modules")
        .join("@scope")
        .join("pi-x");
    std::fs::create_dir_all(&scoped).unwrap();
    std::fs::write(scoped.join("package.json"), r#"{"description":"scoped"}"#).unwrap();
    assert_eq!(
        package_description_in(&base, "npm:@scope/pi-x").as_deref(),
        Some("scoped")
    );
    assert_eq!(package_description_in(&base, "npm:pi-missing"), None);
    // git 三种写法归一到 git/<host>/<path>
    let git_dir = base
        .join("git")
        .join("github.com")
        .join("user")
        .join("repo");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("package.json"), r#"{"description":"git pkg"}"#).unwrap();
    for src in [
        "git:https://github.com/user/repo",
        "git:git@github.com:user/repo.git",
    ] {
        assert_eq!(
            package_description_in(&base, src).as_deref(),
            Some("git pkg")
        );
    }
}

#[test]
fn display_source_strips_npm_and_enabled_sources_filter_disabled() {
    assert_eq!(display_source("npm:pi-web-access"), "pi-web-access");
    assert_eq!(
        display_source("git:https://github.com/u/r"),
        "git:https://github.com/u/r"
    );
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
    let p = write_skill(
        &base,
        "gamma",
        Some("---\nname: gamma\ndescription: G\n---"),
    );

    set_disable_invocation(&p, true).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("disable-model-invocation: true"));
    assert!(text.starts_with("---\n"));
    assert!(text.contains("name: gamma"), "existing keys preserved");
    let fm = frontmatter(&text).unwrap();
    assert!(
        fm.iter()
            .any(|(k, v)| k == "disable-model-invocation" && v == "true")
    );

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
    let p3 = write_skill(
        &base,
        "eps",
        Some("---\nname: eps\ndisable-model-invocation: false\n---"),
    );
    set_disable_invocation(&p3, true).unwrap();
    let text = std::fs::read_to_string(&p3).unwrap();
    assert_eq!(text.matches("disable-model-invocation").count(), 1);
    assert!(text.contains("disable-model-invocation: true"));

    // 反复启停不得让 frontmatter 长胖（旧伤：每写一次多一行 `---`，
    // 用户连点几次就把 SKILL.md 搓成十几行三连杠）
    let p4 = write_skill(
        &base,
        "zeta",
        Some("---\nname: zeta\ndescription: Z\n---\n\nbody\n"),
    );
    for disable in [true, false, true, false, true, false] {
        set_disable_invocation(&p4, disable).unwrap();
    }
    let text = std::fs::read_to_string(&p4).unwrap();
    assert_eq!(
        text.matches("\n---").count(),
        1,
        "only the closing fence: {text:?}"
    );
    assert!(!text.contains("disable-model-invocation"));
    assert!(text.contains("name: zeta") && text.contains("description: Z"));
    assert!(text.contains("\n\nbody\n"), "body untouched: {text:?}");
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
    assert_eq!(
        normalize_source("pi install git:https://x"),
        "git:https://x"
    );
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
    let on_disk = SkillEntry {
        path: real,
        ..sk.clone()
    };
    assert!(body_tokens(&on_disk) > 0);
    let missing = SkillEntry {
        path: PathBuf::from("Z:/no/such/SKILL.md"),
        ..sk.clone()
    };
    assert_eq!(body_tokens(&missing), 0);

    // 休眠技能不进提示词 → 0
    let dormant = SkillEntry {
        disable_invocation: true,
        ..sk
    };
    assert_eq!(prompt_tokens(&dormant), 0);
}
