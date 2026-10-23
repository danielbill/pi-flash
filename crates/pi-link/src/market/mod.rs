//! 市场域地基（040-1）：后续会有多个「远端目录 + 一键安装」的市场页，
//! 每个市场一个 `<来源>_market.rs` 并列在本模块下（v1 只有 [`pi_ext_market`]：
//! Pi 扩展市场，数据源 = npm registry）。
//!
//! 本模块只管各市场共用的东西：
//! - registry base（`PI_FLASH_NPM_REGISTRY` 可覆盖，墙内可指镜像）；
//! - 共享 `ureq` Agent（keep-alive + gzip：实测不复用连接 50 包要 6.0s，
//!   复用后 2.6s）；
//! - HTTP 取 JSON 与失败分级（一律 `Result<String, String>`，错误串直接上 UI）；
//! - 离线判定（`PI_OFFLINE=1` 时不发任何请求，各市场只用本地快照）。
//!
//! 铁律：这里不碰 UI 状态、不写 pi 的 settings.json；安装/卸载/更新一律回落
//! 到 vendored pi CLI（`vendor::run_cli`，见 040/082 的既有管道）。

pub mod pi_ext_market;

use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

/// 默认 npm registry（末尾无 `/`）。
pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";

const HTTP_TIMEOUT_SECS: u64 = 15;
/// 响应体上限：目录端点不该超过这个量级（防异常大包塞内存）。
const MAX_BODY: u64 = 2 * 1024 * 1024;

/// npm registry base，末尾无 `/`。`PI_FLASH_NPM_REGISTRY` 可覆盖
/// （墙内镜像的 search 端点多半不可用，实测 npmmirror 返回 0 条，慎改）。
pub fn registry_base() -> String {
    registry_base_from(std::env::var("PI_FLASH_NPM_REGISTRY").ok())
}

/// 纯函数版（单测用）：空串/纯空白 = 未设置。
pub fn registry_base_from(env: Option<String>) -> String {
    env.filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string())
}

/// `PI_OFFLINE=1`（及 true/yes）→ 一切市场网络访问跳过。
pub fn offline() -> bool {
    std::env::var("PI_OFFLINE")
        .map(|v| {
            v.eq_ignore_ascii_case("1")
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// 各市场共用的 Agent：连接池 + gzip + 15s 超时。
/// `PI_FLASH_HTTP_PROXY`（http:// / socks5h://）可选——不改 Settings（铁律）。
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let mut b = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .user_agent(concat!("pi-flash/", env!("CARGO_PKG_VERSION")));
        if let Ok(p) = std::env::var("PI_FLASH_HTTP_PROXY") {
            let p = p.trim();
            if !p.is_empty()
                && let Ok(proxy) = ureq::Proxy::new(p)
            {
                b = b.proxy(proxy);
            }
        }
        b.build()
    })
}

/// GET 一个 JSON 端点，返回响应体文本（≤2MB，UTF-8 宽松解码）。
/// 错误串面向用户：`HTTP 404` / `network: ...` / `offline`。
/// GET 一个二进制端点，返回响应体字节（上限 max_bytes；tarball/包体下载用）。
pub fn http_get_bytes(url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    if offline() {
        return Err("offline".to_string());
    }
    let resp = agent().get(url).call().map_err(|e| match e {
        ureq::Error::Status(st, _) => format!("HTTP {st}"),
        other => format!("network: {other}"),
    })?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(max_bytes)
        .read_to_end(&mut buf)
        .map_err(|e| format!("network: {e}"))?;
    Ok(buf)
}

pub fn http_get_json(url: &str) -> Result<String, String> {
    if offline() {
        return Err("offline".to_string());
    }
    let resp = agent()
        .get(url)
        .set("accept", "application/json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(st, _) => format!("HTTP {st}"),
            other => format!("network: {other}"),
        })?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MAX_BODY)
        .read_to_end(&mut buf)
        .map_err(|e| format!("network: {e}"))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_base_override_and_fallback() {
        assert_eq!(registry_base_from(None), DEFAULT_REGISTRY);
        assert_eq!(registry_base_from(Some("   ".into())), DEFAULT_REGISTRY);
        assert_eq!(
            registry_base_from(Some(" https://registry.npmmirror.com/ ".into())),
            "https://registry.npmmirror.com"
        );
        assert_eq!(
            registry_base_from(Some("https://a.example/x/".into())),
            "https://a.example/x"
        );
    }
}
