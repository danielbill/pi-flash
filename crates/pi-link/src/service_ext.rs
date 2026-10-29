//! 插件执行域（契约 v4 contributes.service，90-6）：锚点注册表 + 实例配置 + 信箱。
//! 散文版：PF 仓 `docs/模块设计/90-插件系统/90-6-插件执行域-plugin-context.md`。
//!
//! 三个宿主托管事实，插件（含宿主自带 shim）只读不猜：
//! - **锚点** `<token>/<plugin_id>`：token 会话一次铸造、终身不变（注册表持久
//!   化；草稿→会话文件键迁移时整条搬家）——插件把绑定指针存在锚点上，跨迁移、
//!   跨重启稳定（90-6 §2 身份条款）；
//! - **实例配置** `<base>/pf-svc/sessions/<token>.json`：spawn 前宿主写全
//!   （instances + global_dir），经 `PF_SVC_CONFIG` 环境指针交给宿主自带 shim
//!   （[`shim_path`]）；token 不随池键迁移 → 配置路径对运行中进程恒定；
//! - **信箱** `<base>/pf-svc/sessions/<token>-in.jsonl` / `-out.jsonl`：
//!   宿主单写者 in、shim 单写者 out，消费即截断。单写者由文件所有权结构
//!   保证，读-改-写竞态无处滋生（90-6 §5 通道职责）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::session_ext::norm_key;

/// 执行域根（`<base>/pf-svc/`）。
pub fn svc_root(base: &Path) -> PathBuf {
    base.join("pf-svc")
}

/// 全局数据目录（`paths::dir()` 便捷包装；即交付给执行面的 `globalDir`）。
pub fn base() -> Option<PathBuf> {
    crate::paths::dir()
}

fn registry_path(base: &Path) -> PathBuf {
    svc_root(base).join("registry.json")
}

fn registry_read(base: &Path) -> BTreeMap<String, String> {
    let text = match std::fs::read_to_string(registry_path(base)) {
        Ok(t) => t,
        Err(_) => return BTreeMap::new(),
    };
    let Ok(Value::Object(map)) = crate::config::parse_lenient(&text) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| {
            let t = v.as_str()?.to_string();
            Some((norm_key(&k), t))
        })
        .collect()
}

fn registry_write(base: &Path, table: &BTreeMap<String, String>) -> Result<(), String> {
    let value = Value::Object(
        table
            .iter()
            .map(|(k, t)| (k.clone(), Value::String(t.clone())))
            .collect(),
    );
    crate::config::write_json(&registry_path(base), &value)
}

fn mint_token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let h = n ^ ((std::process::id() as u64) << 32) ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("s-{h:016x}")
}

/// 该会话的 token（无则铸造并持久化）。随会话创建/实例配置写入时调用。
pub fn token_for(base: &Path, key: &str) -> Option<String> {
    let mut table = registry_read(base);
    let nk = norm_key(key);
    if let Some(t) = table.get(&nk) {
        return Some(t.clone());
    }
    let t = mint_token();
    table.insert(nk, t.clone());
    registry_write(base, &table).ok()?;
    Some(t)
}

/// 只查不铸（spawn 侧判断「有没有实例配置」用，避免无服务会话污染注册表）。
pub fn token_if_any(base: &Path, key: &str) -> Option<String> {
    registry_read(base).get(&norm_key(key)).cloned()
}

/// 会话键迁移（草稿 → 会话文件路径）：注册表条目整条搬家，token 不变。
pub fn move_key(base: &Path, old: &str, new: &str) -> Result<(), String> {
    let mut table = registry_read(base);
    let (o, n) = (norm_key(old), norm_key(new));
    let Some(t) = table.remove(&o) else {
        return Ok(()); // 无条目 = 该会话无执行域，无事
    };
    table.insert(n, t);
    registry_write(base, &table)
}

/// 会话删除：注册表条目 + 信箱 + 配置一并回收（锚点随之消失，90-6 §6）。
pub fn remove_key(base: &Path, key: &str) -> Result<(), String> {
    let mut table = registry_read(base);
    let nk = norm_key(key);
    let Some(t) = table.remove(&nk) else {
        return Ok(());
    };
    registry_write(base, &table)?;
    for f in [config_path(base, &t), in_path(base, &t), out_path(base, &t)] {
        let _ = std::fs::remove_file(f);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 实例配置 + 信箱
// ---------------------------------------------------------------------------

/// 单实例声明（Chat 侧收集；anchor/信箱路径由本模块按 token 填充）。
pub struct SvcInstanceSpec<'a> {
    pub id: &'a str,
    /// 执行面入口绝对路径（包根 + 清单 entry）
    pub entry: &'a Path,
    /// 执行面私有存储根（`<pkg>/data/svc/`）
    pub dir: &'a Path,
}

pub fn sessions_dir(base: &Path) -> PathBuf {
    svc_root(base).join("sessions")
}

pub fn config_path(base: &Path, token: &str) -> PathBuf {
    sessions_dir(base).join(format!("{token}.json"))
}

pub fn in_path(base: &Path, token: &str) -> PathBuf {
    sessions_dir(base).join(format!("{token}-in.jsonl"))
}

pub fn out_path(base: &Path, token: &str) -> PathBuf {
    sessions_dir(base).join(format!("{token}-out.jsonl"))
}

/// spawn 侧：该会话的实例配置路径（未铸造过 = None，不铸）。
pub fn config_path_if_any(base: &Path, key: &str) -> Option<PathBuf> {
    token_if_any(base, key).map(|t| config_path(base, &t))
}

/// 写某会话的实例配置（spawn 前调用；目录顺手建好）。
pub fn write_session_config(
    base: &Path,
    token: &str,
    instances: &[SvcInstanceSpec<'_>],
) -> Result<PathBuf, String> {
    let sessions = sessions_dir(base);
    std::fs::create_dir_all(&sessions).map_err(|e| e.to_string())?;
    let list: Vec<Value> = instances
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "anchor": format!("{token}/{}", i.id),
                "entry": i.entry.to_string_lossy(),
                "dir": i.dir.to_string_lossy(),
                "inbox": in_path(base, token).to_string_lossy(),
                "outbox": out_path(base, token).to_string_lossy(),
            })
        })
        .collect();
    let value = json!({
        "token": token,
        "global_dir": base.to_string_lossy(),
        "instances": list,
    });
    let path = config_path(base, token);
    crate::config::write_json(&path, &value)?;
    for i in instances {
        let _ = std::fs::create_dir_all(i.dir);
    }
    Ok(path)
}

/// 删某会话的实例配置（实例集清空时；信箱不删——残留事件随消费自然排空）。
pub fn remove_session_config(base: &Path, token: &str) {
    let _ = std::fs::remove_file(config_path(base, token));
}

/// 追加一条信箱事件（单写者 append；JSONL）。
pub fn append_event(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{value}").map_err(|e| e.to_string())
}

/// 消费信箱（单读者：整读后截断）。坏行跳过。
pub fn consume_events(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let _ = std::fs::write(path, "");
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// spawn 时清空双方信箱残留：上一轮生命周期已消费的事件不得重放（至多一次）。
pub fn reset_spools(base: &Path, token: &str) {
    let _ = std::fs::write(in_path(base, token), "");
    let _ = std::fs::write(out_path(base, token), "");
}

// ---------------------------------------------------------------------------
// 宿主自带 shim（`-e` 挂载；定位策略与 subagents 同款）
// ---------------------------------------------------------------------------

const SHIM_REL: &str = "extensions/pf-svc-runtime/index.mjs";

/// 执行域 shim 入口（缺失 = 本构建不带货，service 声明落空并告警）。
pub fn shim_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("PI_FLASH_SVC_SHIM") {
        let p = PathBuf::from(explicit);
        return p.is_file().then_some(p);
    }
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.to_path_buf());
            if let Some(up) = dir.parent() {
                out.push(up.to_path_buf());
                if let Some(root) = up.parent() {
                    out.push(root.to_path_buf());
                }
            }
        }
    }
    if let Some(vendor) = crate::vendor::vendor_dir() {
        if let Some(root) = vendor.parent().and_then(Path::parent) {
            out.push(root.to_path_buf());
        }
    }
    out.iter().map(|b| b.join(SHIM_REL)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pf-svc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn token_stable_across_move_and_restart() {
        let b = tmp("t1");
        let t1 = token_for(&b, "draft-1").unwrap();
        let t2 = token_for(&b, "draft-1").unwrap();
        assert_eq!(t1, t2, "同键同 token");
        move_key(&b, "draft-1", "C:/s/a.jsonl").unwrap();
        assert_eq!(
            token_if_any(&b, "C:/s/a.jsonl").as_deref(),
            Some(t1.as_str()),
            "迁移后 token 不变"
        );
        assert!(token_if_any(&b, "draft-1").is_none());
    }

    #[test]
    fn config_roundtrip_and_spool_consume_truncates() {
        let b = tmp("t2");
        let t = token_for(&b, "C:/s/x.jsonl").unwrap();
        let dir = b.join("pkg").join("data").join("svc");
        let specs = [SvcInstanceSpec {
            id: "demo",
            entry: Path::new("/x/entry.mjs"),
            dir: &dir,
        }];
        let cfg = write_session_config(&b, &t, &specs).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(v["instances"][0]["anchor"], format!("{t}/demo"));
        assert!(v["global_dir"].is_string());

        let inp = in_path(&b, &t);
        append_event(&inp, &json!({"id": "demo", "payload": 1})).unwrap();
        append_event(&inp, &json!({"id": "demo", "payload": 2})).unwrap();
        let got = consume_events(&inp);
        assert_eq!(got.len(), 2);
        assert!(consume_events(&inp).is_empty(), "消费即截断");
    }

    #[test]
    fn remove_key_cleans_files() {
        let b = tmp("t3");
        let t = token_for(&b, "C:/s/y.jsonl").unwrap();
        let _ = write_session_config(
            &b,
            &t,
            &[SvcInstanceSpec {
                id: "d",
                entry: Path::new("/e"),
                dir: &b.join("d"),
            }],
        );
        remove_key(&b, "C:/s/y.jsonl").unwrap();
        assert!(!config_path(&b, &t).exists());
        assert!(!in_path(&b, &t).exists());
    }
}
