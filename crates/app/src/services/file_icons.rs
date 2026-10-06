//! 文件类型图标 —— 挪自 Zed：匹配算法逐字搬自
//! `crates/file_icons/src/file_icons.rs`，默认图标主题数据（映射表）逐字
//! 搬自 `crates/theme/src/icon_theme.rs`（"Zed (Default)"）。SVG 资产为
//! `assets/icons/file_icons/*.svg`（Zed 同名资产）。
//!
//! 去掉的基建：ThemeRegistry/GlobalTheme（本仓只有默认主题这一套）与
//! settings_content::FolderIndicator（此处内联同款枚举）。
//!
//! 渲染注意：gpui svg 只取 alpha 通道按调用色染色，Zed 这套图标是单色
//! 线稿（fill/stroke=black），与 `ui::icon` 的染色机制天然契合。

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use super::paths_sort::{extension_or_hidden_file_name, multiple_extensions};

/// What a panel draws ahead of a directory's name, in render order.
/// （Zed 全量枚举搬入；渲染走 Default=Icon，Chevron/Both 随设置页启用）
#[allow(dead_code)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FolderIndicator {
    #[default]
    Icon,
    Chevron,
    Both,
}

impl FolderIndicator {
    pub fn shows_chevron(&self) -> bool {
        matches!(self, FolderIndicator::Chevron | FolderIndicator::Both)
    }

    pub fn shows_icon(&self) -> bool {
        matches!(self, FolderIndicator::Icon | FolderIndicator::Both)
    }
}

/// What a panel draws ahead of a directory's name, in render order.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FolderIndicators {
    pub chevron: Option<&'static str>,
    pub icon: Option<&'static str>,
}

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
    /// The icons used for chevrons.
    pub chevron_icons: ChevronIcons,
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

/// The icons used for chevrons.
#[derive(Debug, Clone)]
pub struct ChevronIcons {
    /// The path to the icon to use for a collapsed chevron.
    pub collapsed: Option<&'static str>,
    /// The path to the icon to use for an expanded chevron.
    pub expanded: Option<&'static str>,
}

const FILE_STEMS_BY_ICON_KEY: &[(&str, &[&str])] = &[
    ("docker", &["Containerfile", "Dockerfile", ".dockerignore"]),
    ("ruby", &["Podfile"]),
    ("heroku", &["Procfile"]),
];

const FILE_SUFFIXES_BY_ICON_KEY: &[(&str, &[&str])] = &[
    ("astro", &["astro"]),
    (
        "audio",
        &[
            "aac", "flac", "m4a", "mka", "mp3", "ogg", "opus", "wav", "wma", "wv",
        ],
    ),
    ("backup", &["bak"]),
    ("ballerina", &["bal"]),
    ("bicep", &["bicep"]),
    ("bun", &["lockb"]),
    ("c", &["c", "h"]),
    ("cairo", &["cairo"]),
    ("code", &["handlebars", "metadata", "rkt", "scm"]),
    ("coffeescript", &["coffee"]),
    (
        "cpp",
        &[
            "c++", "h++", "cc", "cpp", "cppm", "cxx", "hh", "hpp", "hxx", "inl", "ixx",
        ],
    ),
    ("crystal", &["cr", "ecr"]),
    ("csharp", &["cs"]),
    ("csproj", &["csproj"]),
    ("css", &["css", "pcss", "postcss"]),
    ("cue", &["cue"]),
    ("dart", &["dart"]),
    ("diff", &["diff"]),
    (
        "docker",
        &[
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
        ],
    ),
    (
        "document",
        &[
            "doc", "docx", "mdx", "odp", "ods", "odt", "pdf", "ppt", "pptx", "rtf", "txt", "xls",
            "xlsx",
        ],
    ),
    ("editorconfig", &["editorconfig"]),
    ("elixir", &["eex", "ex", "exs", "heex", "leex", "neex"]),
    ("elm", &["elm"]),
    (
        "erlang",
        &[
            "Emakefile",
            "app.src",
            "erl",
            "escript",
            "hrl",
            "rebar.config",
            "xrl",
            "yrl",
        ],
    ),
    (
        "eslint",
        &[
            "eslint.config.cjs",
            "eslint.config.cts",
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.mts",
            "eslint.config.ts",
            "eslintrc",
            "eslintrc.js",
            "eslintrc.json",
        ],
    ),
    ("font", &["otf", "ttf", "woff", "woff2"]),
    ("fsharp", &["fs"]),
    ("fsproj", &["fsproj"]),
    ("gitlab", &["gitlab-ci.yml", "gitlab-ci.yaml"]),
    ("gleam", &["gleam"]),
    ("go", &["go", "mod", "work"]),
    ("graphql", &["gql", "graphql", "graphqls"]),
    ("haskell", &["hs"]),
    ("hcl", &["hcl"]),
    (
        "helm",
        &[
            "helmfile.yaml",
            "helmfile.yml",
            "Chart.yaml",
            "Chart.yml",
            "Chart.lock",
            "values.yaml",
            "values.yml",
            "requirements.yaml",
            "requirements.yml",
            "tpl",
        ],
    ),
    ("html", &["htm", "html"]),
    (
        "image",
        &[
            "avif", "bmp", "gif", "heic", "heif", "ico", "j2k", "jfif", "jp2", "jpeg", "jpg",
            "jxl", "png", "psd", "qoi", "svg", "tiff", "webp",
        ],
    ),
    ("ipynb", &["ipynb"]),
    ("java", &["java"]),
    ("javascript", &["cjs", "js", "mjs"]),
    ("json", &["json", "jsonc"]),
    ("julia", &["jl"]),
    ("kdl", &["kdl"]),
    ("kotlin", &["kt"]),
    ("lock", &["lock"]),
    ("log", &["log"]),
    ("lua", &["lua"]),
    ("luau", &["luau"]),
    ("markdown", &["markdown", "md"]),
    ("metal", &["metal"]),
    ("nim", &["nim", "nims", "nimble"]),
    ("nix", &["nix"]),
    ("ocaml", &["ml", "mli", "mlx"]),
    ("odin", &["odin"]),
    ("php", &["php"]),
    (
        "prettier",
        &[
            "prettier.config.cjs",
            "prettier.config.js",
            "prettier.config.mjs",
            "prettierignore",
            "prettierrc",
            "prettierrc.cjs",
            "prettierrc.js",
            "prettierrc.json",
            "prettierrc.json5",
            "prettierrc.mjs",
            "prettierrc.toml",
            "prettierrc.yaml",
            "prettierrc.yml",
        ],
    ),
    ("prisma", &["prisma"]),
    ("puppet", &["pp"]),
    ("python", &["py"]),
    ("r", &["r", "R"]),
    ("react", &["cjsx", "ctsx", "jsx", "mjsx", "mtsx", "tsx"]),
    ("roc", &["roc"]),
    ("ruby", &["rb"]),
    ("rust", &["rs"]),
    ("sass", &["sass", "scss"]),
    ("scala", &["scala", "sc"]),
    ("settings", &["conf", "ini"]),
    ("solidity", &["sol"]),
    (
        "storage",
        &[
            "accdb", "csv", "dat", "db", "dbf", "dll", "fmp", "fp7", "frm", "gdb", "ib", "ldf",
            "mdb", "mdf", "myd", "myi", "pdb", "psv", "RData", "rdata", "sav", "sdf", "sql",
            "sqlite", "ssv", "tsv",
        ],
    ),
    (
        "stylelint",
        &[
            "stylelint.config.cjs",
            "stylelint.config.js",
            "stylelint.config.mjs",
            "stylelintignore",
            "stylelintrc",
            "stylelintrc.cjs",
            "stylelintrc.js",
            "stylelintrc.json",
            "stylelintrc.mjs",
            "stylelintrc.yaml",
            "stylelintrc.yml",
        ],
    ),
    ("surrealql", &["surql"]),
    ("svelte", &["svelte"]),
    ("swift", &["swift"]),
    ("tcl", &["tcl"]),
    ("template", &["hbs", "plist", "xml"]),
    (
        "terminal",
        &[
            "bash",
            "bash_aliases",
            "bash_login",
            "bash_logout",
            "bash_profile",
            "bashrc",
            "brushrc",
            "fish",
            "nu",
            "profile",
            "ps1",
            "sh",
            "zlogin",
            "zlogout",
            "zprofile",
            "zsh",
            "zsh_aliases",
            "zsh_histfile",
            "zsh_history",
            "zshenv",
            "zshrc",
        ],
    ),
    ("terraform", &["tf", "tfvars"]),
    ("toml", &["toml"]),
    ("typescript", &["cts", "mts", "ts"]),
    ("v", &["v", "vsh", "vv"]),
    (
        "vcs",
        &[
            "COMMIT_EDITMSG",
            "EDIT_DESCRIPTION",
            "MERGE_MSG",
            "NOTES_EDITMSG",
            "TAG_EDITMSG",
            "gitattributes",
            "gitignore",
            "gitkeep",
            "gitmodules",
        ],
    ),
    ("vbproj", &["vbproj"]),
    ("video", &["avi", "m4v", "mkv", "mov", "mp4", "webm", "wmv"]),
    ("vs_sln", &["sln"]),
    ("vs_suo", &["suo"]),
    ("vue", &["vue"]),
    ("vyper", &["vy", "vyi"]),
    ("wgsl", &["wgsl"]),
    ("yaml", &["yaml", "yml"]),
    ("zig", &["zig"]),
];

/// A mapping of a file type identifier to its corresponding icon.
const FILE_ICONS: &[(&str, &str)] = &[
    ("astro", "icons/file_icons/astro.svg"),
    ("audio", "icons/file_icons/audio.svg"),
    ("ballerina", "icons/file_icons/ballerina.svg"),
    ("bicep", "icons/file_icons/file.svg"),
    ("bun", "icons/file_icons/bun.svg"),
    ("c", "icons/file_icons/c.svg"),
    ("cairo", "icons/file_icons/cairo.svg"),
    ("code", "icons/file_icons/code.svg"),
    ("coffeescript", "icons/file_icons/coffeescript.svg"),
    ("cpp", "icons/file_icons/cpp.svg"),
    ("crystal", "icons/file_icons/file.svg"),
    ("csharp", "icons/file_icons/file.svg"),
    ("csproj", "icons/file_icons/file.svg"),
    ("css", "icons/file_icons/css.svg"),
    ("cue", "icons/file_icons/file.svg"),
    ("dart", "icons/file_icons/dart.svg"),
    ("default", "icons/file_icons/file.svg"),
    ("diff", "icons/file_icons/diff.svg"),
    ("docker", "icons/file_icons/docker.svg"),
    ("document", "icons/file_icons/book.svg"),
    ("editorconfig", "icons/file_icons/editorconfig.svg"),
    ("elixir", "icons/file_icons/elixir.svg"),
    ("elm", "icons/file_icons/elm.svg"),
    ("erlang", "icons/file_icons/erlang.svg"),
    ("eslint", "icons/file_icons/eslint.svg"),
    ("font", "icons/file_icons/font.svg"),
    ("fsharp", "icons/file_icons/fsharp.svg"),
    ("fsproj", "icons/file_icons/file.svg"),
    ("gitlab", "icons/file_icons/gitlab.svg"),
    ("gleam", "icons/file_icons/gleam.svg"),
    ("go", "icons/file_icons/go.svg"),
    ("graphql", "icons/file_icons/graphql.svg"),
    ("haskell", "icons/file_icons/haskell.svg"),
    ("hcl", "icons/file_icons/hcl.svg"),
    ("helm", "icons/file_icons/helm.svg"),
    ("heroku", "icons/file_icons/heroku.svg"),
    ("html", "icons/file_icons/html.svg"),
    ("image", "icons/file_icons/image.svg"),
    ("ipynb", "icons/file_icons/jupyter.svg"),
    ("java", "icons/file_icons/java.svg"),
    ("javascript", "icons/file_icons/javascript.svg"),
    ("json", "icons/file_icons/code.svg"),
    ("julia", "icons/file_icons/julia.svg"),
    ("kdl", "icons/file_icons/kdl.svg"),
    ("kotlin", "icons/file_icons/kotlin.svg"),
    ("lock", "icons/file_icons/lock.svg"),
    ("log", "icons/file_icons/info.svg"),
    ("lua", "icons/file_icons/lua.svg"),
    ("luau", "icons/file_icons/luau.svg"),
    ("markdown", "icons/file_icons/book.svg"),
    ("metal", "icons/file_icons/metal.svg"),
    ("nim", "icons/file_icons/nim.svg"),
    ("nix", "icons/file_icons/nix.svg"),
    ("ocaml", "icons/file_icons/ocaml.svg"),
    ("odin", "icons/file_icons/odin.svg"),
    ("phoenix", "icons/file_icons/phoenix.svg"),
    ("php", "icons/file_icons/php.svg"),
    ("prettier", "icons/file_icons/prettier.svg"),
    ("prisma", "icons/file_icons/prisma.svg"),
    ("puppet", "icons/file_icons/puppet.svg"),
    ("python", "icons/file_icons/python.svg"),
    ("r", "icons/file_icons/r.svg"),
    ("react", "icons/file_icons/react.svg"),
    ("roc", "icons/file_icons/roc.svg"),
    ("ruby", "icons/file_icons/ruby.svg"),
    ("rust", "icons/file_icons/rust.svg"),
    ("sass", "icons/file_icons/sass.svg"),
    ("scala", "icons/file_icons/scala.svg"),
    ("settings", "icons/file_icons/settings.svg"),
    ("solidity", "icons/file_icons/file.svg"),
    ("storage", "icons/file_icons/database.svg"),
    ("stylelint", "icons/file_icons/javascript.svg"),
    ("surrealql", "icons/file_icons/surrealql.svg"),
    ("svelte", "icons/file_icons/html.svg"),
    ("swift", "icons/file_icons/swift.svg"),
    ("tcl", "icons/file_icons/tcl.svg"),
    ("template", "icons/file_icons/html.svg"),
    ("terminal", "icons/file_icons/terminal.svg"),
    ("terraform", "icons/file_icons/terraform.svg"),
    ("toml", "icons/file_icons/toml.svg"),
    ("typescript", "icons/file_icons/typescript.svg"),
    ("v", "icons/file_icons/v.svg"),
    ("vbproj", "icons/file_icons/file.svg"),
    ("vcs", "icons/file_icons/git.svg"),
    ("video", "icons/file_icons/video.svg"),
    ("vs_sln", "icons/file_icons/file.svg"),
    ("vs_suo", "icons/file_icons/file.svg"),
    ("vue", "icons/file_icons/vue.svg"),
    ("vyper", "icons/file_icons/vyper.svg"),
    ("wgsl", "icons/file_icons/wgsl.svg"),
    ("yaml", "icons/file_icons/yaml.svg"),
    ("zig", "icons/file_icons/zig.svg"),
];

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
    chevron_icons: ChevronIcons {
        collapsed: Some("icons/file_icons/chevron_right.svg"),
        expanded: Some("icons/file_icons/chevron_down.svg"),
    },
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

pub fn get_chevron_icon(expanded: bool) -> &'static str {
    let icons = &default_icon_theme().chevron_icons;
    if expanded {
        icons.expanded.unwrap_or("icons/file_icons/chevron_down.svg")
    } else {
        icons.collapsed.unwrap_or("icons/file_icons/chevron_right.svg")
    }
}

/// Resolves what a panel should draw ahead of a directory's name. Shared by every
/// panel that exposes a `folder_indicator` setting so they stay in agreement.
pub fn get_folder_indicators(indicator: FolderIndicator, expanded: bool) -> FolderIndicators {
    let chevron = indicator
        .shows_chevron()
        .then(|| Some(get_chevron_icon(expanded)))
        .flatten();
    let icon = indicator
        .shows_icon()
        .then(|| Some(get_generic_folder_icon(expanded)))
        .flatten();

    FolderIndicators { chevron, icon }
}

// ---------------------------------------------------------------------------
// 测试 —— 文件夹指示器测试搬自 file_icons.rs（gpui::test 改纯单测，默认
// 主题静态可用）；匹配测试按数据表写代表性断言
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_folder_indicators_per_setting() {
        let icon_only = get_folder_indicators(FolderIndicator::Icon, false);
        assert_eq!(icon_only.chevron, None);
        assert_eq!(
            icon_only.icon,
            Some("icons/file_icons/folder.svg"),
            "`icon` should draw the folder icon and no chevron"
        );

        let chevron_only = get_folder_indicators(FolderIndicator::Chevron, false);
        assert_eq!(
            chevron_only.chevron,
            Some("icons/file_icons/chevron_right.svg")
        );
        assert_eq!(
            chevron_only.icon, None,
            "`chevron` should draw the chevron and no folder icon"
        );

        let both = get_folder_indicators(FolderIndicator::Both, false);
        assert_eq!(both.chevron, Some("icons/file_icons/chevron_right.svg"));
        assert_eq!(both.icon, Some("icons/file_icons/folder.svg"));
    }

    #[test]
    fn test_folder_indicators_reflect_expanded_state() {
        let collapsed = get_folder_indicators(FolderIndicator::Both, false);
        assert_eq!(
            collapsed.chevron,
            Some("icons/file_icons/chevron_right.svg")
        );
        assert_eq!(collapsed.icon, Some("icons/file_icons/folder.svg"));

        let expanded = get_folder_indicators(FolderIndicator::Both, true);
        assert_eq!(expanded.chevron, Some("icons/file_icons/chevron_down.svg"));
        assert_eq!(expanded.icon, Some("icons/file_icons/folder_open.svg"));
    }

    #[test]
    fn test_folder_indicator_default_is_icon() {
        assert_eq!(
            get_folder_indicators(FolderIndicator::default(), false),
            get_folder_indicators(FolderIndicator::Icon, false),
            "the default must stay `icon` so existing users see no change"
        );
    }

    fn icon_of(name: &str) -> &'static str {
        get_icon(&PathBuf::from(name))
    }

    #[test]
    fn test_get_icon_matching() {
        // 扩展名直配
        assert_eq!(icon_of("main.rs"), "icons/file_icons/rust.svg");
        assert_eq!(icon_of("app.ts"), "icons/file_icons/typescript.svg");
        assert_eq!(icon_of("app.tsx"), "icons/file_icons/react.svg");
        assert_eq!(icon_of("README.md"), "icons/file_icons/book.svg");
        // stems：完整文件名（含点文件名）优先
        assert_eq!(icon_of("Dockerfile"), "icons/file_icons/docker.svg");
        // suffixes：文件名级（eslint.config.js 走文件名而不是 .js）
        assert_eq!(icon_of("eslint.config.js"), "icons/file_icons/eslint.svg");
        // 多重扩展名（Component.stories.tsx 语义：stories.tsx 无专属表项，
        // 逐段后缀回退命中 tsx）
        assert_eq!(icon_of("Button.stories.tsx"), "icons/file_icons/react.svg");
        // 隐藏文件名：.gitignore → vcs
        assert_eq!(icon_of(".gitignore"), "icons/file_icons/git.svg");
        // `.data.json` 语义：隐藏文件带正常扩展名
        assert_eq!(icon_of(".eslintrc.json"), "icons/file_icons/eslint.svg");
        // 兜底 default
        assert_eq!(icon_of("unknownxyz.zzz"), "icons/file_icons/file.svg");
    }
}
