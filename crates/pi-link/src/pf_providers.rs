//! `~/.pi-flash/providers.json` — PF 自定义 provider 账本（051 M1.1）。
//!
//! **铁律：PF 永不写 `~/.pi/agent/models.json`**（读只读；修复也不写）。
//! 自定义 provider 全部记在这份 PF 私有账本里（schema 沿用 models.json 的
//! providers 形状），spawn 时经官方扩展 `pf-providers.mjs`（`-e` 注入）在
//! pi 进程内 `pi.registerProvider`， apiKey 的 `$PF_KEY_*` 引用由 env 注入
//! 解析。本模块 = 账本 I/O（0600 + 落盘净化）+ 凭据分流 + 扩展模板生成 +
//! models.json 一次性复制迁移（复制不删，pi 文件字节不动）。

use std::path::Path;

use serde_json::{json, Value};

use crate::config::{parse_lenient, write_json_private};
use crate::credentials::{
    classify, env_var_name, pf_ref, SecretVault, StoreMode,
};

pub const EXT_TEMPLATE_VERSION: &str = "v1";

pub fn path() -> Option<std::path::PathBuf> {
    crate::paths::pf_providers_file()
}

pub fn ext_path() -> Option<std::path::PathBuf> {
    crate::paths::pf_providers_ext_file()
}

/// 读账本（无文件 → 空 `{providers:{}}`；坏文件 → Err）。
pub fn read_at(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({ "providers": {} }));
    }
    read_doc(path)
}

pub fn read() -> Result<Value, String> {
    let p = path().ok_or("pf providers dir unavailable")?;
    read_at(&p)
}

fn read_doc(path: &Path) -> Result<Value, String> {
    parse_lenient(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
}

/// 落盘净化（051 事故根治）：丢弃空 id / 缺 id 的模型条目、无有效内容的
/// 空 provider，保证写出去的文件永远满足 pi 的 schema（id minLength 1）。
pub fn sanitize(doc: &mut Value) {
    let Some(obj) = doc.as_object_mut() else {
        *doc = json!({ "providers": {} });
        return;
    };
    let Some(providers) = obj.get_mut("providers").and_then(|p| p.as_object_mut()) else {
        obj.insert("providers".into(), json!({}));
        return;
    };
    let names: Vec<String> = providers.keys().cloned().collect();
    for name in names {
        let keep = providers.get_mut(&name).is_some_and(|entry| {
            let Some(map) = entry.as_object_mut() else { return false };
            // 模型条目：只留 id 为非空字符串的（051 事故根治点）
            if let Some(Value::Array(arr)) = map.remove("models") {
                let kept: Vec<Value> = arr
                    .into_iter()
                    .filter(|m| {
                        m.get("id").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty())
                    })
                    .collect();
                if !kept.is_empty() {
                    map.insert("models".into(), Value::Array(kept));
                }
            }
            entry_has_content(entry)
        });
        if !keep {
            providers.remove(&name);
        }
    }
}

/// provider 条目是否有保留价值（净化判据）。**apiKey 不算内容**——只有
/// apiKey 的裸条目是旧版 catch-all 写入的垃圾，复制迁移同样跳过它。
fn entry_has_content(entry: &Value) -> bool {
    let Some(map) = entry.as_object() else { return false };
    let has_text = |k: &str| map.get(k).and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
    if has_text("baseUrl") || has_text("api") || has_text("name") {
        return true;
    }
    if map.contains_key("oauth") || map.contains_key("modelOverrides") {
        return true;
    }
    map.get("models")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|m| {
                m.get("id").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty())
            })
        })
}

/// 写账本（先净化）。空 id 条目永远不落盘。
pub fn write_at(path: &Path, value: &Value) -> Result<(), String> {
    let mut doc = value.clone();
    sanitize(&mut doc);
    write_json_private(path, &doc)
}

pub fn write(value: &Value) -> Result<(), String> {
    let p = path().ok_or("pf providers dir unavailable")?;
    write_at(&p, value)
}

/// 自定义 provider 的 apiKey 写入分流（编辑器/添加面板共用）：明文 → 凭据
/// 库 + `$PF_KEY_*` 引用；`$`/`!` 高级引用原样；凭据库失败 → 降级明文
/// （账本 0600，PF 私有，可降级）。空串清除字段 → Ok(None)。
pub fn set_provider_key(
    entry: &mut Value,
    provider: &str,
    raw: &str,
    vault: &dyn SecretVault,
) -> Result<Option<StoreMode>, String> {
    let var = env_var_name(provider);
    let mode = if raw.is_empty() {
        None
    } else if raw.starts_with('$') || raw.starts_with('!') {
        Some(StoreMode::Ref)
    } else {
        match vault.set(&var, raw) {
            Ok(()) => Some(StoreMode::Vault),
            Err(_) => Some(StoreMode::File),
        }
    };
    let map = entry
        .as_object_mut()
        .ok_or("provider entry is not an object")?;
    match mode {
        None => {
            map.remove("apiKey");
        }
        Some(StoreMode::Vault) => {
            map.insert("apiKey".into(), Value::String(format!("${var}")));
        }
        Some(_) => {
            map.insert("apiKey".into(), Value::String(raw.to_string()));
        }
    }
    Ok(mode)
}

/// 自定义 provider 改名时同步凭据库条目（vault get→set→delete，幂等）。
pub fn rename_provider_key(old: &str, new: &str, vault: &dyn SecretVault) {
    let (old_var, new_var) = (env_var_name(old), env_var_name(new));
    if old_var == new_var {
        return;
    }
    if let Ok(Some(secret)) = vault.get(&old_var) {
        if vault.set(&new_var, &secret).is_ok() {
            let _ = vault.delete(&old_var);
        }
    }
}

/// 扩展模板（纯函数，便于测试）。读同目录 providers.json 并逐个
/// registerProvider，逐条容错。
fn extension_template() -> String {
    format!(
        "// pi-flash provider injection — GENERATED by pf_providers::ensure_extension_template ({EXT_TEMPLATE_VERSION}), edits will be overwritten.\n\
         // PF never writes pi's models.json: custom providers live in ./providers.json\n\
         // (next to this file) and are registered in-process via the official\n\
         // extension API. See docs/模块设计/051-apikey管理.md.\n\
         import {{ readFileSync }} from \"node:fs\";\n\
         const providers = (() => {{\n\
         \x20   try {{\n\
         \x20       const raw = JSON.parse(readFileSync(new URL(\"./providers.json\", import.meta.url), \"utf8\"));\n\
         \x20       return raw.providers ?? {{}};\n\
         \x20   }} catch {{\n\
         \x20       return {{}};\n\
         \x20   }}\n\
         }})();\n\
         export default function (pi) {{\n\
         \x20   for (const [id, cfg] of Object.entries(providers)) {{\n\
         \x20       try {{\n\
         \x20           pi.registerProvider(id, cfg);\n\
         \x20       }} catch (e) {{\n\
         \x20           console.error(`[pi-flash] registerProvider(${{id}}) failed:`, e?.message ?? e);\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n"
    )
}

/// spawn 前确保 `-e` 扩展存在且为当前版本（缺失/版本变化 → 重写）。
pub fn ensure_extension_template() -> Result<(), String> {
    let Some(ext) = ext_path() else {
        return Err("pf providers dir unavailable".into());
    };
    let template = extension_template();
    let needs_write = match std::fs::read_to_string(&ext) {
        Ok(existing) => existing != template,
        Err(_) => true,
    };
    if needs_write {
        if let Some(parent) = ext.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&ext, template).map_err(|e| format!("write extension template: {e}"))?;
    }
    Ok(())
}

/// 判断 pi models.json 里的条目是不是"真自定义内容"（值得复制）：
/// 有 baseUrl，或有非空 id 的模型。apiKey-only 的裸条目是旧版 catch-all
/// 写进去的垃圾（其 key 已在 pf-auth/凭据库），跳过。
fn is_custom_content(entry: &Value) -> bool {
    if entry.get("baseUrl").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()) {
        return true;
    }
    entry
        .get("models")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| {
            arr.iter()
                .any(|m| m.get("id").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()))
        })
}

/// 一次性复制迁移（051 M1.1）：pi models.json → PF 账本。**复制不删**，
/// pi 文件字节不动；账本已有同名条目跳过（PF 管理优先）；apiKey-only 垃圾
/// 条目跳过；复制进来的明文 key 立即凭据库化。标记文件防重跑。
pub fn adopt_from_models_json_at(
    ledger_path: &Path,
    pi_models_path: &Path,
    marker_path: &Path,
    vault: &dyn SecretVault,
) -> Result<usize, String> {
    if marker_path.exists() || !pi_models_path.exists() {
        return Ok(0);
    }
    let pi_doc = read_doc(pi_models_path)?;
    let mut ledger = read_at(ledger_path)?;
    let mut copied = 0;
    for (name, entry) in crate::models_json::providers(&pi_doc) {
        if !is_custom_content(entry) {
            continue;
        }
        if crate::models_json::provider_entry(&ledger, &name).is_some() {
            continue; // PF 已有同名账本，不覆盖
        }
        let mut copy = entry.clone();
        // 复制进来的明文 key 立即迁凭据库（自己的文件，可放心引用化）
        if let Some(key) = copy.get("apiKey").and_then(|v| v.as_str()).map(str::to_string) {
            if classify(&key) == StoreMode::File {
                let var = env_var_name(&name);
                if vault.set(&var, &key).is_ok() {
                    copy.as_object_mut().unwrap()
                        .insert("apiKey".into(), json!(format!("${var}")));
                }
            }
        }
        ledger
            .as_object_mut()
            .ok_or("providers.json is not an object")?
            .entry("providers")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("providers is not an object")?
            .insert(name, copy);
        copied += 1;
    }
    if copied > 0 {
        write_at(ledger_path, &ledger)?;
    }
    std::fs::write(marker_path, EXT_TEMPLATE_VERSION).map_err(|e| format!("write marker: {e}"))?;
    Ok(copied)
}

/// 引用形态换明文的自愈不用做：pi models.json 是只读的，PF 不再往里写引用。
/// 保留 pf_ref 在此模块的用途：spawn_env 扫账本引用（见 credentials.rs）。
#[allow(dead_code)]
pub(crate) fn is_pf_ref(value: &str) -> bool {
    pf_ref(value).is_some()
}

// -- 便捷封装（生产路径；*_at 变体供测试注路径） ------------------------------

pub fn ensure_extension_template_or_warn() {
    if let Err(e) = ensure_extension_template() {
        eprintln!("[startup] pf-providers extension template skipped: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemVault;
    use serde_json::Value;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pf-providers-{}-{}.json", tag, std::process::id()))
    }

    fn clean(p: &Path) {
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn sanitize_drops_empty_id_models_and_empty_providers() {
        let mut doc = json!({
            "providers": {
                "glm": { "apiKey": "$PF_KEY_X" }, // catch-all 垃圾：无内容 → 丢
                "gw": {
                    "baseUrl": "https://x/v1",
                    "models": [
                        { "id": "" },
                        { "id": "m-1" },
                        { "name": "no id here" }
                    ]
                },
                "blank": {}
            }
        });
        sanitize(&mut doc);
        let providers = doc.get("providers").unwrap().as_object().unwrap();
        assert!(!providers.contains_key("glm"), "apiKey-only 垃圾应丢弃");
        assert!(!providers.contains_key("blank"), "空 provider 应丢弃");
        let models = providers["gw"]["models"].as_array().unwrap();
        assert_eq!(models.len(), 1, "空 id 模型条目应被净化");
        assert_eq!(models[0]["id"], "m-1");
    }

    #[test]
    fn write_at_never_persists_empty_ids() {
        let p = tmp("write");
        clean(&p);
        let doc = json!({
            "providers": { "gw": { "baseUrl": "https://x/v1", "models": [ { "id": "" } ] } }
        });
        write_at(&p, &doc).unwrap();
        let raw = std::fs::read_to_string(&p).unwrap();
        let back: Value = serde_json::from_str(&raw).unwrap();
        assert!(back.pointer("/providers/gw/models").is_none(), "空 id 条目不得落盘");
        assert_eq!(back.pointer("/providers/gw/baseUrl").unwrap(), "https://x/v1");
        clean(&p);
    }

    #[test]
    fn set_provider_key_routes_and_degrades() {
        let v = MemVault::default();
        let mut entry = json!({});
        // 明文 → 凭据库 + 引用
        let mode = set_provider_key(&mut entry, "my-gw", "sk-secret", &v).unwrap();
        assert_eq!(mode, Some(StoreMode::Vault));
        assert_eq!(entry["apiKey"], format!("${}", env_var_name("my-gw")));
        assert_eq!(v.get(&env_var_name("my-gw")).unwrap().as_deref(), Some("sk-secret"));
        // 高级引用原样
        let mode = set_provider_key(&mut entry, "my-gw", "!cmd", &v).unwrap();
        assert_eq!(mode, Some(StoreMode::Ref));
        assert_eq!(entry["apiKey"], "!cmd");
        // 降级
        let mode = set_provider_key(&mut entry, "my-gw", "sk-2", &crate::credentials::FailVault).unwrap();
        assert_eq!(mode, Some(StoreMode::File));
        assert_eq!(entry["apiKey"], "sk-2");
        // 空串清除
        let mode = set_provider_key(&mut entry, "my-gw", "", &v).unwrap();
        assert_eq!(mode, None);
        assert!(entry.get("apiKey").is_none());
    }

    #[test]
    fn adopt_copies_custom_only_and_never_touches_pi_file() {
        let ledger = tmp("adopt-ledger");
        let pi_models = tmp("adopt-pi");
        let marker = tmp("adopt-marker");
        clean(&ledger);
        clean(&pi_models);
        clean(&marker);
        std::fs::write(
            &pi_models,
            r#"{
                "providers": {
                    "my-gw": { "baseUrl": "https://gw/v1", "api": "openai-completions", "apiKey": "sk-plain",
                               "models": [ { "id": "m-1" } ] },
                    "zai-coding-cn": { "apiKey": "$PF_KEY_ZAI_CODING_CN" },
                    "real-empty": { "models": [ { "id": "" } ] }
                }
            }"#,
        )
        .unwrap();
        let before = std::fs::read_to_string(&pi_models).unwrap();
        let v = MemVault::default();
        let n = adopt_from_models_json_at(&ledger, &pi_models, &marker, &v).unwrap();
        assert_eq!(n, 1, "只复制有 baseUrl/非空 id 模型的真自定义；apiKey-only 与空壳跳过");
        // pi 文件字节不动
        assert_eq!(std::fs::read_to_string(&pi_models).unwrap(), before, "models.json 必须字节不变");
        // 复制进来的明文 key 已凭据库化
        let doc = read_at(&ledger).unwrap();
        let var = env_var_name("my-gw");
        assert_eq!(doc.pointer("/providers/my-gw/apiKey").unwrap(), &json!(format!("${var}")));
        assert_eq!(v.get(&var).unwrap().as_deref(), Some("sk-plain"));
        assert!(!doc.pointer("/providers/zai-coding-cn").is_some());
        // 幂等：marker 生效
        assert_eq!(adopt_from_models_json_at(&ledger, &pi_models, &marker, &v).unwrap(), 0);
        clean(&ledger);
        clean(&pi_models);
        clean(&marker);
    }

    #[test]
    fn extension_template_registers_from_sibling_json() {
        let t = extension_template();
        assert!(t.contains("registerProvider(id, cfg)"), "逐个注册");
        assert!(t.contains("\"./providers.json\""), "从同目录账本读配置");
        assert!(t.contains("export default function"), "扩展工厂默认导出");
        assert_eq!(extension_template(), t, "模板内容稳定（版本化重写依赖一致性比较）");
    }
}
