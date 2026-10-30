//! Windows 可执行资源：品牌图标嵌进 pi-flash.exe（资源管理器 / 任务栏 / 窗口图标）。
//! 必须用编译期 `#[cfg(windows)]` 门控（而非运行时判断）：winresource 只在
//! `[target.'cfg(windows)'.build-dependencies]` 里，macOS CI 未链接该 crate，
//! 运行时 if 挡不住编译期 E0433。macOS 图标走 .app bundle 的
//! assets/macos/pi-flash.icns（见 .github/workflows/release-macos.yml）。
//!
//! 另承担 090-5 L3 官方宿主证明注入（纯机制，无秘密）：构建机由发布流水线先产出
//! 已签 proof（紧凑 JSON），经环境变量传入；本脚本只做 env → 二进制常量，
//! **不做任何密码学、不接触签名材料**。签发流程见内部发布文档。

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
            // 开发构建自签：本机有 dev 签名材料就现场签一份（**仅 debug**）
            if let Some((kid, proof)) = dev_sign_proof() {
                println!("cargo:rustc-env=PF_HOST_PROOF={proof}");
                println!("cargo:rustc-env=PF_HOST_PROOF_KID={kid}");
                println!("cargo:warning=开发构建已自带宿主证明（kid={kid}）");
                return;
            }
            println!("cargo:rustc-env=PF_HOST_PROOF=PROOF_PLACEHOLDER");
            println!("cargo:rustc-env=PF_HOST_PROOF_KID=");
            println!(
                "cargo:warning=本次构建不含官方宿主证明（占位符）：内层 wasm 插件将拒绝加载。\
                 官方构建需由发布流水线注入（见内部发布文档）。"
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

/// 开发构建自签宿主证明（**仅 debug**）：调用内部签发脚本，用本机开发签名材料现签。
/// 路径与变量名见内部发布文档（不入库）；材料或脚本缺失 → None（回退占位符）。
///
/// 意义：日常 `cargo run -p app` 就能装内层插件，不必每次手拼环境变量。
/// release 构建不自动签（`PROFILE != "debug"`），发布包仍由流水线显式注入。
fn dev_sign_proof() -> Option<(String, String)> {
    println!("cargo:rerun-if-env-changed=PF_DEV_KEY");
    println!("cargo:rerun-if-env-changed=PF_DEV_KID");
    if std::env::var("PROFILE").ok().as_deref() != Some("debug") {
        return None;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let kid = std::env::var("PF_DEV_KID").unwrap_or_else(|_| "dev-2026".into());
    let key = std::env::var("PF_DEV_KEY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            root.join("../pi-flash-plugins/inner/keys/priv")
                .join(format!("{kid}.pk8.pem"))
        });
    let script = root.join("../pi-flash-plugins/inner/scripts/proof.mjs");
    if !key.is_file() || !script.is_file() {
        return None;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let out = std::process::Command::new("node")
        .arg(&script)
        .args([
            "sign",
            "--kid",
            &kid,
            "--build-id",
            "dev-local",
            "--version",
            &version,
            "--json",
        ])
        .env("PF_BUILD_KEY", &key)
        .output()
        .ok()?;
    if !out.status.success() {
        println!(
            "cargo:warning=dev proof 签名失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    Some((
        v.get("kid")?.as_str()?.to_string(),
        v.get("proof")?.as_str()?.to_string(),
    ))
}
