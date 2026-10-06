//! Embedded asset source: icons compiled into the binary.

use std::borrow::Cow;
use gpui::{AssetSource, SharedString};

pub struct Assets;

macro_rules! assets {
    ($($name:literal),* $(,)?) => {
        const ASSETS: &[(&str, &str)] = &[
            $( ($name, include_str!(concat!("../assets/", $name))) ),*
        ];
    };
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if let Some(pct) = path.strip_prefix("icons/ring-p").and_then(|s| s.strip_suffix(".svg")) {
            if let Ok(p) = pct.parse::<u8>() {
                if (1..=99).contains(&p) {
                    return Ok(Some(Cow::Owned(ring_arc_svg(p))));
                }
            }
        }
        Ok(ASSETS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, s)| Cow::Borrowed(s.as_bytes())))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

/// 上下文比例环进度弧（1–99%）：与 ring-track.svg 同几何（viewBox 16、
/// r=5.5、stroke 2），12 点钟起顺时针 dasharray 弧；100% 走静态 ring-100。
/// gpui 的 svg 渲染只取 alpha 通道再按调用色着色，这里形状即一切。
fn ring_arc_svg(pct: u8) -> Vec<u8> {
    const CIRC: f32 = 2. * std::f32::consts::PI * 5.5; // ≈ 34.5575
    let arc = CIRC * pct as f32 / 100.;
    let rest = CIRC - arc;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\">\
<circle cx=\"8\" cy=\"8\" r=\"5.5\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" \
stroke-dasharray=\"{arc:.2} {rest:.2}\" stroke-linecap=\"round\" transform=\"rotate(-90 8 8)\"/></svg>"
    );
    svg.into_bytes()
}

assets! {
    "icons/git-branch.svg",
    "icons/plus.svg",
    "icons/search.svg",
    "icons/menu.svg",
    "icons/panel-left.svg",
    "icons/history.svg",
    "icons/pencil.svg",
    "icons/copy.svg",
    "icons/file-text.svg",
    "icons/wrench.svg",
    "icons/download.svg",
    "icons/image.svg",
    "icons/settings.svg",
    "icons/lightbulb.svg",
    "icons/scissors.svg",
    "icons/volume.svg",
    "icons/x.svg",
    "icons/chevron-down.svg",
    "icons/chevron-up.svg",
    "icons/chevron-right.svg",
    "icons/folder.svg",
    "icons/file.svg",
    "icons/send.svg",
    "icons/monitor.svg",
    "icons/upload.svg",
    "icons/refresh.svg",
    "icons/layers.svg",
    "icons/loader.svg",
    "icons/trash.svg",
    "icons/check.svg",
    "icons/terminal.svg",
    // v54 UI: lucide additions
    "icons/sliders-horizontal.svg",
    "icons/ellipsis.svg",
    "icons/ellipsis-v.svg",
    "icons/folder-open.svg",
    "icons/folder-closed.svg",
    "icons/message-square.svg",
    "icons/messages-square.svg",
    "icons/folder-tree.svg",
    "icons/clock.svg",
    "icons/arrow-up.svg",
    "icons/arrow-down.svg",
    "icons/wand.svg",
    "icons/bot.svg",
    // v64 topbar：会话标题 icon + ⋯ 菜单（系统提示词 / 工具）
    "icons/message-square-more.svg",
    "icons/file-sliders.svg",
    "icons/plug.svg",
    // v70 设置页：模型（芯片）+ MCP（服务器机架），路径取自 pi-web SettingsSectionIcon
    "icons/cpu.svg",
    "icons/server.svg",
    // v54 UI: iconfont solid set (from the design html)
    "icons/icon-project.svg",
    "icons/icon-new-chat.svg",
    "icons/icon-filebrowser.svg",
    "icons/icon-terminal-solid.svg",
    "icons/icon-trash-solid.svg",
    "icons/icon-organize.svg",
    "icons/icon-sort.svg",
    "icons/icon-grouped.svg",
    "icons/icon-flat.svg",
    "icons/icon-clock-solid.svg",
    "icons/icon-hand.svg",
    "icons/icon-viewdiff.svg",
    // v54 UI: composer context ring（进度弧 ring-p{1-99} 运行时生成）
    "icons/spark.svg",
    "icons/ring-track.svg",
    "icons/ring-100.svg",
    "icons/minus.svg",
    "icons/square.svg",
    "icons/restore.svg",
    // v65 品牌 logo（P+闪电，侧栏头部徽章；exe/.app 图标走 assets/icon 与 assets/macos）
    "icons/logo-marks.svg",
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_icons_load() {
        for (path, src) in ASSETS {
            assert!(src.contains("<svg"), "{path} is not an svg");
            let loaded = Assets.load(path).expect("load").expect("present");
            assert!(!loaded.is_empty());
        }
        assert!(Assets.load("icons/missing.svg").expect("ok").is_none());
    }

    /// 磁盘上有 `.svg` 但没写进 `assets!()` = 资产没编进二进制：`Assets::load`
    /// 返回 None，`gpui::svg()` **静默画空白**（不报错、不落日志）——复制图标
    /// 当初就是这样消失的（`all_icons_load` 只遍历宏列表，漏登的文件它看不见）。
    /// 两个方向都锁：文件↔登记一一对应；源码里 `icon("x")` 用到的名字必须有资产。
    #[test]
    fn every_icon_file_and_call_site_is_registered() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .expect("assets/icons readable")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".svg"))
            .collect();
        files.sort();
        let registered: std::collections::BTreeSet<&str> =
            ASSETS.iter().map(|(p, _)| *p).collect();

        let unregistered: Vec<&String> = files
            .iter()
            .filter(|n| !registered.contains(format!("icons/{n}").as_str()))
            .collect();
        assert!(
            unregistered.is_empty(),
            "assets/icons 下这些 svg 没登记进 assets!()（渲染为空白）：{unregistered:?}"
        );

        // 源码调用点：`icon("copy", …)` / `icon_hover("copy", …)`
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src_dir];
        let mut missing: Vec<String> = Vec::new();
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).expect("src readable") {
                let p = e.expect("entry").path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().and_then(|s| s.to_str()) != Some("rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&p).expect("read rs");
                for line in text.lines() {
                    let line = line.trim_start();
                    if line.starts_with("//") {
                        continue; // 注释里的示例不算调用点
                    }
                    for pat in ["icon(\"", "icon_hover(\""] {
                        let mut rest = line;
                        while let Some(i) = rest.find(pat) {
                            rest = &rest[i + pat.len()..];
                            let Some(end) = rest.find('"') else { break };
                            let name = &rest[..end];
                            if !registered.contains(format!("icons/{name}.svg").as_str()) {
                                missing.push(format!("{}: icon(\"{name}\")", p.display()));
                            }
                            rest = &rest[end..];
                        }
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "这些 icon() 调用点的 svg 没登记（会渲染成空白）：{missing:?}"
        );
    }

    #[test]
    fn ring_arc_generated() {
        for p in [1u8, 13, 50, 87, 99] {
            let svg = Assets
                .load(&format!("icons/ring-p{p}.svg"))
                .expect("ok")
                .expect("generated");
            let s = std::str::from_utf8(&svg).expect("utf8");
            assert!(s.contains("<svg") && s.contains("stroke-dasharray"), "{p} bad");
        }
        // 0 与 100 不生成：0 无弧不渲染，100 走静态 ring-100
        assert!(Assets.load("icons/ring-p0.svg").expect("ok").is_none());
        assert!(Assets.load("icons/ring-p100.svg").expect("ok").is_none());
    }
}
