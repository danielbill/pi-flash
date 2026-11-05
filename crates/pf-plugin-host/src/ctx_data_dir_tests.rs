use super::*;

/// load_ctx 采纳 ctx JSON 的 plugin.dataDir（包层）；缺省保持 instantiate 猜测值。
#[test]
fn ctx_data_dir_parsed_from_plugin_ctx() {
    let hit = ctx_data_dir(
        r#"{"plugin":{"id":"pf-wxbot","version":"0.1.3","dataDir":"C:/u/.pi-flash/plugins/@x__pf-wxbot/data"},"pf":{"version":"0.3.2"}}"#,
    );
    assert_eq!(
        hit.as_deref(),
        Some(Path::new("C:/u/.pi-flash/plugins/@x__pf-wxbot/data"))
    );
    // 缺 dataDir / 空串 / 非 JSON → None（兼容旧 ctx，不改写）
    assert!(ctx_data_dir(r#"{"plugin":{"id":"x"}}"#).is_none());
    assert!(ctx_data_dir(r#"{"plugin":{"dataDir":""}}"#).is_none());
    assert!(ctx_data_dir("not json").is_none());
}
