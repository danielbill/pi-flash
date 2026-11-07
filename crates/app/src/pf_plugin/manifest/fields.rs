//! pf-plugin.toml 字段装配：`[[contributes.*]]` 段头登记与键值臂全量。
//! 自 parse.rs 抽离子函数（vh67 清偿），逻辑逐字未动。

use super::val::TomlVal;
use super::{
    FileMenuContrib, FnBarContrib, PanelContrib, PluginDock, PluginManifest, PopupContrib,
    SUPPORTED_MANIFEST_SCHEMA, ServiceContrib, SessionSlotContrib, StatusBarContrib,
    TopbarMenuContrib,
};

/// `[[contributes.*]]` 段头登记：各贡献点在对应 Vec/Option 里预置空条目，
/// 返回段名（后续键值臂按它分派）。单例段重复声明即拒。
pub(super) fn open_section(m: &mut PluginManifest, name: &str, n: usize) -> Result<String, String> {
    Ok(match name {
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
    })
}

/// 键值臂全量（02 §2）：按 (段名, 键) 分派写入；未知键即拒。
pub(super) fn apply_field(
    m: &mut PluginManifest,
    section: &str,
    k: &str,
    val: &TomlVal,
    n: usize,
) -> Result<(), String> {
    match (section, k) {
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
        ("status_bar", "icon") => m.status_bar.as_mut().unwrap().icon = val.as_str("icon", n)?,
        ("status_bar", "title") => {
            m.status_bar.as_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
        }
        ("topbar_menu", "icon") => m.topbar_menu.as_mut().unwrap().icon = val.as_str("icon", n)?,
        ("topbar_menu", "title") => {
            m.topbar_menu.as_mut().unwrap().title_zh = val.as_i18n("title", n)?.0
        }
        ("fn_bar", "command") => m.fn_bar.as_mut().unwrap().command = val.as_str("command", n)?,
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
        ("file_menu", "icon") => m.file_menu.last_mut().unwrap().icon = val.as_str("icon", n)?,
        ("panels", "id") => m.panels.last_mut().unwrap().id = val.as_str("id", n)?,
        ("panels", "title") => m.panels.last_mut().unwrap().title_zh = val.as_i18n("title", n)?.0,
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
    Ok(())
}
