//! pi 的扩展发现口径（复刻 vendored `core/extensions` 的 `discoverExtensionsInDir`
//! + `resolveExtensionEntries`）。
//!
//! 为什么要复刻：`full` 档要发 `-ne`（精确集：关掉发现/配置/内置扩展）才能
//! 保证**插件零注入**，而 `-ne` 会连「个人扩展」（`~/.pi/agent/extensions/`，
//! 如 pi-notify）一起关掉 —— 用户明确要求个人 ext 必须留着，所以这些路径要
//! 由 app 显式 `-e` 加回来。
//!
//! pi 的规则（bundle `chunk-33XOIQ5N.js` 逐字）：
//! - 目录下 `*.ts` / `*.js`（文件或符号链接）→ 直接算一个扩展；
//! - 子目录（或符号链接目录）→ `package.json` 的 `pi.extensions[]` 逐条相对
//!   包根解析（文件存在才算），否则 `index.ts` → `index.js`；
//! - **只降一层、不递归**：`<dir>/<pkg>/<sub>/index.ts` 这种嵌套**不会**被发现
//!   （pi 把一级子目录当包根处理）；
//! - 裸 `package.json`（无 `pi` 字段或有字段但解析不出条目）→ 忽略。
//!
//! 已知简化（写在 034）：settings `extensions` 数组只认**存在的普通路径**
//! （带 `!`/`-` 排除与 glob 模式的条目跳过）；项目作用域的发现（`.pi/extensions`
//! 与项目 settings）不在这里处理 —— 它受 project trust 约束，交给 pi 自己的
//! 信任流程，不在 full 档的精确集里复刻。

use std::path::{Path, PathBuf};

use serde_json::Value;

fn is_extension_file(name: &str) -> bool {
    name.ends_with(".ts") || name.ends_with(".js")
}

/// 子目录 → 扩展入口（`pi.extensions[]` → `index.ts` → `index.js`）。
fn resolve_entries(dir: &Path) -> Option<Vec<PathBuf>> {
    let manifest_path = dir.join("package.json");
    if let Ok(bytes) = std::fs::read(&manifest_path) {
        let text = String::from_utf8_lossy(&bytes);
        if let Ok(Value::Object(pkg)) = crate::config::parse_lenient(&text) {
            if let Some(entries) = pkg.get("pi").and_then(|p| p.get("extensions")).and_then(Value::as_array) {
                let resolved: Vec<PathBuf> = entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|rel| dir.join(rel))
                    .filter(|p| p.is_file())
                    .collect();
                if !resolved.is_empty() {
                    return Some(resolved);
                }
            }
        }
    }
    for name in ["index.ts", "index.js"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(vec![candidate]);
        }
    }
    None
}

/// 一个目录里的扩展入口（顺序 = pi 的 readdir 顺序，保持文件系统序）。
pub fn discover_in_dir(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        // 符号链接：跟随（pi 对 symlink 一律当候选，交给后续读取判真伪）
        let is_symlink = kind.is_symlink();
        let is_file = kind.is_file() || (is_symlink && path.is_file());
        let is_dir = kind.is_dir() || (is_symlink && path.is_dir());
        if is_file && is_extension_file(&entry.file_name().to_string_lossy()) {
            out.push(path);
            continue;
        }
        if is_dir {
            if let Some(entries) = resolve_entries(&path) {
                out.extend(entries);
            }
        }
    }
    out
}

/// 个人扩展清单（相对/绝对路径字符串，直接喂 `-e`）：
/// 1. `<agent dir>/extensions/` 的发现；
/// 2. 用户 `settings.json` 的 `extensions` 数组里**存在的普通路径**条目
///    （`builtin:`/`+builtin:`/`-builtin:` 与 glob/排除条目跳过；目录走发现，
///    文件直接用；相对路径按 pi 规则相对 agent dir 解析）。
pub fn personal_extensions() -> Vec<String> {
    let agent_dir = crate::config::agent_dir();
    let mut out: Vec<PathBuf> = Vec::new();
    out.extend(discover_in_dir(&agent_dir.join("extensions")));

    if let Ok(settings) = crate::config::read_json(&crate::config::settings_path()) {
        if let Some(entries) = settings.get("extensions").and_then(Value::as_array) {
            for raw in entries.iter().filter_map(Value::as_str) {
                let raw = raw.trim();
                if raw.is_empty() || raw.starts_with('!') || raw.starts_with('-') {
                    continue;
                }
                let raw = raw.strip_prefix('+').unwrap_or(raw);
                if raw.starts_with("builtin:") || raw.contains('*') || raw.contains('?') {
                    continue; // 内置扩展由 enabled_builtin_extensions 处理；glob 不复刻
                }
                let path = if Path::new(raw).is_absolute() {
                    PathBuf::from(raw)
                } else {
                    agent_dir.join(raw)
                };
                if path.is_dir() {
                    out.extend(discover_in_dir(&path));
                } else if path.is_file() {
                    out.push(path);
                }
            }
        }
    }

    let mut seen: Vec<String> = Vec::new();
    for p in out {
        let s = p.to_string_lossy().into_owned();
        if !seen.contains(&s) {
            seen.push(s);
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(base: &Path) {
        let _ = std::fs::remove_dir_all(base);
        std::fs::create_dir_all(base.join("with-manifest").join("src")).unwrap();
        std::fs::create_dir_all(base.join("with-index")).unwrap();
        std::fs::create_dir_all(base.join("nested").join("sub")).unwrap();
        // 顶层裸文件（= ~/.pi/agent/extensions/pi-notify.ts 的位置）
        std::fs::write(base.join("notify.ts"), "export default () => {}").unwrap();
        std::fs::write(base.join("notes.md"), "ignored").unwrap();
        // 一级子目录：index.js 当入口
        std::fs::write(base.join("with-index").join("index.js"), "module.exports=1").unwrap();
        // 一级子目录：package.json 的 pi.extensions[]
        std::fs::write(base.join("with-manifest").join("src").join("a.ts"), "x").unwrap();
        std::fs::write(base.join("with-manifest").join("src").join("b.ts"), "x").unwrap();
        std::fs::write(
            base.join("with-manifest").join("package.json"),
            r#"{"name":"p","pi":{"extensions":["./src/a.ts","./src/b.ts","./missing.ts"]}}"#,
        )
        .unwrap();
        // 嵌套（二级）目录里的 index.ts：pi 不递归 → 不该被发现
        std::fs::write(base.join("nested").join("sub").join("index.ts"), "x").unwrap();
    }

    #[test]
    fn matches_pi_discovery_rules() {
        let base = std::env::temp_dir().join(format!("pi-flash-extscan-{}", std::process::id()));
        tree(&base);
        let paths = discover_in_dir(&base);
        let found: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(found.contains(&"notify.ts".to_string()), "顶层裸 .ts 文件要发现");
        assert!(found.contains(&"a.ts".to_string()) && found.contains(&"b.ts".to_string()),
            "package.json pi.extensions[] 要发现（missing 跳过）");
        assert!(found.contains(&"index.js".to_string()), "一级子目录 index.js 要发现");
        assert!(!found.contains(&"notes.md".to_string()), "非 .ts/.js 忽略");
        assert!(!found.iter().any(|n| n == "missing.ts"), "不存在条目不发现");
        // pi 不递归：二级目录里的 index.ts 不算（一级子目录被当包根处理）
        assert!(
            !paths.iter().any(|p| p.to_string_lossy().contains("nested")),
            "嵌套目录不递归发现（与 pi 一致）"
        );
    }
}
