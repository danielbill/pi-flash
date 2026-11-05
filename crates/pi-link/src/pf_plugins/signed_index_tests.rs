use super::*;

/// 索引信封跨语言（由 inner/scripts/selftest.mjs 注入环境；缺环境跳过）。
#[test]
fn market_index_signed_verify() {
    let Ok(pem) = std::env::var("PF_X_IDX_PUB") else {
        return;
    };
    let signed = std::env::var("PF_X_IDX_SIGNED").unwrap();
    let sig = std::env::var("PF_X_IDX_SIG").unwrap();
    let raw =
        format!(r#"{{"format":"pf-market-signed/1","signed":"{signed}","signature":"{sig}"}}"#);
    let ix = verify_signed_index(&raw, &pem).expect("Node 签发的索引必须验过");
    assert!(ix.packages.is_empty());
    // 篡改 signed（换前 4 字符）→ 必拒
    let tampered = signed.replace(&signed[..4], "AAAA");
    let bad =
        format!(r#"{{"format":"pf-market-signed/1","signed":"{tampered}","signature":"{sig}"}}"#);
    assert!(verify_signed_index(&bad, &pem).is_err(), "篡改必须拒");
}
