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
    assert_eq!(
        read_enabled_models(&path).unwrap(),
        Some(vec!["openai/*".into()])
    );
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
