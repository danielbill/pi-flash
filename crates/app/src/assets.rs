//! Embedded asset source: icons compiled into the binary.

use std::borrow::Cow;
use gpui::{AssetSource, SharedString};

pub struct Assets;

macro_rules! assets {
    ($($name:literal),* $(,)?) => {
        const ASSETS: &[(&str, &str)] = &[
            $( ($name, include_str!(concat!("../assets/", $name))) ),*
        ];

        impl AssetSource for Assets {
            fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
                Ok(ASSETS
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map(|(_, s)| Cow::Borrowed(s.as_bytes())))
            }

            fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
                Ok(ASSETS
                    .iter()
                    .filter(|(p, _)| p.starts_with(path))
                    .map(|(p, _)| SharedString::from(p.clone()))
                    .collect())
            }
        }
    };
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
    // v54 UI: composer context ring buckets
    "icons/spark.svg",
    "icons/ring-track.svg",
    "icons/ring-25.svg",
    "icons/ring-50.svg",
    "icons/ring-75.svg",
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
}
