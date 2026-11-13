//! 文件级写层：读-改-写（只替换本 section）+ 原子落盘 + 同内容跳过 +
//! 首写整份升级（001-1 §3 写迁移）。
//!
//! 为什么必须读-改-写（001-1 §2）：整份重建会静默抹掉用户手工加的未知键、
//! 未来版本写入的 section、其它分支实验字段——001-0 §1 点名的存量缺陷，
//! 本层是它的根治点（单测：写入前后未知键逐字相同）。

use serde_json::{Map, Value};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use super::section::deep_merge;
use super::{Snapshot, sections};

/// 把一个 section 的规范 JSON 落盘（读-改-写），返回是否真的写了。
///
/// - 只替换 `key` 指向的 section：盘上其它键/section（含未知键）原样保留；
/// - section 内既有未知字段透传（typed 值深合并覆盖已知字段）；
/// - section 的 legacy 平铺键同值回写（001-1 §3 写迁移，旧键保留一版）；
/// - **整份升级**：盘上 `version != 2` 时（v1 平铺文档），任一写触发
///   `version: 2` + 全部 in-file section 落位 + 全部 legacy 平铺键保留一版；
/// - 与盘上内容一致则跳过写（少一次 IO + watcher 回声）；
/// - 盘上是坏档时以内存为准重建（坏内容无从保留，读侧有 problem 记录）。
pub(crate) fn write_section(path: &Path, key: &str, snap: &Snapshot) -> Result<bool, String> {
    let section_json = snap
        .section_json
        .get(key)
        .ok_or_else(|| format!("{key} 不在快照"))?;
    let mut map = match super::source::read_doc(path) {
        super::source::Doc::Ok(map) => map,
        super::source::Doc::Missing | super::source::Doc::Broken => Map::new(),
    };

    let full_upgrade = map.get("version").and_then(Value::as_u64) != Some(2);
    if full_upgrade {
        // v1 → v2：全部 section 落位 + 全部 legacy 平铺键保留一版（回滚兼容）
        for def in sections::registered() {
            if !def.in_file {
                continue; // boot-only 段（dev_flags）永不进 app-settings.json
            }
            let Some(json) = snap.section_json.get(def.key) else {
                continue;
            };
            upsert_section(&mut map, def.key, json);
            sections::write_legacy(def.key, json, &mut map);
        }
        map.insert("version".into(), Value::from(2));
    } else {
        upsert_section(&mut map, key, section_json);
        sections::write_legacy(key, section_json, &mut map);
    }

    let value = Value::Object(map);
    let text = serde_json::to_string_pretty(&value).map_err(|e| format!("序列化失败: {e}"))?;
    if std::fs::read_to_string(path).is_ok_and(|cur| cur == text) {
        return Ok(false);
    }
    write_json_no_mkdir(path, &value)?;
    Ok(true)
}

/// 原子写（同目录 tmp + rename），**绝不创建父目录**。
///
/// 与 `pi_link::config::write_json` 的唯一差别就是这条：那个函数自带
/// `create_dir_all(parent)`，因此「目录已被删除之后的一次迟到写」会把整棵
/// 目录树重新建出来——测试收尾删掉的临时目录、用户刚移走的数据目录都会被
/// 复活。数据目录的生命周期归启动期（`Hub::open` / `paths::ensure_dir`），
/// 写路径只负责往里写。
///
/// tmp 名带 pid + 进程内自增序号：并发写同一目标（后台投影线程 vs flush）
/// 不再共用同一个 `.json.tmp` 互相 rename 抢跑（Windows 上表现为 os error 2）。
/// 失败时清掉 tmp，不留垃圾。
pub(crate) fn write_json_no_mkdir(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "路径无父目录".to_string())?;
    if !parent.is_dir() {
        return Err(format!("目录不存在（不重建）: {}", parent.display()));
    }
    static TMP_NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("doc.json");
    let tmp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        TMP_NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("序列化失败: {e}"))?;
    if let Err(e) = std::fs::write(&tmp, text) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// 原地深合并 upsert（不用 remove+insert——preserve_order 下那会把键挪到
/// 末尾，破坏同内容跳过的字节级比对；get_mut 保键位不动）。
fn upsert_section(map: &mut Map<String, Value>, key: &str, section_json: &Value) {
    match map.get_mut(key) {
        Some(slot) if slot.is_object() => deep_merge(slot, section_json),
        // 既有标量/垃圾（或本来就缺）：整段以 typed 值替换
        _ => {
            map.insert(key.to_string(), section_json.clone());
        }
    }
}
