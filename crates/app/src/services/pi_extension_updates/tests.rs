//! pi_extension_updates 测试（itlq.9 自 pi_extension_updates.rs 外移，纯结构搬移）。

use super::*;

const DOC: &str = r#"{
    "name": "pi-mcp-adapter",
    "dist-tags": { "latest": "7.0.0", "beta": "8.0.0-beta.1" },
    "versions": { "5.3.2": {}, "6.0.0": {}, "7.0.0": {}, "8.0.0-beta.1": {} }
}"#;

fn doc() -> serde_json::Value {
    serde_json::from_str(DOC).unwrap()
}

fn v(s: &str) -> semver::Version {
    semver::Version::parse(s).unwrap()
}

/// 无范围 → dist-tags.latest（不得被 8.0.0-beta.1 抢先）
#[test]
fn latest_uses_dist_tag_not_max_key() {
    assert_eq!(latest_from_registry(&doc(), None).unwrap(), v("7.0.0"));
}

/// 范围命中 latest → 直接用 latest
#[test]
fn range_hits_latest() {
    assert_eq!(
        latest_from_registry(&doc(), Some("^7.0.0")).unwrap(),
        v("7.0.0")
    );
}

/// 范围不命中 latest → versions 里 maxSatisfying（^5 → 5.3.2）
#[test]
fn range_falls_back_to_max_satisfying() {
    assert_eq!(
        latest_from_registry(&doc(), Some("^5.3.0")).unwrap(),
        v("5.3.2")
    );
}

/// dist-tag 名当 want（npm:pkg@beta）→ 查 dist-tags.beta
#[test]
fn dist_tag_name_resolves() {
    assert_eq!(
        latest_from_registry(&doc(), Some("beta")).unwrap(),
        v("8.0.0-beta.1")
    );
}

/// 缺 latest / 缺该 dist-tag / 范围无解 → error（不静默当已是最新）
#[test]
fn missing_targets_are_errors() {
    let empty: serde_json::Value = serde_json::from_str("{}").unwrap();
    assert!(latest_from_registry(&empty, None).is_err());
    assert!(latest_from_registry(&doc(), Some("next")).is_err());
    assert!(latest_from_registry(&doc(), Some("^9.0.0")).is_err());
}

/// scoped 来源解析出包名（registry 路径要用）+ 版本范围
#[test]
fn parse_scoped_source_keeps_name() {
    let npm = parse_npm_source("npm:@scope/pkg@^1.2.3").unwrap();
    assert_eq!(npm.name, "@scope/pkg");
    assert_eq!(npm.version.as_deref(), Some("^1.2.3"));
    let bare = parse_npm_source("npm:pi-web-access").unwrap();
    assert_eq!(bare.name, "pi-web-access");
    assert!(bare.version.is_none());
}

/// registry URL 里 scoped 包的 `/` 要编码（name → 路径段）
#[test]
fn scoped_name_encoding() {
    assert_eq!("@scope/pkg".replace('/', "%2F"), "@scope%2Fpkg");
}
