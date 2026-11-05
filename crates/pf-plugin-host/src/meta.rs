use super::*;

/// 插件面板已装目录描述（app 用）。
pub struct PluginMeta {
    pub slug: String,
    pub title: String,
    pub wasm_path: std::path::PathBuf,
}

pub fn scan_plugins_dir(dir: &Path) -> Vec<PluginMeta> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let d = e.path();
        if !d.is_dir() {
            continue;
        }
        let manifest = d.join("pf-plugin.toml");
        if !manifest.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        // 极简解析：name/title/wasm 三行（v1 不引 toml 库）
        let mut name = None;
        let mut title = None;
        let mut wasm = None;
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("name = ") {
                name = Some(unquote(v));
            } else if let Some(v) = line.strip_prefix("title = ") {
                title = Some(unquote(v));
            } else if let Some(v) = line.strip_prefix("wasm = ") {
                wasm = Some(unquote(v));
            }
        }
        let wasm = wasm.unwrap_or_else(|| "plugin.wasm".to_string());
        let wasm_path = d.join(&wasm);
        if !wasm_path.is_file() {
            continue;
        }
        out.push(PluginMeta {
            slug: name.unwrap_or_else(|| "unknown".into()),
            title: title.unwrap_or_else(|| "插件".into()),
            wasm_path,
        });
    }
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    v.trim_matches('"').trim_matches('\'').to_string()
}
