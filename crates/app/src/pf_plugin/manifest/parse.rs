//! PF 插件清单——手写极简解析（02 §2，不引 toml 库——设计定案）：
//! 值形态 [`TomlVal`] / `parse_val` / 行内注释剥离 / [`parse_manifest`]
//! （解析 + 02 §2 校验规则全量）。

use std::path::Path;

use super::val::{TomlVal, is_kebab, parse_val, strip_inline_comment};
use super::{
    FileMenuContrib, FnBarContrib, PanelContrib, PluginDock, PluginManifest, PopupContrib,
    ServiceContrib, SessionSlotContrib, StatusBarContrib, SUPPORTED_MANIFEST_SCHEMA,
    TopbarMenuContrib,
};

/// 解析 + 校验单个插件目录的 pf-plugin.toml（02 §2 校验规则）。
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
            section = match name.trim() {
                "contributes.status_bar" => {
                    if m.status_bar
                        .replace(StatusBarContrib {
                            icon: String::new(),
                            title_zh: String::new(),
                        })
                        .is_some()
                    {
                        return Err("pf-plugin.toml：status_bar 只能声明一个".into());
                    }
                    "status_bar".to_string()
                }
                "contributes.file_menu" => {
                    m.file_menu.push(FileMenuContrib {
                        id: String::new(),
                        title_zh: String::new(),
                        title_en: None,
                        icon: "puzzle".into(),
                    });
                    "file_menu".to_string()
                }
                "contributes.topbar_menu" => {
                    if m.topbar_menu
                        .replace(TopbarMenuContrib {
                            icon: String::new(),
                            title_zh: String::new(),
                        })
                        .is_some()
                    {
                        return Err("pf-plugin.toml：topbar_menu 只能声明一个".into());
                    }
                    "topbar_menu".to_string()
                }
                "contributes.fn_bar" => {
                    if m.fn_bar
                        .replace(FnBarContrib {
                            command: String::new(),
                        })
                        .is_some()
                    {
                        return Err("pf-plugin.toml：fn_bar 只能声明一个".into());
                    }
                    "fn_bar".to_string()
                }
                "contributes.popups" => {
                    m.popups.push(PopupContrib {
                        id: String::new(),
                        title_zh: String::new(),
                        title_en: None,
                        size: "MD".into(),
                        entry: "main.js".into(),
                    });
                    "popups".to_string()
                }
                "contributes.panels" => {
                    m.panels.push(PanelContrib {
                        id: String::new(),
                        title_zh: String::new(),
                        entry: "main.js".into(),
                        dock: PluginDock::Panel,
                    });
                    "panels".to_string()
                }
                "contributes.pi_extensions" => {
                    m.pi_extensions.push(String::new());
                    "pi_extensions".to_string()
                }
                "contributes.service" => {
                    m.service.push(ServiceContrib {
                        id: String::new(),
                        scope: String::new(),
                        entry: String::new(),
                    });
                    "service".to_string()
                }
                "contributes.sessions" => {
                    m.sessions.push(SessionSlotContrib {
                        prefix: String::new(),
                    });
                    "sessions".to_string()
                }
                other => return Err(format!("pf-plugin.toml 行 {}：未知段 [[{other}]]", n + 1)),
            };
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
        match (section.as_str(), k) {
            ("", "schema") => {
                // 契约大版本协商（schema/plugin-contract.json 的 manifest_schema）：
                // 高于宿主支持的版本一律拒装，**不降级运行**
                let got: u32 = val
                    .as_raw("schema", n)?
                    .parse()
                    .map_err(|_| format!("pf-plugin.toml 行 {}：schema 必须是整数", n + 1))?;
                if got != SUPPORTED_MANIFEST_SCHEMA && got != 3 && got != 4 {
                    return Err(format!(
                        "pf-plugin.toml：清单 schema {got} 不被本版 PF 支持（支持 {SUPPORTED_MANIFEST_SCHEMA}，兼容 3/4）"
                    ));
                }
            }
            ("", "id") => m.id = val.as_str("id", n)?,
            ("", "version") => m.version = val.as_str("version", n)?,
            ("", "wasm") => m.wasm = Some(val.as_str("wasm", n)?),
            ("", "bytecode") => m.bytecode = Some(val.as_str("bytecode", n)?),
            ("", "title") => {
                let (zh, en) = val.as_i18n("title", n)?;
                m.title_zh = zh;
                m.title_en = en;
            }
            ("status_bar", "icon") => {
                m.status_bar.as_mut().unwrap().icon = val.as_str("icon", n)?
            }
            ("status_bar", "title") => {
                m.status_bar.as_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
            }
            ("topbar_menu", "icon") => {
                m.topbar_menu.as_mut().unwrap().icon = val.as_str("icon", n)?
            }
            ("topbar_menu", "title") => {
                m.topbar_menu.as_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
            }
            ("fn_bar", "command") => {
                m.fn_bar.as_mut().unwrap().command = val.as_str("command", n)?
            }
            ("popups", "id") => m.popups.last_mut().unwrap().id = val.as_str("id", n)?,
            ("popups", "title") => {
                let (zh, en) = val.as_i18n("title", n)?;
                let p = m.popups.last_mut().unwrap();
                p.title_zh = zh;
                p.title_en = en;
            }
            ("popups", "size") => {
                let s = val.as_str("size", n)?;
                if !matches!(s.as_str(), "SM" | "MD" | "LG") {
                    return Err(format!(
                        "pf-plugin.toml 行 {}：popups.size 只认 \"SM\" | \"MD\" | \"LG\"，得到 {s}",
                        n + 1
                    ));
                }
                m.popups.last_mut().unwrap().size = s;
            }
            ("popups", "entry") => m.popups.last_mut().unwrap().entry = val.as_str("entry", n)?,
            ("pi_extensions", "path") => {
                if let Some(last) = m.pi_extensions.last_mut() {
                    *last = val.as_str("path", n)?.to_string();
                }
            }
            ("file_menu", "id") => m.file_menu.last_mut().unwrap().id = val.as_str("id", n)?,
            ("file_menu", "title") => {
                let (zh, en) = val.as_i18n("title", n)?;
                let fm = m.file_menu.last_mut().unwrap();
                fm.title_zh = zh;
                fm.title_en = en;
            }
            ("file_menu", "icon") => {
                m.file_menu.last_mut().unwrap().icon = val.as_str("icon", n)?
            }
            ("panels", "id") => m.panels.last_mut().unwrap().id = val.as_str("id", n)?,
            ("panels", "title") => {
                m.panels.last_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
            }
            ("panels", "layout") => {
                return Err(format!(
                    "pf-plugin.toml 行 {}：字段 layout 已废弃（契约 v2，见 02 §2）",
                    n + 1
                ));
            }
            ("panels", "store") => {
                return Err(format!(
                    "pf-plugin.toml 行 {}：字段 store 已废弃，数据由插件自管（ctx.dir，见 02 §3）",
                    n + 1
                ));
            }
            ("panels", "entry") => m.panels.last_mut().unwrap().entry = val.as_str("entry", n)?,
            ("panels", "dock") => {
                m.panels.last_mut().unwrap().dock = match val.as_str("dock", n)?.as_str() {
                    "panel" => PluginDock::Panel,
                    "tab" => PluginDock::Tab,
                    other => {
                        return Err(format!(
                            "pf-plugin.toml 行 {}：dock 只认 \"panel\" | \"tab\"，得到 {other}",
                            n + 1
                        ));
                    }
                };
            }
            ("service", "id") => {
                m.service.last_mut().unwrap().id = val.as_str("id", n)?.to_string();
            }
            ("service", "scope") => {
                m.service.last_mut().unwrap().scope = val.as_str("scope", n)?.to_string();
            }
            ("service", "entry") => {
                m.service.last_mut().unwrap().entry = val.as_str("entry", n)?.to_string();
            }
            ("sessions", "prefix") => {
                *m.sessions.last_mut().unwrap() = SessionSlotContrib {
                    prefix: val.as_str("prefix", n)?,
                };
            }
            (sec, key) => return Err(format!("pf-plugin.toml 行 {}：{sec} 段未知键 {key}", n + 1)),
        }
    }

    // ---- 校验 ----
    if m.id.is_empty() {
        return Err("pf-plugin.toml：缺 id".into());
    }
    if !is_kebab(&m.id) {
        return Err(format!("pf-plugin.toml：id「{}」非 kebab-case", m.id));
    }
    // 目录名对账（02 §6 布局两档）：平铺 dev = `<id>/`；market 安装 =
    // `@<scope>__<id>/<version>/`。版本层取父目录名；scope 前缀剥掉后对账。
    let mut base = dir.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    if base.starts_with(|c: char| c.is_ascii_digit())
        && let Some(parent) = dir.parent()
        && let Some(p) = parent.file_name().and_then(|s| s.to_str())
    {
        base = p; // 版本目录 → 上钻到插件目录
        m.pkg_dir = parent.to_path_buf(); // 用户数据挂顶层包目录（data_dir）
    }
    let expected = base.rsplit("__").next().unwrap_or(base);
    if m.id != expected {
        return Err(format!("pf-plugin.toml：id「{}」≠ 目录名「{base}」", m.id));
    }
    if m.title_zh.is_empty() {
        return Err("pf-plugin.toml：缺 title.zh（U7）".into());
    }
    if m.version.is_empty() {
        return Err("pf-plugin.toml：缺 version".into());
    }
    // 内层 wasm 插件（090-5 L3）无 contributes 也合法——验签在 wasm 内
    if m.wasm.is_none()
        && m.bytecode.is_none()
        && m.status_bar.is_none()
        && m.file_menu.is_empty()
        && m.panels.is_empty()
        && m.fn_bar.is_none()
        && m.popups.is_empty()
    {
        return Err("纯库包（没申请任何进驻面），不加载".into());
    }
    if let Some(sb) = &m.status_bar {
        if sb.icon.is_empty() {
            return Err("pf-plugin.toml：status_bar 缺 icon".into());
        }
        if sb.icon.starts_with("assets/") && !dir.join(&sb.icon).is_file() {
            return Err(format!(
                "pf-plugin.toml：status_bar.icon「{}」不存在（M6）",
                sb.icon
            ));
        }
    }
    let mut panel_ids = std::collections::HashSet::new();
    for p in &m.panels {
        if !is_kebab(&p.id) {
            return Err(format!(
                "pf-plugin.toml：panel id「{}」非 kebab-case（M5）",
                p.id
            ));
        }
        if !panel_ids.insert(p.id.clone()) {
            return Err(format!("pf-plugin.toml：panel id「{}」重复（M5）", p.id));
        }
        if p.title_zh.is_empty() {
            return Err(format!(
                "pf-plugin.toml：panel「{}」缺 title.zh（M5）",
                p.id
            ));
        }
        // 内层 wasm 包面板无 entry 文件（渲染面 = wasm render），跳过文件校验
        if m.wasm.is_none() && m.bytecode.is_none() && !dir.join(&p.entry).is_file() {
            return Err(format!(
                "pf-plugin.toml：entry「{}」不存在（02 §2）",
                p.entry
            ));
        }
    }
    for fm in &m.file_menu {
        if !is_kebab(&fm.id) || fm.title_zh.is_empty() {
            return Err(format!(
                "pf-plugin.toml：file_menu「{}」缺 id/title.zh",
                fm.id
            ));
        }
    }
    // 契约 v4：service 校验（id kebab / scope 封闭 / entry 非空）
    for sc in &m.service {
        let ok = !sc.id.is_empty()
            && sc
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !sc.id.starts_with('-')
            && !sc.id.ends_with('-');
        if !ok {
            return Err(format!(
                "pf-plugin.toml：service.id「{}」须 kebab-case",
                sc.id
            ));
        }
        if sc.scope != "session" {
            return Err(format!(
                "pf-plugin.toml：service.scope 只认 \"session\"，得到 {:?}",
                sc.scope
            ));
        }
        if sc.entry.is_empty() {
            return Err("pf-plugin.toml：service.entry 不能为空".into());
        }
    }
    // 契约 v3：fn_bar / popups 校验
    if let Some(fb) = &m.fn_bar {
        if !is_kebab(&fb.command) {
            return Err(format!(
                "pf-plugin.toml：fn_bar.command「{}」非 kebab-case",
                fb.command
            ));
        }
    }
    let mut popup_ids = std::collections::HashSet::new();
    for p in &m.popups {
        if !is_kebab(&p.id) {
            return Err(format!("pf-plugin.toml：popup id「{}」非 kebab-case", p.id));
        }
        if !popup_ids.insert(p.id.clone()) {
            return Err(format!("pf-plugin.toml：popup id「{}」重复", p.id));
        }
        // title 可空：空 = 不画标题栏（旧版弹窗无标题栏形态，用户 2026-10-28 定夺）
        if m.wasm.is_none() && m.bytecode.is_none() && !dir.join(&p.entry).is_file() {
            return Err(format!(
                "pf-plugin.toml：popups entry「{}」不存在（02 §2）",
                p.entry
            ));
        }
    }
    Ok(m)
}
