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
            .map(|(k, v)| {
                (
                    k.clone(),
                    Value::Array(v.iter().map(|s| Value::String(s.clone())).collect()),
                )
            })
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

/// 路径型挂载条目（PF 插件 `pi_extensions` 绑定通道写入的绝对路径，契约 v3）。
/// 台账是**混合通道**：用户勾选的 scheme 源（`npm:`/`git:`…）+ 插件挂载的
/// 路径条目。勾选面板宇宙只含 scheme 源——路径条目不显示、不计数，但落盘
/// 时必须保住（丢 = 下轮 spawn 插件 ext 不挂）。
pub fn is_mount_path(src: &str) -> bool {
    std::path::Path::new(src).is_absolute()
}

/// 用户勾选清单 + 既有路径型条目合并（保序去重）：`user` 为基准，
/// `existing` 里的路径条目原样保留，scheme 条目以 `user` 为准。
pub fn with_mounts_preserved(existing: &[String], user: &[String]) -> Vec<String> {
    let mut out = user.to_vec();
    for src in existing {
        if is_mount_path(src) && !out.contains(src) {
            out.push(src.clone());
        }
    }
    out
}

/// [`with_mounts_preserved`] + 落盘一步到位（勾选面板/重绑共用的写入口）。
pub fn write_for_preserving_mounts(path: &Path, key: &str, user: &[String]) -> Result<(), String> {
    let existing = read_for(path, key);
    write_for(path, key, &with_mounts_preserved(&existing, user))
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
        assert_eq!(
            read_for(&p, "C:/Users/bi_da/.pi/agent/sessions/x.jsonl"),
            vec!["npm:a"]
        );
        // 反向：写正斜杠、读出反斜杠 + 盘符小写
        write_for(&p, "C:/tmp/y.jsonl", &["npm:b".into()]).unwrap();
        let win2 = format!("c:\\tmp\\y.jsonl");
        assert_eq!(read_for(&p, &win2), vec!["npm:b"]);
    }

    /// 插件挂载通道（真机 2026-10-31：勾 2 显 [3]）——scheme 源以勾选为准，
    /// 路径型条目跨勾选保留；路径判定与平台绝对路径口径一致。
    #[test]
    fn mount_paths_survive_user_pick_rewrites() {
        let mount = std::env::temp_dir()
            .join("plugins/wechat-bridge/pi/index.mjs")
            .to_string_lossy()
            .into_owned();
        assert!(is_mount_path(&mount), "绝对路径 = 挂载条目");
        assert!(!is_mount_path("npm:pi-freeflow"));
        assert!(!is_mount_path("git:https://github.com/u/r"));

        let existing = vec!["npm:old".into(), mount.clone()];
        // 用户取消勾了 npm:old、新勾 npm:new——old 该走就走，mount 必须留下
        let merged = with_mounts_preserved(&existing, &["npm:new".to_string()]);
        assert_eq!(merged, vec!["npm:new".to_string(), mount.clone()]);

        // 幂等：以合并结果为 existing 再合并不增生
        let again = with_mounts_preserved(&merged, &["npm:new".to_string()]);
        assert_eq!(again, merged);
    }

    #[test]
    fn write_preserving_mounts_roundtrip() {
        let p = tmp("preserve.json");
        let _ = std::fs::remove_file(&p);
        let mount = std::env::temp_dir()
            .join("plugins/pf-wxbot/pi/index.mjs")
            .to_string_lossy()
            .into_owned();
        // 先按插件挂载通道的形态落一份（历史 attach 曾整表覆写：台账只剩路径）
        write_for(&p, "k", &[mount.clone()]).unwrap();
        // 用户勾选落盘：mount 保留，npm 勾选进来
        write_for_preserving_mounts(&p, "k", &["npm:pi-freeflow".into()]).unwrap();
        assert_eq!(
            read_for(&p, "k"),
            vec!["npm:pi-freeflow".to_string(), mount]
        );
    }

    #[test]
    fn broken_file_degrades_to_empty() {
        let p = tmp("broken.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(read(&p).is_empty());
    }
}

/// 迁移清理（90-6）：把某插件的执行域入口从**所有**会话的扩展清单里摘掉。
///
/// 契约 v4 起，插件 `pi/index.mjs`（以及清单 contributes.service 指的任何
/// `pi/*.mjs`）是执行域入口，不再是可 `-e` 挂载的 pi 扩展；v3 时代写下的
/// 会话清单还指着它 —— pi 在 spawn 时直接报
/// `does not export a valid factory function`，同批 `-e`（含执行域 shim）
/// 一起失效：插件功能面全哑（2026-10-30 真机根因）。
///
/// 按**插件目录**匹配（`…/plugins/<id>/pi/…`）而非整条路径相等：数据目录被
/// 搬迁/复制后旧清单里的绝对路径可能与当前插件目录不同，仍要摘掉。
/// 幂等：没有残留就不写盘。
pub fn purge_exec_entries(plugin_id: &str) -> Result<usize, String> {
    let Some(store) = store_path() else {
        return Ok(0);
    };
    if !store.is_file() {
        return Ok(0);
    }
    let needle = format!("/plugins/{}/pi/", plugin_id.to_ascii_lowercase());
    let mut table = read(&store);
    let mut removed = 0usize;
    for list in table.values_mut() {
        let before = list.len();
        list.retain(|p| !norm_key(p).to_ascii_lowercase().contains(&needle));
        removed += before - list.len();
    }
    if removed > 0 {
        table.retain(|_, v| !v.is_empty());
        write_all(&store, &table)?;
    }
    Ok(removed)
}
