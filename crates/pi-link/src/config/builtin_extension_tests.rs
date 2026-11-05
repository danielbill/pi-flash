use super::*;
use serde_json::json;

/// 期望值 = 全部内置扩展（默认口径）
fn all_builtins() -> Vec<String> {
    BUILTIN_EXTENSIONS.iter().map(|s| s.to_string()).collect()
}
#[test]
fn defaults_to_every_builtin_extension() {
    let on = enabled_builtin_extensions_from(None, None);
    assert_eq!(on, all_builtins());
}

#[test]
fn user_settings_can_disable_and_enable_builtins() {
    let user = json!({ "extensions": ["-builtin:mcp", "+builtin:codemode"] });
    let on = enabled_builtin_extensions_from(Some(&user), None);
    assert!(
        !on.iter().any(|x| x == "builtin:mcp"),
        "-builtin:mcp 应关掉 MCP"
    );
    assert!(on.iter().any(|x| x == "builtin:codemode"));
    // 其它内置扩展不受影响
    assert!(on.iter().any(|x| x == "builtin:tool-search"));
    assert!(on.iter().any(|x| x == "builtin:llama.cpp"));
}

#[test]
fn non_builtin_entries_are_ignored() {
    let user = json!({ "extensions": ["npm:pi-web-access", "./local.ts", "-builtin:nope"] });
    let on = enabled_builtin_extensions_from(Some(&user), None);
    assert_eq!(on, all_builtins());
}

#[test]
fn project_settings_override_user_settings() {
    let user = json!({ "extensions": ["-builtin:mcp", "-builtin:tool-search"] });
    let project = json!({ "extensions": ["builtin:mcp"] });
    let on = enabled_builtin_extensions_from(Some(&user), Some(&project));
    assert!(
        on.iter().any(|x| x == "builtin:mcp"),
        "项目 settings 覆盖用户"
    );
    assert!(
        !on.iter().any(|x| x == "builtin:tool-search"),
        "项目没覆盖的保持用户口径"
    );
}
