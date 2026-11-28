//! 插件设置存储（契约 v6 统一设置，003-1 定案 2026-11-28）。
//!
//! 分工：**真源** = `<数据目录>/plugin-settings/<pkg>.json`（PF 自有领域：
//! 建/读/写/删、卸载清理、版本换代不丢——已安装页设置区渲染与 dirty 比对读它，
//! 卸载随包清理）；**投递镜像** = `<插件包>/data/settings.json`（PF 保存时
//! 原子写、只写不读——90-4「PF 不解析 data/ 下任何文件」不破；插件使用时
//! 现读镜像：pi 扩展 fs 直读 / wasm storage_read / JS ctx.fs.read，零新 API）。
//!
//! 文件形态 = flat map `{ "<字段id>": 值 }`；未保存过的字段以清单
//! `[[contributes.settings]]` default 为准（宿主侧合成，见 app 层）。

use std::path::PathBuf;

use super::dir_name;

fn store_file(name: &str) -> Option<PathBuf> {
    crate::paths::plugin_settings_dir().map(|d| d.join(format!("{name}.json")))
}

/// 包层投递镜像落点：`<plugins>/<pkg>/data/settings.json`（包层 data，无版本
/// 维度——`PluginManifest::data_dir()` 同口径，更新换代镜像不丢）。
fn mirror_file(name: &str) -> Option<PathBuf> {
    crate::paths::plugins_dir().map(|d| d.join(dir_name(name)).join("data").join("settings.json"))
}

/// 读真源：缺文件 = 空表（字段值以清单 default 为准）；坏 JSON 视同空表
/// （坏盘不致命，下次保存覆盖）。
pub fn load(name: &str) -> serde_json::Map<String, serde_json::Value> {
    let Some(f) = store_file(name) else {
        return serde_json::Map::new();
    };
    match std::fs::read_to_string(f) {
        Ok(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(v) if v.is_object() => v.as_object().cloned().unwrap_or_default(),
            _ => serde_json::Map::new(),
        },
        Err(_) => serde_json::Map::new(),
    }
}

/// 保存：写真源（建目录、pretty JSON）+ 镜像投递（tmp+rename 原子写；
/// 插件 data 目录缺失则建——PF 本就保证 `<pkg>/data` 存在，兜底防御）。
pub fn save(name: &str, values: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let f = store_file(name).ok_or("无法定位插件设置真源路径")?;
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(values).map_err(|e| e.to_string())?;
    std::fs::write(&f, &s).map_err(|e| e.to_string())?;
    // 镜像投递：值相同也重写（插件侧以文件 mtime/内容为准，重写无害）
    if let Some(m) = mirror_file(name) {
        if let Some(dir) = m.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = m.with_extension("json.tmp");
        std::fs::write(&tmp, &s).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &m).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 卸载清理：真源随包删除（镜像在插件 data/ 里，卸载整目录删除自然带走）。
pub fn remove(name: &str) {
    if let Some(f) = store_file(name) {
        let _ = std::fs::remove_file(f);
    }
}
