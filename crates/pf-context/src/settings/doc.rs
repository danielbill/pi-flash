//! 文件级写层：读-改-写（只替换本 section）+ 原子落盘 + 同内容跳过。
//!
//! 为什么必须读-改-写（001-1 §2）：整份重建会静默抹掉用户手工加的未知键、
//! 未来版本写入的 section、其它分支实验字段——001-0 §1 点名的存量缺陷，
//! 本层是它的根治点（单测：写入前后未知键逐字相同）。

use serde_json::{Map, Value};
use std::path::Path;

use super::section::deep_merge;
use super::sections;

/// 把一个 section 的规范 JSON 落盘（读-改-写），返回是否真的写了。
///
/// - 只替换 `key` 指向的 section：盘上其它键/section（含未知键）原样保留；
/// - section 内既有未知字段透传（typed 值深合并覆盖已知字段）；
/// - section 的 legacy 平铺键同值回写（001-1 §3 写迁移，旧键保留一版）；
/// - 与盘上内容一致则跳过写（少一次 IO + watcher 回声）；
/// - 盘上是坏档时以内存为准重建（坏内容无从保留，读侧有 problem 记录）。
pub(crate) fn write_section(path: &Path, key: &str, section_json: &Value) -> Result<bool, String> {
    let mut map = match super::source::read_doc(path) {
        super::source::Doc::Ok(map) => map,
        super::source::Doc::Missing | super::source::Doc::Broken => Map::new(),
    };

    // 原地深合并（不用 remove+insert——preserve_order 下那会把键挪到末尾，
    // 破坏同内容跳过的字节级比对；get_mut 保键位不动）
    match map.get_mut(key) {
        Some(slot) if slot.is_object() => deep_merge(slot, section_json),
        // 既有标量/垃圾（或本来就缺）：整段以 typed 值替换
        _ => {
            map.insert(key.to_string(), section_json.clone());
        }
    }
    sections::write_legacy(key, section_json, &mut map);

    let value = Value::Object(map);
    let text = serde_json::to_string_pretty(&value).map_err(|e| format!("序列化失败: {e}"))?;
    if std::fs::read_to_string(path).is_ok_and(|cur| cur == text) {
        return Ok(false);
    }
    // tmp + rename 原子写（pi-link::config 与 workspace 存储层同一条管线）
    pi_link::config::write_json(path, &value)?;
    Ok(true)
}
