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

// ---------------------------------------------------------------------------
// 082 v2 混排：pf_update_rows 纯推导（台账 × 索引）
// ---------------------------------------------------------------------------

use pi_link::pf_plugins::{LedgerEntry, MarketIndex};

fn ledger(name: &str, version: &str, disabled: bool) -> LedgerEntry {
    LedgerEntry {
        name: name.to_string(),
        version: version.to_string(),
        integrity: String::new(),
        installed_at: String::new(),
        source: "market".to_string(),
        disabled,
    }
}

fn index(json: &str) -> MarketIndex {
    MarketIndex::parse(json).unwrap()
}

const IX: &str = r#"{
    "packages": [
        { "name": "@pi-flash-plugins/ctx-compactor", "version": "1.3.0", "integrity": "sha512-x",
          "display": { "zh": "会话上下文自动压缩", "en": "Context compactor" } },
        { "name": "@pi-flash-plugins/same-ver", "version": "2.0.0", "integrity": "sha512-x",
          "display": { "zh": "同版插件", "en": "" } },
        { "name": "@pi-flash-plugins/older-ix", "version": "0.9.0", "integrity": "sha512-x",
          "display": { "zh": "索引更旧", "en": "" } },
        { "name": "@pi-flash-plugins/no-zh", "version": "3.0.0", "integrity": "sha512-x",
          "display": { "zh": "", "en": "No zh" } }
    ]
}"#;

/// 索引版本更高 → 行；前缀 `Pi-Flash · ` + 索引 zh 名
#[test]
fn pf_rows_updatable_with_prefix() {
    let lg = vec![ledger("@pi-flash-plugins/ctx-compactor", "1.2.0", false)];
    let rows = pf_update_rows(&lg, &index(IX));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "@pi-flash-plugins/ctx-compactor");
    assert_eq!(rows[0].display, "Pi-Flash · 会话上下文自动压缩");
}

/// 同版 / 索引更旧 / 索引缺失（dev 侧载）→ 不出行
#[test]
fn pf_rows_skip_non_updatable() {
    let lg = vec![
        ledger("@pi-flash-plugins/same-ver", "2.0.0", false),
        ledger("@pi-flash-plugins/older-ix", "1.0.0", false),
        ledger("@pi-flash-plugins/unindexed", "1.0.0", false),
    ];
    assert!(pf_update_rows(&lg, &index(IX)).is_empty());
}

/// 停用包照出行（与市场已装页检测口径一致）
#[test]
fn pf_rows_include_disabled() {
    let lg = vec![ledger("@pi-flash-plugins/ctx-compactor", "1.2.0", true)];
    assert_eq!(pf_update_rows(&lg, &index(IX)).len(), 1);
}

/// 索引 zh 名为空 → 回落包名（仍带前缀）
#[test]
fn pf_rows_display_falls_back_to_name() {
    let lg = vec![ledger("@pi-flash-plugins/no-zh", "2.0.0", false)];
    let rows = pf_update_rows(&lg, &index(IX));
    assert_eq!(rows[0].display, "Pi-Flash · @pi-flash-plugins/no-zh");
}

/// 混排口径：pi 行在前、PF 行恒排末尾——pf_rows 与 pi results 天然分列，
/// 卡片侧顺序拼接即满足；这里固化 pf_rows 与 pi available() 独立（n = 和）
#[test]
fn pf_rows_independent_of_pi_count() {
    let st = UpdatesState::default();
    assert_eq!(st.available(), 0);
    assert!(st.pf_rows.is_empty());
}
