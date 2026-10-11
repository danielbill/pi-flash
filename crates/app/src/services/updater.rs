//! 软件自动更新（081-软件自动更新.md）：揭幕后延迟加载阶段后台检查 GitHub
//! release → on=后台下载→SHA-256 校验→解压 staging（薄壳启动器下次启动
//! swap 落位）→inputpanel 右下版本号位提示；off=版本号「x.x.x↑」可点击开
//! release 页。新版本首启自动开 topbar「更新日志」tab。
//!
//! 落位协议（与 npm 薄壳 080 §3 协作）：
//! ```text
//! <pkg>/payload/                    当前载荷（exe 正在运行，不可整删）
//! <pkg>/payload.stage-<ver>/        下载解压好的新载荷（含 .payload-version 戳）
//! <pkg>/update-staged.json          staging 标记 {"version","staged_at"}——原子写
//! ```
//! swap 只由启动器做（彼时旧进程已退出、目录无锁）：rename payload→old、
//! rename staging→payload、删标记、删 old。exe 从不自己动 payload 目录。
//!
//! 网络纪律（设计文档定）：检测 timeout 30s，失败重试 2 次、间隔 10 分钟，
//! 全程后台线程，绝不阻塞主进程；失败静默回落 Idle，下次启动再来。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 下载/检查仓库坐标（与 npm/bin/lib.js releaseBase 同源）。
const RELEASES_DOWNLOAD_BASE: &str = "https://github.com/danielbill/pi-flash/releases/download";
const RELEASES_API_BASE: &str = "https://api.github.com/repos/danielbill/pi-flash/releases";
const UA: &str = "pi-flash-updater";
/// 检测超时（设计文档：30s）。
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// 检测失败重试间隔（设计文档：10 分钟 ×2 次）。
const RETRY_INTERVAL: Duration = Duration::from_secs(600);
/// 下载整体预算（zip ≈120MB；连接超时 30s 不封总量）。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

// ---------------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------------

/// 版本号位（新会话页 action_bar）的状态机。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UpdateState {
    /// 无动作：检查中 / 无新版 / 检查失败 / 已是最新。
    Idle,
    /// 后台下载中：转圈 + 「下载 x.x.x 版本中…」（faint）。
    Downloading { version: String },
    /// 新载荷已就位：待重启（「请重启软件切换至 x.x.x 版本」，青蓝）。
    Ready { version: String },
    /// 自动更新 off：有新版（「x.x.x↑」青蓝可点击，开 release 页）。
    Available { version: String },
}

/// 更新日志 tab 的数据（body = markdown；None = 还没拿到）。
#[derive(Debug, Clone)]
pub(crate) struct ChangelogPage {
    pub(crate) version: String,
    pub(crate) body: Option<String>,
    pub(crate) fetching: bool,
}

// ---------------------------------------------------------------------------
// 版本号比较（x.y.z 逐段数值；缺段/非数按 0）
// ---------------------------------------------------------------------------

fn version_key(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// a 是否严格大于 b。
pub(crate) fn version_gt(a: &str, b: &str) -> bool {
    let (ka, kb) = (version_key(a), version_key(b));
    let n = ka.len().max(kb.len());
    for i in 0..n {
        let (x, y) = (ka.get(i).copied().unwrap_or(0), kb.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ---------------------------------------------------------------------------
// 落位协议：pkg 目录 / staging / 标记
// ---------------------------------------------------------------------------

/// npm 包目录（exe 在 `<pkg>/payload/pi-flash.exe`）。开发机 cargo run 时
/// 指向 target/debug 的祖父目录——更新流程在 debug 构建默认关闭，不会写。
fn pkg_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.to_path_buf())
}

fn staging_dir(pkg: &Path, version: &str) -> PathBuf {
    pkg.join(format!("payload.stage-{}", version_key_string(version)))
}

fn marker_path(pkg: &Path) -> PathBuf {
    pkg.join("update-staged.json")
}

fn version_key_string(v: &str) -> String {
    v.trim().trim_start_matches('v').to_string()
}

#[derive(Debug, Clone)]
pub(crate) struct StagedUpdate {
    pub version: String,
    pub staged_at: u64,
}

/// 读 staging 标记（结构合法且 staging 目录里有 exe + 吻合的版本戳才算数；
/// 残缺的当垃圾清掉）。
pub(crate) fn read_staged_in(pkg: &Path) -> Option<StagedUpdate> {
    let raw = std::fs::read_to_string(marker_path(pkg)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let version = v
        .get("version")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let staged_at = v.get("staged_at").and_then(|x| x.as_u64()).unwrap_or(0);
    let dir = staging_dir(pkg, &version);
    let exe_ok = dir.join("pi-flash.exe").is_file();
    let stamp_ok = std::fs::read_to_string(dir.join(".payload-version"))
        .map(|s| s.trim() == version)
        .unwrap_or(false);
    if exe_ok && stamp_ok {
        return Some(StagedUpdate { version, staged_at });
    }
    // 半截 staging（解压中断/被清）：标记一并作废
    let _ = std::fs::remove_file(marker_path(pkg));
    let _ = std::fs::remove_dir_all(&dir);
    None
}

/// 原子写标记（tmp + rename）。
fn write_staged_in(pkg: &Path, staged: &StagedUpdate) -> std::io::Result<()> {
    let path = marker_path(pkg);
    let tmp = path.with_extension("json.tmp");
    let value = serde_json::json!({
        "version": staged.version,
        "staged_at": staged.staged_at,
    });
    std::fs::write(&tmp, serde_json::to_string(&value).unwrap_or_default())?;
    std::fs::rename(&tmp, &path)
}

/// 启动时初始状态：已有合法 staging = Ready（不花网络）；否则 Idle。
pub(crate) fn initial_state() -> UpdateState {
    let Some(pkg) = pkg_dir() else {
        return UpdateState::Idle;
    };
    match read_staged_in(&pkg) {
        Some(s) => UpdateState::Ready {
            version: s.version,
        },
        None => UpdateState::Idle,
    }
}

// ---------------------------------------------------------------------------
// release 拉取
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct ReleaseInfo {
    pub version: String,
    pub body: String,
}

fn api_base() -> String {
    std::env::var("PI_FLASH_RELEASES_API")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RELEASES_API_BASE.to_string())
}

fn download_base() -> String {
    std::env::var("PI_FLASH_MIRROR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|m| m.trim_end_matches('/').to_string())
        .unwrap_or_else(|| RELEASES_DOWNLOAD_BASE.to_string())
}

fn check_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(CHECK_TIMEOUT).build()
}

/// latest release（含 body = 更新日志 markdown）。tag 形如 v1.2.3。
fn fetch_release(url: &str) -> Result<ReleaseInfo, String> {
    let resp = check_agent()
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

fn fetch_latest() -> Result<ReleaseInfo, String> {
    fetch_release(&format!("{}/latest", api_base()))
}

/// 按 tag 拉更新日志（更新日志页缓存缺失时的现拉通道）。
fn fetch_release_body(version: &str) -> Result<String, String> {
    let url = format!("{}/tags/v{}", api_base(), version_key_string(version));
    fetch_release(&url).map(|r| r.body)
}

// ---------------------------------------------------------------------------
// 下载 → 校验 → 解压 staging
// ---------------------------------------------------------------------------

fn asset_name(version: &str) -> String {
    format!("pi-flash-{}-win32-x64.zip", version_key_string(version))
}

/// sha256sum 一行解析（`<hash>  file` / `<hash> *file`，与 lib.js 同规则）。
fn parse_hash_for(text: &str, filename: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let hash = hash.trim().to_ascii_lowercase();
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let name = name.trim().trim_start_matches('*').trim();
        let name = name.strip_prefix("./").unwrap_or(name);
        if name == filename {
            return Some(hash);
        }
    }
    None
}

/// zip 条目名安全化：拒绝绝对路径 / `..` / 盘符；分隔符归一。返回 None = 跳过。
fn safe_zip_path(dest: &Path, name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains(':') {
        return None;
    }
    let mut out = dest.to_path_buf();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let f = std::fs::File::open(zip_path).map_err(|e| format!("{e}"))?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| format!("{e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("{e}"))?;
        let Some(out) = safe_zip_path(dest, entry.name()) else {
            continue;
        };
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("{e}"))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{e}"))?;
        }
        let mut out_f = std::fs::File::create(&out).map_err(|e| format!("{e}"))?;
        std::io::copy(&mut entry, &mut out_f).map_err(|e| format!("{e}"))?;
    }
    Ok(())
}

/// win32 zip 根带 `pi-flash/` 包装目录（release.sh Compress-Archive 行为）；
/// 找到含 pi-flash.exe 的那一层。
fn locate_content_root(extract_dir: &Path) -> Option<PathBuf> {
    for root in [extract_dir.join("pi-flash"), extract_dir.to_path_buf()] {
        if root.join("pi-flash.exe").is_file() {
            return Some(root);
        }
    }
    None
}

/// 下载到文件，边下边算 SHA-256（单遍）。返回 hex 摘要。
fn download_and_hash(url: &str, dest: &Path) -> Result<String, String> {
    use sha2::Digest;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CHECK_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build();
    let resp = agent
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    let mut reader = resp.into_reader();
    let mut out = std::fs::File::create(dest).map_err(|e| format!("{e}"))?;
    let mut h = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{e}"))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("{e}"))?;
    }
    Ok(format!("{:x}", h.finalize()))
}

fn fetch_text(url: &str) -> Result<String, String> {
    let resp = check_agent()
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    resp.into_string().map_err(|e| format!("{e}"))
}

/// 下载 + SHA-256 校验（sidecar → SHA256SUMS 兜底，080 §3.2 同序）+ 解压 +
/// rename 成 staging + 写标记。成功返回版本号。
fn stage_update(rel: &ReleaseInfo) -> Result<String, String> {
    let pkg = pkg_dir().ok_or("无法定位安装目录")?;
    std::fs::create_dir_all(&pkg).map_err(|e| format!("{e}"))?;
    let ver = rel.version.clone();
    let tmp = pkg.join(format!("update.tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("x")).map_err(|e| format!("{e}"))?;

    let result = (|| -> Result<(), String> {
        let asset = asset_name(&ver);
        let base = format!("{}/v{}", download_base(), ver);
        let zip_path = tmp.join(&asset);

        // 下载（3 次尝试 + 线性退避），边下边算 SHA-256
        let mut zip_hash: Option<String> = None;
        let mut last_err = String::new();
        for attempt in 1..=3 {
            match download_and_hash(&format!("{base}/{asset}"), &zip_path) {
                Ok(h) => {
                    zip_hash = Some(h);
                    break;
                }
                Err(e) => {
                    last_err = e;
                    if attempt < 3 {
                        std::thread::sleep(Duration::from_secs(attempt * 2));
                    }
                }
            }
        }
        let zip_hash = zip_hash.ok_or_else(|| format!("下载失败：{last_err}"))?;

        // 校验和：sidecar 404 → SHA256SUMS 兜底
        let expected = {
            let mut found = None;
            for url in [format!("{base}/{asset}.sha256"), format!("{base}/SHA256SUMS")] {
                match fetch_text(&url) {
                    Ok(text) => {
                        if let Some(h) = parse_hash_for(&text, &asset) {
                            found = Some(h);
                            break;
                        }
                    }
                    Err(_) => continue, // 404 / 网络抖动 → 试下一个来源
                }
            }
            found.ok_or_else(|| format!("release 资产里找不到 {asset} 的 SHA-256，放弃安装"))?
        };
        if zip_hash != expected {
            return Err(format!("SHA-256 不符（期望 {expected}，实际 {zip_hash}）"));
        }

        extract_zip(&zip_path, &tmp.join("x"))?;
        let root = locate_content_root(&tmp.join("x"))
            .ok_or("载荷结构异常：找不到 pi-flash.exe")?;

        // rename 成 staging（同卷原子）；旧 staging 先清
        let staging = staging_dir(&pkg, &ver);
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::rename(&root, &staging).map_err(|e| format!("staging 落位失败：{e}"))?;
        std::fs::write(staging.join(".payload-version"), format!("{ver}\n"))
            .map_err(|e| format!("{e}"))?;

        // 更新日志缓存（重启后的更新日志页直接读本地）
        if !rel.body.is_empty() {
            if let Some(p) = pi_link::paths::changelog_cache_file(&ver) {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&p, &rel.body);
            }
        }

        write_staged_in(
            &pkg,
            &StagedUpdate {
                version: ver.clone(),
                staged_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        )
        .map_err(|e| format!("写更新标记失败：{e}"))?;
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result.map(|_| ver)
}

// ---------------------------------------------------------------------------
// last_seen（新版本首启判定）
// ---------------------------------------------------------------------------

fn read_last_seen() -> Option<String> {
    let raw = std::fs::read_to_string(pi_link::paths::update_state_file()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("last_seen_version")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

fn write_last_seen(version: &str) {
    let Some(p) = pi_link::paths::update_state_file() else {
        return;
    };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = pi_link::config::write_json(
        &p,
        &serde_json::json!({ "last_seen_version": version }),
    );
}

fn read_changelog_cache(version: &str) -> Option<String> {
    std::fs::read_to_string(pi_link::paths::changelog_cache_file(version)?)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Chat 接线
// ---------------------------------------------------------------------------

impl crate::Chat {
    /// 调试/开发不跑更新（PI_FLASH_UPDATE_CHECK=1 可强制，0 可关）。
    fn checks_enabled() -> bool {
        match std::env::var("PI_FLASH_UPDATE_CHECK").as_deref() {
            Ok("1") => true,
            Ok("0") => false,
            _ => !cfg!(debug_assertions),
        }
    }

    /// 揭幕后的两个延迟加载钩子（010 §延迟加载「版本更新检查」）：
    /// ① 新版本首启 → 自动开「更新日志」tab；② 后台检查更新。
    pub(crate) fn spawn_startup_update_hooks(&mut self, cx: &mut gpui::Context<Self>) {
        let cur = current_version().to_string();
        match read_last_seen() {
            Some(seen) if version_gt(&cur, &seen) => {
                let cached = read_changelog_cache(&cur);
                self.changelog = Some(ChangelogPage {
                    version: cur.clone(),
                    fetching: cached.is_none(),
                    body: cached,
                });
                self.open_changelog_tab(cx);
                write_last_seen(&cur);
                // 缓存缺失：后台按 tag 现拉一份
                if self.changelog.as_ref().is_some_and(|p| p.fetching) {
                    self.spawn_changelog_fetch(cur, cx);
                }
            }
            None => write_last_seen(&cur), // 首次运行：只记账，不开页
            _ => {}
        }
        self.spawn_update_check(cx);
    }

    fn spawn_changelog_fetch(&mut self, version: String, cx: &mut gpui::Context<Self>) {
        cx.spawn(async move |this, cx| {
            let body = cx
                .background_executor()
                .spawn(async move { fetch_release_body(&version) })
                .await;
            let _ = this.update(cx, |c, cx| {
                if let Some(p) = &mut c.changelog {
                    match body {
                        Ok(b) if !b.trim().is_empty() => p.body = Some(b),
                        _ => {}
                    }
                    p.fetching = false;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 打开（或聚焦）topbar「更新日志」tab。
    pub(crate) fn open_changelog_tab(&mut self, cx: &mut gpui::Context<Self>) {
        match self
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::Changelog))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.panel_tabs.push(crate::PanelTab::Changelog);
                let ix = self.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(crate::ContentView::Changelog);
        cx.notify();
    }

    /// 后台检查更新（on=下载 staging，off=只提示 ↑）。检测失败重试 2 次、
    /// 间隔 10 分钟（设计文档定），全程不阻塞主进程，最终失败静默。
    pub(crate) fn spawn_update_check(&mut self, cx: &mut gpui::Context<Self>) {
        if !Self::checks_enabled() {
            return;
        }
        // 已有 staging：不花网络，等重启
        if matches!(self.update, UpdateState::Ready { .. }) {
            return;
        }
        cx.spawn(async move |this, cx| {
            // ① 检测（3 次尝试，间隔 10 分钟）
            let mut latest: Option<ReleaseInfo> = None;
            for attempt in 0..3 {
                let r = cx
                    .background_executor()
                    .spawn(async { fetch_latest() })
                    .await;
                match r {
                    Ok(rel) => {
                        latest = Some(rel);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[updater] 检查更新失败（{e}）");
                        if attempt < 2 {
                            cx.background_executor().timer(RETRY_INTERVAL).await;
                        }
                    }
                }
            }
            let Some(rel) = latest else { return };
            if !version_gt(&rel.version, current_version()) {
                return; // 已是最新
            }
            // ② 自动更新开关（读最新设置：用户可能中途改）
            let auto = this
                .update(cx, |_, _| crate::services::workspace::auto_update())
                .unwrap_or(true);
            if !auto {
                let _ = this.update(cx, |c, cx| {
                    c.update = UpdateState::Available {
                        version: rel.version.clone(),
                    };
                    cx.notify();
                });
                return;
            }
            // ③ 后台下载 → staging
            let _ = this.update(cx, |c, cx| {
                c.update = UpdateState::Downloading {
                    version: rel.version.clone(),
                };
                cx.notify();
            });
            let outcome = cx
                .background_executor()
                .spawn(async move { stage_update(&rel) })
                .await;
            let _ = this.update(cx, |c, cx| {
                c.update = match &outcome {
                    Ok(v) => UpdateState::Ready {
                        version: v.clone(),
                    },
                    Err(e) => {
                        eprintln!("[updater] 下载更新失败：{e}");
                        UpdateState::Idle
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }
}

/// 打开该版本的 GitHub release 页（off 模式点击「x.x.x↑」）。
pub(crate) fn open_release_page(version: &str) {
    let url = format!(
        "https://github.com/danielbill/pi-flash/releases/tag/v{}",
        version_key_string(version)
    );
    open_url(&url);
}

fn open_url(url: &str) {
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
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = url;
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_gt("1.2.3", "1.2.2"));
        assert!(version_gt("v1.2.3", "1.2.2"));
        assert!(version_gt("1.10.0", "1.9.9"), "逐段数值比较，不是字典序");
        assert!(version_gt("0.2.0", "0.1.99"));
        assert!(!version_gt("1.2.3", "1.2.3"));
        assert!(!version_gt("1.2", "1.2.0"), "缺段按 0");
        assert!(!version_gt("0.1.0", "0.2.0"));
        assert!(!version_gt("abc", "0.0.1"), "非数段按 0");
    }

    #[test]
    fn hash_line_parse() {
        let text = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  pi-flash-0.1.2-win32-x64.zip\n\
                    011f68a3a5e3c1fda41e7f68a9a4f1654eeab5d5f4d19c8e13a7d411d1a25b35 *other.zip\n";
        assert_eq!(
            parse_hash_for(text, "pi-flash-0.1.2-win32-x64.zip").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(parse_hash_for(text, "missing.zip").is_none());
        assert!(parse_hash_for("garbage line\n", "x.zip").is_none());
    }

    #[test]
    fn zip_path_sanitize() {
        let dest = Path::new("D:\\pkg");
        assert!(safe_zip_path(dest, "pi-flash/vendor/pi/x.js").is_some());
        assert!(safe_zip_path(dest, "pi-flash\\vendor\\x.js").is_some());
        assert!(safe_zip_path(dest, "/abs/path").is_none(), "绝对路径拒绝");
        assert!(safe_zip_path(dest, "a/../../etc").is_none(), "上跳拒绝");
        assert!(safe_zip_path(dest, "C:\\evil").is_none(), "盘符拒绝");
        assert!(safe_zip_path(dest, "a/./b").is_some(), "点段折叠");
    }

    #[test]
    fn staged_marker_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "pf-updater-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let pkg = dir.join("pkg");
        std::fs::create_dir_all(staging_dir(&pkg, "0.1.2")).unwrap();
        std::fs::write(staging_dir(&pkg, "0.1.2").join("pi-flash.exe"), b"x").unwrap();
        std::fs::write(staging_dir(&pkg, "0.1.2").join(".payload-version"), "0.1.2\n").unwrap();

        // 半截 staging：无标记 → None
        assert!(read_staged_in(&pkg).is_none());

        write_staged_in(
            &pkg,
            &StagedUpdate {
                version: "0.1.2".into(),
                staged_at: 42,
            },
        )
        .unwrap();
        let staged = read_staged_in(&pkg).unwrap();
        assert_eq!(staged.version, "0.1.2");

        // staging 目录被清 → 标记作废并清理
        std::fs::remove_dir_all(staging_dir(&pkg, "0.1.2")).unwrap();
        assert!(read_staged_in(&pkg).is_none());
        assert!(!marker_path(&pkg).exists(), "残缺标记一并清除");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
