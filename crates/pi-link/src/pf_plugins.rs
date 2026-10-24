//! PF 插件域（090-4 v1 最小切片）：**纯声明式插件**的索引 / 台账 / 安装卸载。
//!
//! 数据流：内置索引（app 侧 include_str! 资产，唯一上架入口）→ 用户点安装 →
//! `npm pack <name>@<ver>` 拉 tarball → sha512 对账索引 integrity → 系统 tar
//! 解包到 `<数据目录>/plugins/<name>/<version>/` → 写台账 `plugins.json`。
//! 索引外的包一律拒装——「只能从市场获得」由这条校验链保证，不是 UI 隐藏。
//!
//! v1 明确不做：TS/sidecar 形态、远端索引、签名验签（090-5 L1 预留 integrity
//! 字段）、混淆、激活。npm 调用与宿主 plugin_updates 同款：内置 node 直跑
//! 内置 npm-cli.js（Windows 不裸 spawn npm，CVE-2024-27980）。

use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

// ---------------------------------------------------------------------------
// 市场索引（app 侧从内置资产解析）
// ---------------------------------------------------------------------------

/// 索引条目展示名（与 npm 包名解耦，UI 一律显示这里）。
#[derive(Deserialize, Clone, Debug)]
pub struct IndexDisplay {
    pub zh: String,
    #[serde(default)]
    pub en: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    /// SRI 格式：`sha512-<base64>`。安装时对真实 tarball 对账，不信任包自带值。
    pub integrity: String,
    /// 宿主 API 下限（`">=0.2"` 语义），app 侧用 semver 校验。
    #[serde(default)]
    pub min_pf: String,
    pub display: IndexDisplay,
    /// 一句话描述（列表卡片副标题；缺省 = 不渲染描述行）。
    #[serde(default)]
    pub desc: Option<IndexDisplay>,
    /// 发布人（v1 恒 "Pi-Flash"，第三方开放时为作者名）。
    #[serde(default)]
    pub publisher: String,
    /// 发布日期（ISO 字符串，直接展示）。
    #[serde(default)]
    pub published_at: String,
    /// 下载档：`"npm"`（默认，按 registry 约定隐式解析）| `"url"`（索引自描述路径，
    /// 直连拉取——私有渠道/内层 wasm 包用，090-3 §3.1 source 双档）。
    #[serde(default)]
    pub source: String,
    /// `source = "url"` 时的 tarball 直链（http/https）。
    #[serde(default)]
    pub url: String,
}

#[derive(Deserialize, Clone, Default)]
pub struct MarketIndex {
    #[serde(default)]
    pub packages: Vec<IndexEntry>,
}

impl MarketIndex {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("索引解析失败: {e}"))
    }

    pub fn entry(&self, name: &str) -> Option<&IndexEntry> {
        self.packages.iter().find(|p| p.name == name)
    }
}

// ---------------------------------------------------------------------------
// 台账（`<数据目录>/plugins.json`）
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LedgerEntry {
    pub name: String,
    pub version: String,
    pub integrity: String,
    #[serde(default)]
    pub installed_at: String,
    /// v1 恒为 "market"（唯一安装入口）。
    pub source: String,
    /// 090-4 v5：用户停用。停用的插件启动扫描跳过（不装载不绘制），运行态
    /// 由市场【已安装】页签的启停开关置位。缺省 false，旧台账直读兼容。
    #[serde(default)]
    pub disabled: bool,
}

/// 读台账；文件不存在/损坏 = 空台账（损坏不报错，下次安装覆盖写回）。
pub fn load_ledger() -> Vec<LedgerEntry> {
    let Some(f) = crate::paths::plugins_ledger_file() else {
        return Vec::new();
    };
    let Ok(s) = std::fs::read_to_string(f) else {
        return Vec::new();
    };
    serde_json::from_str(&s).unwrap_or_default()
}

fn save_ledger(entries: &[LedgerEntry]) -> Result<(), String> {
    let f = crate::paths::plugins_ledger_file().ok_or("无法定位插件台账路径")?;
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(f, s).map_err(|e| e.to_string())
}

fn ledger_upsert(entry: LedgerEntry) -> Result<(), String> {
    let mut all = load_ledger();
    all.retain(|e| e.name != entry.name);
    all.push(entry);
    save_ledger(&all)
}

fn ledger_remove(name: &str) -> Result<(), String> {
    let mut all = load_ledger();
    all.retain(|e| e.name != name);
    save_ledger(&all)
}

/// 某插件已装版本（未装 = None）。
pub fn installed_version(name: &str) -> Option<String> {
    load_ledger()
        .into_iter()
        .find(|e| e.name == name)
        .map(|e| e.version)
}

/// 启停落盘（090-4 v5）：只翻台账的 disabled 位；台账缺该包 = Err。
/// 停用语义在消费端——启动扫描跳过、运行态 entry.disabled 置位后
/// 状态栏 tab / 文件树插件菜单 / 分发全部不再出现该插件。
pub fn set_disabled(name: &str, disabled: bool) -> Result<(), String> {
    let mut all = load_ledger();
    let Some(entry) = all.iter_mut().find(|e| e.name == name) else {
        return Err(format!("台账中无插件 {name}"));
    };
    if entry.disabled == disabled {
        return Ok(());
    }
    entry.disabled = disabled;
    save_ledger(&all)
}

// ---------------------------------------------------------------------------
// 签名索引（090-3 §3.2 双轨后半：私域索引自动获取，**只认签名**）
// ---------------------------------------------------------------------------

/// 私域索引 URL（环境变量 `PI_FLASH_MARKET_INDEX`；未设置 = 只用内置索引，零请求）。
pub fn index_url() -> Option<String> {
    std::env::var("PI_FLASH_MARKET_INDEX")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().trim_end_matches('/').to_string())
}

/// 信封验签：{ format: pf-market-signed/1, signed: <b64(JSON)>, signature: ed25519:<b64> }。
/// 签名对象 = signed 字段解码后的原始字节（免 canonical JSON 折腾：签的就是发的那串）。
pub fn verify_signed_index(raw: &str, pubkey_pem: &str) -> Result<MarketIndex, String> {
    use base64::Engine as _;
    let envelope: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("索引信封解析失败: {e}"))?;
    if envelope["format"].as_str() != Some("pf-market-signed/1") {
        return Err("索引信封格式不符（缺 pf-market-signed/1）".into());
    }
    let signed_b64 = envelope["signed"].as_str().ok_or("索引缺 signed 字段")?;
    let sig_b64 = envelope["signature"]
        .as_str()
        .ok_or("索引缺 signature 字段")?
        .strip_prefix("ed25519:")
        .ok_or("signature 必须以 ed25519: 开头")?;
    let signed = base64::engine::general_purpose::STANDARD
        .decode(signed_b64)
        .map_err(|e| format!("signed 解码失败: {e}"))?;
    let sig = base64::engine::general_purpose::STANDARD
        .decode(sig_b64)
        .map_err(|e| format!("signature 解码失败: {e}"))?;
    let vk = verifying_key_from_pem(pubkey_pem)?;
    use ed25519_dalek::Signature;
    let sig = Signature::from_slice(&sig).map_err(|_| "signature 长度非法".to_string())?;
    vk.verify_strict(&signed, &sig)
        .map_err(|_| "索引验签失败（内容被改或钥不符）".to_string())?;
    MarketIndex::parse(&String::from_utf8(signed).map_err(|_| "signed 不是 UTF-8".to_string())?)
}

/// SPKI PEM → VerifyingKey（ed25519 专属壳 302a300506032b6570032100，手剥免依赖）。
fn verifying_key_from_pem(pem: &str) -> Result<ed25519_dalek::VerifyingKey, String> {
    use base64::Engine as _;
    const ED25519_SPKI_PREFIX: [u8; 12] = [
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("公钥 DER 解码失败: {e}"))?;
    if der.len() != 44 || der[..12] != ED25519_SPKI_PREFIX {
        return Err("公钥不是 ed25519 SPKI PEM".into());
    }
    let raw: [u8; 32] = der[12..]
        .try_into()
        .map_err(|_| "公钥长度非法".to_string())?;
    ed25519_dalek::VerifyingKey::from_bytes(&raw).map_err(|e| format!("公钥非法: {e}"))
}

/// npm 包名 → 磁盘目录名：`@pf-plugins-origin/quick` → `@pf-plugins-origin__quick`。
/// （`/` 是唯一非法字符；scope 属主即身份，不需要再哈希。）
pub fn dir_name(name: &str) -> String {
    name.replace('/', "__")
}

fn plugin_dir(name: &str, version: &str) -> Result<PathBuf, String> {
    let root = crate::paths::plugins_dir().ok_or("无法定位插件安装目录")?;
    Ok(root.join(dir_name(name)).join(version))
}

// ---------------------------------------------------------------------------
// npm pack（与宿主 plugin_updates::npm_invocation 同款调用链）
// ---------------------------------------------------------------------------

/// 组 npm 命令：内置 node 直跑内置 npm-cli.js；找不到时退 PATH 上的 node；
/// 最后才裸 npm（仅 Unix 可用——Windows 裸 npm 是 .cmd，会被 spawn 拒绝）。
fn npm_command(args: &[&str]) -> Result<Command, String> {
    let node = crate::vendor::node_bin();
    let node_path = Path::new(&node);
    let mut cmd = if node_path.is_absolute() {
        let cli = node_path
            .parent()
            .map(|d| d.join("node_modules/npm/bin/npm-cli.js"))
            .filter(|p| p.is_file());
        match cli {
            Some(cli) => {
                let mut c = Command::new(node_path);
                c.arg(cli);
                c
            }
            None => {
                #[cfg(windows)]
                {
                    let _ = args;
                    return Err("未找到内置 npm（node 目录缺 npm-cli.js）".into());
                }
                #[cfg(not(windows))]
                {
                    let mut c = Command::new("npm");
                    c.args(args);
                    return Ok(c);
                }
            }
        }
    } else {
        // 裸 "node"：扫 PATH 找真实路径，才能定位同目录的 npm-cli.js
        let found = find_in_path(if cfg!(windows) { "node.exe" } else { "node" });
        match found {
            Some(p) => {
                let cli = p
                    .parent()
                    .map(|d| d.join("node_modules/npm/bin/npm-cli.js"))
                    .filter(|f| f.is_file());
                match cli {
                    Some(cli) => {
                        let mut c = Command::new(&p);
                        c.arg(cli);
                        c
                    }
                    None => {
                        #[cfg(windows)]
                        {
                            let _ = args;
                            return Err("未找到内置 npm（PATH node 目录缺 npm-cli.js）".into());
                        }
                        #[cfg(not(windows))]
                        {
                            let mut c = Command::new("npm");
                            c.args(args);
                            return Ok(c);
                        }
                    }
                }
            }
            None => {
                #[cfg(windows)]
                {
                    return Err("未找到 node".into());
                }
                #[cfg(not(windows))]
                {
                    let mut c = Command::new("npm");
                    c.args(args);
                    return Ok(c);
                }
            }
        }
    };
    cmd.args(args);
    Ok(cmd)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// `npm pack <name>@<version>` 到 `dest`，返回 tarball 路径。
fn pack(name: &str, version: &str, dest: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let spec = format!("{name}@{version}");
    let out = npm_command(&["pack", "--json", "--pack-destination"])?
        .arg(dest)
        .arg(&spec)
        .current_dir(std::env::temp_dir())
        .output()
        .map_err(|e| format!("npm 启动失败: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "npm pack {spec} 失败: {}",
            err.lines().last().unwrap_or("未知错误")
        ));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("npm pack 输出解析失败: {e}"))?;
    let filename = json[0]["filename"]
        .as_str()
        .ok_or("npm pack 输出缺少 filename")?
        .to_string();
    Ok(dest.join(filename))
}

// ---------------------------------------------------------------------------
// 校验 + 解包 + 安装/卸载
// ---------------------------------------------------------------------------

fn sha512_sri(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读 tarball 失败: {e}"))?;
    let digest = Sha512::digest(&bytes);
    Ok(format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    ))
}

/// 系统 tar 解包（Win10+ 自带 bsdtar；Unix 恒有）。剥掉顶层 `package/`。
/// 市场扫描（market::pf_scan）解清单包复用同一条。
pub fn extract(tarball: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建安装目录失败: {e}"))?;
    let out = Command::new("tar")
        .args([
            "-xzf",
            &tarball.to_string_lossy(),
            "-C",
            &dest.to_string_lossy(),
            "--strip-components=1",
        ])
        .output()
        .map_err(|e| format!("tar 启动失败（系统缺 tar?）: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "解包失败: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
        ));
    }
    Ok(())
}

/// 安装/更新（同一入口：同版本重装 = 覆盖）。返回给 UI 的完成消息。
pub fn install(entry: &IndexEntry) -> Result<String, String> {
    let dest = plugin_dir(&entry.name, &entry.version)?;
    let tmp = crate::paths::plugins_dir()
        .ok_or("无法定位插件安装目录")?
        .join(".tmp-pack");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时目录失败: {e}"))?;

    let result = (|| {
        // 下载档：npm 档按 registry 约定解析（npm pack）；url 档索引自描述路径直连拉取
        // （私有渠道/内层 wasm 包，090-3 §3.1 source 双档）。后续 integrity 对账同一条。
        let tarball = match entry.source.as_str() {
            "url" => {
                if entry.url.is_empty() {
                    return Err("url 档条目缺 url 字段".into());
                }
                let out = tmp.join("pkg.tgz");
                let bytes = crate::market::http_get_bytes(&entry.url, 32 * 1024 * 1024)?;
                std::fs::write(&out, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;
                out
            }
            _ => pack(&entry.name, &entry.version, &tmp)?,
        };
        let actual = sha512_sri(&tarball)?;
        if actual != entry.integrity {
            std::fs::remove_file(&tarball).ok();
            return Err(format!(
                "完整性校验失败：索引 {} ≠ 实际 {actual}",
                entry.integrity
            ));
        }
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| format!("清理旧版本失败: {e}"))?;
        }
        extract(&tarball, &dest)?;
        std::fs::remove_file(&tarball).ok();
        ledger_upsert(LedgerEntry {
            name: entry.name.clone(),
            version: entry.version.clone(),
            integrity: entry.integrity.clone(),
            installed_at: now_iso(),
            source: "market".into(),
            disabled: false,
        })
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    Ok(format!("{} {} 安装完成", entry.display.zh, entry.version))
}

/// 卸载：删目录 + 摘台账。目录已不在视为成功（幂等）。
pub fn uninstall(name: &str) -> Result<String, String> {
    let root = crate::paths::plugins_dir().ok_or("无法定位插件安装目录")?;
    let base = root.join(dir_name(name));
    if base.exists() {
        std::fs::remove_dir_all(&base).map_err(|e| format!("删除插件目录失败: {e}"))?;
    }
    ledger_remove(name)?;
    Ok(format!("{name} 已卸载"))
}

fn now_iso() -> String {
    // 无 chrono 依赖：epoch 秒即可（台账时间戳仅排障用）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{secs}")
}

#[cfg(test)]
mod signed_index_tests {
    use super::*;

    /// 索引信封跨语言（由 inner/scripts/selftest.mjs 注入环境；缺环境跳过）。
    #[test]
    fn market_index_signed_verify() {
        let Ok(pem) = std::env::var("PF_X_IDX_PUB") else {
            return;
        };
        let signed = std::env::var("PF_X_IDX_SIGNED").unwrap();
        let sig = std::env::var("PF_X_IDX_SIG").unwrap();
        let raw =
            format!(r#"{{"format":"pf-market-signed/1","signed":"{signed}","signature":"{sig}"}}"#);
        let ix = verify_signed_index(&raw, &pem).expect("Node 签发的索引必须验过");
        assert!(ix.packages.is_empty());
        // 篡改 signed（换前 4 字符）→ 必拒
        let tampered = signed.replace(&signed[..4], "AAAA");
        let bad = format!(
            r#"{{"format":"pf-market-signed/1","signed":"{tampered}","signature":"{sig}"}}"#
        );
        assert!(verify_signed_index(&bad, &pem).is_err(), "篡改必须拒");
    }
}
