//! 会话级插件清单（031/034：full+plugins「自定义」档按会话保存一份）。
//!
//! 落 `~/.pi-flash/session-ext.json`（pi-flash 自有文件，不碰 pi 的会话文件）：
//!
//! ```json
//! { "<会话 key>": ["npm:pi-freeflow", "npm:pi-web-access"] }
//! ```
//!
//! key = `SessionRuntime::key`：真实会话 = 会话文件绝对路径；草稿 = `draft-N`
//! （草稿不跨重启，写进去也无害，重开应用后自然失效）。
//!
//! 只存「自定义清单」本身，不存工具预设 —— 预设重启回落默认档（自定义）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// 会话键归一化（Windows 上同一份会话文件有两种写法：app 的 `session.open`
/// 给正斜杠、pi 的 `get_state.sessionFile` 给反斜杠；盘符大小写也不固定）。
/// 台账两侧（读/写/删）都走这里，保证「自定义」清单跨重启对得上。
pub fn norm_key(key: &str) -> String {
    let s = key.trim().replace('\\', "/");
    let mut chars: Vec<char> = s.chars().collect();
    if chars.len() > 1 && chars[1] == ':' {
        chars[0] = chars[0].to_ascii_lowercase();
    }
    chars.into_iter().collect()
}

/// 读取整张表（文件缺失/损坏都当空表：清单丢了最坏只是回落到空选择）。
pub fn read(path: &Path) -> BTreeMap<String, Vec<String>> {
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => return BTreeMap::new(),
    };
    let Ok(Value::Object(map)) = crate::config::parse_lenient(&text) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| {
            let list: Vec<String> = v
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            Some((norm_key(&k), list))
        })
        .collect()
}

/// 某个会话的清单（空表/无此会话 → 空 vec）。
pub fn read_for(path: &Path, key: &str) -> Vec<String> {
    read(path).remove(&norm_key(key)).unwrap_or_default()
}

/// 写入某个会话的清单（空清单 = 删掉该条目，不落空数组）。
pub fn write_for(path: &Path, key: &str, sources: &[String]) -> Result<(), String> {
    let mut table = read(path);
    let key = norm_key(key);
    if sources.is_empty() {
        table.remove(&key);
    } else {
        table.insert(key, sources.to_vec());
    }
    write_all(path, &table)
}

/// 整表写盘（原子替换；台账小，直接整写）。
pub fn write_all(path: &Path, table: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    let value = Value::Object(
        table
            .iter()
            .map(|(k, v)| (k.clone(), Value::Array(v.iter().map(|s| Value::String(s.clone())).collect())))
            .collect(),
    );
    crate::config::write_json(path, &value)
}

/// 清掉某个会话的条目（会话被删除时用）。
pub fn remove_for(path: &Path, key: &str) -> Result<(), String> {
    let mut table = read(path);
    if table.remove(&norm_key(key)).is_none() {
        return Ok(());
    }
    write_all(path, &table)
}

/// 便捷取路径（`~/.pi-flash/session-ext.json`）。
pub fn store_path() -> Option<PathBuf> {
    crate::paths::session_ext_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-flash-sessext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn roundtrip_per_conversation() {
        let p = tmp("roundtrip.json");
        let _ = std::fs::remove_file(&p);
        write_for(&p, "C:/s/a.jsonl", &["npm:x".into(), "npm:y".into()]).unwrap();
        write_for(&p, "C:/s/b.jsonl", &["npm:z".into()]).unwrap();
        assert_eq!(read_for(&p, "C:/s/a.jsonl"), vec!["npm:x", "npm:y"]);
        assert_eq!(read_for(&p, "C:/s/b.jsonl"), vec!["npm:z"]);
        assert!(read_for(&p, "missing").is_empty());
    }

    #[test]
    fn empty_list_removes_entry_and_missing_file_is_empty() {
        let p = tmp("empty.json");
        let _ = std::fs::remove_file(&p);
        assert!(read(&p).is_empty(), "文件缺失 = 空表");
        write_for(&p, "k", &["npm:a".into()]).unwrap();
        write_for(&p, "k", &[]).unwrap();
        assert!(read(&p).is_empty(), "空清单不落空数组");
        assert!(remove_for(&p, "nope").is_ok());
    }

    #[test]
    fn key_normalization_matches_both_windows_spellings() {
        let p = tmp("norm.json");
        let _ = std::fs::remove_file(&p);
        // 写入 pi 的反斜杠形态（get_state.sessionFile），读出用 app 的正斜杠形态
        let win = format!("C:\\Users\\bi_da\\.pi\\agent\\sessions\\x.jsonl");
        write_for(&p, &win, &["npm:a".into()]).unwrap();
        assert_eq!(read_for(&p, "C:/Users/bi_da/.pi/agent/sessions/x.jsonl"), vec!["npm:a"]);
        // 反向：写正斜杠、读出反斜杠 + 盘符小写
        write_for(&p, "C:/tmp/y.jsonl", &["npm:b".into()]).unwrap();
        let win2 = format!("c:\\tmp\\y.jsonl");
        assert_eq!(read_for(&p, &win2), vec!["npm:b"]);
    }

    #[test]
    fn broken_file_degrades_to_empty() {
        let p = tmp("broken.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(read(&p).is_empty());
    }
}
