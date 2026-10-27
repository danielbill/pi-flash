//! 042-1 一次性迁移：PF 私有账本 → pi 的两个文件。
//!
//! | 旧 | 新 |
//! |---|---|
//! | `~/.pi-flash/providers.json`（自定义商 + `$PF_KEY_*` 引用） | `models.json` 的 provider 条目 + `auth.json` 标记条目 |
//! | `~/.pi-flash/pf-auth.json`（目录商 key 引用） | `auth.json` 标记条目（秘密本来就在钥匙库） |
//!
//! 幂等：旧文件改名 `.bak` 后不再处理；同名 provider 以 `models.json` 现有为准
//!（不覆盖）。**用户手写条目一律只读**——只会在 provider 不存在时新增。
//!
//! 自校验用 [`crate::auth_json::resolve_secret`]（与 pi 取值同一机制：标记
//! 条目 → 钥匙库），不起 node 进程（`pi auth check` 每家一次太贵）。

use serde_json::{json, Value};

use crate::auth_json;
use crate::credentials::{KeyringVault, SecretVault};
use crate::models_json;
use crate::paths;

fn backup(path: &std::path::Path) -> Result<(), String> {
    let mut target = path.as_os_str().to_os_string();
    target.push(".bak");
    let target = std::path::PathBuf::from(target);
    if target.exists() {
        let _ = std::fs::remove_file(&target);
    }
    std::fs::rename(path, &target).map_err(|e| format!("备份失败 {}: {e}", path.display()))
}

/// `$PF_KEY_*` / `$VAR` → 变量名（只认 PF 前缀，避免把用户自己的引用搬走）。
fn pf_var(value: &str) -> Option<&str> {
    let v = value.strip_prefix('$')?;
    let v = v.strip_prefix('{').unwrap_or(v);
    let v = v.strip_suffix('}').unwrap_or(v);
    (v.starts_with("PF_KEY_") && v.len() > "PF_KEY_".len()).then_some(v)
}

/// 迁移是否可接管该商：无 auth.json 条目，或条目本来就是 PF 标记形态。
fn should_adopt(existing: Option<&Value>) -> bool {
    match existing {
        None => true,
        Some(cred) => auth_json::is_pf_managed(cred),
    }
}

fn read_json_file(path: &std::path::Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    crate::config::parse_lenient(&text).ok()
}

/// 跑一遍迁移，返回可展示的摘要行（空 = 什么都没做）。
pub fn run() -> Vec<String> {
    let mut log = Vec::new();
    run_providers(&mut log);
    run_auth(&mut log);
    cleanup_caches(&mut log);
    log
}

fn run_providers(log: &mut Vec<String>) {
    let Some(path) = paths::pf_providers_file() else {
        return;
    };
    if !path.exists() {
        return;
    }
    let Some(doc) = read_json_file(&path) else {
        log.push(format!("providers.json 无法解析，已跳过：{}", path.display()));
        return;
    };
    let mut models = models_json::read().unwrap_or_else(|_| json!({ "providers": {} }));
    if !models.get("providers").is_some_and(Value::is_object) {
        models["providers"] = json!({});
    }
    let mut moved = 0usize;
    for (name, entry) in models_json::providers(&doc) {
        let entry = entry.clone();
        if models_json::provider_entry(&models, &name).is_none() {
            models_json::upsert_provider(&mut models, &name, entry.clone());
            moved += 1;
        }
        // 凭据：`$PF_KEY_*` → 钥匙库 → auth.json 标记条目（解不出则跳过）
        if let Some(key) = entry.get("apiKey").and_then(Value::as_str) {
            if let Some(var) = pf_var(key) {
                if let Ok(Some(secret)) = KeyringVault.get(var) {
                    if auth_json::store_secret(&name, &secret).is_ok() {
                        // PF 账本里的引用退役：models.json 不写 apiKey
                        if let Some(obj) = models
                            .get_mut("providers")
                            .and_then(|p| p.get_mut(&name))
                            .and_then(Value::as_object_mut)
                        {
                            obj.remove("apiKey");
                        }
                    }
                }
            }
        }
    }
    if moved > 0 || models_json::providers(&doc).iter().any(|(n, _)| {
        auth_json::resolve_secret(n).is_some()
    }) {
        match models_json::save(&models) {
            Ok(()) => log.push(format!("已迁移 {moved} 个自定义 provider 到 models.json")),
            Err(e) => log.push(format!("models.json 写入失败（已跳过迁移）：{e}")),
        }
    }
    if let Err(e) = backup(&path) {
        log.push(e);
    }
}

fn run_auth(log: &mut Vec<String>) {
    let Some(path) = paths::pf_auth_file() else {
        return;
    };
    if !path.exists() {
        return;
    }
    let Some(doc) = read_json_file(&path) else {
        log.push(format!("pf-auth.json 无法解析，已跳过：{}", path.display()));
        return;
    };
    let Some(entries) = doc.as_object() else {
        let _ = backup(&path);
        return;
    };
    let mut moved = 0usize;
    let mut skipped = 0usize;
    for (provider, cred) in entries {
        let Some(key) = cred.get("key").and_then(Value::as_str) else {
            continue;
        };
        // 高级引用（`!cmd` / 非 PF 变量）不搬：那是用户自己的取值方式
        let Some(var) = pf_var(key) else { continue };
        let Ok(Some(secret)) = KeyringVault.get(var) else {
            continue;
        };
        // 规则 1：auth.json 里已有**用户手写**条目（非 PF 标记）时一律不动——
        // 用户自己写的明文 key 由用户自己决定何时收进钥匙库。
        let existing = auth_json::read_credential(provider).ok().flatten();
        if !should_adopt(existing.as_ref()) {
            skipped += 1;
            continue;
        }
        if auth_json::store_secret(provider, &secret).is_ok()
            && auth_json::resolve_secret(provider).is_some()
        {
            moved += 1;
        }
    }
    if moved > 0 {
        log.push(format!("已把 {moved} 个 apikey 的引用改写成 auth.json 标记条目"));
    }
    if skipped > 0 {
        log.push(format!(
            "{skipped} 个商在 auth.json 里已有你手写的凭据，保持原样未动"
        ));
    }
    if let Err(e) = backup(&path) {
        log.push(e);
    }
}

fn cleanup_caches(log: &mut Vec<String>) {
    let mut freed = Vec::new();
    let Some(dir) = paths::ensure_dir() else {
        return;
    };
    for path in [
        dir.join("provider-catalog.json"),
        dir.join("models-dev-cache.json"),
        dir.join("catalog-cache.json"),
    ] {
        if path.exists() && std::fs::remove_file(&path).is_ok() {
            if let Some(name) = path.file_name() {
                freed.push(name.to_string_lossy().to_string());
            }
        }
    }
    if !freed.is_empty() {
        log.push(format!("已清理旧缓存：{}", freed.join(", ")));
    }
}

#[cfg(test)]
mod tests {
    use super::{pf_var, should_adopt};
    use serde_json::json;

    #[test]
    fn adopt_never_touches_user_credentials() {
        assert!(should_adopt(None), "无条目 → 可接管");
        assert!(should_adopt(Some(
            &json!({"type":"api_key","key":"!'C:/x/pif-secret.exe' get deepseek"})
        )));
        assert!(!should_adopt(Some(
            &json!({"type":"api_key","key":"sk-user"})
        )), "用户手写明文必须原样保留");
        assert!(!should_adopt(Some(
            &json!({"type":"oauth","access":"a","refresh":"r","expires":1})
        )), "oauth 归 pi");
    }

    #[test]
    fn pf_var_only_accepts_pf_refs() {
        assert_eq!(pf_var("$PF_KEY_DEEPSEEK"), Some("PF_KEY_DEEPSEEK"));
        assert_eq!(pf_var("${PF_KEY_A}"), Some("PF_KEY_A"));
        assert_eq!(pf_var("$OTHER_VAR"), None);
        assert_eq!(pf_var("!cmd"), None);
        assert_eq!(pf_var("sk-plain"), None);
    }
}
