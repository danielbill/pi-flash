//! Windows 可执行资源：品牌图标嵌进 pi-flash.exe（资源管理器 / 任务栏 / 窗口图标）。
//! 必须用编译期 `#[cfg(windows)]` 门控（而非运行时判断）：winresource 只在
//! `[target.'cfg(windows)'.build-dependencies]` 里，macOS CI 未链接该 crate，
//! 运行时 if 挡不住编译期 E0433。macOS 图标走 .app bundle 的
//! assets/macos/pi-flash.icns（见 .github/workflows/release-macos.yml）。

#[cfg(windows)]
fn main() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon/pi-flash.ico");
    res.set("FileDescription", "pi-flash");
    res.set("ProductName", "pi-flash");
    res.compile().expect("compile windows resource (brand icon)");
}

#[cfg(not(windows))]
fn main() {}
