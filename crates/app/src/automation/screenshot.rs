//! ui.screenshot —— 窗口最近一帧渲染回读为 PNG（pi-flash-0uq）。
//!
//! 走 vendored gpui 的 `Window::render_to_image()`（D3D11 staging 纹理
//! readback，移植自 zed main）：进程内、不抢屏、窗口被遮挡/最小化也能截，
//! 像素即 gpui scene 输出，合规于「禁止 OS 截图」纪律。
//!
//! 注意截的是**最近一次绘制**的帧：exec 改状态后紧跟 shot 可能差一帧，
//! 使用模式是先 `pif-ui wait` 后 shot（或隔一次 CLI 调用）。

use gpui::Window;
use serde_json::{json, Value};

/// ui.screenshot：params `{path?}`（缺省 `<配置目录>/automation/shots/`）
/// → `{path,width,height,bytes}`。只读 op，不改状态、不 notify。
pub fn shot(window: &Window, params: &Value) -> Result<Value, (String, String)> {
    let img = window
        .render_to_image()
        .map_err(|e| ("internal".to_string(), format!("渲染回读失败: {e}")))?;
    let (width, height) = (img.width(), img.height());

    let path: std::path::PathBuf = match params.get("path").and_then(Value::as_str) {
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => default_shots_dir()
            .ok_or_else(|| {
                (
                    "internal".to_string(),
                    "解析截图目录失败（PI_FLASH_DIR/USERPROFILE 均未设）".to_string(),
                )
            })?
            .join(format!("shot-{}.png", now_ms())),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ("internal".to_string(), format!("建目录 {}: {e}", parent.display())))?;
    }

    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ("internal".to_string(), format!("PNG 编码失败: {e}")))?;
    std::fs::write(&path, &buf)
        .map_err(|e| ("internal".to_string(), format!("写 {}: {e}", path.display())))?;
    Ok(json!({
        "path": path.display().to_string(),
        "width": width,
        "height": height,
        "bytes": buf.len(),
    }))
}

/// 缺省落盘目录：`<配置目录>/automation/shots/`（PI_FLASH_DIR 隔离自动生效）。
fn default_shots_dir() -> Option<std::path::PathBuf> {
    pi_link::automation::instances_dir().map(|d| d.join("shots"))
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
