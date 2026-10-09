//! `~/.pi-flash/pf-auth.json` — PF 独立密钥账本（051-apikey管理.md）。
//!
//! 只记目录 provider（自定义 provider 的 key 引用住 models.json 的
//! apiKey 字段，不经这里）。每条：
//!
//! ```json
//! { "deepseek": { "type": "api_key", "key": "$PF_KEY_DEEPSEEK",
//!                 "injectAs": "DEEPSEEK_API_KEY", "mode": "vault" } }
//! ```
//!
//! - `key`：`$PF_KEY_*` 引用（明文在凭据库）/ 用户高级引用 / 降级明文
//! - `injectAs`：spawn 时注入的目标 env 名 = pi 官方名；官方表查不到的
//!   provider 改走 models.json apiKey 引用兜底，injectAs 记自身变量名
//! - `mode`：`vault` | `ref` | `file`（UI「已配置 · …」文案）
//!
//! auth.json 是 pi 的地盘：除首启一次性收编（复制不删）外 PF 不碰。

use std::path::Path;

use serde_json::{json, Value};

use crate::config::{parse_lenient, read_json, write_json_private};
use crate::credentials::{
    classify, env_var_name, official_env_name, pf_ref, SecretVault, StoreMode,
};

/// 账本路径；None = 自有目录不可用（极端环境，调用方按无凭据处理）。
pub fn path() -> Option<std::path::PathBuf> {
    crate::paths::pf_auth_file()
}

/// 读账本（无文件 → 空对象；坏文件 → Err）。
pub fn read_at(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    read_json(path)
}

pub fn write_at(path: &Path, value: &Value) -> Result<(), String> {
    write_json_private(path, value)
}

/// 条目列表：(provider, key 值, injectAs, mode)。
pub fn entries_at(
    path: &Path,
) -> Result<Vec<(String, String, Option<String>, &'static str)>, String> {
    let value = read_at(path)?;
    Ok(value
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(p, e)| {
                    let key = e.get("key")?.as_str()?.to_string();
                    let inject = e.get("injectAs").and_then(|v| v.as_str()).map(str::to_string);
                    let mode = e
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .and_then(StoreMode::from_str)
                        // 旧条目缺 mode：按 key 值形态推断
                        .unwrap_or_else(|| classify(&key))
                        .as_str();
                    Some((p.clone(), key, inject, mode))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// 条目是否存在（UI「已配置」口径）。
pub fn has_entry_at(path: &Path, provider: &str) -> bool {
    read_at(path)
        .ok()
        .and_then(|v| v.get(provider).cloned())
        .is_some()
}

/// 写/覆盖一条（保存 + 收编共用）。
pub fn set_entry_at(
    path: &Path,
    provider: &str,
    key: &str,
    inject_as: Option<&str>,
    mode: StoreMode,
) -> Result<(), String> {
    let mut value = read_at(path)?;
    let obj = value.as_object_mut().ok_or("pf-auth.json is not an object")?;
    let mut entry = json!({ "type": "api_key", "key": key, "mode": mode.as_str() });
    if let Some(inj) = inject_as {
        entry["injectAs"] = json!(inj);
    }
    obj.insert(provider.to_string(), entry);
    write_at(path, &value)
}

/// 删一条（断开连接；幂等）。
pub fn remove_at(path: &Path, provider: &str) -> Result<bool, String> {
    let mut value = read_at(path)?;
    let removed = value
        .as_object_mut()
        .ok_or("pf-auth.json is not an object")?
        .remove(provider)
        .is_some();
    if removed {
        write_at(path, &value)?;
    }
    Ok(removed)
}

/// 目录 provider 保存分流（051 §6）：明文 → 凭据库 + `$PF_KEY_*` 引用 +
/// injectAs 官方 env 名（查不到 → models.json apiKey 引用兜底，injectAs 记
/// 自身变量名）；`$`/`!` 高级引用原样；凭据库失败 → 降级明文（0600 文件）。
#[allow(clippy::too_many_arguments)]
pub fn store_catalog_key_at(
    pf_path: &Path,
    models_path: &Path,
    provider: &str,
    raw: &str,
    vault: &dyn SecretVault,
) -> Result<StoreMode, String> {
    let var = env_var_name(provider);
    let (stored, mode) = if raw.starts_with('$') || raw.starts_with('!') {
        (raw.to_string(), StoreMode::Ref)
    } else {
        match vault.set(&var, raw) {
            Ok(()) => (format!("${var}"), StoreMode::Vault),
            Err(_) => (raw.to_string(), StoreMode::File),
        }
    };
    let official = official_env_name(provider);
    let mut inject = official.map(str::to_string);
    if official.is_none() && mode == StoreMode::Vault {
        // 官方 env 名未知：models.json 的 provider 级 apiKey 是 pi 对任意
        // provider 都认的通道；injectAs 记自身变量名，spawn 照常注入。
        crate::models_json::upsert_provider_key_ref_at(models_path, provider, &stored)?;
        inject = Some(var);
    }
    set_entry_at(pf_path, provider, &stored, inject.as_deref(), mode)?;
    Ok(mode)
}

/// 目录 provider 断开连接：删账本条目 + 凭据库条目（幂等）；若该条目走了
/// models.json 兜底通道（injectAs == 自身变量名），顺带清掉那里的引用。
pub fn delete_catalog_key_at(
    pf_path: &Path,
    models_path: &Path,
    provider: &str,
    vault: &dyn SecretVault,
) -> Result<(), String> {
    let entry = read_at(pf_path).ok().and_then(|v| v.get(provider).cloned());
    let inject = entry
        .as_ref()
        .and_then(|e| e.get("injectAs"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let carried = inject.as_deref() == Some(env_var_name(provider).as_str());
    remove_at(pf_path, provider)?;
    let _ = vault.delete(&env_var_name(provider));
    if carried {
        crate::models_json::clear_provider_key_at(models_path, provider, vault)?;
    }
    Ok(())
}

/// 首启收编：auth.json 里老版本 PF 写入的明文 api_key **复制**进凭据库 +
/// 账本（复制不删，auth.json 归 pi）。已有账本条目的 provider 跳过（PF
/// 管理的新值优先），OAuth 与高级引用跳过。失败停在原状，下次启动重试。
pub fn adopt_from_auth_at(
    pf_path: &Path,
    models_path: &Path,
    auth_path: &Path,
    vault: &dyn SecretVault,
) -> Result<usize, String> {
    if !auth_path.exists() {
        return Ok(0);
    }
    let auth = read_json(auth_path)?;
    let mut adopted = 0;
    if let Some(obj) = auth.as_object() {
        for (provider, entry) in obj {
            if has_entry_at(pf_path, provider) {
                continue; // PF 已管理，不覆盖
            }
            if entry.get("type").and_then(|v| v.as_str()) != Some("api_key") {
                continue; // OAuth 归 pi
            }
            let Some(key) = entry.get("key").and_then(|v| v.as_str()) else {
                continue;
            };
            if pf_ref(key).is_some() || key.starts_with('$') || key.starts_with('!') {
                continue; // 引用形态不收编（pi 侧自管）
            }
            if store_catalog_key_at(pf_path, models_path, provider, key, vault).is_ok() {
                adopted += 1;
            }
        }
    }
    Ok(adopted)
}

/// models.json 里的明文 apiKey 原址迁移（PF UI 写入的存量）：迁凭据库 +
/// 改引用。已是引用/高级形态的跳过。返回迁移条数。
pub fn migrate_models_json_at(models_path: &Path, vault: &dyn SecretVault) -> Result<usize, String> {
    if !models_path.exists() {
        return Ok(0);
    }
    let mut doc = parse_lenient(&std::fs::read_to_string(models_path).map_err(|e| e.to_string())?)?;
    let mut migrated = 0;
    let Some(providers) = doc.get_mut("providers").and_then(|p| p.as_object_mut()) else {
        return Ok(0);
    };
    for (name, entry) in providers.iter_mut() {
        let Some(key) = entry.get("apiKey").and_then(|v| v.as_str()).map(str::to_string) else {
            continue;
        };
        if classify(&key) != StoreMode::File {
            continue;
        }
        let var = env_var_name(name);
        if vault.set(&var, &key).is_ok() {
            entry.as_object_mut().unwrap()
                .insert("apiKey".into(), json!(format!("${var}")));
            migrated += 1;
        }
    }
    if migrated > 0 {
        write_json_private(models_path, &doc)?;
    }
    Ok(migrated)
}

// -- 便捷封装（生产路径；*_at 变体供测试注路径） ------------------------------

pub fn entries() -> Result<Vec<(String, String, Option<String>, &'static str)>, String> {
    let p = path().ok_or("pf-auth dir unavailable")?;
    entries_at(&p)
}

/// UI 凭据清单（全部 api_key；OAuth 归 pi，不在此）。喂 Chat.mc_creds。
pub fn kinds() -> Vec<(String, crate::config::CredentialKind)> {
    path()
        .map(|p| {
            entries_at(&p)
                .unwrap_or_default()
                .into_iter()
                .map(|(provider, _, _, _)| (provider, crate::config::CredentialKind::ApiKey))
                .collect()
        })
        .unwrap_or_default()
}

/// UI「已配置」口径：账本里有该 provider。
pub fn has_entry(provider: &str) -> bool {
    path().is_some_and(|p| has_entry_at(&p, provider))
}

pub fn store_catalog_key(
    provider: &str,
    raw: &str,
    vault: &dyn SecretVault,
) -> Result<StoreMode, String> {
    let p = path().ok_or("pf-auth dir unavailable")?;
    store_catalog_key_at(&p, &crate::models_json::path(), provider, raw, vault)
}

pub fn delete_catalog_key(provider: &str, vault: &dyn SecretVault) -> Result<(), String> {
    let p = path().ok_or("pf-auth dir unavailable")?;
    delete_catalog_key_at(&p, &crate::models_json::path(), provider, vault)
}

/// UI 文案用的条目形态；None = 未配置。
pub fn store_mode(provider: &str) -> Option<StoreMode> {
    let p = path()?;
    read_at(&p).ok()?.get(provider)?.get("mode")?.as_str().and_then(StoreMode::from_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemVault;
    use serde_json::json;

    fn tmp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pf-pfauth-{}-{}.json", tag, std::process::id()))
    }

    fn clean(p: &Path) {
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn set_entries_remove_roundtrip() {
        let p = tmp("roundtrip");
        clean(&p);
        set_entry_at(&p, "deepseek", "$PF_KEY_DEEPSEEK", Some("DEEPSEEK_API_KEY"), StoreMode::Vault).unwrap();
        set_entry_at(&p, "odd-provider", "$PF_KEY_ODD_X1", None, StoreMode::Vault).unwrap();
        let entries = entries_at(&p).unwrap();
        assert_eq!(entries.len(), 2);
        let (pr, key, inject, mode) = &entries[0];
        assert_eq!(pr, "deepseek");
        assert_eq!(key, "$PF_KEY_DEEPSEEK");
        assert_eq!(inject.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert_eq!(*mode, "vault");
        assert!(has_entry_at(&p, "odd-provider"));
        assert!(remove_at(&p, "deepseek").unwrap());
        assert!(!remove_at(&p, "deepseek").unwrap()); // 幂等
        assert!(!has_entry_at(&p, "deepseek"));
        clean(&p);
    }

    #[test]
    fn store_catalog_key_routes_by_env_table() {
        let p = tmp("route");
        clean(&p);
        let mp = tmp("models-route");
        clean(&mp);
        let v = MemVault::default();
        // 官方 env 名已知：injectAs = 官方名，不碰 models.json
        let mode = store_catalog_key_at(&p, &mp, "deepseek", "sk-xxx", &v).unwrap();
        assert_eq!(mode, StoreMode::Vault);
        let entries = entries_at(&p).unwrap();
        assert_eq!(entries[0].2.as_deref(), Some("DEEPSEEK_API_KEY"));
        assert_eq!(v.get("PF_KEY_DEEPSEEK").unwrap().as_deref(), Some("sk-xxx"));
        // 高级引用原样、不进凭据库
        let mode = store_catalog_key_at(&p, &mp, "deepseek", "!my-cmd", &v).unwrap();
        assert_eq!(mode, StoreMode::Ref);
        // 官方名未知：models.json 兜底引用 + injectAs 自身变量名
        let mode = store_catalog_key_at(&p, &mp, "glm", "sk-glm", &v).unwrap();
        assert_eq!(mode, StoreMode::Vault);
        let entries = entries_at(&p).unwrap();
        let glm = entries.iter().find(|(p, _, _, _)| p == "glm").unwrap();
        assert_eq!(glm.2.as_deref(), Some("PF_KEY_GLM"));
        clean(&p);
        clean(&mp);
    }

    #[test]
    fn store_catalog_key_degrades_on_vault_failure() {
        let p = tmp("degrade");
        clean(&p);
        let mp = tmp("models-degrade");
        clean(&mp);
        let mode =
            store_catalog_key_at(&p, &mp, "deepseek", "sk-xxx", &crate::credentials::FailVault)
                .unwrap();
        assert_eq!(mode, StoreMode::File);
        let entries = entries_at(&p).unwrap();
        assert_eq!(entries[0].1, "sk-xxx"); // 明文降级
        assert_eq!(entries[0].3, "file");
        clean(&p);
        clean(&mp);
    }

    #[test]
    fn delete_catalog_key_clears_carrier_ref() {
        let p = tmp("delete");
        clean(&p);
        let mp = tmp("models-delete");
        std::fs::write(&mp, r#"{ "providers": {} }"#).unwrap();
        let v = MemVault::default();
        // glm 走 models.json 兜底 → 断开应清掉 models.json 里的 apiKey
        store_catalog_key_at(&p, &mp, "glm", "sk-glm", &v).unwrap();
        assert!(crate::models_json::read_at(&mp)
            .unwrap()
            .pointer("/providers/glm/apiKey")
            .is_some());
        delete_catalog_key_at(&p, &mp, "glm", &v).unwrap();
        assert!(!has_entry_at(&p, "glm"));
        assert!(v.get("PF_KEY_GLM").unwrap().is_none(), "凭据库条目应删除");
        assert!(
            crate::models_json::read_at(&mp).unwrap().pointer("/providers/glm/apiKey").is_none(),
            "兜底引用应清掉"
        );
        clean(&p);
        clean(&mp);
    }

    #[test]
    fn adopt_from_auth_copies_without_deleting() {
        let p = tmp("adopt");
        clean(&p);
        let mp = tmp("models-adopt");
        clean(&mp);
        let auth = tmp("auth-src");
        std::fs::write(
            &auth,
            json!({
                "deepseek": { "type": "api_key", "key": "sk-legacy" },
                "anthropic": { "type": "oauth", "access": "a", "refresh": "r", "expires": 1 },
                "glm": { "type": "api_key", "key": "$OTHER_ENV" }
            })
            .to_string(),
        )
        .unwrap();
        let v = MemVault::default();
        let n = adopt_from_auth_at(&p, &mp, &auth, &v).unwrap();
        assert_eq!(n, 1, "只收编明文 api_key（oauth/引用跳过）");
        // 复制不删：auth.json 原文不动
        let raw = std::fs::read_to_string(&auth).unwrap();
        assert!(raw.contains("sk-legacy"), "auth.json 必须保持原样");
        assert_eq!(v.get("PF_KEY_DEEPSEEK").unwrap().as_deref(), Some("sk-legacy"));
        // 幂等：已有账本条目不覆盖
        assert_eq!(adopt_from_auth_at(&p, &mp, &auth, &v).unwrap(), 0);
        clean(&p);
        clean(&mp);
        clean(&auth);
    }
}
