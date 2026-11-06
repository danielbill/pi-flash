//! models_config 测试（itlq.9 自 models_config.rs 外移，纯结构搬移）。

use super::*;

fn refs() -> Vec<String> {
    [
        "deepseek/deepseek-chat",
        "deepseek/deepseek-reasoner",
        "openai/gpt-5",
        "openrouter/commandcode/sakana/fugu-ultra",
        "openrouter/z-ai/glm-4.6",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn pattern_matching() {
    let refs = refs();
    assert_eq!(match_base("deepseek", &refs).len(), 2);
    assert_eq!(match_base("deepseek/*", &refs).len(), 2);
    assert_eq!(
        match_base("openrouter/*", &refs),
        Vec::<String>::new() // ids contain '/', `*` stops at the slash
    );
    assert_eq!(match_base("openrouter/**", &refs).len(), 2);
    assert_eq!(match_base("openai/gpt-5", &refs), vec!["openai/gpt-5"]);
    assert_eq!(match_base("OPENAI/GPT-5", &refs), vec!["openai/gpt-5"]);
    assert_eq!(match_base("*", &refs).len(), refs.len());
}

#[test]
fn pin_suffix_parsed_and_preserved() {
    let refs = refs();
    let e = resolve_pattern("deepseek/deepseek-chat:high", &refs);
    assert_eq!(e.matched, vec!["deepseek/deepseek-chat".to_string()]);
    assert_eq!(e.pin.as_deref(), Some("high"));
    // a model id that merely contains a level word is not a pin
    let e = resolve_pattern("openai/gpt-5:highx", &refs);
    assert_eq!(e.pin, None);
}

#[test]
fn provider_globs_verified() {
    let globs = provider_globs(&refs());
    let deepseek = globs.iter().find(|(p, _)| p == "deepseek").unwrap();
    assert_eq!(deepseek.1, "deepseek/*");
    let openrouter = globs.iter().find(|(p, _)| p == "openrouter").unwrap();
    assert_eq!(openrouter.1, "openrouter/**");
}

#[test]
fn all_enabled_state_and_clear() {
    let state = compute_state(None, &refs());
    assert!(state.all_enabled);
    assert_eq!(state.enabled.len(), 5);
    // empty patterns file = all enabled too
    let state = compute_state(Some(&Vec::new()), &refs());
    assert!(state.all_enabled);
    // stale-only patterns also read as all enabled
    let state = compute_state(Some(&vec!["ghost/missing".to_string()]), &refs());
    assert!(state.all_enabled);
    assert_eq!(state.stale, vec!["ghost/missing".to_string()]);
}

#[test]
fn disable_writes_explicit_list_then_serialize_drops_when_reenabled() {
    let refs = refs();
    // disable one model from "all enabled"
    let edit = set_models_enabled(None, &refs, &["openai/gpt-5".to_string()], false).unwrap();
    let patterns = edit.patterns.expect("list kept");
    // deepseek keeps its verified glob; the only openai model is now off
    // so openai has no entries at all; openrouter's two explicit entries
    // are normalized back into its verified `**` glob
    assert_eq!(
        patterns,
        vec!["deepseek/*".to_string(), "openrouter/**".to_string()]
    );
    let state = compute_state(Some(&patterns), &refs);
    assert!(!state.all_enabled);
    assert_eq!(state.enabled.len(), 4);
    assert!(!state.enabled.contains(&"openai/gpt-5".to_string()));
    // re-enable: everything back -> setting dropped entirely
    let edit =
        set_models_enabled(Some(&patterns), &refs, &["openai/gpt-5".to_string()], true).unwrap();
    assert_eq!(edit.patterns, None);
    assert!(compute_state(edit.patterns.as_ref(), &refs).all_enabled);
}

#[test]
fn last_model_guard() {
    let refs = vec!["a/one".to_string(), "b/two".to_string()];
    let edit = set_models_enabled(None, &refs, &["a/one".to_string()], false).unwrap();
    let patterns = edit.patterns.unwrap();
    // 守卫已按用户裁决移除（d3a5c543）：再关一个也允许
    let edit = set_models_enabled(Some(&patterns), &refs, &["b/two".to_string()], false)
        .expect("last-model guard removed");
    assert!(edit.changed);
}

#[test]
fn provider_toggle_and_pins_survive_expansion() {
    let refs = refs();
    // disable the whole deepseek provider from "all enabled"
    let edit = set_provider_enabled(None, &refs, "deepseek", false).unwrap();
    let patterns = edit.patterns.unwrap();
    let state = compute_state(Some(&patterns), &refs);
    assert!(!state.enabled.iter().any(|r| r.starts_with("deepseek/")));
    assert!(
        !patterns.iter().any(|p| p.contains("deepseek")),
        "provider fully off -> no deepseek entries"
    );
    assert!(patterns.contains(&"openai/*".to_string()));

    // a pinned model keeps `:level` on surviving refs when its pattern is
    // expanded; removing the pinned model drops its entry entirely
    let patterns = vec![
        "deepseek/deepseek-chat:medium".to_string(),
        "deepseek/deepseek-reasoner".to_string(),
    ];
    let edit = set_models_enabled(
        Some(&patterns),
        &refs,
        &["deepseek/deepseek-chat".to_string()],
        false,
    )
    .unwrap();
    assert_eq!(
        edit.patterns.unwrap(),
        vec!["deepseek/deepseek-reasoner".to_string()]
    );

    // 守卫已移除：整商关掉最后一个启用的模型也允许（空名单交 pi 解释）
    let patterns = vec!["deepseek/deepseek-chat:medium".to_string()];
    let edit = set_provider_enabled(Some(&patterns), &refs, "deepseek", false)
        .expect("last-model guard removed");
    assert!(edit.changed);
}

#[test]
fn stale_patterns_never_touched_by_edits() {
    let refs = refs();
    let patterns = vec![
        "ghost/missing".to_string(),
        "deepseek/deepseek-chat".to_string(),
    ];
    let edit =
        set_models_enabled(Some(&patterns), &refs, &["openai/gpt-5".to_string()], false).unwrap();
    let out = edit.patterns.unwrap();
    assert!(out.contains(&"ghost/missing".to_string()));
}

/// 删商/删模型后 patterns 清理（2026-11-02 用户实测：删掉的 ds/my-ds 在
/// settings.json 里留了一堆死条目）
#[test]
fn prune_patterns_by_provider_and_by_model() {
    let pats: Vec<String> = [
        "freeflow/glm-5.3-flash-free",
        "my-ds/*",
        "ds/deepseek-flash",
        "ds/glm-5.3-flash:high",
        "dds/glm-5.3-flash",
        "myds/deepseek-flash",
        "myds/deepseek-v4-pro:low",
        "myds/*",
    ]
    .map(String::from)
    .to_vec();
    // 删商：名下全清（glob / 精确 / 带 pin），别家不动
    let out = prune_patterns(&pats, "ds", None).unwrap();
    assert!(!out.iter().any(|p| p.starts_with("ds/")));
    assert!(
        out.contains(&"dds/glm-5.3-flash".to_string()),
        "dds 不是 ds"
    );
    assert_eq!(out.len(), pats.len() - 2);
    let out = prune_patterns(&pats, "MY-DS", None).unwrap();
    assert!(!out.contains(&"my-ds/*".to_string()), "商名不分大小写");
    // 删模型：只去精确 ref（含 pin），glob 留着
    let out = prune_patterns(&pats, "myds", Some("deepseek-v4-pro")).unwrap();
    assert!(!out.iter().any(|p| p.starts_with("myds/deepseek-v4-pro")));
    assert!(out.contains(&"myds/*".to_string()));
    assert!(out.contains(&"myds/deepseek-flash".to_string()));
    // 没命中 = None（不写盘）
    assert!(prune_patterns(&pats, "nope", None).is_none());
    assert!(prune_patterns(&pats, "myds", Some("ghost")).is_none());
}
