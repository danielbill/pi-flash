use super::*;

// ---------------------------------------------------------------------------
// 签名索引（090-3 §3.2 双轨后半：私域索引自动获取，**只认签名**）
// ---------------------------------------------------------------------------

/// 私域索引 URL（环境变量 `PI_FLASH_MARKET_INDEX`；未设置 = 只用内置索引，零请求）。
pub fn index_url() -> Option<String> {
    std::env::var("PI_FLASH_MARKET_INDEX")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().trim_end_matches('/').to_string())
}

/// 信封验签：{ format: pf-market-signed/1, signed: <b64(JSON)>, signature: ed25519:<b64> }。
/// 签名对象 = signed 字段解码后的原始字节（免 canonical JSON 折腾：签的就是发的那串）。
pub fn verify_signed_index(raw: &str, pubkey_pem: &str) -> Result<MarketIndex, String> {
    use base64::Engine as _;
    let envelope: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("索引信封解析失败: {e}"))?;
    if envelope["format"].as_str() != Some("pf-market-signed/1") {
        return Err("索引信封格式不符（缺 pf-market-signed/1）".into());
    }
    let signed_b64 = envelope["signed"].as_str().ok_or("索引缺 signed 字段")?;
    let sig_b64 = envelope["signature"]
        .as_str()
        .ok_or("索引缺 signature 字段")?
        .strip_prefix("ed25519:")
        .ok_or("signature 必须以 ed25519: 开头")?;
    let signed = base64::engine::general_purpose::STANDARD
        .decode(signed_b64)
        .map_err(|e| format!("signed 解码失败: {e}"))?;
    let sig = base64::engine::general_purpose::STANDARD
        .decode(sig_b64)
        .map_err(|e| format!("signature 解码失败: {e}"))?;
    let vk = verifying_key_from_pem(pubkey_pem)?;
    use ed25519_dalek::Signature;
    let sig = Signature::from_slice(&sig).map_err(|_| "signature 长度非法".to_string())?;
    vk.verify_strict(&signed, &sig)
        .map_err(|_| "索引验签失败（内容被改或钥不符）".to_string())?;
    MarketIndex::parse(&String::from_utf8(signed).map_err(|_| "signed 不是 UTF-8".to_string())?)
}

/// SPKI PEM → VerifyingKey（ed25519 专属壳 302a300506032b6570032100，手剥免依赖）。
fn verifying_key_from_pem(pem: &str) -> Result<ed25519_dalek::VerifyingKey, String> {
    use base64::Engine as _;
    const ED25519_SPKI_PREFIX: [u8; 12] = [
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("公钥 DER 解码失败: {e}"))?;
    if der.len() != 44 || der[..12] != ED25519_SPKI_PREFIX {
        return Err("公钥不是 ed25519 SPKI PEM".into());
    }
    let raw: [u8; 32] = der[12..]
        .try_into()
        .map_err(|_| "公钥长度非法".to_string())?;
    ed25519_dalek::VerifyingKey::from_bytes(&raw).map_err(|e| format!("公钥非法: {e}"))
}

/// npm 包名 → 磁盘目录名：`@pf-plugins-origin/quick` → `@pf-plugins-origin__quick`。
/// （`/` 是唯一非法字符；scope 属主即身份，不需要再哈希。）
pub fn dir_name(name: &str) -> String {
    name.replace('/', "__")
}

/// 台账包名 → 磁盘顶层目录的候选名（对账用）：市场安装链落 `@scope__name`，
/// dev 手工平铺常直接叫 `name`。**运行态条目 ↔ 台账条目 唯一可用的桥**——
/// 版本层目录名是版本号，拿它比包名永远比不中（090-4 v5 停用形同虚设的根因）。
pub fn dir_aliases(name: &str) -> Vec<String> {
    let full = dir_name(name);
    let mut out = vec![full.clone()];
    if let Some(short) = name.rsplit('/').next()
        && short != name
        && short != full
    {
        out.push(short.to_string());
    }
    out
}
