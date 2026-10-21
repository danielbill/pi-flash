//! 系统 shell 打开器（无 UI 自由函数）：系统浏览器打开 html/htm、资源
//! 管理器打开目录、文件定位选中（explorer /select, / macOS open -R）。
//! Windows + macOS 双实现。
//! （P2 归并：actions_panels / actions_files 的 OS 打开器。）

use std::path::Path;

/// 用系统默认浏览器打开文件（html/htm）。
pub(crate) fn open_in_browser(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

/// 用系统默认浏览器打开 URL（040-2 市场页的 npm/repo/report 链接）。
pub(crate) fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
}

/// 用系统默认应用打开文件（editor「无法打开」灰屏的兜底动作）：cmd start /
/// macOS open 本就按文件关联选应用，与 open_in_browser 同实现异名——
/// 调用点语义是「默认应用」而非「浏览器」。
pub(crate) fn open_in_default_app(path: &Path) {
    open_in_browser(path);
}

/// 在系统文件浏览器中打开目录（Windows explorer / macOS open）。
pub(crate) fn open_in_explorer(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
}

/// 「在资源管理器中打开」的文件语义（与 open_in_explorer 的目录语义分
/// 开）：文件 = 打开所在目录并选中（explorer /select, / macOS open -R），
/// 目录 = 直接进入。
pub(crate) fn reveal_in_explorer(path: &Path) {
    #[cfg(windows)]
    {
        if path.is_dir() {
            let _ = std::process::Command::new("explorer").arg(path).spawn();
        } else {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if path.is_dir() {
            let _ = std::process::Command::new("open").arg(path).spawn();
        } else {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .spawn();
        }
    }
}
