//! 开发旗标（dev flags）——重型模块的「发版排除」配置。
//!
//! 事实源 = `<PF数据目录>/dev-flags.json`（[`crate::paths::dev_flags_file`]；
//! debug 构建 = `./pi-flash-dev/`，release = `~/.pi-flash/`，用户机器无此
//! 文件 → 全部键走默认 false，发布包即关态）。解析走
//! [`crate::config::read_json`]：容忍 `//` 注释 / 尾逗号 / BOM，缺文件 =
//! 空对象，文件可自带说明注释。
//!
//! 键约定：
//! - 键名 = 模块名 snake_case，注册在 [`KNOWN`]（后续模块在此加键即接入）；
//! - 值 bool，**false / 缺省 = 关**（发版排除语义）；
//! - 环境变量覆盖最高优先：`PI_FLASH_<键大写>`，`1`/`true` 强开、`0`/`false`
//!   强关（如 `subagents` → `PI_FLASH_SUBAGENTS`）；
//! - 本模块**不缓存**（调用频率 = spawn 级）；app 渲染侧用
//!   `workspace::dev_flag_enabled` 的 OnceLock 快照——改动重启生效。

use serde_json::Value;

/// 已知旗标键（模块发版排除注册表）。新增可配置模块 = 在此加一个键。
pub const KNOWN: &[&str] = &["subagents", "pi_ext_market", "pi_native_market", "wx"];

/// 某模块旗标是否开启：env 覆盖 > dev-flags.json 键值 > false。
pub fn enabled(key: &str) -> bool {
    let env_name = format!("PI_FLASH_{}", key.to_uppercase());
    if let Ok(flag) = std::env::var(&env_name) {
        if flag == "0" || flag.eq_ignore_ascii_case("false") {
            return false;
        }
        if flag == "1" || flag.eq_ignore_ascii_case("true") {
            return true;
        }
    }
    from_file(key)
}

/// 只看文件的键值（不含 env 覆盖；缺文件/缺键/非 bool = false）。
pub fn from_file(key: &str) -> bool {
    file_values()
        .and_then(|v| v.get(key).and_then(Value::as_bool))
        .unwrap_or(false)
}

/// dev-flags 全文件内容；缺文件 = 空对象，坏文件 = None（fail-closed 全关）。
fn file_values() -> Option<Value> {
    let path = crate::paths::dev_flags_file()?;
    crate::config::read_json(&path).ok()
}

/// 启动日志一行：KNOWN 全键状态（`subagents=off`）+ 未知键提示（typo 检测）。
pub fn summary() -> String {
    let mut out = String::new();
    let file = file_values();
    for key in KNOWN {
        let on = file
            .as_ref()
            .and_then(|v| v.get(*key).and_then(Value::as_bool))
            .unwrap_or(false);
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("{key}={}", if on { "on" } else { "off" }));
    }
    if let Some(v) = &file {
        let unknown: Vec<&str> = v
            .as_object()
            .map(|o| {
                o.keys()
                    .map(String::as_str)
                    .filter(|k| !KNOWN.contains(k))
                    .collect()
            })
            .unwrap_or_default();
        if !unknown.is_empty() {
            out.push_str(&format!(
                " (未知键 {}——已知键清单见 pi-link dev_flags::KNOWN)",
                unknown.join(",")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PI_FLASH_DIR 指到临时目录再读写 dev-flags.json。
    /// env 操作在测试进程内是进程级的：本测试串行改 / 恢复，且不用并行断言。
    struct DirGuard(std::path::PathBuf);
    impl DirGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("devflags-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            // 先例 = updater.rs 测试：env 突变进程级，接受并行测试间的权衡
            unsafe { std::env::set_var("PI_FLASH_DIR", &dir) };
            Self(dir)
        }
    }
    impl Drop for DirGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("PI_FLASH_DIR") };
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 单测试串行覆盖全部路径：env（PI_FLASH_DIR）是进程级的，拆多个测试
    /// 会在并行下互踩（from_file/summary 曾因此各拿各的目录）。
    #[test]
    fn file_keys_and_summary_all_paths() {
        let _g = DirGuard::new("all");
        // 缺文件 = 全关
        assert!(!from_file("subagents"));
        let empty = summary();
        assert!(empty.contains("subagents=off"), "{empty}");
        // true（注释与尾逗号都认）/ false / 缺键 = 关；未知键进 summary 提示
        let path = crate::paths::dev_flags_file().unwrap();
        std::fs::write(&path, "// 注释也认得\n{ \"subagents\": true, }").unwrap();
        assert!(from_file("subagents"));
        assert!(summary().contains("subagents=on"));
        std::fs::write(&path, "{ \"subagents\": false }").unwrap();
        assert!(!from_file("subagents"));
        std::fs::write(&path, "{ \"subagent\": 1 }").unwrap();
        assert!(!from_file("subagents"), "缺键 = 关");
        let s = summary();
        assert!(s.contains("subagents=off"), "{s}");
        assert!(s.contains("未知键 subagent"), "{s}");
    }
}
