//! PF 插件清单——手写极简解析（02 §2，不引 toml 库——设计定案）：
//! [`parse_manifest`] 主循环；值形态（`TomlVal`，见 [`super::val`]）与
//! `parse_val` 在 [`super::val`]。090-vh67 拆分：段头/键值臂在
//! [`super::fields`]，校验规则全量在 [`super::validate`]。

use std::path::Path;

use super::PluginManifest;
use super::val::{parse_val, strip_inline_comment};

pub(crate) fn parse_manifest(dir: &Path) -> Result<PluginManifest, String> {
    let path = dir.join("pf-plugin.toml");
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("{} 不可读: {e}", path.display()))?;

    let mut m = PluginManifest {
        dir: dir.to_path_buf(),
        pkg_dir: dir.to_path_buf(), // 平铺默认；版本布局在目录名对账处上钻
        pkg: String::new(),         // 扫描期回填（顶层包目录名）
        id: String::new(),
        title_zh: String::new(),
        title_en: None,
        version: String::new(),
        status_bar: None,
        topbar_menu: None,
        fn_bar: None,
        popups: Vec::new(),
        file_menu: Vec::new(),
        panels: Vec::new(),
        pi_extensions: Vec::new(),
        service: Vec::new(),
        sessions: Vec::new(),
        wasm: None,
        bytecode: None,
    };
    let mut section = String::new();
    for (n, raw) in src.lines().enumerate() {
        let line = strip_inline_comment(raw.trim());
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("[[") {
            let name = rest
                .strip_suffix("]]")
                .ok_or(format!("pf-plugin.toml 行 {}：段头未闭合", n + 1))?;
            section = super::fields::open_section(&mut m, name.trim(), n)?;
            continue;
        }
        if line.starts_with('[') {
            return Err(format!(
                "pf-plugin.toml 行 {}：不支持 [table] 段（用 [[contributes.*]]）",
                n + 1
            ));
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("pf-plugin.toml 行 {}：缺 =", n + 1));
        };
        let k = k.trim();
        let val = parse_val(v, n)?;
        super::fields::apply_field(&mut m, section.as_str(), k, &val, n)?;
    }

    super::validate::validate(&mut m, dir)?;
    Ok(m)
}
