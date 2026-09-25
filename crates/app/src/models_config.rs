//! `enabledModels` whitelist editing — Rust port of pi-web
//! `lib/enabled-models.ts` (pure logic) with a pragmatic subset of the SDK
//! pattern matcher (exact refs, `provider`, `provider/*`, `provider/**`,
//! `*`/`**`, `:level` pins; no fuzzy aliases).

/// Thinking-level suffixes a pattern may pin (`formatEntry` parity).
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// One configured pattern and what it resolves to right now.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub pattern: String,
    pub matched: Vec<String>,
    pub pin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EnabledState {
    /// No pattern narrows the list — the selector shows everything.
    pub all_enabled: bool,
    /// Enabled `provider/modelId` refs (restricted to what is available).
    pub enabled: Vec<String>,
    /// ref → thinking level pinned by a `:level` pattern.
    pub pins: Vec<(String, String)>,
    /// Patterns matching no available model (preserved by every edit).
    pub stale: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    /// New pattern list; `None` removes the setting entirely.
    pub patterns: Option<Vec<String>>,
    pub changed: bool,
}

/// Provider id of a `provider/modelId` ref (ids may contain `/`).
pub fn ref_provider(r: &str) -> &str {
    match r.find('/') {
        Some(i) => &r[..i],
        None => r,
    }
}

/// Match one pattern (no pin suffix) against refs, case-insensitively.
/// Returns matched refs in input order.
pub fn match_base(pattern: &str, refs: &[String]) -> Vec<String> {
    let p = pattern.to_lowercase();
    if p == "*" || p == "**" {
        return refs.to_vec();
    }
    let (prov, id_part) = match p.find('/') {
        Some(i) => (&p[..i], Some(&p[i + 1..])),
        None => (p.as_str(), None),
    };
    match id_part {
        // bare provider name scopes the whole provider
        None => refs
            .iter()
            .filter(|r| ref_provider(r).to_lowercase() == prov)
            .cloned()
            .collect(),
        Some("*") => refs
            .iter()
            .filter(|r| {
                let r = r.to_lowercase();
                let (rp, rid) = (ref_provider(&r), &r[ref_provider(&r).len() + 1..]);
                rp == prov && !rid.contains('/')
            })
            .cloned()
            .collect(),
        Some("**") => refs
            .iter()
            .filter(|r| ref_provider(r).to_lowercase() == prov)
            .cloned()
            .collect(),
        Some(id) => {
            let want = format!("{prov}/{id}");
            refs.iter()
                .filter(|r| r.to_lowercase() == want)
                .cloned()
                .collect()
        }
    }
}

/// Resolve one stored pattern (may carry a `:level` suffix).
pub fn resolve_pattern(pattern: &str, refs: &[String]) -> Entry {
    let (base, pin) = split_pin(pattern);
    Entry {
        pattern: pattern.to_string(),
        matched: match_base(&base, refs),
        pin,
    }
}

fn split_pin(pattern: &str) -> (String, Option<String>) {
    if let Some(colon) = pattern.rfind(':') {
        let suffix = &pattern[colon + 1..];
        if LEVELS.contains(&suffix) && colon > 0 {
            return (pattern[..colon].to_string(), Some(suffix.to_string()));
        }
    }
    (pattern.to_string(), None)
}

fn resolve_all(patterns: &[String], refs: &[String]) -> Vec<Entry> {
    patterns.iter().map(|p| resolve_pattern(p, refs)).collect()
}

/// For each provider, the glob matching exactly its available models
/// (verified: `provider/*` misses ids containing `/`, so use `**` then).
pub fn provider_globs(refs: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut providers: Vec<&str> = Vec::new();
    for r in refs {
        let p = ref_provider(r);
        if !providers.contains(&p) {
            providers.push(p);
        }
    }
    for p in providers {
        let ids: Vec<&str> = refs
            .iter()
            .filter(|r| ref_provider(r) == p)
            .map(|r| &r[p.len() + 1..])
            .collect();
        if ids.is_empty() {
            continue;
        }
        let glob = if ids.iter().all(|id| !id.contains('/')) {
            format!("{p}/*")
        } else {
            format!("{p}/**")
        };
        // verify: matches exactly this provider's models
        if match_base(&glob, refs) == refs.iter().filter(|r| ref_provider(r) == p).cloned().collect::<Vec<_>>() {
            out.push((p.to_string(), glob));
        }
    }
    out
}

fn enabled_refs(entries: &[Entry]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut refs = Vec::new();
    for e in entries {
        for r in &e.matched {
            if seen.insert(r.clone()) {
                refs.push(r.clone());
            }
        }
    }
    refs
}

/// "Everything enabled" made explicit so a single toggle can edit it
/// (verified provider glob when possible, else one entry per model).
fn materialize(entries: Vec<Entry>, refs: &[String], globs: &[(String, String)]) -> Vec<Entry> {
    if !enabled_refs(&entries).is_empty() {
        return entries;
    }
    let mut out = entries;
    let mut providers: Vec<&str> = Vec::new();
    for r in refs {
        let p = ref_provider(r);
        if !providers.contains(&p) {
            providers.push(p);
        }
    }
    for p in providers {
        let prov_refs: Vec<String> = refs
            .iter()
            .filter(|r| ref_provider(r) == p)
            .cloned()
            .collect();
        match globs.iter().find(|(gp, _)| gp == p) {
            Some((_, glob)) => out.push(Entry { pattern: glob.clone(), matched: prov_refs, pin: None }),
            None => {
                for r in prov_refs {
                    out.push(Entry { pattern: r.clone(), matched: vec![r], pin: None });
                }
            }
        }
    }
    out
}

/// Replace a fully enabled provider's entries with its verified glob
/// (self-healing against catalog renames; skipped with pins or partial sets).
fn collapse_provider(mut entries: Vec<Entry>, refs: &[String], globs: &[(String, String)]) -> Vec<Entry> {
    for (provider, glob) in globs {
        let prov_refs: Vec<String> = refs
            .iter()
            .filter(|r| ref_provider(r) == provider.as_str())
            .cloned()
            .collect();
        if prov_refs.is_empty() {
            continue;
        }
        let prov_set: std::collections::HashSet<String> = prov_refs.iter().cloned().collect();
        let involved_ix: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.matched.iter().any(|r| prov_set.contains(r)))
            .map(|(i, _)| i)
            .collect();
        if involved_ix.iter().any(|&i| entries[i].pin.is_some()) {
            continue;
        }
        let covered: std::collections::HashSet<String> = involved_ix
            .iter()
            .flat_map(|&i| entries[i].matched.iter().cloned())
            .collect();
        if prov_refs.iter().any(|r| !covered.contains(r)) {
            continue;
        }
        let is_subset = |e: &Entry| !e.matched.is_empty() && e.matched.iter().all(|r| prov_set.contains(r));
        let subsets: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| is_subset(e))
            .map(|(i, _)| i)
            .collect();
        if subsets.len() < 2 {
            continue;
        }
        let first = subsets[0];
        let glob_entry = Entry { pattern: glob.clone(), matched: prov_refs, pin: None };
        entries = entries
            .iter()
            .enumerate()
            .filter(|(i, e)| *i == first || !is_subset(e))
            .map(|(i, e)| if i == first { glob_entry.clone() } else { e.clone() })
            .collect();
    }
    entries
}

/// Drop the setting entirely once it stops narrowing anything (never when it
/// would discard stale entries or a thinking pin).
fn serialize(entries: &[Entry], refs: &[String]) -> Option<Vec<String>> {
    let has_stale = entries.iter().any(|e| e.matched.is_empty());
    let has_pins = entries.iter().any(|e| e.pin.is_some());
    let enabled: std::collections::HashSet<String> = enabled_refs(entries).into_iter().collect();
    let covers_all = refs.iter().all(|r| enabled.contains(r));
    if covers_all && !has_stale && !has_pins {
        return None;
    }
    Some(entries.iter().map(|e| e.pattern.clone()).collect())
}

/// Current selection derived from the configured patterns.
pub fn compute_state(patterns: Option<&Vec<String>>, refs: &[String]) -> EnabledState {
    let Some(patterns) = patterns else {
        return EnabledState {
            all_enabled: true,
            enabled: refs.to_vec(),
            ..Default::default()
        };
    };
    let entries = resolve_all(patterns, refs);
    let enabled = enabled_refs(&entries);
    let mut pins = Vec::new();
    for e in &entries {
        if let Some(pin) = &e.pin {
            for r in &e.matched {
                if !pins.iter().any(|(p, _)| p == r) {
                    pins.push((r.clone(), pin.clone()));
                }
            }
        }
    }
    // pi falls back to every available model when patterns resolve to nothing
    let all_enabled = enabled.is_empty();
    EnabledState {
        all_enabled,
        enabled: if all_enabled { refs.to_vec() } else { enabled },
        pins,
        stale: entries.iter().filter(|e| e.matched.is_empty()).map(|e| e.pattern.clone()).collect(),
    }
}

/// Toggle refs on/off with the smallest possible pattern edit. Fails when
/// disabling would leave no enabled model (pi reads an empty scope as "none").
pub fn set_models_enabled(
    patterns: Option<&Vec<String>>,
    refs: &[String],
    targets: &[String],
    enable: bool,
) -> Result<Edit, &'static str> {
    let available: std::collections::HashSet<&String> = refs.iter().collect();
    let targets: Vec<&String> = targets
        .iter()
        .filter(|t| available.contains(t))
        .collect();
    if targets.is_empty() {
        return Ok(Edit { patterns: patterns.cloned(), changed: false });
    }
    let globs = provider_globs(refs);
    let input_patterns = patterns.cloned();
    let mut entries = materialize(resolve_all(patterns.unwrap_or(&Vec::new()), refs), refs, &globs);

    if enable {
        let current: std::collections::HashSet<String> = enabled_refs(&entries).into_iter().collect();
        for t in &targets {
            if current.contains(*t) {
                continue;
            }
            entries.push(Entry {
                pattern: (*t).clone(),
                matched: vec![(*t).clone()],
                pin: None,
            });
        }
    } else {
        let removed: std::collections::HashSet<&&String> = targets.iter().collect();
        let mut next: Vec<Entry> = Vec::new();
        for e in entries {
            if !e.matched.iter().any(|m| removed.contains(&m)) {
                next.push(e);
                continue;
            }
            // expand only this pattern, keeping its pin on survivors
            for r in e.matched.iter().filter(|m| !removed.contains(&m)) {
                next.push(Entry {
                    pattern: match &e.pin {
                        Some(pin) => format!("{r}:{pin}"),
                        None => r.clone(),
                    },
                    matched: vec![r.clone()],
                    pin: e.pin.clone(),
                });
            }
        }
        if enabled_refs(&next).is_empty() {
            return Err("last-model");
        }
        entries = next;
    }

    let entries = collapse_provider(entries, refs, &globs);
    let patterns = serialize(&entries, refs);
    Ok(Edit {
        changed: patterns != input_patterns,
        patterns,
    })
}

/// Enable/disable every available model of one provider (op:"provider").
pub fn set_provider_enabled(
    patterns: Option<&Vec<String>>,
    refs: &[String],
    provider: &str,
    enable: bool,
) -> Result<Edit, &'static str> {
    let targets: Vec<String> = refs
        .iter()
        .filter(|r| ref_provider(r) == provider)
        .cloned()
        .collect();
    set_models_enabled(patterns, refs, &targets, enable)
}

/// Drop the scope so every model is enabled again (the one op that also
/// discards stale patterns).
pub fn clear_scope() -> Edit {
    Edit { patterns: None, changed: true }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(patterns, vec!["deepseek/*".to_string(), "openrouter/**".to_string()]);
        let state = compute_state(Some(&patterns), &refs);
        assert!(!state.all_enabled);
        assert_eq!(state.enabled.len(), 4);
        assert!(!state.enabled.contains(&"openai/gpt-5".to_string()));
        // re-enable: everything back -> setting dropped entirely
        let edit = set_models_enabled(Some(&patterns), &refs, &["openai/gpt-5".to_string()], true).unwrap();
        assert_eq!(edit.patterns, None);
        assert!(compute_state(edit.patterns.as_ref(), &refs).all_enabled);
    }

    #[test]
    fn last_model_guard() {
        let refs = vec!["a/one".to_string(), "b/two".to_string()];
        let edit = set_models_enabled(None, &refs, &["a/one".to_string()], false).unwrap();
        let patterns = edit.patterns.unwrap();
        // disabling the only remaining enabled model fails
        let err = set_models_enabled(Some(&patterns), &refs, &["b/two".to_string()], false);
        assert_eq!(err, Err("last-model"));
    }

    #[test]
    fn provider_toggle_and_pins_survive_expansion() {
        let refs = refs();
        // disable the whole deepseek provider from "all enabled"
        let edit = set_provider_enabled(None, &refs, "deepseek", false).unwrap();
        let patterns = edit.patterns.unwrap();
        let state = compute_state(Some(&patterns), &refs);
        assert!(!state.enabled.iter().any(|r| r.starts_with("deepseek/")));
        assert!(!patterns.iter().any(|p| p.contains("deepseek")), "provider fully off -> no deepseek entries");
        assert!(patterns.contains(&"openai/*".to_string()));

        // a pinned model keeps `:level` on surviving refs when its pattern is
        // expanded; removing the pinned model drops its entry entirely
        let patterns = vec![
            "deepseek/deepseek-chat:medium".to_string(),
            "deepseek/deepseek-reasoner".to_string(),
        ];
        let edit = set_models_enabled(Some(&patterns), &refs, &["deepseek/deepseek-chat".to_string()], false).unwrap();
        assert_eq!(edit.patterns.unwrap(), vec!["deepseek/deepseek-reasoner".to_string()]);

        // whitelist semantics: disabling the only pattern-covered model is
        // rejected by the last-model guard (pi-web parity)
        let patterns = vec!["deepseek/deepseek-chat:medium".to_string()];
        let err = set_provider_enabled(Some(&patterns), &refs, "deepseek", false);
        assert_eq!(err, Err("last-model"));
    }

    #[test]
    fn stale_patterns_never_touched_by_edits() {
        let refs = refs();
        let patterns = vec![
            "ghost/missing".to_string(),
            "deepseek/deepseek-chat".to_string(),
        ];
        let edit = set_models_enabled(Some(&patterns), &refs, &["openai/gpt-5".to_string()], false).unwrap();
        let out = edit.patterns.unwrap();
        assert!(out.contains(&"ghost/missing".to_string()));
    }
}
