use super::*;
use std::path::PathBuf;

#[test]
fn test_folder_icon_reflects_expanded_state() {
    assert_eq!(
        get_generic_folder_icon(false),
        "icons/file_icons/folder.svg",
        "collapsed dir draws the closed folder icon"
    );
    assert_eq!(
        get_generic_folder_icon(true),
        "icons/file_icons/folder_open.svg",
        "expanded dir draws the open folder icon"
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
