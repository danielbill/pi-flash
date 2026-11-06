//! PF 插件清单——插件资产注册表（状态栏 icon）：gpui AssetSource 走磁盘，
//! 内存缓存（PLUGIN_ASSETS）+ 插件 id → 安装目录登记（PLUGIN_DIRS）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub(super) static PLUGIN_ASSETS: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> =
    OnceLock::new();

/// 插件 id → 实际安装目录（扫描时登记）。market 版本层布局下资产不在
/// `plugins/<id>/` 平铺位，解析走这里。
static PLUGIN_DIRS: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();

pub(super) fn register_plugin_dir(id: &str, dir: &Path) {
    if let Ok(mut c) = PLUGIN_DIRS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        c.insert(id.to_string(), dir.to_path_buf());
    }
}

/// 卸载后清插件 id 的进程级残留：目录登记 + 资产缓存（否则同名重装读到
/// 旧字节 / 旧路径，图标永远是死链）。
pub(crate) fn plugin_forget(id: &str) {
    if let Ok(mut c) = PLUGIN_DIRS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        c.remove(id);
    }
    if let Ok(mut c) = PLUGIN_ASSETS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        let prefix = format!("plugins/{id}/");
        c.retain(|k, _| !k.starts_with(&prefix));
    }
}

/// `rel` = `<slug>/<assets 相对路径>`（assets.rs 的 `plugins/` 前缀段）。
pub(crate) fn plugin_asset(rel: &str) -> Option<Vec<u8>> {
    let cache = PLUGIN_ASSETS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(hit) = cache.lock() {
        if let Some(hit) = hit.get(rel) {
            return hit.clone();
        }
    }
    // 平铺位优先（dev 手工拷贝）；未中则查扫描登记的实际目录（版本层布局）
    let mut bytes = pi_link::paths::plugins_dir().and_then(|d| std::fs::read(d.join(rel)).ok());
    if bytes.is_none()
        && let Some((id, rest)) = rel.split_once('/')
    {
        // 登记键 = manifest.id（pf-wxbot）；部分调用方给的是**目录别名**
        // （@pi-flash-plugins__pf-wxbot，如市场卡片 contrib_icon_path）——
        // 直查落空，按 `__<id>` 后缀对回登记键（真机 2026-11-03：市场卡片
        // 图标空白，会话标签同款 icon 正常，正因两条链键口径不同）
        let dir = PLUGIN_DIRS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .ok()
            .and_then(|c| {
                c.get(id).cloned().or_else(|| {
                    c.iter()
                        .find(|(k, _)| id.ends_with(&format!("__{k}")))
                        .map(|(_, v)| v.clone())
                })
            });
        if let Some(dir) = dir {
            bytes = std::fs::read(dir.join(rest)).ok();
        }
    }
    if let Some(b) = &bytes {
        if cfg!(debug_assertions) {
            eprintln!("[pf-plugin] asset {rel} -> {} bytes", b.len());
        }
    } else {
        // 未找到不缓存：扫描登记晚于首帧渲染时，下帧重查即中
        if cfg!(debug_assertions) {
            eprintln!("[pf-plugin] asset {rel} -> 未找到");
        }
        return None;
    }
    if let Ok(mut c) = cache.lock() {
        c.insert(rel.to_string(), bytes.clone());
    }
    bytes
}
