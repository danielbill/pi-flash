use super::*;

const DEEPSEEK_FLASH_TOML: &str = include_str!("../fixtures/deepseek-flash.toml");

#[test]
fn parse_schema2_offpeak_base() {
    let e = parse_toml(DEEPSEEK_FLASH_TOML).expect("schema 2 fixture parses");
    assert_eq!(e.cost.input, Some(0.15));
    assert_eq!(e.cost.output, Some(0.6));
    assert_eq!(e.cost.cache_read, Some(0.003));
    assert_eq!(e.cost.cache_write, None);
    assert_eq!(e.context_window, Some(1_048_576));
    assert_eq!(e.max_tokens, Some(393_216));
    assert!(e.reasoning);
    assert_eq!(e.source, SOURCE_MODELS_DEV);
}

#[test]
fn parse_bails_on_unknown_schema_or_garbage() {
    assert!(parse_toml("schema = 1\n").is_none());
    assert!(parse_toml("not toml {{{").is_none());
    assert!(parse_toml("schema = 2\n[cost.text.input]\nprice = 1\n").is_none()); // 缺 output
}

#[test]
fn host_core_strips_service_prefixes() {
    assert_eq!(
        host_core("https://api.deepseek.com/v1").as_deref(),
        Some("deepseek")
    );
    assert_eq!(
        host_core("https://open.bigmodel.cn/api/paas/v4").as_deref(),
        Some("bigmodel")
    );
    assert_eq!(
        host_core("http://127.0.0.1:8080/v1").as_deref(),
        Some("127")
    );
    assert_eq!(host_core(""), None);
}

#[test]
fn dir_ladder_core_then_alias_then_slug() {
    let dirs = provider_dir_candidates("https://api.deepseek.com/v1", "my-ds");
    assert_eq!(dirs, vec!["deepseek", "my-ds"]);
    let dirs = provider_dir_candidates("https://open.bigmodel.cn/api/paas/v4", "zhipu");
    assert_eq!(dirs, vec!["bigmodel", "zhipuai", "zai", "zhipu"]);
    let dirs = provider_dir_candidates("https://dashscope.aliyuncs.com/compatible-mode/v1", "x");
    assert_eq!(dirs, vec!["dashscope", "alibaba", "alibaba-cn", "x"]);
}

#[test]
fn fill_name_cost_zero_heals_and_never_overwrites() {
    let mk = |i: f64, o: f64| Entry {
        id: "m".into(),
        name: "M".into(),
        reasoning: true,
        input: vec![],
        context_window: None,
        max_tokens: None,
        cost: Cost {
            input: Some(i),
            output: Some(o),
            cache_read: None,
            cache_write: None,
        },
        source: SOURCE_MODELS_DEV.into(),
    };
    // 存量 0000（缺省补的）= 可填；无名条目顺带补名
    let mut m = serde_json::json!({"id": "m", "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}});
    assert_eq!(fill_name_cost(&mut m, &mk(0.15, 0.6)), 3);
    assert_eq!(m["cost"]["input"], 0.15);
    assert_eq!(m["cost"]["output"], 0.6);
    // 非零不覆盖；泛化标签名自愈
    let mut m = serde_json::json!({"id": "m", "name": "DeepSeek Flash Latest",
        "cost": {"input": 1.0, "output": 2.0, "cacheRead": 0, "cacheWrite": 0}});
    assert_eq!(fill_name_cost(&mut m, &mk(0.15, 0.6)), 1);
    assert_eq!(m["cost"]["input"], 1.0);
    assert_eq!(m["name"], "M");
}

#[test]
fn cache_roundtrip_and_expiry_keying() {
    let dir = std::env::temp_dir().join(format!("pf-mprice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.json");
    let e = parse_toml(DEEPSEEK_FLASH_TOML).unwrap();
    write_cache_entry(&path, "deepseek/deepseek-flash", &e);
    let doc = read_cache(&path);
    assert!(doc.entries.contains_key("deepseek/deepseek-flash"));
    assert_eq!(
        doc.entries["deepseek/deepseek-flash"].entry.cost.input,
        Some(0.15)
    );
    std::fs::remove_dir_all(&dir).ok();
}
