//! PF 官方插件公域扫描（docs/插件分发规范.md §2）：**无内置索引**。
//!
//! 数据源 = npm registry 上的清单包 `@pi-flash-plugins/market`：随插件发版
//! 由插件仓 `npm run dist` 重新组包发布。扫描链（全是 registry 直查，无搜索
//! 依赖——npm 搜索索引对新包有小时级时滞，且 scope 名会被模糊匹配淹没）：
//!
//! 1. `GET /@pi-flash-plugins%2fmarket/latest` → 清单包版本 + tarball URL
//! 2. 下载 tarball → 系统 tar 解到临时目录 → 读 `manifest.json`
//! 3. 逐包 `GET /<name>/latest` → `dist.integrity` + `dist.tarball`
//!    → 构造 [`IndexEntry`]（`source = "url"`，registry 直连下载）
//!
//! 准入铁律：清单内且 integrity 齐全的包才进市场；scope 之外一律拒装由
//! 清单包本身保证（它只由组织成员发包权更新）。

use crate::pf_plugins::{IndexDisplay, IndexEntry, MarketIndex};

/// 官方 scope 与清单包名。
pub const PF_SCOPE: &str = "@pi-flash-plugins";
const MARKET_PKG: &str = "@pi-flash-plugins/market";
const MAX_TARBALL: u64 = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// registry 端点
// ---------------------------------------------------------------------------

/// scoped 包名的 registry 路径段：`/` → `%2f`（小写，registry 两种都收）。
fn enc(name: &str) -> String {
    name.replace('/', "%2f")
}

/// 清单包 latest 元数据端点。
pub fn manifest_pkg_url() -> String {
    format!(
        "{}/{}/latest",
        crate::market::registry_base(),
        enc(MARKET_PKG)
    )
}

/// 单包 latest 元数据端点。
pub fn pkg_url(name: &str) -> String {
    format!("{}/{}/latest", crate::market::registry_base(), enc(name))
}

// ---------------------------------------------------------------------------
// 解析（纯函数，单测主战场）
// ---------------------------------------------------------------------------

/// registry packument `dist` 字段。
#[derive(Default, serde::Deserialize)]
pub struct Dist {
    #[serde(default)]
    integrity: String,
    #[serde(default)]
    tarball: String,
}

/// 清单包 `manifest.json`（插件仓 dist 链生成）。
#[derive(Default, serde::Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub packages: Vec<ManifestEntry>,
}

#[derive(Default, serde::Deserialize)]
pub struct ManifestEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub display: Option<IndexDisplay>,
    #[serde(default)]
    pub desc: Option<IndexDisplay>,
    /// 市场卡片预览图（包内资产相对路径，如 "assets/w.svg"）。
    #[serde(default)]
    pub icon: Option<String>,
}

/// 从清单条目构造市场条目（integrity/tarball 来自 registry packument，
/// 清单本身不携带完整性值——不信任包自带值）。
pub fn entry_from(m: &ManifestEntry, integrity: String, tarball: String) -> Option<IndexEntry> {
    if !m.name.starts_with(&format!("{PF_SCOPE}/")) || integrity.is_empty() || tarball.is_empty() {
        return None; // scope 外 / 校验值缺失：一律不进市场
    }
    Some(IndexEntry {
        name: m.name.clone(),
        version: String::new(), // 由调用方从 packument 填
        integrity,
        min_pf: String::new(),
        display: m.display.clone().unwrap_or(IndexDisplay {
            zh: m
                .name
                .trim_start_matches(&format!("{PF_SCOPE}/"))
                .to_string(),
            en: String::new(),
        }),
        desc: m.desc.clone(),
        icon: m.icon.clone(),
        publisher: "Pi-Flash".into(),
        published_at: String::new(),
        source: "url".into(),
        url: tarball,
    })
}

/// packument（/latest 响应）→ (version, Dist)。
pub fn parse_latest(json: &str) -> Option<(String, Dist)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = v["version"].as_str()?.to_string();
    let dist: Dist = serde_json::from_value(v["dist"].clone()).ok()?;
    Some((version, dist))
}

/// tarball 内的清单 JSON → [`Manifest`]。
pub fn parse_manifest(json: &str) -> Result<Manifest, String> {
    serde_json::from_str(json).map_err(|e| format!("manifest.json 解析失败: {e}"))
}

// ---------------------------------------------------------------------------
// 扫描（网络）
// ---------------------------------------------------------------------------

/// 全量扫描：清单包 → 逐包 packument → 市场索引。任一环失败即整体失败
/// （调用方显示空清单——拉不到 = 空清单，无兜底）。
pub fn fetch() -> Result<MarketIndex, String> {
    let (mver, mdist) = {
        let raw = crate::market::http_get_json(&manifest_pkg_url())?;
        parse_latest(&raw).ok_or("清单包元数据缺 version/dist")?
    };
    if mver == "0.0.0" || mdist.tarball.is_empty() {
        return Err("清单包元数据不完整".into());
    }

    // 解包清单（复用 pf_plugins::extract 的系统 tar + strip 约定）
    let tmp = std::env::temp_dir().join("pf-market-manifest");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let result = (|| {
        let tgz = tmp.join("manifest-pkg.tgz");
        let bytes = crate::market::http_get_bytes(&mdist.tarball, MAX_TARBALL)?;
        std::fs::write(&tgz, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;
        let dir = tmp.join("x");
        crate::pf_plugins::extract(&tgz, &dir)?;
        let json = std::fs::read_to_string(dir.join("manifest.json"))
            .map_err(|e| format!("清单包缺 manifest.json: {e}"))?;
        parse_manifest(&json)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    let manifest = result?;

    // 逐包 packument（scope 包数量少，串行即可；共享 Agent 连接复用）
    let mut packages = Vec::new();
    for m in &manifest.packages {
        let Some((version, dist)) = parse_latest(&crate::market::http_get_json(&pkg_url(&m.name))?)
        else {
            eprintln!("[pf-market] {} 元数据不完整，跳过", m.name);
            continue;
        };
        match entry_from(m, dist.integrity, dist.tarball) {
            Some(mut e) => {
                e.version = version;
                packages.push(e);
            }
            None => eprintln!("[pf-market] {} 不满足准入（scope/integrity），跳过", m.name),
        }
    }
    Ok(MarketIndex { packages })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_latest_reads_version_and_dist() {
        let (v, d) = parse_latest(
            r#"{"version":"0.1.0","dist":{"integrity":"sha512-AAA=","tarball":"https://x/y.tgz"}}"#,
        )
        .unwrap();
        assert_eq!(v, "0.1.0");
        assert_eq!(d.integrity, "sha512-AAA=");
        assert_eq!(d.tarball, "https://x/y.tgz");
    }

    #[test]
    fn entry_requires_scope_and_integrity() {
        let m = ManifestEntry {
            name: "evil/pkg".into(),
            display: None,
            desc: None,
            icon: None,
        };
        assert!(entry_from(&m, "sha512-A".into(), "https://t".into()).is_none());
        let m = ManifestEntry {
            name: "@pi-flash-plugins/quick-panel".into(),
            display: None,
            desc: None,
            icon: None,
        };
        assert!(entry_from(&m, String::new(), "https://t".into()).is_none());
        let e = entry_from(&m, "sha512-A".into(), "https://t".into()).unwrap();
        assert_eq!(e.display.zh, "quick-panel"); // 缺 display 时回退 slug
        assert_eq!(e.source, "url");
    }

    #[test]
    fn parse_manifest_tolerates_missing_fields() {
        let m = parse_manifest(
            r#"{"schema":"pf-market/1","packages":[{"name":"@pi-flash-plugins/a"}]}"#,
        )
        .unwrap();
        assert_eq!(m.packages.len(), 1);
        assert!(parse_manifest("not json").is_err());
    }

    #[test]
    fn manifest_entry_icon_passthrough() {
        let m = parse_manifest(
            r#"{"schema":"pf-market/1","packages":[{"name":"@pi-flash-plugins/w","icon":"assets/w.svg"}]}"#,
        )
        .unwrap();
        assert_eq!(m.packages[0].icon.as_deref(), Some("assets/w.svg"));
        let e = entry_from(&m.packages[0], "sha512-x".into(), "https://t/w.tgz".into()).unwrap();
        assert_eq!(e.icon.as_deref(), Some("assets/w.svg"));
    }
}
