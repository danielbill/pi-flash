//! GitHub release 抓取与下载（081）：release API / 镜像测速切换下载 /
//! SHA-256 校验 / zip 解压消毒 / 进度原子量 / stage_update 落位 /
//! last_seen 与更新日志缓存。自 updater.rs 拆出（090 重构）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use super::*;

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
// 下载 → 校验 → 落位
// ---------------------------------------------------------------------------

pub(crate) fn asset_name(version: &str) -> String {
    format!("pi-flash-{}-win32-x64.zip", version_key_string(version))
}

/// sha256sum 一行解析（`<hash>  file` / `<hash> *file`，与 lib.js 同规则）。
pub(crate) fn parse_hash_for(text: &str, filename: &str) -> Option<String> {
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
pub(crate) fn safe_zip_path(dest: &Path, name: &str) -> Option<PathBuf> {
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

/// 并行解压。条目互不依赖，按原子下标工作窃取；Windows 瓶颈是 Defender
/// 对每个新建文件的实时扫描，多线程摊薄单文件延迟（单线程 4 万小文件
/// 分钟级，并行后 10s 上下）。进度写 EXTRACT_PCT 供版本号位读取。
/// 任一 Worker 出错即置 failed 停止取活，返回首个错误。
pub(crate) fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let f = std::fs::File::open(zip_path).map_err(|e| format!("{e}"))?;
    let total = {
        let probe = zip::ZipArchive::new(f).map_err(|e| format!("{e}"))?;
        probe.len()
    };
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicBool::new(false));
    let first_err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    std::thread::scope(|s| {
        for _ in 0..workers {
            // 每 worker 独立打开：File::try_clone 复制的句柄共享文件指针，
            // 各线程 seek 会互相踩（「failed to fill whole buffer」实录）
            let f = match std::fs::File::open(zip_path) {
                Ok(f) => f,
                Err(e) => {
                    failed.store(true, Ordering::Relaxed);
                    let mut slot = first_err.lock().unwrap();
                    if slot.is_none() {
                        *slot = Some(format!("{e}"));
                    }
                    break;
                }
            };
            let next = next.clone();
            let done = done.clone();
            let failed = failed.clone();
            let first_err = first_err.clone();
            s.spawn(move || {
                let mut archive = match zip::ZipArchive::new(f) {
                    Ok(a) => a,
                    Err(e) => {
                        failed.store(true, Ordering::Relaxed);
                        let mut slot = first_err.lock().unwrap();
                        if slot.is_none() {
                            *slot = Some(format!("{e}"));
                        }
                        return;
                    }
                };
                loop {
                    if failed.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= total {
                        return;
                    }
                    let r = (|| -> Result<(), String> {
                        let mut entry = archive.by_index(i).map_err(|e| format!("{e}"))?;
                        let Some(out) = safe_zip_path(dest, entry.name()) else {
                            return Ok(());
                        };
                        if entry.is_dir() {
                            std::fs::create_dir_all(&out).map_err(|e| format!("{e}"))?;
                            return Ok(());
                        }
                        if let Some(parent) = out.parent() {
                            // 并行下兄弟条目可能同时在建同一父目录，
                            // AlreadyExists 视为成功
                            if let Err(e) = std::fs::create_dir_all(parent) {
                                if e.kind() != std::io::ErrorKind::AlreadyExists {
                                    return Err(format!("{e}"));
                                }
                            }
                        }
                        let mut out_f = std::fs::File::create(&out).map_err(|e| format!("{e}"))?;
                        std::io::copy(&mut entry, &mut out_f)
                            .map(|_| ())
                            .map_err(|e| format!("{e}"))
                    })();
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if total > 0 {
                        EXTRACT_PCT.store(((n * 100) / total).min(100) as u8, Ordering::Relaxed);
                    }
                    if let Err(e) = r {
                        failed.store(true, Ordering::Relaxed);
                        let mut slot = first_err.lock().unwrap();
                        if slot.is_none() {
                            *slot = Some(e);
                        }
                        return;
                    }
                }
            });
        }
    });

    if let Some(e) = first_err.lock().unwrap().take() {
        return Err(e);
    }
    Ok(())
}

/// win32 zip 根带 `pi-flash/` 包装目录（release.sh Compress-Archive 行为）；
/// 找到含 pi-flash.exe 的那一层。
pub(crate) fn locate_content_root(extract_dir: &Path) -> Option<PathBuf> {
    for root in [extract_dir.join("pi-flash"), extract_dir.to_path_buf()] {
        if root.join(exe_rel()).is_file() {
            return Some(root);
        }
    }
    None
}

/// 下载进度（0-100），None = 无进度数据（未在下载 / 响应无 content-length）。
/// 下载线程 store、新会话页版本号位渲染 load——spinner 动画每帧触发重绘，
/// 原子读足够，无需实体通知。
static DOWNLOAD_PCT: AtomicU8 = AtomicU8::new(PCT_UNKNOWN);
const PCT_UNKNOWN: u8 = 255;

/// 当前下载进度（0-100），供版本号位 UI 与 pif-ui snapshot 读取。
pub(crate) fn download_percent() -> Option<u8> {
    match DOWNLOAD_PCT.load(Ordering::Relaxed) {
        PCT_UNKNOWN => None,
        pct => Some(pct),
    }
}

/// 写进度：None = 未知（哨兵 255）。已知值钳到 100。
pub(crate) fn set_download_percent(pct: Option<u8>) {
    let v = match pct {
        Some(p) => p.min(100),
        None => PCT_UNKNOWN,
    };
    DOWNLOAD_PCT.store(v, Ordering::Relaxed);
}

/// 流水线相位：版本号位的进度通道二选一（下载 NN% / 解压 NN%）。
/// 解压此前无通道，100% 后单线程解压 4 万小文件（Defender 逐个实时扫描）
/// 要分钟级，UI 只能干等——0.1.8 用户实录「下载完卡一分钟」。
pub(crate) const PHASE_IDLE: u8 = 0;
pub(crate) const PHASE_DOWNLOAD: u8 = 1;
pub(crate) const PHASE_EXTRACT: u8 = 2;

static UPDATE_PHASE: AtomicU8 = AtomicU8::new(PHASE_IDLE);
static EXTRACT_PCT: AtomicU8 = AtomicU8::new(PCT_UNKNOWN);

/// 当前相位（PHASE_*），供版本号位 UI 与 pif-ui snapshot 读取。
pub(crate) fn update_phase() -> u8 {
    UPDATE_PHASE.load(Ordering::Relaxed)
}

/// 解压进度（0-100），None = 无进度数据（不在解压相位）。
pub(crate) fn extract_percent() -> Option<u8> {
    match EXTRACT_PCT.load(Ordering::Relaxed) {
        PCT_UNKNOWN => None,
        pct => Some(pct),
    }
}

/// 切相位并清两条进度通道（UI 永远吃不到上一相位的陈旧百分比）。
pub(crate) fn set_phase(phase: u8) {
    UPDATE_PHASE.store(phase, Ordering::Relaxed);
    DOWNLOAD_PCT.store(PCT_UNKNOWN, Ordering::Relaxed);
    EXTRACT_PCT.store(PCT_UNKNOWN, Ordering::Relaxed);
}

/// 测速参数（env 可调，与 npm/bin/lib.js 同名变量同默认值；测试注入更小的
/// probe/阈值以秒级完成）。
pub(crate) fn speed_probe() -> Duration {
    Duration::from_millis(
        std::env::var("PI_FLASH_PROBE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000),
    )
}

pub(crate) fn min_speed_bps() -> f64 {
    std::env::var("PI_FLASH_MIN_KBPS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|kbps| kbps * 1024.0)
        .unwrap_or(MIN_SPEED_BPS)
}

/// 下载到文件，边下边算 SHA-256（单遍）。开传满 probe 平均速度仍低于 min_bps
/// 即 Err（测速不过，由上层换源）。返回 hex 摘要。
pub(crate) fn download_and_hash_cfg(
    url: &str,
    dest: &Path,
    probe: Duration,
    min_bps: f64,
) -> Result<String, String> {
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
    let total: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    set_download_percent((total > 0).then_some(0));
    let mut reader = resp.into_reader();
    let mut out = std::fs::File::create(dest).map_err(|e| format!("{e}"))?;
    let mut h = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut got: u64 = 0;
    let mut last_pct: u8 = 0;
    let t0 = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{e}"))?;
        if n == 0 {
            break;
        }
        got += n as u64;
        let elapsed = t0.elapsed();
        if elapsed >= probe {
            let speed = got as f64 / elapsed.as_secs_f64();
            if speed < min_bps {
                return Err(format!(
                    "测速不过（{}KB/s < {}KB/s）",
                    (speed / 1024.0) as u64,
                    (min_bps / 1024.0) as u64
                ));
            }
        }
        if total > 0 {
            let pct = ((got as f64 / total as f64) * 100.0).min(100.0) as u8;
            if pct != last_pct {
                set_download_percent(Some(pct));
                last_pct = pct;
            }
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("{e}"))?;
    }
    Ok(format!("{:x}", h.finalize()))
}

/// 按候选源逐个试下载（每源一次，测速不过/网络错误都换下一个），全灭才报错。
/// 返回（zip 的 SHA-256，命中源 base——校验和钉官方优先时兜底用）。
pub(crate) fn download_mirrored(asset: &str, dest: &Path, ver: &str) -> Result<(String, String), String> {
    download_mirrored_cfg(
        asset,
        dest,
        ver,
        &download_bases(),
        speed_probe(),
        min_speed_bps(),
    )
}

pub(crate) fn download_mirrored_cfg(
    asset: &str,
    dest: &Path,
    ver: &str,
    bases: &[String],
    probe: Duration,
    min_bps: f64,
) -> Result<(String, String), String> {
    let mut errors = Vec::new();
    for base in bases {
        eprintln!("[updater] 下载源：{base}");
        match download_and_hash_cfg(&format!("{base}/v{ver}/{asset}"), dest, probe, min_bps) {
            Ok(h) => return Ok((h, base.clone())),
            Err(e) => {
                eprintln!("[updater] 源不可用（{e}），换下一个");
                errors.push(format!("{base}：{e}"));
            }
        }
    }
    Err(format!(
        "所有下载源均失败：{}；可设 PI_FLASH_MIRROR=<base> 指定源，或到 GitHub release 页手动下载",
        errors.join("；")
    ))
}

pub(crate) fn fetch_text(url: &str) -> Result<String, String> {
    let resp = check_agent()
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    resp.into_string().map_err(|e| format!("{e}"))
}

/// 下载 + SHA-256 校验（sidecar → SHA256SUMS 兜底，080 §3.2 同序）+ 解压到
/// 新的载荷目录 `<root>/<基名>-<版本>/`（版本戳最后写）。成功返回版本号。
/// 落位目标就是「更新后要运行的目录」——没有 rename、没有标记文件。
pub(crate) fn stage_update(rel: &ReleaseInfo) -> Result<String, String> {
    let me = my_exe_dir().ok_or("无法定位当前载荷目录")?;
    stage_update_in(&me, rel)
}

/// 显式传入**当前载荷目录**的版本（测试注入用：测试进程的 current_exe 指向
/// target/，不隔离）。下载源仍走 download_bases()（测试用 PI_FLASH_MIRROR
/// 定向本地假源）。
pub(crate) fn stage_update_in(me: &Path, rel: &ReleaseInfo) -> Result<String, String> {
    let root = me.parent().ok_or("无法定位安装根目录")?.to_path_buf();
    let ver = version_key_string(&rel.version);
    let stem = base_name(&me.file_name().unwrap_or_default().to_string_lossy());
    let target = root.join(format!("{stem}-{ver}"));
    if target == me {
        return Err(format!("目标目录就是当前运行目录（v{ver}），拒绝覆盖"));
    }
    let tmp = root.join(format!("update.tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("x")).map_err(|e| format!("{e}"))?;

    let result = (|| -> Result<(), String> {
        let asset = asset_name(&ver);
        let zip_path = tmp.join(&asset);

        set_phase(PHASE_DOWNLOAD);
        // 下载（测速自动换源：直连慢/挂就按序切镜像），边下边算 SHA-256
        let (zip_hash, chosen) = download_mirrored(&asset, &zip_path, &ver)?;

        // 校验和：直连优先钉定官方哈希（zip 走镜像时防串包）→ 命中源兜底；
        // sidecar 404 → SHA256SUMS 兜底
        let expected = {
            let mut found = None;
            let mut bases = vec![RELEASES_DOWNLOAD_BASE.to_string()];
            if chosen != RELEASES_DOWNLOAD_BASE {
                bases.push(chosen);
            }
            'outer: for b in &bases {
                for suffix in [format!("{asset}.sha256"), "SHA256SUMS".to_string()] {
                    match fetch_text(&format!("{b}/v{ver}/{suffix}")) {
                        Ok(text) => {
                            if let Some(h) = parse_hash_for(&text, &asset) {
                                found = Some(h);
                                break 'outer;
                            }
                        }
                        Err(_) => continue, // 404 / 网络抖动 → 试下一个来源
                    }
                }
            }
            found.ok_or_else(|| format!("release 资产里找不到 {asset} 的 SHA-256，放弃安装"))?
        };
        if zip_hash != expected {
            return Err(format!("SHA-256 不符（期望 {expected}，实际 {zip_hash}）"));
        }

        set_phase(PHASE_EXTRACT);
        extract_zip(&zip_path, &tmp.join("x"))?;
        let content =
            locate_content_root(&tmp.join("x")).ok_or("载荷结构异常：找不到 pi-flash.exe")?;

        // 上一轮半截的同版本目录先清（解压中断/被清），再 rename 成最终目录
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&content, &target).map_err(|e| format!("载荷落位失败：{e}"))?;

        // 安装阶段把成本花完：每个文件读一遍，然后才写版本戳。有戳 = 装好且
        // 成本已付（点重启、以后每次打开都是秒开）；没戳 = 没装（交接不用它，
        // 下次重装）。
        touch_tree(&target);
        std::fs::write(target.join(".payload-version"), format!("{ver}\n"))
            .map_err(|e| format!("写版本戳失败：{e}"))?;

        // 更新日志缓存（重启后的更新日志页直接读本地）
        if !rel.body.is_empty() {
            if let Some(p) = pi_link::paths::changelog_cache_file(&ver) {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&p, &rel.body);
            }
        }
        Ok(())
    })();

    set_phase(PHASE_IDLE);
    let _ = std::fs::remove_dir_all(&tmp);
    result.map(|_| ver)
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

