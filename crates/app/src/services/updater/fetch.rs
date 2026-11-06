//! GitHub release 抓取与 last_seen 记账（081）：release API / 候选下载源
//! 装配 / 更新日志按 tag 现拉 / last_seen 与更新日志缓存。自 updater.rs
//! 拆出（090 重构）。itlq.9 超规拆分后下载引擎与进度通道在 `download`、
//! 落位管线在 `stage`。

use super::*;
use std::time::Duration;

// ---------------------------------------------------------------------------
// release 拉取
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct ReleaseInfo {
    pub version: String,
    pub body: String,
}

pub(crate) fn api_base() -> String {
    std::env::var("PI_FLASH_RELEASES_API")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RELEASES_API_BASE.to_string())
}

/// 候选下载源：`PI_FLASH_MIRROR` 指定唯一源（跳过测速切换）；否则直连优先 +
/// `PI_FLASH_MIRRORS` 追加 + 默认镜像（与 npm/bin/lib.js downloadBases 同规则）。
pub(crate) fn download_bases() -> Vec<String> {
    if let Ok(m) = std::env::var("PI_FLASH_MIRROR") {
        let m = m.trim().trim_end_matches('/').to_string();
        if !m.is_empty() {
            return vec![m];
        }
    }
    let extra = std::env::var("PI_FLASH_MIRRORS")
        .map(|s| {
            s.split([',', ' ', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut bases = vec![RELEASES_DOWNLOAD_BASE.to_string()];
    bases.extend(extra);
    bases.extend(DEFAULT_MIRRORS.iter().map(|s| s.to_string()));
    bases
}

pub(crate) fn check_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(CHECK_TIMEOUT).build()
}

/// latest release（含 body = 更新日志 markdown）。tag 形如 v1.2.3。timeout
/// 由调用方定：更新检查走设计文档的 30s，更新日志现拉走 15s。
pub(crate) fn fetch_release(url: &str, timeout: Duration) -> Result<ReleaseInfo, String> {
    let resp = ureq::AgentBuilder::new()
        .timeout(timeout)
        .build()
        .get(url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("{e}"))?;
    // 注意：ureq 2 的 into_string 返回 io::Result（与 call 的 ureq::Error 不同型）
    let text = resp.into_string().map_err(|e| format!("{e}"))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("release 缺 tag_name")?;
    Ok(ReleaseInfo {
        version: version_key_string(tag),
        body: json
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

pub(crate) fn fetch_latest() -> Result<ReleaseInfo, String> {
    fetch_release(&format!("{}/latest", api_base()), CHECK_TIMEOUT)
}

/// 按 tag 拉更新日志（更新日志页缓存缺失时的现拉通道）。15s 超时：被墙/
/// 断网时尽快落「暂时无法连接 GitHub」提示，不让用户对着转圈干等。
pub(crate) fn fetch_release_body(version: &str) -> Result<String, String> {
    let url = format!("{}/tags/v{}", api_base(), version_key_string(version));
    fetch_release(&url, CHANGELOG_TIMEOUT).map(|r| r.body)
}

// ---------------------------------------------------------------------------
// last_seen（新版本首启判定）
// ---------------------------------------------------------------------------

pub(crate) fn read_last_seen() -> Option<String> {
    let raw = std::fs::read_to_string(pi_link::paths::update_state_file()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("last_seen_version")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

pub(crate) fn write_last_seen(version: &str) {
    let Some(p) = pi_link::paths::update_state_file() else {
        return;
    };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = pi_link::config::write_json(&p, &serde_json::json!({ "last_seen_version": version }));
}

pub(crate) fn read_changelog_cache(version: &str) -> Option<String> {
    std::fs::read_to_string(pi_link::paths::changelog_cache_file(version)?)
        .ok()
        .filter(|s| !s.trim().is_empty())
}
