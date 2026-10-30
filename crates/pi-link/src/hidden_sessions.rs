//! 会话过滤清单（PF 自有）：让"影子会话"**不进 PF 的会话列表**。
//!
//! 文件：`<PF 数据目录>/hidden-sessions.json`
//! ```json
//! { "entries": [ { "owner": "pf-demo", "name": "pf-demo-261024164500123" } ] }
//! ```
//!
//! 谁写：拥有影子会话的插件自己——认领线建线/重连时**注入一条**自己的
//! 条目（按 `owner` 整体替换，不累积；owner = 插件 id）。PF 不内置任何
//! 插件身份，条目内容全部由插件侧自报。
//!
//! 为什么不让 pi 知道：这是**显示口径**而不是存储口径——会话文件照旧躺在
//! pi 的 sessions 目录里（认领线要能被 pi 打开/续写），只是 PF 的列表
//! （psp 会话侧栏 / 最近会话 / 项目分组）按本清单过滤掉。
//!
//! 匹配口径三选一（任一命中即隐藏）：`path` 精确（大小写/分隔符归一）、
//! `name` 精确（pi 的 `session_info.name`）、`prefix` 前缀（同名族的旧会话
//! 也不进列表——认领线不复用旧会话、也不再删旧文件，靠前缀兜住历史）。
//! 另有一道不依赖本清单的静态铁闸：启动恢复按插件**清单声明**的认领
//! 前缀过滤（见 `pf_plugin::declared_claim_prefixes`）。

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::paths;

/// 一条过滤记录。`owner` 只用于插件自管（插件重连时整体替换自己的条目）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub owner: Option<String>,
    pub name: Option<String>,
    pub path: Option<String>,
    pub prefix: Option<String>,
}

/// 过滤清单快照。
#[derive(Debug, Clone, Default)]
pub struct Hidden {
    pub entries: Vec<Entry>,
}

impl Hidden {
    /// 这条会话该不该从列表里藏掉。
    pub fn hides(&self, name: Option<&str>, path: &Path) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let p = norm(path.to_string_lossy().as_ref());
        self.entries.iter().any(|e| {
            e.path.as_deref().is_some_and(|ep| norm(ep) == p)
                || matches!((e.name.as_deref(), name), (Some(a), Some(b)) if a == b)
                || matches!((e.prefix.as_deref(), name), (Some(pre), Some(b)) if b.starts_with(pre))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn norm(s: &str) -> String {
    s.replace('/', "\\").to_lowercase()
}

pub fn file_path() -> Option<PathBuf> {
    paths::dir().map(|d| d.join("hidden-sessions.json"))
}

/// 读清单（文件缺失/损坏 = 空清单，绝不阻断列表刷新）。
pub fn load() -> Hidden {
    let Some(p) = file_path() else {
        return Hidden::default();
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Hidden::default();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Hidden::default();
    };
    let entries = v["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| Entry {
                    owner: e["owner"].as_str().map(str::to_string),
                    name: e["name"].as_str().map(str::to_string),
                    path: e["path"].as_str().map(str::to_string),
                    prefix: e["prefix"].as_str().map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default();
    Hidden { entries }
}

pub fn save(h: &Hidden) {
    let Some(p) = file_path() else { return };
    let entries: Vec<Value> = h
        .entries
        .iter()
        .map(|e| {
            let mut m = serde_json::Map::new();
            if let Some(o) = &e.owner {
                m.insert("owner".into(), json!(o));
            }
            if let Some(n) = &e.name {
                m.insert("name".into(), json!(n));
            }
            if let Some(pa) = &e.path {
                m.insert("path".into(), json!(pa));
            }
            if let Some(pre) = &e.prefix {
                m.insert("prefix".into(), json!(pre));
            }
            Value::Object(m)
        })
        .collect();
    let value = json!({ "version": 1, "entries": entries });
    let _ = paths::ensure_dir();
    if let Err(err) = crate::config::write_json(&p, &value) {
        eprintln!("[hidden-sessions] 写清单失败 {}: {err}", p.display());
    }
}

/// 插件注入/替换自己的一条（`owner` 决定替换范围）。`prefix` 可选：同族
/// 旧会话（如历史残留）一并挡住。
pub fn set_owned(owner: &str, name: Option<&str>, path: Option<&Path>, prefix: Option<&str>) {
    let mut h = load();
    h.entries.retain(|e| e.owner.as_deref() != Some(owner));
    h.entries.push(Entry {
        owner: Some(owner.to_string()),
        name: name.map(str::to_string),
        path: path.map(|p| p.to_string_lossy().to_string()),
        prefix: prefix.map(str::to_string),
    });
    save(&h);
}

/// 插件收尾（例如认领线彻底停用）时撤掉自己的条目。
pub fn clear_owned(owner: &str) {
    let mut h = load();
    let before = h.entries.len();
    h.entries.retain(|e| e.owner.as_deref() != Some(owner));
    if h.entries.len() != before {
        save(&h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_name_and_normalized_path() {
        let h = Hidden {
            entries: vec![
                Entry {
                    owner: Some("pf-demo".into()),
                    name: Some("pf-demo-1".into()),
                    path: None,
                    prefix: None,
                },
                Entry {
                    owner: None,
                    name: None,
                    path: Some("C:/Users/x/SESSIONS/a.jsonl".into()),
                    prefix: None,
                },
                Entry {
                    owner: None,
                    name: None,
                    path: None,
                    prefix: Some("pf-demo-".into()),
                },
            ],
        };
        assert!(h.hides(Some("pf-demo-1"), Path::new("C:\\other\\b.jsonl")));
        assert!(h.hides(Some("plain"), Path::new("c:\\users\\X\\sessions\\A.jsonl")));
        assert!(!h.hides(Some("plain"), Path::new("C:\\other\\c.jsonl")));
        assert!(!Hidden::default().hides(Some("pf-demo-1"), Path::new("x")));
    }
}
