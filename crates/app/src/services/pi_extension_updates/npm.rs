//! npm 侧探测原语（pi-web npm-source.ts / plugin-updates.ts 手写版，不用
//! 正则 crate）：来源串解析 / 实装版本读取 / registry 直连（替代 `npm view`
//! 子进程）。自 pi_extension_updates.rs 切出（itlq.9 超规拆分，纯结构搬移）。

use std::path::Path;
use std::time::Duration;

/// registry 直连超时（一个 GET，比子进程宽松没意义）。
pub(crate) const REGISTRY_TIMEOUT: Duration = Duration::from_secs(8);
const REGISTRY_DEFAULT: &str = "https://registry.npmjs.org";
const UA: &str = "pi-flash-updater";

pub(crate) struct NpmSource {
    /// 包名（registry 路径用；scoped 形如 `@scope/pkg`）
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

/// `npm:@scope/pkg@1.2.3` → spec=`@scope/pkg@1.2.3`，version=`1.2.3`。
/// pi-web parseNpmSource 正则 `^(@?[^@]+(?:\/[^@]+)?)(?:@(.+))?$` 的手写版。
pub(crate) fn parse_npm_source(source: &str) -> Option<NpmSource> {
    let spec = source.strip_prefix("npm:")?.trim();
    let name_end = match spec.strip_prefix('@') {
        // scoped：跳过 `@scope/`，版本 @ 从包名段之后找
        Some(rest) => match rest.find('/') {
            Some(i) => i + 2,
            None => spec.len(),
        },
        None => 0,
    };
    let (name, version) = match spec[name_end.min(spec.len())..].find('@') {
        Some(i) => (
            &spec[..name_end + i],
            Some(spec[name_end + i + 1..].to_string()),
        ),
        None => (spec, None),
    };
    (!name.is_empty()).then(|| NpmSource {
        name: name.to_string(),
        version,
    })
}

pub(crate) fn read_installed_version(dir: &Path) -> Result<semver::Version, String> {
    let text = std::fs::read_to_string(dir.join("package.json"))
        .map_err(|_| "not installed".to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid package.json: {e}"))?;
    let raw = v
        .get("version")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "installed package has no version".to_string())?;
    semver::Version::parse(raw).map_err(|_| format!("invalid installed version {raw:?}"))
}

/// 拉取 registry 元数据（裁剪文档）：`GET <registry>/<name>`，scoped 包把 `/`
/// 编成 `%2F`。基础地址默认 `registry.npmjs.org`，可用
/// `PI_FLASH_NPM_REGISTRY` 覆盖（私有镜像/代理）；`.npmrc` 不读。
pub(crate) fn registry_doc(name: &str, timeout: Duration) -> Result<serde_json::Value, String> {
    let base = std::env::var("PI_FLASH_NPM_REGISTRY")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| REGISTRY_DEFAULT.to_string());
    let url = format!("{base}/{}", name.replace('/', "%2F"));
    // Accept 取 install-v1 裁剪文档（实测 pi-mcp-adapter 全文 115 KB，够用）
    let resp = ureq::AgentBuilder::new()
        .timeout(timeout)
        .build()
        .get(&url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.npm.install-v1+json")
        .call()
        .map_err(|e| format!("{e}"))?;
    // ureq 2 的 into_string 返回 io::Result（与 call 的 ureq::Error 不同型）
    let text = resp.into_string().map_err(|e| format!("{e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("unexpected registry response: {e}"))
}

/// registry 文档 → 目标版本。`want` 的三种语义（对齐 `npm view <spec> version`）：
/// - `None`：`dist-tags.latest`（不拿全量 keys 取最大，否则预发布版会误报）；
/// - semver 范围（`^1.2.3`）：`dist-tags.latest` 命中则用，否则在 `versions`
///   里取满足范围的最大值（maxSatisfying）；
/// - 其它（`beta`/`next` 等 dist-tag）：直查 `dist-tags.<want>`。
/// 钉死精确版本已在 `check_one` 归 unsupported，不会走到这。
pub(crate) fn latest_from_registry(
    doc: &serde_json::Value,
    want: Option<&str>,
) -> Result<semver::Version, String> {
    let tag = |name: &str| -> Option<semver::Version> {
        doc.get("dist-tags")
            .and_then(|t| t.get(name))
            .and_then(|v| v.as_str())
            .and_then(|s| semver::Version::parse(s).ok())
    };
    let Some(want) = want else {
        return tag("latest").ok_or_else(|| "registry has no dist-tags.latest".to_string());
    };
    match semver::VersionReq::parse(want) {
        Ok(req) => {
            if let Some(v) = tag("latest") {
                if req.matches(&v) {
                    return Ok(v);
                }
            }
            let mut best: Option<semver::Version> = None;
            if let Some(map) = doc.get("versions").and_then(|v| v.as_object()) {
                for key in map.keys() {
                    if let Ok(v) = semver::Version::parse(key) {
                        if req.matches(&v) && best.as_ref().map_or(true, |b| v > *b) {
                            best = Some(v);
                        }
                    }
                }
            }
            best.ok_or_else(|| format!("no published version matches {want}"))
        }
        // 不是范围 → 当 dist-tag 查（`npm:pkg@beta`）
        Err(_) => tag(want).ok_or_else(|| format!("registry has no dist-tag {want:?}")),
    }
}
