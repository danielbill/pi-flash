use super::*;

#[test]
fn lookup_supports_dotted_and_bracket_paths() {
    let v = json!({"tabs": [{"file": "a.md", "dirty": false}, {"file": "b.md"}], "app": {"booted": true}});
    assert_eq!(lookup(&v, "app.booted"), Some(&json!(true)));
    assert_eq!(lookup(&v, "tabs.0.file"), Some(&json!("a.md")));
    assert_eq!(lookup(&v, "tabs[0].file"), Some(&json!("a.md")));
    assert_eq!(lookup(&v, "tabs[1].dirty"), None); // 键不存在
    assert_eq!(lookup(&v, "tabs.5.file"), None); // 越界
    assert_eq!(lookup(&v, "app.booted.x"), None); // 标量再下钻
    assert_eq!(lookup(&v, ".app..booted"), Some(&json!(true))); // 容忍空段
    assert_eq!(lookup(&v, "[0].x"), None); // 根不是数组
}

#[test]
fn truthy_matrix() {
    assert!(value_truthy(&json!(true)));
    assert!(value_truthy(&json!(1)));
    assert!(value_truthy(&json!("x")));
    assert!(value_truthy(&json!([0])));
    assert!(value_truthy(&json!({"a": 1})));
    assert!(!value_truthy(&json!(false)));
    assert!(!value_truthy(&json!(null)));
    assert!(!value_truthy(&json!(0)));
    assert!(!value_truthy(&json!("")));
    assert!(!value_truthy(&json!([])));
}

#[test]
fn split_args_collects_repeatable_value_flags() {
    let args: Vec<String> = [
        "file.open",
        "--arg",
        "path=C:/x/y.md",
        "--arg",
        "all=true",
        "leftover",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (pos, flags) = split_args(&args, &["arg"]);
    assert_eq!(pos, vec!["file.open".to_string(), "leftover".to_string()]);
    assert_eq!(flags.len(), 2);
    assert_eq!(flags[0], ("arg".to_string(), "path=C:/x/y.md".to_string()));
    // 值按 JSON 解析的路径在 exec 主流程；此处只验切分
}
