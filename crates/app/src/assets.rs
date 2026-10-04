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
    "icons/plug.svg",
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
