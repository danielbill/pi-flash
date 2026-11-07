//! 文件类型图标 —— 挪自 Zed：匹配算法逐字搬自
//! `crates/file_icons/src/file_icons.rs`，默认图标主题数据（映射表）逐字
//! 搬自 `crates/theme/src/icon_theme.rs`（"Zed (Default)"）。SVG 资产为
//! `assets/icons/file_icons/*.svg`（Zed 同名资产）。
//!
//! 去掉的基建：ThemeRegistry/GlobalTheme（本仓只有默认主题这一套）与
//! settings_content::FolderIndicator（Zed 默认 icon 模式已够用，chevron
//! 机制未搬；要加该设置时再从 Zed crates/file_icons 补）。
//!
//! 渲染注意：gpui svg 只取 alpha 通道按调用色染色，Zed 这套图标是单色
//! 线稿（fill/stroke=black），与 `ui::icon` 的染色机制天然契合。

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use super::paths_sort::{extension_or_hidden_file_name, multiple_extensions};

// ---------------------------------------------------------------------------
// 主题数据结构 + 默认主题 —— 搬自 Zed crates/theme/src/icon_theme.rs
// ---------------------------------------------------------------------------

/// An icon theme.
#[derive(Debug)]
pub struct IconTheme {
    /// The icons used for directories.
    pub directory_icons: DirectoryIcons,
    /// The icons used for named directories.（Zed 主题字段全量保留；默认主题
    /// 为空表，接 named-folder 设置时启用）
    #[allow(dead_code)]
    pub named_directory_icons: HashMap<&'static str, DirectoryIcons>,
    /// The mapping of file stems to their associated icon keys.
    pub file_stems: HashMap<&'static str, &'static str>,
    /// The mapping of file suffixes to their associated icon keys.
    pub file_suffixes: HashMap<&'static str, &'static str>,
    /// The mapping of icon keys to icon definitions.
    pub file_icons: HashMap<&'static str, &'static str>,
}

/// The icons used for directories.
#[derive(Debug, Clone)]
pub struct DirectoryIcons {
    /// The path to the icon to use for a collapsed directory.
    pub collapsed: Option<&'static str>,
    /// The path to the icon to use for an expanded directory.
    pub expanded: Option<&'static str>,
}

mod tables;

use tables::{FILE_ICONS, FILE_STEMS_BY_ICON_KEY, FILE_SUFFIXES_BY_ICON_KEY};
/// Returns a mapping of file associations to icon keys.
fn icon_keys_by_association(
    associations_by_icon_key: &[(&'static str, &'static [&'static str])],
) -> HashMap<&'static str, &'static str> {
    let mut icon_keys_by_association = HashMap::default();
    for (icon_key, associations) in associations_by_icon_key {
        for association in *associations {
            icon_keys_by_association.insert(*association, *icon_key);
        }
    }

    icon_keys_by_association
}

/// The name of the default icon theme.（Zed 主题名；设置页列表化时启用）
#[allow(dead_code)]
pub const DEFAULT_ICON_THEME_NAME: &str = "Zed (Default)";

static DEFAULT_ICON_THEME: LazyLock<IconTheme> = LazyLock::new(|| IconTheme {
    directory_icons: DirectoryIcons {
        collapsed: Some("icons/file_icons/folder.svg"),
        expanded: Some("icons/file_icons/folder_open.svg"),
    },
    named_directory_icons: HashMap::default(),
    file_stems: icon_keys_by_association(FILE_STEMS_BY_ICON_KEY),
    file_suffixes: icon_keys_by_association(FILE_SUFFIXES_BY_ICON_KEY),
    file_icons: FILE_ICONS.iter().copied().collect(),
});

/// Returns the default icon theme.
pub fn default_icon_theme() -> &'static IconTheme {
    &DEFAULT_ICON_THEME
}

// ---------------------------------------------------------------------------
// 匹配算法 —— 逐字搬自 Zed crates/file_icons/src/file_icons.rs:28-108
// （多级匹配：完整文件名 → 逐段后缀 → 多重扩展名 → 扩展名/隐藏文件名 →
// 扩展名 → default 兜底；stems 优先于 suffixes）
// ---------------------------------------------------------------------------

/// Resolve the SVG asset path ("icons/file_icons/rust.svg") for a file, or
/// the default file icon when nothing matches.
pub fn get_icon(path: &Path) -> &'static str {
    let theme = default_icon_theme();

    let get_icon_from_suffix = |suffix: &str| -> Option<&'static str> {
        theme
            .file_stems
            .get(suffix)
            .or_else(|| theme.file_suffixes.get(suffix))
            .and_then(|typ| get_icon_for_type(typ))
    };

    if let Some(mut typ) = path.file_name().and_then(|typ| typ.to_str()) {
        // check if file name is in suffixes
        // e.g. catch file named `eslint.config.js` instead of `.eslint.config.js`
        let maybe_path = get_icon_from_suffix(typ);
        if maybe_path.is_some() {
            return maybe_path.unwrap();
        }

        // check if suffix based on first dot is in suffixes
        // e.g. consider `module.js` as suffix to angular's module file named `auth.module.js`
        while let Some((_, suffix)) = typ.split_once('.') {
            let maybe_path = get_icon_from_suffix(suffix);
            if maybe_path.is_some() {
                return maybe_path.unwrap();
            }
            typ = suffix;
        }
    }

    // handle cases where the file extension is made up of multiple important
    // parts (e.g Component.stories.tsx) that refer to an alternative icon style
    if let Some(suffix) = multiple_extensions(path) {
        let maybe_path = get_icon_from_suffix(&suffix);
        if maybe_path.is_some() {
            return maybe_path.unwrap();
        }
    }

    // primary case: check if the files extension or the hidden file name
    // matches some icon path
    if let Some(suffix) = extension_or_hidden_file_name(path) {
        let maybe_path = get_icon_from_suffix(suffix);
        if maybe_path.is_some() {
            return maybe_path.unwrap();
        }
    }

    // this _should_ only happen when the file is hidden (has leading '.')
    // and is not a "special" file we have an icon (e.g. not `.eslint.config.js`)
    // that should be caught above. In the remaining cases, we want to check
    // for a normal supported extension e.g. `.data.json` -> `json`
    let extension = path.extension().and_then(|ext| ext.to_str());
    if let Some(extension) = extension {
        let maybe_path = get_icon_from_suffix(extension);
        if maybe_path.is_some() {
            return maybe_path.unwrap();
        }
    }
    get_icon_for_type("default").unwrap_or("icons/file_icons/file.svg")
}

fn get_icon_for_type(typ: &str) -> Option<&'static str> {
    default_icon_theme().file_icons.get(typ).copied()
}

/// Generic folder icon; falls back to the generic file icon if the theme
/// lacks one (default theme always has both).
pub fn get_generic_folder_icon(expanded: bool) -> &'static str {
    let icons = &default_icon_theme().directory_icons;
    if expanded {
        icons.expanded.unwrap_or("icons/file_icons/folder_open.svg")
    } else {
        icons.collapsed.unwrap_or("icons/file_icons/folder.svg")
    }
}

// ---------------------------------------------------------------------------
// 测试 —— 匹配测试按数据表写代表性断言；目录图标开/闭切换是文件树唯一
// 的展开态视觉信号（无 chevron），用测试锁住

#[cfg(test)]
mod tests;
