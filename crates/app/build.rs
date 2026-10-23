//! Windows 可执行资源：品牌图标嵌进 pi-flash.exe（资源管理器 / 任务栏 / 窗口图标）。
//! 必须用编译期 `#[cfg(windows)]` 门控（而非运行时判断）：winresource 只在
//! `[target.'cfg(windows)'.build-dependencies]` 里，macOS CI 未链接该 crate，
//! 运行时 if 挡不住编译期 E0433。macOS 图标走 .app bundle 的
//! assets/macos/pi-flash.icns（见 .github/workflows/release-macos.yml）。
//!
//! 另承担 090-5 L3 官方宿主证明注入（纯机制，无秘密）：构建机由发布流水线先跑
//! 插件仓 `inner/scripts/proof.mjs` 产出已签 proof（紧凑 JSON），经环境变量传入；
//! 本脚本只做 env → 二进制常量，**不做任何密码学、不接触私钥**。

fn main() {
    inject_host_proof();
    embed_windows_icon();
}

/// 090-5 L3：`PF_BUILD_PROOF`（已签 proof，紧凑 JSON `{kid,payload,proof}`）与
/// `PF_BUILD_KID` → `PF_HOST_PROOF` / `PF_HOST_PROOF_KID` 常量。
/// 未设置的构建 = 占位符（`cargo:warning` 提示）：内层 wasm 插件验签必失败 → 拒载。
fn inject_host_proof() {
    println!("cargo:rerun-if-env-changed=PF_BUILD_PROOF");
    println!("cargo:rerun-if-env-changed=PF_BUILD_KID");
    match (
        std::env::var("PF_BUILD_PROOF"),
        std::env::var("PF_BUILD_KID"),
    ) {
        (Ok(proof), Ok(kid)) if !proof.is_empty() && !kid.is_empty() => {
            println!("cargo:rustc-env=PF_HOST_PROOF={proof}");
            println!("cargo:rustc-env=PF_HOST_PROOF_KID={kid}");
        }
        _ => {
            println!("cargo:rustc-env=PF_HOST_PROOF=PROOF_PLACEHOLDER");
            println!("cargo:rustc-env=PF_HOST_PROOF_KID=");
            println!(
                "cargo:warning=PF_BUILD_PROOF/PF_BUILD_KID 未设置：本次构建不含官方宿主证明（占位符）。\
                 内层 wasm 插件将拒绝加载（090-5 L3）。官方构建需先在插件仓 inner/ 跑 scripts/proof.mjs。"
            );
        }
    }
}

#[cfg(windows)]
fn embed_windows_icon() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon/pi-flash.ico");
    res.set("FileDescription", "pi-flash");
    res.set("ProductName", "pi-flash");
    res.compile()
        .expect("compile windows resource (brand icon)");
}

#[cfg(not(windows))]
fn embed_windows_icon() {}
