//! pf-plugin.toml 校验规则全量（02 §2 + 契约 v3/v4 增补）。
//! 自 parse.rs 抽离子函数（vh67 清偿），逻辑逐字未动。

use std::path::Path;

use super::PluginManifest;
use super::SettingsKind;
use super::val::is_kebab;

/// 解析完成后的结构校验：必填字段 / 目录名对账 / 路径存在性 / 契约规则。
pub(super) fn validate(m: &mut PluginManifest, dir: &Path) -> Result<(), String> {
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
    // 内层 wasm 插件（090-5 L3）无 contributes 也合法——验签在 wasm 内。
    // pi_extensions（v2.4/v3）/ service（v4）都是进驻面（真机 2026-11-27：
    // 只携 pi 扩展的 hook 型插件被误判纯库包拒装，测试暴露）
    if m.wasm.is_none()
        && m.bytecode.is_none()
        && m.status_bar.is_none()
        && m.file_menu.is_empty()
        && m.panels.is_empty()
        && m.fn_bar.is_none()
        && m.popups.is_empty()
        && m.pi_extensions.is_empty()
        && m.service.is_empty()
        && m.settings.is_empty()
    {
        return Err("纯库包（没申请任何进驻面），不加载".into());
    }
    // 契约 v6 统一插件设置（003-1）：id 非空唯一（点路径）、label 必给、
    // 按 type 验必填与值域（heavy 校验在插件仓 check 门；这里守住能装的底线）
    let mut setting_ids = std::collections::HashSet::new();
    for sf in &m.settings {
        if sf.id.is_empty() {
            return Err("pf-plugin.toml：settings 缺 id（字段 id = 存储键）".into());
        }
        if !setting_ids.insert(sf.id.clone()) {
            return Err(format!("pf-plugin.toml：settings id「{}」重复", sf.id));
        }
        if sf.label_zh.is_empty() {
            return Err(format!(
                "pf-plugin.toml：settings「{}」缺 label.zh（U7）",
                sf.id
            ));
        }
        match sf.kind {
            SettingsKind::Stepper => {
                let (Some(mn), Some(mx), Some(st)) = (sf.min, sf.max, sf.step) else {
                    return Err(format!(
                        "pf-plugin.toml：settings「{}」stepper 缺 min/max/step",
                        sf.id
                    ));
                };
                if mn >= mx || st == 0 {
                    return Err(format!(
                        "pf-plugin.toml：settings「{}」stepper 要求 min < max 且 step > 0",
                        sf.id
                    ));
                }
                if let Some(dv) = sf.default_num
                    && (dv < mn || dv > mx || (dv - mn) % st != 0)
                {
                    return Err(format!(
                        "pf-plugin.toml：settings「{}」default 必须落在 [min,max] 且按 step 对齐",
                        sf.id
                    ));
                }
            }
            SettingsKind::Model => {
                // session_option 只描述菜单能力；default_model 任选
                if sf.default_model.as_deref() == Some("session") && !sf.session_option {
                    return Err(format!(
                        "pf-plugin.toml：settings「{}」default_model=\"session\" 需要 session_option = true",
                        sf.id
                    ));
                }
            }
        }
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
    Ok(())
}
