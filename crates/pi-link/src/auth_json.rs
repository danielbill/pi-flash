//! `~/.pi/agent/auth.json` 的 PF 视图。
//!
//! 铁律（042-1 §2）：**只碰 PF 自己写的「标记条目」**，用户手写的明文与
//! pi 的 OAuth 条目一律原样透传。
//!
//! 标记形态：
//! ```json
//! { "deepseek": { "type": "api_key", "key": "!'C:/…/pif-secret.exe' get deepseek" } }
//! ```
//! pi 的 `AuthStorage.read()` 对 api_key 的 `key` 走 `resolveConfigValue`，
//! `!` 前缀 = 执行命令取 stdout 作为 key。于是秘密只存在系统钥匙库，盘上
//! 永远没有明文；任何前端（PF / pi TUI / pi-web）都由 pi 自己取回。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{auth_path, read_json, write_json_private, CredentialKind};
use crate::credentials::SecretVault;
use crate::fs_lock;

/// 标记条目里的命令前缀成员：判据靠它区分「PF 写的」与「用户写的」。
const MARKER_TAG: &str = "pif-secret";

/// helper 路径：与当前可执行文件同目录（开发期 = target/debug，安装后 =
/// 安装目录；两处都随构建产出 pif-secret）。
pub fn helper_exe() -> PathBuf {
    let name = if cfg!(windows) {
        "pif-secret.exe"
    } else {
        "pif-secret"
    };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// 写进 auth.json 的 key 值。路径统一正斜杠 + 单引号：pi 在 Windows 走
/// Git Bash 的 `bash -c`，没有 Git Bash 时回落 `sh -c`，两种 shell 都吃
/// 单引号；反斜杠会被 shell 当转义符吞掉。
pub fn marker_key(provider: &str) -> String {
    let raw = helper_exe().display().to_string().replace('\\', "/");
    let exe = raw.replace('\'', "'\\''");
    format!("!'{exe}' get {provider}")
}

/// 是否 PF 标记条目。
pub fn is_pf_managed(cred: &Value) -> bool {
    if cred.get("type").and_then(Value::as_str) != Some("api_key") {
        return false;
    }
    cred.get("key")
        .and_then(Value::as_str)
        .is_some_and(|k| k.starts_with('!') && k.contains(MARKER_TAG))
}

/// 用户手写的明文 key（非标记、非 oauth）。
pub fn plaintext_key(cred: &Value) -> Option<&str> {
    if is_pf_managed(cred) {
        return None;
    }
    if cred.get("type").and_then(Value::as_str) != Some("api_key") {
        return None;
    }
    cred.get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|k| !k.is_empty() && !k.starts_with('!'))
}

fn read_strict(path: &Path) -> Result<Value, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Default::default()))
        }
        Err(e) => return Err(e.to_string()),
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(text).map_err(|e| format!("auth.json 无法解析，已拒绝写入：{e}"))
}

// ---------------------------------------------------------------------------
// 读
// ---------------------------------------------------------------------------

pub fn kinds() -> Result<Vec<(String, CredentialKind)>, String> {
    kinds_at(&auth_path())
}

pub fn kinds_at(path: &Path) -> Result<Vec<(String, CredentialKind)>, String> {
    let value = read_json(path)?;
    let mut out = Vec::new();
    if let Some(obj) = value.as_object() {
        for (provider, cred) in obj {
            let kind = if cred.get("type").and_then(Value::as_str) == Some("oauth") {
                CredentialKind::OAuth
            } else {
                CredentialKind::ApiKey
            };
            out.push((provider.clone(), kind));
        }
    }
    Ok(out)
}

pub fn read_credential(provider: &str) -> Result<Option<Value>, String> {
    read_credential_at(&auth_path(), provider)
}

pub fn read_credential_at(path: &Path, provider: &str) -> Result<Option<Value>, String> {
    let value = read_json(path)?;
    Ok(value.get(provider).cloned())
}

// ---------------------------------------------------------------------------
// 写（锁内整份读改写）
// ---------------------------------------------------------------------------

/// 写 PF 标记条目（覆盖同商旧条目——含用户手写明文的覆盖，决策 042-1 §9-4）。
pub fn store_marker(provider: &str) -> Result<(), String> {
    store_marker_at(&auth_path(), provider, &marker_key(provider))
}

pub fn store_marker_at(path: &Path, provider: &str, marker: &str) -> Result<(), String> {
    update(path, |doc| {
        doc.insert(
            provider.to_string(),
            serde_json::json!({ "type": "api_key", "key": marker }),
        );
    })
}

/// 只删 PF 标记条目；非标记（用户明文 / oauth）→ `Ok(false)` 且文件不动。
pub fn remove_marker(provider: &str) -> Result<bool, String> {
    remove_marker_at(&auth_path(), provider)
}

pub fn remove_marker_at(path: &Path, provider: &str) -> Result<bool, String> {
    let mut removed = false;
    update(path, |doc| {
        let own = doc.get(provider).is_some_and(is_pf_managed);
        if own {
            doc.remove(provider);
            removed = true;
        }
    })?;
    Ok(removed)
}

/// 迁移器用：把一条**用户明文**条目收进标记形态（钥匙库由调用方先写）。
/// 只在当前值仍等于 `secret` 时改写，避免覆盖用户刚改过的新值。
pub fn seal_plaintext(provider: &str, secret: &str) -> Result<bool, String> {
    seal_plaintext_at(&auth_path(), provider, secret, &marker_key(provider))
}

pub fn seal_plaintext_at(
    path: &Path,
    provider: &str,
    secret: &str,
    marker: &str,
) -> Result<bool, String> {
    let mut sealed = false;
    update(path, |doc| {
        let Some(cred) = doc.get(provider) else { return };
        if plaintext_key(cred) != Some(secret) {
            return;
        }
        doc.insert(
            provider.to_string(),
            serde_json::json!({ "type": "api_key", "key": marker }),
        );
        sealed = true;
    })?;
    Ok(sealed)
}

// ---------------------------------------------------------------------------
// 秘密（钥匙库）+ 标记条目：一个入口
// ---------------------------------------------------------------------------

/// 保存 apikey：①钥匙库写入并读回校验 ②auth.json 写标记条目（失败回滚钥匙库）。
/// 覆盖同商旧条目（含用户手写明文的覆盖——决策 042-1 §9-4）。
pub fn store_secret(provider: &str, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err("API Key 不能为空".to_string());
    }
    let entry = crate::credentials::env_var_name(provider);
    let vault = crate::credentials::KeyringVault;
    vault.set(&entry, secret)?;
    let readback = vault.get(&entry).ok().flatten();
    if readback.as_deref() != Some(secret) {
        let _ = vault.delete(&entry);
        return Err("钥匙库读回校验失败（保存已回滚）".to_string());
    }
    if let Err(e) = store_marker(provider) {
        let _ = vault.delete(&entry);
        return Err(e);
    }
    Ok(())
}

/// 移除 apikey：只删 PF 标记条目（+ 对应钥匙库条目）。用户明文 / oauth 不动。
pub fn delete_secret(provider: &str) -> Result<bool, String> {
    let removed = remove_marker(provider)?;
    if removed {
        let _ =
            crate::credentials::KeyringVault.delete(&crate::credentials::env_var_name(provider));
    }
    Ok(removed)
}

/// 取该商可用的秘密（PF 标记 → 钥匙库；用户明文 → 原值）。UI 掩码预填与
/// 通路探查共用。
pub fn resolve_secret(provider: &str) -> Option<String> {
    let cred = read_credential(provider).ok().flatten()?;
    if is_pf_managed(&cred) {
        crate::credentials::KeyringVault
            .get(&crate::credentials::env_var_name(provider))
            .ok()
            .flatten()
    } else {
        plaintext_key(&cred).map(str::to_string)
    }
}

fn update<F>(path: &Path, f: F) -> Result<(), String>
where
    F: FnOnce(&mut serde_json::Map<String, Value>),
{
    let _guard = fs_lock::lock(path, std::time::Duration::from_secs(10))?;
    // 不可读拒写：用户把文件写坏了也不能整份覆盖（pi 的 schema 是整份校验）
    let value = read_strict(path)?;
    let mut obj = match value {
        Value::Object(o) => o,
        _ => return Err("auth.json 顶层必须是对象，已拒绝写入".to_string()),
    };
    f(&mut obj);
    write_json_private(path, &Value::Object(obj))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::SystemTime;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pf-auth-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("auth.json")
    }

    const MARKER: &str = "!'C:/pf/pif-secret.exe' get deepseek";

    #[test]
    fn marker_detection_covers_three_shapes() {
        assert!(is_pf_managed(&json!({"type":"api_key","key":MARKER})));
        assert!(!is_pf_managed(&json!({"type":"api_key","key":"sk-plain"})));
        assert!(!is_pf_managed(
            &json!({"type":"oauth","access":"a","refresh":"r","expires":1})
        ));
        assert_eq!(
            plaintext_key(&json!({"type":"api_key","key":"sk-plain"})),
            Some("sk-plain")
        );
        assert_eq!(plaintext_key(&json!({"type":"api_key","key":MARKER})), None);
    }

    #[test]
    fn store_and_remove_marker_roundtrip() {
        let path = tmp("roundtrip");
        store_marker_at(&path, "deepseek", MARKER).unwrap();
        let cred = read_credential_at(&path, "deepseek").unwrap().unwrap();
        assert!(is_pf_managed(&cred));
        assert!(remove_marker_at(&path, "deepseek").unwrap());
        assert!(read_credential_at(&path, "deepseek").unwrap().is_none());
        // 二次删除：幂等
        assert!(!remove_marker_at(&path, "deepseek").unwrap());
    }

    #[test]
    fn remove_refuses_user_plaintext_and_oauth_and_keeps_bytes() {
        let path = tmp("refuse");
        let original = json!({
            "deepseek": { "type": "api_key", "key": "sk-user-plain" },
            "anthropic": { "type": "oauth", "access": "a", "refresh": "r", "expires": 9 }
        });
        write_json_private(&path, &original).unwrap();
        let before = std::fs::read(&path).unwrap();

        assert!(!remove_marker_at(&path, "deepseek").unwrap());
        assert!(!remove_marker_at(&path, "anthropic").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), before, "非标记条目不得改动");
    }

    #[test]
    fn store_marker_preserves_other_entries() {
        let path = tmp("preserve");
        write_json_private(
            &path,
            &json!({
                "anthropic": { "type": "oauth", "access": "a", "refresh": "r", "expires": 9 },
                "glm": { "type": "api_key", "key": "!mine" }
            }),
        )
        .unwrap();
        store_marker_at(&path, "deepseek", MARKER).unwrap();
        let doc = read_json(&path).unwrap();
        assert_eq!(doc["anthropic"]["type"], "oauth");
        assert_eq!(doc["glm"]["key"], "!mine");
        assert!(is_pf_managed(&doc["deepseek"]));
    }

    #[test]
    fn seal_plaintext_only_when_value_matches() {
        let path = tmp("seal");
        write_json_private(
            &path,
            &json!({ "deepseek": { "type": "api_key", "key": "sk-old" } }),
        )
        .unwrap();
        assert!(!seal_plaintext_at(&path, "deepseek", "sk-newer", MARKER).unwrap());
        assert!(seal_plaintext_at(&path, "deepseek", "sk-old", MARKER).unwrap());
        assert!(is_pf_managed(
            &read_credential_at(&path, "deepseek").unwrap().unwrap()
        ));
    }

    #[test]
    fn broken_file_is_never_clobbered() {
        let path = tmp("broken");
        std::fs::write(&path, b"{ not json").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(store_marker_at(&path, "deepseek", MARKER).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn kinds_reads_types() {
        let path = tmp("kinds");
        write_json_private(
            &path,
            &json!({
                "a": { "type": "api_key", "key": "x" },
                "b": { "type": "oauth", "access": "a", "refresh": "r", "expires": 1 }
            }),
        )
        .unwrap();
        let kinds = kinds_at(&path).unwrap();
        assert!(kinds.contains(&("a".into(), CredentialKind::ApiKey)));
        assert!(kinds.contains(&("b".into(), CredentialKind::OAuth)));
    }

    #[test]
    fn marker_key_is_shell_safe() {
        let key = marker_key("deepseek");
        assert!(key.starts_with("!'"));
        assert!(key.ends_with("' get deepseek"));
        assert!(!key.contains('\\'), "路径反斜杠会被 shell 吞掉");
    }
}
