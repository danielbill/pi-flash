//! Windows 可执行资源：品牌图标嵌进 pi-flash.exe（资源管理器 / 任务栏 / 窗口图标）。
//! 非 Windows 目标（macOS CI）不执行任何资源编译；macOS 图标走 .app bundle 的
//! assets/macos/pi-flash.icns（见 .github/workflows/release-macos.yml）。

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/pi-flash.ico");
        res.set("FileDescription", "pi-flash");
        res.set("ProductName", "pi-flash");
        res.compile().expect("compile windows resource (brand icon)");
    }
}
