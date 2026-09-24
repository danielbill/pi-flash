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
}

pub const ICON: &str = "icons";

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
